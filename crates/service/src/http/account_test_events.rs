use std::convert::Infallible;
use std::io::{self, Read};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::RawQuery;
use axum::http::{
    HeaderMap as AxumHeaderMap, HeaderValue as AxumHeaderValue, StatusCode as AxumStatusCode,
};
use axum::response::{IntoResponse, Response as AxumResponse};
use crossbeam_channel::RecvTimeoutError;
use futures_util::stream;
use tiny_http::{Header, Request, Response, StatusCode};

const EVENT_NAME: &str = "account-test-event";
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

fn request_header_value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().trim())
        .filter(|value| !value.is_empty())
}

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

fn response_header(name: &'static str, value: &'static str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid static header")
}

fn account_test_event_data(event: &crate::account_test::AccountTestEvent) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string())
}

fn account_test_sse_frame(event: &crate::account_test::AccountTestEvent) -> Vec<u8> {
    format!(
        "event: {EVENT_NAME}\ndata: {}\n\n",
        account_test_event_data(event)
    )
    .into_bytes()
}

fn account_test_id_from_query(query: Option<&str>) -> Option<String> {
    let mut values = query
        .into_iter()
        .flat_map(|query| url::form_urlencoded::parse(query.as_bytes()))
        .filter(|(key, _)| key == "testId")
        .map(|(_, value)| value.into_owned());
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    crate::account_test::normalize_account_test_id(&value)
}

fn next_account_test_event_chunk(
    receiver: crate::account_test::AccountTestEventSubscription,
) -> Option<(crate::account_test::AccountTestEventSubscription, Vec<u8>)> {
    let chunk = match receiver.recv_timeout(KEEPALIVE_INTERVAL) {
        Ok(event) => account_test_sse_frame(&event),
        Err(RecvTimeoutError::Timeout) => b": keep-alive\n\n".to_vec(),
        Err(RecvTimeoutError::Disconnected) => return None,
    };
    Some((receiver, chunk))
}

struct AccountTestEventStream {
    receiver: crate::account_test::AccountTestEventSubscription,
    pending: Vec<u8>,
    pending_offset: usize,
    opened: bool,
}

impl AccountTestEventStream {
    fn new(receiver: crate::account_test::AccountTestEventSubscription) -> Self {
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
            Ok(event) => account_test_sse_frame(&event),
            Err(RecvTimeoutError::Timeout) => b": keep-alive\n\n".to_vec(),
            Err(RecvTimeoutError::Disconnected) => return Ok(false),
        };
        self.pending_offset = 0;
        Ok(true)
    }
}

impl Read for AccountTestEventStream {
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

pub(crate) fn handle_account_test_events(request: Request) {
    if request.method().as_str() != "GET" {
        let _ = request.respond(Response::from_string("{}").with_status_code(405));
        return;
    }
    if !rpc_token_valid(&request) {
        let _ = request.respond(Response::from_string("{}").with_status_code(401));
        return;
    }

    let test_id = account_test_id_from_query(request.url().split_once('?').map(|(_, query)| query));
    let Some(test_id) = test_id else {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    };
    let receiver = crate::account_test::subscribe_account_test_events(&test_id);
    let headers = vec![
        response_header("Content-Type", "text/event-stream"),
        response_header("Cache-Control", "no-cache"),
        response_header("Connection", "keep-alive"),
        response_header("X-Accel-Buffering", "no"),
    ];
    let response = Response::new(
        StatusCode(200),
        headers,
        AccountTestEventStream::new(receiver),
        None,
        None,
    );
    let _ = request.respond(response);
}

pub(crate) async fn handle_account_test_events_http(
    headers: AxumHeaderMap,
    RawQuery(query): RawQuery,
) -> AxumResponse {
    if !axum_rpc_token_valid(&headers) {
        return (AxumStatusCode::UNAUTHORIZED, "{}").into_response();
    }

    let Some(test_id) = account_test_id_from_query(query.as_deref()) else {
        return (AxumStatusCode::BAD_REQUEST, "{}").into_response();
    };
    let receiver = crate::account_test::subscribe_account_test_events(&test_id);
    let event_stream = stream::unfold((receiver, false), |(receiver, opened)| async move {
        if !opened {
            return Some((
                Ok::<Bytes, Infallible>(Bytes::from_static(b": connected\n\n")),
                (receiver, true),
            ));
        }

        let next = tokio::task::spawn_blocking(move || next_account_test_event_chunk(receiver))
            .await
            .ok()
            .flatten()?;
        Some((Ok(Bytes::from(next.1)), (next.0, true)))
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
mod tests {
    use super::account_test_id_from_query;

    #[test]
    fn account_test_event_query_requires_one_valid_test_id() {
        assert_eq!(
            account_test_id_from_query(Some(
                "other=value&testId=550e8400-e29b-41d4-a716-446655440000"
            ))
            .as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert!(account_test_id_from_query(None).is_none());
        assert!(account_test_id_from_query(Some("other=value")).is_none());
        assert!(account_test_id_from_query(Some("testId=%20%20")).is_none());
        assert!(account_test_id_from_query(Some("testId=bad%2Fid")).is_none());
        assert!(account_test_id_from_query(Some("testId=one&testId=two")).is_none());
    }
}
