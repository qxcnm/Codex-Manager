use super::*;
use crate::http::gateway_response_body::GatewayResponseBody;

fn export_fixture(name: &str, body: &[u8]) {
    if let Some(directory) = std::env::var_os("CM_REPAIR_FIXTURE_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(std::path::Path::new(&directory).join(name), body).unwrap();
    }
}

fn collect(chunks: &[Value], done: bool) -> (Vec<Value>, PassthroughSseCollector) {
    let mut wire = chunks
        .iter()
        .map(|value| format!("data: {value}\n\n"))
        .collect::<String>();
    if done {
        wire.push_str("data: [DONE]\n\n");
    }
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_pump(
        UpstreamSseFramePump::from_reader(Cursor::new(wire.into_bytes())),
        collector.clone(),
        Some("fallback"),
        Instant::now(),
    );
    let body = crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            let mut body = Vec::new();
            let mut buffer = [0; 17];
            loop {
                let size = reader.read_async(&mut buffer).await.unwrap();
                if size == 0 {
                    break;
                }
                body.extend_from_slice(&buffer[..size]);
            }
            assert!(reader.is_end_stream());
            body
        });
    if chunks
        .first()
        .is_some_and(|chunk| chunk["id"] == "chatcmpl_x")
    {
        export_fixture("chat-to-responses-text.sse", &body);
    }
    if chunks.first().is_some_and(|chunk| {
        chunk
            .pointer("/choices/0/delta/tool_calls/0/id")
            .is_some_and(|id| id == "call_b")
    }) {
        export_fixture("chat-to-responses-tools.sse", &body);
    }
    let events = String::from_utf8(body)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).unwrap())
        .collect();
    let result = collector.lock().unwrap().clone();
    (events, result)
}

#[test]
fn chat_stream_maps_text_and_usage_after_finish_reason() {
    let (events, collector) = collect(
        &[
            json!({"id":"chatcmpl_x", "model":"test", "choices":[{"index":0,"delta":{"role":"assistant","content":"你"},"finish_reason":null}]}),
            json!({"choices":[{"index":0,"delta":{"content":"好"},"finish_reason":"stop"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":4},"completion_tokens_details":{"reasoning_tokens":1}}}),
        ],
        true,
    );
    assert_eq!(events[0]["type"], "response.created");
    for (sequence, event) in events.iter().enumerate() {
        assert_eq!(event["sequence_number"], sequence as u64);
    }
    let completed = events.last().unwrap();
    assert_eq!(completed["type"], "response.completed");
    assert_eq!(
        completed["response"]["output"][0]["content"][0]["text"],
        "你好"
    );
    assert_eq!(completed["response"]["usage"]["input_tokens"], 10);
    assert_eq!(
        completed["response"]["usage"]["input_tokens_details"]["cached_tokens"],
        4
    );
    assert!(collector.saw_terminal && collector.terminal_delivered);
    assert!(collector.usage.completed_successfully);
    assert_eq!(collector.usage.input_tokens, Some(10));
    assert!(collector
        .usage
        .response_id
        .as_ref()
        .unwrap()
        .starts_with("resp_"));
}

#[test]
fn chat_stream_maps_interleaved_tool_calls_and_text_in_output_order() {
    let (events, _) = collect(
        &[
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_b","type":"function","function":{"name":"second","arguments":"{\"b\":"}},{"index":0,"id":"call_a","type":"function","function":{"name":"first","arguments":"{\"a\":"}}]},"finish_reason":null}]}),
            json!({"choices":[{"index":0,"delta":{"content":"工具说明", "tool_calls":[{"index":0,"function":{"arguments":"1}"}},{"index":1,"function":{"arguments":"2}"}}]},"finish_reason":"tool_calls"}]}),
        ],
        true,
    );
    let output = &events.last().unwrap()["response"]["output"];
    assert_eq!(output[0]["call_id"], "call_b");
    assert_eq!(output[0]["arguments"], "{\"b\":2}");
    assert_eq!(output[1]["call_id"], "call_a");
    assert_eq!(output[1]["arguments"], "{\"a\":1}");
    assert_eq!(output[2]["content"][0]["text"], "工具说明");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "response.function_call_arguments.delta")
            .count(),
        4
    );
}

#[test]
fn chat_stream_premature_eof_is_failed_not_completed() {
    let (events, collector) = collect(
        &[json!({"choices":[{"index":0,"delta":{"content":"partial"},"finish_reason":null}]})],
        false,
    );
    let terminal = events.last().unwrap();
    assert_eq!(terminal["type"], "response.failed");
    assert_eq!(
        terminal["response"]["error"]["code"],
        "upstream_stream_incomplete"
    );
    assert!(collector.terminal_error.is_some());
    assert!(!collector.usage.completed_successfully);
}

