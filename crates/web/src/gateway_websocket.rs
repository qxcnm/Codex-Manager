use super::AppState;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::FromRequestParts;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::CloseFrame as UpstreamCloseFrame;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
// A Responses response.create frame can contain several inline image data URLs. Match the
// service-side bounded limit so the Web UI bridge cannot truncate a valid frame first.
const GATEWAY_WS_MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;

fn gateway_websocket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(GATEWAY_WS_MAX_MESSAGE_BYTES))
        .max_frame_size(Some(GATEWAY_WS_MAX_MESSAGE_BYTES))
}

pub(super) fn target_url(service_addr: &str, uri: &axum::http::Uri) -> String {
    let service_addr = service_addr.trim().trim_end_matches('/');
    let base = if let Some(value) = service_addr.strip_prefix("https://") {
        format!("wss://{value}")
    } else if let Some(value) = service_addr.strip_prefix("http://") {
        format!("ws://{value}")
    } else if service_addr.starts_with("ws://") || service_addr.starts_with("wss://") {
        service_addr.to_string()
    } else {
        format!("ws://{service_addr}")
    };
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    format!("{base}{path_and_query}")
}

pub(super) fn is_upgrade_request(headers: &HeaderMap) -> bool {
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

fn is_hop_by_hop_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("connection")
        || name.eq_ignore_ascii_case("keep-alive")
        || name.eq_ignore_ascii_case("proxy-authenticate")
        || name.eq_ignore_ascii_case("proxy-authorization")
        || name.eq_ignore_ascii_case("te")
        || name.eq_ignore_ascii_case("trailer")
        || name.eq_ignore_ascii_case("transfer-encoding")
        || name.eq_ignore_ascii_case("upgrade")
}

fn should_skip_request_header(name: &header::HeaderName, value: &HeaderValue) -> bool {
    let lower = name.as_str();
    is_hop_by_hop_header(lower)
        || lower.eq_ignore_ascii_case("host")
        || lower.eq_ignore_ascii_case("content-length")
        || lower.starts_with("sec-websocket-")
        || value.to_str().is_err()
}

fn should_skip_response_header(name: &header::HeaderName) -> bool {
    let lower = name.as_str();
    is_hop_by_hop_header(lower)
        || lower.eq_ignore_ascii_case("content-length")
        || lower.starts_with("sec-websocket-")
}

fn upstream_error_response(
    err: tokio_tungstenite::tungstenite::Error,
    target_url: &str,
) -> Response {
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            let (parts, body) = response.into_parts();
            let mut out = Response::new(Body::from(body.unwrap_or_default()));
            *out.status_mut() = parts.status;
            for (name, value) in parts.headers.iter() {
                if should_skip_response_header(name) {
                    continue;
                }
                out.headers_mut().append(name.clone(), value.clone());
            }
            out
        }
        other => {
            log::warn!(
                "event=web_gateway_websocket_connect_failed target_url={} err={}",
                target_url,
                other
            );
            (
                StatusCode::BAD_GATEWAY,
                format!("gateway websocket upstream error: {other}"),
            )
                .into_response()
        }
    }
}

fn client_message_to_upstream(message: Message) -> Option<UpstreamMessage> {
    match message {
        Message::Text(text) => Some(UpstreamMessage::Text(text.to_string().into())),
        Message::Binary(bytes) => Some(UpstreamMessage::Binary(bytes.to_vec().into())),
        Message::Ping(bytes) => Some(UpstreamMessage::Ping(bytes.to_vec().into())),
        Message::Pong(bytes) => Some(UpstreamMessage::Pong(bytes.to_vec().into())),
        Message::Close(frame) => Some(UpstreamMessage::Close(frame.map(|frame| {
            UpstreamCloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }
        }))),
    }
}

fn upstream_message_to_client(message: UpstreamMessage) -> Option<Message> {
    match message {
        UpstreamMessage::Text(text) => Some(Message::Text(text.to_string().into())),
        UpstreamMessage::Binary(bytes) => Some(Message::Binary(bytes.to_vec().into())),
        UpstreamMessage::Ping(bytes) => Some(Message::Ping(bytes.to_vec().into())),
        UpstreamMessage::Pong(bytes) => Some(Message::Pong(bytes.to_vec().into())),
        UpstreamMessage::Close(frame) => Some(Message::Close(frame.map(|frame| {
            axum::extract::ws::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }
        }))),
        UpstreamMessage::Frame(_) => None,
    }
}

