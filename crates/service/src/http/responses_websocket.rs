use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::FromRequestParts;
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::{Request as HttpRequest, Response, StatusCode};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::net::IpAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::extensions::compression::deflate::DeflateConfig;
use tokio_tungstenite::tungstenite::extensions::ExtensionsConfig;
use tokio_tungstenite::tungstenite::handshake::client::{
    Request as WsClientRequest, Response as WsClientResponse,
};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;
use tokio_tungstenite::{client_async_tls_with_config, connect_async_tls_with_config};

use crate::http::codex_source::{
    response_create_client_metadata, ResponseCreateWsRequest, ResponsesWsRequest,
    RESPONSES_ENDPOINT, X_CODEX_PARENT_THREAD_ID_HEADER, X_CODEX_TURN_METADATA_HEADER,
    X_CODEX_WINDOW_ID_HEADER, X_OPENAI_SUBAGENT_HEADER,
};
use crate::http::proxy_response::{text_error_response, text_response};
use crate::storage_helpers::open_storage;

#[path = "responses_websocket_rebase.rs"]
mod responses_websocket_rebase;

use responses_websocket_rebase::{
    expand_response_create_previous_response, rebase_response_create_for_account_change,
    rebase_response_create_for_missing_tool_call, CompletedWsResponseCache,
    CompletedWsToolCallCache, WsToolCallKind,
};

const RESPONSES_WS_ERROR_CODE: &str = "responses_websocket_error";
const RESPONSES_WS_CONTEXT_REBASE_ERROR_CODE: &str = "responses_websocket_context_rebase_failed";
const RESPONSES_WEBSOCKETS_BETA_HEADER_VALUE: &str = "responses_websockets=2026-02-06";
const MAX_BUFFERED_WS_PREAMBLE_EVENTS: usize = 16;
// Resumed image-heavy turns are serialized as one response.create text frame. Keep the
// transport bounded, but large enough to match the existing 256 MiB compressed-body safety cap.
const RESPONSES_WS_MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;
// Keep both WebSocket legs active across long turns; TCP keepalive does not prevent
// application-layer proxy/NAT idle eviction.
const RESPONSES_WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE: &str = "websocket_connection_limit_reached";
const RESPONSES_WS_REQUEST_IN_FLIGHT_CODE: &str = "response_in_flight";
const WEBSOCKET_CONNECTION_LIMIT_REACHED_MESSAGE: &str =
    "Responses websocket connection limit reached (60 minutes). Create a new websocket connection to continue.";
// A freshly handshaken socket can still be reset before its first client frame reaches the
// upstream. Allow one additional fresh socket, but keep recovery bounded and replay-free.
const RESPONSES_WS_MAX_PENDING_FRAME_SEND_ATTEMPTS: usize = 2;
// A response can emit only connection preamble events before a transport reset. Reconnect the
// lane at most twice in that state; once substantive output exists, replay is not safe.
const RESPONSES_WS_MAX_PRE_COMPLETION_RECOVERY_ATTEMPTS: u8 = 2;

#[derive(Clone)]
struct WsRequestContext {
    api_key: codexmanager_core::storage::ApiKey,
    incoming_headers: crate::gateway::IncomingHeaderSnapshot,
    prompt_cache_key: Option<String>,
    route_conversation_id: Option<String>,
    route_conversation_source:
        Option<crate::gateway::conversation_binding::RouteConversationSource>,
    effective_upstream_base: String,
    prefer_raw_errors: bool,
}

#[derive(Clone)]
struct PreparedClientFrame {
    text: String,
    input: Value,
    client_model: Option<String>,
    model: Option<String>,
    previous_response_id: Option<String>,
    store: bool,
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
    conversation_routing: Option<crate::gateway::conversation_binding::ConversationRoutingContext>,
    forwarded_upstream_event: bool,
    forwarded_non_preamble_event: bool,
    upstream_disconnect_recovery_attempts: u8,
    suppress_replayed_preamble: bool,
    buffered_upstream_preamble: Vec<String>,
    buffer_retry_preamble: bool,
    attempted_account_ids: HashSet<String>,
    retried_missing_tool_call_context: bool,
}

type UpstreamWebsocketStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

struct ConnectedUpstreamWebsocket {
    stream: UpstreamWebsocketStream,
    account_id: String,
    account: codexmanager_core::storage::Account,
    conversation_routing: Option<crate::gateway::conversation_binding::ConversationRoutingContext>,
    candidate_account_ids: HashSet<String>,
    upstream_url: String,
    route_strategy: &'static str,
    route_source: &'static str,
}

#[derive(Clone)]
struct WsUpstreamAuthorization {
    value: String,
    task_id: Option<String>,
    uses_agent_identity: bool,
    is_fedramp: bool,
    account_scope_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct WsConnectError {
    message: String,
    status_code: Option<u16>,
    response_body: Vec<u8>,
}

impl WsConnectError {
    fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status_code: None,
            response_body: Vec::new(),
        }
    }

    fn from_tungstenite(err: tokio_tungstenite::tungstenite::Error) -> Self {
        let (status_code, response_body) = match &err {
            tokio_tungstenite::tungstenite::Error::Http(response) => (
                Some(response.status().as_u16()),
                response.body().as_deref().unwrap_or_default().to_vec(),
            ),
            _ => (None, Vec::new()),
        };
        Self {
            message: err.to_string(),
            status_code,
            response_body,
        }
    }

    fn is_unauthorized(&self) -> bool {
        self.status_code == Some(401)
    }

    fn is_agent_identity_task_invalid(&self) -> bool {
        crate::agent_identity::is_agent_identity_task_invalid_response(
            self.status_code.unwrap_or_default(),
            &self.response_body,
        ) || crate::agent_identity::is_agent_identity_task_invalid_error(&self.message)
    }

    fn is_websocket_connection_limit_reached(&self) -> bool {
        if self
            .message
            .to_ascii_lowercase()
            .contains("websocket connection limit reached")
        {
            return true;
        }
        let body = String::from_utf8_lossy(&self.response_body).to_ascii_lowercase();
        body.contains(WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE)
            || body.contains(&WEBSOCKET_CONNECTION_LIMIT_REACHED_MESSAGE.to_ascii_lowercase())
    }

    fn is_compression_negotiation_rejection(&self) -> bool {
        let message = self.message.to_ascii_lowercase();
        let body = String::from_utf8_lossy(&self.response_body).to_ascii_lowercase();
        let extension_signal = message.contains("sec-websocket-extensions")
            || message.contains("permessage-deflate")
            || body.contains("sec-websocket-extensions")
            || body.contains("permessage-deflate")
            || body.contains("unsupported extension")
            || body.contains("compression extension");
        let rejected_handshake = matches!(self.status_code, Some(400 | 426));
        extension_signal && (rejected_handshake || self.status_code.is_none())
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
    estimated_input_tokens: i64,
}

