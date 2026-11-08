use super::*;
use std::io::{Cursor, Read};
use std::thread;
use std::time::Duration;

#[test]
fn anthropic_responses_terminal_marks_end_without_waiting_for_provider_eof() {
    use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
    use crate::http::gateway_response_body::GatewayResponseBody;
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    sender.try_send(GatewayByteStreamItem::Chunk(bytes::Bytes::from_static(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"model\":\"test\"}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"))).unwrap();
    let mut reader = ResponsesFromAnthropicSseReader::from_pump(
        UpstreamSseFramePump::from_stream(GatewayByteStream::from_receiver(receiver)),
        Arc::new(Mutex::new(UpstreamResponseUsage::default())),
        None,
        Instant::now(),
    );
    crate::gateway::response_test_runtime()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let mut buffer = [0; 17];
                while !reader.is_end_stream() {
                    assert!(reader.read_async(&mut buffer).await.unwrap() > 0);
                }
                reader.finish_async().await;
            })
            .await
            .expect("complete Responses terminal must not depend on provider EOF");
        });
}

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
            thread::sleep(Duration::from_millis(50));
        }
        Ok(0)
    }
}

#[test]
fn metadata_only_upstream_frame_records_first_response_before_keepalive() {
    let _guard = crate::test_env_guard();
    let _runtime_guard = super::super::SseKeepaliveRuntimeGuard::enabled_with_interval(1);
    let upstream = concat!(
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
    );
    let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
    let mut reader = ResponsesFromAnthropicSseReader::from_reader(
        PausingReader::new(upstream),
        Arc::clone(&usage_collector),
        Some("fallback-model"),
        Instant::now(),
    );
    let mut buf = [0_u8; 4096];

    let mapped = reader.read(&mut buf).expect("read mapped metadata");
    let read = reader.read(&mut buf).expect("read keepalive");

    assert!(mapped > 0);
    assert!(read > 0);
    assert_eq!(
        std::str::from_utf8(&buf[..read]).expect("utf8"),
        std::str::from_utf8(SseKeepAliveFrame::Comment.bytes()).expect("utf8")
    );
    let usage = usage_collector.lock().expect("usage lock").clone();
    assert!(usage.first_response_ms.is_some());
}

#[test]
fn anthropic_text_sse_maps_to_responses_sse() {
    let upstream = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"deepseek/deepseek-v4-pro\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
    let mut reader = ResponsesFromAnthropicSseReader::from_reader(
        Cursor::new(upstream.as_bytes().to_vec()),
        Arc::clone(&usage_collector),
        Some("fallback-model"),
        Instant::now(),
    );
    let mut out = String::new();

    reader.read_to_string(&mut out).expect("read mapped stream");

    assert!(out.contains("event: response.created"));
    assert!(out.contains("event: response.output_text.delta"));
    assert!(out.contains("\"delta\":\"hello\""));
    assert!(out.contains("event: response.completed"));
    assert!(out.contains("\"model\":\"deepseek/deepseek-v4-pro\""));
    let usage = usage_collector.lock().expect("usage lock").clone();
    assert_eq!(usage.input_tokens, Some(3));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.output_text.as_deref(), Some("hello"));
}

#[test]
fn resource_error_after_partial_output_preserves_usage_and_marks_failure() {
    struct ErrorAfterPayload(Cursor<Vec<u8>>);

    impl Read for ErrorAfterPayload {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let read = self.0.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            Err(std::io::Error::other(
                "upstream_sse_frame_too_large: fixture exceeded frame budget",
            ))
        }
    }

    let upstream = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_partial\",\"model\":\"bridge-model\",\"usage\":{\"input_tokens\":11,\"output_tokens\":3}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial output\"}}\n\n",
    );
    let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage {
        completed_successfully: true,
        ..Default::default()
    }));
    let mut reader = ResponsesFromAnthropicSseReader::from_reader(
        ErrorAfterPayload(Cursor::new(upstream.as_bytes().to_vec())),
        Arc::clone(&usage_collector),
        Some("fallback-model"),
        Instant::now(),
    );
    let mut out = String::new();

    reader.read_to_string(&mut out).expect("read failed stream");

    assert!(out.contains("event: response.output_text.delta"));
    assert!(out.contains("event: response.failed"));
    assert!(out.contains("upstream_sse_frame_too_large"));
    assert!(!out.contains("event: response.completed"));
    let usage = usage_collector.lock().expect("usage lock").clone();
    assert!(usage.explicit_failure);
    assert!(!usage.completed_successfully);
    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.output_tokens, Some(3));
    assert_eq!(usage.output_text.as_deref(), Some("partial output"));
}

#[test]
fn anthropic_to_responses_reader_accepts_openai_usage_fields() {
    let upstream = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_openai_usage\",\"model\":\"bridge-model\",\"usage\":{\"prompt_tokens\":11,\"prompt_tokens_details\":{\"cached_tokens\":5},\"completion_tokens\":2,\"completion_tokens_details\":{\"reasoning_tokens\":1},\"total_tokens\":13}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
    let mut reader = ResponsesFromAnthropicSseReader::from_reader(
        Cursor::new(upstream.as_bytes().to_vec()),
        Arc::clone(&usage_collector),
        Some("fallback-model"),
        Instant::now(),
    );
    let mut out = String::new();

    reader.read_to_string(&mut out).expect("read mapped stream");

    assert!(out.contains("\"input_tokens\":11"));
    assert!(out.contains("\"cached_tokens\":5"));
    assert!(out.contains("\"output_tokens\":2"));
    assert!(out.contains("\"reasoning_tokens\":1"));
    assert!(out.contains("\"total_tokens\":13"));
    let usage = usage_collector.lock().expect("usage lock").clone();
    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.cached_input_tokens, Some(5));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(13));
    assert_eq!(usage.reasoning_output_tokens, Some(1));
}

#[test]
fn anthropic_tool_use_sse_is_in_completed_responses_output() {
    let upstream = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_tool\",\"model\":\"deepseek-v4-pro\",\"usage\":{\"input_tokens\":4}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_file\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"/tmp/a\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
    let mut reader = ResponsesFromAnthropicSseReader::from_reader(
        Cursor::new(upstream.as_bytes().to_vec()),
        usage_collector,
        Some("fallback-model"),
        Instant::now(),
    );
    let mut out = String::new();

    reader.read_to_string(&mut out).expect("read mapped stream");

    assert!(out.contains("event: response.output_item.done"));
    assert!(out.contains("\"type\":\"function_call\""));
    assert!(out.contains("\"name\":\"read_file\""));
    assert!(out.contains("\"output\":["));
    assert!(out.contains("\"id\":\"toolu_1\""));
    assert!(out.contains("\"arguments\":\"{\\\"path\\\":\\\"/tmp/a\\\"}\""));
}
