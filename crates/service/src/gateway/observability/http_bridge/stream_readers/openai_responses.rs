use super::{
    classify_upstream_stream_read_error, mark_first_response_ms, mark_terminal_delivered,
    stream_idle_timed_out, stream_idle_timeout_message, stream_reader_disconnected_message,
    stream_wait_timeout, upstream_hint_or_stream_incomplete_message, Arc, Cursor, Mutex,
    OpenAIResponsesEvent, OpenAIResponsesOutputTextState, PassthroughSseCollector, Read,
    SseKeepAliveFrame, SseTerminal,
};
use crate::gateway::upstream::attempt_flow::transport::runtime::upstream_runtime;
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse};
use eventsource_stream::{Event, Eventsource};
use futures_util::pin_mut;
use futures_util::stream::unfold;
use futures_util::StreamExt;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Instant;
use tokio::sync::mpsc::Receiver;

const OPENAI_RESPONSES_SSE_CHANNEL_CAPACITY: usize = 128;

#[derive(Debug)]
enum OpenAIResponsesSidecarItem {
    Event(OpenAIResponsesEvent),
    Eof,
    Error(String),
}

struct OpenAIResponsesSidecarObserver {
    rx: Receiver<OpenAIResponsesSidecarItem>,
}

impl OpenAIResponsesSidecarObserver {
    fn new(byte_stream: GatewayByteStream) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(OPENAI_RESPONSES_SSE_CHANNEL_CAPACITY);
        if let Ok(runtime) = upstream_runtime() {
            runtime.spawn(async move {
                let byte_stream = unfold(Some(byte_stream), |state| async move {
                    let mut byte_stream = state?;
                    match byte_stream.recv_async().await {
                        Some(GatewayByteStreamItem::Chunk(bytes)) => {
                            Some((Ok(bytes), Some(byte_stream)))
                        }
                        Some(GatewayByteStreamItem::Error(err)) => Some((Err(err), None)),
                        Some(GatewayByteStreamItem::Eof) | None => None,
                    }
                });
                let stream = byte_stream.eventsource();
                pin_mut!(stream);
                loop {
                    let event = tokio::select! {
                        _ = tx.closed() => return,
                        event = stream.next() => event,
                    };
                    let (item, terminal) = match event {
                        Some(Ok(event)) => {
                            let Some(parsed) =
                                OpenAIResponsesEvent::parse(&event_to_sse_lines(&event))
                            else {
                                continue;
                            };
                            (OpenAIResponsesSidecarItem::Event(parsed), false)
                        }
                        Some(Err(err)) => {
                            (OpenAIResponsesSidecarItem::Error(err.to_string()), true)
                        }
                        None => (OpenAIResponsesSidecarItem::Eof, true),
                    };
                    if tx.send(item).await.is_err() || terminal {
                        return;
                    }
                }
            });
        }
        Self { rx }
    }

    fn try_recv(&mut self) -> Result<OpenAIResponsesSidecarItem, mpsc::TryRecvError> {
        self.rx.try_recv().map_err(|err| match err {
            tokio::sync::mpsc::error::TryRecvError::Empty => mpsc::TryRecvError::Empty,
            tokio::sync::mpsc::error::TryRecvError::Disconnected => {
                mpsc::TryRecvError::Disconnected
            }
        })
    }

    async fn recv(&mut self) -> Option<OpenAIResponsesSidecarItem> {
        self.rx.recv().await
    }
}

fn event_to_sse_lines(event: &Event) -> Vec<String> {
    let mut lines = Vec::new();
    if !event.id.is_empty() {
        lines.push(format!("id: {}\n", event.id));
    }
    if let Some(retry) = event.retry {
        lines.push(format!("retry: {}\n", retry.as_millis()));
    }
    if !event.event.is_empty() && !event.event.eq_ignore_ascii_case("message") {
        lines.push(format!("event: {}\n", event.event));
    }
    for data_line in event.data.split('\n') {
        lines.push(format!("data: {data_line}\n"));
    }
    lines.push("\n".to_string());
    lines
}

pub(crate) struct OpenAIResponsesPassthroughSseReader {
    raw_upstream: GatewayByteStream,
    observer: OpenAIResponsesSidecarObserver,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    usage_text_state: OpenAIResponsesOutputTextState,
    keepalive_frame: SseKeepAliveFrame,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    finished: bool,
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
        let (raw_upstream, sidecar_upstream) = upstream.into_body().tee();
        Self {
            raw_upstream,
            observer: OpenAIResponsesSidecarObserver::new(sidecar_upstream),
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            usage_text_state: OpenAIResponsesOutputTextState::default(),
            keepalive_frame,
            request_started_at,
            last_upstream_activity: Instant::now(),
            finished: false,
        }
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