async fn relay(
    mut client: WebSocket,
    mut upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) {
    loop {
        tokio::select! {
            client_message = client.recv() => {
                let Some(client_message) = client_message else {
                    let _ = upstream.close(None).await;
                    return;
                };
                let client_message = match client_message {
                    Ok(message) => message,
                    Err(err) => {
                        log::warn!("event=web_gateway_websocket_client_receive_failed err={err}");
                        let _ = upstream.close(None).await;
                        return;
                    }
                };
                let client_closed = matches!(client_message, Message::Close(_));
                if client_closed {
                    // Receiving Close queues its reply; flush it before dropping
                    // this leg or waiting on the other peer to avoid client 1006.
                    let _ = tokio::time::timeout(Duration::from_secs(3), client.flush()).await;
                }
                if let Some(message) = client_message_to_upstream(client_message) {
                    if let Err(err) = upstream.send(message).await {
                        log::warn!("event=web_gateway_websocket_upstream_send_failed err={err}");
                        let _ = client.close().await;
                        return;
                    }
                }
                if client_closed {
                    return;
                }
            }
            upstream_message = upstream.next() => {
                let Some(upstream_message) = upstream_message else {
                    let _ = client.close().await;
                    return;
                };
                let upstream_message = match upstream_message {
                    Ok(message) => message,
                    Err(err) => {
                        log::warn!("event=web_gateway_websocket_upstream_receive_failed err={err}");
                        let _ = client.close().await;
                        return;
                    }
                };
                let upstream_closed = matches!(upstream_message, UpstreamMessage::Close(_));
                if upstream_closed {
                    let _ = tokio::time::timeout(Duration::from_secs(3), upstream.flush()).await;
                }
                if let Some(message) = upstream_message_to_client(upstream_message) {
                    if let Err(err) = client.send(message).await {
                        log::warn!("event=web_gateway_websocket_client_send_failed err={err}");
                        let _ = upstream.close(None).await;
                        return;
                    }
                }
                if upstream_closed {
                    return;
                }
            }
        }
    }
}

pub(super) async fn proxy(state: Arc<AppState>, mut parts: axum::http::request::Parts) -> Response {
    let target_url = target_url(state.service_addr.as_str(), &parts.uri);
    let incoming_headers = parts.headers.clone();
    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(upgrade) => upgrade
            .max_message_size(GATEWAY_WS_MAX_MESSAGE_BYTES)
            .max_frame_size(GATEWAY_WS_MAX_MESSAGE_BYTES),
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("gateway websocket upgrade rejected: {err}"),
            )
                .into_response();
        }
    };

    let mut upstream_request = match target_url.as_str().into_client_request() {
        Ok(request) => request,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("gateway websocket request build failed: {err}"),
            )
                .into_response();
        }
    };
    for (name, value) in incoming_headers.iter() {
        if should_skip_request_header(name, value) {
            continue;
        }
        upstream_request
            .headers_mut()
            .append(name.clone(), value.clone());
    }

    let connect_result = tokio::time::timeout(
        CONNECT_TIMEOUT,
        connect_async_with_config(upstream_request, Some(gateway_websocket_config()), false),
    )
    .await;
    let (upstream, upstream_response) = match connect_result {
        Ok(Ok(result)) => result,
        Ok(Err(err)) => return upstream_error_response(err, target_url.as_str()),
        Err(_) => {
            log::warn!(
                "event=web_gateway_websocket_connect_timeout target_url={} timeout_ms={}",
                target_url,
                CONNECT_TIMEOUT.as_millis()
            );
            return (
                StatusCode::GATEWAY_TIMEOUT,
                "gateway websocket upstream connect timed out",
            )
                .into_response();
        }
    };

    let mut response = upgrade
        .on_upgrade(move |client| relay(client, upstream))
        .into_response();
    for (name, value) in upstream_response.headers().iter() {
        if should_skip_response_header(name) {
            continue;
        }
        response.headers_mut().append(name.clone(), value.clone());
    }
    response
}

#[cfg(test)]
#[path = "gateway_websocket_tests.rs"]
mod tests;
