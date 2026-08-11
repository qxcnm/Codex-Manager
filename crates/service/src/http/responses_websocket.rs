use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::FromRequestParts;
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::{Request as HttpRequest, Response, StatusCode};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::{
    Request as WsClientRequest, Response as WsClientResponse,
};
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;
use tokio_tungstenite::{client_async_tls_with_config, connect_async_tls_with_config};

use crate::http::codex_source::{
    response_create_client_metadata, ResponseCreateWsRequest, ResponsesWsRequest,
    RESPONSES_ENDPOINT, X_CODEX_PARENT_THREAD_ID_HEADER, X_CODEX_TURN_METADATA_HEADER,
    X_CODEX_WINDOW_ID_HEADER, X_OPENAI_SUBAGENT_HEADER,
};
use crate::http::proxy_response::{text_error_response, text_response};
use crate::storage_helpers::{hash_platform_key, open_storage};

const RESPONSES_WS_ERROR_CODE: &str = "responses_websocket_error";
const RESPONSES_WEBSOCKETS_BETA_HEADER_VALUE: &str = "responses_websockets=2026-02-06";

struct WsRequestContext {
    api_key: codexmanager_core::storage::ApiKey,
    incoming_headers: crate::gateway::IncomingHeaderSnapshot,
    prompt_cache_key: Option<String>,
    effective_upstream_base: String,
    prefer_raw_errors: bool,
    key_policy: codexmanager_core::storage::ApiKeyPolicy,
    _api_key_inflight_guard: crate::gateway::ApiKeyInFlightGuard,
}

#[derive(Clone)]
struct PreparedClientFrame {
    text: String,
    client_model: Option<String>,
    model: Option<String>,
    model_source: Option<String>,
    client_reasoning_effort: Option<String>,
    reasoning_effort: Option<String>,
    reasoning_source: Option<String>,
    service_tier: Option<String>,
    effective_service_tier: Option<String>,
    service_tier_source: Option<String>,
    raw_service_tier: Option<String>,
    has_service_tier_field: bool,
}

struct PendingWsRequestState {
    log: PendingWsRequestLog,
    prepared: PreparedClientFrame,
    forwarded_upstream_event: bool,
}

struct ConnectedUpstreamWebsocket {
    stream: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
    account_id: String,
    upstream_url: String,
    route_strategy: &'static str,
    route_source: &'static str,
}

#[derive(Debug)]
pub(crate) struct WsConnectError {
    message: String,
    status_code: Option<u16>,
}

impl WsConnectError {
    fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: None,
        }
    }

    fn from_tungstenite(err: tokio_tungstenite::tungstenite::Error) -> Self {
        let status_code = match &err {
            tokio_tungstenite::tungstenite::Error::Http(response) => {
                Some(response.status().as_u16())
            }
            _ => None,
        };
        Self {
            message: err.to_string(),
            status_code,
        }
    }

    fn is_unauthorized(&self) -> bool {
        self.status_code == Some(401)
    }
}

impl fmt::Display for WsConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

struct WebsocketTarget {
    host: String,
    port: u16,
    authority: String,
}

struct PendingWsRequestLog {
    trace_id: String,
    route_strategy: Option<String>,
    route_source: Option<String>,
    client_model: Option<String>,
    model: Option<String>,
    model_source: Option<String>,
    client_reasoning_effort: Option<String>,
    reasoning_effort: Option<String>,
    reasoning_source: Option<String>,
    service_tier: Option<String>,
    effective_service_tier: Option<String>,
    service_tier_source: Option<String>,
    started_at: Instant,
    first_response_ms: Option<i64>,
}

struct WsSessionError {
    status: u16,
    code: String,
    message: String,
}

impl WsSessionError {
    fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(400, "invalid_request_error", message)
    }

    fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(502, RESPONSES_WS_ERROR_CODE, message)
    }

    fn forbidden_bilingual(
        chinese_description: impl AsRef<str>,
        english_raw_message: impl AsRef<str>,
    ) -> Self {
        Self::new(
            403,
            "permission_denied",
            crate::gateway::bilingual_error(chinese_description, english_raw_message),
        )
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(503, RESPONSES_WS_ERROR_CODE, message)
    }

    fn bad_request_bilingual(
        chinese_description: impl AsRef<str>,
        english_raw_message: impl AsRef<str>,
    ) -> Self {
        Self::bad_request(crate::gateway::bilingual_error(
            chinese_description,
            english_raw_message,
        ))
    }

    fn bad_gateway_bilingual(
        chinese_description: impl AsRef<str>,
        english_raw_message: impl AsRef<str>,
    ) -> Self {
        Self::bad_gateway(crate::gateway::bilingual_error(
            chinese_description,
            english_raw_message,
        ))
    }

    fn service_unavailable_bilingual(
        chinese_description: impl AsRef<str>,
        english_raw_message: impl AsRef<str>,
    ) -> Self {
        Self::service_unavailable(crate::gateway::bilingual_error(
            chinese_description,
            english_raw_message,
        ))
    }
}

pub(super) fn is_websocket_upgrade_request(headers: &HeaderMap) -> bool {
    let upgrade_is_websocket = headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let connection_has_upgrade = headers
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        });
    upgrade_is_websocket && connection_has_upgrade
}

pub(super) async fn upgrade_responses_websocket(request: HttpRequest<Body>) -> Response<Body> {
    let (mut parts, _) = request.into_parts();

    let context = match authorize_websocket_request(&parts.headers) {
        Ok(context) => context,
        Err(response) => return response,
    };

    let ws = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(ws) => ws,
        Err(err) => {
            return text_error_response(
                StatusCode::BAD_REQUEST,
                crate::gateway::error_message_for_client(
                    context.prefer_raw_errors,
                    crate::gateway::bilingual_error(
                        "WebSocket 升级失败",
                        format!("websocket upgrade rejected: {err}"),
                    ),
                ),
            );
        }
    };

    ws.on_upgrade(move |socket| async move {
        run_responses_websocket_session(socket, context).await;
    })
}

