use axum::body::{to_bytes, Body};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, Request as HttpRequest, Response, StatusCode};
use axum::routing::{any, get, post};
use axum::Router;
use bytes::Bytes;
use reqwest::Client;
use std::io;
use std::io::Read;

use crate::http::proxy_bridge::run_proxy_server;
use crate::http::proxy_request::{build_target_url, filter_request_headers};
use crate::http::proxy_response::{merge_upstream_headers, text_error_response};

const DEFAULT_FRONT_PROXY_MAX_BLOCKING_THREADS: usize = 32;
const DEFAULT_FRONT_PROXY_WORKER_THREADS: usize = 2;
const ZSTD_MAX_CONCURRENT_DECODES: usize = 4;
const ENV_FRONT_PROXY_MAX_BLOCKING_THREADS: &str = "CODEXMANAGER_FRONT_PROXY_MAX_BLOCKING_THREADS";
const ENV_FRONT_PROXY_WORKER_THREADS: &str = "CODEXMANAGER_FRONT_PROXY_WORKER_THREADS";

static ZSTD_DECODE_SEMAPHORE: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(ZSTD_MAX_CONCURRENT_DECODES);

#[derive(Clone)]
struct ProxyState {
    backend_base_url: String,
    client: Client,
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
    log::warn!(
        "event=front_proxy_error code={} status={} target_url={} message={}",
        crate::error_codes::classify_message(message).as_str(),
        status.as_u16(),
        target_url,
        message
    );
}

/// 函数 `build_backend_base_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - backend_addr: 参数 backend_addr
///
/// # 返回
/// 返回函数执行结果
fn build_backend_base_url(backend_addr: &str) -> String {
    format!("http://{backend_addr}")
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

fn front_proxy_max_blocking_threads() -> usize {
    env_usize_or(
        ENV_FRONT_PROXY_MAX_BLOCKING_THREADS,
        crate::storage_helpers::storage_max_connections()
            .min(DEFAULT_FRONT_PROXY_MAX_BLOCKING_THREADS),
    )
    .max(1)
}

fn front_proxy_worker_threads() -> usize {
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
    let (parts, body) = request.into_parts();
    let prefer_raw_errors = crate::gateway::prefers_raw_errors_for_http_headers(&parts.headers);
    let target_url = build_target_url(&state.backend_base_url, &parts.uri);
    let max_body_bytes = crate::gateway::front_proxy_max_body_bytes();
    let zstd_max_body_bytes = crate::gateway::front_proxy_zstd_max_body_bytes();
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
    let body_bytes = match to_bytes(body, read_limit).await {
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

    let mut builder = state.client.request(parts.method, target_url.as_str());
    builder = builder.headers(outbound_headers);
    builder = builder.body(body_bytes);

    let upstream = match builder.send().await {
        Ok(response) => response,
        Err(err) => {
            let message = crate::gateway::bilingual_error(
                "后端代理请求失败",
                format!("backend proxy error: {err}"),
            );
            log_proxy_error(
                StatusCode::BAD_GATEWAY,
                target_url.as_str(),
                message.as_str(),
            );
            return text_error_response(
                StatusCode::BAD_GATEWAY,
                crate::gateway::error_message_for_client(prefer_raw_errors, message),
            );
        }
    };

    let response_builder = merge_upstream_headers(
        Response::builder().status(upstream.status()),
        upstream.headers(),
    );

    match response_builder.body(Body::from_stream(upstream.bytes_stream())) {
        Ok(response) => response,
        Err(err) => {
            let message = crate::gateway::bilingual_error(
                "构建响应失败",
                format!("build response failed: {err}"),
            );
            log_proxy_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                target_url.as_str(),
                message.as_str(),
            );
            text_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::gateway::error_message_for_client(prefer_raw_errors, message),
            )
        }
    }
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
fn build_front_proxy_app(state: ProxyState) -> Router {
    Router::new()
        .route(
            "/rpc",
            post(crate::http::rpc_endpoint::handle_rpc_http)
                .layer(DefaultBodyLimit::max(crate::RPC_BODY_LIMIT_BYTES)),
        )
        .route(
            "/events/usage-refresh",
            get(crate::http::usage_events::handle_usage_refresh_events_http),
        )
        .route(
            "/events/account-test",
            get(crate::http::account_test_events::handle_account_test_events_http),
        )
        .route("/v1/responses", any(responses_handler))
        .route("/proxy-test-upload", post(proxy_test_upload))
        .fallback(any(proxy_handler))
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
pub(crate) fn run_front_proxy(addr: &str, backend_addr: &str) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(front_proxy_worker_threads())
        .max_blocking_threads(front_proxy_max_blocking_threads())
        .enable_all()
        .build()
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    runtime.block_on(async move {
        let client = build_local_backend_client()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        let state = ProxyState {
            backend_base_url: build_backend_base_url(backend_addr),
            client,
        };
        let app = build_front_proxy_app(state);
        run_proxy_server(addr, app).await
    })
}

#[cfg(test)]
#[path = "tests/proxy_runtime_tests.rs"]
mod tests;
