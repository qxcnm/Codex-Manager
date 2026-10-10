use crate::http::gateway_request::GatewayRequest as Request;
use crate::http::gateway_response::{Header, Response, StatusCode};
use serde_json::Value;

use super::extract_error_message_from_json;

pub(super) fn extract_error_message_from_json_bytes(body: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    extract_error_message_from_json(&value)
}

pub(super) fn replace_content_type_header(headers: &mut Vec<Header>, content_type: &str) {
    headers.retain(|header| !header.field.as_str().eq_ignore_ascii_case("Content-Type"));
    if let Ok(header) = Header::from_bytes(b"Content-Type".as_slice(), content_type.as_bytes()) {
        headers.push(header);
    }
}

pub(super) async fn respond_json_bytes(
    request: Request,
    status: StatusCode,
    mut headers: Vec<Header>,
    body: Vec<u8>,
) -> Option<String> {
    replace_content_type_header(&mut headers, "application/json");
    let len = Some(body.len());
    let response = Response::new(status, headers, std::io::Cursor::new(body), len);
    request
        .respond_async(response)
        .await
        .err()
        .map(|err| err.to_string())
}

pub(super) fn force_openai_responses_stream_content_type(
    headers: &mut Vec<Header>,
    request_path: &str,
    is_stream: bool,
) {
    if is_stream && request_path.starts_with("/v1/responses") {
        replace_content_type_header(headers, "text/event-stream");
    }
}
