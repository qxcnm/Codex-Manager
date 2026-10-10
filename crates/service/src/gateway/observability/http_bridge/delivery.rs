use crate::http::gateway_request::GatewayRequest as Request;
use crate::http::gateway_response::{Header, Response, StatusCode};
use serde_json::Value;
use std::sync::{Arc, Mutex};

use crate::gateway::upstream::GatewayStreamResponse;

use super::super::{GeminiStreamOutputMode, ResponseAdapter, ToolNameRestoreMap};
use super::body_conversion::{
    compatibility_stream_content_type, convert_chat_completions_body_to_compact,
    convert_error_body_for_adapter, convert_success_body_for_adapter,
    gemini_cli_wrap_response_envelope, merge_usage_from_body_without_output_text,
};
use super::compact_delivery::{
    respond_compact_success_body, respond_invalid_compact_non_success_body,
    respond_invalid_compact_success_body, respond_normalized_passthrough_non_success_body,
};
use super::compact_errors::{
    compact_non_success_body_should_be_normalized, compact_success_body_is_valid,
    extract_error_hint_from_body_or_headers, non_success_body_should_be_normalized,
    with_upstream_debug_suffix,
};
use super::manual_chunked::respond_streaming_chunked;
#[cfg(test)]
use super::manual_chunked::write_streaming_chunked_response;
use super::metadata::{
    copy_upstream_response_headers, log_bridge_stream_diagnostics,
    terminal_bridge_result_with_debug_meta, upstream_response_metadata, with_bridge_debug_meta,
};
use super::response_helpers::{
    extract_error_message_from_json_bytes, force_openai_responses_stream_content_type,
    replace_content_type_header, respond_json_bytes,
};
use super::{
    collect_non_stream_json_from_sse_bytes, extract_error_hint_from_body, looks_like_sse_payload,
    parse_usage_from_json, usage_has_signal, AnthropicSseReader,
    ChatCompletionsFromResponsesSseReader, GeminiSseReader, ImagesFromResponsesSseReader,
    ImagesResponseFormat, OpenAIResponsesPassthroughSseReader, PassthroughSseCollector,
    PassthroughSseProtocol, PassthroughSseUsageReader, ResponsesFromAnthropicSseReader,
    SseKeepAliveFrame, UpstreamResponseBridgeResult, UpstreamResponseUsage,
};

/// 函数 `is_compact_request_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn is_compact_request_path(path: &str) -> bool {
    path == "/v1/responses/compact" || path.starts_with("/v1/responses/compact?")
}

#[cfg(test)]
fn response_adapter_uses_manual_chunked_streaming(response_adapter: ResponseAdapter) -> bool {
    matches!(
        response_adapter,
        ResponseAdapter::ResponsesFromAnthropicMessages
    )
}

/// 函数 `should_suppress_deactivation_delivery`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - upstream_error_hint: 参数 upstream_error_hint
/// - allow_failover_for_deactivation: 参数 allow_failover_for_deactivation
///
/// # 返回
/// 返回函数执行结果
fn should_suppress_deactivation_delivery(
    upstream_error_hint: Option<&str>,
    allow_failover_for_deactivation: bool,
) -> bool {
    allow_failover_for_deactivation
        && upstream_error_hint.is_some_and(|message| {
            crate::account_status::deactivation_reason_from_message(message).is_some()
        })
}

struct UpstreamDebugMetaRefs<'a> {
    request_id: &'a Option<String>,
    cf_ray: &'a Option<String>,
    auth_error: &'a Option<String>,
    identity_error_code: &'a Option<String>,
    content_type: &'a Option<String>,
}

async fn respond_usage_collector_stream(
    request: Request,
    status: StatusCode,
    headers: Vec<Header>,
    response_body: Box<dyn crate::http::gateway_response_body::GatewayResponseBody>,
    usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
    meta: UpstreamDebugMetaRefs<'_>,
) -> UpstreamResponseBridgeResult {
    let delivery_error = respond_streaming_chunked(request, status, headers, response_body)
        .await
        .err()
        .map(|err| err.to_string());
    let usage = usage_collector
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    terminal_bridge_result_with_debug_meta(
        usage,
        delivery_error,
        None,
        meta.request_id,
        meta.cf_ray,
        meta.auth_error,
        meta.identity_error_code,
        meta.content_type,
    )
}

