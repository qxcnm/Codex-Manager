//! Transport-independent input for the gateway's routing and delivery code.
//!
//! Axum owns the only production socket. The bounded response channel preserves
//! backpressure and reports a closed HTTP body to async delivery, so accounting
//! and finalization run on downstream cancellation too. Short terminal responses
//! retain a synchronous helper for in-memory bodies.
use std::cell::RefCell;
use std::future::Future;
use std::io::{self, Cursor, Read};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use axum::body::Body;
use axum::http::{header, request::Parts, Method, Response, StatusCode};
use bytes::Bytes;
use tokio::sync::{mpsc, oneshot, watch};

thread_local! {
    static RESPONSE_CANCELLATION: RefCell<Option<watch::Receiver<bool>>> = const { RefCell::new(None) };
}

tokio::task_local! {
    static ASYNC_RESPONSE_CANCELLATION: watch::Receiver<bool>;
}

pub(crate) async fn scope_response_cancellation<F: Future>(
    cancellation: watch::Receiver<bool>,
    future: F,
) -> F::Output {
    ASYNC_RESPONSE_CANCELLATION
        .scope(cancellation, future)
        .await
}

// Retained for the legacy synchronous request bridge; native handlers use task-local cancellation.
#[allow(dead_code)]
pub(crate) struct ResponseCancellationScope(Option<watch::Receiver<bool>>);

impl Drop for ResponseCancellationScope {
    fn drop(&mut self) {
        RESPONSE_CANCELLATION.with(|current| {
            current.replace(self.0.take());
        });
    }
}

pub(crate) struct CancelResponseOnDrop(Option<watch::Sender<bool>>);

/// Trusted ingress metadata for an internally dispatched WebSocket turn.
/// HTTP headers never create this marker or override the actual POST method.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WebSocketHttpBridge;

/// Internal per-turn delivery confirmation. Reading the bridged HTTP body is
/// only an intermediate step: a WebSocket terminal must also reach its sink.
#[derive(Clone)]
pub(crate) struct WebSocketDeliveryGate(watch::Receiver<bool>);

impl WebSocketDeliveryGate {
    pub(crate) fn new() -> (watch::Sender<bool>, Self) {
        let (confirmation, delivered) = watch::channel(false);
        (confirmation, Self(delivered))
    }
}

/// Shared with outer HTTP middleware through response extensions. Only the
/// body consumer may confirm a complete protocol terminal, never the parser.
#[derive(Clone, Default)]
pub(crate) struct ResponseBodyDeliveryState(Arc<AtomicBool>);

impl ResponseBodyDeliveryState {
    pub(crate) fn terminal_consumed(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn mark_terminal_consumed(&self) {
        self.0.store(true, Ordering::Release);
    }
}

struct ResponseBodyChunk {
    bytes: io::Result<Bytes>,
    terminal_consumed: Option<oneshot::Sender<()>>,
}

impl ResponseBodyChunk {
    fn new(bytes: io::Result<Bytes>) -> Self {
        Self {
            bytes,
            terminal_consumed: None,
        }
    }
}

impl CancelResponseOnDrop {
    pub(crate) fn disarm(&mut self) {
        self.0.take();
    }
}

impl Drop for CancelResponseOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = &self.0 {
            sender.send_replace(true);
        }
    }
}

/// Capture cancellation on the domain worker before the future can move to an
/// async runtime. Old standalone reader tests have no HTTP cancellation scope.
pub(crate) fn with_response_cancellation<F: Future>(
    future: F,
) -> impl Future<Output = Result<F::Output, ()>> {
    let cancellation = ASYNC_RESPONSE_CANCELLATION
        .try_with(Clone::clone)
        .ok()
        .or_else(|| RESPONSE_CANCELLATION.with(|current| current.borrow().clone()));
    async move {
        match cancellation {
            Some(mut cancellation) => tokio::select! {
                biased;
                _ = cancellation.wait_for(|cancelled| *cancelled) => Err(()),
                result = future => Ok(result),
            },
            None => Ok(future.await),
        }
    }
}

