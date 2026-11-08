use super::responses_output_repair::ResponsesOutputRepair;
use super::{
    classify_upstream_stream_read_error, mark_first_response_ms, mark_terminal_delivered,
    stream_idle_timed_out, stream_idle_timeout_message, stream_reader_disconnected_message,
    stream_wait_timeout, upstream_hint_or_stream_incomplete_message, Arc, Cursor, Mutex,
    OpenAIResponsesEvent, OpenAIResponsesOutputTextState, PassthroughSseCollector, Read,
    SseKeepAliveFrame, SseTerminal, UpstreamSseFramePump, UpstreamSseFramePumpItem,
};
use crate::gateway::upstream::GatewayStreamResponse;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Instant;

pub(crate) struct OpenAIResponsesPassthroughSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    usage_text_state: OpenAIResponsesOutputTextState,
    output_repair: ResponsesOutputRepair,
    keepalive_frame: SseKeepAliveFrame,
    request_started_at: Instant,
    pending_separator: bool,
    finished: bool,
    compatibility_events: bool,
}

impl OpenAIResponsesPassthroughSseReader {
    #[cfg(test)]
    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        keepalive_frame: SseKeepAliveFrame,
        request_started_at: Instant,
    ) -> Self {
        Self::from_stream_response(
            GatewayStreamResponse::from_blocking_response(upstream),
            usage_collector,
            keepalive_frame,
            request_started_at,
        )
    }

    pub(crate) fn from_stream_response(
        upstream: GatewayStreamResponse,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        keepalive_frame: SseKeepAliveFrame,
        request_started_at: Instant,
    ) -> Self {
        Self {
            upstream: UpstreamSseFramePump::from_stream(upstream.into_body()),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            usage_text_state: OpenAIResponsesOutputTextState::default(),
            output_repair: ResponsesOutputRepair::default(),
            keepalive_frame,
            request_started_at,
            pending_separator: false,
            finished: false,
            compatibility_events: false,
        }
    }

    pub(crate) fn with_compatibility_events(mut self, enabled: bool) -> Self {
        self.compatibility_events = enabled;
        self
    }

    fn update_usage_from_event(&mut self, event: OpenAIResponsesEvent) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(event_type) = event.event_type.as_ref() {
                collector.last_event_type = Some(event_type.clone());
            }
            event.merge_usage_into(&mut collector.usage, &mut self.usage_text_state);
            if let Some(upstream_error_hint) = event.upstream_error_hint.as_ref() {
                collector.upstream_error_hint = Some(upstream_error_hint.clone());
            }
            if let Some(terminal) = event.terminal.as_ref() {
                collector.saw_terminal = true;
                if let SseTerminal::Err(message) = terminal {
                    collector.terminal_error = Some(message.clone());
                }
            }
        }
    }

    fn update_usage_from_frame(&mut self, frame: &[String]) -> Option<bool> {
        let event = OpenAIResponsesEvent::parse(frame)?;
        let terminal_delivered = Self::is_delivered_terminal(&event);
        self.update_usage_from_event(event);
        Some(terminal_delivered)
    }

    fn is_delivered_terminal(event: &OpenAIResponsesEvent) -> bool {
        // Use the parsed top-level type, never terminal words inside ordinary
        // deltas or nested tool data. Generic errors still need normalization to
        // response.failed even though they are semantic upstream failures.
        matches!(
            event.event_type.as_deref(),
            Some(
                "response.completed" | "response.failed" | "response.incomplete" | "response.done"
            )
        )
    }

    fn is_standalone_done_frame(frame: &[String]) -> bool {
        // Check the complete joined data payload without allocating another copy
        // of a potentially large malformed frame. Empty data lines only add
        // surrounding whitespace; every other nonempty line must be rejected.
        let mut saw_done = false;
        for (index, line) in frame.iter().enumerate() {
            let line = if index == 0 {
                line.strip_prefix('\u{feff}').unwrap_or(line)
            } else {
                line
            };
            let line = line.trim_end_matches(['\r', '\n']);
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data != "[DONE]" || saw_done {
                return false;
            }
            saw_done = true;
        }
        saw_done
    }

    fn record_read_error(&self, error: &str) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector
                .terminal_error
                .get_or_insert_with(|| classify_upstream_stream_read_error(error));
        }
    }

    async fn finish_upstream(&mut self) {
        // Cancel network input before draining. The pump finishes parsing only
        // bytes already received, retaining late usage even after client close.
        self.upstream.stop_input().await;
        // A response terminal ends the protocol, even when the provider keeps
        // HTTP open or appends [DONE]/comments. Its usage was already observed;
        // trailing bytes must not turn a completed response into a read error.
        // On an earlier client cancellation, still drain buffered observations.
        if self.finished {
            return;
        }
        loop {
            match self.upstream.recv_async().await {
                Some(UpstreamSseFramePumpItem::Frame(frame)) => {
                    let _ = self.update_usage_from_frame(&frame);
                }
                Some(UpstreamSseFramePumpItem::Error(error)) => {
                    self.record_read_error(&error);
                    return;
                }
                Some(UpstreamSseFramePumpItem::Eof) | None => return,
            }
        }
    }

    fn terminal_error_chunk(&mut self) -> Vec<u8> {
        self.finished = true;
        let message = self.usage_collector.lock().ok().and_then(|collector| {
            (!collector.terminal_delivered)
                .then(|| collector.terminal_error.clone())
                .flatten()
        });
        let Some(message) = message else {
            return Vec::new();
        };
        let code = [
            "upstream_sse_frame_too_large",
            "upstream_sse_buffer_exhausted",
            "upstream_sse_invalid_utf8",
        ]
        .into_iter()
        .find(|code| message.starts_with(&format!("{code}:")))
        .unwrap_or("upstream_error");

        // Codex currently ignores generic Responses `error` events except for a
        // small set of special cases. Normalize an undelivered terminal failure
        // to `response.failed` so the client reports the upstream error instead
        // of replacing it with "stream closed before response.completed".
        let payload = serde_json::json!({
            "type": "response.failed",
            "response": {
                "id": "resp_proxy",
                "object": "response",
                "status": "failed",
                "error": {
                    "type": "server_error",
                    "code": code,
                    "message": message
                }
            }
        });
        // EOF may have flushed a final frame without its empty-line delimiter.
        // Preserve those bytes while keeping our error in its own SSE event.
        let separator = if self.pending_separator { "\n\n" } else { "" };
        let chunk = format!("{separator}event: response.failed\ndata: {payload}\n\n").into_bytes();
        mark_terminal_delivered(&self.usage_collector, &chunk);
        chunk
    }

    async fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        match self
            .upstream
            .recv_timeout_async(stream_wait_timeout(
                self.upstream
                    .last_activity()
                    .unwrap_or(self.request_started_at),
            ))
            .await
        {
            Ok(UpstreamSseFramePumpItem::Frame(frame)) => {
                mark_first_response_ms(&self.usage_collector, self.request_started_at);
                let mut compatible_frame = None;
                let (terminal_delivered, ends_response, mut bytes) =
                    match OpenAIResponsesEvent::parse_with_payload(&frame) {
                        Some((mut event, mut payload)) => {
                            if self.compatibility_events {
                                use crate::gateway::responses_event_compat::{
                                    normalize_compatible_responses_event, CompatibleResponsesEvent,
                                };
                                match normalize_compatible_responses_event(&mut payload) {
                                    CompatibleResponsesEvent::Omit => {
                                        self.update_usage_from_event(event);
                                        return Ok(Vec::new());
                                    }
                                    CompatibleResponsesEvent::Rewrite => {
                                        let kind =
                                            payload["type"].as_str().unwrap_or("response.failed");
                                        event = OpenAIResponsesEvent::parse(&[format!(
                                            "data: {payload}\n"
                                        )])
                                        .unwrap_or(event);
                                        compatible_frame = Some(
                                            format!("event: {kind}\ndata: {payload}\n\n")
                                                .into_bytes(),
                                        );
                                    }
                                    CompatibleResponsesEvent::Preserve => {}
                                }
                            }
                            let successful_completion =
                                matches!(event.terminal, Some(SseTerminal::Ok))
                                    && !event.usage.explicit_failure
                                    && event.upstream_error_hint.is_none();
                            let terminal_delivered = Self::is_delivered_terminal(&event);
                            self.update_usage_from_event(event);
                            let can_repair = successful_completion
                                && self.usage_collector.lock().is_ok_and(|collector| {
                                    collector.terminal_error.is_none()
                                        && collector.upstream_error_hint.is_none()
                                        && !collector.usage.explicit_failure
                                });
                            // Codex commits text from output_item.done, not the
                            // output snapshot inside response.completed. Repair
                            // missing text items only on this delivery path;
                            // cancellation cleanup must never synthesize output.
                            let repaired = self.output_repair.process(&payload, can_repair);
                            (terminal_delivered, terminal_delivered, repaired)
                        }
                        // A bare [DONE] is not a Responses completion. Preserve
                        // the existing incomplete-stream/error diagnostics.
                        None => (Self::is_standalone_done_frame(&frame), false, Vec::new()),
                    };
                // Parsing and delivery share the bounded complete frame. No raw
                // tee may forward a partial oversized JSON/base64 event first.
                self.pending_separator = !frame
                    .last()
                    .is_some_and(|line| line == "\n" || line == "\r" || line == "\r\n");
                // The inserted event is now the stream prefix. Omit the optional
                // upstream BOM on this repaired path: in the middle of the stream
                // it would hide the terminal's data/event field. Ordinary raw
                // streams, including their BOM, remain byte-for-byte unchanged.
                let strip_bom = !bytes.is_empty()
                    && frame
                        .first()
                        .is_some_and(|line| line.starts_with('\u{feff}'));
                bytes.reserve(frame.iter().map(String::len).sum());
                for (index, line) in frame.iter().enumerate() {
                    if compatible_frame.is_some() {
                        break;
                    }
                    let line = if strip_bom && index == 0 {
                        line.strip_prefix('\u{feff}').unwrap_or(line)
                    } else {
                        line
                    };
                    bytes.extend_from_slice(line.as_bytes());
                }
                if let Some(frame) = compatible_frame {
                    bytes.extend_from_slice(&frame);
                    self.pending_separator = false;
                }
                if ends_response {
                    // Codex's SSE decoder needs a complete delimiter at EOF.
                    // The pump emits a CR-terminated frame immediately, before
                    // an optional LF arrives; supply that LF ourselves instead
                    // of waiting on an already completed provider response.
                    if self.pending_separator {
                        bytes.extend_from_slice(b"\n\n");
                    } else if bytes.last() == Some(&b'\r') {
                        bytes.push(b'\n');
                    }
                    self.pending_separator = false;
                }
                if terminal_delivered {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector.terminal_delivered = true;
                    }
                }
                // read_async must drain every byte in this frame (including
                // any repaired output items) before advertising end-of-stream.
                self.finished = ends_response;
                Ok(bytes)
            }
            Ok(UpstreamSseFramePumpItem::Eof) => {
                self.finish_upstream().await;
                if let Ok(mut collector) = self.usage_collector.lock() {
                    if !collector.saw_terminal {
                        let hint = collector.upstream_error_hint.clone();
                        collector.terminal_error.get_or_insert_with(|| {
                            upstream_hint_or_stream_incomplete_message(hint.as_deref())
                        });
                    }
                }
                Ok(self.terminal_error_chunk())
            }
            Ok(UpstreamSseFramePumpItem::Error(err)) => {
                self.record_read_error(&err);
                self.finish_upstream().await;
                Ok(self.terminal_error_chunk())
            }
            Err(RecvTimeoutError::Timeout) => {
                // A large frame may arrive over many chunks without a newline.
                // Byte activity, rather than completed frames, controls idleness.
                if stream_idle_timed_out(
                    self.upstream
                        .last_activity()
                        .unwrap_or(self.request_started_at),
                ) {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(stream_idle_timeout_message);
                    }
                    self.finish_upstream().await;
                    return Ok(self.terminal_error_chunk());
                }
                if crate::gateway::current_sse_keepalive_enabled() {
                    Ok(self.keepalive_frame.bytes().to_vec())
                } else {
                    Ok(Vec::new())
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.finish_upstream().await;
                if let Ok(mut collector) = self.usage_collector.lock() {
                    let hint = collector.upstream_error_hint.clone();
                    collector.terminal_error.get_or_insert_with(|| {
                        hint.unwrap_or_else(stream_reader_disconnected_message)
                    });
                }
                Ok(self.terminal_error_chunk())
            }
        }
    }
}

