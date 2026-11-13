use super::*;
use serde_json::{json, Value};
use tokio_tungstenite::accept_hdr_async_with_config;

#[derive(Clone, Copy, Debug)]
enum ToolContextOutcome {
    Recover,
    MixedKnownCalls,
    RejectRetry,
    SubstantiveOutput,
    UnknownCall,
}

async fn next_context_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
                Some(Ok(Message::Ping(payload))) => {
                    socket.send(Message::Pong(payload)).await.unwrap();
                }
                other => panic!("expected tool-context websocket text, got {other:?}"),
            }
        }
    })
    .await
    .expect("tool-context websocket text timeout")
}

async fn send_context_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>, value: Value)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .expect("send tool-context fixture");
}

async fn finish_context_mock<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Close(_) => {
                let _ = socket.flush().await;
                return;
            }
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await.unwrap(),
            Message::Text(text) => panic!("unexpected extra tool-context request: {text}"),
            _ => {}
        }
    }
}

async fn receive_context_terminal<S>(
    socket: &mut tokio_tungstenite::WebSocketStream<S>,
) -> Vec<Value>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut events = Vec::new();
    loop {
        let event = next_context_json(socket).await;
        let terminal = matches!(
            event["type"].as_str(),
            Some("response.completed" | "response.failed" | "error")
        );
        events.push(event);
        if terminal {
            return events;
        }
    }
}

