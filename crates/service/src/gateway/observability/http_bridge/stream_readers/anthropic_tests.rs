use super::*;
use std::thread;
use std::time::Duration;

struct PausingReader {
    payload: Cursor<Vec<u8>>,
    paused: bool,
}

impl PausingReader {
    fn new(payload: &str) -> Self {
        Self {
            payload: Cursor::new(payload.as_bytes().to_vec()),
            paused: false,
        }
    }
}

impl Read for PausingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.payload.read(buf)?;
        if read > 0 {
            return Ok(read);
        }
        if !self.paused {
            self.paused = true;
            thread::sleep(Duration::from_millis(200));
        }
        Ok(0)
    }
}

#[test]
fn metadata_only_upstream_frame_records_first_response_before_keepalive() {
    let _guard = crate::test_env_guard();
    let _runtime_guard = super::super::SseKeepaliveRuntimeGuard::enabled_with_interval(1);
    let upstream = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_first\",\"model\":\"gpt-5.4\"}}\n\n",
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = AnthropicSseReader::from_reader(
        PausingReader::new(upstream),
        Arc::clone(&usage_collector),
        Some("fallback-model"),
        None,
        Instant::now(),
    );
    let mut buf = [0_u8; 4096];

    let read = reader.read(&mut buf).expect("read keepalive");

    assert!(read > 0);
    assert_eq!(
        std::str::from_utf8(&buf[..read]).expect("utf8"),
        std::str::from_utf8(SseKeepAliveFrame::Anthropic.bytes()).expect("utf8")
    );
    let usage = usage_collector.lock().expect("usage lock").clone();
    assert!(usage.usage.first_response_ms.is_some());
}

#[test]
fn missing_upstream_model_uses_current_bridge_default() {
    let upstream = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_without_model\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
        "data: [DONE]\n\n"
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = AnthropicSseReader::from_reader(
        Cursor::new(upstream.as_bytes().to_vec()),
        usage_collector,
        None,
        None,
        Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read anthropic SSE");
    assert!(out.contains("\"model\":\"gpt-6-sol\""));
}

fn run_fixture(sse: &str) -> (String, PassthroughSseCollector) {
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = AnthropicSseReader::from_reader(
        Cursor::new(sse.as_bytes().to_vec()),
        Arc::clone(&collector),
        Some("gpt-5.4"),
        None,
        Instant::now(),
    );
    let body = read_body_with_timeout(&mut reader);
    let state = collector.lock().expect("collector").clone();
    (body, state)
}

fn read_body_with_timeout(reader: &mut AnthropicSseReader) -> String {
    use crate::http::gateway_response_body::GatewayResponseBody;
    crate::gateway::response_test_runtime()
        .expect("runtime")
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(1), async {
                let mut bytes = Vec::new();
                // Split the terminal marker across reads to exercise delivery tracking.
                let mut buf = [0_u8; 7];
                loop {
                    let count = reader.read_async(&mut buf).await.expect("read body");
                    if count == 0 {
                        return String::from_utf8(bytes).expect("SSE UTF-8");
                    }
                    bytes.extend_from_slice(&buf[..count]);
                }
            })
            .await
            .expect("terminal event must finish without waiting for upstream EOF")
        })
}

fn error_from_body(body: &str) -> Value {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|value| value.get("error").cloned())
        .expect("Anthropic SSE error")
}

fn assert_no_false_success(body: &str) {
    assert_eq!(body.matches("event: error\n").count(), 1, "{body}");
    assert!(!body.contains("event: message_delta"), "{body}");
    assert!(!body.contains("event: message_stop"), "{body}");
    assert!(!body.contains("data: [DONE]"), "{body}");
}