pub(crate) struct GatewayRequest {
    method: Method,
    request_type: &'static str,
    websocket_delivery: Option<watch::Receiver<bool>>,
    uri: String,
    headers: Vec<super::gateway_response::Header>,
    remote_addr: Option<std::net::SocketAddr>,
    body: Cursor<Bytes>,
    response: oneshot::Sender<Response<Body>>,
    cancellation: watch::Sender<bool>,
    cancelled: watch::Receiver<bool>,
    upstream_replay_guard: super::gateway_replay_guard::UpstreamReplayGuard,
    lifetime_guards: Vec<Box<dyn Send>>,
    response_admission: Option<tokio::sync::SemaphorePermit<'static>>,
    completion: Option<oneshot::Receiver<()>>,
    #[cfg(test)]
    test_request: Option<tiny_http::Request>,
    #[cfg(test)]
    test_response_receiver: Option<oneshot::Receiver<Response<Body>>>,
}

impl GatewayRequest {
    pub(crate) fn new(parts: Parts, body: Bytes) -> (Self, oneshot::Receiver<Response<Body>>) {
        let (response, receiver) = oneshot::channel();
        let (cancellation, cancelled) = watch::channel(false);
        let headers = parts
            .headers
            .iter()
            .filter_map(|(name, value)| {
                super::gateway_response::Header::from_bytes(
                    name.as_str().as_bytes(),
                    value.as_bytes(),
                )
                .ok()
            })
            .collect();
        (
            Self {
                method: parts.method,
                request_type: if parts.extensions.get::<WebSocketHttpBridge>().is_some() {
                    "ws"
                } else {
                    "http"
                },
                websocket_delivery: parts.extensions.get::<WebSocketHttpBridge>().and_then(|_| {
                    parts
                        .extensions
                        .get::<WebSocketDeliveryGate>()
                        .map(|gate| gate.0.clone())
                }),
                uri: parts
                    .uri
                    .path_and_query()
                    .map(|uri| uri.as_str())
                    .unwrap_or("/")
                    .to_owned(),
                headers,
                remote_addr: parts
                    .extensions
                    .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                    .map(|peer| peer.0),
                body: Cursor::new(body),
                response,
                cancellation,
                cancelled,
                upstream_replay_guard: Default::default(),
                lifetime_guards: Vec::new(),
                response_admission: None,
                completion: None,
                #[cfg(test)]
                test_request: None,
                #[cfg(test)]
                test_response_receiver: None,
            },
            receiver,
        )
    }