async fn run_responses_websocket_session(mut socket: WebSocket, context: WsRequestContext) {
    let first_text = match receive_initial_request(&mut socket).await {
        Ok(Some(text)) => text,
        Ok(None) => return,
        Err(err) => {
            send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
            return;
        }
    };

    let prepared_first = match rewrite_client_frame(first_text.as_str(), &context) {
        Ok(prepared) => prepared,
        Err(err) => {
            send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
            return;
        }
    };

    let mut upstream =
        match connect_upstream_websocket(&context, prepared_first.model.as_deref()).await {
            Ok(stream) => stream,
            Err(err) => {
                send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                return;
            }
        };
    let first_pending = PendingWsRequestState {
        log: begin_ws_request_log(
            &context,
            &prepared_first,
            upstream.route_strategy,
            upstream.route_source,
        ),
        prepared: prepared_first.clone(),
        forwarded_upstream_event: false,
    };

    if let Err(err) = upstream
        .stream
        .send(UpstreamMessage::Text(
            first_pending.prepared.text.clone().into(),
        ))
        .await
    {
        finalize_ws_request_log(
            &context,
            &first_pending.log,
            Some(upstream.account_id.as_str()),
            Some(upstream.upstream_url.as_str()),
            502,
            crate::gateway::RequestLogUsage::default(),
            Some(crate::gateway::bilingual_error(
                "发送上游 WebSocket 首帧失败",
                format!("send first upstream websocket frame failed: {err}"),
            )),
        );
        send_ws_error_and_close(
            &mut socket,
            WsSessionError::bad_gateway_bilingual(
                "发送上游 WebSocket 首帧失败",
                format!("send first upstream websocket frame failed: {err}"),
            ),
            context.prefer_raw_errors,
        )
        .await;
        return;
    }
    let mut pending_request = Some(first_pending);

    loop {
        tokio::select! {
            maybe_client = socket.recv() => {
                let Some(client_result) = maybe_client else {
                    let _ = upstream.stream.close(None).await;
                    break;
                };
                match client_result {
                    Ok(Message::Text(text)) => {
                        match rewrite_client_frame(text.as_str(), &context) {
                            Ok(prepared) => {
                                if let Some(previous_pending) = pending_request.take() {
                                    finalize_ws_request_log(
                                        &context,
                                        &previous_pending.log,
                                        Some(upstream.account_id.as_str()),
                                        Some(upstream.upstream_url.as_str()),
                                        499,
                                        crate::gateway::RequestLogUsage::default(),
                                        Some(crate::gateway::bilingual_error(
                                            "WebSocket 请求在完成前被覆盖",
                                            "websocket request superseded before completion",
                                        )),
                                    );
                                }
                                let current_pending = PendingWsRequestState {
                                    log: begin_ws_request_log(
                                        &context,
                                        &prepared,
                                        upstream.route_strategy,
                                        upstream.route_source,
                                    ),
                                    prepared,
                                    forwarded_upstream_event: false,
                                };
                                if let Err(err) = upstream.stream.send(UpstreamMessage::Text(
                                    current_pending.prepared.text.clone().into(),
                                )).await {
                                    finalize_ws_request_log(
                                        &context,
                                        &current_pending.log,
                                        Some(upstream.account_id.as_str()),
                                        Some(upstream.upstream_url.as_str()),
                                        502,
                                        crate::gateway::RequestLogUsage::default(),
                                        Some(crate::gateway::bilingual_error(
                                            "发送上游 WebSocket 帧失败",
                                            format!("send upstream websocket frame failed: {err}"),
                                        )),
                                    );
                                    send_ws_error_and_close(
                                        &mut socket,
                                        WsSessionError::bad_gateway_bilingual(
                                            "发送上游 WebSocket 帧失败",
                                            format!("send upstream websocket frame failed: {err}"),
                                        ),
                                        context.prefer_raw_errors,
                                    ).await;
                                    let _ = upstream.stream.close(None).await;
                                    break;
                                }
                                pending_request = Some(current_pending);
                            }
                            Err(err) => {
                                send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                                let _ = upstream.stream.close(None).await;
                                break;
                            }
                        }
                    }
                    Ok(Message::Ping(payload)) => {
                        let _ = upstream.stream.send(UpstreamMessage::Ping(payload)).await;
                    }
                    Ok(Message::Pong(payload)) => {
                        let _ = upstream.stream.send(UpstreamMessage::Pong(payload)).await;
                    }
                    Ok(Message::Binary(bytes)) => {
                        if let Err(err) = upstream.stream.send(UpstreamMessage::Binary(bytes)).await {
                            send_ws_error_and_close(
                                &mut socket,
                                WsSessionError::bad_gateway_bilingual(
                                    "发送上游 WebSocket 二进制消息失败",
                                    format!("send upstream websocket binary failed: {err}"),
                                ),
                                context.prefer_raw_errors,
                            ).await;
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        let _ = upstream.stream.close(None).await;
                        break;
                    }
                    Err(err) => {
                        send_ws_error_and_close(
                            &mut socket,
                            WsSessionError::bad_request_bilingual(
                                "接收客户端 WebSocket 帧失败",
                                format!("receive client websocket frame failed: {err}"),
                            ),
                            context.prefer_raw_errors,
                        ).await;
                        let _ = upstream.stream.close(None).await;
                        break;
                    }
                }
            }
            maybe_upstream = upstream.stream.next() => {
                let Some(upstream_result) = maybe_upstream else {
                    let _ = socket.close().await;
                    break;
                };
                match upstream_result {
                    Ok(UpstreamMessage::Text(text)) => {
                        if let Some(terminal) = inspect_ws_terminal_event(text.as_str()) {
                            let retry_model = pending_request
                                .as_ref()
                                .and_then(|pending| pending.prepared.model.clone());
                            let retry_succeeded = if let Some(pending) = pending_request.as_mut() {
                                if !pending.forwarded_upstream_event {
                                    try_retry_ws_request_after_terminal(&context, &mut upstream, pending, &terminal).await
                                } else {
                                    false
                                }
                            } else {
                                false
                            };
                            if retry_succeeded {
                                continue;
                            }

                            if let Some(mut pending) = pending_request.take() {
                                mark_ws_first_response(&mut pending);
                                finalize_ws_request_log(
                                    &context,
                                    &pending.log,
                                    Some(upstream.account_id.as_str()),
                                    Some(upstream.upstream_url.as_str()),
                                    terminal.status_code,
                                    terminal.usage,
                                    terminal.error,
                                );
                            }
                            if let Err(err) = socket
                                .send(Message::Text(text.to_string().into()))
                                .await
                            {
                                log::warn!("event=responses_ws_client_send_terminal_failed err={err}");
                                break;
                            }
                            let _ = retry_model;
                            continue;
                        }

                        if let Some(pending) = pending_request.as_mut() {
                            mark_ws_first_response(pending);
                        }
                        if let Err(err) = socket
                            .send(Message::Text(text.to_string().into()))
                            .await
                        {
                            log::warn!("event=responses_ws_client_send_failed err={err}");
                            break;
                        }
                    }
                    Ok(UpstreamMessage::Binary(bytes)) => {
                        if let Err(err) = socket.send(Message::Binary(bytes)).await {
                            log::warn!("event=responses_ws_client_send_binary_failed err={err}");
                            break;
                        }
                    }
                    Ok(UpstreamMessage::Ping(payload)) => {
                        let _ = socket.send(Message::Ping(payload)).await;
                    }
                    Ok(UpstreamMessage::Pong(payload)) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Ok(UpstreamMessage::Close(_)) => {
                        let _ = socket.close().await;
                        break;
                    }
                    Ok(UpstreamMessage::Frame(_)) => {}
                    Err(err) => {
                        send_ws_error_and_close(
                            &mut socket,
                            WsSessionError::bad_gateway_bilingual(
                                "接收上游 WebSocket 帧失败",
                                format!("receive upstream websocket frame failed: {err}"),
                            ),
                            context.prefer_raw_errors,
                        ).await;
                        break;
                    }
                }
            }
        }
    }
}

fn authorize_websocket_request(headers: &HeaderMap) -> Result<WsRequestContext, Response<Body>> {
    let prefer_raw_errors = crate::gateway::prefers_raw_errors_for_http_headers(headers);
    let incoming_headers = crate::gateway::IncomingHeaderSnapshot::from_http_headers(headers);
    let Some(platform_key) = incoming_headers.platform_key() else {
        return Err(text_error_response(
            StatusCode::UNAUTHORIZED,
            crate::gateway::error_message_for_client(
                prefer_raw_errors,
                crate::gateway::bilingual_error("缺少平台 API Key", "missing platform api key"),
            ),
        ));
    };

    let storage = open_storage().ok_or_else(|| {
        text_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            crate::gateway::error_message_for_client(
                prefer_raw_errors,
                crate::gateway::bilingual_error("存储不可用", "storage unavailable"),
            ),
        )
    })?;
    let api_key = storage
        .find_api_key_by_hash(&hash_platform_key(platform_key))
        .map_err(|err| {
            text_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                crate::gateway::error_message_for_client(
                    prefer_raw_errors,
                    crate::gateway::bilingual_error(
                        "读取存储失败",
                        format!("storage read failed: {err}"),
                    ),
                ),
            )
        })?
        .ok_or_else(|| {
            text_error_response(
                StatusCode::FORBIDDEN,
                crate::gateway::error_message_for_client(
                    prefer_raw_errors,
                    crate::gateway::bilingual_error(
                        "平台 API Key 不存在",
                        "platform api key not found",
                    ),
                ),
            )
        })?;

    if api_key.status != "active" {
        return Err(text_error_response(
            StatusCode::FORBIDDEN,
            crate::gateway::error_message_for_client(
                prefer_raw_errors,
                crate::gateway::bilingual_error("API Key 已禁用", "api key disabled"),
            ),
        ));
    }
    if let Some(limit) = storage
        .find_api_key_quota_limit(&api_key.id)
        .map_err(|err| {
            text_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read api key quota failed: {err}"),
            )
        })?
    {
        let used = storage
            .api_key_total_token_usage(&api_key.id)
            .map_err(|err| {
                text_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("read api key usage failed: {err}"),
                )
            })?;
        if limit > 0 && used >= limit {
            return Err(text_error_response(
                StatusCode::TOO_MANY_REQUESTS,
                crate::gateway::error_message_for_client(
                    prefer_raw_errors,
                    crate::gateway::bilingual_error(
                        "API Key 额度已用尽",
                        format!("api key quota exhausted: used {used}, limit {limit}"),
                    ),
                ),
            ));
        }
    }
    let key_policy = storage
        .find_api_key_policy(&api_key.id)
        .map_err(|err| {
            text_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read api key policy failed: {err}"),
            )
        })?
        .unwrap_or_default();
    if key_policy
        .expires_at
        .is_some_and(|expires_at| expires_at <= codexmanager_core::storage::now_ts())
    {
        return Err(text_error_response(
            StatusCode::FORBIDDEN,
            crate::gateway::error_message_for_client(
                prefer_raw_errors,
                crate::gateway::bilingual_error("API Key 已过期", "api key expired"),
            ),
        ));
    }
    if !key_policy.allowed_platforms.is_empty()
        && !key_policy
            .allowed_platforms
            .iter()
            .any(|value| value.eq_ignore_ascii_case("codex"))
    {
        return Err(text_error_response(
            StatusCode::FORBIDDEN,
            crate::gateway::error_message_for_client(
                prefer_raw_errors,
                crate::gateway::bilingual_error(
                    "此 API Key 不允许访问 Codex",
                    "platform_not_allowed_by_key: codex",
                ),
            ),
        ));
    }
    let api_key_inflight_guard =
        crate::gateway::try_acquire_api_key_inflight(&api_key.id, key_policy.concurrency_limit)
            .map_err(|current| {
                text_error_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    crate::gateway::error_message_for_client(
                        prefer_raw_errors,
                        crate::gateway::bilingual_error(
                            "API Key 并发请求已达到上限",
                            format!("api key concurrency limit exceeded: active {current}"),
                        ),
                    ),
                )
            })?;
    crate::wallet_precheck_for_api_key(&storage, &api_key.id).map_err(|err| {
        text_error_response(
            if err.contains("余额不足") {
                StatusCode::PAYMENT_REQUIRED
            } else {
                StatusCode::FORBIDDEN
            },
            err,
        )
    })?;

    if !crate::gateway::gateway_supports_official_responses_websocket(&api_key) {
        return Err(upgrade_required_response(
            crate::gateway::error_message_for_client(
                prefer_raw_errors,
                crate::gateway::bilingual_error(
                    "Responses WebSocket 仅支持官方 Codex 上游",
                    "responses websocket is only available for official Codex upstream",
                ),
            ),
        ));
    }

    let (incoming_headers, prompt_cache_key) =
        crate::gateway::gateway_resolve_ws_prompt_cache_key(&storage, &api_key, &incoming_headers)
            .map_err(|err| {
                text_error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    crate::gateway::error_message_for_client(
                        prefer_raw_errors,
                        crate::gateway::bilingual_error("读取会话绑定失败", err),
                    ),
                )
            })?;

    Ok(WsRequestContext {
        effective_upstream_base: crate::gateway::gateway_resolve_effective_upstream_base(&api_key),
        api_key,
        incoming_headers,
        prompt_cache_key,
        prefer_raw_errors,
        key_policy,
        _api_key_inflight_guard: api_key_inflight_guard,
    })
}

