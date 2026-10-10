use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http_body::Body as HttpBody;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static DEFAULT_REQUEST_LIMITS: LazyLock<RequestLimits> =
    LazyLock::new(|| RequestLimits::new(256, 64));

#[derive(Clone)]
pub(crate) struct RequestLimits {
    normal: Arc<Semaphore>,
    streaming: Arc<Semaphore>,
}

impl RequestLimits {
    pub(crate) fn new(normal: usize, streaming: usize) -> Self {
        Self {
            normal: Arc::new(Semaphore::new(normal)),
            streaming: Arc::new(Semaphore::new(streaming)),
        }
    }

    pub(crate) fn from_extensions(extensions: &axum::http::Extensions) -> Self {
        extensions
            .get::<Self>()
            .cloned()
            .unwrap_or_else(|| DEFAULT_REQUEST_LIMITS.clone())
    }

    pub(crate) fn try_acquire(&self, streaming: bool) -> Result<OwnedSemaphorePermit, Response> {
        let slots = if streaming {
            &self.streaming
        } else {
            &self.normal
        };
        slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response())
    }
}

struct PermitBody {
    body: Body,
    permit: Option<OwnedSemaphorePermit>,
}

impl HttpBody for PermitBody {
    type Data = bytes::Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let frame = Pin::new(&mut self.body).poll_frame(cx);
        if matches!(frame, Poll::Ready(None) | Poll::Ready(Some(Err(_))))
            || self.body.is_end_stream()
        {
            self.permit.take();
        }
        frame
    }

    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
    }
}

/// Hold a slot until the response reaches EOS, errors, or is dropped by a
/// disconnected client. Forward frames unchanged, including HTTP trailers.
pub(crate) fn hold_response_permit(response: Response, permit: OwnedSemaphorePermit) -> Response {
    let (parts, body) = response.into_parts();
    if body.is_end_stream() {
        drop(permit);
        return Response::from_parts(parts, body);
    }
    Response::from_parts(
        parts,
        Body::new(PermitBody {
            body,
            permit: Some(permit),
        }),
    )
}

fn is_responses_websocket(request: &Request) -> bool {
    request.method() == axum::http::Method::GET
        && request.uri().path() == "/v1/responses"
        && crate::http::responses_websocket::is_websocket_upgrade_request(request.headers())
}

pub(crate) fn defer_gateway_policy(path: &str) -> bool {
    path.starts_with("/v1/")
}

pub(crate) fn accepts_event_stream(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

/// Environment variable controlling the short-request deadline applied by the
/// Axum front proxy. Values are expressed in milliseconds. The default keeps
/// the historical two-minute deadline; setting it to `0` disables this layer
/// (streaming/SSE and WebSocket requests are always exempt).
pub(crate) const ENV_HTTP_TIMEOUT_MS: &str = "CODEXMANAGER_HTTP_TIMEOUT_MS";
const DEFAULT_HTTP_TIMEOUT_MS: u64 = 120_000;

/// Resolve the configured HTTP request timeout for the current process.
///
/// Reading the environment at request time allows the existing settings/env
/// override mechanism to take effect without rebuilding the router. Invalid,
/// negative, or overflowing values fall back to the safe default; `0` means
/// that no short-request deadline is installed.
pub(crate) fn http_request_timeout() -> Option<Duration> {
    http_request_timeout_from(std::env::var(ENV_HTTP_TIMEOUT_MS).ok().as_deref())
}

fn http_request_timeout_from(raw: Option<&str>) -> Option<Duration> {
    let value = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HTTP_TIMEOUT_MS);
    (value > 0).then(|| Duration::from_millis(value))
}

/// Adds a stable request id to every Service HTTP request and response.
/// Existing caller supplied ids are preserved for log correlation.
pub(crate) async fn request_id(mut request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            format!("cm-{}-{sequence}", crate::http::middleware::unix_millis())
        });
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        request.headers_mut().insert("x-request-id", value);
    }
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// Applies a bounded deadline to short HTTP requests while leaving SSE and
/// WebSocket connections under their dedicated gateway idle/total timeouts.
pub(crate) async fn request_timeout(request: Request, next: Next) -> Response {
    let stream_like = accepts_event_stream(request.headers())
        || request.uri().path().starts_with("/events/")
        || is_responses_websocket(&request);
    // The gateway decides JSON stream:true only after bounded body reading
    // and decompression. Its handler applies the ordinary deadline itself.
    if stream_like || defer_gateway_policy(request.uri().path()) {
        return next.run(request).await;
    }
    match http_request_timeout() {
        Some(timeout) => match tokio::time::timeout(timeout, next.run(request)).await {
            Ok(response) => response,
            Err(_) => crate::http::proxy_response::text_error_response(
                axum::http::StatusCode::REQUEST_TIMEOUT,
                "request timed out",
            ),
        },
        None => next.run(request).await,
    }
}

