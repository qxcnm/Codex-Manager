use super::*;
use std::io::{self, Cursor, Read};

fn run_fixture(sse: &str) -> (String, PassthroughSseCollector) {
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::from_pump(
        UpstreamSseFramePump::from_reader(Cursor::new(sse.as_bytes().to_vec())),
        Arc::clone(&collector),
        Instant::now(),
    );
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    let state = collector.lock().unwrap().clone();
    (body, state)
}

fn error_from_body(body: &str) -> Value {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|v| v.get("error").cloned())
        .expect("downstream must receive an explicit error")
}

fn no_false_success(body: &str) {
    assert!(!body.contains("\"finish_reason\":\"stop\""));
    assert!(!body.contains("\"finish_reason\":\"tool_calls\""));
    assert!(!body.contains("data: [DONE]"));
}

#[test]
fn repair_eof_emits_error_without_fabricating_success() {
    let (body, state) =
        run_fixture("data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n");
    assert_eq!(error_from_body(&body)["code"], "upstream_stream_incomplete");
    assert!(state.terminal_error.is_some());
    assert!(!state.saw_terminal);
    assert_eq!(
        state.last_event_type.as_deref(),
        Some("response.output_text.delta")
    );
    no_false_success(&body);
}

#[test]
fn repair_explicit_failure_preserves_code_param_and_message() {
    let (body, state) = run_fixture("data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_synthetic\",\"status\":\"failed\",\"error\":{\"code\":\"usage_limit_reached\",\"type\":\"server_error\",\"param\":\"input\",\"message\":\"Synthetic quota failure\"}}}\n\n");
    let error = error_from_body(&body);
    assert_eq!(error["code"], "usage_limit_reached");
    assert_eq!(error["message"], "Synthetic quota failure");
    assert_eq!(error["param"], "input");
    assert!(state.saw_terminal);
    assert_eq!(state.last_event_type.as_deref(), Some("response.failed"));
    no_false_success(&body);
}

#[test]
fn repair_incomplete_is_not_a_success() {
    let (body, state) = run_fixture("data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n");
    assert_eq!(
        error_from_body(&body)["code"],
        "upstream_response_incomplete"
    );
    assert!(state.saw_terminal);
    no_false_success(&body);
}

#[test]
fn repair_valid_completion_keeps_stop_and_usage() {
    let (body, state) = run_fixture("data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n");
    assert!(body.contains("\"finish_reason\":\"stop\""));
    assert!(body.contains("data: [DONE]"));
    assert!(state.saw_terminal);
    assert!(state.terminal_error.is_none());
}

struct BrokenReader;
impl Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::ConnectionReset,
            "synthetic connection reset",
        ))
    }
}

#[test]
fn repair_read_error_reaches_downstream() {
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::from_pump(
        UpstreamSseFramePump::from_reader(BrokenReader),
        collector,
        Instant::now(),
    );
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    let error = error_from_body(&body);
    assert_eq!(error["code"], "upstream_stream_read_error");
    assert!(error["message"]
        .as_str()
        .unwrap()
        .contains("synthetic connection reset"));
    no_false_success(&body);
}

#[test]
fn repair_top_level_error_fields_are_preserved() {
    let (body, _) = run_fixture("event: error\ndata: {\"type\":\"error\",\"code\":\"upstream_503\",\"message\":\"Synthetic unavailable\",\"param\":null}\n\n");
    let error = error_from_body(&body);
    assert_eq!(error["code"], "upstream_503");
    assert_eq!(error["message"], "Synthetic unavailable");
    no_false_success(&body);
}

#[test]
fn repair_done_marker_without_completed_is_not_success() {
    let (body, state) = run_fixture("data: [DONE]\n\n");
    assert_eq!(error_from_body(&body)["code"], "upstream_stream_incomplete");
    assert!(!state.saw_terminal);
    no_false_success(&body);
}

#[test]
fn repair_terminal_error_variants_fail_closed() {
    for sse in [
        "event: response.cancelled\n\n",
        "event: response.failed\ndata: not-json\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"Synthetic failed despite event name\"}}}\n\n",
        "data: {\"response\":{\"status\":\"incomplete\",\"status_details\":{\"error\":{\"code\":\"timeout\",\"message\":\"Synthetic upstream timeout\"}}}}\n\n",
    ] {
        let (body, state) = run_fixture(sse);
        error_from_body(&body);
        assert!(state.terminal_error.is_some());
        no_false_success(&body);
    }
}

#[test]
fn repair_idle_timeout_reaches_downstream() {
    let _guard = crate::test_env_guard();
    struct Restore(u64);
    impl Drop for Restore {
        fn drop(&mut self) {
            crate::gateway::set_upstream_stream_timeout_ms(self.0);
        }
    }
    let _restore = Restore(crate::gateway::current_upstream_stream_timeout_ms());
    crate::gateway::set_upstream_stream_timeout_ms(10);
    let (_sender, receiver) = tokio::sync::mpsc::channel(1);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::from_pump(
        UpstreamSseFramePump::from_stream(
            crate::gateway::upstream::GatewayByteStream::from_receiver(receiver),
        ),
        collector,
        Instant::now(),
    );
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    assert_eq!(
        error_from_body(&body)["code"],
        "upstream_stream_idle_timeout"
    );
    no_false_success(&body);
}

// Optional local synthetic artefacts for exercising the real Pi parser. No credentials.
#[test]
fn repair_export_synthetic_wires() {
    let Ok(dir) = std::env::var("CM_REPAIR_FIXTURE_DIR") else {
        return;
    };
    let root = std::path::Path::new(&dir);
    assert!(root.is_absolute());
    std::fs::create_dir_all(root).unwrap();
    let cases = [
        ("eof", "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n"),
        ("failed", "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"usage_limit_reached\",\"type\":\"server_error\",\"message\":\"Synthetic quota failure\"}}}\n\n"),
        ("complete", "data: {\"type\":\"response.output_text.delta\",\"delta\":\"SYNTHETIC_OK\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"),
        ("tool", "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"call_id\":\"call_fixture\",\"name\":\"read_fixture\",\"arguments\":\"{}\"}]}}\n\n"),
    ];
    for (name, sse) in cases {
        let (body, state) = run_fixture(sse);
        std::fs::write(root.join(format!("{name}.sse")), body).unwrap();
        std::fs::write(root.join(format!("{name}.json")), serde_json::to_vec_pretty(&serde_json::json!({
            "saw_terminal":state.saw_terminal,"terminal_error":state.terminal_error,"last_event_type":state.last_event_type
        })).unwrap()).unwrap();
    }
}