async fn receive_initial_request(socket: &mut WebSocket) -> Result<Option<String>, WsSessionError> {
    loop {
        let Some(message) = socket.recv().await else {
            return Ok(None);
        };
        match message {
            Ok(Message::Text(text)) => return Ok(Some(text.to_string())),
            Ok(Message::Ping(payload)) => {
                let _ = socket.send(Message::Pong(payload)).await;
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Close(_)) => return Ok(None),
            Ok(Message::Binary(_)) => {
                return Err(WsSessionError::bad_request_bilingual(
                    "首个 WebSocket 帧必须是 response.create 文本帧",
                    "initial websocket frame must be a response.create text frame",
                ));
            }
            Err(err) => {
                return Err(WsSessionError::bad_request_bilingual(
                    "接收首个 WebSocket 帧失败",
                    format!("receive initial websocket frame failed: {err}"),
                ));
            }
        }
    }
}

fn rewrite_client_frame(
    text: &str,
    context: &WsRequestContext,
) -> Result<PreparedClientFrame, WsSessionError> {
    let mut payload = serde_json::from_str::<Value>(text).map_err(|err| {
        WsSessionError::bad_request_bilingual(
            "WebSocket JSON 载荷无效",
            format!("invalid websocket json payload: {err}"),
        )
    })?;
    let Some(object) = payload.as_object_mut() else {
        return Err(WsSessionError::bad_request_bilingual(
            "WebSocket 载荷必须是 JSON 对象",
            "websocket payload must be a JSON object",
        ));
    };
    let message_type = object
        .remove("type")
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| {
            WsSessionError::bad_request_bilingual(
                "WebSocket 载荷缺少 type=response.create",
                "websocket payload missing type=response.create",
            )
        })?;
    if message_type != "response.create" {
        return Err(WsSessionError::bad_request_bilingual(
            "不支持的 WebSocket 消息类型",
            format!("unsupported websocket message type: {message_type}"),
        ));
    }

    let service_tier_diagnostic =
        crate::gateway::inspect_service_tier_value(object.get("service_tier"));
    let explicit_service_tier_for_log = service_tier_diagnostic.normalized_value.clone();
    let client_model_for_log = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    let client_reasoning_for_log = object
        .get("reasoning")
        .and_then(|value| value.get("effort"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let previous_response_id = object.remove("previous_response_id");
    let generate = object.remove("generate");
    let client_metadata = object.remove("client_metadata");

    let rewritten_body = crate::gateway::gateway_rewrite_ws_responses_body(
        RESPONSES_ENDPOINT,
        serde_json::to_vec(&Value::Object(object.clone())).map_err(|err| {
            WsSessionError::bad_request_bilingual(
                "序列化 WebSocket 请求失败",
                format!("serialize websocket payload failed: {err}"),
            )
        })?,
        &context.api_key,
        context.prompt_cache_key.as_deref(),
    );
    let mut rewritten_value = serde_json::from_slice::<Value>(&rewritten_body).map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "重写 WebSocket 请求失败",
            format!("rewrite websocket payload failed: {err}"),
        )
    })?;
    let Some(rewritten_object) = rewritten_value.as_object_mut() else {
        return Err(WsSessionError::bad_gateway_bilingual(
            "重写后的 WebSocket 请求不是对象",
            "rewritten websocket payload must be a JSON object",
        ));
    };
    if let Some(previous_response_id) = previous_response_id {
        rewritten_object.insert("previous_response_id".to_string(), previous_response_id);
    }
    if let Some(generate) = generate {
        rewritten_object.insert("generate".to_string(), generate);
    }
    let merged_client_metadata = merge_client_metadata(
        rewritten_object.remove("client_metadata"),
        client_metadata,
        &context.incoming_headers,
    );
    if let Some(client_metadata) = merged_client_metadata {
        rewritten_object.insert("client_metadata".to_string(), client_metadata);
    }

    let request: ResponseCreateWsRequest =
        serde_json::from_value(Value::Object(rewritten_object.clone())).map_err(|err| {
            WsSessionError::bad_request_bilingual(
                "WebSocket 请求不符合官方 Codex request 形状",
                format!("invalid official codex websocket request shape: {err}"),
            )
        })?;
    if !context.key_policy.allowed_models.is_empty()
        && !context
            .key_policy
            .allowed_models
            .iter()
            .any(|allowed| allowed == "*" || allowed.eq_ignore_ascii_case(request.model.as_str()))
    {
        return Err(WsSessionError::forbidden_bilingual(
            "模型不在此 API Key 的白名单中",
            format!("model_not_allowed_by_key: {}", request.model),
        ));
    }
    let effective_service_tier = request
        .service_tier
        .as_deref()
        .and_then(crate::apikey::service_tier::normalize_service_tier_for_log)
        .map(str::to_string);
    let service_tier_source = resolve_ws_service_tier_source_for_log(
        explicit_service_tier_for_log.as_deref(),
        effective_service_tier.as_deref(),
        context.api_key.service_tier.as_deref(),
    );
    let model_source = resolve_ws_override_source_for_log(
        client_model_for_log.as_deref(),
        Some(request.model.as_str()),
        context.api_key.model_slug.as_deref(),
    );
    let reasoning_effort = request
        .reasoning
        .as_ref()
        .and_then(|value| value.get("effort"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let reasoning_source = resolve_ws_override_source_for_log(
        client_reasoning_for_log.as_deref(),
        reasoning_effort.as_deref(),
        context.api_key.reasoning_effort.as_deref(),
    );
    let text = serde_json::to_string(&ResponsesWsRequest::ResponseCreate(request.clone()))
        .map_err(|err| {
            WsSessionError::bad_request_bilingual(
                "序列化官方 Codex WebSocket 请求失败",
                format!("serialize official codex websocket request failed: {err}"),
            )
        })?;

    Ok(PreparedClientFrame {
        text,
        client_model: client_model_for_log,
        model: Some(request.model),
        model_source,
        client_reasoning_effort: client_reasoning_for_log,
        reasoning_effort,
        reasoning_source,
        service_tier: explicit_service_tier_for_log,
        effective_service_tier,
        service_tier_source,
        raw_service_tier: service_tier_diagnostic.raw_value,
        has_service_tier_field: service_tier_diagnostic.has_field,
    })
}

fn resolve_ws_service_tier_source_for_log(
    client_service_tier: Option<&str>,
    effective_service_tier: Option<&str>,
    api_key_service_tier: Option<&str>,
) -> Option<String> {
    match (client_service_tier, effective_service_tier) {
        (Some(client), Some(effective)) if client.eq_ignore_ascii_case(effective) => {
            Some("client_request".to_string())
        }
        (Some(_), Some(_)) => Some("gateway_override".to_string()),
        (None, Some(_)) => {
            if api_key_service_tier
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                Some("api_key_profile".to_string())
            } else {
                Some("gateway_config".to_string())
            }
        }
        (Some(_), None) => Some("client_request".to_string()),
        (None, None) => Some("unset".to_string()),
    }
}