#[test]
fn completion_finishes_without_waiting_for_upstream_to_close() {
    for terminal in ["response.completed", "response.done"] {
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        sender
            .try_send(UpstreamSseFramePumpItem::Frame(vec![
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n".into(),
                "\n".into(),
            ]))
            .unwrap();
        sender.try_send(UpstreamSseFramePumpItem::Frame(vec![
            format!("event: {terminal}\n"),
            "data: {\"response\":{\"id\":\"resp_test\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"input_tokens_details\":{\"cached_tokens\":2},\"output_tokens\":3,\"total_tokens\":7}}}\n".into(),
            "\n".into(),
        ])).unwrap();
        // Keep the sender alive: the upstream HTTP body has not reached EOF.
        let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
        let mut reader = AnthropicSseReader::from_pump(
            UpstreamSseFramePump::from_receiver(receiver),
            Arc::clone(&collector),
            None,
            None,
            Instant::now(),
        );
        let body = read_body_with_timeout(&mut reader);
        assert!(body.contains("hello"));
        assert_eq!(body.matches("event: message_stop\n").count(), 1);
        assert_eq!(body.matches("event: message_delta\n").count(), 1);
        assert!(body.contains("\"stop_reason\":\"end_turn\""));
        assert!(body.contains("\"cache_read_input_tokens\":2"));
        assert!(!body.contains("event: error"));
        let state = collector.lock().unwrap();
        assert!(state.saw_terminal);
        assert!(state.terminal_delivered);
        assert!(state.usage.completed_successfully);
        assert_eq!(state.last_event_type.as_deref(), Some(terminal));
        assert_eq!(state.usage.response_id.as_deref(), Some("resp_test"));
        assert_eq!(state.usage.total_tokens, Some(7));
        assert_eq!(state.usage.output_text.as_deref(), Some("hello"));
        assert!(state.terminal_error.is_none());
        drop(sender);
    }
}

#[test]
fn explicit_failure_preserves_scalar_error_fields_and_usage() {
    let (body, state) = run_fixture(concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"usage_limit_reached\",\"type\":\"server_error\",\"param\":\"input\",\"message\":\"Synthetic quota failure\",\"raw_body\":\"must not leak\"},\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
    ));
    let error = error_from_body(&body);
    assert_eq!(error["code"], "usage_limit_reached");
    assert_eq!(error["type"], "server_error");
    assert_eq!(error["message"], "Synthetic quota failure");
    assert_eq!(error["param"], "input");
    assert!(!body.contains("must not leak"));
    assert!(state.saw_terminal);
    assert!(state.terminal_delivered);
    assert!(state.usage.explicit_failure);
    assert!(!state.usage.completed_successfully);
    assert_eq!(state.usage.output_text.as_deref(), Some("partial"));
    assert_eq!(state.usage.total_tokens, Some(6));
    assert_eq!(
        state.terminal_error.as_deref(),
        Some("Synthetic quota failure")
    );
    assert_no_false_success(&body);
}

#[test]
fn incomplete_and_error_variants_fail_without_a_successful_stop() {
    for sse in [
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n",
        "event: error\ndata: not-json\n\n",
        "event: response.failed\ndata: not-json\n\n",
        "event: response.error\n\n",
        "event: response.cancelled\n\n",
        "event: response.canceled\n\n",
        "event: message\ndata: {\"type\":\"response.failed\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"failed despite completion event\"}}}\n\n",
        "data: {\"response\":{\"status\":\"incomplete\",\"status_details\":{\"error\":{\"code\":\"timeout\",\"message\":\"Synthetic upstream timeout\"}}}}\n\n",
    ] {
        let (body, state) = run_fixture(sse);
        error_from_body(&body);
        assert!(state.saw_terminal, "{sse}");
        assert!(state.terminal_error.is_some(), "{sse}");
        assert_no_false_success(&body);
    }
    let (body, _) = run_fixture("data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n");
    assert_eq!(
        error_from_body(&body)["code"],
        "upstream_response_incomplete"
    );
    assert_eq!(error_from_body(&body)["message"], "max_output_tokens");
}

#[test]
fn top_level_error_fields_are_preserved() {
    let (body, _) = run_fixture("event: error\ndata: {\"type\":\"error\",\"code\":\"upstream_503\",\"message\":\"Synthetic unavailable\",\"param\":null}\n\n");
    let error = error_from_body(&body);
    assert_eq!(error["code"], "upstream_503");
    assert_eq!(error["message"], "Synthetic unavailable");
    assert_eq!(error["type"], "api_error");
    assert_no_false_success(&body);
}

#[test]
fn eof_and_done_marker_without_completion_are_errors() {
    for sse in [
        "",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_truncated\"\n\n",
        "data: [DONE]\n\n",
    ] {
        let (body, state) = run_fixture(sse);
        assert_eq!(error_from_body(&body)["code"], "upstream_stream_incomplete");
        assert!(!state.saw_terminal);
        assert!(state.terminal_delivered);
        assert!(state.terminal_error.is_some());
        assert_no_false_success(&body);
    }
}

