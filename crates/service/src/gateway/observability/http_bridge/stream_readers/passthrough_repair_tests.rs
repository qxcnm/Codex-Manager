// 真实断流修复的定向回归：上游没有终止事件时，必须把原因显式传给下游，而不是静默结束。
use super::*;
use std::io::{self, Cursor};

fn run_fixture(sse: &str) -> (String, PassthroughSseCollector) {
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = PassthroughSseUsageReader::from_pump(
        UpstreamSseFramePump::from_reader(Cursor::new(sse.as_bytes().to_vec())),
        Arc::clone(&collector),
        SseKeepAliveFrame::Comment,
        PassthroughSseProtocol::Generic,
        Instant::now(),
    );
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    let state = collector.lock().unwrap().clone();
    (body, state)
}

fn error_frame(body: &str) -> serde_json::Value {
    body.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value.get("error").is_some())
        .expect("downstream must receive an explicit error frame")
}

#[test]
fn repair_passthrough_eof_emits_error_without_terminal_event() {
    let (body, state) = run_fixture("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"PARTIAL\"},\"finish_reason\":null}]}\n\n");
    let frame = error_frame(&body);
    assert_eq!(frame["code"], "upstream_stream_incomplete");
    assert_eq!(frame["type"], "error");
    assert!(state.terminal_error.is_some());
    assert!(!state.saw_terminal);
    assert!(!body.contains("\"finish_reason\":\"stop\""));
    assert!(!body.contains("[DONE]"));
    assert!(body.contains("PARTIAL"), "已收到的内容必须保留给客户端");
}

struct BrokenReader;
impl Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::ConnectionReset, "synthetic connection reset"))
    }
}

#[test]
fn repair_passthrough_read_error_reaches_downstream() {
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = PassthroughSseUsageReader::from_pump(
        UpstreamSseFramePump::from_reader(BrokenReader),
        collector,
        SseKeepAliveFrame::Comment,
        PassthroughSseProtocol::Generic,
        Instant::now(),
    );
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    let frame = error_frame(&body);
    assert_eq!(frame["code"], "upstream_stream_read_error");
    assert!(frame["error"]["message"].as_str().unwrap().contains("synthetic connection reset"));
}

#[test]
fn repair_passthrough_valid_completion_is_unchanged() {
    let (body, state) = run_fixture("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
    assert!(body.contains("\"finish_reason\":\"stop\""));
    assert!(body.contains("[DONE]"));
    assert!(state.saw_terminal);
    assert!(error_frame_absent(&body));
}

fn error_frame_absent(body: &str) -> bool {
    !body.lines().any(|line| line.contains("\"error\":{"))
}