fn resolve_ws_override_source_for_log(
    client_value: Option<&str>,
    effective_value: Option<&str>,
    api_key_profile_value: Option<&str>,
) -> Option<String> {
    match (client_value, effective_value) {
        (Some(client), Some(effective)) if client.eq_ignore_ascii_case(effective) => {
            Some("client_request".to_string())
        }
        (Some(_), Some(_)) => Some("gateway_override".to_string()),
        (None, Some(_)) => {
            if api_key_profile_value
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                Some("api_key_profile".to_string())
            } else {
                Some("gateway_config".to_string())
            }
        }
        (Some(_), None) => Some("client_request".to_string()),
        (None, None) => Some("unset".to_string()),
    }
}

fn merge_metadata_value(mapped: &mut HashMap<String, String>, client_metadata: Option<Value>) {
    if let Some(Value::Object(object)) = client_metadata {
        for (key, value) in object {
            if let Some(value) = value.as_str() {
                mapped.insert(key, value.to_string());
            } else if let Some(value) = value.as_i64() {
                mapped.insert(key, value.to_string());
            } else if let Some(value) = value.as_u64() {
                mapped.insert(key, value.to_string());
            } else if let Some(value) = value.as_bool() {
                mapped.insert(key, value.to_string());
            }
        }
    }
}

fn insert_header_metadata(mapped: &mut HashMap<String, String>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        mapped.insert(key.to_string(), value.to_string());
    }
}

fn merge_client_metadata(
    rewritten_metadata: Option<Value>,
    client_metadata: Option<Value>,
    incoming_headers: &crate::gateway::IncomingHeaderSnapshot,
) -> Option<Value> {
    let mut mapped = HashMap::new();
    merge_metadata_value(&mut mapped, client_metadata);
    merge_metadata_value(&mut mapped, rewritten_metadata);
    insert_header_metadata(
        &mut mapped,
        X_CODEX_TURN_METADATA_HEADER,
        incoming_headers.turn_metadata(),
    );
    insert_header_metadata(
        &mut mapped,
        X_CODEX_WINDOW_ID_HEADER,
        incoming_headers.window_id(),
    );
    insert_header_metadata(
        &mut mapped,
        X_OPENAI_SUBAGENT_HEADER,
        incoming_headers.subagent(),
    );
    insert_header_metadata(
        &mut mapped,
        X_CODEX_PARENT_THREAD_ID_HEADER,
        incoming_headers.parent_thread_id(),
    );
    response_create_client_metadata((!mapped.is_empty()).then_some(mapped))
        .and_then(|value| serde_json::to_value(value).ok())
}

