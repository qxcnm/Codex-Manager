//! Built-in transport for the independently authorized Basispoints product.
//! This module presents normalized Responses SSE to the existing delivery stack.
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use super::super::attempt_flow::transport::UpstreamRequestContext;
use super::super::{
    GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse, GatewayUpstreamResponse,
};
use crate::account::bps::BpsCredentials;
use crate::gateway::bps_protocol::{self, Bridge};

pub(in crate::gateway) const RESPONSES_URL: &str =
    "https://bps.openai.com/basispoints/api/responses";
const WS_URL: &str = "wss://bps.openai.com/basispoints/api/responses";
static ACTIVE_STREAMS: Semaphore = Semaphore::const_new(128);

/// Account-card diagnostics use the same adapter, credentials, proxy and transport
/// as gateway traffic. Dropping this stream cancels its upstream connection.
pub(crate) async fn account_test_request(
    local_account_id: &str,
    credentials: BpsCredentials,
    body: Bytes,
    deadline: Instant,
) -> Result<impl futures_util::Stream<Item = Result<Bytes, String>> + Send + 'static, String> {
    let guard = crate::http::gateway_replay_guard::UpstreamReplayGuard::default();
    let context = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: "openai_compat",
        is_fedramp: false,
        original_body: None,
        capture: None,
        replay_guard: Some(&guard),
    };
    let response = execute(
        &reqwest::Method::POST,
        "/v1/responses",
        context,
        &crate::gateway::IncomingHeaderSnapshot::default(),
        &body,
        local_account_id,
        credentials,
        Some(deadline),
    )
    .await;
    let status = response.status();
    let response = match response {
        GatewayUpstreamResponse::Stream(response) => response,
        #[cfg(test)]
        GatewayUpstreamResponse::Blocking(_) => unreachable!("BPS always returns an async stream"),
    };
    if !status.is_success() {
        let body = response.read_all_bytes_async().await?;
        let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        return Err(value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("BPS account test request failed")
            .to_owned());
    }
    Ok(futures_util::stream::unfold(
        response.into_body(),
        |mut body| async move {
            match body.recv_async().await {
                Some(GatewayByteStreamItem::Chunk(bytes)) => Some((Ok(bytes), body)),
                Some(GatewayByteStreamItem::Error(error)) => Some((Err(error), body)),
                Some(GatewayByteStreamItem::Eof) | None => None,
            }
        },
    ))
}

pub(super) fn error_response(status: u16, message: &str) -> GatewayUpstreamResponse {
    let body = json!({"error":{"type":"bps_error","code":"bps_request_failed","message":message}});
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "application/json".parse().unwrap(),
    );
    isolated_response(
        reqwest::StatusCode::from_u16(status).unwrap_or(reqwest::StatusCode::BAD_GATEWAY),
        headers,
        GatewayByteStream::from_bytes(Bytes::from(body.to_string())),
    )
}

