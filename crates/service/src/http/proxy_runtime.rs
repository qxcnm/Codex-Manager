use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{header, HeaderMap, Request as HttpRequest, Response, StatusCode};
use axum::routing::{any, get, post};
use axum::Router;
use bytes::Bytes;
use http_body::Body as HttpBody;
#[cfg(test)]
use reqwest::Client;
use std::io;
use std::io::Read;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::http::proxy_bridge::run_proxy_server;
#[cfg(test)]
use crate::http::proxy_request::build_target_url;
use crate::http::proxy_request::filter_request_headers;
#[cfg(test)]
use crate::http::proxy_response::merge_upstream_headers;
use crate::http::proxy_response::text_error_response;

const DEFAULT_FRONT_PROXY_MAX_BLOCKING_THREADS: usize = 32;
const DEFAULT_FRONT_PROXY_WORKER_THREADS: usize = 2;
const ZSTD_MAX_CONCURRENT_DECODES: usize = 4;
const ENV_FRONT_PROXY_MAX_BLOCKING_THREADS: &str = "CODEXMANAGER_FRONT_PROXY_MAX_BLOCKING_THREADS";
const ENV_FRONT_PROXY_WORKER_THREADS: &str = "CODEXMANAGER_FRONT_PROXY_WORKER_THREADS";

static ZSTD_DECODE_SEMAPHORE: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(ZSTD_MAX_CONCURRENT_DECODES);
static GATEWAY_PREPARATION_SLOTS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(32)));

/// Tracks whether a streamed proxy response reached a terminal frame. A body
/// dropped before EOS means the downstream client disconnected; record it as
/// HTTP 499 so cancellation remains visible while the legacy backend is
/// being retired.
struct DisconnectTrackingBody {
    body: Body,
    path: String,
    completed: bool,
}

impl HttpBody for DisconnectTrackingBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let frame = Pin::new(&mut self.body).poll_frame(cx);
        if matches!(frame, Poll::Ready(None) | Poll::Ready(Some(Err(_)))) {
            self.completed = true;
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

impl Drop for DisconnectTrackingBody {
    fn drop(&mut self) {
        if !self.completed {
            crate::gateway::record_gateway_request_outcome(
                self.path.as_str(),
                499,
                Some("front_proxy"),
            );
            log::info!(
                "event=front_proxy_client_disconnect path={} status=499",
                self.path
            );
        }
    }
}

async fn classify_gateway_stream(
    body: Bytes,
    accept_stream: bool,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<bool, Response<Body>> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        // Ignore all other fields rather than allocating a second full JSON
        // tree. Serde's depth limit also bounds nested-object traversal.
        #[derive(serde::Deserialize)]
        struct StreamFlag {
            #[serde(default)]
            stream: bool,
        }
        accept_stream || serde_json::from_slice::<StreamFlag>(&body).is_ok_and(|value| value.stream)
    })
    .await
    .map_err(|_| {
        text_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "request classification failed",
        )
    })
}

#[derive(Clone, Default)]
pub(crate) struct ProxyState {
    // Only policy tests can substitute an isolated backend fixture.
    #[cfg(test)]
    pub(crate) backend_base_url: String,
    #[cfg(test)]
    pub(crate) client: Client,
}

#[allow(dead_code)]
fn build_backend_base_url(backend_addr: &str) -> String {
    format!("http://{backend_addr}")
}

/// 函数 `log_proxy_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - target_url: 参数 target_url
/// - message: 参数 message
///
/// # 返回
/// 无
fn log_proxy_error(status: StatusCode, target_url: &str, message: &str) {
    let target_path = url::Url::parse(target_url)
        .map(|url| url.path().to_owned())
        .unwrap_or_else(|_| "invalid-target".to_owned());
    log::warn!(
        "event=front_proxy_error code={} status={} target_path={} message={}",
        crate::error_codes::classify_message(message).as_str(),
        status.as_u16(),
        target_path,
        message
    );
}

fn make_http_span(request: &HttpRequest<Body>) -> tracing::Span {
    // URI queries may contain OAuth codes/state. Never record the full URI,
    // credentials, headers, or request/response bodies in the Tower span.
    tracing::info_span!(
        "http_request",
        method = %request.method(),
        path = %request.uri().path(),
        request_id = request.headers().get("x-request-id").and_then(|value| value.to_str().ok()).unwrap_or("")
    )
}