    pub(crate) fn method(&self) -> &Method {
        &self.method
    }
    pub(crate) fn request_type(&self) -> &'static str {
        self.request_type
    }
    pub(crate) fn url(&self) -> &str {
        &self.uri
    }
    pub(crate) fn headers(&self) -> &[super::gateway_response::Header] {
        &self.headers
    }
    pub(crate) fn remote_addr(&self) -> Option<&std::net::SocketAddr> {
        self.remote_addr.as_ref()
    }
    pub(crate) fn as_reader(&mut self) -> &mut dyn Read {
        &mut self.body
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.response.is_closed() || *self.cancelled.borrow()
    }

    pub(crate) fn upstream_replay_guard(
        &self,
    ) -> &super::gateway_replay_guard::UpstreamReplayGuard {
        &self.upstream_replay_guard
    }

    pub(crate) fn cancellation_guard(&self) -> CancelResponseOnDrop {
        CancelResponseOnDrop(Some(self.cancellation.clone()))
    }

    pub(crate) fn cancellation_receiver(&self) -> watch::Receiver<bool> {
        self.cancelled.clone()
    }

    pub(crate) fn hold_until_complete(&mut self, guard: impl Send + 'static) {
        self.lifetime_guards.push(Box::new(guard));
    }

    pub(crate) fn take_lifetime_guards(&mut self) -> Vec<Box<dyn Send>> {
        std::mem::take(&mut self.lifetime_guards)
    }

    pub(crate) fn has_response_admission(&self) -> bool {
        self.response_admission.is_some()
    }

    pub(crate) fn hold_response_admission(
        &mut self,
        permit: tokio::sync::SemaphorePermit<'static>,
    ) {
        self.response_admission = Some(permit);
    }

    pub(crate) fn take_response_admission(
        &mut self,
    ) -> Option<tokio::sync::SemaphorePermit<'static>> {
        self.response_admission.take()
    }

    pub(crate) fn finish_body_after(&mut self, completion: oneshot::Receiver<()>) {
        self.completion = Some(completion);
    }

    pub(crate) fn is_native(&self) -> bool {
        #[cfg(test)]
        if self.test_request.is_some() {
            return false;
        }
        true
    }

    /// Native delivery waits for upstream chunks and downstream capacity with
    /// await. A slow stream therefore owns no blocking-domain worker.
    #[allow(unused_mut)]
    pub(crate) async fn respond_async<R>(
        mut self,
        response: super::gateway_response::Response<R>,
    ) -> io::Result<()>
    where
        R: super::gateway_response_body::GatewayResponseBody + 'static,
    {
        #[cfg(test)]
        if let Some(request) = self.test_request.take() {
            let (status, headers, body, length) = response.into_parts();
            return tokio::task::spawn_blocking(move || {
                request.respond(
                    super::gateway_response::Response::new(
                        status,
                        headers,
                        super::gateway_response_body::TestBodyReader(body),
                        length,
                    )
                    .into_test_response(),
                )
            })
            .await
            .map_err(io::Error::other)?;
        }
        let status = StatusCode::from_u16(response.status_code().0).map_err(io::Error::other)?;
        // Error/JSON responses are converted into WS errors by the caller and
        // retain their actual HTTP status. Only successful SSE turns wait for
        // the outer socket's terminal-send confirmation.
        let websocket_delivery = self.websocket_delivery.take().filter(|_| {
            status.is_success()
                && response.headers().iter().any(|header| {
                    header.field.as_str().eq_ignore_ascii_case("content-type")
                        && header
                            .value
                            .as_str()
                            .to_ascii_lowercase()
                            .starts_with("text/event-stream")
                })
        });
        let mut builder = Response::builder().status(status);
        for item in response.headers() {
            let name = item.field.as_str();
            let header_name =
                axum::http::HeaderName::from_bytes(name.as_bytes()).map_err(io::Error::other)?;
            if !super::header_filter::should_skip_response_header(&header_name) {
                builder = builder.header(name, item.value.as_str());
            }
        }
        let expected_length = response.data_length();
        if let Some(length) = expected_length {
            builder = builder.header(header::CONTENT_LENGTH, length);
        }
        let suppress_body = self.method == Method::HEAD
            || status == StatusCode::NO_CONTENT
            || status == StatusCode::NOT_MODIFIED;
        let delivery_state = ResponseBodyDeliveryState::default();
        builder = builder.extension(delivery_state.clone());
        let (sender, receiver) = mpsc::channel::<ResponseBodyChunk>(2);
        // The WS bridge must observe HTTP EOF before deciding that a terminal
        // is missing. Waiting for finalization here would wait for its own
        // socket confirmation in that case. Ordinary HTTP keeps its EOF barrier.
        let completion = self
            .completion
            .take()
            .filter(|_| websocket_delivery.is_none());
        let stream = futures_util::stream::unfold(
            (
                receiver,
                self.cancellation_guard(),
                completion,
                delivery_state,
            ),
            |(mut receiver, cancellation, completion, delivery_state)| async move {
                match receiver.recv().await {
                    Some(chunk) => {
                        // A terminal read is only complete once HTTP takes the
                        // last bytes, not merely when the producer queues them.
                        if let Some(consumed) = chunk.terminal_consumed {
                            delivery_state.mark_terminal_consumed();
                            let _ = consumed.send(());
                        }
                        Some((
                            chunk.bytes,
                            (receiver, cancellation, completion, delivery_state),
                        ))
                    }
                    None => {
                        if let Some(completion) = completion {
                            let _ = completion.await;
                        }
                        None
                    }
                }
            },
        );
        let body = if suppress_body {
            Body::empty()
        } else {
            Body::from_stream(stream)
        };
        self.response
            .send(builder.body(body).map_err(io::Error::other)?)
            .map_err(|_| disconnected())?;
        if suppress_body {
            return Ok(());
        }
        let mut reader = response.into_reader();
        let mut buffer = [0_u8; 16 * 1024];
        let mut written = 0_usize;
        let delivery = loop {
            if expected_length.is_some_and(|length| written >= length) {
                break Ok(());
            }
            let read_limit = expected_length
                .map(|length| length.saturating_sub(written).min(buffer.len()))
                .unwrap_or(buffer.len());
            let read = tokio::select! {
                biased;
                _ = sender.closed() => break Err(disconnected()),
                _ = self.cancelled.wait_for(|cancelled| *cancelled) => break Err(disconnected()),
                read = reader.read_async(&mut buffer[..read_limit]) => read,
            };
            match read {
                Ok(0) if expected_length.is_some_and(|length| written < length) => {
                    let error = || {
                        io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "gateway response body ended early",
                        )
                    };
                    let _ = sender.send(ResponseBodyChunk::new(Err(error()))).await;
                    break Err(error());
                }
                Ok(0) => break Ok(()),
                Ok(length) => {
                    let (terminal_consumed, terminal_ack) = if reader.is_end_stream() {
                        let (consumed, acknowledgement) = oneshot::channel();
                        (Some(consumed), Some(acknowledgement))
                    } else {
                        (None, None)
                    };
                    if sender
                        .send(ResponseBodyChunk {
                            bytes: Ok(Bytes::copy_from_slice(&buffer[..length])),
                            terminal_consumed,
                        })
                        .await
                        .is_err()
                    {
                        break Err(disconnected());
                    }
                    written = written.saturating_add(length);
                    if let Some(acknowledgement) = terminal_ack {
                        // Do not race a completed protocol response against a
                        // later body drop or upstream EOF. Dropping the body
                        // before consuming this chunk drops its sender, so a
                        // genuine truncated delivery still reports BrokenPipe.
                        break acknowledgement.await.map_err(|_| disconnected());
                    }
                }
                Err(error) => {
                    let _ = sender
                        .send(ResponseBodyChunk::new(Err(io::Error::new(
                            error.kind(),
                            "gateway response body failed",
                        ))))
                        .await;
                    break Err(error);
                }
            }
        };
        reader.finish_async().await;
        // Let the bridge observe EOF even while finalization waits for the
        // outer WS send, including readers that end without a protocol terminal.
        drop(sender);
        delivery?;
        if let Some(mut confirmed) = websocket_delivery {
            confirmed
                .wait_for(|delivered| *delivered)
                .await
                .map_err(|_| disconnected())?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn enter_cancellation_scope(&self) -> ResponseCancellationScope {
        ResponseCancellationScope(
            RESPONSE_CANCELLATION.with(|current| current.replace(Some(self.cancelled.clone()))),
        )
    }

    /// Deliver a short in-memory terminal response without occupying a Tokio
    /// blocking worker. Provider bodies must use respond_async instead.
    #[allow(unused_mut)]
    pub(crate) fn respond<R: Read>(
        mut self,
        response: super::gateway_response::Response<R>,
    ) -> io::Result<()> {
        #[cfg(test)]
        if let Some(request) = self.test_request.take() {
            return request.respond(response.into_test_response());
        }
        let status = StatusCode::from_u16(response.status_code().0).map_err(io::Error::other)?;
        let mut builder = Response::builder().status(status);
        for item in response.headers() {
            let name = item.field.as_str();
            let header_name =
                axum::http::HeaderName::from_bytes(name.as_bytes()).map_err(io::Error::other)?;
            if !super::header_filter::should_skip_response_header(&header_name) {
                builder = builder.header(name, item.value.as_str());
            }
        }
        let expected_length = response.data_length();
        if let Some(length) = expected_length {
            builder = builder.header(header::CONTENT_LENGTH, length);
        }
        let suppress_body = self.method == Method::HEAD
            || status == StatusCode::NO_CONTENT
            || status == StatusCode::NOT_MODIFIED;
        let body = if suppress_body {
            Body::empty()
        } else {
            let mut reader = response.into_reader();
            let mut bytes = Vec::new();
            if let Some(length) = expected_length {
                reader.take(length as u64).read_to_end(&mut bytes)?;
                if bytes.len() < length {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "gateway response body ended early",
                    ));
                }
            } else {
                reader.read_to_end(&mut bytes)?;
            }
            Body::from(bytes)
        };
        self.response
            .send(builder.body(body).map_err(io::Error::other)?)
            .map_err(|_| disconnected())
    }
}
fn disconnected() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "broken pipe: downstream HTTP body closed",
    )
}