/// Authenticate local RPC/event routes before they consume a request slot.
/// The handlers retain their validation as a defence in depth measure, while
/// this route middleware ensures unauthorised traffic cannot exhaust capacity.
pub(crate) async fn require_rpc_auth(request: Request, next: Next) -> Response {
    if let Some(response) = rpc_auth_error(&request) {
        return response;
    }
    next.run(request).await
}

fn rpc_auth_error(request: &Request) -> Option<Response> {
    // Preserve the RPC handler's historical validation precedence: malformed
    // media types are reported as 415 before authentication is evaluated.
    if request.uri().path() == "/rpc"
        && !request
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return Some((StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}").into_response());
    }
    let authorised = request
        .headers()
        .get("X-CodexManager-Rpc-Token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(crate::rpc_auth_token_matches);
    if !authorised {
        return Some((StatusCode::UNAUTHORIZED, "{}").into_response());
    }
    if request
        .headers()
        .get("Sec-Fetch-Site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("cross-site"))
    {
        return Some((StatusCode::FORBIDDEN, "{}").into_response());
    }
    if request
        .headers()
        .get("Origin")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !is_loopback_origin(value))
    {
        return Some((StatusCode::FORBIDDEN, "{}").into_response());
    }
    None
}

fn is_loopback_origin(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value.trim()) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(address)) => address == std::net::Ipv4Addr::LOCALHOST,
        Some(url::Host::Ipv6(address)) => address == std::net::Ipv6Addr::LOCALHOST,
        None => false,
    }
}

fn is_stream_request(request: &Request) -> bool {
    accepts_event_stream(request.headers()) || request.uri().path().starts_with("/events/")
}

/// Try to reserve one of the independent normal/streaming request slots.
/// Saturation is reported immediately as 503 instead of building an
/// unbounded queue in Tower's concurrency layer.
pub(crate) async fn concurrency_gate(request: Request, next: Next) -> Response {
    if request.uri().path() == "/rpc" || request.uri().path().starts_with("/events/") {
        if let Some(response) = rpc_auth_error(&request) {
            return response;
        }
    }
    // WebSocket lifecycle is governed by responses_websocket's dedicated
    // connection limit; the upgrade response body cannot own that lifecycle.
    if is_responses_websocket(&request) || defer_gateway_policy(request.uri().path()) {
        return next.run(request).await;
    }
    let limits = RequestLimits::from_extensions(request.extensions());
    let permit = match limits.try_acquire(is_stream_request(&request)) {
        Ok(permit) => permit,
        Err(response) => return response,
    };
    hold_response_permit(next.run(request).await, permit)
}

fn unix_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "tests/middleware_policy_tests.rs"]
mod policy_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request as HttpRequest, middleware, routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn request_id_is_generated_and_echoed() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(request_id));
        let response = app
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let value = response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .unwrap();
        assert!(value.starts_with("cm-"));
    }

    #[tokio::test]
    async fn caller_request_id_is_preserved() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(request_id));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("x-request-id", "client-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.headers().get("x-request-id").unwrap(),
            "client-123"
        );
    }

    #[test]
    fn http_timeout_defaults_to_two_minutes() {
        assert_eq!(
            http_request_timeout_from(None),
            Some(Duration::from_secs(120))
        );
    }

    #[test]
    fn http_timeout_zero_disables_deadline() {
        assert_eq!(http_request_timeout_from(Some("0")), None);
    }

    #[test]
    fn http_timeout_invalid_value_uses_default() {
        assert_eq!(
            http_request_timeout_from(Some("not-a-duration")),
            Some(Duration::from_secs(120))
        );
    }

    #[tokio::test]
    async fn configured_timeout_returns_408_with_request_id() {
        let _env_lock = crate::test_env_guard();
        let previous = std::env::var_os(ENV_HTTP_TIMEOUT_MS);
        std::env::set_var(ENV_HTTP_TIMEOUT_MS, "5");
        let app = Router::new()
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    "late"
                }),
            )
            .layer(middleware::from_fn(request_timeout))
            .layer(middleware::from_fn(request_id));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/slow")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::REQUEST_TIMEOUT);
        assert!(response.headers().contains_key("x-request-id"));
        match previous {
            Some(value) => std::env::set_var(ENV_HTTP_TIMEOUT_MS, value),
            None => std::env::remove_var(ENV_HTTP_TIMEOUT_MS),
        }
    }

    #[tokio::test]
    async fn rpc_auth_rejects_missing_token_before_handler() {
        let app = Router::new()
            .route("/rpc", get(|| async { "should not run" }))
            .layer(middleware::from_fn(require_rpc_auth));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/rpc")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rpc_auth_accepts_token_and_rejects_cross_site_origin() {
        let token = crate::rpc_auth_token().to_string();
        let app = Router::new()
            .route("/rpc", get(|| async { "ok" }))
            .layer(middleware::from_fn(require_rpc_auth));
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/rpc")
                    .header("content-type", "application/json")
                    .header("X-CodexManager-Rpc-Token", token)
                    .header("Origin", "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn concurrency_gate_allows_regular_request() {
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn(concurrency_gate));
        let response = app
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