fn isolated_response(
    status: reqwest::StatusCode,
    headers: reqwest::header::HeaderMap,
    body: GatewayByteStream,
) -> GatewayUpstreamResponse {
    GatewayUpstreamResponse::Stream(
        GatewayStreamResponse::new(status, headers, body).with_isolated_account_state(),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    method: &reqwest::Method,
    path: &str,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &crate::gateway::IncomingHeaderSnapshot,
    body: &Bytes,
    local_account_id: &str,
    credentials: BpsCredentials,
    deadline: Option<Instant>,
) -> GatewayUpstreamResponse {
    if method != reqwest::Method::POST || path.split('?').next() != Some("/v1/responses") {
        return error_response(
            400,
            "BPS forwarding supports Responses generation only; this operation is not supported",
        );
    }
    let scope = format!(
        "{}:{}",
        local_account_id,
        incoming_headers.session_id().unwrap_or_default()
    );
    let (prepared, bridge) = match bps_protocol::prepare(body.as_ref(), &scope) {
        Ok(prepared) => prepared,
        Err(error) => return error_response(400, &error),
    };
    let proxy =
        match crate::gateway::current_websocket_proxy_url_for_account(local_account_id, WS_URL) {
            Ok(proxy) => proxy,
            Err(_) => return error_response(502, "BPS account proxy configuration is invalid"),
        };
    match send_prepared(
        WS_URL,
        proxy.as_deref(),
        &credentials,
        &prepared,
        bridge,
        deadline,
        request_ctx,
    )
    .await
    {
        Ok(response) => response,
        Err((status, error)) => error_response(status, &error),
    }
}

fn random_uuid() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn profile() -> Vec<(&'static str, &'static str)> {
    let platform = if cfg!(target_os = "macos") {
        "Mac"
    } else {
        "PC"
    };
    vec![
        (
            "X-Openai-Internal-Basispoints-Client-Product",
            "basispoints-excel-plugin",
        ),
        ("X-Openai-Internal-Basispoints-Client-Platform", "excel"),
        (
            "X-Openai-Internal-Basispoints-Client-Agent-Profile",
            "excel",
        ),
        ("X-Openai-Internal-Basispoints-Client-Editor", "excel"),
        ("X-Openai-Internal-Basispoints-Client-Host", "office"),
        ("X-Openai-Internal-Basispoints-Client-Runtime", "desktop"),
        (
            "X-Openai-Internal-Basispoints-Client-Platform-Class",
            platform,
        ),
        ("X-Openai-Internal-Basispoints-Office-Host", "Excel"),
        ("X-Openai-Internal-Basispoints-Office-Platform", platform),
        ("X-Openai-Internal-Basispoints-Browser-Name", "unknown"),
    ]
}

fn websocket_request(
    base_url: &str,
    credentials: &BpsCredentials,
    request_id: &str,
) -> Result<
    (
        String,
        tokio_tungstenite::tungstenite::handshake::client::Request,
    ),
    String,
> {
    let profile = profile();
    let info: serde_json::Map<String, Value> = profile
        .iter()
        .map(|(key, value)| (key.to_string(), json!(value)))
        .collect();
    let mut url = url::Url::parse(base_url).map_err(|_| "invalid BPS endpoint")?;
    url.query_pairs_mut()
        .append_pair("bps_client_info", &Value::Object(info).to_string())
        .append_pair("bps_auth_mode", "chatgpt")
        .append_pair("bps_ws_affinity", request_id)
        .append_pair("bps_control_frames", "upstream_sent,heartbeat");
    let url = url.to_string();
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|_| "invalid BPS WebSocket request")?;
    let headers = request.headers_mut();
    for (name, value) in profile {
        headers.insert(
            tokio_tungstenite::tungstenite::http::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "invalid BPS profile header")?,
            value.parse().map_err(|_| "invalid BPS profile value")?,
        );
    }
    for (name, value) in [
        (
            "authorization",
            format!("Bearer {}", credentials.access_token),
        ),
        ("chatgpt-account-id", credentials.account_id.clone()),
        ("x-openai-account-id", credentials.account_id.clone()),
        ("x-basispoints-auth-mode", "chatgpt".to_owned()),
        ("origin", "https://bps.openai.com".to_owned()),
        (
            "user-agent",
            concat!("CodexManager/", env!("CARGO_PKG_VERSION")).to_owned(),
        ),
        (
            "sec-websocket-protocol",
            format!("responses, openai-bearer.{}", credentials.access_token),
        ),
    ] {
        headers.insert(
            tokio_tungstenite::tungstenite::http::HeaderName::from_static(name),
            value
                .parse()
                .map_err(|_| "BPS credentials contain invalid header characters")?,
        );
    }
    Ok((url, request))
}