async fn respond_passthrough_collector_stream(
    request: Request,
    status: StatusCode,
    headers: Vec<Header>,
    response_body: Box<dyn crate::http::gateway_response_body::GatewayResponseBody>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    meta: UpstreamDebugMetaRefs<'_>,
) -> UpstreamResponseBridgeResult {
    let delivery_error = respond_streaming_chunked(request, status, headers, response_body)
        .await
        .err()
        .map(|err| err.to_string());
    let collector = usage_collector
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    with_bridge_debug_meta(
        UpstreamResponseBridgeResult {
            usage: collector.usage,
            stream_terminal_seen: collector.saw_terminal,
            stream_terminal_delivered: collector.terminal_delivered,
            stream_terminal_error: collector.terminal_error,
            delivery_error,
            upstream_error_hint: collector.upstream_error_hint,
            delivered_status_code: None,
            upstream_request_id: None,
            upstream_cf_ray: None,
            upstream_auth_error: None,
            upstream_identity_error_code: None,
            upstream_content_type: None,
            last_sse_event_type: collector.last_event_type,
        },
        meta.request_id,
        meta.cf_ray,
        meta.auth_error,
        meta.identity_error_code,
        meta.content_type,
        None,
    )
}

/// 函数 `respond_with_upstream`
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
#[cfg(test)]
pub(crate) fn respond_with_upstream(
    request: Request,
    upstream: reqwest::blocking::Response,
    _inflight_guard: super::super::AccountInFlightGuard,
    response_adapter: ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
    is_stream: bool,
    allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    crate::gateway::response_test_runtime()
        .map_err(|error| error.to_string())?
        .block_on(respond_with_stream_upstream(
            request,
            GatewayStreamResponse::from_blocking_response(upstream),
            _inflight_guard,
            response_adapter,
            passthrough_sse_protocol,
            gemini_stream_output_mode,
            request_path,
            tool_name_restore_map,
            is_stream,
            allow_failover_for_deactivation,
            trace_id,
            fallback_model,
            request_started_at,
        ))
}