impl crate::http::gateway_response_body::GatewayResponseBody
    for OpenAIResponsesPassthroughSseReader
{
    fn is_end_stream(&self) -> bool {
        self.finished && self.out_cursor.position() >= self.out_cursor.get_ref().len() as u64
    }

    fn finish_async(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.finish_upstream().await;
        })
    }

    fn read_async<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> crate::http::gateway_response_body::BodyReadFuture<'a> {
        Box::pin(async move {
            loop {
                let read = self.out_cursor.read(buf)?;
                if read > 0 {
                    return Ok(read);
                }
                if self.finished {
                    return Ok(0);
                }
                self.out_cursor = Cursor::new(self.next_chunk().await?);
            }
        })
    }
}

#[cfg(test)]
impl Read for OpenAIResponsesPassthroughSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        crate::gateway::response_test_runtime()?.block_on(
            crate::http::gateway_response_body::GatewayResponseBody::read_async(self, buf),
        )
    }
}

#[cfg(test)]
struct ExactWireKeepaliveGuard<'lock> {
    original_environment: Option<std::ffi::OsString>,
    original_enabled: bool,
    // Keep the original configuration lock until the cache and environment are
    // both restored. The test harness itself retains its default concurrency.
    _lock: &'lock std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl<'lock> ExactWireKeepaliveGuard<'lock> {
    const ENVIRONMENT: &'static str = "CODEXMANAGER_SSE_KEEPALIVE_ENABLED";

    fn enter(lock: &'lock std::sync::MutexGuard<'static, ()>) -> Self {
        // Initialize the cache before taking its snapshot. The setter ignores
        // writes while an environment override exists, so remove that override
        // only within this locked, reversible fixture scope.
        let original_enabled = crate::gateway::current_sse_keepalive_enabled();
        let guard = Self {
            original_environment: std::env::var_os(Self::ENVIRONMENT),
            original_enabled,
            _lock: lock,
        };
        std::env::remove_var(Self::ENVIRONMENT);
        assert!(!crate::gateway::set_sse_keepalive_enabled(false));
        guard
    }
}

