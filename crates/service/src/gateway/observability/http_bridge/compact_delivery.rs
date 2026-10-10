use crate::http::gateway_request::GatewayRequest as Request;
use crate::http::gateway_response::{Header, Response, StatusCode};

use super::compact_errors::{
    build_invalid_compact_success_message, build_passthrough_non_success_message,
    compact_success_body_is_valid,
};
use super::metadata::with_bridge_debug_meta;
use super::{UpstreamResponseBridgeResult, UpstreamResponseUsage};

async fn respond_synthesized_compact_error_body(
    request: Request,
    status_code: u16,
    usage: UpstreamResponseUsage,
    message: String,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    let response_message = crate::gateway::error_message_for_client(
        crate::gateway::prefers_raw_errors_for_gateway_request(&request),
        message.as_str(),
    );
    let response = crate::gateway::error_response::terminal_text_response(
        status_code,
        response_message,
        trace_id,
    );
    let delivery_error = request
        .respond_async(response)
        .await
        .err()
        .map(|err| err.to_string());
    UpstreamResponseBridgeResult {
        usage,
        stream_terminal_seen: true,
        stream_terminal_delivered: false,
        stream_terminal_error: None,
        delivery_error,
        upstream_error_hint: Some(message),
        delivered_status_code: Some(status_code),
        upstream_request_id: request_id.map(str::to_string),
        upstream_cf_ray: cf_ray.map(str::to_string),
        upstream_auth_error: None,
        upstream_identity_error_code: None,
        upstream_content_type: Some("application/json".to_string()),
        last_sse_event_type: None,
    }
}

pub(super) async fn respond_invalid_compact_success_body(
    request: Request,
    usage: UpstreamResponseUsage,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    with_bridge_debug_meta(
        respond_synthesized_compact_error_body(
            request,
            502,
            usage,
            build_invalid_compact_success_message(
                body,
                request_id,
                cf_ray,
                auth_error,
                identity_error_code,
            ),
            request_id,
            cf_ray,
            trace_id,
        )
        .await,
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        &Some("application/json".to_string()),
        None,
    )
}

pub(super) async fn respond_compact_success_body(
    request: Request,
    status: StatusCode,
    headers: Vec<Header>,
    usage: UpstreamResponseUsage,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    content_type: &Option<String>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    if !compact_success_body_is_valid(body) {
        return respond_invalid_compact_success_body(
            request,
            usage,
            body,
            request_id,
            cf_ray,
            auth_error,
            identity_error_code,
            trace_id,
        )
        .await;
    }

    let len = Some(body.len());
    let response = Response::new(status, headers, std::io::Cursor::new(body.to_vec()), len);
    let delivery_error = request
        .respond_async(response)
        .await
        .err()
        .map(|err| err.to_string());
    with_bridge_debug_meta(
        UpstreamResponseBridgeResult {
            usage,
            stream_terminal_seen: true,
            stream_terminal_delivered: false,
            stream_terminal_error: None,
            delivery_error,
            upstream_error_hint: None,
            delivered_status_code: None,
            upstream_request_id: None,
            upstream_cf_ray: None,
            upstream_auth_error: None,
            upstream_identity_error_code: None,
            upstream_content_type: None,
            last_sse_event_type: None,
        },
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        content_type,
        None,
    )
}

pub(super) async fn respond_invalid_compact_non_success_body(
    request: Request,
    status_code: u16,
    usage: UpstreamResponseUsage,
    body: &[u8],
    content_type: Option<&str>,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    let gateway_status_code = 502;
    let message = build_passthrough_non_success_message(
        status_code,
        content_type,
        body,
        request_id,
        cf_ray,
        auth_error,
        identity_error_code,
    );
    with_bridge_debug_meta(
        respond_synthesized_compact_error_body(
            request,
            gateway_status_code,
            usage,
            message,
            request_id,
            cf_ray,
            trace_id,
        )
        .await,
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        &Some("application/json".to_string()),
        None,
    )
}

pub(super) async fn respond_normalized_passthrough_non_success_body(
    request: Request,
    usage: UpstreamResponseUsage,
    body: &[u8],
    content_type: Option<&str>,
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
    trace_id: Option<&str>,
) -> UpstreamResponseBridgeResult {
    let message = build_passthrough_non_success_message(
        502,
        content_type,
        body,
        request_id,
        cf_ray,
        auth_error,
        identity_error_code,
    );
    with_bridge_debug_meta(
        respond_synthesized_compact_error_body(
            request, 502, usage, message, request_id, cf_ray, trace_id,
        )
        .await,
        &request_id.map(str::to_string),
        &cf_ray.map(str::to_string),
        &auth_error.map(str::to_string),
        &identity_error_code.map(str::to_string),
        &Some("application/json".to_string()),
        None,
    )
}