#[derive(Debug)]
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

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(503, RESPONSES_WS_ERROR_CODE, message)
    }

    fn context_rebase_failed(message: impl Into<String>) -> Self {
        Self::new(409, RESPONSES_WS_CONTEXT_REBASE_ERROR_CODE, message)
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

    ws.max_message_size(RESPONSES_WS_MAX_MESSAGE_BYTES)
        .max_frame_size(RESPONSES_WS_MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| async move {
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

    if let Err(err) = validate_ws_api_key_for_new_request(&context).await {
        record_rejected_ws_request(&context, &err);
        send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
        return;
    }

    let prepared_first =
        match rewrite_client_frame_with_model_fast_policy(first_text.as_str(), &context).await {
            Ok(prepared) => prepared,
            Err(err) => {
                record_rejected_ws_request(&context, &err);
                send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                return;
            }
        };

    let mut first_log = begin_ws_request_log(
        &context,
        &prepared_first,
        "unresolved",
        "initial_upstream_connect",
    );
    let mut upstream = match connect_upstream_websocket_with_timeout(
        &context,
        prepared_first.model.as_deref(),
        None,
    )
    .await
    {
        Ok(stream) => stream,
        Err(err) => {
            finalize_ws_request_log(
                &context,
                &first_log,
                None,
                None,
                err.status,
                crate::gateway::RequestLogUsage::default(),
                Some(err.message.clone()),
            );
            send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
            return;
        }
    };
    first_log.route_strategy = Some(upstream.route_strategy.to_string());
    first_log.route_source = Some(upstream.route_source.to_string());
    let first_attempted_account_ids = HashSet::from([upstream.account_id.clone()]);
    let mut first_pending = PendingWsRequestState {
        log: first_log,
        prepared: prepared_first.clone(),
        conversation_routing: upstream.conversation_routing.clone(),
        forwarded_upstream_event: false,
        forwarded_non_preamble_event: false,
        upstream_disconnect_recovery_attempts: 0,
        suppress_replayed_preamble: false,
        buffered_upstream_preamble: Vec::new(),
        buffer_retry_preamble: should_buffer_ws_retry_preamble(
            &upstream,
            &first_attempted_account_ids,
            prepared_first.text.as_str(),
            false,
        ),
        attempted_account_ids: first_attempted_account_ids,
        retried_missing_tool_call_context: false,
    };

    let mut completed_tool_calls = CompletedWsToolCallCache::default();
    let mut completed_responses = CompletedWsResponseCache::default();
    if let Err(err) = upstream
        .stream
        .send(UpstreamMessage::Text(
            first_pending.prepared.text.clone().into(),
        ))
        .await
    {
        let previous_account_id = upstream.account_id.clone();
        log::warn!(
            "event=responses_ws_initial_stale_send account_id={} frame_bytes={} err={err}",
            previous_account_id,
            first_pending.prepared.text.len(),
        );
        let _ = upstream.stream.close(None).await;
        match reconnect_upstream_for_pending_request(
            &context,
            &mut first_pending,
            Some(previous_account_id.as_str()),
            &completed_responses,
            &completed_tool_calls,
        )
        .await
        {
            Ok(replacement) => {
                log::info!(
                    "event=responses_ws_upstream_reconnected previous_account_id={} account_id={} reason=initial_send",
                    previous_account_id,
                    replacement.account_id,
                );
                upstream = replacement;
            }
            Err(err) => {
                let log_error = crate::gateway::bilingual_error(
                    "上游 WebSocket 首帧失败后的有限恢复失败",
                    format!(
                        "initial upstream websocket send recovery failed: {}",
                        err.message
                    ),
                );
                let session_error = WsSessionError::new(err.status, err.code, log_error.clone());
                finalize_ws_request_log(
                    &context,
                    &first_pending.log,
                    Some(previous_account_id.as_str()),
                    None,
                    session_error.status,
                    crate::gateway::RequestLogUsage::default(),
                    Some(log_error),
                );
                send_ws_error_and_close(&mut socket, session_error, context.prefer_raw_errors)
                    .await;
                return;
            }
        }
    }
    let mut pending_request = Some(first_pending);
    let mut heartbeat = responses_ws_heartbeat_interval();

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Err(err) = socket.send(Message::Ping(Vec::new().into())).await {
                    log::info!("event=responses_ws_client_heartbeat_failed err={err}");
                    let _ = upstream.stream.close(None).await;
                    break;
                }
                if let Err(err) = upstream
                    .stream
                    .send(UpstreamMessage::Ping(Vec::new().into()))
                    .await
                {
                    log::info!(
                        "event=responses_ws_upstream_heartbeat_failed account_id={} err={err}",
                        upstream.account_id,
                    );
                }
                if pending_request.is_none() {
                    match ws_account_requires_reselection(&context, &upstream, None) {
                        Ok(true) => {
                            log::info!(
                                "event=responses_ws_idle_account_invalidated account_id={} reason=account_eligibility_changed",
                                upstream.account_id,
                            );
                            let _ = upstream.stream.close(None).await;
                        }
                        Ok(false) => {}
                        Err(err) => {
                            log::warn!(
                                "event=responses_ws_idle_account_check_failed account_id={} err={}",
                                upstream.account_id,
                                err.message,
                            );
                        }
                    }
                } else {
                    match ws_account_is_unavailable(
                        &context,
                        &upstream,
                        pending_request
                            .as_ref()
                            .and_then(|pending| pending.prepared.model.as_deref()),
                    ) {
                        Ok(true) => {
                            log::info!(
                                "event=responses_ws_inflight_account_invalidated account_id={} reason=account_no_longer_selectable",
                                upstream.account_id,
                            );
                            let _ = upstream.stream.close(None).await;
                        }
                        Ok(false) => {}
                        Err(err) => {
                            log::warn!(
                                "event=responses_ws_inflight_account_check_failed account_id={} err={}",
                                upstream.account_id,
                                err.message,
                            );
                            let _ = upstream.stream.close(None).await;
                        }
                    }
                }
            }
            maybe_client = socket.recv() => {
                let Some(client_result) = maybe_client else {
                    let _ = upstream.stream.close(None).await;
                    break;
                };
                match client_result {
                    Ok(Message::Text(text)) => {
                        match rewrite_client_frame(text.as_str(), &context) {
                            Ok(prepared) => {
                                if pending_request.is_some() {
                                    log::warn!(
                                        "event=responses_ws_request_rejected_while_in_flight account_id={} upstream_url={}",
                                        upstream.account_id,
                                        upstream.upstream_url,
                                    );
                                    send_ws_error(
                                        &mut socket,
                                        WsSessionError::new(
                                            400,
                                            RESPONSES_WS_REQUEST_IN_FLIGHT_CODE,
                                            crate::gateway::bilingual_error(
                                                "当前 WebSocket 响应尚未完成",
                                                "another response is already in flight on this websocket connection",
                                            ),
                                        ),
                                        context.prefer_raw_errors,
                                    )
                                    .await;
                                    continue;
                                }
                                if let Err(err) = validate_ws_api_key_for_new_request(&context).await {
                                    record_rejected_ws_request(&context, &err);
                                    send_ws_error_and_close(
                                        &mut socket,
                                        err,
                                        context.prefer_raw_errors,
                                    )
                                    .await;
                                    let _ = upstream.stream.close(None).await;
                                    break;
                                }
                                let prepared = match apply_model_fast_policy(prepared).await {
                                    Ok(prepared) => prepared,
                                    Err(err) => {
                                        record_rejected_ws_request(&context, &err);
                                        send_ws_error_and_close(
                                            &mut socket,
                                            err,
                                            context.prefer_raw_errors,
                                        )
                                        .await;
                                        let _ = upstream.stream.close(None).await;
                                        break;
                                    }
                                };
                                let attempted_account_ids =
                                    HashSet::from([upstream.account_id.clone()]);
                                let buffer_retry_preamble = should_buffer_ws_retry_preamble(
                                    &upstream,
                                    &attempted_account_ids,
                                    prepared.text.as_str(),
                                    false,
                                );
                                let mut current_pending = PendingWsRequestState {
                                    log: begin_ws_request_log(
                                        &context,
                                        &prepared,
                                        upstream.route_strategy,
                                        upstream.route_source,
                                    ),
                                    prepared,
                                    conversation_routing: upstream.conversation_routing.clone(),
                                    forwarded_upstream_event: false,
                                    forwarded_non_preamble_event: false,
                                    upstream_disconnect_recovery_attempts: 0,
                                    suppress_replayed_preamble: false,
                                    buffered_upstream_preamble: Vec::new(),
                                    buffer_retry_preamble,
                                    attempted_account_ids,
                                    retried_missing_tool_call_context: false,
                                };
                                let (must_reselect_account, fresh_conversation_routing, fresh_route_strategy, fresh_route_source) = match ws_collect_routed_candidates(
                                    &context,
                                    current_pending.prepared.model.as_deref(),
                                ) {
                                    Ok(routed) => (
                                        crate::gateway::gateway_ws_account_requires_switch(
                                            &routed,
                                            upstream.account_id.as_str(),
                                        ),
                                        routed.conversation_routing,
                                        routed.route_strategy,
                                        routed.route_source,
                                    ),
                                    Err(err) => {
                                        finalize_ws_request_log(
                                            &context,
                                            &current_pending.log,
                                            Some(upstream.account_id.as_str()),
                                            Some(upstream.upstream_url.as_str()),
                                            err.status,
                                            crate::gateway::RequestLogUsage::default(),
                                            Some(err.message.clone()),
                                        );
                                        send_ws_error_and_close(
                                            &mut socket,
                                            err,
                                            context.prefer_raw_errors,
                                        )
                                        .await;
                                        break;
                                    }
                                };
                                if !must_reselect_account {
                                    upstream.conversation_routing =
                                        fresh_conversation_routing.clone();
                                    upstream.route_strategy = fresh_route_strategy;
                                    upstream.route_source = fresh_route_source;
                                    current_pending.conversation_routing =
                                        fresh_conversation_routing;
                                    current_pending.log.route_strategy =
                                        Some(fresh_route_strategy.to_string());
                                    current_pending.log.route_source =
                                        Some(fresh_route_source.to_string());
                                }
                                if must_reselect_account {
                                    let previous_account_id = upstream.account_id.clone();
                                    log::info!(
                                        "event=responses_ws_account_reselect_before_turn account_id={} model={} reason=account_eligibility_changed",
                                        previous_account_id,
                                        current_pending.prepared.model.as_deref().unwrap_or("-"),
                                    );
                                    let _ = upstream.stream.close(None).await;
                                    match reconnect_upstream_for_pending_request(
                                        &context,
                                        &mut current_pending,
                                        Some(previous_account_id.as_str()),
                                        &completed_responses,
                                        &completed_tool_calls,
                                    )
                                    .await
                                    {
                                        Ok(replacement) => {
                                            log::info!(
                                                "event=responses_ws_account_reselected_before_turn_complete previous_account_id={} account_id={}",
                                                previous_account_id,
                                                replacement.account_id,
                                            );
                                            upstream = replacement;
                                        }
                                        Err(err) => {
                                            finalize_ws_request_log(
                                                &context,
                                                &current_pending.log,
                                                None,
                                                None,
                                                err.status,
                                                crate::gateway::RequestLogUsage::default(),
                                                Some(err.message.clone()),
                                            );
                                            send_ws_error_and_close(
                                                &mut socket,
                                                err,
                                                context.prefer_raw_errors,
                                            )
                                            .await;
                                            break;
                                        }
                                    }
                                } else if let Err(send_err) = upstream.stream.send(
                                    UpstreamMessage::Text(current_pending.prepared.text.clone().into()),
                                ).await {
                                    let previous_account_id = upstream.account_id.clone();
                                    log::warn!(
                                        "event=responses_ws_upstream_stale_send account_id={} err={send_err}",
                                        previous_account_id,
                                    );
                                    let _ = upstream.stream.close(None).await;
                                    match reconnect_upstream_for_pending_request(
                                        &context,
                                        &mut current_pending,
                                        Some(previous_account_id.as_str()),
                                        &completed_responses,
                                        &completed_tool_calls,
                                    )
                                    .await
                                    {
                                        Ok(replacement) => {
                                            log::info!(
                                                "event=responses_ws_upstream_reconnected previous_account_id={} account_id={} reason=stale_send",
                                                previous_account_id,
                                                replacement.account_id,
                                            );
                                            upstream = replacement;
                                        }
                                        Err(err) => {
                                            finalize_ws_request_log(
                                                &context,
                                                &current_pending.log,
                                                None,
                                                None,
                                                err.status,
                                                crate::gateway::RequestLogUsage::default(),
                                                Some(err.message.clone()),
                                            );
                                            send_ws_error_and_close(
                                                &mut socket,
                                                err,
                                                context.prefer_raw_errors,
                                            )
                                            .await;
                                            break;
                                        }
                                    }
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
                    Ok(Message::Binary(_)) => {
                        send_ws_error(
                            &mut socket,
                            WsSessionError::bad_request_bilingual(
                                "Responses WebSocket 只接受 response.create 文本帧",
                                "Responses WebSocket accepts response.create text frames only",
                            ),
                            context.prefer_raw_errors,
                        )
                        .await;
                    }
                    Ok(Message::Close(_)) => {
                        let _ = upstream.stream.close(None).await;
                        break;
                    }
                    Err(err) => {
                        log::warn!("event=responses_ws_client_receive_failed err={err}");
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
                    if pending_request.is_some() {
                        let retry_result = retry_pending_request_after_upstream_disconnect(
                            &context,
                            &mut upstream,
                            pending_request
                                .as_mut()
                                .expect("pending request checked above"),
                            &completed_responses,
                            &completed_tool_calls,
                            "early_eof",
                        )
                        .await;
                        match retry_result {
                            Ok(true) => {
                                continue;
                            }
                            Ok(false) => {}
                            Err(err) => {
                                if let Some(pending) = pending_request.take() {
                                    finalize_ws_request_log(
                                        &context,
                                        &pending.log,
                                        Some(upstream.account_id.as_str()),
                                        Some(upstream.upstream_url.as_str()),
                                        err.status,
                                        crate::gateway::RequestLogUsage::default(),
                                        Some(err.message.clone()),
                                    );
                                }
                                send_ws_error_and_close(
                                    &mut socket,
                                    err,
                                    context.prefer_raw_errors,
                                )
                                .await;
                                break;
                            }
                        }
                        let err = WsSessionError::bad_gateway_bilingual(
                            "上游 WebSocket 在响应完成前断开",
                            "upstream websocket disconnected before response completion",
                        );
                        if let Some(pending) = pending_request.take() {
                            finalize_ws_request_log(
                                &context,
                                &pending.log,
                                Some(upstream.account_id.as_str()),
                                Some(upstream.upstream_url.as_str()),
                                err.status,
                                crate::gateway::RequestLogUsage::default(),
                                Some(err.message.clone()),
                            );
                        }
                        send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                        break;
                    }
                    let previous_account_id = upstream.account_id.clone();
                    log::info!(
                        "event=responses_ws_upstream_idle_disconnected account_id={} reason=eof",
                        previous_account_id,
                    );
                    match wait_for_client_request_and_reconnect_upstream(
                        &mut socket,
                        &context,
                        previous_account_id.as_str(),
                        &completed_responses,
                        &completed_tool_calls,
                    )
                    .await
                    {
                        Ok(Some((replacement, pending))) => {
                            log::info!(
                                "event=responses_ws_upstream_reconnected previous_account_id={} account_id={} reason=eof",
                                previous_account_id,
                                replacement.account_id,
                            );
                                upstream = replacement;
                                pending_request = Some(pending);
                            continue;
                        }
                        Ok(None) => break,
                        Err(err) => {
                            send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                            break;
                        }
                    }
                };
                match upstream_result {
                    Ok(UpstreamMessage::Text(text)) => {
                        completed_tool_calls.observe_upstream_event(text.as_str());
                        if let Some(terminal) = inspect_ws_terminal_event(text.as_str()) {
                            if terminal.is_websocket_connection_limit {
                                if let Some(pending) = pending_request.as_mut() {
                                    let previous_account_id = upstream.account_id.clone();
                                    let _ = upstream.stream.close(None).await;
                                    match retry_pending_request_after_upstream_disconnect(
                                        &context,
                                        &mut upstream,
                                        pending,
                                        &completed_responses,
                                        &completed_tool_calls,
                                        "connection_limit_reached",
                                    )
                                    .await
                                    {
                                        Ok(true) => {
                                            log::info!(
                                                "event=responses_ws_connection_limit_recovered previous_account_id={} account_id={}",
                                                previous_account_id,
                                                upstream.account_id,
                                            );
                                            continue;
                                        }
                                        Ok(false) => {}
                                        Err(err) => {
                                            let log_error = err.message.clone();
                                            if let Some(pending) = pending_request.take() {
                                                finalize_ws_request_log(
                                                    &context,
                                                    &pending.log,
                                                    Some(upstream.account_id.as_str()),
                                                    Some(upstream.upstream_url.as_str()),
                                                    err.status,
                                                    crate::gateway::RequestLogUsage::default(),
                                                    Some(log_error),
                                                );
                                            }
                                            send_ws_error_and_close(
                                                &mut socket,
                                                err,
                                                context.prefer_raw_errors,
                                            )
                                            .await;
                                            break;
                                        }
                                    }
                                } else {
                                    let previous_account_id = upstream.account_id.clone();
                                    let _ = upstream.stream.close(None).await;
                                    match wait_for_client_request_and_reconnect_upstream(
                                        &mut socket,
                                        &context,
                                        previous_account_id.as_str(),
                                        &completed_responses,
                                        &completed_tool_calls,
                                    )
                                    .await
                                    {
                                        Ok(Some((replacement, pending))) => {
                                            log::info!(
                                                "event=responses_ws_connection_limit_recovered previous_account_id={} account_id={} reason=idle",
                                                previous_account_id,
                                                replacement.account_id,
                                            );
                                            upstream = replacement;
                                            pending_request = Some(pending);
                                            continue;
                                        }
                                        Ok(None) => break,
                                        Err(err) => {
                                            send_ws_error_and_close(
                                                &mut socket,
                                                err,
                                                context.prefer_raw_errors,
                                            )
                                            .await;
                                            break;
                                        }
                                    }
                                }
                            }
                            apply_ws_terminal_account_follow_up(
                                upstream.account_id.as_str(),
                                &terminal,
                            );
                            let retry_result = if let Some(pending) = pending_request.as_mut() {
                                if !pending.forwarded_upstream_event {
                                    try_retry_ws_request_after_terminal(
                                        &context,
                                        &mut upstream,
                                        pending,
                                        &terminal,
                                        &completed_responses,
                                        &completed_tool_calls,
                                    )
                                    .await
                                } else {
                                    Ok(false)
                                }
                            } else {
                                Ok(false)
                            };
                            match retry_result {
                                Ok(true) => continue,
                                Ok(false) => {}
                                Err(err) => {
                                    let log_error = err.message.clone();
                                    if let Some(pending) = pending_request.take() {
                                        finalize_ws_request_log(
                                            &context,
                                            &pending.log,
                                            Some(upstream.account_id.as_str()),
                                            Some(upstream.upstream_url.as_str()),
                                            err.status,
                                            terminal.usage,
                                            Some(log_error),
                                        );
                                    }
                                    send_ws_error_and_close(
                                        &mut socket,
                                        err,
                                        context.prefer_raw_errors,
                                    )
                                    .await;
                                    let _ = upstream.stream.close(None).await;
                                    break;
                                }
                            }

                            if let Some(mut pending) = pending_request.take() {
                                if terminal.status_code == 200 {
                                    cache_completed_ws_response(
                                        &mut completed_responses,
                                        &pending.prepared,
                                        text.as_str(),
                                    );
                                }
                                if let Err(err) = flush_ws_upstream_preamble(&mut socket, &mut pending).await {
                                    log::warn!("event=responses_ws_client_send_preamble_failed err={err}");
                                    break;
                                }
                                mark_ws_first_response(&mut pending);
                                if let Some(storage) = open_storage() {
                                    if let Err(err) = crate::gateway::conversation_binding::record_conversation_binding_terminal_response(
                                        &storage,
                                        pending.conversation_routing.as_ref(),
                                        &upstream.account,
                                        pending.prepared.model.as_deref(),
                                        terminal.status_code,
                                    ) {
                                        log::warn!(
                                            "event=responses_ws_conversation_binding_record_failed account_id={} status={} err={}",
                                            upstream.account_id,
                                            terminal.status_code,
                                            err,
                                        );
                                    }
                                }
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
                            continue;
                        }

                        if let Some(pending) = pending_request.as_mut() {
                            if pending.suppress_replayed_preamble
                                && should_buffer_ws_upstream_preamble(text.as_str(), 0)
                            {
                                continue;
                            }
                            let should_buffer_preamble = pending.buffer_retry_preamble
                                && should_buffer_ws_upstream_preamble(
                                text.as_str(),
                                pending.buffered_upstream_preamble.len(),
                            );
                            if should_buffer_preamble {
                                pending
                                    .buffered_upstream_preamble
                                    .push(text.to_string());
                                continue;
                            }
                            if let Err(err) = flush_ws_upstream_preamble(&mut socket, pending).await {
                                log::warn!("event=responses_ws_client_send_preamble_failed err={err}");
                                break;
                            }
                            mark_ws_forwarded_event(pending, text.as_str());
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
                        if let Some(pending) = pending_request.as_mut() {
                            if let Err(err) = flush_ws_upstream_preamble(&mut socket, pending).await {
                                log::warn!("event=responses_ws_client_send_preamble_failed err={err}");
                                break;
                            }
                            mark_ws_first_response(pending);
                            pending.forwarded_non_preamble_event = true;
                            pending.suppress_replayed_preamble = false;
                        }
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
                        if pending_request.is_some() {
                            let retry_result = retry_pending_request_after_upstream_disconnect(
                                &context,
                                &mut upstream,
                                pending_request
                                    .as_mut()
                                    .expect("pending request checked above"),
                                &completed_responses,
                                &completed_tool_calls,
                                "early_close",
                            )
                            .await;
                            match retry_result {
                                Ok(true) => {
                                    continue;
                                }
                                Ok(false) => {}
                                Err(err) => {
                                    if let Some(pending) = pending_request.take() {
                                        finalize_ws_request_log(
                                            &context,
                                            &pending.log,
                                            Some(upstream.account_id.as_str()),
                                            Some(upstream.upstream_url.as_str()),
                                            err.status,
                                            crate::gateway::RequestLogUsage::default(),
                                            Some(err.message.clone()),
                                        );
                                    }
                                    send_ws_error_and_close(
                                        &mut socket,
                                        err,
                                        context.prefer_raw_errors,
                                    )
                                    .await;
                                    break;
                                }
                            }
                            let err = WsSessionError::bad_gateway_bilingual(
                                "上游 WebSocket 在响应完成前关闭",
                                "upstream websocket closed before response completion",
                            );
                            if let Some(pending) = pending_request.take() {
                                finalize_ws_request_log(
                                    &context,
                                    &pending.log,
                                    Some(upstream.account_id.as_str()),
                                    Some(upstream.upstream_url.as_str()),
                                    err.status,
                                    crate::gateway::RequestLogUsage::default(),
                                    Some(err.message.clone()),
                                );
                            }
                            send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                            break;
                        }
                        let previous_account_id = upstream.account_id.clone();
                        log::info!(
                            "event=responses_ws_upstream_idle_disconnected account_id={} reason=close",
                            previous_account_id,
                        );
                        match wait_for_client_request_and_reconnect_upstream(
                            &mut socket,
                            &context,
                            previous_account_id.as_str(),
                            &completed_responses,
                            &completed_tool_calls,
                        )
                        .await
                        {
                            Ok(Some((replacement, pending))) => {
                                log::info!(
                                    "event=responses_ws_upstream_reconnected previous_account_id={} account_id={} reason=close",
                                    previous_account_id,
                                    replacement.account_id,
                                );
                                upstream = replacement;
                                pending_request = Some(pending);
                                continue;
                            }
                            Ok(None) => break,
                            Err(err) => {
                                send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                                break;
                            }
                        }
                    }
                    Ok(UpstreamMessage::Frame(_)) => {}
                    Err(read_err) => {
                        if pending_request.is_some() {
                            let retry_result = retry_pending_request_after_upstream_disconnect(
                                &context,
                                &mut upstream,
                                pending_request
                                    .as_mut()
                                    .expect("pending request checked above"),
                                &completed_responses,
                                &completed_tool_calls,
                                "early_read_error",
                            )
                            .await;
                            match retry_result {
                                Ok(true) => {
                                    continue;
                                }
                                Ok(false) => {}
                                Err(err) => {
                                    if let Some(pending) = pending_request.take() {
                                        finalize_ws_request_log(
                                            &context,
                                            &pending.log,
                                            Some(upstream.account_id.as_str()),
                                            Some(upstream.upstream_url.as_str()),
                                            err.status,
                                            crate::gateway::RequestLogUsage::default(),
                                            Some(err.message.clone()),
                                        );
                                    }
                                    send_ws_error_and_close(
                                        &mut socket,
                                        err,
                                        context.prefer_raw_errors,
                                    )
                                    .await;
                                    break;
                                }
                            }
                            let err = WsSessionError::bad_gateway_bilingual(
                                "接收上游 WebSocket 帧失败",
                                format!("receive upstream websocket frame failed: {read_err}"),
                            );
                            if let Some(pending) = pending_request.take() {
                                finalize_ws_request_log(
                                    &context,
                                    &pending.log,
                                    Some(upstream.account_id.as_str()),
                                    Some(upstream.upstream_url.as_str()),
                                    err.status,
                                    crate::gateway::RequestLogUsage::default(),
                                    Some(err.message.clone()),
                                );
                            }
                            send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                            break;
                        }
                        let previous_account_id = upstream.account_id.clone();
                        log::info!(
                            "event=responses_ws_upstream_idle_disconnected account_id={} reason=read_error err={read_err}",
                            previous_account_id,
                        );
                        match wait_for_client_request_and_reconnect_upstream(
                            &mut socket,
                            &context,
                            previous_account_id.as_str(),
                            &completed_responses,
                            &completed_tool_calls,
                        )
                        .await
                        {
                            Ok(Some((replacement, pending))) => {
                                log::info!(
                                    "event=responses_ws_upstream_reconnected previous_account_id={} account_id={} reason=read_error",
                                    previous_account_id,
                                    replacement.account_id,
                                );
                            upstream = replacement;
                            pending_request = Some(pending);
                                continue;
                            }
                            Ok(None) => break,
                            Err(err) => {
                                send_ws_error_and_close(&mut socket, err, context.prefer_raw_errors).await;
                                break;
                            }
                        }
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
    let api_key =
        crate::gateway::load_active_gateway_api_key(&storage, platform_key, RESPONSES_ENDPOINT)
            .map_err(|(status, message)| {
                text_error_response(
                    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    crate::gateway::error_message_for_client(prefer_raw_errors, message),
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

    let routing =
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
        incoming_headers: routing.incoming_headers,
        prompt_cache_key: routing.prompt_cache_key,
        route_conversation_id: routing.route_conversation_id,
        route_conversation_source: routing.route_conversation_source,
        prefer_raw_errors,
    })
}

async fn validate_ws_api_key_for_new_request(
    context: &WsRequestContext,
) -> Result<(), WsSessionError> {
    let key_id = context.api_key.id.clone();
    tokio::task::spawn_blocking(move || {
        let storage = open_storage().ok_or_else(|| {
            WsSessionError::new(
                500,
                RESPONSES_WS_ERROR_CODE,
                crate::gateway::bilingual_error("存储不可用", "storage unavailable"),
            )
        })?;
        crate::gateway::load_active_gateway_api_key_by_id(
            &storage,
            key_id.as_str(),
            RESPONSES_ENDPOINT,
        )
        .map(|_| ())
        .map_err(|(status, message)| WsSessionError::new(status, RESPONSES_WS_ERROR_CODE, message))
    })
    .await
    .map_err(|err| {
        WsSessionError::new(
            500,
            RESPONSES_WS_ERROR_CODE,
            crate::gateway::bilingual_error(
                "处理 WebSocket API Key 校验任务失败",
                format!("websocket api key validation task failed: {err}"),
            ),
        )
    })?
}

async fn receive_initial_request(socket: &mut WebSocket) -> Result<Option<String>, WsSessionError> {
    let mut heartbeat = responses_ws_heartbeat_interval();
    loop {
        let message = tokio::select! {
            maybe_message = socket.recv() => {
                let Some(message) = maybe_message else {
                    return Ok(None);
                };
                message
            }
            _ = heartbeat.tick() => {
                if let Err(err) = socket.send(Message::Ping(Vec::new().into())).await {
                    log::info!("event=responses_ws_client_heartbeat_failed_during_wait err={err}");
                    return Ok(None);
                }
                continue;
            }
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
                log::warn!("event=responses_ws_client_initial_receive_failed err={err}");
                return Err(WsSessionError::bad_request_bilingual(
                    "接收首个 WebSocket 帧失败",
                    format!("receive initial websocket frame failed: {err}"),
                ));
            }
        }
    }
}

fn responses_ws_heartbeat_interval() -> tokio::time::Interval {
    tokio::time::interval_at(
        tokio::time::Instant::now() + RESPONSES_WS_HEARTBEAT_INTERVAL,
        RESPONSES_WS_HEARTBEAT_INTERVAL,
    )
}

async fn rewrite_client_frame_with_model_fast_policy(
    text: &str,
    context: &WsRequestContext,
) -> Result<PreparedClientFrame, WsSessionError> {
    let prepared = rewrite_client_frame(text, context)?;
    apply_model_fast_policy(prepared).await
}

async fn apply_model_fast_policy(
    prepared: PreparedClientFrame,
) -> Result<PreparedClientFrame, WsSessionError> {
    tokio::task::spawn_blocking(move || {
        let storage = open_storage().ok_or_else(|| {
            WsSessionError::new(
                500,
                RESPONSES_WS_ERROR_CODE,
                crate::gateway::bilingual_error("存储不可用", "storage unavailable"),
            )
        })?;
        apply_model_fast_policy_with_storage(prepared, &storage)
    })
    .await
    .map_err(|err| {
        WsSessionError::new(
            500,
            RESPONSES_WS_ERROR_CODE,
            crate::gateway::bilingual_error(
                "处理 WebSocket 模型策略任务失败",
                format!("websocket model policy task failed: {err}"),
            ),
        )
    })?
}

fn apply_model_fast_policy_with_storage(
    mut prepared: PreparedClientFrame,
    storage: &codexmanager_core::storage::Storage,
) -> Result<PreparedClientFrame, WsSessionError> {
    let Some(model_slug) = prepared
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(prepared);
    };
    let model = storage
        .get_enabled_model_v2(model_slug)
        .map_err(|err| {
            WsSessionError::new(
                500,
                RESPONSES_WS_ERROR_CODE,
                format!("model_catalog_v2_read_failed: {err}"),
            )
        })?
        .ok_or_else(|| {
            WsSessionError::new(
                404,
                "model_not_found",
                format!("model_not_found: {model_slug}"),
            )
        })?;
    let (body, applied) = crate::models_v2::fast_policy::apply(
        prepared.text.as_bytes().to_vec(),
        model.fast_policy,
        prepared.has_service_tier_field,
    )
    .map_err(|_| {
        WsSessionError::new(
            400,
            crate::models_v2::fast_policy::FAST_REQUEST_BLOCKED,
            crate::gateway::bilingual_error(
                format!("模型 {model_slug} 不允许 Fast 请求"),
                format!("model {model_slug} does not allow Fast requests"),
            ),
        )
    })?;
    if !applied {
        return Ok(prepared);
    }

    let value = serde_json::from_slice::<Value>(&body).map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "读取模型 Fast 策略结果失败",
            format!("parse model fast policy result failed: {err}"),
        )
    })?;
    prepared.effective_service_tier = value
        .get("service_tier")
        .and_then(Value::as_str)
        .and_then(crate::apikey::service_tier::normalize_service_tier_for_log)
        .map(str::to_string);
    prepared.service_tier_source = Some("model_policy".to_string());
    prepared.text = String::from_utf8(body).map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "序列化模型 Fast 策略结果失败",
            format!("serialize model fast policy result failed: {err}"),
        )
    })?;
    Ok(prepared)
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
    // `stream` and `background` are HTTP transport controls. Responses
    // WebSocket mode sends response.create events directly and does not use
    // either field, even when the incoming Codex-compatible payload contains
    // them.
    object.remove("stream");
    object.remove("background");
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
    let rewritten_body = crate::gateway::align_existing_prompt_cache_key_with_native_anchor(
        rewritten_body,
        &context.incoming_headers,
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
    // Keep the transport-field boundary after gateway rewriting as well: a
    // downstream override must not reintroduce HTTP-only WebSocket fields.
    rewritten_object.remove("stream");
    rewritten_object.remove("background");
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
    let reasoning_source = resolve_ws_reasoning_source_for_log(
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
        input: request.input,
        client_model: client_model_for_log,
        model: Some(request.model),
        previous_response_id: request.previous_response_id,
        store: request.store,
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

fn resolve_ws_reasoning_source_for_log(
    client_value: Option<&str>,
    effective_value: Option<&str>,
    api_key_profile_value: Option<&str>,
) -> Option<String> {
    if api_key_profile_value
        .map(str::trim)
        .is_none_or(str::is_empty)
        && crate::reasoning_effort::is_ultra_to_max_normalization(client_value, effective_value)
    {
        return Some("client_request_normalized".to_string());
    }
    resolve_ws_override_source_for_log(client_value, effective_value, api_key_profile_value)
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

fn ws_collect_routed_candidates(
    context: &WsRequestContext,
    model: Option<&str>,
) -> Result<crate::gateway::GatewayRoutedCandidates, WsSessionError> {
    let storage = open_storage().ok_or_else(|| {
        WsSessionError::service_unavailable_bilingual("存储不可用", "storage unavailable")
    })?;
    crate::gateway::gateway_collect_routed_candidates_for_ws(
        &storage,
        &context.api_key.id,
        model,
        context.route_conversation_id.as_deref(),
        context.route_conversation_source,
    )
    .map_err(|err| {
        WsSessionError::service_unavailable_bilingual(
            "读取 WebSocket 账号候选失败",
            format!("read websocket account candidates failed: {err}"),
        )
    })
}

fn ws_account_requires_reselection(
    context: &WsRequestContext,
    upstream: &ConnectedUpstreamWebsocket,
    model: Option<&str>,
) -> Result<bool, WsSessionError> {
    let routed = ws_collect_routed_candidates(context, model)?;
    Ok(crate::gateway::gateway_ws_account_requires_switch(
        &routed,
        upstream.account_id.as_str(),
    ))
}

fn ws_account_is_unavailable(
    context: &WsRequestContext,
    upstream: &ConnectedUpstreamWebsocket,
    model: Option<&str>,
) -> Result<bool, WsSessionError> {
    let routed = ws_collect_routed_candidates(context, model)?;
    Ok(!routed
        .candidates
        .iter()
        .any(|(account, _)| account.id == upstream.account_id))
}

async fn connect_upstream_websocket_excluding_accounts(
    context: &WsRequestContext,
    model: Option<&str>,
    previous_account_id: Option<&str>,
    excluded_account_ids: &HashSet<String>,
) -> Result<ConnectedUpstreamWebsocket, WsSessionError> {
    let storage = open_storage().ok_or_else(|| {
        WsSessionError::service_unavailable_bilingual("存储不可用", "storage unavailable")
    })?;
    let routed = crate::gateway::gateway_collect_routed_candidates_for_ws(
        &storage,
        &context.api_key.id,
        model,
        context.route_conversation_id.as_deref(),
        context.route_conversation_source,
    )?;
    if routed.candidates.is_empty() {
        return Err(WsSessionError::service_unavailable_bilingual(
            "没有可用的上游账号",
            "no available upstream accounts",
        ));
    }
    let candidate_account_ids = routed
        .candidates
        .iter()
        .map(|(account, _)| account.id.clone())
        .collect::<HashSet<_>>();
    let conversation_routing = routed.conversation_routing.clone();
    let has_unexcluded_candidate = routed
        .candidates
        .iter()
        .any(|(account, _)| !excluded_account_ids.contains(account.id.as_str()));
    let candidates = routed
        .candidates
        .into_iter()
        .filter(|(account, _)| {
            !has_unexcluded_candidate || !excluded_account_ids.contains(account.id.as_str())
        })
        .collect::<Vec<_>>();
    drop(storage);

    let ws_url = build_upstream_websocket_url(&context.effective_upstream_base)?;
    let mut last_error = None;
    for (account, token) in candidates {
        match connect_account_upstream_websocket(
            context,
            &account,
            token,
            ws_url.as_str(),
            previous_account_id.is_some_and(|account_id| account_id != account.id),
        )
        .await
        {
            Ok(stream) => {
                return Ok(ConnectedUpstreamWebsocket {
                    stream,
                    account_id: account.id.clone(),
                    account,
                    conversation_routing,
                    candidate_account_ids,
                    upstream_url: ws_url.clone(),
                    route_strategy: routed.route_strategy,
                    route_source: routed.route_source,
                });
            }
            Err(err) => {
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

async fn connect_upstream_websocket_with_timeout(
    context: &WsRequestContext,
    model: Option<&str>,
    previous_account_id: Option<&str>,
) -> Result<ConnectedUpstreamWebsocket, WsSessionError> {
    connect_upstream_websocket_with_timeout_excluding_accounts(
        context,
        model,
        previous_account_id,
        &HashSet::new(),
    )
    .await
}

async fn connect_upstream_websocket_with_timeout_excluding_accounts(
    context: &WsRequestContext,
    model: Option<&str>,
    previous_account_id: Option<&str>,
    excluded_account_ids: &HashSet<String>,
) -> Result<ConnectedUpstreamWebsocket, WsSessionError> {
    let connect_timeout =
        crate::gateway::current_upstream_connect_timeout().max(std::time::Duration::from_secs(1));
    match tokio::time::timeout(
        connect_timeout,
        connect_upstream_websocket_excluding_accounts(
            context,
            model,
            previous_account_id,
            excluded_account_ids,
        ),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(WsSessionError::new(
            504,
            RESPONSES_WS_ERROR_CODE,
            crate::gateway::bilingual_error(
                "连接上游 WebSocket 超时",
                format!(
                    "connect upstream websocket timed out after {} ms",
                    connect_timeout.as_millis()
                ),
            ),
        )),
    }
}

async fn reconnect_upstream_for_pending_request(
    context: &WsRequestContext,
    pending: &mut PendingWsRequestState,
    previous_account_id: Option<&str>,
    completed_responses: &CompletedWsResponseCache,
    completed_tool_calls: &CompletedWsToolCallCache,
) -> Result<ConnectedUpstreamWebsocket, WsSessionError> {
    let mut previous_account_id = previous_account_id.map(str::to_owned);
    let mut excluded_account_ids = HashSet::new();
    if let Some(account_id) = previous_account_id.as_deref() {
        excluded_account_ids.insert(account_id.to_string());
    }
    let mut last_send_error = None;

    for attempt in 1..=RESPONSES_WS_MAX_PENDING_FRAME_SEND_ATTEMPTS {
        let mut replacement = connect_upstream_websocket_with_timeout_excluding_accounts(
            context,
            pending.prepared.model.as_deref(),
            previous_account_id.as_deref(),
            &excluded_account_ids,
        )
        .await?;
        let account_changed = previous_account_id
            .as_deref()
            .is_some_and(|account_id| account_id != replacement.account_id);
        if let Err(err) = prepare_ws_request_for_new_connection(
            pending,
            completed_responses,
            completed_tool_calls,
            account_changed,
        ) {
            let _ = replacement.stream.close(None).await;
            return Err(err);
        }

        pending.attempted_account_ids.clear();
        pending
            .attempted_account_ids
            .insert(replacement.account_id.clone());
        pending.log.route_strategy = Some(replacement.route_strategy.to_string());
        pending.log.route_source = Some(replacement.route_source.to_string());
        pending.conversation_routing = replacement.conversation_routing.clone();
        pending.buffer_retry_preamble = should_buffer_ws_retry_preamble(
            &replacement,
            &pending.attempted_account_ids,
            pending.prepared.text.as_str(),
            pending.retried_missing_tool_call_context,
        );
        match replacement
            .stream
            .send(UpstreamMessage::Text(pending.prepared.text.clone().into()))
            .await
        {
            Ok(()) => {
                if attempt > 1 {
                    log::info!(
                        "event=responses_ws_reconnect_send_recovered attempt={} account_id={}",
                        attempt,
                        replacement.account_id,
                    );
                }
                return Ok(replacement);
            }
            Err(err) => {
                let account_id = replacement.account_id.clone();
                log::warn!(
                    "event=responses_ws_reconnect_send_failed attempt={} max_attempts={} account_id={} err={err}",
                    attempt,
                    RESPONSES_WS_MAX_PENDING_FRAME_SEND_ATTEMPTS,
                    account_id,
                );
                let _ = replacement.stream.close(None).await;
                last_send_error = Some(format!(
                    "send upstream websocket frame after reconnect failed for account {account_id}: {err}"
                ));
                excluded_account_ids.insert(account_id.clone());
                previous_account_id = Some(account_id);
            }
        }
    }

    Err(WsSessionError::bad_gateway_bilingual(
        "重连后发送上游 WebSocket 帧失败",
        last_send_error.unwrap_or_else(|| {
            "send upstream websocket frame after reconnect failed after bounded retries".to_string()
        }),
    ))
}

async fn retry_pending_request_after_upstream_disconnect(
    context: &WsRequestContext,
    upstream: &mut ConnectedUpstreamWebsocket,
    pending: &mut PendingWsRequestState,
    completed_responses: &CompletedWsResponseCache,
    completed_tool_calls: &CompletedWsToolCallCache,
    reason: &str,
) -> Result<bool, WsSessionError> {
    if pending.forwarded_non_preamble_event
        || pending.upstream_disconnect_recovery_attempts
            >= RESPONSES_WS_MAX_PRE_COMPLETION_RECOVERY_ATTEMPTS
    {
        return Ok(false);
    }

    let suppress_replayed_preamble = pending.forwarded_upstream_event;
    pending.upstream_disconnect_recovery_attempts += 1;
    pending.suppress_replayed_preamble = suppress_replayed_preamble;
    pending.buffered_upstream_preamble.clear();
    let previous_account_id = upstream.account_id.clone();
    log::info!(
        "event=responses_ws_pre_completion_recovery_attempt account_id={} attempt={} max_attempts={} reason={}",
        previous_account_id,
        pending.upstream_disconnect_recovery_attempts,
        RESPONSES_WS_MAX_PRE_COMPLETION_RECOVERY_ATTEMPTS,
        reason,
    );
    let replacement = reconnect_upstream_for_pending_request(
        context,
        pending,
        Some(previous_account_id.as_str()),
        completed_responses,
        completed_tool_calls,
    )
    .await?;
    log::info!(
        "event=responses_ws_upstream_reconnected previous_account_id={} account_id={} reason={}",
        previous_account_id,
        replacement.account_id,
        reason,
    );
    *upstream = replacement;
    Ok(true)
}

async fn wait_for_client_request_and_reconnect_upstream(
    socket: &mut WebSocket,
    context: &WsRequestContext,
    previous_account_id: &str,
    completed_responses: &CompletedWsResponseCache,
    completed_tool_calls: &CompletedWsToolCallCache,
) -> Result<Option<(ConnectedUpstreamWebsocket, PendingWsRequestState)>, WsSessionError> {
    let Some(text) = receive_initial_request(socket).await? else {
        return Ok(None);
    };
    validate_ws_api_key_for_new_request(context).await?;
    let prepared = match rewrite_client_frame_with_model_fast_policy(text.as_str(), context).await {
        Ok(prepared) => prepared,
        Err(err) => {
            record_rejected_ws_request(context, &err);
            return Err(err);
        }
    };
    let mut pending = PendingWsRequestState {
        log: begin_ws_request_log(context, &prepared, "unresolved", "upstream_reconnect"),
        prepared,
        conversation_routing: None,
        forwarded_upstream_event: false,
        forwarded_non_preamble_event: false,
        upstream_disconnect_recovery_attempts: 0,
        suppress_replayed_preamble: false,
        buffered_upstream_preamble: Vec::new(),
        buffer_retry_preamble: false,
        attempted_account_ids: HashSet::new(),
        retried_missing_tool_call_context: false,
    };
    match reconnect_upstream_for_pending_request(
        context,
        &mut pending,
        Some(previous_account_id),
        completed_responses,
        completed_tool_calls,
    )
    .await
    {
        Ok(replacement) => Ok(Some((replacement, pending))),
        Err(err) => {
            finalize_ws_request_log(
                context,
                &pending.log,
                None,
                None,
                err.status,
                crate::gateway::RequestLogUsage::default(),
                Some(err.message.clone()),
            );
            Err(err)
        }
    }
}

async fn connect_account_upstream_websocket(
    context: &WsRequestContext,
    account: &codexmanager_core::storage::Account,
    token: codexmanager_core::storage::Token,
    ws_url: &str,
    strip_session_affinity: bool,
) -> Result<UpstreamWebsocketStream, String> {
    let (authorization, token) =
        resolve_upstream_authorization_for_websocket(account.clone(), token).await?;
    let request = build_upstream_websocket_request(
        ws_url,
        account,
        &authorization,
        context,
        strip_session_affinity,
    )
    .map_err(|err| err.message)?;
    let proxy_url =
        crate::gateway::current_websocket_proxy_url_for_account(account.id.as_str(), ws_url)?;
    let first_error =
        match connect_upstream_websocket_request_detailed(request, ws_url, proxy_url.as_deref())
            .await
        {
            Ok((stream, _)) => return Ok(stream),
            Err(err) => err,
        };
    if first_error.is_websocket_connection_limit_reached() {
        log::info!(
            "event=responses_ws_connection_limit_handshake_retry account_id={}",
            account.id,
        );
        let retry_request = build_upstream_websocket_request(
            ws_url,
            account,
            &authorization,
            context,
            strip_session_affinity,
        )
        .map_err(|err| err.message)?;
        return connect_upstream_websocket_request_detailed(
            retry_request,
            ws_url,
            proxy_url.as_deref(),
        )
        .await
        .map(|(stream, _)| stream)
        .map_err(|err| format!("{err} (after websocket connection limit recovery)"));
    }
    if !first_error.is_unauthorized() {
        return Err(first_error.to_string());
    }

    let retry_authorization = if authorization.uses_agent_identity {
        if !first_error.is_agent_identity_task_invalid() {
            return Err(first_error.to_string());
        }
        let failed_task_id = authorization
            .task_id
            .as_deref()
            .ok_or_else(|| "agent identity websocket authorization omitted task_id".to_string())?;
        let recovered = recover_agent_identity_authorization_for_websocket(
            account.clone(),
            token.clone(),
            failed_task_id.to_string(),
        )
        .await?
        .ok_or_else(|| "agent identity disappeared during websocket task recovery".to_string())?;
        log::warn!(
            "event=responses_ws_agent_identity_task_recovery_retry account_id={} recovery=success",
            account.id
        );
        recovered
    } else {
        let Some(refreshed_bearer) = refresh_websocket_bearer(
            account.clone(),
            token,
            context.effective_upstream_base.clone(),
        )
        .await?
        else {
            return Err(first_error.to_string());
        };
        log::warn!(
            "event=responses_ws_unauthorized_refresh_retry account_id={}",
            account.id
        );
        WsUpstreamAuthorization {
            value: refreshed_bearer,
            task_id: None,
            uses_agent_identity: false,
            is_fedramp: false,
            account_scope_id: None,
        }
    };

    let retry_request = build_upstream_websocket_request(
        ws_url,
        account,
        &retry_authorization,
        context,
        strip_session_affinity,
    )
    .map_err(|err| err.message)?;
    connect_upstream_websocket_request_detailed(retry_request, ws_url, proxy_url.as_deref())
        .await
        .map(|(stream, _)| stream)
        .map_err(|err| format!("{err} (after websocket authorization recovery)"))
}

async fn resolve_upstream_authorization_for_websocket(
    account: codexmanager_core::storage::Account,
    token: codexmanager_core::storage::Token,
) -> Result<(WsUpstreamAuthorization, codexmanager_core::storage::Token), String> {
    let join_result = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let storage = open_storage()
            .ok_or_else(|| crate::gateway::bilingual_error("存储不可用", "storage unavailable"))?;
        let client = crate::gateway::upstream_client_for_account(account.id.as_str())?;
        match crate::agent_identity::resolve_or_bootstrap_account_agent_identity_authorization(
            &storage, &client, &account, &token,
        ) {
            Ok(Some(resolved)) => {
                return Ok((
                    WsUpstreamAuthorization {
                        value: resolved.value,
                        task_id: Some(resolved.task_id),
                        uses_agent_identity: true,
                        is_fedramp: resolved.is_fedramp,
                        account_scope_id: resolved.account_scope_id,
                    },
                    token,
                ));
            }
            Ok(None) => {}
            Err(err) => {
                if token.access_token.trim().is_empty() {
                    return Err(err);
                }
                log::warn!(
                    "event=responses_ws_agent_identity_resolution_failed account_id={} error={}",
                    account.id,
                    err
                );
            }
        }
        let mut token = token;
        let bearer =
            crate::gateway::gateway_resolve_openai_bearer_token(&storage, &account, &mut token)?;
        Ok((
            WsUpstreamAuthorization {
                value: bearer,
                task_id: None,
                uses_agent_identity: false,
                is_fedramp: false,
                account_scope_id: None,
            },
            token,
        ))
    })
    .await;

    match join_result {
        Ok(result) => result,
        Err(err) => Err(crate::gateway::bilingual_error(
            "上游鉴权任务合并失败",
            format!("upstream authorization task join failed: {err}"),
        )),
    }
}

async fn recover_agent_identity_authorization_for_websocket(
    account: codexmanager_core::storage::Account,
    token: codexmanager_core::storage::Token,
    failed_task_id: String,
) -> Result<Option<WsUpstreamAuthorization>, String> {
    tokio::task::spawn_blocking(move || {
        let storage = open_storage()
            .ok_or_else(|| crate::gateway::bilingual_error("存储不可用", "storage unavailable"))?;
        let client = crate::gateway::upstream_client_for_account(account.id.as_str())?;
        crate::agent_identity::recover_account_agent_identity_authorization(
            &storage,
            &client,
            &account,
            &token,
            &failed_task_id,
        )
        .map(|resolved| {
            resolved.map(|resolved| WsUpstreamAuthorization {
                value: resolved.value,
                task_id: Some(resolved.task_id),
                uses_agent_identity: true,
                is_fedramp: resolved.is_fedramp,
                account_scope_id: resolved.account_scope_id,
            })
        })
    })
    .await
    .map_err(|err| format!("agent identity recovery task join failed: {err}"))?
}

async fn refresh_websocket_bearer(
    account: codexmanager_core::storage::Account,
    token: codexmanager_core::storage::Token,
    effective_upstream_base: String,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let storage = open_storage()
            .ok_or_else(|| crate::gateway::bilingual_error("存储不可用", "storage unavailable"))?;
        let mut token = token;
        match try_refresh_websocket_bearer(
            &storage,
            effective_upstream_base.as_str(),
            &account,
            &mut token,
        ) {
            Ok(bearer) => Ok(bearer),
            Err(err) => {
                let _ = crate::account_status::mark_account_unavailable_for_refresh_token_error(
                    &storage,
                    &account.id,
                    &err,
                );
                Err(err)
            }
        }
    })
    .await
    .map_err(|err| format!("websocket bearer refresh task join failed: {err}"))?
}

fn try_refresh_websocket_bearer(
    storage: &codexmanager_core::storage::Storage,
    effective_upstream_base: &str,
    account: &codexmanager_core::storage::Account,
    token: &mut codexmanager_core::storage::Token,
) -> Result<Option<String>, String> {
    if crate::gateway::gateway_is_openai_api_base(effective_upstream_base) {
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
    let compressed_request = request.clone();
    let first_result = connect_upstream_websocket_request_with_config(
        compressed_request,
        ws_url,
        proxy_url,
        responses_ws_transport_config(true),
    )
    .await;
    match first_result {
        Ok(result) => Ok(result),
        Err(err) if err.is_compression_negotiation_rejection() => {
            log::info!(
                "event=responses_ws_compression_rejected retry=without_permessage_deflate ws_url={}",
                ws_url
            );
            connect_upstream_websocket_request_with_config(
                request,
                ws_url,
                proxy_url,
                responses_ws_transport_config(false),
            )
            .await
        }
        Err(err) => Err(err),
    }
}

async fn connect_upstream_websocket_request_with_config(
    request: WsClientRequest,
    ws_url: &str,
    proxy_url: Option<&str>,
    config: WebSocketConfig,
) -> Result<
    (
        tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>,
        WsClientResponse,
    ),
    WsConnectError,
> {
    let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return connect_async_tls_with_config(request, Some(config), false, None)
            .await
            .map_err(WsConnectError::from_tungstenite);
    };

    let stream = connect_websocket_proxy_tcp(ws_url, proxy_url)
        .await
        .map_err(WsConnectError::from_message)?;
    client_async_tls_with_config(request, stream, Some(config), None)
        .await
        .map_err(WsConnectError::from_tungstenite)
}

fn responses_ws_transport_config(enable_permessage_deflate: bool) -> WebSocketConfig {
    let mut config = WebSocketConfig::default()
        .max_message_size(Some(RESPONSES_WS_MAX_MESSAGE_BYTES))
        .max_frame_size(Some(RESPONSES_WS_MAX_MESSAGE_BYTES));
    if enable_permessage_deflate {
        let mut extensions = ExtensionsConfig::default();
        extensions.permessage_deflate = Some(DeflateConfig::default());
        config.extensions = extensions;
    }
    config
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
    authorization: &WsUpstreamAuthorization,
    context: &WsRequestContext,
    strip_session_affinity: bool,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, WsSessionError> {
    let mut request = ws_url.into_client_request().map_err(|err| {
        WsSessionError::bad_gateway_bilingual(
            "构建上游 WebSocket 请求失败",
            format!("build upstream websocket request failed: {err}"),
        )
    })?;
    let headers = request.headers_mut();
    insert_header(
        headers,
        "Authorization",
        &crate::agent_identity::format_upstream_authorization(&authorization.value),
    )?;
    if authorization.is_fedramp {
        insert_header(headers, "x-openai-fedramp", "true")?;
    }
    if let Some(account_id) = authorization
        .account_scope_id
        .as_deref()
        .or(account.workspace_id.as_deref())
        .or(account.chatgpt_account_id.as_deref())
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
    if !strip_session_affinity {
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
    if !strip_session_affinity {
        if let Some(turn_state) = context.incoming_headers.turn_state() {
            insert_header(
                headers,
                crate::http::codex_source::X_CODEX_TURN_STATE_HEADER,
                turn_state,
            )?;
        }
    }
    if !strip_session_affinity {
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
        estimated_input_tokens: crate::gateway::estimate_input_tokens_from_body(
            prepared.text.as_bytes(),
        ),
    }
}

fn record_rejected_ws_request(context: &WsRequestContext, err: &WsSessionError) {
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
        None,
        None,
        None,
        true,
        "ws",
        effective_protocol_type,
    );
    let started_at = Instant::now();
    if let Some(storage) = open_storage() {
        crate::gateway::write_request_log(
            &storage,
            crate::gateway::RequestLogTraceContext {
                trace_id: Some(trace_id.as_str()),
                original_path: Some(RESPONSES_ENDPOINT),
                adapted_path: Some(RESPONSES_ENDPOINT),
                request_type: Some("ws"),
                route_strategy: Some("unresolved"),
                route_source: Some("local_validation"),
                ..Default::default()
            },
            Some(context.api_key.id.as_str()),
            None,
            RESPONSES_ENDPOINT,
            "GET",
            None,
            None,
            None,
            Some(err.status),
            crate::gateway::RequestLogUsage::default(),
            Some(err.message.as_str()),
            Some(started_at.elapsed().as_millis()),
        );
    }
    crate::gateway::log_request_final(
        trace_id.as_str(),
        err.status,
        None,
        None,
        Some(err.message.as_str()),
        started_at.elapsed().as_millis(),
    );
}

fn should_buffer_ws_upstream_preamble(text: &str, buffered_count: usize) -> bool {
    if buffered_count >= MAX_BUFFERED_WS_PREAMBLE_EVENTS {
        return false;
    }
    let event_type = serde_json::from_str::<Value>(text).ok().and_then(|value| {
        value
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
    });
    matches!(
        event_type.as_deref(),
        Some("response.created" | "response.queued" | "response.in_progress")
    )
}

fn has_unattempted_ws_failover_candidate(
    upstream: &ConnectedUpstreamWebsocket,
    attempted_account_ids: &HashSet<String>,
) -> bool {
    upstream
        .candidate_account_ids
        .iter()
        .any(|account_id| !attempted_account_ids.contains(account_id))
}

fn ws_request_has_tool_call_output(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    let Some(input) = value.get("input") else {
        return false;
    };
    let has_tool_output = |item: &Value| {
        item.get("type")
            .and_then(Value::as_str)
            .is_some_and(|item_type| {
                item_type.eq_ignore_ascii_case("custom_tool_call_output")
                    || item_type.eq_ignore_ascii_case("function_call_output")
            })
    };
    match input {
        Value::Array(items) => items.iter().any(has_tool_output),
        item => has_tool_output(item),
    }
}

fn should_buffer_ws_retry_preamble(
    upstream: &ConnectedUpstreamWebsocket,
    attempted_account_ids: &HashSet<String>,
    request_text: &str,
    retried_missing_tool_call_context: bool,
) -> bool {
    has_unattempted_ws_failover_candidate(upstream, attempted_account_ids)
        || (!retried_missing_tool_call_context && ws_request_has_tool_call_output(request_text))
}

async fn flush_ws_upstream_preamble(
    socket: &mut WebSocket,
    pending: &mut PendingWsRequestState,
) -> Result<(), String> {
    let buffered = std::mem::take(&mut pending.buffered_upstream_preamble);
    for text in buffered {
        socket
            .send(Message::Text(text.into()))
            .await
            .map_err(|err| err.to_string())?;
        mark_ws_first_response(pending);
    }
    Ok(())
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
    pending.buffer_retry_preamble = false;
}

fn mark_ws_forwarded_event(pending: &mut PendingWsRequestState, text: &str) {
    mark_ws_first_response(pending);
    if !should_buffer_ws_upstream_preamble(text, 0) {
        pending.forwarded_non_preamble_event = true;
        pending.suppress_replayed_preamble = false;
    }
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
    if usage.estimated_input_tokens.is_none() {
        usage.estimated_input_tokens = Some(pending.estimated_input_tokens);
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

fn ws_context_rebase_error(raw_message: impl Into<String>) -> WsSessionError {
    let raw_message = raw_message.into();
    WsSessionError::context_rebase_failed(crate::gateway::bilingual_error(
        "无法为新的上游 WebSocket 重建完整上下文",
        raw_message,
    ))
}

fn prepare_ws_request_for_new_connection(
    pending: &mut PendingWsRequestState,
    completed_responses: &CompletedWsResponseCache,
    completed_tool_calls: &CompletedWsToolCallCache,
    account_changed: bool,
) -> Result<(), WsSessionError> {
    if pending.prepared.previous_response_id.is_some()
        && (!pending.prepared.store || account_changed)
    {
        let expanded = expand_response_create_previous_response(
            pending.prepared.text.as_str(),
            completed_responses,
        )
        .map_err(ws_context_rebase_error)?
        .ok_or_else(|| {
            ws_context_rebase_error(
                "response.create declared previous_response_id but no recoverable id was found",
            )
        })?;
        pending.prepared.text = expanded;
        pending.prepared.previous_response_id = None;
        pending.prepared.input = ws_request_input_from_text(pending.prepared.text.as_str())?;
    }
    if account_changed {
        pending.prepared.text = rebase_ws_request_for_account_change(
            pending.prepared.text.as_str(),
            completed_tool_calls,
        )?;
        pending.prepared.input = ws_request_input_from_text(pending.prepared.text.as_str())?;
    }
    Ok(())
}

fn ws_request_input_from_text(text: &str) -> Result<Value, WsSessionError> {
    serde_json::from_str::<Value>(text)
        .map_err(|err| {
            ws_context_rebase_error(format!("parse recovered response.create failed: {err}"))
        })?
        .get("input")
        .cloned()
        .ok_or_else(|| ws_context_rebase_error("recovered response.create is missing input"))
}

fn cache_completed_ws_response(
    completed_responses: &mut CompletedWsResponseCache,
    prepared: &PreparedClientFrame,
    terminal_text: &str,
) {
    if let Err(err) = completed_responses.observe_completed_response(
        terminal_text,
        prepared.previous_response_id.as_deref(),
        &prepared.input,
    ) {
        log::warn!(
            "event=responses_ws_response_history_not_cached previous_response_id={} err={}",
            prepared.previous_response_id.as_deref().unwrap_or("-"),
            err,
        );
    }
}

fn rebase_ws_request_for_account_change(
    text: &str,
    completed_tool_calls: &CompletedWsToolCallCache,
) -> Result<String, WsSessionError> {
    rebase_response_create_for_account_change(text, completed_tool_calls)
        .map_err(ws_context_rebase_error)
}

fn prepare_missing_ws_tool_call_retry(
    text: &str,
    completed_tool_calls: &CompletedWsToolCallCache,
    terminal: &WsTerminalEvent,
    already_retried: &mut bool,
) -> Result<Option<String>, WsSessionError> {
    let Some((kind, call_id)) = missing_ws_tool_call_from_terminal(terminal) else {
        return Ok(None);
    };
    if *already_retried {
        return Ok(None);
    }
    let Some(rebased) = rebase_response_create_for_missing_tool_call(
        text,
        completed_tool_calls,
        kind,
        call_id.as_str(),
    )
    .map_err(ws_context_rebase_error)?
    else {
        return Ok(None);
    };
    *already_retried = true;
    Ok(Some(rebased))
}

struct WsTerminalEvent {
    status_code: u16,
    usage: crate::gateway::RequestLogUsage,
    error: Option<String>,
    is_usage_limit: bool,
    is_websocket_connection_limit: bool,
}

fn should_rotate_ws_upstream(status_code: u16) -> bool {
    matches!(status_code, 401 | 403 | 404 | 408 | 409 | 429)
}

fn apply_ws_terminal_account_follow_up(account_id: &str, terminal: &WsTerminalEvent) {
    if !should_rotate_ws_upstream(terminal.status_code) {
        return;
    }
    crate::gateway::gateway_mark_account_cooldown_for_status(account_id, terminal.status_code);
    if terminal.status_code == 429 {
        let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
    }
    if !terminal.is_usage_limit {
        return;
    }
    let Some(storage) = open_storage() else {
        return;
    };
    let usage_limit_error = terminal
        .error
        .as_deref()
        .filter(|message| crate::account_status::usage_limit_reason_from_message(message).is_some())
        .unwrap_or("The usage limit has been reached");
    let _ = crate::account_status::mark_account_unavailable_for_gateway_error(
        &storage,
        account_id,
        usage_limit_error,
    );
}

async fn try_retry_ws_request_after_terminal(
    context: &WsRequestContext,
    upstream: &mut ConnectedUpstreamWebsocket,
    pending: &mut PendingWsRequestState,
    terminal: &WsTerminalEvent,
    completed_responses: &CompletedWsResponseCache,
    completed_tool_calls: &CompletedWsToolCallCache,
) -> Result<bool, WsSessionError> {
    if terminal.status_code == 200 || pending.forwarded_upstream_event {
        return Ok(false);
    }
    let mut retry_text = None;
    if missing_ws_tool_call_from_terminal(terminal).is_some() {
        let Some(rebased) = prepare_missing_ws_tool_call_retry(
            pending.prepared.text.as_str(),
            completed_tool_calls,
            terminal,
            &mut pending.retried_missing_tool_call_context,
        )?
        else {
            return Ok(false);
        };
        retry_text = Some(rebased);
    } else if is_previous_response_not_found_terminal(terminal) {
        if strip_previous_response_id_from_ws_text(pending.prepared.text.as_str()).is_none() {
            return Ok(false);
        }
        let expanded = expand_response_create_previous_response(
            pending.prepared.text.as_str(),
            completed_responses,
        )
        .map_err(ws_context_rebase_error)?
        .ok_or_else(|| {
            ws_context_rebase_error(
                "previous_response_id was rejected and no recoverable response id was found",
            )
        })?;
        pending.prepared.text = expanded;
        pending.prepared.previous_response_id = None;
        pending.prepared.input = ws_request_input_from_text(pending.prepared.text.as_str())?;
        retry_text = Some(pending.prepared.text.clone());
    } else {
        let previous_account_id = upstream.account_id.clone();
        if !try_rotate_ws_upstream_after_terminal(
            context,
            upstream,
            pending.prepared.model.as_deref(),
            terminal,
            &mut pending.attempted_account_ids,
        )
        .await
        {
            return Ok(false);
        }
        if upstream.account_id != previous_account_id {
            prepare_ws_request_for_new_connection(
                pending,
                completed_responses,
                completed_tool_calls,
                true,
            )?;
            retry_text = Some(pending.prepared.text.clone());
            pending.log.route_strategy = Some(upstream.route_strategy.to_string());
            pending.log.route_source = Some(upstream.route_source.to_string());
            pending.conversation_routing = upstream.conversation_routing.clone();
        }
    }
    let retry_text = retry_text.unwrap_or_else(|| pending.prepared.text.clone());
    let retry_input = ws_request_input_from_text(retry_text.as_str())?;
    match upstream
        .stream
        .send(UpstreamMessage::Text(retry_text.clone().into()))
        .await
    {
        Ok(()) => {
            pending.prepared.text = retry_text;
            pending.prepared.input = retry_input;
            pending.forwarded_upstream_event = false;
            pending.buffered_upstream_preamble.clear();
            pending.buffer_retry_preamble = should_buffer_ws_retry_preamble(
                upstream,
                &pending.attempted_account_ids,
                pending.prepared.text.as_str(),
                pending.retried_missing_tool_call_context,
            );
            pending.log.first_response_ms = None;
            Ok(true)
        }
        Err(err) => {
            log::warn!(
                "event=responses_ws_retry_send_failed account_id={} status={} err={}",
                upstream.account_id,
                terminal.status_code,
                err
            );
            Ok(false)
        }
    }
}

async fn try_rotate_ws_upstream_after_terminal(
    context: &WsRequestContext,
    upstream: &mut ConnectedUpstreamWebsocket,
    model: Option<&str>,
    terminal: &WsTerminalEvent,
    attempted_account_ids: &mut HashSet<String>,
) -> bool {
    let status_code = terminal.status_code;
    if !should_rotate_ws_upstream(status_code) {
        return false;
    }

    let current_account_id = upstream.account_id.clone();
    attempted_account_ids.insert(current_account_id.clone());

    let storage = match open_storage() {
        Some(storage) => storage,
        None => return false,
    };
    let routed = match crate::gateway::gateway_collect_routed_candidates_for_ws(
        &storage,
        &context.api_key.id,
        model,
        context.route_conversation_id.as_deref(),
        context.route_conversation_source,
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
    let route_strategy = routed.route_strategy;
    let route_source = routed.route_source;
    let candidates = routed.candidates;
    let conversation_routing = routed.conversation_routing.clone();
    let candidate_account_ids = candidates
        .iter()
        .map(|(account, _)| account.id.clone())
        .collect::<HashSet<_>>();
    drop(storage);

    for (account, token) in candidates {
        if !attempted_account_ids.insert(account.id.clone()) {
            continue;
        }
        match connect_account_upstream_websocket(
            context,
            &account,
            token,
            upstream.upstream_url.as_str(),
            true,
        )
        .await
        {
            Ok(stream) => {
                let replacement = ConnectedUpstreamWebsocket {
                    stream,
                    account_id: account.id.clone(),
                    account,
                    conversation_routing,
                    candidate_account_ids,
                    upstream_url: upstream.upstream_url.clone(),
                    route_strategy,
                    route_source,
                };
                crate::gateway::gateway_record_failover_attempt();
                let _ = upstream.stream.close(None).await;
                *upstream = replacement;
                return true;
            }
            Err(err) => {
                log::warn!(
                    "event=responses_ws_failover_connect_failed from_account_id={} candidate_account_id={} status={} err={}",
                    current_account_id,
                    account.id,
                    status_code,
                    err
                );
            }
        }
    }

    false
}

fn inspect_ws_terminal_event(text: &str) -> Option<WsTerminalEvent> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    let event_type = value
        .get("type")
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    let error = extract_ws_error_message(&value);
    let error_code = extract_ws_error_code(&value);
    let usage_limit_signal = matches!(
        event_type.as_str(),
        "response.failed" | "response.incomplete" | "error"
    )
    .then(|| {
        error
            .as_deref()
            .filter(|message| {
                crate::account_status::usage_limit_reason_from_message(message).is_some()
            })
            .map(str::to_string)
            .or_else(|| {
                error_code
                    .as_deref()
                    .filter(|code| {
                        crate::account_status::usage_limit_reason_from_message(code).is_some()
                    })
                    .map(str::to_string)
            })
    })
    .flatten();
    if let Some(usage_limit_signal) = usage_limit_signal {
        return Some(WsTerminalEvent {
            status_code: 429,
            usage: parse_ws_usage(&value),
            error: error.or(Some(usage_limit_signal)),
            is_usage_limit: true,
            is_websocket_connection_limit: false,
        });
    }
    let is_websocket_connection_limit =
        is_websocket_connection_limit_error(error_code.as_deref(), error.as_deref());
    match event_type.as_str() {
        "response.completed" => Some(WsTerminalEvent {
            status_code: 200,
            usage: parse_ws_usage(&value),
            error: None,
            is_usage_limit: false,
            is_websocket_connection_limit: false,
        }),
        "response.failed" | "error" => Some(WsTerminalEvent {
            status_code: infer_ws_terminal_status(&value, error.as_deref()),
            usage: parse_ws_usage(&value),
            error: error.or(error_code),
            is_usage_limit: false,
            is_websocket_connection_limit,
        }),
        "response.incomplete" => Some(WsTerminalEvent {
            status_code: 502,
            usage: parse_ws_usage(&value),
            error: error.or_else(|| Some("连接中断（可能是网络波动或客户端主动取消）".to_string())),
            is_usage_limit: false,
            is_websocket_connection_limit: false,
        }),
        _ => None,
    }
}

fn is_websocket_connection_limit_error(
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> bool {
    error_code
        .is_some_and(|code| code.eq_ignore_ascii_case(WEBSOCKET_CONNECTION_LIMIT_REACHED_CODE))
        || error_message.is_some_and(|message| {
            message
                .to_ascii_lowercase()
                .contains("responses websocket connection limit reached")
        })
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

fn parse_missing_ws_tool_call_error_message(message: &str) -> Option<(WsToolCallKind, String)> {
    let message = message.trim();
    let lower = message.to_ascii_lowercase();
    let prefixes = [
        (
            "no tool call found for custom tool call output with call_id ",
            WsToolCallKind::Custom,
        ),
        (
            "no tool call found for function tool call output with call_id ",
            WsToolCallKind::Function,
        ),
        (
            "no tool call found for function call output with call_id ",
            WsToolCallKind::Function,
        ),
    ];
    let (prefix, kind) = prefixes
        .into_iter()
        .find(|(prefix, _)| lower.starts_with(prefix))?;
    let call_id = message
        .get(prefix.len()..)?
        .trim()
        .trim_end_matches('.')
        .trim()
        .trim_matches(|character| matches!(character, '\'' | '"' | '`'));
    if call_id.is_empty() || call_id.chars().any(char::is_whitespace) {
        return None;
    }
    Some((kind, call_id.to_string()))
}

fn missing_ws_tool_call_from_terminal(
    terminal: &WsTerminalEvent,
) -> Option<(WsToolCallKind, String)> {
    if terminal.status_code != 400 {
        return None;
    }
    parse_missing_ws_tool_call_error_message(terminal.error.as_deref()?)
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
        if parse_missing_ws_tool_call_error_message(message).is_some() {
            return 400;
        }
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
        cache_write_tokens: usage
            .and_then(|map| map.get("input_tokens_details"))
            .and_then(|details| details.get("cache_write_tokens"))
            .and_then(Value::as_i64)
            .or_else(|| {
                usage
                    .and_then(|map| map.get("prompt_tokens_details"))
                    .and_then(|details| details.get("cache_write_tokens"))
                    .and_then(Value::as_i64)
            })
            .or_else(|| {
                usage
                    .and_then(|map| map.get("cache_write_input_tokens"))
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
        estimated_input_tokens: None,
    }
}

fn extract_ws_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| error.get("message").or(Some(error)))
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
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("message").or(Some(error)))
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
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_string)
        })
}

fn extract_ws_error_code(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(Value::as_str)
        .or_else(|| value.get("code").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("error"))
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("status_details"))
                .and_then(|details| details.get("error"))
                .and_then(|error| error.get("code"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_string)
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

fn ws_error_payload(err: WsSessionError, prefer_raw_errors: bool) -> Message {
    let message = crate::gateway::error_message_for_client(prefer_raw_errors, err.message);
    Message::Text(
        json!({
            "type": "error",
            "status": err.status,
            "error": {
                "code": err.code,
                "message": message,
            }
        })
        .to_string()
        .into(),
    )
}

async fn send_ws_error(socket: &mut WebSocket, err: WsSessionError, prefer_raw_errors: bool) {
    let _ = socket.send(ws_error_payload(err, prefer_raw_errors)).await;
}

async fn send_ws_error_and_close(
    socket: &mut WebSocket,
    err: WsSessionError,
    prefer_raw_errors: bool,
) {
    send_ws_error(socket, err, prefer_raw_errors).await;
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