#[cfg(test)]
impl Drop for ExactWireKeepaliveGuard<'_> {
    fn drop(&mut self) {
        // Restore the cached value before the environment override can make
        // its setter a no-op. No other runtime setting needs to be reloaded.
        std::env::remove_var(Self::ENVIRONMENT);
        crate::gateway::set_sse_keepalive_enabled(self.original_enabled);
        match &self.original_environment {
            Some(value) => std::env::set_var(Self::ENVIRONMENT, value),
            None => std::env::remove_var(Self::ENVIRONMENT),
        }
    }
}

#[cfg(test)]
fn assert_exact_wire_bytes(actual: &[u8], expected: &[u8], context: &str) {
    // Compare every byte; bounded failure metadata avoids printing a 64 MiB
    // base64 response twice. No prefix, comment, or payload is removed.
    if actual == expected {
        return;
    }
    let offset = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected)
        .unwrap_or(actual.len().min(expected.len()));
    panic!(
        "{context}: wire bytes differ; actual_len={} expected_len={} first_difference={} \
         actual_window={:?} expected_window={:?}",
        actual.len(),
        expected.len(),
        offset,
        &actual[offset..actual.len().min(offset + 16)],
        &expected[offset..expected.len().min(offset + 16)],
    );
}

#[cfg(test)]
mod exact_wire_fixture_tests {
    use super::*;
    use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
    use bytes::Bytes;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::time::Duration;