#[test]
fn read_error_and_channel_disconnection_reach_the_client() {
    for read_error in [false, true] {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        if read_error {
            sender
                .try_send(UpstreamSseFramePumpItem::Error(
                    "synthetic connection reset".into(),
                ))
                .unwrap();
        }
        drop(sender);
        let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
        let mut reader = AnthropicSseReader::from_pump(
            UpstreamSseFramePump::from_receiver(receiver),
            Arc::clone(&collector),
            None,
            None,
            Instant::now(),
        );
        let body = read_body_with_timeout(&mut reader);
        let error = error_from_body(&body);
        assert_eq!(
            error["code"],
            if read_error {
                "upstream_stream_read_error"
            } else {
                "upstream_stream_disconnected"
            }
        );
        if read_error {
            assert!(error["message"]
                .as_str()
                .unwrap()
                .contains("synthetic connection reset"));
        }
        let state = collector.lock().unwrap();
        assert!(!state.saw_terminal);
        assert!(state.terminal_error.is_some());
        assert_no_false_success(&body);
    }
}

#[test]
fn keepalive_does_not_hide_an_upstream_idle_timeout() {
    let _guard = crate::test_env_guard();
    let _runtime_guard = super::super::SseKeepaliveRuntimeGuard::enabled_with_interval(1);
    struct Restore(u64);
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::gateway::set_upstream_stream_timeout_ms(self.0);
        }
    }
    let _restore = Restore(crate::gateway::current_upstream_stream_timeout_ms());
    crate::gateway::set_upstream_stream_timeout_ms(25);
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    sender
        .try_send(UpstreamSseFramePumpItem::Frame(vec![
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_idle\"}}\n".into(),
            "\n".into(),
        ]))
        .unwrap();
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = AnthropicSseReader::from_pump(
        UpstreamSseFramePump::from_receiver(receiver),
        Arc::clone(&collector),
        None,
        None,
        Instant::now(),
    );
    let body = read_body_with_timeout(&mut reader);
    assert!(body.contains("event: ping\n"));
    assert_eq!(
        error_from_body(&body)["code"],
        "upstream_stream_idle_timeout"
    );
    assert!(!collector.lock().unwrap().saw_terminal);
    assert_no_false_success(&body);
    drop(sender);
}

#[test]
fn successful_tool_turn_preserves_arguments_and_does_not_duplicate_completion() {
    let (body, state) = run_fixture(concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"checking\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_tool\",\"name\":\"lookup\",\"arguments\":\"{\\\"query\\\":\\\"hello\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"checking\"}]},{\"type\":\"function_call\",\"call_id\":\"call_tool\",\"name\":\"lookup\",\"arguments\":\"{}\"}]}}\n\n",
        "data: [DONE]\n\n",
    ));
    assert_eq!(body.matches("event: message_stop\n").count(), 1);
    assert_eq!(body.matches("\"type\":\"tool_use\"").count(), 1);
    assert_eq!(body.matches("checking").count(), 1);
    assert!(body.contains("\"stop_reason\":\"tool_use\""));
    assert!(body.contains("\"partial_json\":\"{\\\"query\\\":\\\"hello\\\"}\""));
    assert!(state.saw_terminal);
    assert!(state.terminal_error.is_none());
}

#[test]
fn completed_only_tool_calls_preserve_inputs_usage_and_tool_stop() {
    let response = json!({
        "type": "response.completed",
        "response": {
            "id": "resp_completed_tools", "status": "completed", "model": "test-model",
            "output": [
                {"type": "function_call", "id": "fc_a", "call_id": "call_a", "name": "lookup", "arguments": "{\"city\":\"上海\"}"},
                {"type": "function_call", "id": "fc_b", "call_id": "call_b", "name": "count", "arguments": {"n": 2}}
            ],
            "usage": {"input_tokens": 13, "output_tokens": 7, "total_tokens": 20}
        }
    });
    let (body, state) = run_fixture(&format!("data: {response}\n\n"));
    let events = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let starts = events
        .iter()
        .filter(|v| v["type"] == "content_block_start")
        .map(|v| v["content_block"].clone())
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[0]["id"], "call_a");
    assert_eq!(starts[1]["id"], "call_b");
    let inputs = events
        .iter()
        .filter_map(|v| v.pointer("/delta/partial_json").and_then(Value::as_str))
        .map(|v| serde_json::from_str::<Value>(v).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(inputs, vec![json!({"city": "上海"}), json!({"n": 2})]);
    assert_eq!(body.matches("event: message_stop\n").count(), 1);
    assert!(body.contains("\"stop_reason\":\"tool_use\""));
    assert!(state.saw_terminal);
    assert_eq!(state.usage.input_tokens, Some(13));
    assert_eq!(state.usage.output_tokens, Some(7));
    assert!(state.terminal_error.is_none());
}

