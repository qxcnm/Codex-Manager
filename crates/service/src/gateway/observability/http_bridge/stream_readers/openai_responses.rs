use super::{
    classify_upstream_stream_read_error, mark_first_response_ms, stream_idle_timed_out,
    stream_idle_timeout_message, stream_reader_disconnected_message, stream_wait_timeout,
    upstream_hint_or_stream_incomplete_message, Arc, CanonicalEvent, Cursor, Mutex,
    OpenAIResponsesOutputTextState, PassthroughSseCollector, Read, SseKeepAliveFrame, SseTerminal,
};
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse};
use eventsource_stream::{Event, Eventsource};
use futures_util::pin_mut;
use futures_util::stream::unfold;
use futures_util::task::noop_waker_ref;
use futures_util::Stream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};

const OPENAI_RESPONSES_SSE_CHANNEL_CAPACITY: usize = 128;
const OPENAI_RESPONSES_SIDECAR_DRAIN_TIMEOUT: Duration = Duration::from_millis(50);

#[derive(Debug)]
enum OpenAIResponsesSidecarItem {
    Event(CanonicalEvent),
    Eof,
    Error(String),
}

struct OpenAIResponsesSidecarObserver {
    rx: Receiver<OpenAIResponsesSidecarItem>,
}

impl OpenAIResponsesSidecarObserver {
    fn new(byte_stream: GatewayByteStream) -> Self {
        let (tx, rx) =
            mpsc::sync_channel::<OpenAIResponsesSidecarItem>(OPENAI_RESPONSES_SSE_CHANNEL_CAPACITY);
        thread::spawn(move || {
            let byte_stream = unfold(Some(byte_stream), |state| async move {
                let byte_stream = state?;
                match byte_stream.recv() {
                    Ok(GatewayByteStreamItem::Chunk(bytes)) => Some((Ok(bytes), Some(byte_stream))),
                    Ok(GatewayByteStreamItem::Eof) => None,
                    Ok(GatewayByteStreamItem::Error(err)) => Some((Err(err), None)),
                    Err(_) => None,
                }
            });

            let stream = byte_stream.eventsource();
            pin_mut!(stream);
            let waker = noop_waker_ref();
            let mut cx = Context::from_waker(waker);

            loop {
                match stream.as_mut().poll_next(&mut cx) {
                    Poll::Ready(Some(Ok(event))) => {
                        let lines = event_to_sse_lines(&event);
                        if let Some(parsed) = CanonicalEvent::parse(&lines) {
                            if tx.send(OpenAIResponsesSidecarItem::Event(parsed)).is_err() {
                                return;
                            }
                        }
                    }
                    Poll::Ready(Some(Err(err))) => {
                        let _ = tx.send(OpenAIResponsesSidecarItem::Error(err.to_string()));
                        return;
                    }
                    Poll::Ready(None) => {
                        let _ = tx.send(OpenAIResponsesSidecarItem::Eof);
                        return;
                    }
                    Poll::Pending => thread::yield_now(),
                }
            }
        });
        Self { rx }
    }

    fn try_recv(&self) -> Result<OpenAIResponsesSidecarItem, mpsc::TryRecvError> {
        self.rx.try_recv()
    }

    fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<OpenAIResponsesSidecarItem, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
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

    fn update_usage_from_event(&mut self, event: CanonicalEvent) {
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

    fn drain_sidecar_with_deadline(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            self.drain_sidecar_events();
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            match self
                .observer
                .recv_timeout(deadline.saturating_duration_since(now))
            {
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
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        self.drain_sidecar_events();
        match self
            .raw_upstream
            .recv_timeout(stream_wait_timeout(self.last_upstream_activity))
        {
            Ok(GatewayByteStreamItem::Chunk(bytes)) => {
                self.last_upstream_activity = Instant::now();
                mark_first_response_ms(&self.usage_collector, self.request_started_at);
                self.drain_sidecar_events();
                Ok(bytes.to_vec())
            }
            Ok(GatewayByteStreamItem::Eof) => {
                self.drain_sidecar_with_deadline(OPENAI_RESPONSES_SIDECAR_DRAIN_TIMEOUT);
                if let Ok(mut collector) = self.usage_collector.lock() {
                    if !collector.saw_terminal {
                        let hint = collector.upstream_error_hint.clone();
                        collector.terminal_error.get_or_insert_with(|| {
                            upstream_hint_or_stream_incomplete_message(hint.as_deref())
                        });
                    }
                }
                self.finished = true;
                Ok(Vec::new())
            }
            Ok(GatewayByteStreamItem::Error(err)) => {
                self.last_upstream_activity = Instant::now();
                self.drain_sidecar_with_deadline(OPENAI_RESPONSES_SIDECAR_DRAIN_TIMEOUT);
                if let Ok(mut collector) = self.usage_collector.lock() {
                    collector
                        .terminal_error
                        .get_or_insert_with(|| classify_upstream_stream_read_error(&err));
                }
                self.finished = true;
                Ok(Vec::new())
            }
            Err(RecvTimeoutError::Timeout) => {
                self.drain_sidecar_events();
                if stream_idle_timed_out(self.last_upstream_activity) {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(stream_idle_timeout_message);
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
                Ok(self.keepalive_frame.bytes().to_vec())
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.drain_sidecar_with_deadline(OPENAI_RESPONSES_SIDECAR_DRAIN_TIMEOUT);
                if let Ok(mut collector) = self.usage_collector.lock() {
                    let hint = collector.upstream_error_hint.clone();
                    collector.terminal_error.get_or_insert_with(|| {
                        hint.unwrap_or_else(stream_reader_disconnected_message)
                    });
                }
                self.finished = true;
                Ok(Vec::new())
            }
        }
    }
}

impl Read for OpenAIResponsesPassthroughSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let read = self.out_cursor.read(buf)?;
            if read > 0 {
                return Ok(read);
            }
            if self.finished {
                return Ok(0);
            }
            self.out_cursor = Cursor::new(self.next_chunk()?);
        }
    }
}