    struct KeepaliveIntervalGuard<'lock> {
        original_environment: Option<std::ffi::OsString>,
        original_interval: u64,
        _lock: &'lock std::sync::MutexGuard<'static, ()>,
    }

    impl<'lock> KeepaliveIntervalGuard<'lock> {
        const ENVIRONMENT: &'static str = "CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS";

        fn enter(lock: &'lock std::sync::MutexGuard<'static, ()>) -> Self {
            let guard = Self {
                original_environment: std::env::var_os(Self::ENVIRONMENT),
                original_interval: crate::gateway::current_sse_keepalive_interval_ms(),
                _lock: lock,
            };
            std::env::remove_var(Self::ENVIRONMENT);
            assert_eq!(crate::gateway::set_sse_keepalive_interval_ms(1), Ok(1));
            guard
        }
    }

    impl Drop for KeepaliveIntervalGuard<'_> {
        fn drop(&mut self) {
            std::env::remove_var(Self::ENVIRONMENT);
            // The saved getter value is always >= 1, so it remains valid.
            let _ = crate::gateway::set_sse_keepalive_interval_ms(self.original_interval);
            match &self.original_environment {
                Some(value) => std::env::set_var(Self::ENVIRONMENT, value),
                None => std::env::remove_var(Self::ENVIRONMENT),
            }
        }
    }

    #[test]
    fn native_next_chunk_keepalive_toggle_preserves_upstream_comment_and_terminal_bytes() {
        let lock = crate::test_env_guard();
        let original_enabled = crate::gateway::current_sse_keepalive_enabled();
        let original_enabled_environment = std::env::var_os(ExactWireKeepaliveGuard::ENVIRONMENT);
        let original_interval = crate::gateway::current_sse_keepalive_interval_ms();
        let original_interval_environment = std::env::var_os(KeepaliveIntervalGuard::ENVIRONMENT);
        let restore_enabled = ExactWireKeepaliveGuard::enter(&lock);
        let restore_interval = KeepaliveIntervalGuard::enter(&lock);
        crate::gateway::set_sse_keepalive_enabled(true);
        crate::gateway::response_test_runtime().unwrap().block_on(async {
            let (provider, input) = tokio::sync::mpsc::channel(2);
            let usage = Arc::new(Mutex::new(PassthroughSseCollector::default()));
            let mut reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
                GatewayStreamResponse::new(
                    reqwest::StatusCode::OK,
                    reqwest::header::HeaderMap::new(),
                    GatewayByteStream::from_receiver(input),
                ),
                usage.clone(),
                SseKeepAliveFrame::Comment,
                Instant::now(),
            );
            tokio::time::timeout(Duration::from_secs(2), async {
                assert_eq!(reader.next_chunk().await.unwrap(), b": keep-alive\n\n".as_slice());
                {
                    let collected = usage.lock().unwrap();
                    assert!(collected.usage.first_response_ms.is_none());
                    assert!(!collected.saw_terminal);
                    assert!(!collected.terminal_delivered);
                    assert!(collected.terminal_error.is_none());
                }
                crate::gateway::set_sse_keepalive_enabled(false);
                assert!(reader.next_chunk().await.unwrap().is_empty());
                {
                    let collected = usage.lock().unwrap();
                    assert!(collected.usage.first_response_ms.is_none());
                    assert!(!collected.saw_terminal);
                    assert!(!collected.terminal_delivered);
                    assert!(collected.terminal_error.is_none());
                }

                // This identical comment belongs to the provider. Disabling
                // generated heartbeats must preserve it, including every byte.
                let upstream = concat!(
                    ": keep-alive\n\n",
                    "event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_comment_wire\",\"status\":\"completed\",\"usage\":{\"input_tokens\":7}}}\r\n\r\n",
                );
                provider
                    .send(GatewayByteStreamItem::Chunk(Bytes::from_static(upstream.as_bytes())))
                    .await
                    .unwrap();
                provider.send(GatewayByteStreamItem::Eof).await.unwrap();
                let mut actual = Vec::new();
                while !reader.finished {
                    actual.extend_from_slice(&reader.next_chunk().await.unwrap());
                }
                assert_exact_wire_bytes(&actual, upstream.as_bytes(), "provider comment and terminal");
                let collected = usage.lock().unwrap();
                assert!(collected.usage.first_response_ms.is_some());
                assert_eq!(collected.usage.input_tokens, Some(7));
                assert!(collected.saw_terminal);
                assert!(collected.terminal_delivered);
                assert!(collected.terminal_error.is_none());
            }).await.expect("paired keepalive and exact-wire fixture stays bounded");
        });
        drop(restore_interval);
        drop(restore_enabled);
        assert_eq!(
            crate::gateway::current_sse_keepalive_enabled(),
            original_enabled
        );
        assert_eq!(
            crate::gateway::current_sse_keepalive_interval_ms(),
            original_interval
        );
        assert!(
            std::env::var_os(ExactWireKeepaliveGuard::ENVIRONMENT) == original_enabled_environment
        );
        assert!(
            std::env::var_os(KeepaliveIntervalGuard::ENVIRONMENT) == original_interval_environment
        );
    }

    #[test]
    fn exact_wire_guard_restores_cached_and_environment_keepalive_settings() {
        let lock = crate::test_env_guard();
        let original_enabled = crate::gateway::current_sse_keepalive_enabled();
        let original_environment = std::env::var_os(ExactWireKeepaliveGuard::ENVIRONMENT);
        let interval = crate::gateway::current_sse_keepalive_interval_ms();
        let restore_original = ExactWireKeepaliveGuard::enter(&lock);
        for (enabled, environment) in [
            (true, None),
            (true, Some("1")),
            (false, Some("off")),
            (false, Some("")),
        ] {
            std::env::remove_var(ExactWireKeepaliveGuard::ENVIRONMENT);
            crate::gateway::set_sse_keepalive_enabled(enabled);
            if let Some(value) = environment {
                std::env::set_var(ExactWireKeepaliveGuard::ENVIRONMENT, value);
            }
            {
                let _wire = ExactWireKeepaliveGuard::enter(&lock);
                assert!(!crate::gateway::current_sse_keepalive_enabled());
                assert!(std::env::var_os(ExactWireKeepaliveGuard::ENVIRONMENT).is_none());
                assert_eq!(
                    crate::gateway::current_sse_keepalive_interval_ms(),
                    interval
                );
            }
            assert_eq!(crate::gateway::current_sse_keepalive_enabled(), enabled);
            assert!(
                std::env::var_os(ExactWireKeepaliveGuard::ENVIRONMENT)
                    == environment.map(std::ffi::OsString::from)
            );
        }
        drop(restore_original);
        assert_eq!(
            crate::gateway::current_sse_keepalive_enabled(),
            original_enabled
        );
        assert!(std::env::var_os(ExactWireKeepaliveGuard::ENVIRONMENT) == original_environment);
    }

    #[test]
    fn exact_wire_guard_restores_keepalive_after_fixture_panic() {
        let lock = crate::test_env_guard();
        let _restore_original = ExactWireKeepaliveGuard::enter(&lock);
        crate::gateway::set_sse_keepalive_enabled(true);
        std::env::set_var(ExactWireKeepaliveGuard::ENVIRONMENT, "1");
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _wire = ExactWireKeepaliveGuard::enter(&lock);
            assert!(!crate::gateway::current_sse_keepalive_enabled());
            panic!("injected exact-wire fixture panic");
        }));
        assert!(result.is_err());
        assert!(crate::gateway::current_sse_keepalive_enabled());
        assert!(
            std::env::var_os(ExactWireKeepaliveGuard::ENVIRONMENT)
                == Some(std::ffi::OsString::from("1"))
        );
    }

    #[test]
    fn exact_wire_comparison_rejects_comments_changed_bytes_and_truncation() {
        let expected = b"data: complete\n\n";
        assert_exact_wire_bytes(expected, expected, "identical fixture");
        for actual in [
            b": keep-alive\n\ndata: complete\n\n".as_slice(),
            b"data: completE\n\n".as_slice(),
            b"data: complete\n".as_slice(),
        ] {
            assert!(catch_unwind(|| {
                assert_exact_wire_bytes(actual, expected, "injected wire mismatch");
            })
            .is_err());
        }
    }
}