#[test]
fn completed_tool_snapshot_recovers_missing_calls_without_replaying_done_calls() {
    let tool = json!({"type": "function_call", "id": "fc_a", "call_id": "call_a", "name": "lookup", "arguments": "{\"n\":1}"});
    let events = [
        json!({"type": "response.output_item.done", "item": tool}),
        json!({"type": "response.output_item.done", "item": tool}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [tool,
            {"type": "function_call", "id": "fc_b", "call_id": "call_b", "name": "count", "arguments": "{\"n\":2}"}
        ]}}),
    ];
    let sse = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    let (body, state) = run_fixture(&sse);
    assert_eq!(body.matches("\"type\":\"tool_use\"").count(), 2);
    assert_eq!(body.matches("\"id\":\"call_a\"").count(), 1);
    assert_eq!(body.matches("\"id\":\"call_b\"").count(), 1);
    assert_eq!(body.matches("event: content_block_stop\n").count(), 2);
    assert_eq!(body.matches("event: message_stop\n").count(), 1);
    assert!(body.contains("\"stop_reason\":\"tool_use\""));
    assert!(state.saw_terminal);
}

#[test]
fn failed_completion_snapshots_never_emit_executable_tools() {
    let tool = json!({"type": "function_call", "id": "fc_failure", "call_id": "call_failure", "name": "lookup", "arguments": "{}"});
    for (event_type, status, error_at) in [
        ("response.completed", "failed", None),
        ("response.completed", "incomplete", None),
        ("response.done", "cancelled", None),
        ("response.completed", "completed", Some("response")),
        ("response.completed", "completed", Some("top")),
    ] {
        let mut event = json!({"type": event_type, "response": {"status": status, "output": [tool], "usage": {"input_tokens": 4, "output_tokens": 2}}});
        if let Some(location) = error_at {
            let error =
                json!({"code": "synthetic_failure", "message": "do not execute failed output"});
            if location == "top" {
                event["error"] = error;
            } else {
                event["response"]["error"] = error;
            }
        }
        let (body, state) = run_fixture(&format!("data: {event}\n\n"));
        assert_no_false_success(&body);
        assert!(!body.contains("\"type\":\"tool_use\""), "{body}");
        assert!(state.usage.explicit_failure);
        assert!(!state.usage.completed_successfully);
        assert_eq!(state.usage.output_tokens, Some(2));
    }
}

#[test]
fn tool_snapshots_without_any_id_fail_instead_of_inventing_toolu_unknown() {
    let event = json!({"type": "response.completed", "response": {"status": "completed", "output": [
        {"type": "function_call", "name": "lookup", "arguments": "{}"}
    ]}});
    let (body, state) = run_fixture(&format!("data: {event}\n\n"));
    assert_eq!(
        error_from_body(&body)["code"],
        "upstream_tool_call_missing_id"
    );
    assert_no_false_success(&body);
    assert!(!body.contains("toolu_unknown"));
    assert!(!body.contains("\"type\":\"tool_use\""));
    assert!(!state.usage.completed_successfully);
}

#[test]
fn tool_identity_tracking_has_call_count_and_byte_limits() {
    let make_reader = || {
        AnthropicSseReader::from_reader(
            Cursor::new(Vec::<u8>::new()),
            Arc::new(Mutex::new(PassthroughSseCollector::default())),
            None,
            None,
            Instant::now(),
        )
    };
    let mut reader = make_reader();
    for number in 0..MAX_TRACKED_TOOL_CALLS {
        let item = json!({"type": "function_call", "call_id": format!("call_{number}"), "name": "lookup", "arguments": "{}"});
        reader
            .append_tool_call(item.as_object().unwrap(), &mut String::new())
            .unwrap();
    }
    let extra = json!({"type": "response.output_item.done", "item": {"type": "function_call", "call_id": "overflow", "name": "lookup", "arguments": "{}"}});
    let body =
        String::from_utf8(reader.consume_openai_event(&extra, Some("response.output_item.done")))
            .unwrap();
    assert_eq!(
        error_from_body(&body)["code"],
        "upstream_tool_tracking_limit"
    );
    assert_no_false_success(&body);
    assert_eq!(reader.state.emitted_tool_count, MAX_TRACKED_TOOL_CALLS);
    assert!(reader.state.emitted_tool_ids.len() <= MAX_TRACKED_TOOL_CALLS * 2);
    assert!(reader.state.emitted_tool_id_bytes <= MAX_TRACKED_TOOL_ID_BYTES);

    let mut reader = make_reader();
    for prefix in ["a", "b"] {
        let item = json!({"type": "function_call", "call_id": format!("{prefix}{}", "x".repeat(100_000)), "name": "lookup", "arguments": "{}"});
        reader
            .append_tool_call(item.as_object().unwrap(), &mut String::new())
            .unwrap();
    }
    let extra = json!({"type": "function_call", "call_id": "z".repeat(100_000), "name": "lookup", "arguments": "{}"});
    let mut out = String::new();
    assert_eq!(
        reader
            .append_tool_call(extra.as_object().unwrap(), &mut out)
            .unwrap_err()
            .0,
        "upstream_tool_tracking_limit"
    );
    assert!(out.is_empty());
    assert_eq!(reader.state.emitted_tool_count, 2);
    assert!(reader.state.emitted_tool_id_bytes <= MAX_TRACKED_TOOL_ID_BYTES);

    let mut reader = make_reader();
    let oversized = json!({"type": "function_call", "call_id": "x".repeat(MAX_TRACKED_TOOL_ID_BYTES), "name": "lookup", "arguments": "{}"});
    assert!(reader
        .append_tool_call(oversized.as_object().unwrap(), &mut String::new())
        .is_err());
    assert!(reader.state.emitted_tool_ids.is_empty());
    assert_eq!(reader.state.emitted_tool_id_bytes, 0);
}

