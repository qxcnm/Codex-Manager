//! Transport-independent input for the gateway's routing and delivery code.
//!
//! Axum owns the only production socket. The bounded response channel preserves
//! backpressure and reports a closed HTTP body to async delivery, so accounting
//! and finalization run on downstream cancellation too. Short terminal responses
//! retain a synchronous helper for in-memory bodies.
use std::cell::RefCell;
use std::future::Future;
use std::io::{self, Cursor, Read};

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
    uri: String,
    headers: Vec<super::gateway_response::Header>,
    remote_addr: Option<std::net::SocketAddr>,
    body: Cursor<Bytes>,
    response: oneshot::Sender<Response<Body>>,
    cancellation: watch::Sender<bool>,
    cancelled: watch::Receiver<bool>,
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
        let (sender, receiver) = mpsc::channel::<io::Result<Bytes>>(2);
        let stream = futures_util::stream::unfold(
            (receiver, self.cancellation_guard(), self.completion.take()),
            |(mut receiver, cancellation, completion)| async move {
                match receiver.recv().await {
                    Some(chunk) => Some((chunk, (receiver, cancellation, completion))),
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
                    let _ = sender.send(Err(error())).await;
                    break Err(error());
                }
                Ok(0) => break Ok(()),
                Ok(length) => {
                    if sender
                        .send(Ok(Bytes::copy_from_slice(&buffer[..length])))
                        .await
                        .is_err()
                    {
                        break Err(disconnected());
                    }
                    written = written.saturating_add(length);
                }
                Err(error) => {
                    let _ = sender
                        .send(Err(io::Error::new(
                            error.kind(),
                            "gateway response body failed",
                        )))
                        .await;
                    break Err(error);
                }
            }
        };
        reader.finish_async().await;
        delivery
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