    fn drain_sidecar_events(&mut self) {
        loop {
            match self.observer.try_recv() {
                Ok(OpenAIResponsesSidecarItem::Event(event)) => {
                    self.update_usage_from_event(event);
                }
                Ok(OpenAIResponsesSidecarItem::Eof) => return,
                Ok(OpenAIResponsesSidecarItem::Error(err)) => {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(|| classify_upstream_stream_read_error(&err));
                    }
                    return;
                }
                Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }
    }

    async fn finish_sidecar(&mut self) {
        // Stop network input first. The tee then flushes its bounded observer
        // queue and closes it, so EOF confirms all received usage was parsed.
        self.raw_upstream.close();
        loop {
            match self.observer.recv().await {
                Some(OpenAIResponsesSidecarItem::Event(event)) => {
                    self.update_usage_from_event(event);
                }
                Some(OpenAIResponsesSidecarItem::Eof) => return,
                Some(OpenAIResponsesSidecarItem::Error(err)) => {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(|| classify_upstream_stream_read_error(&err));
                    }
                    return;
                }
                None => return,
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
                    "code": "upstream_error",
                    "message": message
                }
            }
        });
        let chunk = format!("event: response.failed\ndata: {payload}\n\n").into_bytes();
        mark_terminal_delivered(&self.usage_collector, &chunk);
        chunk
    }

    async fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        self.drain_sidecar_events();
        match self
            .raw_upstream
            .recv_timeout_async(stream_wait_timeout(self.last_upstream_activity))
            .await
        {
            Ok(GatewayByteStreamItem::Chunk(bytes)) => {
                self.last_upstream_activity = Instant::now();
                mark_first_response_ms(&self.usage_collector, self.request_started_at);
                self.drain_sidecar_events();
                let bytes = bytes.to_vec();
                mark_terminal_delivered(&self.usage_collector, &bytes);
                Ok(bytes)
            }
            Ok(GatewayByteStreamItem::Eof) => {
                self.finish_sidecar().await;
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
            Ok(GatewayByteStreamItem::Error(err)) => {
                self.last_upstream_activity = Instant::now();
                self.finish_sidecar().await;
                if let Ok(mut collector) = self.usage_collector.lock() {
                    collector
                        .terminal_error
                        .get_or_insert_with(|| classify_upstream_stream_read_error(&err));
                }
                Ok(self.terminal_error_chunk())
            }
            Err(RecvTimeoutError::Timeout) => {
                self.drain_sidecar_events();
                if stream_idle_timed_out(self.last_upstream_activity) {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(stream_idle_timeout_message);
                    }
                    return Ok(self.terminal_error_chunk());
                }
                if crate::gateway::current_sse_keepalive_enabled() {
                    Ok(self.keepalive_frame.bytes().to_vec())
                } else {
                    Ok(Vec::new())
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.finish_sidecar().await;
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
    fn finish_async(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.finish_sidecar().await;
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
mod async_finish_tests {
    use super::*;
    use crate::http::gateway_response_body::GatewayResponseBody;
    use std::time::Duration;

    #[test]
    fn native_finish_waits_for_late_usage_after_stopping_provider() {
        crate::gateway::response_test_runtime().unwrap().block_on(async {
            let (_provider, input) = tokio::sync::mpsc::channel(2);
            let (cancel, mut cancelled) = tokio::sync::oneshot::channel();
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
            // Model a queued frame whose observer parsing completes after the
            // former 50 ms deadline. Cleanup must wait for its explicit EOF.
            let (events, receiver) = tokio::sync::mpsc::channel(2);
            reader.observer = OpenAIResponsesSidecarObserver { rx: receiver };
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(120)).await;
                let event = OpenAIResponsesEvent::parse(&[
                    "event: response.completed\n".to_owned(),
                    "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":11,\"output_tokens\":7}}}\n".to_owned(),
                    "\n".to_owned(),
                ]).unwrap();
                events.send(OpenAIResponsesSidecarItem::Event(event)).await.unwrap();
                events.send(OpenAIResponsesSidecarItem::Eof).await.unwrap();
            });
            let mut finishing = Box::pin(reader.finish_async());
            tokio::select! {
                result = &mut cancelled => result.unwrap(),
                _ = &mut finishing => panic!("observer cleanup finished before provider cancellation"),
            }
            tokio::time::timeout(Duration::from_secs(2), finishing)
                .await.expect("queued observer events drain to EOF");
            let collected = usage.lock().unwrap();
            assert_eq!(collected.usage.input_tokens, Some(11));
            assert_eq!(collected.usage.output_tokens, Some(7));
        });
    }
}

#[cfg(test)]
mod terminal_error_tests {
    use super::*;
    use bytes::Bytes;

    fn read_fixture(sse: &[u8]) -> (String, PassthroughSseCollector) {
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
        );
        let mut body = String::new();
        reader.read_to_string(&mut body).unwrap();
        let collected = usage.lock().unwrap().clone();
        (body, collected)
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
}
