use crate::http::gateway_request::GatewayRequest as Request;

use super::super::super::request_log::RequestLogUsage;
use super::super::GatewayUpstreamResponse;
use super::execution_context::GatewayUpstreamExecutionContext;

pub(super) enum FinalizeUpstreamResponseOutcome {
    Handled,
}

/// 函数 `respond_terminal`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn respond_terminal(
    request: Request,
    status_code: u16,
    message: String,
    trace_id: Option<&str>,
) -> Result<(), String> {
    let response_message = super::super::super::error_message_for_client(
        super::super::super::prefers_raw_errors_for_gateway_request(&request),
        message,
    );
    let response = super::super::super::error_response::terminal_text_response(
        status_code,
        response_message,
        trace_id,
    );
    let _ = request.respond(response);
    Ok(())
}

/// 函数 `is_client_disconnect_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
fn is_client_disconnect_error(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.contains("broken pipe")
        || normalized.contains("connection reset")
        || normalized.contains("connection aborted")
        || normalized.contains("connection was forcibly closed")
        || normalized.contains("os error 32")
        || normalized.contains("os error 54")
        || normalized.contains("os error 104")
        || normalized.contains("os error 10053")
        || normalized.contains("os error 10054")
        || normalized.contains("os error 10058")
}

fn derive_final_error(
    status_code: u16,
    last_attempt_error: Option<&str>,
    upstream_error_hint: Option<&str>,
    bridge_error_message: Option<String>,
) -> Option<String> {
    upstream_error_hint
        .map(str::to_string)
        .or_else(|| {
            (status_code >= 400)
                .then(|| last_attempt_error.map(str::to_string))
                .flatten()
        })
        .or(bridge_error_message)
}

fn derive_status_for_log(
    status_code: u16,
    delivered_status_code: Option<u16>,
    bridge_ok: bool,
    gateway_failover: bool,
    upstream_stream_failed: bool,
    client_delivery_failed: bool,
) -> u16 {
    if client_delivery_failed {
        499
    } else if let Some(delivered_status_code) = delivered_status_code {
        delivered_status_code
    } else if status_code >= 400 {
        status_code
    } else if upstream_stream_failed || gateway_failover || !bridge_ok {
        502
    } else {
        status_code
    }
}

/// 函数 `respond_total_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn respond_total_timeout(
    request: Request,
    context: &GatewayUpstreamExecutionContext<'_>,
    trace_id: &str,
    started_at: std::time::Instant,
    model_for_log: Option<&str>,
    attempted_account_ids: Option<&[String]>,
) -> Result<(), String> {
    let message = "upstream total timeout exceeded".to_string();
    context.log_final_result_with_model(
        None,
        None,
        model_for_log,
        504,
        RequestLogUsage::default(),
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
        attempted_account_ids,
    );
    respond_terminal(request, 504, message, Some(trace_id))
}

/// 函数 `finalize_terminal_candidate`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn finalize_terminal_candidate(
    request: Request,
    context: &GatewayUpstreamExecutionContext<'_>,
    account_id: &str,
    last_attempt_url: Option<&str>,
    status_code: u16,
    message: String,
    trace_id: &str,
    started_at: std::time::Instant,
    model_for_log: Option<&str>,
    attempted_account_ids: Option<&[String]>,
) -> Result<(), String> {
    let _ = context.mark_account_unavailable_for_gateway_error(account_id, &message);
    context.log_final_result_with_model(
        Some(account_id),
        last_attempt_url,
        model_for_log,
        status_code,
        RequestLogUsage::default(),
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
        attempted_account_ids,
    );
    respond_terminal(request, status_code, message, Some(trace_id))
}