#[allow(clippy::too_many_arguments)]
async fn send_prepared(
    endpoint: &str,
    proxy: Option<&str>,
    credentials: &BpsCredentials,
    prepared: &[u8],
    bridge: Bridge,
    deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
) -> Result<GatewayUpstreamResponse, (u16, String)> {
    let permit = ACTIVE_STREAMS
        .try_acquire()
        .map_err(|_| (503, "BPS stream capacity reached".to_owned()))?;
    let request_id = random_uuid();
    let (url, request) =
        websocket_request(endpoint, credentials, &request_id).map_err(|error| (400, error))?;
    let mut frame: Value = serde_json::from_slice(prepared)
        .map_err(|_| (400, "invalid prepared BPS request".to_owned()))?;
    let object = frame
        .as_object_mut()
        .ok_or_else(|| (400, "invalid prepared BPS request".to_owned()))?;
    object.insert("type".to_owned(), json!("response.create"));
    object.insert("basispoints_request_id".to_owned(), json!(request_id));
    object.insert(
        "basispoints_resume_token".to_owned(),
        json!(format!("resume_{}", random_uuid())),
    );
    let frame = frame.to_string();
    let connect_timeout =
        crate::gateway::current_upstream_connect_timeout().max(Duration::from_secs(1));
    let timeout = deadline
        .map(|d| {
            d.saturating_duration_since(Instant::now())
                .min(connect_timeout)
        })
        .unwrap_or(connect_timeout);
    if timeout.is_zero() {
        return Err((504, "BPS request deadline exceeded".to_owned()));
    }
    let connect = crate::http::responses_websocket::connect_upstream_websocket_request_with_limit(
        request,
        &url,
        proxy,
        16 * 1024 * 1024,
    );
    let (mut socket, _) = match tokio::time::timeout(timeout, connect).await {
        Ok(Ok(connected)) => connected,
        Ok(Err(error)) => {
            let status = error
                .status_code()
                .filter(|status| (400..600).contains(status))
                .unwrap_or(502);
            return Err((
                status,
                format!("BPS WebSocket handshake failed (HTTP {status})"),
            ));
        }
        Err(_) => return Err((504, "BPS WebSocket handshake timed out".to_owned())),
    };
    if let Some(capture) = request_ctx.capture {
        // Tokens live in the handshake only; never log the subprotocol or auth headers.
        let mut captured: Value = serde_json::from_str(&frame).unwrap_or(Value::Null);
        captured["basispoints_resume_token"] = json!("[REDACTED]");
        crate::gateway::capture_outbound_payload(
            capture,
            "WS",
            RESPONSES_URL,
            "websocket",
            &[],
            &Bytes::from(captured.to_string()),
            None,
        );
    }
    let send_timeout = deadline
        .map(|d| {
            d.saturating_duration_since(Instant::now())
                .min(connect_timeout)
        })
        .unwrap_or(connect_timeout);
    if send_timeout.is_zero() {
        return Err((
            504,
            "BPS request deadline exceeded before sending".to_owned(),
        ));
    }
    if !matches!(
        tokio::time::timeout(send_timeout, socket.send(Message::Text(frame.into()))).await,
        Ok(Ok(()))
    ) {
        if let Some(guard) = request_ctx.replay_guard {
            guard.block_after_ambiguous_delivery();
        }
        return Err((
            502,
            "BPS request send failed; automatic replay is disabled".to_owned(),
        ));
    }
    // A successful send is also a commitment: any later failure must be reported
    // on this request, never replayed through Codex or another account.
    if let Some(guard) = request_ctx.replay_guard {
        guard.block_after_ambiguous_delivery();
    }
    let (tx, rx) = mpsc::channel(32);
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    let token = credentials.access_token.clone();
    tokio::spawn(async move {
        let _permit = permit;
        let work = pump_events(&mut socket, bridge, &tx, deadline, &token);
        tokio::select! {
            _ = &mut cancel_rx => {},
            _ = work => {},
            _ = async {
                if let Some(deadline) = deadline {
                    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                let _ = tx.try_send(GatewayByteStreamItem::Error("BPS request deadline exceeded".to_owned()));
            },
        }
        // Dropping the socket closes it immediately, including on backpressure/cancellation.
    });
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "text/event-stream".parse().unwrap(),
    );
    Ok(isolated_response(
        reqwest::StatusCode::OK,
        headers,
        GatewayByteStream::from_receiver_with_cancel(rx, Some(cancel_tx)),
    ))
}