async fn connect_upstream_websocket(
    context: &WsRequestContext,
    model: Option<&str>,
) -> Result<ConnectedUpstreamWebsocket, WsSessionError> {
    let storage = open_storage().ok_or_else(|| {
        WsSessionError::service_unavailable_bilingual("存储不可用", "storage unavailable")
    })?;
    let routed = crate::gateway::gateway_collect_routed_candidates_with_log_source(
        &storage,
        &context.api_key.id,
        model,
    )?;
    if routed.candidates.is_empty() {
        return Err(WsSessionError::service_unavailable_bilingual(
            "没有可用的上游账号",
            "no available upstream accounts",
        ));
    }

    let ws_url = build_upstream_websocket_url(&context.effective_upstream_base)?;
    let mut last_error = None;
    ensure_rustls_crypto_provider();
    for (account, mut token) in routed.candidates {
        let bearer = match resolve_bearer_token_for_websocket(
            account.clone(),
            token,
            !crate::gateway::gateway_is_openai_api_base(context.effective_upstream_base.as_str()),
        )
        .await
        {
            Ok((bearer, resolved_token)) => {
                token = resolved_token;
                bearer
            }
            Err(err) => {
                last_error = Some(format!(
                    "resolve bearer token for account {} failed: {err}",
                    account.id
                ));
                continue;
            }
        };
        let request =
            build_upstream_websocket_request(ws_url.as_str(), &account, bearer.as_str(), context)?;
        let proxy_url = crate::gateway::current_upstream_proxy_url_for_account(account.id.as_str());
        match connect_upstream_websocket_request_detailed(
            request,
            ws_url.as_str(),
            proxy_url.as_deref(),
        )
        .await
        {
            Ok((stream, _)) => {
                return Ok(ConnectedUpstreamWebsocket {
                    stream,
                    account_id: account.id,
                    upstream_url: ws_url.clone(),
                    route_strategy: routed.route_strategy,
                    route_source: routed.route_source,
                });
            }
            Err(err) => {
                if err.is_unauthorized() {
                    let refreshed = if bearer.starts_with("AgentAssertion ") {
                        crate::account_agent_identity::recover_agent_identity_authorization(
                            &storage,
                            &account.id,
                        )
                    } else {
                        try_refresh_websocket_bearer(&storage, context, &account, &mut token)
                    };
                    match refreshed {
                        Ok(Some(refreshed_bearer)) => {
                            log::warn!(
                                "event=responses_ws_unauthorized_refresh_retry account_id={}",
                                account.id
                            );
                            let retry_request = build_upstream_websocket_request(
                                ws_url.as_str(),
                                &account,
                                refreshed_bearer.as_str(),
                                context,
                            )?;
                            match connect_upstream_websocket_request_detailed(
                                retry_request,
                                ws_url.as_str(),
                                proxy_url.as_deref(),
                            )
                            .await
                            {
                                Ok((stream, _)) => {
                                    return Ok(ConnectedUpstreamWebsocket {
                                        stream,
                                        account_id: account.id,
                                        upstream_url: ws_url.clone(),
                                        route_strategy: routed.route_strategy,
                                        route_source: routed.route_source,
                                    });
                                }
                                Err(retry_err) => {
                                    last_error = Some(format!(
                                        "connect upstream websocket for account {} failed after token refresh: {retry_err}",
                                        account.id
                                    ));
                                    continue;
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(refresh_err) => {
                            let _ =
                                crate::account_status::mark_account_unavailable_for_refresh_token_error(
                                    &storage,
                                    &account.id,
                                    &refresh_err,
                                );
                            log::warn!(
                                "event=responses_ws_unauthorized_refresh_failed account_id={} err={}",
                                account.id,
                                refresh_err
                            );
                        }
                    }
                }
                last_error = Some(format!(
                    "connect upstream websocket for account {} failed: {err}",
                    account.id
                ));
            }
        }
    }

    Err(WsSessionError::bad_gateway_bilingual(
        "连接上游 WebSocket 失败",
        last_error.unwrap_or_else(|| "connect upstream websocket failed".to_string()),
    ))
}

async fn resolve_bearer_token_for_websocket(
    account: codexmanager_core::storage::Account,
    token: codexmanager_core::storage::Token,
    use_agent_identity: bool,
) -> Result<(String, codexmanager_core::storage::Token), String> {
    let join_result = tokio::task::spawn_blocking(move || {
        let storage = open_storage()
            .ok_or_else(|| crate::gateway::bilingual_error("存储不可用", "storage unavailable"))?;
        let mut token = token;
        let bearer = if use_agent_identity {
            crate::account_agent_identity::build_agent_identity_authorization(
                &storage,
                &account.id,
            )?
            .unwrap_or_else(|| token.access_token.clone())
        } else {
            crate::gateway::gateway_resolve_openai_bearer_token(&storage, &account, &mut token)?
        };
        Ok((bearer, token))
    })
    .await;

    match join_result {
        Ok(result) => result,
        Err(err) => Err(crate::gateway::bilingual_error(
            "Bearer Token 任务合并失败",
            format!("bearer token task join failed: {err}"),
        )),
    }
}

fn try_refresh_websocket_bearer(
    storage: &codexmanager_core::storage::Storage,
    context: &WsRequestContext,
    account: &codexmanager_core::storage::Account,
    token: &mut codexmanager_core::storage::Token,
) -> Result<Option<String>, String> {
    if crate::gateway::gateway_is_openai_api_base(context.effective_upstream_base.as_str()) {
        return Ok(None);
    }
    if token.refresh_token.trim().is_empty() {
        return Ok(None);
    }

    let previous_api_key_access_token = token.api_key_access_token.clone();
    let issuer = if account.issuer.trim().is_empty() {
        crate::gateway::gateway_token_exchange_default_issuer()
    } else {
        account.issuer.clone()
    };
    let client_id = crate::gateway::gateway_token_exchange_client_id();
    crate::usage_token_refresh::refresh_and_persist_access_token(
        storage,
        token,
        issuer.as_str(),
        client_id.as_str(),
        crate::usage_token_refresh::token_refresh_ahead_secs(),
    )?;

    if token.api_key_access_token == previous_api_key_access_token {
        token.api_key_access_token = None;
        storage.insert_token(token).map_err(|err| err.to_string())?;
    }

    let bearer = crate::gateway::gateway_resolve_openai_bearer_token(storage, account, token)?;
    if bearer.trim().is_empty() {
        return Err("refreshed websocket bearer token is empty".to_string());
    }
    Ok(Some(bearer))
}

fn build_upstream_websocket_url(upstream_base: &str) -> Result<String, WsSessionError> {
    let (target_url, _) =
        crate::gateway::gateway_compute_upstream_url(upstream_base, RESPONSES_ENDPOINT);
    let mut url = url::Url::parse(target_url.as_str()).map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "上游 WebSocket URL 无效",
            format!("invalid upstream websocket url: {err}"),
        )
    })?;
    match url.scheme() {
        "http" => {
            let _ = url.set_scheme("ws");
        }
        "https" => {
            let _ = url.set_scheme("wss");
        }
        "ws" | "wss" => {}
        other => {
            return Err(WsSessionError::bad_gateway_bilingual(
                "不支持的上游 WebSocket 协议",
                format!("unsupported upstream websocket scheme: {other}"),
            ));
        }
    }
    Ok(url.to_string())
}

pub(crate) async fn connect_upstream_websocket_request(
    request: WsClientRequest,
    ws_url: &str,
    proxy_url: Option<&str>,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
        WsClientResponse,
    ),
    String,
> {
    connect_upstream_websocket_request_detailed(request, ws_url, proxy_url)
        .await
        .map_err(|err| err.to_string())
}

pub(crate) async fn connect_upstream_websocket_request_detailed(
    request: WsClientRequest,
    ws_url: &str,
    proxy_url: Option<&str>,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
        WsClientResponse,
    ),
    WsConnectError,
> {
    ensure_rustls_crypto_provider();
    let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return connect_async_tls_with_config(request, None, false, None)
            .await
            .map_err(WsConnectError::from_tungstenite);
    };

    let stream = connect_websocket_proxy_tcp(ws_url, proxy_url)
        .await
        .map_err(WsConnectError::from_message)?;
    client_async_tls_with_config(request, stream, None, None)
        .await
        .map_err(WsConnectError::from_tungstenite)
}

async fn connect_websocket_proxy_tcp(ws_url: &str, proxy_url: &str) -> Result<TcpStream, String> {
    let target = parse_websocket_target(ws_url)?;
    let proxy = url::Url::parse(proxy_url)
        .map_err(|err| format!("invalid websocket proxy url {proxy_url}: {err}"))?;
    match proxy.scheme() {
        "http" => connect_http_proxy_tunnel(&proxy, &target).await,
        "socks" | "socks5" | "socks5h" => connect_socks5_proxy_tunnel(&proxy, &target).await,
        other => Err(format!("unsupported websocket proxy scheme: {other}")),
    }
}

fn parse_websocket_target(ws_url: &str) -> Result<WebsocketTarget, String> {
    let url = url::Url::parse(ws_url).map_err(|err| format!("invalid websocket url: {err}"))?;
    let raw_host = url
        .host_str()
        .map(str::to_string)
        .ok_or_else(|| "websocket url missing host".to_string())?;
    let host = raw_host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(raw_host.as_str())
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "websocket url missing port".to_string())?;
    let authority_host = authority_host(host.as_str());
    Ok(WebsocketTarget {
        host,
        port,
        authority: format!("{authority_host}:{port}"),
    })
}

fn proxy_host_port(proxy: &url::Url) -> Result<(String, u16), String> {
    let host = proxy
        .host_str()
        .map(str::to_string)
        .ok_or_else(|| "websocket proxy url missing host".to_string())?;
    let port = proxy
        .port_or_known_default()
        .unwrap_or(match proxy.scheme() {
            "http" => 80,
            "socks" | "socks5" | "socks5h" => 1080,
            _ => 0,
        });
    if port == 0 {
        return Err("websocket proxy url missing port".to_string());
    }
    Ok((host, port))
}

fn authority_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