#[cfg(test)]
mod async_finish_tests {
    use super::*;
    use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};
    use crate::http::gateway_response_body::GatewayResponseBody;
    use bytes::Bytes;
    use std::time::Duration;

    #[test]
    fn native_finish_waits_for_late_usage_after_stopping_provider() {
        crate::gateway::response_test_runtime().unwrap().block_on(async {
            let (provider, input) = tokio::sync::mpsc::channel(2);
            let (cancel, cancelled) = tokio::sync::oneshot::channel();
            // More frames than the pump queue can hold: the terminal usage is
            // already received but cannot be parsed/sent until cleanup drains.
            let mut queued = ": queued provider frame\n\n".repeat(384);
            queued.push_str("event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":11,\"output_tokens\":7}}}\n\n");
            provider
                .try_send(GatewayByteStreamItem::Chunk(Bytes::from(queued)))
                .unwrap();
            let usage = Arc::new(Mutex::new(PassthroughSseCollector::default()));
            let mut reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
                GatewayStreamResponse::new(
                    reqwest::StatusCode::OK,
                    reqwest::header::HeaderMap::new(),
                    GatewayByteStream::from_receiver_with_cancel(input, Some(cancel)),
                ),
                usage.clone(),
                SseKeepAliveFrame::Comment,
                Instant::now(),
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
            assert!(usage.lock().unwrap().usage.input_tokens.is_none());
            tokio::time::timeout(Duration::from_secs(2), reader.finish_async())
                .await
                .expect("queued frames drain despite a full frame queue");
            tokio::time::timeout(Duration::from_secs(1), cancelled)
                .await
                .expect("cleanup must stop provider input")
                .expect("provider cancellation sender is signalled");
            assert!(provider.is_closed(), "cleanup closes provider byte input");
            let collected = usage.lock().unwrap();
            assert_eq!(collected.usage.input_tokens, Some(11));
            assert_eq!(collected.usage.output_tokens, Some(7));
            assert!(collected.saw_terminal);
            assert!(!collected.terminal_delivered, "cleanup never delivers queued frames");
        });
    }

    #[test]
    fn native_split_utf8_and_crlf_are_preserved_by_the_shared_pump() {
        use eventsource_stream::Eventsource;
        use futures_util::StreamExt;

        let lock = crate::test_env_guard();
        let _wire = ExactWireKeepaliveGuard::enter(&lock);
        crate::gateway::response_test_runtime().unwrap().block_on(async {
            let upstream = concat!(
                "event: response.output_text.delta\r\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"你好\"}\r\n\r\n",
                "event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_split\",\"status\":\"completed\",\"output_text\":\"你好\",\"usage\":{\"input_tokens\":3}}}\r\n\r\n",
            );
            let (provider, input) = tokio::sync::mpsc::channel(1);
            let producer = tokio::spawn(async move {
                for &byte in upstream.as_bytes() {
                    provider
                        .send(GatewayByteStreamItem::Chunk(Bytes::from(vec![byte])))
                        .await
                        .unwrap();
                }
                provider.send(GatewayByteStreamItem::Eof).await.unwrap();
            });
            let usage = Arc::new(Mutex::new(PassthroughSseCollector::default()));
            let mut reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
                GatewayStreamResponse::new(
                    reqwest::StatusCode::OK,
                    reqwest::header::HeaderMap::new(),
                    GatewayByteStream::from_receiver(input),
                ),
                usage.clone(),
                SseKeepAliveFrame::Comment,
                Instant::now(),
            );
            let mut body = Vec::new();
            let mut buffer = [0_u8; 7];
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let count = reader.read_async(&mut buffer).await.unwrap();
                    if count == 0 { break; }
                    body.extend_from_slice(&buffer[..count]);
                }
            }).await.expect("split UTF-8/CRLF frame must complete");
            producer.await.unwrap();
            // Complete the final CRLF locally without waiting for the optional
            // upstream LF. Earlier CRLF/UTF-8 bytes remain exact as well.
            assert_eq!(body, upstream.as_bytes());
            let chunks = body.into_iter().map(|byte| Ok::<_, std::io::Error>(Bytes::from(vec![byte])));
            let events = futures_util::stream::iter(chunks).eventsource().collect::<Vec<_>>().await;
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].as_ref().unwrap().event, "response.output_text.delta");
            assert_eq!(events[1].as_ref().unwrap().event, "response.completed");
            let collected = usage.lock().unwrap();
            assert_eq!(collected.usage.input_tokens, Some(3));
            assert_eq!(collected.usage.output_text.as_deref(), Some("你好"));
            assert!(collected.terminal_error.is_none());
        });
    }

    #[test]
    fn native_cr_only_terminal_is_delivered_before_provider_closes() {
        let lock = crate::test_env_guard();
        let _wire = ExactWireKeepaliveGuard::enter(&lock);
        crate::gateway::response_test_runtime().unwrap().block_on(async {
            let upstream = "event: response.completed\rdata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_live_cr\",\"status\":\"completed\",\"usage\":{\"input_tokens\":9}}}\r\r";
            let (provider, input) = tokio::sync::mpsc::channel(2);
            provider.try_send(GatewayByteStreamItem::Chunk(Bytes::copy_from_slice(upstream.as_bytes()))).unwrap();
            let usage = Arc::new(Mutex::new(PassthroughSseCollector::default()));
            let mut reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
                GatewayStreamResponse::new(
                    reqwest::StatusCode::OK,
                    reqwest::header::HeaderMap::new(),
                    GatewayByteStream::from_receiver(input),
                ),
                usage.clone(),
                SseKeepAliveFrame::Comment,
                Instant::now(),
            );
            let bytes = tokio::time::timeout(Duration::from_secs(2), reader.next_chunk())
                .await.expect("CR-only terminal cannot wait for provider close").unwrap();
            assert_eq!(bytes, format!("{upstream}\n").as_bytes());
            assert!(!provider.is_closed());
            {
                let collected = usage.lock().unwrap();
                assert_eq!(collected.usage.input_tokens, Some(9));
                assert!(collected.saw_terminal);
                assert!(collected.terminal_delivered);
                assert!(collected.terminal_error.is_none());
            }
            provider.send(GatewayByteStreamItem::Eof).await.unwrap();
            assert!(reader.next_chunk().await.unwrap().is_empty());
        });
    }
}

