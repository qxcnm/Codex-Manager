use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response as AxumResponse};
use bytes::BytesMut;
use codexmanager_core::rpc::types::{
    JsonRpcError, JsonRpcErrorObject, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
};
use futures_util::{FutureExt, StreamExt};
#[cfg(test)]
use std::io::Read as _;
use std::panic::AssertUnwindSafe;
#[cfg(test)]
use tiny_http::Request;
#[cfg(test)]
use tiny_http::Response;
use url::Url;

static RPC_BLOCKING_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(32);
static RPC_ASYNC_REQUESTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(64);

/// 函数 `rpc_response_failed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
///
/// # 返回
/// 返回函数执行结果
fn rpc_response_failed(resp: &codexmanager_core::rpc::types::JsonRpcResponse) -> bool {
    if resp.result.get("error").is_some() {
        return true;
    }
    matches!(
        resp.result.get("ok").and_then(|value| value.as_bool()),
        Some(false)
    )
}

/// 函数 `get_header_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn get_header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim())
        .filter(|value| !value.is_empty())
}

/// 函数 `is_json_content_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn is_json_content_type(request: &Request) -> bool {
    get_header_value(request, "Content-Type")
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

#[cfg(test)]
fn rpc_actor_from_request_headers(request: &Request) -> crate::RpcActor {
    crate::RpcActor::from_parts(
        get_header_value(request, "X-CodexManager-Rpc-Actor-Role"),
        get_header_value(request, "X-CodexManager-Rpc-Actor-User-Id"),
    )
}

fn rpc_actor_from_axum_headers(headers: &HeaderMap) -> crate::RpcActor {
    let role = headers
        .get("X-CodexManager-Rpc-Actor-Role")
        .and_then(|value| value.to_str().ok());
    let user_id = headers
        .get("X-CodexManager-Rpc-Actor-User-Id")
        .and_then(|value| value.to_str().ok());
    crate::RpcActor::from_parts(role, user_id)
}

/// 函数 `is_loopback_origin`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - origin: 参数 origin
///
/// # 返回
/// 返回函数执行结果
fn is_loopback_origin(origin: &str) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

/// 函数 `panic_payload_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - payload: 参数 payload
///
/// # 返回
/// 返回函数执行结果
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    "unknown panic payload".to_string()
}

/// 函数 `jsonrpc_message_success`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
fn jsonrpc_message_success(message: &JsonRpcMessage) -> bool {
    match message {
        JsonRpcMessage::Response(resp) => !rpc_response_failed(resp),
        JsonRpcMessage::Notification(_) => true,
        JsonRpcMessage::Error(_) => false,
        JsonRpcMessage::Request(_) => true,
    }
}

/// 函数 `handle_parsed_rpc_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
/// - handler: 参数 handler
///
/// # 返回
/// 返回函数执行结果
fn handle_parsed_rpc_request<F>(req: JsonRpcRequest, handler: F) -> (String, bool)
where
    F: FnOnce(JsonRpcRequest) -> JsonRpcMessage,
{
    let request_id = req.id.clone();
    let request_method = req.method.clone();
    match std::panic::catch_unwind(AssertUnwindSafe(|| handler(req))) {
        Ok(message) => {
            let success = jsonrpc_message_success(&message);
            let json = match message {
                JsonRpcMessage::Notification(_) => String::new(),
                _ => serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string()),
            };
            (json, success)
        }
        Err(payload) => {
            let panic_message = panic_payload_message(payload.as_ref());
            log::error!(
                "rpc handler panicked: method={} id={} panic={}",
                request_method,
                request_id,
                panic_message
            );
            let message = JsonRpcMessage::Error(JsonRpcError {
                id: request_id,
                error: JsonRpcErrorObject {
                    code: -32603,
                    data: None,
                    message: format!("internal_error: {panic_message}"),
                },
            });
            let json = serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string());
            (json, false)
        }
    }
}