async fn tool_context_case(tool_search: bool, outcome: ToolContextOutcome) {
    let call = if tool_search {
        json!({
            "type": "tool_search_call", "id": "ts_context", "call_id": "call_context",
            "execution": "client", "status": "completed", "arguments": {"paths": ["lookup"]}
        })
    } else {
        json!({
            "type": "function_call", "id": "fc_context", "call_id": "call_context",
            "name": "lookup", "arguments": "{\"query\":\"keep this argument\"}", "status": "completed"
        })
    };
    let output_call_id = if matches!(outcome, ToolContextOutcome::UnknownCall) {
        "call_unknown"
    } else {
        "call_context"
    };
    let output = if tool_search {
        json!({
            "type": "tool_search_output", "call_id": output_call_id, "execution": "client",
            "tools": [{"type": "function", "name": "lookup", "description": "keep the real tool schema", "parameters": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}, "strict": true}]
        })
    } else {
        json!({"type": "function_call_output", "call_id": output_call_id, "output": "complete real tool result"})
    };
    let mut follow_up = json!({
        "type": "response.create", "model": "gpt-4.1", "store": false,
        "previous_response_id": "resp_tool_context_seed",
        "client_metadata": {"trace_label": "keep_metadata"},
        "input": [
            {"role": "user", "content": [{"type": "input_text", "text": "Continue using the original instruction and this complete tool result."}]},
            output
        ]
    });
    let mut seed_calls = vec![call.clone()];
    let extra_pair = matches!(outcome, ToolContextOutcome::MixedKnownCalls).then(|| {
        assert!(!tool_search, "the first rejected output must be a function result");
        let search_call = json!({
            "type": "tool_search_call", "id": "ts_context_search", "call_id": "call_context_search",
            "execution": "client", "status": "completed", "arguments": {"paths": ["second_lookup"]}
        });
        let search_output = json!({
            "type": "tool_search_output", "call_id": "call_context_search", "execution": "client",
            "tools": [{"type": "function", "name": "second_lookup", "description": "preserve the second tool schema", "parameters": {"type": "object", "properties": {}, "required": []}, "strict": true}]
        });
        seed_calls.push(search_call.clone());
        follow_up["input"].as_array_mut().unwrap().push(search_output.clone());
        (search_call, search_output)
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap().to_string();
    let expected_call = call.clone();
    let expected_follow_up = follow_up.clone();
    let expected_output = output.clone();
    let upstream = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut seed = accept_hdr_async_with_config(
            stream,
            |_: &Request, response: Response| Ok(response),
            Some(test_upstream_ws_config()),
        )
        .await
        .unwrap();
        let seed_request = next_context_json(&mut seed).await;
        assert_eq!(
            seed_request["input"],
            "Original instruction: preserve the full conversation."
        );
        send_context_json(
            &mut seed,
            json!({"type": "response.created", "response": {"id": "resp_tool_context_seed"}}),
        )
        .await;
        for (output_index, item) in seed_calls.iter().enumerate() {
            send_context_json(
                &mut seed,
                json!({"type": "response.output_item.done", "response_id": "resp_tool_context_seed", "output_index": output_index, "item": item}),
            )
            .await;
        }
        send_context_json(
            &mut seed,
            json!({"type": "response.completed", "response": {"id": "resp_tool_context_seed", "status": "completed", "output": seed_calls}}),
        )
        .await;
        finish_context_mock(&mut seed).await;

        // The next frontend task opens a fresh downstream/upstream socket, but
        // its valid previous_response_id still refers to the original history.
        let (stream, _) = listener.accept().await.unwrap();
        let mut resumed = accept_hdr_async_with_config(
            stream,
            |_: &Request, response: Response| Ok(response),
            Some(test_upstream_ws_config()),
        )
        .await
        .unwrap();
        let initial = next_context_json(&mut resumed).await;
        assert_eq!(initial["previous_response_id"], "resp_tool_context_seed");
        assert_eq!(initial["input"], expected_follow_up["input"]);
        send_context_json(
            &mut resumed,
            json!({"type": "response.created", "response": {"id": "resp_tool_context_attempt"}}),
        )
        .await;
        if matches!(outcome, ToolContextOutcome::SubstantiveOutput) {
            send_context_json(
                &mut resumed,
                json!({"type": "response.output_text.delta", "response_id": "resp_tool_context_attempt", "output_index": 0, "content_index": 0, "delta": "already delivered"}),
            )
            .await;
        }
        let tool_label = if tool_search {
            "tool search"
        } else {
            "function call"
        };
        let error = json!({
            "type": "error", "status": 400,
            "error": {"type": "invalid_request_error", "message": format!("No tool call found for {tool_label} output with call_id {output_call_id}.")}
        });
        send_context_json(&mut resumed, error.clone()).await;
        if matches!(
            outcome,
            ToolContextOutcome::Recover
                | ToolContextOutcome::MixedKnownCalls
                | ToolContextOutcome::RejectRetry
        ) {
            let retry = next_context_json(&mut resumed).await;
            assert_eq!(
                retry["previous_response_id"], "resp_tool_context_seed",
                "tool repair must preserve the only reference to original history"
            );
            assert_eq!(retry["client_metadata"]["trace_label"], "keep_metadata");
            assert_eq!(
                retry["input"].as_array().unwrap().len(),
                if extra_pair.is_some() { 4 } else { 3 }
            );
            assert_eq!(retry["input"][0], expected_follow_up["input"][0]);
            assert_eq!(
                retry["input"][1], expected_call,
                "repair must use the observed call verbatim"
            );
            assert_eq!(
                retry["input"][2], expected_output,
                "repair must preserve the complete real result"
            );
            if let Some((search_call, search_output)) = extra_pair {
                // A valid previous response may already contain other calls.
                // Only the call explicitly rejected by upstream is restored.
                assert_eq!(retry["input"][3], search_output, "the first repair must leave the other result intact without injecting an unreported call");
                send_context_json(
                    &mut resumed,
                    json!({
                        "type": "error", "status": 400,
                        "error": {"type": "invalid_request_error", "message": "No tool call found for tool search output with call_id call_context_search."}
                    }),
                ).await;
                let second_retry = next_context_json(&mut resumed).await;
                assert_eq!(
                    second_retry["previous_response_id"],
                    "resp_tool_context_seed"
                );
                assert_eq!(
                    second_retry["client_metadata"]["trace_label"],
                    "keep_metadata"
                );
                assert_eq!(second_retry["input"].as_array().unwrap().len(), 5);
                assert_eq!(second_retry["input"][0], expected_follow_up["input"][0]);
                assert_eq!(
                    second_retry["input"][1], expected_call,
                    "a distinct-call retry must retain the first recovered call"
                );
                assert_eq!(second_retry["input"][2], expected_output);
                assert_eq!(
                    second_retry["input"][3], search_call,
                    "the second repair must restore the newly reported call"
                );
                assert_eq!(
                    second_retry["input"][4], search_output,
                    "the second complete result must survive repair"
                );
            }
            if matches!(outcome, ToolContextOutcome::RejectRetry) {
                send_context_json(&mut resumed, error).await;
            } else {
                send_context_json(
                    &mut resumed,
                    json!({"type": "response.created", "response": {"id": "resp_tool_context_recovered"}}),
                )
                .await;
                send_context_json(
                    &mut resumed,
                    json!({"type": "response.completed", "response": {"id": "resp_tool_context_recovered", "status": "completed", "output": []}}),
                )
                .await;
            }
        }
        // Keeping the mock alive until client close detects extra retries,
        // fabricated recovery for unknown calls, and replay after real output.
        finish_context_mock(&mut resumed).await;
    });

    let db_path = new_test_db_path(&format!("ws-tool-context-{tool_search}-{outcome:?}"));
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    insert_api_key_record(
        &storage,
        "platform_key_ws_tool_context",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(crate::gateway::reload_runtime_config_from_env)
        .await
        .unwrap();
    let (addr, shutdown, server) = start_front_proxy_test_server(ProxyState {
        backend_base_url: "http://127.0.0.1:1".into(),
        client: Client::new(),
    })
    .await;
    let session_id = format!("session_tool_context_{tool_search}_{outcome:?}");
    let headers = [
        ("OpenAI-Beta", "responses_websockets=2026-02-06"),
        ("session_id", session_id.as_str()),
    ];
    let url = format!("ws://{addr}/v1/responses");
    let (mut seed, _) = connect_async(build_ws_request(
        &url,
        "platform_key_ws_tool_context",
        &headers,
    ))
    .await
    .unwrap();
    send_context_json(
        &mut seed,
        json!({"type": "response.create", "model": "gpt-4.1", "store": false, "input": "Original instruction: preserve the full conversation."}),
    )
    .await;
    let seed_events = receive_context_terminal(&mut seed).await;
    assert_eq!(seed_events.last().unwrap()["type"], "response.completed");
    seed.close(None).await.unwrap();

    let (mut resumed, _) = connect_async(build_ws_request(
        &url,
        "platform_key_ws_tool_context",
        &headers,
    ))
    .await
    .unwrap();
    send_context_json(&mut resumed, follow_up).await;
    let events = receive_context_terminal(&mut resumed).await;
    let expected_terminal = if matches!(
        outcome,
        ToolContextOutcome::Recover | ToolContextOutcome::MixedKnownCalls
    ) {
        "response.completed"
    } else {
        "error"
    };
    assert_eq!(events.last().unwrap()["type"], expected_terminal);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event["type"].as_str(),
                Some("error" | "response.failed" | "response.completed")
            ))
            .count(),
        1
    );
    if matches!(
        outcome,
        ToolContextOutcome::Recover | ToolContextOutcome::MixedKnownCalls
    ) {
        assert_eq!(
            events.last().unwrap()["response"]["id"],
            "resp_tool_context_recovered"
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "response.created")
                .count(),
            1,
            "failed attempt preamble must stay hidden"
        );
    }
    if matches!(outcome, ToolContextOutcome::SubstantiveOutput) {
        assert_eq!(
            events
                .iter()
                .filter(|event| event["type"] == "response.output_text.delta")
                .count(),
            1
        );
        assert_eq!(events[1]["delta"], "already delivered");
    }
    resumed.close(None).await.unwrap();
    let _ = shutdown.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("tool-context proxy shutdown timeout")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), upstream)
        .await
        .expect("tool-context upstream shutdown timeout")
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_tool_context_recovery_preserves_history_across_tasks() {
    let _guard = crate::test_env_guard();
    for tool_search in [false, true] {
        tool_context_case(tool_search, ToolContextOutcome::Recover).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_tool_context_recovers_mixed_calls_with_distinct_retries() {
    let _guard = crate::test_env_guard();
    tool_context_case(false, ToolContextOutcome::MixedKnownCalls).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_tool_context_recovery_is_bounded() {
    let _guard = crate::test_env_guard();
    for tool_search in [false, true] {
        tool_context_case(tool_search, ToolContextOutcome::RejectRetry).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_tool_context_never_replays_real_output_or_unknown_calls() {
    let _guard = crate::test_env_guard();
    for tool_search in [false, true] {
        tool_context_case(tool_search, ToolContextOutcome::SubstantiveOutput).await;
        tool_context_case(tool_search, ToolContextOutcome::UnknownCall).await;
    }
}
