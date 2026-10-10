use super::super::ResponseAdapter;
use super::{UpstreamResponseBridgeResult, UpstreamResponseUsage};
use crate::http::gateway_response::Header;

pub(super) const REQUEST_ID_HEADER_CANDIDATES: &[&str] = &["x-request-id", "x-oai-request-id"];
pub(super) const CF_RAY_HEADER_NAME: &str = "cf-ray";
pub(super) const AUTH_ERROR_HEADER_NAME: &str = "x-openai-authorization-error";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct UpstreamResponseMetadata {
    pub(super) request_id: Option<String>,
    pub(super) cf_ray: Option<String>,
    pub(super) auth_error: Option<String>,
    pub(super) identity_error_code: Option<String>,
    pub(super) content_type: Option<String>,
    pub(super) is_sse: bool,
    pub(super) is_json: bool,
}

pub(super) fn upstream_response_metadata(
    headers: &reqwest::header::HeaderMap,
) -> UpstreamResponseMetadata {
    let content_type = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let normalized_content_type = content_type.as_deref().map(str::to_ascii_lowercase);

    UpstreamResponseMetadata {
        request_id: first_upstream_header(headers, REQUEST_ID_HEADER_CANDIDATES),
        cf_ray: first_upstream_header(headers, &[CF_RAY_HEADER_NAME]),
        auth_error: first_upstream_header(headers, &[AUTH_ERROR_HEADER_NAME]),
        identity_error_code: crate::gateway::extract_identity_error_code_from_headers(headers),
        content_type,
        is_sse: normalized_content_type
            .as_deref()
            .is_some_and(|value| value.starts_with("text/event-stream")),
        is_json: normalized_content_type
            .as_deref()
            .is_some_and(|value| value.contains("application/json")),
    }
}

pub(super) fn first_upstream_header(
    headers: &reqwest::header::HeaderMap,
    names: &[&str],
) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

pub(super) fn copy_upstream_response_headers(
    upstream_headers: &reqwest::header::HeaderMap,
    trace_id: Option<&str>,
) -> Vec<Header> {
    let mut headers = Vec::new();
    for (name, value) in upstream_headers.iter() {
        let name_str = name.as_str();
        if name_str.eq_ignore_ascii_case("transfer-encoding")
            || name_str.eq_ignore_ascii_case("content-length")
            || name_str.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
            headers.push(header);
        }
    }
    push_trace_id_header(&mut headers, trace_id);
    headers
}

fn push_trace_id_header(headers: &mut Vec<Header>, trace_id: Option<&str>) {
    let Some(trace_id) = trace_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if let Ok(header) = Header::from_bytes(
        crate::error_codes::TRACE_ID_HEADER_NAME.as_bytes(),
        trace_id.as_bytes(),
    ) {
        headers.push(header);
    }
}

pub(super) fn with_bridge_debug_meta(
    mut result: UpstreamResponseBridgeResult,
    upstream_request_id: &Option<String>,
    upstream_cf_ray: &Option<String>,
    upstream_auth_error: &Option<String>,
    upstream_identity_error_code: &Option<String>,
    upstream_content_type: &Option<String>,
    last_sse_event_type: Option<String>,
) -> UpstreamResponseBridgeResult {
    result.upstream_request_id = upstream_request_id.clone();
    result.upstream_cf_ray = upstream_cf_ray.clone();
    result.upstream_auth_error = upstream_auth_error.clone();
    result.upstream_identity_error_code = upstream_identity_error_code.clone();
    result.upstream_content_type = upstream_content_type.clone();
    result.last_sse_event_type = last_sse_event_type;
    result
}

pub(super) fn terminal_bridge_result_with_debug_meta(
    usage: UpstreamResponseUsage,
    delivery_error: Option<String>,
    upstream_error_hint: Option<String>,
    upstream_request_id: &Option<String>,
    upstream_cf_ray: &Option<String>,
    upstream_auth_error: &Option<String>,
    upstream_identity_error_code: &Option<String>,
    upstream_content_type: &Option<String>,
) -> UpstreamResponseBridgeResult {
    with_bridge_debug_meta(
        UpstreamResponseBridgeResult {
            usage,
            stream_terminal_seen: true,
            stream_terminal_delivered: false,
            delivery_error,
            upstream_error_hint,
            ..UpstreamResponseBridgeResult::default()
        },
        upstream_request_id,
        upstream_cf_ray,
        upstream_auth_error,
        upstream_identity_error_code,
        upstream_content_type,
        None,
    )
}

pub(super) fn log_bridge_stream_diagnostics(
    response_adapter: ResponseAdapter,
    request_path: &str,
    result: &UpstreamResponseBridgeResult,
) {
    if result.delivery_error.is_none()
        && result.stream_terminal_seen
        && result.stream_terminal_error.is_none()
    {
        return;
    }

    log::warn!(
        "event=gateway_bridge_stream_diagnostics adapter={:?} path={} stream_terminal_seen={} stream_terminal_delivered={} stream_terminal_error={} delivery_error={} upstream_error_hint={} last_sse_event_type={} upstream_request_id={} upstream_cf_ray={} upstream_content_type={}",
        response_adapter,
        request_path,
        if result.stream_terminal_seen { "true" } else { "false" },
        if result.stream_terminal_delivered { "true" } else { "false" },
        result.stream_terminal_error.as_deref().unwrap_or("-"),
        result.delivery_error.as_deref().unwrap_or("-"),
        result.upstream_error_hint.as_deref().unwrap_or("-"),
        result.last_sse_event_type.as_deref().unwrap_or("-"),
        result.upstream_request_id.as_deref().unwrap_or("-"),
        result.upstream_cf_ray.as_deref().unwrap_or("-"),
        result.upstream_content_type.as_deref().unwrap_or("-"),
    );
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
