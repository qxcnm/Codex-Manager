use super::*;
use bytes::Bytes;
use eventsource_stream::{Event, Eventsource};
use futures_util::StreamExt;
use std::io;
use std::time::Duration;

fn reader_from_pump(
    pump: UpstreamSseFramePump,
    protocol: PassthroughSseProtocol,
) -> (
    PassthroughSseUsageReader,
    Arc<Mutex<PassthroughSseCollector>>,
) {
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let reader = PassthroughSseUsageReader::from_pump(
        pump,
        Arc::clone(&collector),
        match protocol {
            PassthroughSseProtocol::Generic => SseKeepAliveFrame::Comment,
            PassthroughSseProtocol::AnthropicNative => SseKeepAliveFrame::Anthropic,
        },
        protocol,
        Instant::now(),
    );
    (reader, collector)
}

fn run_fixture(sse: &str, protocol: PassthroughSseProtocol) -> (String, PassthroughSseCollector) {
    let _env = crate::test_env_guard();
    let (mut reader, collector) = reader_from_pump(
        UpstreamSseFramePump::from_stream(crate::gateway::upstream::GatewayByteStream::from_bytes(
            Bytes::copy_from_slice(sse.as_bytes()),
        )),
        protocol,
    );
    let mut body = String::new();
    reader.read_to_string(&mut body).unwrap();
    let state = collector.lock().unwrap().clone();
    (body, state)
}

// Use a client SSE parser, including its multiline-data and event-dispatch rules.
// Parsing individual data lines would miss merged events and truncated JSON.
fn events(body: &str) -> Vec<(String, serde_json::Value)> {
    let chunks = body
        .as_bytes()
        .chunks(3)
        .map(|chunk| Ok::<_, io::Error>(Bytes::copy_from_slice(chunk)))
        .collect::<Vec<_>>();
    let parsed: Vec<Event> = crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            futures_util::stream::iter(chunks)
                .eventsource()
                .map(|event| event.expect("valid downstream SSE event"))
                .collect()
                .await
        });
    parsed
        .into_iter()
        .filter(|event| event.data != "[DONE]")
        .map(|event| {
            let json = serde_json::from_str(&event.data)
                .unwrap_or_else(|error| panic!("invalid event JSON {:?}: {error}", event.data));
            (event.event, json)
        })
        .collect()
}

fn assert_error(body: &str, protocol: PassthroughSseProtocol, code: &str) -> serde_json::Value {
    let errors = events(body)
        .into_iter()
        .filter(|(_, value)| value.get("error").is_some())
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1, "exactly one parseable downstream error");
    let (event, frame) = errors.into_iter().next().unwrap();
    assert_eq!(frame["type"], "error");
    assert_eq!(frame["error"]["code"], code);
    match protocol {
        PassthroughSseProtocol::Generic => {
            assert_eq!(event, "message");
            assert_eq!(frame["code"], code);
            assert_eq!(frame["error"]["type"], "upstream_error");
        }
        PassthroughSseProtocol::AnthropicNative => {
            assert_eq!(event, "error");
            assert_eq!(frame["error"]["type"], "api_error");
        }
    }
    assert!(!body.contains("[DONE]"));
    frame
}

const CHAT_PARTIAL: &str = r#"data: {"object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"PARTIAL"},"finish_reason":null}]}"#;
const ANTHROPIC_PARTIAL: &str = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"PARTIAL\"}}";

#[test]
fn repair_passthrough_closes_valid_tail_before_eof_error() {
    for (protocol, partial) in [
        (PassthroughSseProtocol::Generic, CHAT_PARTIAL),
        (PassthroughSseProtocol::AnthropicNative, ANTHROPIC_PARTIAL),
    ] {
        for suffix in ["", "\n", "\n\n", "\r\n", "\r\n\r\n"] {
            let (body, state) = run_fixture(&format!("{partial}{suffix}"), protocol);
            let parsed = events(&body);
            assert_eq!(parsed.len(), 2, "suffix={suffix:?}");
            assert!(parsed[0].1.to_string().contains("PARTIAL"));
            assert_error(&body, protocol, "upstream_stream_incomplete");
            assert!(state.terminal_error.is_some());
            assert!(!state.saw_terminal);
        }
    }
}