#[cfg(test)]
mod terminal_error_tests {
    use super::*;
    use crate::gateway::upstream::GatewayByteStream;
    use bytes::Bytes;

    fn read_fixture(sse: &[u8]) -> (String, PassthroughSseCollector) {
        read_fixture_with_compatibility(sse, false)
    }

    fn read_fixture_with_compatibility(
        sse: &[u8],
        compatible: bool,
    ) -> (String, PassthroughSseCollector) {
        let lock = crate::test_env_guard();
        let _wire = ExactWireKeepaliveGuard::enter(&lock);
        let usage = Arc::new(Mutex::new(PassthroughSseCollector::default()));
        let response = GatewayStreamResponse::new(
            reqwest::StatusCode::OK,
            reqwest::header::HeaderMap::new(),
            GatewayByteStream::from_bytes(Bytes::copy_from_slice(sse)),
        );
        let mut reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
            response,
            usage.clone(),
            SseKeepAliveFrame::Comment,
            Instant::now(),
        )
        .with_compatibility_events(compatible);
        let mut body = String::new();
        reader.read_to_string(&mut body).unwrap();
        let collected = usage.lock().unwrap().clone();
        (body, collected)
    }

    #[test]
    fn compatible_native_responses_filters_metadata_and_normalizes_done_for_sdk() {
        let message = serde_json::json!({"id":"msg_sdk_compat","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"compatibility ok","annotations":[]}]});
        let response = serde_json::json!({"id":"resp_sdk_compat","object":"response","created_at":1,"model":"test","status":"completed","output":[message],"error":null,"incomplete_details":null,"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}});
        let mut created = response.clone();
        created["status"] = serde_json::json!("in_progress");
        created["output"] = serde_json::json!([]);
        let events = [
            serde_json::json!({"type":"response.created","sequence_number":0,"response":created}),
            serde_json::json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"id":"msg_sdk_compat","type":"message","role":"assistant","status":"in_progress","content":[]}}),
            serde_json::json!({"type":"response.content_part.added","sequence_number":2,"item_id":"msg_sdk_compat","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
            serde_json::json!({"type":"response.output_text.delta","sequence_number":3,"item_id":"msg_sdk_compat","output_index":0,"content_index":0,"delta":"compatibility ok"}),
            serde_json::json!({"type":"response.metadata","sequence_number":4,"metadata":{"fixture":true}}),
            serde_json::json!({"type":"response.done","sequence_number":5,"response":response}),
        ];
        let wire = events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().unwrap()
                )
            })
            .collect::<String>();
        let (body, collector) = read_fixture_with_compatibility(wire.as_bytes(), true);
        assert!(!body.contains("response.metadata"));
        assert!(!body.contains("response.done"));
        assert!(body.contains("event: response.completed\n"));
        assert!(body.contains("compatibility ok"));
        assert!(collector.saw_terminal && collector.terminal_error.is_none());
        assert_eq!(collector.usage.input_tokens, Some(3));
        if let Some(directory) = std::env::var_os("CM_REPAIR_FIXTURE_DIR") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                std::path::Path::new(&directory).join("responses-sdk-compat-http.sse"),
                &body,
            )
            .unwrap();
        }
        let (native, _) = read_fixture(wire.as_bytes());
        assert!(native.contains("response.metadata"));
        assert!(native.contains("response.done"));
    }

    #[test]
    fn compatible_native_done_failure_stays_a_semantic_failure() {
        let wire = b"event: response.done\ndata: {\"type\":\"response.done\",\"response\":{\"id\":\"resp_failed\",\"status\":\"failed\",\"output\":[],\"error\":{\"code\":\"quota_exceeded\",\"message\":\"quota exhausted\"}}}\n\n";
        let (body, collector) = read_fixture_with_compatibility(wire, true);
        assert!(body.contains("event: response.failed\n"));
        assert!(!body.contains("response.completed"));
        assert!(body.contains("quota_exceeded"));
        assert!(collector.usage.explicit_failure);
        assert!(collector.terminal_error.is_some());
    }

    #[test]
    fn compatible_metadata_usage_is_observed_before_omitting_wire_event() {
        let wire = b"data: {\"type\":\"response.metadata\",\"usage\":{\"input_tokens\":9,\"output_tokens\":4}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_usage\",\"status\":\"completed\",\"output\":[]}}\n\n";
        let (body, collector) = read_fixture_with_compatibility(wire, true);
        assert!(!body.contains("response.metadata"));
        assert_eq!(collector.usage.input_tokens, Some(9));
        assert_eq!(collector.usage.output_tokens, Some(4));
    }

    #[test]
    fn native_terminal_delimiter_reaches_the_client_decoder_before_eof() {
        use eventsource_stream::Eventsource;
        use futures_util::StreamExt;

        let terminal = "event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_eof\",\"status\":\"completed\",\"output\":[]}}";
        for ending in ["", "\n", "\r", "\r\n", "\r\r", "\n\n"] {
            let (body, collected) = read_fixture(format!("{terminal}{ending}").as_bytes());
            let chunks = body
                .into_bytes()
                .into_iter()
                .map(|byte| Ok::<_, std::io::Error>(Bytes::from(vec![byte])));
            let events = crate::gateway::response_test_runtime().unwrap().block_on(
                futures_util::stream::iter(chunks)
                    .eventsource()
                    .collect::<Vec<_>>(),
            );
            assert_eq!(events.len(), 1, "terminal ending {ending:?}");
            let event = events[0].as_ref().unwrap();
            assert_eq!(event.event, "response.completed");
            let value: serde_json::Value = serde_json::from_str(&event.data).unwrap();
            assert_eq!(value["response"]["id"], "resp_eof");
            assert!(collected.saw_terminal);
            assert!(collected.terminal_error.is_none());
        }
    }

    #[test]
    fn native_generic_error_is_normalized_to_response_failed() {
        let upstream = b"event: error\ndata: {\"type\":\"error\",\"code\":\"internal_error\",\"message\":\"response protection is unavailable\",\"param\":null}\n\n";
        let (body, collected) = read_fixture(upstream);

        assert!(body.starts_with(std::str::from_utf8(upstream).unwrap()));
        assert!(body.contains("event: response.failed\n"));
        assert!(body.contains("\"type\":\"response.failed\""));
        assert!(body.contains("response protection is unavailable"));
        assert!(!body.contains("response.completed"));
        assert!(collected.saw_terminal);
        assert!(collected.terminal_delivered);
        assert_eq!(collected.last_event_type.as_deref(), Some("error"));
        assert!(collected
            .terminal_error
            .as_deref()
            .is_some_and(|message| message.contains("response protection is unavailable")));
    }

    #[test]
    fn native_long_terminal_frames_are_not_duplicated() {
        for terminal in [
            "response.failed",
            "response.incomplete",
            "response.completed",
        ] {
            let response =
                serde_json::json!({"error": {"message": "upstream failure ".repeat(100)}});
            let payload = format!(r#"{{"type":"{terminal}","response":{response}}}"#);
            let upstream = format!("event: {terminal}\ndata: {payload}\n\n");
            let (body, collected) = read_fixture(upstream.as_bytes());
            assert_eq!(
                body, upstream,
                "terminal event {terminal} must pass through unchanged"
            );
            assert!(collected.terminal_delivered);
        }
    }

    #[test]
    fn native_response_failed_is_not_duplicated() {
        let upstream = b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"upstream failure\"}}}\n\n";
        let (body, collected) = read_fixture(upstream);

        assert_eq!(body.as_bytes(), upstream);
        assert_eq!(body.matches("response.failed").count(), 2);
        assert!(collected.saw_terminal);
        assert!(collected.terminal_delivered);
        assert_eq!(
            collected.terminal_error.as_deref(),
            Some("code=server_error upstream failure")
        );
    }

    #[test]
    fn native_large_image_base64_frame_preserves_every_byte() {
        // The bridge treats image results as opaque base64. Exercise a 64 MiB
        // syntactically valid base64 response rather than a large request body.
        let image_b64 = "QUFB".repeat(16 * 1024 * 1024);
        let upstream = format!(
            "event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp_large_image\",\"status\":\"completed\",\"output\":[{{\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"{image_b64}\",\"output_format\":\"png\"}}],\"usage\":{{\"input_tokens\":5,\"output_tokens\":2}}}}}}\n\n"
        );
        drop(image_b64);
        let (body, collected) = read_fixture(upstream.as_bytes());
        assert_exact_wire_bytes(
            body.as_bytes(),
            upstream.as_bytes(),
            "large image JSON/base64 must not be truncated",
        );
        assert_eq!(
            collected.usage.response_id.as_deref(),
            Some("resp_large_image")
        );
        assert_eq!(collected.usage.input_tokens, Some(5));
        assert_eq!(collected.usage.output_tokens, Some(2));
        assert!(collected.saw_terminal);
        assert!(collected.terminal_delivered);
        assert!(collected.terminal_error.is_none());
    }

    #[test]
    fn native_budget_error_is_a_protocol_failure_without_partial_payload() {
        use crate::gateway::upstream::GatewayByteStreamItem;
        let lock = crate::test_env_guard();
        let _wire = ExactWireKeepaliveGuard::enter(&lock);
        let (provider, input) = tokio::sync::mpsc::channel(1);
        provider
            .try_send(GatewayByteStreamItem::Error(
                "upstream_sse_frame_too_large: SSE frame exceeds 128 bytes".to_owned(),
            ))
            .unwrap();
        let usage = Arc::new(Mutex::new(PassthroughSseCollector::default()));
        let mut reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
            GatewayStreamResponse::new(
                reqwest::StatusCode::OK,
                reqwest::header::HeaderMap::new(),
                GatewayByteStream::from_receiver(input),
            ),
            usage.clone(),
            SseKeepAliveFrame::Comment,
            Instant::now(),
        );
        let mut body = String::new();
        reader.read_to_string(&mut body).unwrap();
        assert!(body.starts_with("event: response.failed\n"));
        assert!(body.contains("\"code\":\"upstream_sse_frame_too_large\""));
        assert!(!body.contains("response.completed"));
        assert_eq!(body.matches("event: response.failed").count(), 1);
        assert!(usage.lock().unwrap().terminal_error.is_some());
    }

    fn read_prefix_then_frame_budget_error(prefix: &str) -> (String, PassthroughSseCollector) {
        use super::super::common::SseFrameLimits;
        use tokio::sync::Semaphore;

        let lock = crate::test_env_guard();
        let _wire = ExactWireKeepaliveGuard::enter(&lock);
        let upstream = format!("{prefix}data: {}", "x".repeat(257));
        let budget = Arc::new(Semaphore::new(8192));
        let pump = UpstreamSseFramePump::from_stream_with_limits(
            GatewayByteStream::from_bytes(Bytes::from(upstream)),
            SseFrameLimits {
                frame_bytes: 256,
                queued_bytes: 512,
            },
            budget.clone(),
        );
        let usage = Arc::new(Mutex::new(PassthroughSseCollector::default()));
        let mut reader = OpenAIResponsesPassthroughSseReader {
            upstream: pump,
            out_cursor: Cursor::new(Vec::new()),
            usage_collector: usage.clone(),
            usage_text_state: OpenAIResponsesOutputTextState::default(),
            output_repair: ResponsesOutputRepair::default(),
            keepalive_frame: SseKeepAliveFrame::Comment,
            request_started_at: Instant::now(),
            pending_separator: false,
            finished: false,
            compatibility_events: false,
        };
        let mut body = String::new();
        reader.read_to_string(&mut body).unwrap();
        assert_eq!(budget.available_permits(), 8192);
        let collected = usage.lock().unwrap().clone();
        (body, collected)
    }

    fn assert_frame_budget_failure_after_prefix(prefix: &str) {
        let (body, collected) = read_prefix_then_frame_budget_error(prefix);
        assert!(body.starts_with(prefix));
        assert_eq!(
            body.matches("event: response.failed\n").count(),
            1,
            "nonterminal prefix {prefix:?} must not suppress the real pump error"
        );
        assert!(body.contains("\"code\":\"upstream_sse_frame_too_large\""));
        assert!(!body[prefix.len()..].contains("event: response.completed\n"));
        assert!(collected.terminal_error.is_some());
    }

    #[test]
    fn native_empty_terminal_header_does_not_hide_a_later_frame_budget_error() {
        assert_frame_budget_failure_after_prefix("event: response.completed\n\n");
    }

    #[test]
    fn native_invalid_json_terminal_header_does_not_hide_a_later_frame_budget_error() {
        assert_frame_budget_failure_after_prefix("event: response.completed\ndata: {broken}\n\n");
    }

    #[test]
    fn native_done_mixed_with_other_data_does_not_hide_a_later_frame_budget_error() {
        for prefix in [
            "data: [DONE]\ndata: other\n\n",
            "data: other\ndata: [DONE]\n\n",
            "data: [DONE]\ndata: [DONE]\n\n",
            "data: [done]\n\n",
        ] {
            assert_frame_budget_failure_after_prefix(prefix);
        }
    }

    #[test]
    fn native_standalone_done_and_bom_done_do_not_add_a_later_budget_failure() {
        for prefix in [
            "data: [DONE]\n\n",
            "\u{feff}data: [DONE]\r\n\r\n",
            "event: message\ndata:\ndata: [DONE]\ndata: \n\n",
        ] {
            let (body, collected) = read_prefix_then_frame_budget_error(prefix);
            assert_eq!(body, prefix);
            assert!(collected.terminal_delivered);
        }
    }

    #[test]
    fn native_delta_terminal_words_do_not_hide_a_later_frame_budget_error() {
        for marker in [
            "response.completed",
            "response.failed",
            "response.incomplete",
            "response.done",
            "data: [DONE]",
        ] {
            let delta = serde_json::json!({
                "type": "response.output_text.delta",
                "delta": format!("Explain the literal text {marker}"),
                "metadata": {"type": "response.completed"},
            });
            // Both data-only frames and explicitly typed events use the same
            // top-level JSON semantics. Neither nested types nor an inconsistent
            // event header can turn this delta into a completed response.
            for prefix in [
                format!("event: response.output_text.delta\ndata: {delta}\n\n"),
                format!("data: {delta}\n\n"),
                format!("event: response.completed\ndata: {delta}\n\n"),
            ] {
                assert_frame_budget_failure_after_prefix(&prefix);
            }
        }
    }

    #[test]
    fn native_crlf_comments_and_multiline_data_keep_the_original_bytes() {
        let upstream = concat!(
            ": provider comment\r\n\r\n",
            "id: event_1\r\nevent: response.completed\r\n",
            "data: {\"type\":\"response.completed\",\r\n",
            "data: \"response\":{\"id\":\"resp_crlf\",\"status\":\"completed\",\"output_text\":\"你好\",\"usage\":{\"input_tokens\":11}}}\r\n\r\n",
        );
        let (body, collected) = read_fixture(upstream.as_bytes());
        assert_eq!(body, upstream);
        assert_eq!(collected.usage.response_id.as_deref(), Some("resp_crlf"));
        assert_eq!(collected.usage.input_tokens, Some(11));
        assert_eq!(collected.usage.output_text.as_deref(), Some("你好"));
        assert!(collected.terminal_error.is_none());
    }

    #[test]
    fn native_bom_data_only_completed_frame_keeps_raw_bytes_and_usage() {
        let upstream = "\u{feff}data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_bom\",\"status\":\"completed\",\"usage\":{\"input_tokens\":13,\"output_tokens\":4}}}\r\n\r\n";
        let (body, collected) = read_fixture(upstream.as_bytes());
        assert_eq!(body, upstream, "BOM remains in raw passthrough bytes");
        assert_eq!(collected.usage.response_id.as_deref(), Some("resp_bom"));
        assert_eq!(collected.usage.input_tokens, Some(13));
        assert_eq!(collected.usage.output_tokens, Some(4));
        assert!(collected.saw_terminal);
        assert!(collected.terminal_delivered);
        assert!(collected.terminal_error.is_none());
    }

    #[test]
    fn native_unterminated_generic_error_keeps_its_normalized_failure_separate() {
        let upstream = b"event: error\ndata: {\"type\":\"error\",\"message\":\"upstream failed\"}";
        let (body, _) = read_fixture(upstream);
        assert!(body.starts_with(std::str::from_utf8(upstream).unwrap()));
        assert!(body.contains("}\n\nevent: response.failed\n"));
        assert_eq!(body.matches("event: response.failed").count(), 1);
    }
}