fn terminal_event(event: &Value) -> bool {
    matches!(
        event.get("type").and_then(Value::as_str),
        Some("response.completed" | "response.failed" | "response.incomplete" | "error")
    )
}

async fn send_event(tx: &mpsc::Sender<GatewayByteStreamItem>, event: &Value) -> bool {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("error");
    if !kind
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return false;
    }
    tx.send(GatewayByteStreamItem::Chunk(Bytes::from(format!(
        "event: {kind}\ndata: {event}\n\n"
    ))))
    .await
    .is_ok()
}

async fn failure_event(tx: &mpsc::Sender<GatewayByteStreamItem>, message: &str) {
    let _ = send_event(tx, &json!({"type":"response.failed","response":{"status":"failed","output":[],"error":{"code":"bps_transport_error","message":message}}})).await;
    let _ = tx.send(GatewayByteStreamItem::Eof).await;
}

async fn pump_events(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    mut bridge: Bridge,
    tx: &mpsc::Sender<GatewayByteStreamItem>,
    deadline: Option<Instant>,
    token: &str,
) {
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(25),
        Duration::from_secs(25),
    );
    let mut last_message = Instant::now();
    loop {
        let idle = crate::gateway::current_upstream_stream_timeout_ms();
        let idle_remaining = Duration::from_millis(idle).saturating_sub(last_message.elapsed());
        let read_timeout = match (
            deadline.map(|d| d.saturating_duration_since(Instant::now())),
            idle,
        ) {
            (Some(remaining), 0) => remaining,
            (Some(remaining), _) => remaining.min(idle_remaining),
            (None, 0) => Duration::from_secs(24 * 60 * 60),
            (None, _) => idle_remaining,
        };
        let next = tokio::select! {
            _ = heartbeat.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    failure_event(tx, "BPS connection lost; automatic replay is disabled").await;
                    return;
                }
                continue;
            },
            next = tokio::time::timeout(read_timeout, socket.next()) => next,
        };
        let message = match next {
            Ok(Some(Ok(message))) => message,
            Err(_) => {
                failure_event(tx, "BPS stream timed out").await;
                return;
            }
            _ => {
                failure_event(tx, "BPS connection closed before a terminal response").await;
                return;
            }
        };
        last_message = Instant::now();
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Ping(payload) => {
                if socket.send(Message::Pong(payload)).await.is_err() {
                    failure_event(tx, "BPS heartbeat failed").await;
                    return;
                }
                continue;
            }
            Message::Pong(_) => continue,
            Message::Close(_) => {
                failure_event(tx, "BPS connection closed before a terminal response").await;
                return;
            }
            _ => {
                failure_event(tx, "BPS returned an unsupported WebSocket frame").await;
                return;
            }
        };
        let event: Value = match serde_json::from_str(&text) {
            Ok(event) => event,
            Err(_) => {
                failure_event(tx, "BPS returned invalid event JSON").await;
                return;
            }
        };
        let events = match bridge.translate_event(event) {
            Ok(events) => events,
            Err(error) => {
                failure_event(tx, &error.replace(token, "[REDACTED]")).await;
                return;
            }
        };
        for event in events {
            let terminal = terminal_event(&event);
            let event = if terminal && !token.is_empty() {
                serde_json::from_str(&event.to_string().replace(token, "[REDACTED]")).unwrap_or_else(|_| json!({"type":"error","error":{"message":"invalid BPS terminal event"}}))
            } else {
                event
            };
            if !send_event(tx, &event).await {
                return;
            }
            if terminal {
                let _ = tx.send(GatewayByteStreamItem::Eof).await;
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "bps_tests.rs"]
mod tests;