/// 函数 `build_local_backend_client`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn build_local_backend_client() -> Result<Client, reqwest::Error> {
    Client::builder().no_proxy().build()
}

fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

pub(crate) fn front_proxy_max_blocking_threads() -> usize {
    env_usize_or(
        ENV_FRONT_PROXY_MAX_BLOCKING_THREADS,
        crate::storage_helpers::storage_max_connections()
            .min(DEFAULT_FRONT_PROXY_MAX_BLOCKING_THREADS),
    )
    .max(1)
}

pub(crate) fn front_proxy_worker_threads() -> usize {
    env_usize_or(
        ENV_FRONT_PROXY_WORKER_THREADS,
        DEFAULT_FRONT_PROXY_WORKER_THREADS,
    )
    .max(1)
}

#[derive(Debug)]
struct IncomingBodyDecodeError {
    status: StatusCode,
    message: String,
}

fn has_zstd_content_encoding(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| value.trim().eq_ignore_ascii_case("zstd"))
}

fn has_zstd_magic(body: &[u8]) -> bool {
    body.starts_with(&[0x28, 0xB5, 0x2F, 0xFD])
}

fn zstd_body_limit(max_body_bytes: usize, zstd_max_body_bytes: usize) -> usize {
    let zstd_max_body_bytes = zstd_max_body_bytes.max(1);
    if max_body_bytes == 0 {
        zstd_max_body_bytes
    } else {
        max_body_bytes.min(zstd_max_body_bytes)
    }
}

fn try_acquire_zstd_decode_permit(
) -> Result<tokio::sync::SemaphorePermit<'static>, IncomingBodyDecodeError> {
    ZSTD_DECODE_SEMAPHORE
        .try_acquire()
        .map_err(|_| IncomingBodyDecodeError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: crate::gateway::bilingual_error(
                "zstd 解压任务繁忙",
                "zstd request decoder is busy; retry later",
            ),
        })
}

fn decode_zstd_body(body: &[u8], decode_limit: usize) -> Result<Vec<u8>, IncomingBodyDecodeError> {
    let decoder =
        zstd::stream::read::Decoder::new(body).map_err(|err| IncomingBodyDecodeError {
            status: StatusCode::BAD_REQUEST,
            message: crate::gateway::bilingual_error(
                "zstd 请求体解压失败",
                format!("invalid zstd request body: {err}"),
            ),
        })?;
    let mut decoded = Vec::new();
    let mut limited = decoder.take(decode_limit.saturating_add(1) as u64);
    let read_result = limited.read_to_end(&mut decoded);
    read_result.map_err(|err| IncomingBodyDecodeError {
        status: StatusCode::BAD_REQUEST,
        message: crate::gateway::bilingual_error(
            "zstd 请求体解压失败",
            format!("invalid zstd request body: {err}"),
        ),
    })?;
    if decoded.len() > decode_limit {
        return Err(IncomingBodyDecodeError {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            message: crate::gateway::bilingual_error(
                "请求体过大",
                format!("request body too large after zstd decompression: >{decode_limit}"),
            ),
        });
    }
    Ok(decoded)
}

async fn normalize_incoming_request_body(
    headers: &mut HeaderMap,
    body: Bytes,
    decode_limit: usize,
    decode_permit: Option<tokio::sync::SemaphorePermit<'static>>,
) -> Result<Bytes, IncomingBodyDecodeError> {
    if body.is_empty() || (!has_zstd_content_encoding(headers) && !has_zstd_magic(body.as_ref())) {
        return Ok(body);
    }

    let permit = match decode_permit {
        Some(permit) => permit,
        None => try_acquire_zstd_decode_permit()?,
    };
    let decoded = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        decode_zstd_body(body.as_ref(), decode_limit)
    })
    .await
    .map_err(|err| IncomingBodyDecodeError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: crate::gateway::bilingual_error(
            "zstd 解压任务失败",
            format!("zstd request decoder task failed: {err}"),
        ),
    })??;
    headers.remove(header::CONTENT_ENCODING);
    headers.remove(header::CONTENT_LENGTH);
    Ok(Bytes::from(decoded))
}

