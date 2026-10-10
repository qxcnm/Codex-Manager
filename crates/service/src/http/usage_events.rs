use std::convert::Infallible;
#[cfg(test)]
use std::io::{self, Read};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{
    HeaderMap as AxumHeaderMap, HeaderValue as AxumHeaderValue, StatusCode as AxumStatusCode,
};
use axum::response::{IntoResponse, Response as AxumResponse};
#[cfg(test)]
use crossbeam_channel::{Receiver, RecvTimeoutError};
use futures_util::stream;
#[cfg(test)]
use tiny_http::{Header, Request, Response, StatusCode};

const EVENT_NAME: &str = "usage-refresh-completed";
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

#[cfg(test)]
fn request_header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
fn rpc_token_valid(request: &Request) -> bool {
    request_header_value(request, "X-CodexManager-Rpc-Token")
        .is_some_and(crate::rpc_auth_token_matches)
}

fn axum_rpc_token_valid(headers: &AxumHeaderMap) -> bool {
    headers
        .get("X-CodexManager-Rpc-Token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(crate::rpc_auth_token_matches)
}

#[cfg(test)]
fn response_header(name: &'static str, value: &'static str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid static header")
}

fn usage_refresh_event_data(event: &crate::UsageRefreshCompletedEvent) -> String {
    serde_json::json!({
        "source": event.source,
        "processed": event.processed,
        "total": event.total,
        "completed_at": event.completed_at,
    })
    .to_string()
}

fn usage_refresh_sse_frame(event: &crate::UsageRefreshCompletedEvent) -> Vec<u8> {
    format!(
        "event: {EVENT_NAME}\ndata: {}\n\n",
        usage_refresh_event_data(event)
    )
    .into_bytes()
}

async fn next_usage_refresh_event_chunk(
    receiver: &mut tokio::sync::broadcast::Receiver<crate::UsageRefreshCompletedEvent>,
    keepalive_interval: Duration,
) -> Option<Vec<u8>> {
    loop {
        match tokio::time::timeout(keepalive_interval, receiver.recv()).await {
            Ok(Ok(event)) => return Some(usage_refresh_sse_frame(&event)),
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
            Err(_) => return Some(b": keep-alive\n\n".to_vec()),
        }
    }
}

#[cfg(test)]
struct UsageRefreshEventStream {
    receiver: Receiver<crate::UsageRefreshCompletedEvent>,
    pending: Vec<u8>,
    pending_offset: usize,
    opened: bool,
}

#[cfg(test)]
impl UsageRefreshEventStream {
    fn new(receiver: Receiver<crate::UsageRefreshCompletedEvent>) -> Self {
        Self {
            receiver,
            pending: Vec::new(),
            pending_offset: 0,
            opened: false,
        }
    }

    fn refill(&mut self) -> io::Result<bool> {
        if !self.opened {
            self.opened = true;
            self.pending = b": connected\n\n".to_vec();
            self.pending_offset = 0;
            return Ok(true);
        }

        self.pending = match self.receiver.recv_timeout(KEEPALIVE_INTERVAL) {
            Ok(event) => usage_refresh_sse_frame(&event),
            Err(RecvTimeoutError::Timeout) => b": keep-alive\n\n".to_vec(),
            Err(RecvTimeoutError::Disconnected) => return Ok(false),
        };
        self.pending_offset = 0;
        Ok(true)
    }
}

#[cfg(test)]
impl Read for UsageRefreshEventStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }

        if self.pending_offset >= self.pending.len() && !self.refill()? {
            return Ok(0);
        }

        let remaining = &self.pending[self.pending_offset..];
        let count = remaining.len().min(out.len());
        out[..count].copy_from_slice(&remaining[..count]);
        self.pending_offset += count;
        Ok(count)
    }
}

#[cfg(test)]
pub(crate) fn handle_usage_refresh_events(request: Request) {
    if request.method().as_str() != "GET" {
        let _ = request.respond(Response::from_string("{}").with_status_code(405));
        return;
    }
    if !rpc_token_valid(&request) {
        let _ = request.respond(Response::from_string("{}").with_status_code(401));
        return;
    }

    let receiver = crate::usage_refresh::subscribe_usage_refresh_completed();
    let headers = vec![
        response_header("Content-Type", "text/event-stream"),
        response_header("Cache-Control", "no-cache"),
        response_header("Connection", "keep-alive"),
        response_header("X-Accel-Buffering", "no"),
    ];
    let response = Response::new(
        StatusCode(200),
        headers,
        UsageRefreshEventStream::new(receiver),
        None,
        None,
    );
    let _ = request.respond(response);
}

pub(crate) async fn handle_usage_refresh_events_http(headers: AxumHeaderMap) -> AxumResponse {
    if !axum_rpc_token_valid(&headers) {
        return (AxumStatusCode::UNAUTHORIZED, "{}").into_response();
    }

    let receiver = crate::usage_refresh::subscribe_usage_refresh_completed_async();
    let event_stream = stream::unfold((receiver, false), |(mut receiver, opened)| async move {
        if !opened {
            return Some((
                Ok::<Bytes, Infallible>(Bytes::from_static(b": connected\n\n")),
                (receiver, true),
            ));
        }

        // Broadcast receive is cancellation-safe: dropping the response body
        // drops this receiver without leaving a blocked worker behind.
        let chunk = next_usage_refresh_event_chunk(&mut receiver, KEEPALIVE_INTERVAL).await?;
        Some((Ok(Bytes::from(chunk)), (receiver, true)))
    });

    let mut response = AxumResponse::new(Body::from_stream(event_stream));
    *response.status_mut() = AxumStatusCode::OK;
    response.headers_mut().insert(
        "content-type",
        AxumHeaderValue::from_static("text/event-stream"),
    );
    response
        .headers_mut()
        .insert("cache-control", AxumHeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert("x-accel-buffering", AxumHeaderValue::from_static("no"));
    response
}

#[cfg(test)]
#[path = "usage_events_tests.rs"]
mod tests;
