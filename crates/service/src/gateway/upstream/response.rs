use bytes::Bytes;
use std::io::Read;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

const GATEWAY_STREAM_READ_CHUNK_BYTES: usize = 8 * 1024;
const GATEWAY_STREAM_CHANNEL_CAPACITY: usize = 128;
const OPENAI_RESPONSES_PREFLIGHT_MAX_BYTES: usize = 256 * 1024;
const OPENAI_RESPONSES_STABLE_BUFFER_MAX_BYTES: usize = 16 * 1024 * 1024;

fn next_sse_frame_end(buffer: &[u8]) -> Option<usize> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| index + 2);
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4);
    match (lf, crlf) {
        (Some(lf), Some(crlf)) => Some(lf.min(crlf)),
        (Some(end), None) | (None, Some(end)) => Some(end),
        (None, None) => None,
    }
}

fn openai_responses_sse_frame(frame: &[u8]) -> Option<(String, serde_json::Value)> {
    let Ok(frame) = std::str::from_utf8(frame) else {
        return None;
    };
    let event = frame
        .lines()
        .find_map(|line| line.strip_prefix("event:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.trim().is_empty() {
        return event.map(|event| (event, serde_json::Value::Null));
    }
    let value = serde_json::from_str::<serde_json::Value>(&data).ok()?;
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(event)?;
    Some((event_type, value))
}

fn frame_is_only_openai_responses_prelude(frame: &[u8]) -> bool {
    let Ok(frame_text) = std::str::from_utf8(frame) else {
        return false;
    };
    if !frame_text.lines().any(|line| line.starts_with("data:")) {
        // SSE comments/keepalives are safe to buffer with the initial prelude.
        return true;
    }
    openai_responses_sse_frame(frame).is_some_and(|(event_type, _)| {
        matches!(
            event_type.as_str(),
            "response.created" | "response.in_progress"
        )
    })
}

fn response_terminal_error_summary(value: &serde_json::Value) -> String {
    const POINTERS: &[&str] = &[
        "/response/error/code",
        "/response/error/type",
        "/response/error/message",
        "/response/status_details/error/code",
        "/response/status_details/error/type",
        "/response/status_details/error/message",
        "/response/incomplete_details/reason",
        "/error/code",
        "/error/type",
        "/error/message",
        "/code",
        "/message",
    ];
    POINTERS
        .iter()
        .filter_map(|pointer| value.pointer(pointer).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenAiResponsesTerminalFailureClass {
    UpstreamTransient,
    RateLimited,
    AccountUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenAiResponsesTerminalFailure {
    pub(crate) class: OpenAiResponsesTerminalFailureClass,
    pub(crate) message: String,
}

fn frame_retryable_openai_responses_terminal(
    frame: &[u8],
) -> Option<OpenAiResponsesTerminalFailure> {
    let (event_type, value) = openai_responses_sse_frame(frame)?;
    if event_type == "response.output_text.delta" {
        let delta = value
            .get("delta")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        let normalized = delta.to_ascii_lowercase();
        if [
            "you've hit your usage limit",
            "you have hit your usage limit",
            "usage limit reached",
            "rate limit exceeded",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
        {
            return Some(OpenAiResponsesTerminalFailure {
                class: OpenAiResponsesTerminalFailureClass::RateLimited,
                message: format!(
                    "response.output_text.delta before downstream commit: {}",
                    if delta.is_empty() {
                        "usage limit"
                    } else {
                        delta
                    }
                ),
            });
        }
    }
    if !matches!(
        event_type.as_str(),
        "response.failed" | "response.incomplete"
    ) {
        return None;
    }
    let summary = response_terminal_error_summary(&value);
    let normalized = summary.to_ascii_lowercase();
    let class = if [
        "rate_limit",
        "rate limit",
        "insufficient_quota",
        "usage_limit",
        "usage limit",
        "quota_exceeded",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        Some(OpenAiResponsesTerminalFailureClass::RateLimited)
    } else if [
        "account_deactivated",
        "account deactivated",
        "account_disabled",
        "account disabled",
        "invalid_token",
        "token_expired",
        "unauthorized",
        "authentication",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        Some(OpenAiResponsesTerminalFailureClass::AccountUnavailable)
    } else if [
        "upstream_error",
        "stream_timeout",
        "internal_server_error",
        "server_error",
        "service_unavailable",
        "temporarily_unavailable",
        "overloaded",
        "timed out",
        "timeout",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
    {
        Some(OpenAiResponsesTerminalFailureClass::UpstreamTransient)
    } else {
        None
    }?;
    let message = if summary.is_empty() {
        format!("{event_type} before semantic response content")
    } else {
        format!("{event_type} before semantic response content: {summary}")
    };
    Some(OpenAiResponsesTerminalFailure { class, message })
}

#[derive(Debug)]
enum GatewayStreamPreflight {
    Ready(GatewayStreamResponse),
    TransportFailure(String),
    RetryableTerminal {
        response: GatewayStreamResponse,
        failure: OpenAiResponsesTerminalFailure,
    },
}

fn buffered_stream(prefix: Vec<u8>, remaining: GatewayByteStream) -> GatewayByteStream {
    let (tx, rx) = mpsc::sync_channel::<GatewayByteStreamItem>(GATEWAY_STREAM_CHANNEL_CAPACITY);
    thread::spawn(move || {
        if !prefix.is_empty()
            && tx
                .send(GatewayByteStreamItem::Chunk(Bytes::from(prefix)))
                .is_err()
        {
            return;
        }
        loop {
            match remaining.recv() {
                Ok(item) => {
                    let terminal = matches!(
                        item,
                        GatewayByteStreamItem::Eof | GatewayByteStreamItem::Error(_)
                    );
                    if tx.send(item).is_err() || terminal {
                        return;
                    }
                }
                Err(_) => {
                    let _ = tx.send(GatewayByteStreamItem::Eof);
                    return;
                }
            }
        }
    });
    GatewayByteStream::from_receiver(rx)
}

#[derive(Debug, Clone)]
pub(crate) enum GatewayByteStreamItem {
    Chunk(Bytes),
    Eof,
    Error(String),
}

#[derive(Debug)]
pub(crate) struct GatewayByteStream {
    rx: Receiver<GatewayByteStreamItem>,
}

impl GatewayByteStream {
    pub(crate) fn from_blocking_response(mut response: reqwest::blocking::Response) -> Self {
        let (tx, rx) = mpsc::sync_channel::<GatewayByteStreamItem>(GATEWAY_STREAM_CHANNEL_CAPACITY);
        thread::spawn(move || loop {
            let mut buffer = vec![0_u8; GATEWAY_STREAM_READ_CHUNK_BYTES];
            match response.read(&mut buffer) {
                Ok(0) => {
                    let _ = tx.send(GatewayByteStreamItem::Eof);
                    return;
                }
                Ok(read) => {
                    buffer.truncate(read);
                    if tx
                        .send(GatewayByteStreamItem::Chunk(Bytes::from(buffer)))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(err) => {
                    let _ = tx.send(GatewayByteStreamItem::Error(err.to_string()));
                    return;
                }
            }
        });
        Self { rx }
    }

    pub(crate) fn from_receiver(rx: Receiver<GatewayByteStreamItem>) -> Self {
        Self { rx }
    }

    pub(crate) fn recv(&self) -> Result<GatewayByteStreamItem, mpsc::RecvError> {
        self.rx.recv()
    }

    pub(crate) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<GatewayByteStreamItem, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    pub(crate) fn tee(self) -> (Self, Self) {
        let (left_tx, left_rx) =
            mpsc::sync_channel::<GatewayByteStreamItem>(GATEWAY_STREAM_CHANNEL_CAPACITY);
        let (right_tx, right_rx) =
            mpsc::sync_channel::<GatewayByteStreamItem>(GATEWAY_STREAM_CHANNEL_CAPACITY);
        thread::spawn(move || loop {
            match self.rx.recv() {
                Ok(item) => {
                    let is_terminal = matches!(
                        item,
                        GatewayByteStreamItem::Eof | GatewayByteStreamItem::Error(_)
                    );
                    let left_open = left_tx.send(item.clone()).is_ok();
                    let right_open = right_tx.send(item).is_ok();
                    if is_terminal || (!left_open && !right_open) {
                        return;
                    }
                }
                Err(_) => {
                    let _ = left_tx.send(GatewayByteStreamItem::Eof);
                    let _ = right_tx.send(GatewayByteStreamItem::Eof);
                    return;
                }
            }
        });
        (Self { rx: left_rx }, Self { rx: right_rx })
    }

    pub(crate) fn read_all_bytes(self) -> Result<Bytes, String> {
        let mut buffer = Vec::new();
        loop {
            match self.rx.recv() {
                Ok(GatewayByteStreamItem::Chunk(bytes)) => buffer.extend_from_slice(bytes.as_ref()),
                Ok(GatewayByteStreamItem::Eof) => return Ok(Bytes::from(buffer)),
                Ok(GatewayByteStreamItem::Error(err)) => return Err(err),
                Err(_) => return Ok(Bytes::from(buffer)),
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct GatewayStreamResponse {
    status: reqwest::StatusCode,
    headers: reqwest::header::HeaderMap,
    body: GatewayByteStream,
    actual_source_id: Option<String>,
}

impl GatewayStreamResponse {
    pub(crate) fn new(
        status: reqwest::StatusCode,
        headers: reqwest::header::HeaderMap,
        body: GatewayByteStream,
    ) -> Self {
        Self {
            status,
            headers,
            body,
            actual_source_id: None,
        }
    }

    pub(crate) fn from_blocking_response(response: reqwest::blocking::Response) -> Self {
        let status = response.status();
        let headers = response.headers().clone();
        let body = GatewayByteStream::from_blocking_response(response);
        Self::new(status, headers, body)
    }

    pub(crate) fn with_actual_source_id(mut self, source_id: Option<String>) -> Self {
        self.actual_source_id = source_id.filter(|value| !value.trim().is_empty());
        self
    }

    pub(crate) fn actual_source_id(&self) -> Option<&str> {
        self.actual_source_id.as_deref()
    }

    /// Buffers a normal-sized OpenAI Responses stream through its terminal
    /// frame. This keeps a mid-stream transport reset retryable because no
    /// semantic bytes have been committed to the downstream Codex client yet.
    /// Very large streams fall back to live delivery after a bounded buffer.
    fn preflight_openai_responses_stream(mut self) -> GatewayStreamPreflight {
        let mut prefix = Vec::new();
        let mut unparsed = Vec::new();
        let mut saw_semantic_frame = false;
        loop {
            match self.body.recv() {
                Ok(GatewayByteStreamItem::Chunk(bytes)) => {
                    prefix.extend_from_slice(bytes.as_ref());
                    unparsed.extend_from_slice(bytes.as_ref());

                    while let Some(frame_end) = next_sse_frame_end(unparsed.as_slice()) {
                        let frame = unparsed.drain(..frame_end).collect::<Vec<_>>();
                        if frame_is_only_openai_responses_prelude(frame.as_slice()) {
                            continue;
                        }
                        if let Some(failure) =
                            frame_retryable_openai_responses_terminal(frame.as_slice())
                        {
                            self.body = buffered_stream(prefix, self.body);
                            return GatewayStreamPreflight::RetryableTerminal {
                                response: self,
                                failure,
                            };
                        }
                        let event_type = openai_responses_sse_frame(frame.as_slice())
                            .map(|(event_type, _)| event_type);
                        if event_type.as_deref().is_some_and(|event_type| {
                            matches!(event_type, "response.completed" | "response.done")
                        }) {
                            self.body = buffered_stream(prefix, self.body);
                            return GatewayStreamPreflight::Ready(self);
                        }
                        // A permanent failed/incomplete response is already a provider
                        // terminal result. Preserve it instead of retrying it as a
                        // transport failure.
                        if event_type.as_deref().is_some_and(|event_type| {
                            matches!(event_type, "response.failed" | "response.incomplete")
                        }) {
                            self.body = buffered_stream(prefix, self.body);
                            return GatewayStreamPreflight::Ready(self);
                        }
                        saw_semantic_frame = true;
                    }

                    if saw_semantic_frame
                        && prefix.len() >= OPENAI_RESPONSES_STABLE_BUFFER_MAX_BYTES
                    {
                        self.body = buffered_stream(prefix, self.body);
                        return GatewayStreamPreflight::Ready(self);
                    }
                    if !saw_semantic_frame && prefix.len() >= OPENAI_RESPONSES_PREFLIGHT_MAX_BYTES {
                        return GatewayStreamPreflight::TransportFailure(format!(
                            "upstream responses preflight exceeded {} bytes before semantic content",
                            OPENAI_RESPONSES_PREFLIGHT_MAX_BYTES
                        ));
                    }
                }
                Ok(GatewayByteStreamItem::Eof) => {
                    return GatewayStreamPreflight::TransportFailure(if saw_semantic_frame {
                        "upstream stream ended before response.completed".to_string()
                    } else {
                        "upstream stream disconnected before semantic response content".to_string()
                    });
                }
                Ok(GatewayByteStreamItem::Error(error)) => {
                    return GatewayStreamPreflight::TransportFailure(if saw_semantic_frame {
                        format!("upstream stream disconnected before response.completed: {error}")
                    } else {
                        format!(
                            "upstream stream disconnected before semantic response content: {error}"
                        )
                    });
                }
                Err(_) => {
                    return GatewayStreamPreflight::TransportFailure(
                        "upstream stream reader disconnected before semantic response content"
                            .to_string(),
                    );
                }
            }
        }
    }

    pub(crate) fn status(&self) -> reqwest::StatusCode {
        self.status
    }

    pub(crate) fn headers(&self) -> &reqwest::header::HeaderMap {
        &self.headers
    }

    pub(crate) fn read_all_bytes(self) -> Result<Bytes, String> {
        self.body.read_all_bytes()
    }

    pub(crate) fn into_body(self) -> GatewayByteStream {
        self.body
    }
}

#[derive(Debug)]
pub(crate) enum GatewayUpstreamResponse {
    Blocking(reqwest::blocking::Response),
    Stream(GatewayStreamResponse),
}

#[derive(Debug)]
pub(crate) enum OpenAiResponsesPreflight {
    Ready(GatewayUpstreamResponse),
    TransportFailure(String),
    RetryableTerminal {
        response: GatewayUpstreamResponse,
        failure: OpenAiResponsesTerminalFailure,
    },
}

impl GatewayUpstreamResponse {
    pub(crate) fn status(&self) -> reqwest::StatusCode {
        match self {
            Self::Blocking(response) => response.status(),
            Self::Stream(response) => response.status(),
        }
    }

    pub(crate) fn headers(&self) -> &reqwest::header::HeaderMap {
        match self {
            Self::Blocking(response) => response.headers(),
            Self::Stream(response) => response.headers(),
        }
    }

    pub(crate) fn actual_source_id(&self) -> Option<&str> {
        match self {
            Self::Blocking(_) => None,
            Self::Stream(response) => response.actual_source_id(),
        }
    }

    pub(crate) fn preflight_openai_responses_stream(self) -> OpenAiResponsesPreflight {
        match self {
            Self::Stream(response) if response.status().is_success() => {
                match response.preflight_openai_responses_stream() {
                    GatewayStreamPreflight::Ready(response) => {
                        OpenAiResponsesPreflight::Ready(Self::Stream(response))
                    }
                    GatewayStreamPreflight::TransportFailure(error) => {
                        OpenAiResponsesPreflight::TransportFailure(error)
                    }
                    GatewayStreamPreflight::RetryableTerminal { response, failure } => {
                        OpenAiResponsesPreflight::RetryableTerminal {
                            response: Self::Stream(response),
                            failure,
                        }
                    }
                }
            }
            response => OpenAiResponsesPreflight::Ready(response),
        }
    }
}

impl From<reqwest::blocking::Response> for GatewayUpstreamResponse {
    fn from(response: reqwest::blocking::Response) -> Self {
        Self::Blocking(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn empty_stream_response() -> GatewayStreamResponse {
        let (_tx, rx) = mpsc::sync_channel(1);
        GatewayStreamResponse::new(
            reqwest::StatusCode::OK,
            reqwest::header::HeaderMap::new(),
            GatewayByteStream::from_receiver(rx),
        )
    }

    fn stream_response(items: Vec<GatewayByteStreamItem>) -> GatewayStreamResponse {
        let (tx, rx) = mpsc::sync_channel(items.len().max(1));
        for item in items {
            tx.send(item).unwrap();
        }
        drop(tx);
        GatewayStreamResponse::new(
            reqwest::StatusCode::OK,
            reqwest::header::HeaderMap::new(),
            GatewayByteStream::from_receiver(rx),
        )
    }

    fn expect_ready(outcome: GatewayStreamPreflight) -> GatewayStreamResponse {
        match outcome {
            GatewayStreamPreflight::Ready(response) => response,
            other => panic!("expected ready preflight, got {other:?}"),
        }
    }

    fn expect_transport_failure(outcome: GatewayStreamPreflight) -> String {
        match outcome {
            GatewayStreamPreflight::TransportFailure(error) => error,
            other => panic!("expected transport failure, got {other:?}"),
        }
    }

    fn expect_terminal_failure(
        outcome: GatewayStreamPreflight,
    ) -> (GatewayStreamResponse, OpenAiResponsesTerminalFailure) {
        match outcome {
            GatewayStreamPreflight::RetryableTerminal { response, failure } => (response, failure),
            other => panic!("expected retryable terminal, got {other:?}"),
        }
    }

    #[test]
    fn stream_response_exposes_selected_runtime_credential() {
        let response =
            empty_stream_response().with_actual_source_id(Some("grok-credential-7".into()));
        assert_eq!(response.actual_source_id(), Some("grok-credential-7"));

        let upstream = GatewayUpstreamResponse::Stream(response);
        assert_eq!(upstream.actual_source_id(), Some("grok-credential-7"));
    }

    #[test]
    fn stream_response_rejects_blank_runtime_credential() {
        let response = empty_stream_response().with_actual_source_id(Some("   ".into()));
        assert_eq!(response.actual_source_id(), None);
    }

    #[test]
    fn responses_preflight_rejects_disconnect_after_only_created_and_in_progress() {
        let response = stream_response(vec![
            GatewayByteStreamItem::Chunk(Bytes::from_static(
                b"data: {\"type\":\"response.created\"}\r\n\r\n",
            )),
            GatewayByteStreamItem::Chunk(Bytes::from_static(
                b"data: {\"type\":\"response.in_progress\"}\n\n",
            )),
            GatewayByteStreamItem::Error("connection reset".to_string()),
        ]);

        let error = expect_transport_failure(response.preflight_openai_responses_stream());
        assert!(error.contains("before semantic response content"));
        assert!(error.contains("connection reset"));
    }

    #[test]
    fn responses_preflight_replays_prelude_once_semantic_event_arrives() {
        let created = b"data: {\"type\":\"response.created\"}\r\n\r\n";
        let delta = b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n";
        let completed = b"data: {\"type\":\"response.completed\"}\n\n";
        let response = stream_response(vec![
            GatewayByteStreamItem::Chunk(Bytes::from_static(created)),
            GatewayByteStreamItem::Chunk(Bytes::from_static(delta)),
            GatewayByteStreamItem::Chunk(Bytes::from_static(completed)),
            GatewayByteStreamItem::Eof,
        ]);

        let response = expect_ready(response.preflight_openai_responses_stream());
        let bytes = response.read_all_bytes().unwrap();
        let expected = [created.as_slice(), delta.as_slice(), completed.as_slice()].concat();
        assert_eq!(bytes.as_ref(), expected.as_slice());
    }

    #[test]
    fn responses_preflight_retries_disconnect_after_unknown_semantic_event() {
        let unknown = b"data: {\"type\":\"provider.custom_event\"}\n\n";
        let response = stream_response(vec![
            GatewayByteStreamItem::Chunk(Bytes::from_static(unknown)),
            GatewayByteStreamItem::Eof,
        ]);

        let error = expect_transport_failure(response.preflight_openai_responses_stream());
        assert!(error.contains("before response.completed"));
    }

    #[test]
    fn responses_preflight_retries_transport_error_after_semantic_event() {
        let response = stream_response(vec![
            GatewayByteStreamItem::Chunk(Bytes::from_static(
                b"data: {\"type\":\"response.output_item.added\"}\n\n",
            )),
            GatewayByteStreamItem::Error("h2 stream reset".to_string()),
        ]);

        let error = expect_transport_failure(response.preflight_openai_responses_stream());
        assert!(error.contains("before response.completed"));
        assert!(error.contains("h2 stream reset"));
    }

    #[test]
    fn responses_preflight_retries_transient_failed_event_before_commit() {
        let response = stream_response(vec![GatewayByteStreamItem::Chunk(Bytes::from_static(
            b"event: response.failed\ndata: {\"error\":{\"code\":\"internal_server_error\",\"message\":\"Internal server error\"}}\n\n",
        ))]);

        let (response, failure) =
            expect_terminal_failure(response.preflight_openai_responses_stream());
        assert_eq!(
            failure.class,
            OpenAiResponsesTerminalFailureClass::UpstreamTransient
        );
        assert!(failure.message.contains("response.failed"));
        assert!(failure.message.contains("internal_server_error"));
        assert!(response
            .read_all_bytes()
            .unwrap()
            .starts_with(b"event: response.failed"));
    }

    #[test]
    fn responses_preflight_delivers_permanent_failed_event_without_replay() {
        let failed = b"event: response.failed\ndata: {\"response\":{\"error\":{\"code\":\"model_not_found\",\"type\":\"invalid_request_error\"}}}\n\n";
        let response = stream_response(vec![
            GatewayByteStreamItem::Chunk(Bytes::from_static(failed)),
            GatewayByteStreamItem::Eof,
        ]);

        let response = expect_ready(response.preflight_openai_responses_stream());
        assert_eq!(response.read_all_bytes().unwrap().as_ref(), failed);
    }

    #[test]
    fn responses_preflight_retries_stream_timeout_incomplete_before_commit() {
        let response = stream_response(vec![GatewayByteStreamItem::Chunk(Bytes::from_static(
            b"event: response.incomplete\ndata: {\"response\":{\"status_details\":{\"error\":{\"code\":\"stream_timeout\",\"message\":\"stream timeout at upstream\"}}}}\n\n",
        ))]);

        let (_, failure) = expect_terminal_failure(response.preflight_openai_responses_stream());
        assert_eq!(
            failure.class,
            OpenAiResponsesTerminalFailureClass::UpstreamTransient
        );
        assert!(failure.message.contains("response.incomplete"));
        assert!(failure.message.contains("stream_timeout"));
    }

    #[test]
    fn responses_preflight_delivers_token_limit_incomplete_without_replay() {
        let incomplete = b"data: {\"type\":\"response.incomplete\",\"response\":{\"incomplete_details\":{\"reason\":\"max_output_tokens\"}}}\n\n";
        let response = stream_response(vec![
            GatewayByteStreamItem::Chunk(Bytes::from_static(incomplete)),
            GatewayByteStreamItem::Eof,
        ]);

        let response = expect_ready(response.preflight_openai_responses_stream());
        assert_eq!(response.read_all_bytes().unwrap().as_ref(), incomplete);
    }

    #[test]
    fn responses_preflight_classifies_event_only_upstream_error_incomplete() {
        let response = stream_response(vec![GatewayByteStreamItem::Chunk(Bytes::from_static(
            b"event: response.incomplete\ndata: {\"response\":{\"status_details\":{\"error\":{\"code\":\"upstream_error\"}}}}\n\n",
        ))]);

        let (_, failure) = expect_terminal_failure(response.preflight_openai_responses_stream());
        assert_eq!(
            failure.class,
            OpenAiResponsesTerminalFailureClass::UpstreamTransient
        );
        assert!(failure.message.contains("upstream_error"));
    }

    #[test]
    fn responses_preflight_classifies_rate_limit_and_account_failures() {
        let rate_limited = stream_response(vec![GatewayByteStreamItem::Chunk(
            Bytes::from_static(
                b"data: {\"type\":\"response.failed\",\"error\":{\"code\":\"rate_limit_exceeded\"}}\n\n",
            ),
        )]);
        let (_, rate_failure) =
            expect_terminal_failure(rate_limited.preflight_openai_responses_stream());
        assert_eq!(
            rate_failure.class,
            OpenAiResponsesTerminalFailureClass::RateLimited
        );

        let unavailable = stream_response(vec![GatewayByteStreamItem::Chunk(
            Bytes::from_static(
                b"data: {\"type\":\"response.failed\",\"error\":{\"code\":\"account_deactivated\"}}\n\n",
            ),
        )]);
        let (_, account_failure) =
            expect_terminal_failure(unavailable.preflight_openai_responses_stream());
        assert_eq!(
            account_failure.class,
            OpenAiResponsesTerminalFailureClass::AccountUnavailable
        );
    }

    #[test]
    fn responses_preflight_retries_usage_limit_text_before_downstream_commit() {
        let response = stream_response(vec![GatewayByteStreamItem::Chunk(Bytes::from_static(
            b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. Try again later.\"}\n\n",
        ))]);

        let (_, failure) = expect_terminal_failure(response.preflight_openai_responses_stream());
        assert_eq!(
            failure.class,
            OpenAiResponsesTerminalFailureClass::RateLimited
        );
        assert!(failure.message.contains("usage limit"));
    }

    #[test]
    fn responses_preflight_does_not_commit_after_only_oversized_prelude() {
        let mut prelude = Vec::new();
        while prelude.len() < OPENAI_RESPONSES_PREFLIGHT_MAX_BYTES {
            prelude.extend_from_slice(b": keepalive\n\n");
        }
        let response = stream_response(vec![GatewayByteStreamItem::Chunk(Bytes::from(prelude))]);

        let error = expect_transport_failure(response.preflight_openai_responses_stream());
        assert!(error.contains("exceeded"));
        assert!(error.contains("before semantic content"));
    }
}