#[test]
fn completion_repairs_only_matching_text_suffix_without_replaying_prefix() {
    for (prefix, snapshot, expected) in [
        ("上海", "上海23°C", "上海23°C"),
        ("same", "same", "same"),
        ("first", "different", "first"),
    ] {
        let sse = [json!({"type":"response.output_text.delta","delta":prefix}),
            json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":snapshot}]}]}})]
            .iter().map(|v| format!("data: {v}\n\n")).collect::<String>();
        let (body, _) = run_fixture(&sse);
        let text = body
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter_map(|v| {
                v.pointer("/delta/text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .collect::<String>();
        assert_eq!(text, expected);
        assert_eq!(body.matches("event: message_stop\n").count(), 1);
    }
}

#[test]
fn output_limit_keeps_partial_text_but_never_synthesizes_truncated_tool() {
    let partial = json!({"type":"response.incomplete","response":{
        "status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},
        "output":[{"type":"message","content":[{"type":"output_text","text":"上海23°C"}]},
            {"type":"function_call","call_id":"call_truncated","name":"lookup","arguments":"{\"city\":"}],
        "usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18}
    }});
    let sse = format!("data: {{\"type\":\"response.output_text.delta\",\"delta\":\"上海\"}}\n\ndata: {partial}\n\n");
    let (body, state) = run_fixture(&sse);
    assert!(body.contains("\"stop_reason\":\"max_tokens\""));
    assert!(body.contains("23°C"));
    assert!(!body.contains("\"type\":\"tool_use\""));
    assert!(!body.contains("event: error"));
    assert_eq!(body.matches("event: message_stop\n").count(), 1);
    assert_eq!(state.usage.output_tokens, Some(7));
    assert!(!state.usage.explicit_failure);
    assert!(!state.usage.completed_successfully);
}

#[test]
fn output_limit_does_not_replay_an_already_delivered_complete_tool() {
    let tool = json!({"type":"function_call","id":"fc_done","call_id":"call_done","name":"lookup","arguments":"{}"});
    let events = [
        json!({"type":"response.output_item.done","item":tool}),
        json!({"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[tool,{"type":"message","content":[{"type":"output_text","text":"partial"}]}]}}),
    ];
    let (body, _) = run_fixture(
        &events
            .iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>(),
    );
    assert_eq!(body.matches("\"id\":\"call_done\"").count(), 1);
    assert_eq!(body.matches("\"type\":\"tool_use\"").count(), 1);
    assert!(body.contains("\"stop_reason\":\"max_tokens\""));
}

#[test]
fn tool_sub_event_errors_do_not_abort_successful_response() {
    for event in ["response.mcp_call.failed", "response.mcp_list_tools.failed"] {
        let sse = format!("data: {{\"type\":\"{event}\",\"error\":{{\"message\":\"tool unavailable\"}}}}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\",\"output\":[{{\"type\":\"message\",\"content\":[{{\"type\":\"output_text\",\"text\":\"fallback answer\"}}]}}]}}}}\n\n");
        let (body, state) = run_fixture(&sse);
        assert!(body.contains("fallback answer"));
        assert!(body.contains("event: message_stop"));
        assert!(!body.contains("event: error"));
        assert!(state.terminal_error.is_none());
    }
}