/// 函数 `finalize_upstream_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
#[allow(clippy::too_many_arguments)]
pub(super) async fn finalize_upstream_response(
    request: Request,
    response: GatewayUpstreamResponse,
    inflight_guard: super::super::super::AccountInFlightGuard,
    context: &GatewayUpstreamExecutionContext<'_>,
    account_id: &str,
    last_attempt_url: Option<&str>,
    last_attempt_error: Option<&str>,
    response_adapter: super::super::super::ResponseAdapter,
    gemini_stream_output_mode: Option<super::super::super::GeminiStreamOutputMode>,
    tool_name_restore_map: &super::super::super::ToolNameRestoreMap,
    client_is_stream: bool,
    path: &str,
    trace_id: &str,
    started_at: std::time::Instant,
    model_for_log: Option<&str>,
    attempted_account_ids: Option<&[String]>,
    has_more_candidates: bool,
) -> Result<FinalizeUpstreamResponseOutcome, String> {
    let status_code = response.status().as_u16();

    #[cfg(test)]
    if !request.is_native() {
        let bridge = super::super::super::http_bridge::respond_with_upstream_async(
            request,
            response,
            inflight_guard,
            response_adapter,
            None,
            gemini_stream_output_mode,
            path,
            Some(tool_name_restore_map),
            client_is_stream,
            // Once delivery starts, the response is committed and the request cannot be retried.
            // Retryable stream errors are therefore gated before this function is called.
            false,
            Some(trace_id),
            model_for_log,
            started_at,
        )
        .await?;
        return finalize_bridge_result(
            bridge,
            status_code,
            context,
            account_id,
            last_attempt_url,
            last_attempt_error,
            response_adapter,
            client_is_stream,
            path,
            trace_id,
            started_at,
            model_for_log,
            attempted_account_ids,
            has_more_candidates,
        );
    }
    let owned_context = context.to_owned();
    let account_id_owned = account_id.to_owned();
    let last_attempt_url_owned = last_attempt_url.map(str::to_owned);
    let last_attempt_error_owned = last_attempt_error.map(str::to_owned);
    let path_owned = path.to_owned();
    let trace_id_owned = trace_id.to_owned();
    let model_for_log_owned = model_for_log.map(str::to_owned);
    let attempted_account_ids_owned = attempted_account_ids.map(<[String]>::to_vec);
    super::super::super::defer_upstream_response(
        request,
        response,
        inflight_guard,
        response_adapter,
        None,
        gemini_stream_output_mode,
        path,
        Some(tool_name_restore_map),
        client_is_stream,
        Some(trace_id),
        model_for_log,
        started_at,
        move |bridge| {
            let Some(storage) = crate::storage_helpers::open_storage() else {
                log::error!("event=gateway_response_finalization_storage_unavailable");
                return;
            };
            let context = owned_context.as_borrowed(&storage);
            let _ = finalize_bridge_result(
                bridge,
                status_code,
                &context,
                &account_id_owned,
                last_attempt_url_owned.as_deref(),
                last_attempt_error_owned.as_deref(),
                response_adapter,
                client_is_stream,
                &path_owned,
                &trace_id_owned,
                started_at,
                model_for_log_owned.as_deref(),
                attempted_account_ids_owned.as_deref(),
                has_more_candidates,
            );
        },
    )?;
    Ok(FinalizeUpstreamResponseOutcome::Handled)
}