async fn connect_http_proxy_tunnel(
    proxy: &url::Url,
    target: &WebsocketTarget,
) -> Result<TcpStream, String> {
    let (proxy_host, proxy_port) = proxy_host_port(proxy)?;
    let mut stream = TcpStream::connect((proxy_host.as_str(), proxy_port))
        .await
        .map_err(|err| format!("connect websocket http proxy failed: {err}"))?;

    let mut request = format!(
        "CONNECT {0} HTTP/1.1\r\nHost: {0}\r\nProxy-Connection: Keep-Alive\r\n",
        target.authority
    );
    if let Some(header) = proxy_basic_auth_header(proxy)? {
        request.push_str("Proxy-Authorization: ");
        request.push_str(header.as_str());
        request.push_str("\r\n");
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|err| format!("write websocket http proxy CONNECT failed: {err}"))?;

    let mut response = Vec::new();
    let mut buffer = [0_u8; 1024];
    while response.len() < 8192 {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|err| format!("read websocket http proxy CONNECT failed: {err}"))?;
        if read == 0 {
            return Err("websocket http proxy closed before CONNECT response".to_string());
        }
        response.extend_from_slice(&buffer[..read]);
        if response.windows(4).any(|window| window == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(response.as_slice());
            let status = text.lines().next().unwrap_or_default();
            if status.split_whitespace().nth(1) == Some("200") {
                return Ok(stream);
            }
            return Err(format!("websocket http proxy CONNECT rejected: {status}"));
        }
    }
    Err("websocket http proxy CONNECT response too large".to_string())
}

fn proxy_basic_auth_header(proxy: &url::Url) -> Result<Option<String>, String> {
    if proxy.username().is_empty() {
        return Ok(None);
    }
    let mut credentials = proxy.username().to_string();
    if let Some(password) = proxy.password() {
        credentials.push(':');
        credentials.push_str(password);
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes());
    Ok(Some(format!("Basic {encoded}")))
}

async fn connect_socks5_proxy_tunnel(
    proxy: &url::Url,
    target: &WebsocketTarget,
) -> Result<TcpStream, String> {
    let (proxy_host, proxy_port) = proxy_host_port(proxy)?;
    let mut stream = TcpStream::connect((proxy_host.as_str(), proxy_port))
        .await
        .map_err(|err| format!("connect websocket socks5 proxy failed: {err}"))?;

    let username = proxy.username();
    let password = proxy.password().unwrap_or("");
    if username.is_empty() {
        stream
            .write_all(&[0x05, 0x01, 0x00])
            .await
            .map_err(|err| format!("write socks5 greeting failed: {err}"))?;
    } else {
        stream
            .write_all(&[0x05, 0x02, 0x00, 0x02])
            .await
            .map_err(|err| format!("write socks5 greeting failed: {err}"))?;
    }

    let mut method = [0_u8; 2];
    stream
        .read_exact(&mut method)
        .await
        .map_err(|err| format!("read socks5 method failed: {err}"))?;
    if method[0] != 0x05 {
        return Err("invalid socks5 greeting response".to_string());
    }
    match method[1] {
        0x00 => {}
        0x02 => authenticate_socks5_proxy(&mut stream, username, password).await?,
        0xff => return Err("socks5 proxy rejected supported auth methods".to_string()),
        other => return Err(format!("unsupported socks5 auth method: {other}")),
    }

    let request = build_socks5_connect_request(target)?;
    stream
        .write_all(request.as_slice())
        .await
        .map_err(|err| format!("write socks5 connect request failed: {err}"))?;

    let mut head = [0_u8; 4];
    stream
        .read_exact(&mut head)
        .await
        .map_err(|err| format!("read socks5 connect response failed: {err}"))?;
    if head[0] != 0x05 {
        return Err("invalid socks5 connect response".to_string());
    }
    if head[1] != 0x00 {
        return Err(format!("socks5 connect rejected with code {}", head[1]));
    }
    match head[3] {
        0x01 => read_exact_discard(&mut stream, 4).await?,
        0x03 => {
            let mut len = [0_u8; 1];
            stream
                .read_exact(&mut len)
                .await
                .map_err(|err| format!("read socks5 bound domain length failed: {err}"))?;
            read_exact_discard(&mut stream, len[0] as usize).await?;
        }
        0x04 => read_exact_discard(&mut stream, 16).await?,
        other => {
            return Err(format!(
                "unsupported socks5 address type in response: {other}"
            ))
        }
    }
    read_exact_discard(&mut stream, 2).await?;
    Ok(stream)
}

async fn authenticate_socks5_proxy(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
) -> Result<(), String> {
    if username.len() > u8::MAX as usize || password.len() > u8::MAX as usize {
        return Err("socks5 proxy username/password is too long".to_string());
    }
    let mut request = Vec::with_capacity(3 + username.len() + password.len());
    request.push(0x01);
    request.push(username.len() as u8);
    request.extend_from_slice(username.as_bytes());
    request.push(password.len() as u8);
    request.extend_from_slice(password.as_bytes());
    stream
        .write_all(request.as_slice())
        .await
        .map_err(|err| format!("write socks5 auth failed: {err}"))?;
    let mut response = [0_u8; 2];
    stream
        .read_exact(&mut response)
        .await
        .map_err(|err| format!("read socks5 auth failed: {err}"))?;
    if response[1] == 0x00 {
        Ok(())
    } else {
        Err(format!("socks5 auth rejected with code {}", response[1]))
    }
}

fn build_socks5_connect_request(target: &WebsocketTarget) -> Result<Vec<u8>, String> {
    let mut request = vec![0x05, 0x01, 0x00];
    if let Ok(ip) = target.host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(addr) => {
                request.push(0x01);
                request.extend_from_slice(&addr.octets());
            }
            IpAddr::V6(addr) => {
                request.push(0x04);
                request.extend_from_slice(&addr.octets());
            }
        }
    } else {
        let host = target.host.as_bytes();
        if host.len() > u8::MAX as usize {
            return Err("websocket target host is too long for socks5".to_string());
        }
        request.push(0x03);
        request.push(host.len() as u8);
        request.extend_from_slice(host);
    }
    request.extend_from_slice(&target.port.to_be_bytes());
    Ok(request)
}

async fn read_exact_discard(stream: &mut TcpStream, len: usize) -> Result<(), String> {
    let mut buffer = vec![0_u8; len];
    stream
        .read_exact(buffer.as_mut_slice())
        .await
        .map_err(|err| format!("read socks5 response body failed: {err}"))?;
    Ok(())
}

fn build_upstream_websocket_request(
    ws_url: &str,
    account: &codexmanager_core::storage::Account,
    bearer_token: &str,
    context: &WsRequestContext,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, WsSessionError> {
    let mut request = ws_url.into_client_request().map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "构建上游 WebSocket 请求失败",
            format!("build upstream websocket request failed: {err}"),
        )
    })?;
    let headers = request.headers_mut();
    let authorization = if bearer_token.trim().starts_with("AgentAssertion ") {
        bearer_token.trim().to_string()
    } else {
        format!("Bearer {}", bearer_token.trim())
    };
    insert_header(headers, "Authorization", &authorization)?;
    if let Some(account_id) = account
        .chatgpt_account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        insert_header(headers, "ChatGPT-Account-ID", account_id)?;
    }
    insert_header(
        headers,
        "User-Agent",
        &crate::gateway::current_codex_user_agent(),
    )?;
    insert_header(
        headers,
        "originator",
        &crate::gateway::current_wire_originator(),
    )?;
    insert_header(
        headers,
        "OpenAI-Beta",
        RESPONSES_WEBSOCKETS_BETA_HEADER_VALUE,
    )?;
    if let Some(residency_requirement) = crate::gateway::current_residency_requirement() {
        insert_header(
            headers,
            "x-openai-internal-codex-residency",
            residency_requirement.as_str(),
        )?;
    }
    if let Some(session_id) = context.incoming_headers.session_id() {
        insert_header(headers, "session_id", session_id)?;
    }
    if let Some(window_id) = context.incoming_headers.window_id() {
        insert_header(
            headers,
            crate::http::codex_source::X_CODEX_WINDOW_ID_HEADER,
            window_id,
        )?;
    }
    if let Some(client_request_id) = context.incoming_headers.client_request_id() {
        insert_header(headers, "x-client-request-id", client_request_id)?;
    }
    if let Some(subagent) = context.incoming_headers.subagent() {
        insert_header(
            headers,
            crate::http::codex_source::X_OPENAI_SUBAGENT_HEADER,
            subagent,
        )?;
    }
    if let Some(beta_features) = context.incoming_headers.beta_features() {
        insert_header(headers, "x-codex-beta-features", beta_features)?;
    }
    if let Some(turn_state) = context.incoming_headers.turn_state() {
        insert_header(
            headers,
            crate::http::codex_source::X_CODEX_TURN_STATE_HEADER,
            turn_state,
        )?;
    }
    if let Some(turn_metadata) = context.incoming_headers.turn_metadata() {
        insert_header(
            headers,
            crate::http::codex_source::X_CODEX_TURN_METADATA_HEADER,
            turn_metadata,
        )?;
    }
    if let Some(parent_thread_id) = context.incoming_headers.parent_thread_id() {
        insert_header(
            headers,
            crate::http::codex_source::X_CODEX_PARENT_THREAD_ID_HEADER,
            parent_thread_id,
        )?;
    }
    if let Some(include_timing_metrics) = context
        .incoming_headers
        .responsesapi_include_timing_metrics()
    {
        insert_header(
            headers,
            crate::http::codex_source::X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER,
            include_timing_metrics,
        )?;
    }
    if let Some(oai_attestation) = context.incoming_headers.oai_attestation() {
        insert_header(headers, "x-oai-attestation", oai_attestation)?;
    }
    Ok(request)
}