/// 函数 `proxy_handler`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - State(state): 参数 State(state)
/// - request: 参数 request
///
/// # 返回
/// 返回函数执行结果
async fn proxy_handler(
    State(state): State<ProxyState>,
    request: HttpRequest<Body>,
) -> Response<Body> {
    let started = std::time::Instant::now();
    let (mut parts, body) = request.into_parts();
    let deferred_policy = crate::http::middleware::defer_gateway_policy(parts.uri.path());
    let limits = crate::http::middleware::RequestLimits::from_extensions(&parts.extensions);
    // Bound requests retained while the body is read/decompressed/classified.
    // No async worker parses a potentially large JSON document synchronously.
    let preparation_permit = match GATEWAY_PREPARATION_SLOTS.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => return text_error_response(StatusCode::SERVICE_UNAVAILABLE, "service busy"),
    };
    // Runtime configuration no longer constructs blocking HTTP clients.
    let max_body_bytes = crate::gateway::front_proxy_max_body_bytes();
    let zstd_max_body_bytes = crate::gateway::front_proxy_zstd_max_body_bytes();
    let prefer_raw_errors = crate::gateway::prefers_raw_errors_for_http_headers(&parts.headers);
    let target_url = format!("http://gateway.local{}", parts.uri.path());
    let zstd_decode_limit = zstd_body_limit(max_body_bytes, zstd_max_body_bytes);
    let declared_zstd = has_zstd_content_encoding(&parts.headers);
    let zstd_decode_permit = if declared_zstd {
        match try_acquire_zstd_decode_permit() {
            Ok(permit) => Some(permit),
            Err(err) => {
                log_proxy_error(err.status, target_url.as_str(), err.message.as_str());
                return text_error_response(
                    err.status,
                    crate::gateway::error_message_for_client(prefer_raw_errors, err.message),
                );
            }
        }
    } else {
        None
    };
    let request_body_limit = if declared_zstd {
        zstd_decode_limit
    } else {
        max_body_bytes
    };

    if let Some(content_length) = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
    {
        if request_body_limit > 0 && content_length > request_body_limit as u64 {
            let message = crate::gateway::bilingual_error(
                "请求体过大",
                format!("request body too large: content-length={content_length}"),
            );
            log_proxy_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                target_url.as_str(),
                message.as_str(),
            );
            return text_error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                crate::gateway::error_message_for_client(prefer_raw_errors, message),
            );
        }
    }

    let mut outbound_headers = filter_request_headers(&parts.headers);
    let read_limit = if request_body_limit == 0 {
        usize::MAX
    } else {
        request_body_limit
    };
    // Even a request that will eventually ask for SSE must finish uploading
    // its input. Exempt only the response stream, not an unfinished upload.
    let body_read = match crate::http::middleware::http_request_timeout() {
        Some(timeout) => match tokio::time::timeout(
            timeout.saturating_sub(started.elapsed()),
            to_bytes(body, read_limit),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => return text_error_response(StatusCode::REQUEST_TIMEOUT, "request timed out"),
        },
        None => to_bytes(body, read_limit).await,
    };
    let body_bytes = match body_read {
        Ok(bytes) => bytes,
        Err(_) => {
            let message = if request_body_limit == 0 {
                crate::gateway::bilingual_error("请求体过大", "request body too large")
            } else {
                crate::gateway::bilingual_error(
                    "请求体过大",
                    format!("request body too large: content-length>{request_body_limit}"),
                )
            };
            log_proxy_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                target_url.as_str(),
                message.as_str(),
            );
            return text_error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                crate::gateway::error_message_for_client(prefer_raw_errors, message),
            );
        }
    };
    let encoded_body_bytes = body_bytes.len();
    let body_bytes = match normalize_incoming_request_body(
        &mut outbound_headers,
        body_bytes,
        zstd_decode_limit,
        zstd_decode_permit,
    )
    .await
    {
        Ok(body) => body,
        Err(err) => {
            log_proxy_error(err.status, target_url.as_str(), err.message.as_str());
            return text_error_response(
                err.status,
                crate::gateway::error_message_for_client(prefer_raw_errors, err.message),
            );
        }
    };
    if body_bytes.len() != encoded_body_bytes {
        log::info!(
            "event=front_proxy_request_decompressed path={} algorithm=zstd pre_bytes={} post_bytes={}",
            parts.uri.path(),
            encoded_body_bytes,
            body_bytes.len()
        );
    }

    let mut permit = None;
    let timeout = if deferred_policy {
        let streaming = match classify_gateway_stream(
            body_bytes.clone(),
            crate::http::middleware::accepts_event_stream(&parts.headers),
            preparation_permit,
        )
        .await
        {
            Ok(streaming) => streaming,
            Err(response) => return response,
        };
        permit = match limits.try_acquire(streaming) {
            Ok(permit) => Some(permit),
            Err(response) => return response,
        };
        if streaming {
            None
        } else {
            crate::http::middleware::http_request_timeout()
        }
    } else {
        drop(preparation_permit);
        None
    };

    parts.headers = outbound_headers;
    let path = parts.uri.path().to_owned();
    let dispatch = dispatch_gateway(state, parts, body_bytes);
    let response = match timeout {
        Some(timeout) => {
            match tokio::time::timeout(timeout.saturating_sub(started.elapsed()), dispatch).await {
                Ok(result) => result,
                Err(_) => {
                    return text_error_response(StatusCode::REQUEST_TIMEOUT, "request timed out")
                }
            }
        }
        None => dispatch.await,
    };
    let (response_parts, response_body) = response.into_parts();
    let response_body = if let Some(timeout) = timeout {
        match tokio::time::timeout(
            timeout.saturating_sub(started.elapsed()),
            to_bytes(response_body, usize::MAX),
        )
        .await
        {
            Ok(Ok(body)) => Body::from(body),
            Ok(Err(_)) => {
                return text_error_response(StatusCode::BAD_GATEWAY, "gateway response body failed")
            }
            Err(_) => return text_error_response(StatusCode::REQUEST_TIMEOUT, "request timed out"),
        }
    } else {
        Body::new(DisconnectTrackingBody {
            body: response_body,
            path,
            completed: false,
        })
    };
    let response = Response::from_parts(response_parts, response_body);
    match permit {
        Some(permit) => crate::http::middleware::hold_response_permit(response, permit),
        None => response,
    }
}