#[test]
fn repair_passthrough_discards_truncated_tail_and_preserves_complete_events() {
    for (protocol, partial, tail) in [
        (
            PassthroughSseProtocol::Generic,
            CHAT_PARTIAL,
            "data: {\"choices\":[{\"delta\":{\"content\":\"TRUNCATED",
        ),
        (
            PassthroughSseProtocol::AnthropicNative,
            ANTHROPIC_PARTIAL,
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"TRUNCATED",
        ),
    ] {
        for suffix in ["", "\n", "\r\n"] {
            let (body, state) =
                run_fixture(&format!("{partial}\n\n{tail}{suffix}"), protocol);
            assert_eq!(events(&body).len(), 2);
            assert!(body.contains("PARTIAL"));
            assert!(!body.contains("TRUNCATED"));
            assert_error(&body, protocol, "upstream_stream_incomplete");
            assert!(!state.saw_terminal);
        }
    }
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
fn repair_passthrough_read_error_reaches_downstream() {
    for protocol in [
        PassthroughSseProtocol::Generic,
        PassthroughSseProtocol::AnthropicNative,
    ] {
        let (mut reader, _) =
            reader_from_pump(UpstreamSseFramePump::from_reader(BrokenReader), protocol);
        let mut body = String::new();
        reader.read_to_string(&mut body).unwrap();
        let frame = assert_error(&body, protocol, "upstream_stream_read_error");
        assert!(frame["error"]["message"]
            .as_str()
            .unwrap()
            .contains("synthetic connection reset"));
    }
}

#[derive(Clone, Copy, Debug)]
enum Ending {
    Eof,
    ReadError,
    IdleTimeout,
    Disconnected,
}

const ENDINGS: [Ending; 4] = [
    Ending::Eof,
    Ending::ReadError,
    Ending::IdleTimeout,
    Ending::Disconnected,
];

struct StreamTimeoutGuard {
    value: u64,
    env: Option<std::ffi::OsString>,
}

impl StreamTimeoutGuard {
    fn new() -> Self {
        let guard = Self {
            value: crate::gateway::current_upstream_stream_timeout_ms(),
            env: std::env::var_os("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS"),
        };
        crate::gateway::set_upstream_stream_timeout_ms(25);
        guard
    }
}

impl Drop for StreamTimeoutGuard {
    fn drop(&mut self) {
        crate::gateway::set_upstream_stream_timeout_ms(self.value);
        match &self.env {
            Some(value) => std::env::set_var("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS", value),
            None => std::env::remove_var("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS"),
        }
    }
}

fn run_ending_fixture(
    sse: &str,
    protocol: PassthroughSseProtocol,
    ending: Ending,
) -> (String, PassthroughSseCollector) {
    let _env = crate::test_env_guard();
    let _timeout = StreamTimeoutGuard::new();
    let frames = sse.split_inclusive("\n\n").collect::<Vec<_>>();
    let (tx, rx) = tokio::sync::mpsc::channel(frames.len() + 1);
    for frame in &frames {
        tx.try_send(UpstreamSseFramePumpItem::Frame(
            frame.split_inclusive('\n').map(str::to_string).collect(),
        ))
        .unwrap();
    }
    let (mut reader, collector) =
        reader_from_pump(UpstreamSseFramePump::from_receiver(rx), protocol);
    let mut body = String::new();
    for _ in frames {
        let chunk = crate::gateway::response_test_runtime()
            .unwrap()
            .block_on(reader.next_chunk())
            .unwrap();
        body.push_str(&String::from_utf8(chunk).unwrap());
    }
    match ending {
        Ending::Eof => tx.try_send(UpstreamSseFramePumpItem::Eof).unwrap(),
        Ending::ReadError => tx
            .try_send(UpstreamSseFramePumpItem::Error(
                "connection reset".to_string(),
            ))
            .unwrap(),
        Ending::IdleTimeout => {
            reader.last_upstream_activity = Instant::now() - Duration::from_secs(1);
        }
        Ending::Disconnected => drop(tx),
    }
    reader.read_to_string(&mut body).unwrap();
    let state = collector.lock().unwrap().clone();
    (body, state)
}

#[test]
fn repair_passthrough_all_endings_report_unfinished_text_and_tools() {
    for (protocol, partial) in [
        (PassthroughSseProtocol::Generic, CHAT_PARTIAL),
        (
            PassthroughSseProtocol::Generic,
            r#"data: {"object":"chat.completion.chunk","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]},"finish_reason":null}]}"#,
        ),
        (PassthroughSseProtocol::AnthropicNative, ANTHROPIC_PARTIAL),
        (
            PassthroughSseProtocol::AnthropicNative,
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}",
        ),
    ] {
        let sse = format!("{partial}\n\n");
        for ending in ENDINGS {
            let (body, state) = run_ending_fixture(&sse, protocol, ending);
            let code = match ending {
                Ending::Eof => "upstream_stream_incomplete",
                Ending::ReadError => "upstream_stream_read_error",
                Ending::IdleTimeout => "upstream_stream_idle_timeout",
                Ending::Disconnected => "upstream_stream_disconnected",
            };
            assert!(body.starts_with(&sse));
            assert_error(&body, protocol, code);
            assert!(!state.saw_terminal);
            assert!(state.terminal_error.is_some());
        }
    }
}