#[allow(clippy::too_many_arguments)]
fn finalize_bridge_result(
    bridge: crate::gateway::http_bridge::UpstreamResponseBridgeResult,
    status_code: u16,
    context: &GatewayUpstreamExecutionContext<'_>,
    account_id: &str,
    last_attempt_url: Option<&str>,
    last_attempt_error: Option<&str>,
    response_adapter: super::super::super::ResponseAdapter,
    client_is_stream: bool,
    path: &str,
    trace_id: &str,
    started_at: std::time::Instant,
    model_for_log: Option<&str>,
    attempted_account_ids: Option<&[String]>,
    has_more_candidates: bool,
) -> Result<FinalizeUpstreamResponseOutcome, String> {
    let bridge_output_text_len = bridge
        .usage
        .output_text
        .as_deref()
        .map(str::trim)
        .map(str::len)
        .unwrap_or(0);
    super::super::super::trace_log::log_bridge_result(
        super::super::super::trace_log::BridgeResultLog {
            trace_id,
            adapter: format!("{response_adapter:?}").as_str(),
            path,
            is_stream: client_is_stream,
            stream_terminal_seen: bridge.stream_terminal_seen,
            stream_terminal_delivered: bridge.stream_terminal_delivered,
            stream_terminal_error: bridge.stream_terminal_error.as_deref(),
            delivery_error: bridge.delivery_error.as_deref(),
            output_text_len: bridge_output_text_len,
            output_tokens: bridge.usage.output_tokens,
            first_response_ms: bridge.usage.first_response_ms,
            delivered_status_code: bridge.delivered_status_code,
            upstream_error_hint: bridge.upstream_error_hint.as_deref(),
            upstream_request_id: bridge.upstream_request_id.as_deref(),
            upstream_cf_ray: bridge.upstream_cf_ray.as_deref(),
            upstream_auth_error: bridge.upstream_auth_error.as_deref(),
            upstream_identity_error_code: bridge.upstream_identity_error_code.as_deref(),
            upstream_content_type: bridge.upstream_content_type.as_deref(),
            last_sse_event_type: bridge.last_sse_event_type.as_deref(),
        },
    );
    let bridge_ok = bridge.is_ok(client_is_stream);
    let bridge_error_message = (!bridge_ok).then(|| {
        bridge
            .error_message(client_is_stream)
            .unwrap_or_else(|| "upstream response incomplete".to_string())
    });
    let final_error = derive_final_error(
        status_code,
        last_attempt_error,
        bridge.upstream_error_hint.as_deref(),
        bridge_error_message,
    );
    let gateway_error_follow_up = final_error
        .as_deref()
        .map(|error| context.apply_gateway_error_follow_up(account_id, error, has_more_candidates));
    let gateway_failover =
        gateway_error_follow_up.is_some_and(|follow_up| follow_up.should_failover);

    let upstream_stream_failed = client_is_stream
        && (!bridge.stream_terminal_seen || bridge.stream_terminal_error.is_some());
    let client_delivery_failed = bridge
        .delivery_error
        .as_deref()
        .is_some_and(is_client_disconnect_error);
    let status_for_log = derive_status_for_log(
        status_code,
        bridge.delivered_status_code,
        bridge_ok,
        gateway_failover,
        upstream_stream_failed,
        client_delivery_failed,
    );

    if upstream_stream_failed {
        super::super::super::mark_account_cooldown(
            account_id,
            super::super::super::CooldownReason::Network,
        );
        super::super::super::record_route_quality(account_id, 502);
    }

    clear_model_rejection_after_success(
        account_id,
        model_for_log,
        status_code,
        client_is_stream,
        &bridge,
    );
    let usage = bridge.usage;
    let response_id = (bridge_ok && status_for_log < 400)
        .then(|| usage.response_id.as_deref())
        .flatten();
    context.log_final_result_with_model(
        Some(account_id),
        last_attempt_url,
        model_for_log,
        status_for_log,
        RequestLogUsage {
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            reasoning_output_tokens: usage.reasoning_output_tokens,
            first_response_ms: usage.first_response_ms,
            estimated_input_tokens: None,
        },
        final_error.as_deref(),
        started_at.elapsed().as_millis(),
        attempted_account_ids,
    );
    if let Some(response_id) = response_id {
        context.record_completed_response_id(response_id);
    }
    Ok(FinalizeUpstreamResponseOutcome::Handled)
}

fn clear_model_rejection_after_success(
    account_id: &str,
    model: Option<&str>,
    status_code: u16,
    is_stream: bool,
    bridge: &crate::gateway::http_bridge::UpstreamResponseBridgeResult,
) {
    let completed = if is_stream {
        bridge.stream_terminal_seen && bridge.stream_terminal_error.is_none()
    } else {
        bridge.usage.completed_successfully
    };
    if (200..=299).contains(&status_code)
        && bridge
            .delivered_status_code
            .is_none_or(|code| (200..=299).contains(&code))
        && completed
        && !bridge.usage.explicit_failure
        && bridge.is_ok(is_stream)
        && bridge.upstream_error_hint.is_none()
        && bridge.stream_terminal_error.is_none()
    {
        if let Some(model) = model {
            crate::account::model_support::clear_unsupported(account_id, model);
        }
    }
}

#[cfg(test)]
#[path = "response_finalize_tests.rs"]
mod tests;