async fn dispatch_gateway(
    _state: ProxyState,
    parts: axum::http::request::Parts,
    body: Bytes,
) -> Response<Body> {
    #[cfg(test)]
    if !_state.backend_base_url.is_empty() {
        let url = build_target_url(&_state.backend_base_url, &parts.uri);
        return match _state
            .client
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body)
            .send()
            .await
        {
            Ok(upstream) => merge_upstream_headers(
                Response::builder().status(upstream.status()),
                upstream.headers(),
            )
            .body(Body::from_stream(upstream.bytes_stream()))
            .unwrap(),
            Err(_) => text_error_response(StatusCode::BAD_GATEWAY, "test backend unavailable"),
        };
    }
    crate::http::gateway_endpoint::handle_gateway_http(parts, body).await
}

async fn responses_handler(
    State(state): State<ProxyState>,
    request: HttpRequest<Body>,
) -> Response<Body> {
    if request.method() == axum::http::Method::GET
        && crate::http::responses_websocket::is_websocket_upgrade_request(request.headers())
    {
        return crate::http::responses_websocket::upgrade_responses_websocket(request).await;
    }
    proxy_handler(State(state), request).await
}

/// Lightweight liveness endpoint served by the Axum edge.  Keeping this out
/// of the blocking compatibility backend means health probes continue to
/// succeed while the legacy request queues are saturated.
async fn health_handler() -> &'static str {
    "ok"
}

/// Prometheus metrics endpoint served directly from the in-process registry.
async fn metrics_handler() -> Response<Body> {
    let body = crate::gateway::gateway_metrics_prometheus();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; version=0.0.4")
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::from("metrics unavailable")))
}