#[cfg(test)]
impl From<tiny_http::Request> for GatewayRequest {
    fn from(mut request: tiny_http::Request) -> Self {
        // Test fixtures can retain their existing in-memory request builders.
        // Production code never creates or accepts a tiny_http Request.
        let mut bytes = Vec::new();
        request
            .as_reader()
            .read_to_end(&mut bytes)
            .expect("fixture request body");
        let mut builder = axum::http::Request::builder()
            .method(request.method().as_str())
            .uri(request.url());
        for item in request.headers() {
            builder = builder.header(item.field.as_str().as_str(), item.value.as_str());
        }
        let (parts, ()) = builder.body(()).expect("fixture request").into_parts();
        let (mut gateway, receiver) = Self::new(parts, Bytes::from(bytes));
        gateway.test_request = Some(request);
        // The fixture owns its actual tiny_http socket. Keep the unused native
        // response channel alive so its dummy receiver cannot signal a false
        // client disconnect before the upstream attempt.
        gateway.test_response_receiver = Some(receiver);
        gateway
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn request(method: &str) -> (GatewayRequest, oneshot::Receiver<Response<Body>>) {
        let (parts, ()) = axum::http::Request::builder()
            .method(method)
            .uri("/v1/responses?test=1")
            .header("authorization", "Bearer fixture")
            .body(())
            .unwrap()
            .into_parts();
        GatewayRequest::new(parts, Bytes::from_static(b"{}"))
    }

    #[test]
    fn websocket_http_bridge_metadata_preserves_post_method() {
        let (parts, ()) = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .extension(WebSocketHttpBridge)
            .body(())
            .unwrap()
            .into_parts();
        let (request, _receiver) = GatewayRequest::new(parts, Bytes::new());
        assert_eq!(request.request_type(), "ws");
        assert_eq!(request.method(), Method::POST);
    }

    #[test]
    fn websocket_type_cannot_be_spoofed_with_http_headers() {
        let (parts, ()) = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .header("x-codexmanager-request-type", "ws")
            .header("x-codexmanager-websocket-http-bridge", "true")
            .header("upgrade", "websocket")
            .body(())
            .unwrap()
            .into_parts();
        let (request, _receiver) = GatewayRequest::new(parts, Bytes::new());
        assert_eq!(request.request_type(), "http");
        assert_eq!(request.method(), Method::POST);
    }

    #[tokio::test]
    async fn native_response_preserves_status_headers_and_unframed_payload() {
        let (request, receiver) = request("POST");
        assert_eq!(request.url(), "/v1/responses?test=1");
        let worker = tokio::task::spawn_blocking(move || {
            request.respond(
                super::super::gateway_response::Response::from_string("data: hello\n\n")
                    .with_status_code(201)
                    .with_header(
                        super::super::gateway_response::Header::from_bytes(
                            "Content-Type",
                            "text/event-stream",
                        )
                        .unwrap(),
                    )
                    .with_header(
                        super::super::gateway_response::Header::from_bytes(
                            "Connection",
                            "keep-alive",
                        )
                        .unwrap(),
                    ),
            )
        });
        let response = receiver.await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        assert!(!response.headers().contains_key("connection"));
        assert!(!response.headers().contains_key("transfer-encoding"));
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap(),
            "data: hello\n\n"
        );
        worker.await.unwrap().unwrap();
    }