/// 函数 `handle_rpc_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn handle_rpc_body(body: &str, actor: crate::RpcActor) -> (u16, String, bool) {
    if body.trim().is_empty() {
        return (400, "{}".to_string(), false);
    }

    let msg: JsonRpcMessage = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return (400, "{}".to_string(), false),
    };
    let (json, success) = match msg {
        JsonRpcMessage::Request(req) => {
            handle_parsed_rpc_request(req, |req| crate::handle_request_with_actor(req, actor))
        }
        JsonRpcMessage::Notification(_) => (String::new(), true),
        JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => {
            return (400, "{}".to_string(), false)
        }
    };
    (200, json, success)
}

/// 函数 `is_axum_json_content_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn is_axum_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get("Content-Type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

/// 函数 `validate_axum_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn validate_axum_headers(headers: &HeaderMap) -> Option<AxumResponse> {
    if !is_axum_json_content_type(headers) {
        return Some((StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}").into_response());
    }

    match headers
        .get("X-CodexManager-Rpc-Token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(token) => {
            if !crate::rpc_auth_token_matches(token) {
                return Some((StatusCode::UNAUTHORIZED, "{}").into_response());
            }
        }
        None => return Some((StatusCode::UNAUTHORIZED, "{}").into_response()),
    }

    if let Some(fetch_site) = headers
        .get("Sec-Fetch-Site")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
    {
        if fetch_site.eq_ignore_ascii_case("cross-site") {
            return Some((StatusCode::FORBIDDEN, "{}").into_response());
        }
    }
    if let Some(origin) = headers
        .get("Origin")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
    {
        if !is_loopback_origin(origin) {
            return Some((StatusCode::FORBIDDEN, "{}").into_response());
        }
    }

    None
}

async fn read_axum_rpc_body_bounded(body: Body) -> Result<String, StatusCode> {
    let mut stream = body.into_data_stream();
    let mut bytes = BytesMut::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
        let next_len = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or(StatusCode::PAYLOAD_TOO_LARGE)?;
        if next_len > crate::RPC_BODY_LIMIT_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| StatusCode::BAD_REQUEST)
}

async fn try_handle_async_network_body(
    body: &str,
    actor: &crate::RpcActor,
) -> Option<(u16, String, bool)> {
    let JsonRpcMessage::Request(request) = serde_json::from_str(body).ok()? else {
        return None;
    };
    if !crate::rpc_dispatch::is_async_method(&request.method) {
        return None;
    }
    let Ok(_permit) = RPC_ASYNC_REQUESTS.try_acquire() else {
        return Some((503, "{}".to_owned(), false));
    };
    let result = AssertUnwindSafe(crate::rpc_dispatch::try_handle_network_request_async(
        &request, actor,
    ))
    .catch_unwind()
    .await;
    let message = match result {
        Ok(message) => message?,
        Err(payload) => {
            let panic_message = panic_payload_message(payload.as_ref());
            log::error!(
                "rpc handler panicked: method={} id={} panic={}",
                request.method,
                request.id,
                panic_message
            );
            JsonRpcMessage::Error(JsonRpcError {
                id: request.id,
                error: JsonRpcErrorObject {
                    code: -32603,
                    data: None,
                    message: format!("internal_error: {panic_message}"),
                },
            })
        }
    };
    let success = jsonrpc_message_success(&message);
    let json = serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_owned());
    Some((200, json, success))
}