/// Reference upload endpoint for proxy testing.
/// Reads and discards client payload stream, limiting the body size to prevent memory exhaustion.
///
/// NOTE: This localhost route is for development/mocking purposes only. In a self-hosted or production
/// deployment, the actual upload endpoint must be reachable through the proxy egress. Localhost
/// targets cannot measure real proxy upload throughput.
async fn proxy_test_upload(
    req: axum::extract::Request,
) -> Result<StatusCode, (StatusCode, String)> {
    use futures_util::StreamExt;

    const MAX_UPLOAD_BYTES: u64 = 110_000_000;
    let mut total = 0u64;
    let mut stream = req.into_body().into_data_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        total += chunk.len() as u64;
        if total > MAX_UPLOAD_BYTES {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "body too large".into()));
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 函数 `build_front_proxy_app`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - state: 参数 state
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
pub(crate) fn build_front_proxy_app(state: ProxyState) -> Router {
    build_front_proxy_app_with_limits(state, crate::http::middleware::RequestLimits::new(256, 64))
}

pub(crate) fn build_front_proxy_app_with_limits(
    state: ProxyState,
    limits: crate::http::middleware::RequestLimits,
) -> Router {
    // Keep liveness/metrics outside the gateway concurrency gate so probes
    // remain responsive when request workers are saturated.
    let gateway_routes = Router::new()
        .route(
            "/rpc",
            post(crate::http::rpc_endpoint::handle_rpc_http)
                .layer(RequestBodyLimitLayer::new(crate::RPC_BODY_LIMIT_BYTES))
                .layer(axum::middleware::from_fn(
                    crate::http::middleware::require_rpc_auth,
                )),
        )
        .route(
            "/events/usage-refresh",
            get(crate::http::usage_events::handle_usage_refresh_events_http).layer(
                axum::middleware::from_fn(crate::http::middleware::require_rpc_auth),
            ),
        )
        .route(
            "/events/account-test",
            get(crate::http::account_test_events::handle_account_test_events_http).layer(
                axum::middleware::from_fn(crate::http::middleware::require_rpc_auth),
            ),
        )
        .route(
            "/auth/callback",
            get(crate::http::callback_endpoint::handle_callback_http),
        )
        .route("/v1/responses", any(responses_handler))
        .route("/v1/chat/completions", any(proxy_handler))
        .route("/proxy-test-upload", post(proxy_test_upload))
        .fallback(any(proxy_handler))
        .layer(axum::middleware::from_fn(
            crate::http::middleware::concurrency_gate,
        ))
        .layer(CatchPanicLayer::custom(
            |_panic: Box<dyn std::any::Any + Send>| {
                // Panic payloads may contain request credentials or body data;
                // never include the payload in logs. The request-id middleware
                // still provides correlation for diagnostics.
                log::error!("event=http_handler_panic message=panic in request handler");
                text_error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
            },
        ));

    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route(
            "/__shutdown",
            get(crate::http::shutdown_endpoint::shutdown).layer(axum::middleware::from_fn(
                crate::http::middleware::require_rpc_auth,
            )),
        )
        .merge(gateway_routes)
        .layer(TraceLayer::new_for_http().make_span_with(make_http_span))
        .layer(axum::middleware::from_fn(
            crate::http::middleware::request_timeout,
        ))
        // Add request-id last so even timeout/panic responses carry the same
        // correlation header that was generated for the incoming request.
        .layer(axum::middleware::from_fn(
            crate::http::middleware::request_id,
        ))
        .layer(axum::Extension(limits))
        .with_state(state)
}

/// 函数 `run_front_proxy`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn front_proxy_runtime() -> io::Result<&'static tokio::runtime::Runtime> {
    // Shared HTTP clients keep connection drivers on the runtime that opened
    // them. Listener shutdown must not destroy that runtime while background
    // authentication or a restarted listener can reuse those connections.
    // Thread limits are startup settings, captured by the first listener.
    crate::runtime::service_runtime::process_runtime().map_err(io::Error::other)
}

pub(crate) fn run_front_proxy(addr: &str) -> io::Result<()> {
    let runtime = front_proxy_runtime()?;

    runtime.block_on(async move {
        let app = crate::http::router::build_router(crate::http::router::AppState::new());
        run_proxy_server(addr, app).await
    })
}

#[cfg(test)]
#[path = "tests/proxy_runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/proxy_policy_tests.rs"]
mod policy_tests;