    struct InfiniteReader(Arc<AtomicUsize>);
    impl Read for InfiniteReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.fetch_add(1, Ordering::SeqCst);
            buffer.fill(b'x');
            Ok(buffer.len())
        }
    }

    impl super::super::gateway_response_body::GatewayResponseBody for InfiniteReader {
        fn read_async<'a>(
            &'a mut self,
            buffer: &'a mut [u8],
        ) -> super::super::gateway_response_body::BodyReadFuture<'a> {
            Box::pin(async move { self.read(buffer) })
        }
    }

    struct PendingReader(mpsc::UnboundedSender<()>);

    impl super::super::gateway_response_body::GatewayResponseBody for PendingReader {
        fn read_async<'a>(
            &'a mut self,
            _buffer: &'a mut [u8],
        ) -> super::super::gateway_response_body::BodyReadFuture<'a> {
            Box::pin(async move {
                self.0.send(()).unwrap();
                std::future::pending().await
            })
        }
    }

    struct ProtocolTerminalReader {
        bytes: Cursor<Vec<u8>>,
        terminal_read: Option<oneshot::Sender<()>>,
    }

    impl super::super::gateway_response_body::GatewayResponseBody for ProtocolTerminalReader {
        fn read_async<'a>(
            &'a mut self,
            buffer: &'a mut [u8],
        ) -> super::super::gateway_response_body::BodyReadFuture<'a> {
            Box::pin(async move {
                assert!(
                    !self.is_end_stream(),
                    "protocol completion must not wait for another provider read"
                );
                let read = self.bytes.read(buffer)?;
                if self.is_end_stream() {
                    if let Some(terminal_read) = self.terminal_read.take() {
                        let _ = terminal_read.send(());
                    }
                }
                Ok(read)
            })
        }

        fn is_end_stream(&self) -> bool {
            self.bytes.position() == self.bytes.get_ref().len() as u64
        }
    }

    fn terminal_response_fixture(
        bytes: Vec<u8>,
    ) -> (
        oneshot::Receiver<Response<Body>>,
        oneshot::Receiver<()>,
        tokio::task::JoinHandle<io::Result<()>>,
    ) {
        let (request, receiver) = request("POST");
        let (terminal_read, read_completed) = oneshot::channel();
        // Exercise trait-object forwarding as used by the production bridge.
        let reader: Box<dyn super::super::gateway_response_body::GatewayResponseBody> =
            Box::new(ProtocolTerminalReader {
                bytes: Cursor::new(bytes),
                terminal_read: Some(terminal_read),
            });
        let worker = tokio::spawn(request.respond_async(
            super::super::gateway_response::Response::new(
                super::super::gateway_response::StatusCode(200),
                vec![],
                reader,
                None,
            ),
        ));
        (receiver, read_completed, worker)
    }

    #[tokio::test]
    async fn native_terminal_chunk_dropped_before_consumption_is_broken_pipe() {
        let (receiver, terminal_read, worker) =
            terminal_response_fixture(b"event: response.completed\n\n".to_vec());
        let response = receiver.await.unwrap();
        terminal_read.await.unwrap();
        assert!(!worker.is_finished(), "queueing is not consumption");
        drop(response);
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .expect("discarding the queued terminal chunk cancels its acknowledgement")
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[tokio::test]
    async fn native_consumed_terminal_chunk_succeeds_after_immediate_body_drop() {
        let bytes = b"event: response.completed\n\n".to_vec();
        let (receiver, terminal_read, worker) = terminal_response_fixture(bytes.clone());
        let mut body = receiver.await.unwrap().into_body().into_data_stream();
        terminal_read.await.unwrap();
        assert_eq!(body.next().await.unwrap().unwrap().as_ref(), bytes);
        // Emulate Codex ending its SSE read as soon as completion arrives.
        drop(body);
        tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .expect("completed delivery must not await provider EOF")
            .unwrap()
            .unwrap();
    }

    fn websocket_terminal_fixture(
        trusted: bool,
    ) -> (
        watch::Sender<bool>,
        oneshot::Receiver<Response<Body>>,
        tokio::task::JoinHandle<io::Result<()>>,
    ) {
        let (confirmation, gate) = WebSocketDeliveryGate::new();
        let mut builder = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .extension(gate);
        if trusted {
            builder = builder.extension(WebSocketHttpBridge);
        }
        let (parts, ()) = builder.body(()).unwrap().into_parts();
        let (request, receiver) = GatewayRequest::new(parts, Bytes::new());
        // All three events fit in one HTTP chunk. Consuming that chunk can
        // precede sending even the first WS event, so it cannot confirm WS EOS.
        let bytes = b"event: response.created\ndata: {\"type\":\"response.created\"}\n\nevent: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\"}\n\n".to_vec();
        let reader = ProtocolTerminalReader {
            bytes: Cursor::new(bytes),
            terminal_read: None,
        };
        let worker = tokio::spawn(request.respond_async(
            super::super::gateway_response::Response::new(
                super::super::gateway_response::StatusCode(200),
                vec![super::super::gateway_response::Header::from_bytes(
                    "content-type",
                    "text/event-stream",
                )
                .unwrap()],
                reader,
                None,
            ),
        ));
        (confirmation, receiver, worker)
    }

    #[tokio::test]
    async fn websocket_http_terminal_consumption_waits_for_socket_confirmation() {
        let (confirmation, receiver, mut worker) = websocket_terminal_fixture(true);
        let mut body = receiver.await.unwrap().into_body().into_data_stream();
        assert!(body.next().await.unwrap().unwrap().ends_with(b"\n\n"));
        drop(body);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut worker)
                .await
                .is_err(),
            "internal HTTP consumption must not finalize a WS turn"
        );
        confirmation.send_replace(true);
        tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn websocket_disconnect_after_http_terminal_consumption_is_broken_pipe() {
        let (confirmation, receiver, worker) = websocket_terminal_fixture(true);
        let mut body = receiver.await.unwrap().into_body().into_data_stream();
        body.next().await.unwrap().unwrap();
        drop(body);
        // Socket send failure, connection close and response.interrupt all drop
        // the per-turn sender without confirming the terminal.
        drop(confirmation);
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[tokio::test]
    async fn websocket_confirmation_does_not_override_unconsumed_http_terminal() {
        let (confirmation, receiver, worker) = websocket_terminal_fixture(true);
        let response = receiver.await.unwrap();
        confirmation.send_replace(true);
        drop(response);
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[tokio::test]
    async fn websocket_delivery_gate_requires_trusted_ingress_marker() {
        let (_confirmation, receiver, worker) = websocket_terminal_fixture(false);
        let mut body = receiver.await.unwrap().into_body().into_data_stream();
        body.next().await.unwrap().unwrap();
        drop(body);
        tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn websocket_missing_terminal_eof_does_not_wait_for_its_own_finalization() {
        let (confirmation, gate) = WebSocketDeliveryGate::new();
        let (parts, ()) = axum::http::Request::builder()
            .method("POST")
            .uri("/v1/responses")
            .extension(WebSocketHttpBridge)
            .extension(gate)
            .body(())
            .unwrap()
            .into_parts();
        let (mut request, receiver) = GatewayRequest::new(parts, Bytes::new());
        let (_finalized, completion) = oneshot::channel();
        request.finish_body_after(completion);
        let incomplete = b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
        let worker = tokio::spawn(request.respond_async(
            super::super::gateway_response::Response::new(
                super::super::gateway_response::StatusCode(200),
                vec![super::super::gateway_response::Header::from_bytes(
                    "content-type", "text/event-stream",
                ).unwrap()],
                Cursor::new(incomplete.to_vec()),
                None,
            ),
        ));
        let response = receiver.await.unwrap();
        let body = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            axum::body::to_bytes(response.into_body(), 4096),
        )
        .await
        .expect("bridge must observe missing-terminal EOF before finalization")
        .unwrap();
        assert_eq!(body.as_ref(), incomplete);
        assert!(
            !worker.is_finished(),
            "EOF alone must not confirm WS delivery"
        );
        // The bridge now reports missing terminal and drops the active turn.
        drop(confirmation);
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[tokio::test]
    async fn native_partially_consumed_terminal_frame_remains_broken_pipe() {
        let bytes = vec![b'x'; 2 * 16 * 1024 + 31];
        let (receiver, terminal_read, worker) = terminal_response_fixture(bytes);
        let mut body = receiver.await.unwrap().into_body().into_data_stream();
        // The first two fragments fill the queue; the terminal fragment is
        // already read but cannot finish its send until capacity is available.
        terminal_read.await.unwrap();
        assert_eq!(body.next().await.unwrap().unwrap().len(), 16 * 1024);
        drop(body);
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .expect("unconsumed terminal fragments must not become successful delivery")
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[tokio::test]
    async fn native_terminal_ack_waits_for_all_queued_frame_fragments() {
        let bytes = (0..2 * 16 * 1024 + 31)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>();
        let (receiver, terminal_read, worker) = terminal_response_fixture(bytes.clone());
        let mut body = receiver.await.unwrap().into_body().into_data_stream();
        terminal_read.await.unwrap();
        let mut received = Vec::new();
        for _ in 0..2 {
            let fragment = body.next().await.unwrap().unwrap();
            assert_eq!(fragment.len(), 16 * 1024);
            received.extend_from_slice(&fragment);
            assert!(
                !worker.is_finished(),
                "earlier fragments cannot acknowledge the terminal fragment"
            );
        }
        let final_fragment = body.next().await.unwrap().unwrap();
        assert_eq!(final_fragment.len(), 31);
        received.extend_from_slice(&final_fragment);
        drop(body);
        tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .expect("consuming the final fragment completes delivery")
            .unwrap()
            .unwrap();
        assert_eq!(received, bytes);
    }

    #[tokio::test]
    async fn native_async_body_is_bounded_and_disconnect_interrupts_delivery() {
        let (request, receiver) = request("POST");
        let reads = Arc::new(AtomicUsize::new(0));
        let reader = InfiniteReader(reads.clone());
        let worker = tokio::spawn(request.respond_async(
            super::super::gateway_response::Response::new(
                super::super::gateway_response::StatusCode(200),
                vec![],
                reader,
                None,
            ),
        ));
        let response = receiver.await.unwrap();
        tokio::task::yield_now().await;
        assert_eq!(reads.load(Ordering::SeqCst), 3);
        drop(response);
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
            .await
            .expect("disconnect cancels a backpressured async producer")
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn native_async_idle_streams_do_not_occupy_blocking_workers() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let (started, mut starts) = mpsc::unbounded_channel();
            let mut workers = Vec::new();
            let mut responses = Vec::new();
            for _ in 0..64 {
                let (request, receiver) = request("POST");
                workers.push(tokio::spawn(request.respond_async(
                    super::super::gateway_response::Response::new(
                        super::super::gateway_response::StatusCode(200),
                        vec![],
                        PendingReader(started.clone()),
                        None,
                    ),
                )));
                responses.push(receiver.await.unwrap());
            }
            for _ in 0..64 {
                starts.recv().await.unwrap();
            }
            assert_eq!(
                tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    tokio::task::spawn_blocking(|| 17),
                )
                .await
                .expect("an idle stream must leave the only blocking worker free")
                .unwrap(),
                17,
            );
            drop(responses);
            for worker in workers {
                let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
                    .await
                    .expect("disconnect cancels an idle provider wait")
                    .unwrap()
                    .unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            }
        });
    }

    #[tokio::test]
    async fn native_async_body_eof_waits_for_accounting_completion() {
        let (mut request, receiver) = request("POST");
        let (finish, completed) = oneshot::channel();
        request.finish_body_after(completed);
        let producer = tokio::spawn(request.respond_async(
            super::super::gateway_response::Response::from_string("completed"),
        ));
        let response = receiver.await.unwrap();
        let mut consumer = tokio::spawn(axum::body::to_bytes(response.into_body(), 1024));
        producer.await.unwrap().unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut consumer)
                .await
                .is_err()
        );
        finish.send(()).unwrap();
        assert_eq!(consumer.await.unwrap().unwrap(), "completed");
    }

    #[tokio::test]
    async fn async_task_scope_cancels_pending_upstream_headers() {
        let (cancel, cancelled) = watch::channel(false);
        let waiting = tokio::spawn(scope_response_cancellation(cancelled, async {
            with_response_cancellation(std::future::pending::<()>()).await
        }));
        tokio::task::yield_now().await;
        cancel.send_replace(true);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
                .await
                .expect("task-scoped cancellation interrupts the pending header wait")
                .unwrap(),
            Err(()),
        );
    }

    #[tokio::test]
    async fn native_sync_terminal_response_runs_on_async_runtime() {
        let (request, receiver) = request("POST");
        request
            .respond(super::super::gateway_response::Response::from_string(
                "terminal",
            ))
            .unwrap();
        let response = receiver.await.unwrap();
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap(),
            "terminal",
        );
    }
    #[tokio::test]
    async fn native_head_suppresses_body_without_reading_provider_stream() {
        let (request, receiver) = request("HEAD");
        let reads = Arc::new(AtomicUsize::new(0));
        let reader = InfiniteReader(reads.clone());
        let worker = tokio::task::spawn_blocking(move || {
            request.respond(super::super::gateway_response::Response::new(
                super::super::gateway_response::StatusCode(200),
                vec![],
                reader,
                Some(23),
            ))
        });
        let response = receiver.await.unwrap();
        assert_eq!(response.headers()["content-length"], "23");
        assert!(axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .is_empty());
        worker.await.unwrap().unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }
}