fn begin_ws_request_log(
    context: &WsRequestContext,
    prepared: &PreparedClientFrame,
    route_strategy: &'static str,
    route_source: &'static str,
) -> PendingWsRequestLog {
    let trace_id = crate::gateway::next_trace_id();
    let effective_protocol_type = crate::apikey_profile::resolve_gateway_protocol_type(
        context.api_key.protocol_type.as_str(),
        RESPONSES_ENDPOINT,
    );
    crate::gateway::log_request_start(
        trace_id.as_str(),
        context.api_key.id.as_str(),
        "GET",
        RESPONSES_ENDPOINT,
        prepared.model.as_deref(),
        prepared.reasoning_effort.as_deref(),
        prepared.service_tier.as_deref(),
        true,
        "ws",
        effective_protocol_type,
    );
    crate::gateway::log_client_service_tier(
        trace_id.as_str(),
        "ws",
        RESPONSES_ENDPOINT,
        prepared.has_service_tier_field,
        prepared.raw_service_tier.as_deref(),
        prepared.service_tier.as_deref(),
    );
    PendingWsRequestLog {
        trace_id,
        route_strategy: Some(route_strategy.to_string()),
        route_source: Some(route_source.to_string()),
        client_model: prepared.client_model.clone(),
        model: prepared.model.clone(),
        model_source: prepared.model_source.clone(),
        client_reasoning_effort: prepared.client_reasoning_effort.clone(),
        reasoning_effort: prepared.reasoning_effort.clone(),
        reasoning_source: prepared.reasoning_source.clone(),
        service_tier: prepared.service_tier.clone(),
        effective_service_tier: prepared.effective_service_tier.clone(),
        service_tier_source: prepared.service_tier_source.clone(),
        started_at: Instant::now(),
        first_response_ms: None,
    }
}

fn mark_ws_first_response(pending: &mut PendingWsRequestState) {
    if pending.log.first_response_ms.is_none() {
        pending.log.first_response_ms = Some(
            pending
                .log
                .started_at
                .elapsed()
                .as_millis()
                .min(i64::MAX as u128) as i64,
        );
    }
    pending.forwarded_upstream_event = true;
}

fn finalize_ws_request_log(
    context: &WsRequestContext,
    pending: &PendingWsRequestLog,
    account_id: Option<&str>,
    upstream_url: Option<&str>,
    status_code: u16,
    mut usage: crate::gateway::RequestLogUsage,
    error: Option<String>,
) {
    let Some(storage) = open_storage() else {
        return;
    };
    if usage.first_response_ms.is_none() {
        usage.first_response_ms = pending.first_response_ms;
    }
    crate::gateway::write_request_log(
        &storage,
        crate::gateway::RequestLogTraceContext {
            trace_id: Some(pending.trace_id.as_str()),
            original_path: Some(RESPONSES_ENDPOINT),
            adapted_path: Some(RESPONSES_ENDPOINT),
            request_type: Some("ws"),
            route_strategy: pending.route_strategy.as_deref(),
            route_source: pending.route_source.as_deref(),
            client_model: pending.client_model.as_deref(),
            model_source: pending.model_source.as_deref(),
            client_reasoning_effort: pending.client_reasoning_effort.as_deref(),
            reasoning_source: pending.reasoning_source.as_deref(),
            service_tier: pending.service_tier.as_deref(),
            effective_service_tier: pending.effective_service_tier.as_deref(),
            service_tier_source: pending.service_tier_source.as_deref(),
            ..Default::default()
        },
        Some(context.api_key.id.as_str()),
        account_id,
        RESPONSES_ENDPOINT,
        "GET",
        pending.model.as_deref(),
        pending.reasoning_effort.as_deref(),
        upstream_url,
        Some(status_code),
        usage,
        error.as_deref(),
        Some(pending.started_at.elapsed().as_millis()),
    );
    crate::gateway::log_request_final(
        pending.trace_id.as_str(),
        status_code,
        account_id,
        upstream_url,
        error.as_deref(),
        pending.started_at.elapsed().as_millis(),
    );
}

struct WsTerminalEvent {
    status_code: u16,
    usage: crate::gateway::RequestLogUsage,
    error: Option<String>,
}

fn should_rotate_ws_upstream(status_code: u16) -> bool {
    matches!(status_code, 401 | 403 | 404 | 408 | 409 | 429)
}

async fn try_retry_ws_request_after_terminal(
    context: &WsRequestContext,
    upstream: &mut ConnectedUpstreamWebsocket,
    pending: &mut PendingWsRequestState,
    terminal: &WsTerminalEvent,
) -> bool {
    if terminal.status_code == 200 || pending.forwarded_upstream_event {
        return false;
    }
    let mut retry_text = None;
    if is_previous_response_not_found_terminal(terminal) {
        retry_text = strip_previous_response_id_from_ws_text(pending.prepared.text.as_str());
        if retry_text.is_none() {
            return false;
        }
    } else {
        let previous_account_id = upstream.account_id.clone();
        if !try_rotate_ws_upstream_after_terminal(
            context,
            upstream,
            pending.prepared.model.as_deref(),
            terminal.status_code,
        )
        .await
        {
            return false;
        }
        if upstream.account_id != previous_account_id {
            retry_text = strip_previous_response_id_from_ws_text(pending.prepared.text.as_str());
            pending.log.route_strategy = Some(upstream.route_strategy.to_string());
            pending.log.route_source = Some(upstream.route_source.to_string());
        }
    }
    let retry_text = retry_text.unwrap_or_else(|| pending.prepared.text.clone());
    match upstream
        .stream
        .send(UpstreamMessage::Text(retry_text.clone().into()))
        .await
    {
        Ok(()) => {
            pending.prepared.text = retry_text;
            pending.forwarded_upstream_event = false;
            pending.log.first_response_ms = None;
            true
        }
        Err(err) => {
            log::warn!(
                "event=responses_ws_retry_send_failed account_id={} status={} err={}",
                upstream.account_id,
                terminal.status_code,
                err
            );
            false
        }
    }
}