#[test]
fn repair_passthrough_all_endings_preserve_response_success() {
    for (protocol, sse) in [
        (
            PassthroughSseProtocol::Generic,
            "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\n",
        ),
        (
            PassthroughSseProtocol::Generic,
            "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        ),
        (PassthroughSseProtocol::Generic, "data: [DONE]\n\n"),
        (
            PassthroughSseProtocol::Generic,
            "event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n",
        ),
        (
            PassthroughSseProtocol::AnthropicNative,
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ),
        (
            PassthroughSseProtocol::AnthropicNative,
            concat!(
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"OK\"}}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            ),
        ),
        (
            PassthroughSseProtocol::AnthropicNative,
            concat!(
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
                "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
            ),
        ),
    ] {
        for ending in ENDINGS {
            let (body, state) = run_ending_fixture(sse, protocol, ending);
            assert_eq!(body, sse, "{protocol:?}/{ending:?}");
            assert!(state.saw_terminal);
            assert!(state.terminal_error.is_none());
            assert!(!events(&body).iter().any(|(_, value)| value.get("error").is_some()));
        }
    }
}

#[test]
fn repair_passthrough_tool_events_are_not_response_success() {
    for event in [
        "response.web_search_call.completed",
        "response.web_search_call.failed",
        "response.code_interpreter_call.incomplete",
        "response.function_call_arguments.done",
        "response.output_item.done",
        "content_block_stop",
    ] {
        for protocol in [
            PassthroughSseProtocol::Generic,
            PassthroughSseProtocol::AnthropicNative,
        ] {
            let sse = format!("event: {event}\ndata: {{\"type\":\"{event}\"}}\n\n");
            for ending in ENDINGS {
                let (body, state) = run_ending_fixture(&sse, protocol, ending);
                assert!(!state.saw_terminal, "{protocol:?}/{event}/{ending:?}");
                assert_eq!(events(&body).len(), 2);
                assert!(events(&body)[1].1.get("error").is_some());
            }
        }
    }
}

#[test]
fn repair_passthrough_valid_completion_is_unchanged() {
    let sse = "data: {\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"OK\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let (body, state) = run_fixture(sse, PassthroughSseProtocol::Generic);
    assert_eq!(body, sse);
    assert!(state.saw_terminal);
    assert!(!events(&body)
        .iter()
        .any(|(_, value)| value.get("error").is_some()));
}

#[test]
fn repair_passthrough_mcp_item_errors_allow_response_to_continue() {
    for event in ["response.mcp_call.failed", "response.mcp_list_tools.failed"] {
        for with_header in [true, false] {
            for message in ["tool server failed", "quota exceeded"] {
                let payload = serde_json::json!({
                    "type": event,
                    "error": {"code": "tool_error", "message": message}
                });
                let tool = if with_header {
                    format!("event: {event}\ndata: {payload}\n\n")
                } else {
                    format!("data: {payload}\n\n")
                };
                let (body, state) = run_fixture(&tool, PassthroughSseProtocol::Generic);
                assert!(!state.saw_terminal, "{event}/{with_header}/{message}");
                assert_eq!(events(&body).len(), 2);
                assert_eq!(events(&body)[1].1["code"], "upstream_stream_incomplete");
                let success =
                    "event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n";
                let sse = format!("{tool}{success}");
                let (body, state) = run_fixture(&sse, PassthroughSseProtocol::Generic);
                assert_eq!(body, sse);
                assert!(state.saw_terminal);
                assert!(state.terminal_error.is_none());
            }
        }
    }
}

#[test]
fn repair_passthrough_upstream_response_error_is_not_duplicated() {
    for (protocol, sse) in [
        (
            PassthroughSseProtocol::Generic,
            "data: {\"error\":{\"message\":\"upstream failure\",\"code\":\"server_error\"}}\n\n",
        ),
        (
            PassthroughSseProtocol::Generic,
            "event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"message\":\"upstream failure\"}}}\n\n",
        ),
        (
            PassthroughSseProtocol::AnthropicNative,
            "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"upstream failure\"}}\n\n",
        ),
    ] {
        for ending in ENDINGS {
            let (body, state) = run_ending_fixture(sse, protocol, ending);
            assert_eq!(body, sse, "{protocol:?}/{ending:?}");
            assert!(state.saw_terminal);
            assert!(state
                .terminal_error
                .as_deref()
                .is_some_and(|message| message.contains("upstream failure")));
            assert_eq!(events(&body).len(), 1);
        }
    }
}

#[test]
fn repair_passthrough_valid_terminal_tail_is_closed_without_error() {
    for (protocol, terminal) in [
        (PassthroughSseProtocol::Generic, "data: [DONE]"),
        (
            PassthroughSseProtocol::AnthropicNative,
            "event: message_stop\ndata: {\"type\":\"message_stop\"}",
        ),
    ] {
        for suffix in ["", "\n"] {
            let (body, state) = run_fixture(&format!("{terminal}{suffix}"), protocol);
            assert_eq!(body, format!("{terminal}\n\n"));
            assert!(state.saw_terminal);
            assert!(state.terminal_error.is_none());
            events(&body);
        }
    }
}

#[test]
fn repair_passthrough_terminal_name_without_data_is_not_response_success() {
    for (protocol, event) in [
        (PassthroughSseProtocol::Generic, "response.completed"),
        (PassthroughSseProtocol::AnthropicNative, "message_stop"),
    ] {
        for suffix in ["", "\n", "\n\n"] {
            let (body, state) = run_fixture(&format!("event: {event}{suffix}"), protocol);
            assert!(!state.saw_terminal);
            assert_error(&body, protocol, "upstream_stream_incomplete");
        }
    }
}

#[test]
fn repair_passthrough_response_envelope_failures_override_completion() {
    for event in ["response.created", "response.completed", "response.done"] {
        for response in [
            serde_json::json!({"error": {"message": "response failed"}}),
            serde_json::json!({"status_details": {"error": {"message": "response failed"}}}),
            serde_json::json!({"status": "failed"}),
            serde_json::json!({"status": "incomplete"}),
            serde_json::json!({"status": "cancelled"}),
            serde_json::json!({"status": "canceled"}),
        ] {
            let payload = serde_json::json!({"type": event, "response": response});
            for with_header in [true, false] {
                let sse = if with_header {
                    format!("event: {event}\ndata: {payload}\n\n")
                } else {
                    format!("data: {payload}\n\n")
                };
                let (body, state) = run_fixture(&sse, PassthroughSseProtocol::Generic);
                assert_eq!(body, sse);
                assert!(state.saw_terminal, "{event}/{response}/{with_header}");
                assert!(
                    state.terminal_error.is_some(),
                    "{event}/{response}/{with_header}"
                );
                assert_eq!(events(&body).len(), 1);
            }
        }
        let payload = serde_json::json!({"type": event, "error": {"message": "lifecycle failed"}});
        let sse = format!("event: {event}\ndata: {payload}\n\n");
        let (body, state) = run_fixture(&sse, PassthroughSseProtocol::Generic);
        assert_eq!(body, sse);
        assert_eq!(state.terminal_error.as_deref(), Some("lifecycle failed"));
    }
}

#[test]
fn repair_passthrough_generic_message_header_uses_payload_type() {
    for (payload, failed) in [
        (
            serde_json::json!({"type": "error", "error": {"message": "response failed"}}),
            true,
        ),
        (serde_json::json!({"type": "response.done"}), false),
    ] {
        let sse = format!("event: message\ndata: {payload}\n\n");
        let (body, state) = run_fixture(&sse, PassthroughSseProtocol::Generic);
        assert_eq!(body, sse);
        assert!(state.saw_terminal);
        assert_eq!(state.terminal_error.is_some(), failed);
    }
}

#[test]
fn repair_passthrough_malformed_named_error_is_failure_not_success() {
    let lines = vec![
        "event: error\n".to_string(),
        "data: not-json\n".to_string(),
        "\n".to_string(),
    ];
    for protocol in [
        PassthroughSseProtocol::Generic,
        PassthroughSseProtocol::AnthropicNative,
    ] {
        let inspected = inspect_sse_frame_for_protocol(&lines, protocol);
        assert!(matches!(inspected.terminal, Some(SseTerminal::Err(_))));
    }
    let lines = vec![
        "event: response.completed\n".to_string(),
        "data: not-json\n".to_string(),
        "\n".to_string(),
    ];
    let inspected = inspect_sse_frame_for_protocol(&lines, PassthroughSseProtocol::Generic);
    assert!(inspected.terminal.is_none());
}