/// 函数 `handle_rpc_http`
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
pub(crate) async fn handle_rpc_http(request: axum::extract::Request) -> AxumResponse {
    let state = request
        .extensions()
        .get::<std::sync::Arc<super::state::AppState>>()
        .cloned();
    let mut rpc_metrics_guard = crate::gateway::begin_rpc_request();
    let headers = request.headers();
    if let Some(response) = validate_axum_headers(headers) {
        return response;
    }
    let actor = rpc_actor_from_axum_headers(headers);
    if headers
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .is_some_and(|length| length > crate::RPC_BODY_LIMIT_BYTES as u64)
    {
        return (StatusCode::PAYLOAD_TOO_LARGE, "{}").into_response();
    }
    let body_for_task = match read_axum_rpc_body_bounded(request.into_body()).await {
        Ok(body) => body,
        Err(status) => return (status, "{}").into_response(),
    };
    if let Some(state) = state {
        if let Ok(JsonRpcMessage::Request(req)) = serde_json::from_str(&body_for_task) {
            if let Some(message) =
                crate::rpc_dispatch::storage_async::handle(state, &req, &actor).await
            {
                if jsonrpc_message_success(&message) {
                    rpc_metrics_guard.mark_success();
                }
                return (
                    StatusCode::OK,
                    serde_json::to_string(&message).unwrap_or_else(|_| "{}".into()),
                )
                    .into_response();
            }
        }
    }
    if let Some((status, body, success)) =
        try_handle_async_network_body(&body_for_task, &actor).await
    {
        if success {
            rpc_metrics_guard.mark_success();
        }
        return (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), body).into_response();
    }
    let permit = match RPC_BLOCKING_WORKERS.try_acquire() {
        Ok(permit) => permit,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, "{}").into_response(),
    };
    let (status, response_body, success) = match crate::runtime::blocking::run("rpc", move || {
        // A cancelled HTTP future must not release capacity while a
        // synchronous dispatch is still executing.
        let _permit = permit;
        handle_rpc_body(&body_for_task, actor)
    })
    .await
    {
        Ok(result) => result,
        Err(err) => {
            log::error!("rpc http blocking task failed: {}", err);
            let fallback = JsonRpcResponse {
                id: 0.into(),
                result: crate::error_codes::rpc_error_payload(
                    "internal_error: rpc task failed".to_string(),
                ),
            };
            let body = serde_json::to_string(&fallback).unwrap_or_else(|_| "{}".to_string());
            (200, body, false)
        }
    };
    if success {
        rpc_metrics_guard.mark_success();
    }
    (
        StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
        response_body,
    )
        .into_response()
}

/// 函数 `handle_rpc`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
///
/// # 返回
/// 无
#[cfg(test)]
pub fn handle_rpc(mut request: Request) {
    let mut rpc_metrics_guard = crate::gateway::begin_rpc_request();
    if request.method().as_str() != "POST" {
        let _ = request.respond(Response::from_string("{}").with_status_code(405));
        return;
    }
    if !is_json_content_type(&request) {
        let _ = request.respond(Response::from_string("{}").with_status_code(415));
        return;
    }

    match get_header_value(&request, "X-CodexManager-Rpc-Token") {
        Some(token) => {
            if !crate::rpc_auth_token_matches(token) {
                let _ = request.respond(Response::from_string("{}").with_status_code(401));
                return;
            }
        }
        None => {
            let _ = request.respond(Response::from_string("{}").with_status_code(401));
            return;
        }
    }

    if let Some(fetch_site) = get_header_value(&request, "Sec-Fetch-Site") {
        if fetch_site.eq_ignore_ascii_case("cross-site") {
            let _ = request.respond(Response::from_string("{}").with_status_code(403));
            return;
        }
    }
    if let Some(origin) = get_header_value(&request, "Origin") {
        if !is_loopback_origin(origin) {
            let _ = request.respond(Response::from_string("{}").with_status_code(403));
            return;
        }
    }

    if let Some(content_length) =
        get_header_value(&request, "Content-Length").and_then(|value| value.parse::<usize>().ok())
    {
        if content_length > crate::RPC_BODY_LIMIT_BYTES {
            let _ = request.respond(Response::from_string("{}").with_status_code(413));
            return;
        }
    }

    let actor = rpc_actor_from_request_headers(&request);
    let mut body = String::new();
    let read_result = request
        .as_reader()
        .take(crate::RPC_BODY_LIMIT_BYTES.saturating_add(1) as u64)
        .read_to_string(&mut body);
    if read_result.is_err() {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }
    if body.len() > crate::RPC_BODY_LIMIT_BYTES {
        let _ = request.respond(Response::from_string("{}").with_status_code(413));
        return;
    }
    if body.trim().is_empty() {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }

    let (status, response_body, success) = handle_rpc_body(&body, actor);
    if success {
        rpc_metrics_guard.mark_success();
    }
    let _ = request.respond(Response::from_string(response_body).with_status_code(status));
}

#[cfg(test)]
#[path = "rpc_endpoint_tests.rs"]
mod tests;