#[test]
fn chat_stream_limit_is_incomplete_and_preserves_partial_output() {
    let (events, collector) = collect(
        &[json!({"choices":[{"index":0,"delta":{"content":"partial"},"finish_reason":"length"}]})],
        true,
    );
    let terminal = events.last().unwrap();
    assert_eq!(terminal["type"], "response.incomplete");
    assert_eq!(
        terminal["response"]["incomplete_details"]["reason"],
        "max_output_tokens"
    );
    assert_eq!(
        terminal["response"]["output"][0]["content"][0]["text"],
        "partial"
    );
    assert!(collector.terminal_error.is_none());
    assert!(!collector.usage.completed_successfully);
}

#[test]
fn chat_stream_error_preserves_provider_code_and_message() {
    let (events, collector) = collect(
        &[
            json!({"error":{"code":"quota_exceeded","message":"quota exhausted","type":"rate_limit_error"}}),
        ],
        false,
    );
    assert_eq!(
        events.last().unwrap()["response"]["error"]["code"],
        "quota_exceeded"
    );
    assert_eq!(collector.terminal_error.as_deref(), Some("quota exhausted"));
}

#[test]
fn chat_stream_truncated_or_malformed_tools_are_never_announced_as_done() {
    for reason in ["length", "tool_calls"] {
        let (events, collector) = collect(
            &[
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_partial","function":{"name":"lookup","arguments":"{\"q\":"}}]},"finish_reason":reason}]}),
            ],
            true,
        );
        assert_eq!(events.last().unwrap()["type"], "response.failed");
        assert!(!events
            .iter()
            .any(|event| event["type"] == "response.function_call_arguments.done"));
        assert!(!events
            .iter()
            .any(|event| event["type"] == "response.output_item.done"));
        assert!(collector.terminal_error.is_some());
    }
}

#[test]
fn chat_json_maps_text_tools_refusal_and_usage() {
    let body = json!({"object":"chat.completion","model":"test","created":123,"choices":[{"index":0,"message":{"content":"text","refusal":"refusal","tool_calls":[{"id":"call_a","type":"function","function":{"name":"lookup","arguments":"{}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":3,"completion_tokens":5,"total_tokens":8}});
    let mapped: Value = serde_json::from_slice(
        &convert_chat_completions_body_to_responses(&serde_json::to_vec(&body).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(mapped["status"], "completed");
    assert_eq!(mapped["output"][0]["content"][0]["text"], "text");
    assert_eq!(mapped["output"][1]["content"][0]["refusal"], "refusal");
    assert_eq!(mapped["output"][2]["call_id"], "call_a");
    assert_eq!(mapped["usage"]["total_tokens"], 8);
    assert_eq!(mapped["created_at"], 123);
}

#[test]
fn chat_json_error_and_limit_do_not_become_success() {
    for (body, status) in [
        (json!({"error":{"code":"broken","message":"bad"}}), "failed"),
        (
            json!({"choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}),
            "incomplete",
        ),
        (
            json!({"choices":[{"message":{"content":"partial"},"finish_reason":null}]}),
            "failed",
        ),
    ] {
        let mapped: Value = serde_json::from_slice(
            &convert_chat_completions_body_to_responses(&serde_json::to_vec(&body).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(mapped["status"], status);
    }
}

#[test]
fn chat_usage_totals_are_saturating_and_nonnegative() {
    for usage in [
        json!({"prompt_tokens":i64::MAX,"completion_tokens":i64::MAX}),
        json!({"prompt_tokens":-1,"completion_tokens":-2,"total_tokens":-3}),
    ] {
        let body =
            json!({"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"usage":usage});
        let mapped: Value = serde_json::from_slice(
            &convert_chat_completions_body_to_responses(&serde_json::to_vec(&body).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(mapped["usage"]["input_tokens"].as_i64().unwrap() >= 0);
        assert!(mapped["usage"]["total_tokens"].as_i64().unwrap() >= 0);
    }
}

#[test]
fn chat_terminal_body_does_not_wait_for_upstream_eof() {
    use crate::gateway::upstream::GatewayByteStream;
    use crate::gateway::upstream::GatewayByteStreamItem;
    use bytes::Bytes;
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    sender.try_send(GatewayByteStreamItem::Chunk(Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))).unwrap();
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ResponsesFromChatCompletionsSseReader::from_pump(
        UpstreamSseFramePump::from_stream(GatewayByteStream::from_receiver(receiver)),
        collector,
        None,
        Instant::now(),
    );
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                let mut buffer = [0; 4096];
                while !reader.is_end_stream() {
                    assert!(reader.read_async(&mut buffer).await.unwrap() > 0);
                }
                reader.finish_async().await;
            })
            .await
            .expect("terminal must not wait for provider socket close");
        });
}