pub(crate) async fn respond_with_stream_upstream(
    request: Request,
    upstream: GatewayStreamResponse,
    _inflight_guard: super::super::AccountInFlightGuard,
    response_adapter: ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&ToolNameRestoreMap>,
    is_stream: bool,
    allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    let keepalive_frame = SseKeepAliveFrame::Comment;
    let passthrough_sse_protocol =
        passthrough_sse_protocol.unwrap_or(PassthroughSseProtocol::Generic);
    let upstream_meta = upstream_response_metadata(upstream.headers());
    let upstream_request_id = upstream_meta.request_id;
    let upstream_cf_ray = upstream_meta.cf_ray;
    let upstream_auth_error = upstream_meta.auth_error;
    let upstream_identity_error_code = upstream_meta.identity_error_code;
    let upstream_content_type = upstream_meta.content_type;
    let is_sse = upstream_meta.is_sse;
    let is_json = upstream_meta.is_json;
    if response_adapter != ResponseAdapter::Passthrough {
        let status = StatusCode(upstream.status().as_u16());
        let mut headers = copy_upstream_response_headers(upstream.headers(), trace_id);

        if !is_stream {
            let upstream_body = upstream
                .read_all_bytes_cancellable(request.cancellation_receiver())
                .await
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let detected_sse =
                is_sse || (!is_json && looks_like_sse_payload(upstream_body.as_ref()));
            let (body, usage) = if detected_sse {
                let (synthesized, mut usage) =
                    collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let body = synthesized.unwrap_or_else(|| upstream_body.to_vec());
                merge_usage_from_body_without_output_text(&mut usage, &body);
                (body, usage)
            } else {
                let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                    .ok()
                    .map(|value| parse_usage_from_json(&value))
                    .unwrap_or_default();
                (upstream_body.to_vec(), usage)
            };
            let response_body = if status.0 >= 400 {
                let message = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        &body,
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                    .or_else(|| extract_error_message_from_json_bytes(&body)),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                )
                .unwrap_or_else(|| "upstream compatibility bridge failed".to_string());
                convert_error_body_for_adapter(response_adapter, &message)
            } else {
                convert_success_body_for_adapter(
                    response_adapter,
                    &body,
                    request_path,
                    tool_name_restore_map,
                )
                .unwrap_or_else(|| body.clone())
            };
            let delivery_error = respond_json_bytes(request, status, headers, response_body).await;
            return Ok(terminal_bridge_result_with_debug_meta(
                usage,
                delivery_error,
                None,
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
            ));
        }

        if status.0 >= 400 && !is_sse {
            let upstream_body = upstream
                .read_all_bytes_cancellable(request.cancellation_receiver())
                .await
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let message = with_upstream_debug_suffix(
                extract_error_hint_from_body_or_headers(
                    status.0,
                    upstream_content_type.as_deref(),
                    upstream_body.as_ref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                )
                .or_else(|| extract_error_message_from_json_bytes(upstream_body.as_ref())),
                None,
                upstream_request_id.as_deref(),
                upstream_cf_ray.as_deref(),
                upstream_auth_error.as_deref(),
                upstream_identity_error_code.as_deref(),
            )
            .unwrap_or_else(|| "upstream compatibility bridge failed".to_string());
            let response_body = convert_error_body_for_adapter(response_adapter, &message);
            let delivery_error = respond_json_bytes(request, status, headers, response_body).await;
            return Ok(terminal_bridge_result_with_debug_meta(
                UpstreamResponseUsage::default(),
                delivery_error,
                Some(message),
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
            ));
        }

        replace_content_type_header(
            &mut headers,
            compatibility_stream_content_type(response_adapter, gemini_stream_output_mode),
        );
        match response_adapter {
            ResponseAdapter::AnthropicMessagesFromResponses => {
                let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
                let response_body: Box<
                    dyn crate::http::gateway_response_body::GatewayResponseBody,
                > = Box::new(AnthropicSseReader::from_stream_response(
                    upstream,
                    Arc::clone(&usage_collector),
                    fallback_model,
                    tool_name_restore_map.cloned(),
                    request_started_at,
                ));
                return Ok(respond_usage_collector_stream(
                    request,
                    status,
                    headers,
                    response_body,
                    usage_collector,
                    UpstreamDebugMetaRefs {
                        request_id: &upstream_request_id,
                        cf_ray: &upstream_cf_ray,
                        auth_error: &upstream_auth_error,
                        identity_error_code: &upstream_identity_error_code,
                        content_type: &upstream_content_type,
                    },
                )
                .await);
            }
            ResponseAdapter::ResponsesFromAnthropicMessages => {
                let usage_collector = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
                let response_body: Box<
                    dyn crate::http::gateway_response_body::GatewayResponseBody,
                > = Box::new(ResponsesFromAnthropicSseReader::from_stream_response(
                    upstream,
                    Arc::clone(&usage_collector),
                    fallback_model,
                    request_started_at,
                ));
                return Ok(respond_usage_collector_stream(
                    request,
                    status,
                    headers,
                    response_body,
                    usage_collector,
                    UpstreamDebugMetaRefs {
                        request_id: &upstream_request_id,
                        cf_ray: &upstream_cf_ray,
                        auth_error: &upstream_auth_error,
                        identity_error_code: &upstream_identity_error_code,
                        content_type: &upstream_content_type,
                    },
                )
                .await);
            }
            ResponseAdapter::ChatCompletionsFromResponses => {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<
                    dyn crate::http::gateway_response_body::GatewayResponseBody,
                > = Box::new(ChatCompletionsFromResponsesSseReader::from_stream_response(
                    upstream,
                    Arc::clone(&usage_collector),
                    request_started_at,
                ));
                return Ok(respond_passthrough_collector_stream(
                    request,
                    status,
                    headers,
                    response_body,
                    usage_collector,
                    UpstreamDebugMetaRefs {
                        request_id: &upstream_request_id,
                        cf_ray: &upstream_cf_ray,
                        auth_error: &upstream_auth_error,
                        identity_error_code: &upstream_identity_error_code,
                        content_type: &upstream_content_type,
                    },
                )
                .await);
            }
            ResponseAdapter::CompactFromChatCompletions => unreachable!(),
            ResponseAdapter::ImagesB64JsonFromResponses
            | ResponseAdapter::ImagesUrlFromResponses => {
                let response_format = if response_adapter == ResponseAdapter::ImagesUrlFromResponses
                {
                    ImagesResponseFormat::Url
                } else {
                    ImagesResponseFormat::B64Json
                };
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<
                    dyn crate::http::gateway_response_body::GatewayResponseBody,
                > = Box::new(ImagesFromResponsesSseReader::from_stream_response(
                    upstream,
                    Arc::clone(&usage_collector),
                    request_started_at,
                    response_format,
                ));
                return Ok(respond_passthrough_collector_stream(
                    request,
                    status,
                    headers,
                    response_body,
                    usage_collector,
                    UpstreamDebugMetaRefs {
                        request_id: &upstream_request_id,
                        cf_ray: &upstream_cf_ray,
                        auth_error: &upstream_auth_error,
                        identity_error_code: &upstream_identity_error_code,
                        content_type: &upstream_content_type,
                    },
                )
                .await);
            }
            ResponseAdapter::GeminiJson | ResponseAdapter::GeminiCliJson => unreachable!(),
            ResponseAdapter::GeminiSse | ResponseAdapter::GeminiCliSse => {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<
                    dyn crate::http::gateway_response_body::GatewayResponseBody,
                > = Box::new(GeminiSseReader::from_stream_response(
                    upstream,
                    Arc::clone(&usage_collector),
                    tool_name_restore_map.cloned(),
                    gemini_stream_output_mode.unwrap_or(GeminiStreamOutputMode::Sse),
                    gemini_cli_wrap_response_envelope(response_adapter),
                    request_started_at,
                ));
                return Ok(respond_passthrough_collector_stream(
                    request,
                    status,
                    headers,
                    response_body,
                    usage_collector,
                    UpstreamDebugMetaRefs {
                        request_id: &upstream_request_id,
                        cf_ray: &upstream_cf_ray,
                        auth_error: &upstream_auth_error,
                        identity_error_code: &upstream_identity_error_code,
                        content_type: &upstream_content_type,
                    },
                )
                .await);
            }
            ResponseAdapter::Passthrough => {}
        }
    }
    match response_adapter {
        ResponseAdapter::Passthrough => {
            let status = StatusCode(upstream.status().as_u16());
            let mut headers = copy_upstream_response_headers(upstream.headers(), trace_id);
            if !is_stream {
                let upstream_body = upstream
                    .read_all_bytes_cancellable(request.cancellation_receiver())
                    .await
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let detected_sse =
                    is_sse || (!is_json && looks_like_sse_payload(upstream_body.as_ref()));
                let is_compact_request = is_compact_request_path(request_path);
                if detected_sse {
                    let (synthesized_body, mut usage) =
                        collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                    let synthesized_response = synthesized_body.is_some();
                    let body = synthesized_body.unwrap_or_else(|| upstream_body.to_vec());
                    merge_usage_from_body_without_output_text(&mut usage, &body);
                    let upstream_error_hint = with_upstream_debug_suffix(
                        extract_error_hint_from_body_or_headers(
                            status.0,
                            upstream_content_type.as_deref(),
                            &body,
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        ),
                        None,
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    );
                    if should_suppress_deactivation_delivery(
                        upstream_error_hint.as_deref(),
                        allow_failover_for_deactivation,
                    ) {
                        return Ok(terminal_bridge_result_with_debug_meta(
                            usage,
                            None,
                            upstream_error_hint,
                            &upstream_request_id,
                            &upstream_cf_ray,
                            &upstream_auth_error,
                            &upstream_identity_error_code,
                            &upstream_content_type,
                        ));
                    }
                    if synthesized_response {
                        replace_content_type_header(&mut headers, "application/json");
                    }
                    if status.0 < 400
                        && is_compact_request
                        && !compact_success_body_is_valid(body.as_ref())
                    {
                        return Ok(respond_invalid_compact_success_body(
                            request,
                            usage,
                            body.as_ref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        )
                        .await);
                    }
                    if is_compact_request
                        && compact_non_success_body_should_be_normalized(
                            status.0,
                            upstream_content_type.as_deref(),
                            body.as_ref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        )
                    {
                        return Ok(respond_invalid_compact_non_success_body(
                            request,
                            status.0,
                            usage,
                            body.as_ref(),
                            upstream_content_type.as_deref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        )
                        .await);
                    }
                    if status.0 >= 400
                        && non_success_body_should_be_normalized(
                            status.0,
                            upstream_content_type.as_deref(),
                            body.as_ref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        )
                    {
                        return Ok(respond_normalized_passthrough_non_success_body(
                            request,
                            usage,
                            body.as_ref(),
                            upstream_content_type.as_deref(),
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                            trace_id,
                        )
                        .await);
                    }
                    let len = Some(body.len());
                    let response = Response::new(status, headers, std::io::Cursor::new(body), len);
                    let delivery_error = request
                        .respond_async(response)
                        .await
                        .err()
                        .map(|err| err.to_string());
                    return Ok(terminal_bridge_result_with_debug_meta(
                        usage,
                        delivery_error,
                        upstream_error_hint,
                        &upstream_request_id,
                        &upstream_cf_ray,
                        &upstream_auth_error,
                        &upstream_identity_error_code,
                        &upstream_content_type,
                    ));
                }

                let (_, sse_usage) = collect_non_stream_json_from_sse_bytes(upstream_body.as_ref());
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else if usage_has_signal(&sse_usage) {
                    sse_usage
                } else {
                    UpstreamResponseUsage::default()
                };
                if status.0 < 400
                    && is_compact_request
                    && !compact_success_body_is_valid(upstream_body.as_ref())
                {
                    return Ok(respond_invalid_compact_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    )
                    .await);
                }
                if is_compact_request
                    && compact_non_success_body_should_be_normalized(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                {
                    return Ok(respond_invalid_compact_non_success_body(
                        request,
                        status.0,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    )
                    .await);
                }
                if status.0 >= 400
                    && non_success_body_should_be_normalized(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                {
                    return Ok(respond_normalized_passthrough_non_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    )
                    .await);
                }
                let upstream_error_hint = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    ),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                if should_suppress_deactivation_delivery(
                    upstream_error_hint.as_deref(),
                    allow_failover_for_deactivation,
                ) {
                    return Ok(terminal_bridge_result_with_debug_meta(
                        usage,
                        None,
                        upstream_error_hint,
                        &upstream_request_id,
                        &upstream_cf_ray,
                        &upstream_auth_error,
                        &upstream_identity_error_code,
                        &upstream_content_type,
                    ));
                }
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                );
                let delivery_error = request
                    .respond_async(response)
                    .await
                    .err()
                    .map(|err| err.to_string());
                return Ok(terminal_bridge_result_with_debug_meta(
                    usage,
                    delivery_error,
                    upstream_error_hint,
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                ));
            }
            if is_stream && !is_sse && status.0 >= 400 {
                let upstream_body = upstream
                    .read_all_bytes_cancellable(request.cancellation_receiver())
                    .await
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else {
                    UpstreamResponseUsage::default()
                };
                if non_success_body_should_be_normalized(
                    status.0,
                    upstream_content_type.as_deref(),
                    upstream_body.as_ref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                ) {
                    return Ok(respond_normalized_passthrough_non_success_body(
                        request,
                        usage,
                        upstream_body.as_ref(),
                        upstream_content_type.as_deref(),
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                        trace_id,
                    )
                    .await);
                }
                let upstream_error_hint = with_upstream_debug_suffix(
                    extract_error_hint_from_body_or_headers(
                        status.0,
                        upstream_content_type.as_deref(),
                        upstream_body.as_ref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    ),
                    None,
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                let len = Some(upstream_body.len());
                let response = Response::new(
                    status,
                    headers,
                    std::io::Cursor::new(upstream_body.to_vec()),
                    len,
                );
                let delivery_error = request
                    .respond_async(response)
                    .await
                    .err()
                    .map(|err| err.to_string());
                return Ok(terminal_bridge_result_with_debug_meta(
                    usage,
                    delivery_error,
                    upstream_error_hint,
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                ));
            }
            if is_stream && !is_sse && status.0 < 400 && is_compact_request_path(request_path) {
                let upstream_body = upstream
                    .read_all_bytes_cancellable(request.cancellation_receiver())
                    .await
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let usage = if is_json {
                    serde_json::from_slice::<Value>(upstream_body.as_ref())
                        .ok()
                        .map(|value| parse_usage_from_json(&value))
                        .unwrap_or_default()
                } else {
                    UpstreamResponseUsage::default()
                };
                return Ok(respond_compact_success_body(
                    request,
                    status,
                    headers,
                    usage,
                    upstream_body.as_ref(),
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                    &upstream_content_type,
                    trace_id,
                )
                .await);
            }
            if is_sse || is_stream {
                let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
                let response_body: Box<
                    dyn crate::http::gateway_response_body::GatewayResponseBody,
                > = if request_path.starts_with("/v1/responses") {
                    Box::new(OpenAIResponsesPassthroughSseReader::from_stream_response(
                        upstream,
                        Arc::clone(&usage_collector),
                        keepalive_frame,
                        request_started_at,
                    ))
                } else {
                    Box::new(PassthroughSseUsageReader::from_stream_response(
                        upstream,
                        Arc::clone(&usage_collector),
                        keepalive_frame,
                        passthrough_sse_protocol,
                        request_started_at,
                    ))
                };
                force_openai_responses_stream_content_type(&mut headers, request_path, is_stream);
                let delivery_error =
                    respond_streaming_chunked(request, status, headers, response_body)
                        .await
                        .err()
                        .map(|err| err.to_string());
                let collector = usage_collector
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                let last_sse_event_type = collector.last_event_type.clone();
                let result = with_bridge_debug_meta(
                    UpstreamResponseBridgeResult {
                        usage: collector.usage,
                        stream_terminal_seen: collector.saw_terminal,
                        stream_terminal_delivered: collector.terminal_delivered,
                        stream_terminal_error: collector.terminal_error,
                        delivery_error,
                        upstream_error_hint: with_upstream_debug_suffix(
                            collector.upstream_error_hint,
                            None,
                            upstream_request_id.as_deref(),
                            upstream_cf_ray.as_deref(),
                            upstream_auth_error.as_deref(),
                            upstream_identity_error_code.as_deref(),
                        ),
                        delivered_status_code: None,
                        upstream_request_id: None,
                        upstream_cf_ray: None,
                        upstream_auth_error: None,
                        upstream_identity_error_code: None,
                        upstream_content_type: None,
                        last_sse_event_type: None,
                    },
                    &upstream_request_id,
                    &upstream_cf_ray,
                    &upstream_auth_error,
                    &upstream_identity_error_code,
                    &upstream_content_type,
                    last_sse_event_type,
                );
                log_bridge_stream_diagnostics(response_adapter, request_path, &result);
                return Ok(result);
            }
            let upstream_body = upstream
                .read_all_bytes_cancellable(request.cancellation_receiver())
                .await?;
            let len = Some(upstream_body.len());
            let response = Response::new(status, headers, std::io::Cursor::new(upstream_body), len);
            let delivery_error = request
                .respond_async(response)
                .await
                .err()
                .map(|err| err.to_string());
            Ok(terminal_bridge_result_with_debug_meta(
                UpstreamResponseUsage::default(),
                delivery_error,
                None,
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
            ))
        }
        ResponseAdapter::CompactFromChatCompletions => {
            let status = StatusCode(upstream.status().as_u16());
            let headers = copy_upstream_response_headers(upstream.headers(), trace_id);
            let upstream_body = upstream
                .read_all_bytes_cancellable(request.cancellation_receiver())
                .await
                .map_err(|err| format!("read upstream body failed: {err}"))?;
            let usage = serde_json::from_slice::<Value>(upstream_body.as_ref())
                .ok()
                .map(|value| parse_usage_from_json(&value))
                .unwrap_or_default();
            let response_body = if status.0 < 400 {
                convert_chat_completions_body_to_compact(upstream_body.as_ref())
                    .unwrap_or_else(|| upstream_body.to_vec())
            } else {
                upstream_body.to_vec()
            };
            let upstream_error_hint = (status.0 >= 400)
                .then(|| {
                    with_upstream_debug_suffix(
                        extract_error_hint_from_body(status.0, upstream_body.as_ref()),
                        None,
                        upstream_request_id.as_deref(),
                        upstream_cf_ray.as_deref(),
                        upstream_auth_error.as_deref(),
                        upstream_identity_error_code.as_deref(),
                    )
                })
                .flatten();
            let delivery_error = respond_json_bytes(request, status, headers, response_body).await;
            Ok(terminal_bridge_result_with_debug_meta(
                usage,
                delivery_error,
                upstream_error_hint,
                &upstream_request_id,
                &upstream_cf_ray,
                &upstream_auth_error,
                &upstream_identity_error_code,
                &upstream_content_type,
            ))
        }
        ResponseAdapter::AnthropicMessagesFromResponses
        | ResponseAdapter::ResponsesFromAnthropicMessages
        | ResponseAdapter::ChatCompletionsFromResponses
        | ResponseAdapter::ImagesB64JsonFromResponses
        | ResponseAdapter::ImagesUrlFromResponses
        | ResponseAdapter::GeminiJson
        | ResponseAdapter::GeminiCliJson
        | ResponseAdapter::GeminiSse
        | ResponseAdapter::GeminiCliSse => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;