async fn try_rotate_ws_upstream_after_terminal(
    context: &WsRequestContext,
    upstream: &mut ConnectedUpstreamWebsocket,
    model: Option<&str>,
    status_code: u16,
) -> bool {
    if !should_rotate_ws_upstream(status_code) {
        return false;
    }

    let current_account_id = upstream.account_id.clone();
    crate::gateway::gateway_mark_account_cooldown_for_status(
        current_account_id.as_str(),
        status_code,
    );
    if status_code == 429 {
        let _ =
            crate::usage_refresh::enqueue_usage_refresh_for_account(current_account_id.as_str());
    }

    let storage = match open_storage() {
        Some(storage) => storage,
        None => return false,
    };
    let routed = match crate::gateway::gateway_collect_routed_candidates_with_log_source(
        &storage,
        &context.api_key.id,
        model,
    ) {
        Ok(routed) => routed,
        Err(err) => {
            log::warn!(
                "event=responses_ws_failover_candidates_failed account_id={} status={} err={}",
                current_account_id,
                status_code,
                err
            );
            return false;
        }
    };
    let Some((account, token)) = routed
        .candidates
        .into_iter()
        .find(|(account, _)| account.id != current_account_id)
    else {
        return false;
    };

    let bearer = match resolve_bearer_token_for_websocket(account.clone(), token, true).await {
        Ok((bearer, _token)) => bearer,
        Err(err) => {
            log::warn!(
                "event=responses_ws_failover_bearer_failed account_id={} next_account_id={} status={} err={}",
                current_account_id,
                account.id,
                status_code,
                err
            );
            return false;
        }
    };
    let request = match build_upstream_websocket_request(
        upstream.upstream_url.as_str(),
        &account,
        bearer.as_str(),
        context,
    ) {
        Ok(request) => request,
        Err(err) => {
            log::warn!(
                "event=responses_ws_failover_request_failed account_id={} next_account_id={} status={} err={}",
                current_account_id,
                account.id,
                status_code,
                err.message
            );
            return false;
        }
    };

    ensure_rustls_crypto_provider();
    let proxy_url = crate::gateway::current_upstream_proxy_url_for_account(account.id.as_str());
    let replacement = match connect_upstream_websocket_request(
        request,
        upstream.upstream_url.as_str(),
        proxy_url.as_deref(),
    )
    .await
    {
        Ok((stream, _)) => ConnectedUpstreamWebsocket {
            stream,
            account_id: account.id,
            upstream_url: upstream.upstream_url.clone(),
            route_strategy: routed.route_strategy,
            route_source: routed.route_source,
        },
        Err(err) => {
            log::warn!(
                "event=responses_ws_failover_connect_failed account_id={} status={} err={}",
                current_account_id,
                status_code,
                err
            );
            return false;
        }
    };

    crate::gateway::gateway_record_failover_attempt();
    let _ = upstream.stream.close(None).await;
    *upstream = replacement;
    true
}

fn inspect_ws_terminal_event(text: &str) -> Option<WsTerminalEvent> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    let event_type = value
        .get("type")
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    match event_type.as_str() {
        "response.completed" | "response.done" => Some(WsTerminalEvent {
            status_code: 200,
            usage: parse_ws_usage(&value),
            error: None,
        }),
        "response.failed" | "error" => {
            let error = extract_ws_error_message(&value);
            Some(WsTerminalEvent {
                status_code: infer_ws_terminal_status(&value, error.as_deref()),
                usage: parse_ws_usage(&value),
                error,
            })
        }
        "response.incomplete" => Some(WsTerminalEvent {
            status_code: 502,
            usage: parse_ws_usage(&value),
            error: extract_ws_error_message(&value)
                .or_else(|| Some("连接中断（可能是网络波动或客户端主动取消）".to_string())),
        }),
        _ => None,
    }
}

fn is_previous_response_not_found_terminal(terminal: &WsTerminalEvent) -> bool {
    if terminal.status_code != 400 {
        return false;
    }
    let Some(error) = terminal.error.as_deref() else {
        return false;
    };
    let lower = error.to_ascii_lowercase();
    lower.contains("previous response") && lower.contains("not found")
}

fn strip_previous_response_id_from_ws_text(text: &str) -> Option<String> {
    let mut value = serde_json::from_str::<Value>(text).ok()?;
    let object = value.as_object_mut()?;
    if object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "response.create")
        && object.remove("previous_response_id").is_some()
    {
        return serde_json::to_string(&value).ok();
    }
    None
}

fn infer_ws_terminal_status(value: &Value, error_message: Option<&str>) -> u16 {
    if let Some(status_code) = value
        .get("status")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
    {
        return status_code;
    }
    if let Some(message) = error_message {
        if crate::account_status::usage_limit_reason_from_message(message).is_some() {
            return 429;
        }
        if crate::account_status::deactivation_reason_from_message(message).is_some() {
            return 403;
        }
    }
    502
}

fn parse_ws_usage(value: &Value) -> crate::gateway::RequestLogUsage {
    let top_usage = value.get("usage").and_then(Value::as_object);
    let response_usage = value
        .get("response")
        .and_then(|response| response.get("usage"))
        .and_then(Value::as_object);
    let usage = response_usage.or(top_usage);
    crate::gateway::RequestLogUsage {
        input_tokens: usage
            .and_then(|map| map.get("input_tokens"))
            .and_then(Value::as_i64)
            .or_else(|| {
                usage
                    .and_then(|map| map.get("prompt_tokens"))
                    .and_then(Value::as_i64)
            }),
        cached_input_tokens: usage
            .and_then(|map| map.get("input_tokens_details"))
            .and_then(|details| details.get("cached_tokens"))
            .and_then(Value::as_i64)
            .or_else(|| {
                usage
                    .and_then(|map| map.get("prompt_tokens_details"))
                    .and_then(|details| details.get("cached_tokens"))
                    .and_then(Value::as_i64)
            })
            .or_else(|| {
                usage
                    .and_then(|map| map.get("cached_input_tokens"))
                    .and_then(Value::as_i64)
            }),
        output_tokens: usage
            .and_then(|map| map.get("output_tokens"))
            .and_then(Value::as_i64)
            .or_else(|| {
                usage
                    .and_then(|map| map.get("completion_tokens"))
                    .and_then(Value::as_i64)
            }),
        total_tokens: usage
            .and_then(|map| map.get("total_tokens"))
            .and_then(Value::as_i64),
        reasoning_output_tokens: usage
            .and_then(|map| map.get("output_tokens_details"))
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_i64)
            .or_else(|| {
                usage
                    .and_then(|map| map.get("completion_tokens_details"))
                    .and_then(|details| details.get("reasoning_tokens"))
                    .and_then(Value::as_i64)
            })
            .or_else(|| {
                usage
                    .and_then(|map| map.get("reasoning_output_tokens"))
                    .and_then(Value::as_i64)
            }),
        first_response_ms: None,
    }
}

fn extract_ws_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
        .or_else(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("status_details"))
                .and_then(|details| details.get("error"))
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_string)
        })
}

fn insert_header(headers: &mut HeaderMap, name: &str, value: &str) -> Result<(), WsSessionError> {
    let header_name = header::HeaderName::from_bytes(name.as_bytes()).map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "上游 WebSocket 请求头名称无效",
            format!("invalid upstream websocket header name {name}: {err}"),
        )
    })?;
    let header_value = HeaderValue::from_str(value).map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "上游 WebSocket 请求头值无效",
            format!("invalid upstream websocket header {name}: {err}"),
        )
    })?;
    headers.insert(header_name, header_value);
    Ok(())
}

fn ensure_rustls_crypto_provider() {
    static RUSTLS_PROVIDER_READY: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let _ = RUSTLS_PROVIDER_READY.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

async fn send_ws_error_and_close(
    socket: &mut WebSocket,
    err: WsSessionError,
    prefer_raw_errors: bool,
) {
    let message = crate::gateway::error_message_for_client(prefer_raw_errors, err.message);
    let payload = json!({
        "type": "error",
        "status": err.status,
        "error": {
            "code": err.code,
            "message": message,
        }
    });
    let _ = socket.send(Message::Text(payload.to_string().into())).await;
    let _ = socket.close().await;
}

fn upgrade_required_response(message: impl Into<String>) -> Response<Body> {
    let mut response = text_response(StatusCode::UPGRADE_REQUIRED, message.into());
    response
        .headers_mut()
        .insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    response.headers_mut().insert(
        crate::error_codes::ERROR_CODE_HEADER_NAME,
        HeaderValue::from_static("upgrade_required"),
    );
    response
}

impl From<String> for WsSessionError {
    fn from(value: String) -> Self {
        WsSessionError::bad_gateway(value)
    }
}

#[cfg(test)]
#[path = "responses_websocket_tests.rs"]
mod tests;
