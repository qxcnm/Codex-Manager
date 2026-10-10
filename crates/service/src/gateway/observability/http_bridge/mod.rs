use crate::http::gateway_request::GatewayRequest as Request;

use crate::gateway::upstream::GatewayUpstreamResponse;

mod aggregate;
mod body_conversion;
mod compact_delivery;
mod compact_errors;
mod images;
mod manual_chunked;
mod metadata;
#[cfg(test)]
mod openai;
mod response_helpers;
use aggregate::openai_responses_event::{OpenAIResponsesEvent, OpenAIResponsesOutputTextState};
pub(crate) use aggregate::PassthroughSseProtocol;
pub(crate) use aggregate::UpstreamResponseBridgeResult;
#[allow(unused_imports)]
use aggregate::{
    append_output_text, collect_output_text_from_event_fields, collect_response_output_text,
    collect_response_reasoning_summary_text,
};
use aggregate::{
    collect_non_stream_json_from_sse_bytes, extract_error_hint_from_body,
    extract_error_message_from_json, inspect_sse_frame_for_protocol, looks_like_sse_payload,
    merge_usage, parse_usage_from_json, reload_output_text_from_env, usage_has_signal, SseTerminal,
    UpstreamResponseUsage,
};
#[cfg(test)]
use aggregate::{
    inspect_sse_frame, output_text_limit_bytes, parse_sse_frame_json, parse_usage_from_sse_frame,
    OUTPUT_TEXT_TRUNCATED_MARKER,
};
use images::{
    build_images_api_response, chat_image_payload, collect_image_generation_chat_images,
    collect_image_generation_data_urls, collect_image_generation_results,
    image_generation_result_payload, images_usage_value, mime_type_from_codex_output_format,
    ImagesResponseFormat,
};

/// 函数 `reload_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn reload_from_env() {
    reload_output_text_from_env();
}

/// 函数 `summarize_upstream_error_hint_from_body`
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
pub(crate) fn summarize_upstream_error_hint_from_body(
    status_code: u16,
    body: &[u8],
) -> Option<String> {
    aggregate::extract_error_hint_from_body(status_code, body)
}

mod delivery;
mod stream_readers;

const DEFAULT_BRIDGE_MODEL: &str = "gpt-6-sol";

static FINALIZATION_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(32);
static DEFERRED_RESPONSES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(256);
static ACTIVE_DEFERRED_RESPONSES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Reserve response/accounting capacity before contacting a provider, so a
/// saturated service never spends provider tokens on an unaccounted response.
pub(crate) fn reserve_deferred_response(request: &mut Request) -> bool {
    if request.has_response_admission() {
        return true;
    }
    let Ok(permit) = DEFERRED_RESPONSES.try_acquire() else {
        return false;
    };
    request.hold_response_admission(permit);
    true
}

struct DeferredResponseGuard {
    _permit: tokio::sync::SemaphorePermit<'static>,
    _request_guards: Vec<Box<dyn Send>>,
    completion: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for DeferredResponseGuard {
    fn drop(&mut self) {
        self._request_guards.clear();
        if let Some(completion) = self.completion.take() {
            let _ = completion.send(());
        }
        ACTIVE_DEFERRED_RESPONSES.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

/// Listener shutdown must drain accounting even when the client disconnected
/// and there is no response body left for Hyper to await.
pub(crate) async fn drain_deferred_responses() {
    crate::http::gateway_endpoint::drain_domain_workers().await;
    while ACTIVE_DEFERRED_RESPONSES.load(std::sync::atomic::Ordering::Acquire) != 0 {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    if crate::shutdown_requested() {
        crate::auth_callback::drain_login_server().await;
        crate::auth_tokens::drain_device_login_tasks().await;
        crate::auth_tokens::drain_auth_completions().await;
        crate::account::background::drain_account_background_tasks().await;
        crate::usage_refresh::drain_usage_background_tasks().await;
        crate::plugin::drain_scheduler().await;
        crate::runtime::blocking::drain().await;
    }
    crate::usage_token_refresh::drain_token_refresh_tasks().await;
    if let Err(error) = crate::gateway::trace_log::drain_trace_writer().await {
        log::error!("gateway trace shutdown flush failed: {error}");
    }
}

/// The routing worker transfers ownership before reading the response body.
/// Provider waits and downstream backpressure run on async tasks; only the
/// final database/accounting callback occupies a bounded blocking worker.
#[allow(clippy::too_many_arguments)]
pub(super) fn defer_upstream_response<F>(
    mut request: Request,
    upstream: GatewayUpstreamResponse,
    inflight_guard: super::AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<super::GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&super::ToolNameRestoreMap>,
    is_stream: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
    finalize: F,
) -> Result<(), String>
where
    F: FnOnce(UpstreamResponseBridgeResult) + Send + 'static,
{
    // Production requests reserve at the HTTP boundary. Direct bridge fixtures
    // use the same reservation here before starting their delivery task.
    if !reserve_deferred_response(&mut request) {
        let response = crate::http::gateway_response::Response::from_string("service busy")
            .with_status_code(503);
        let _ = request.respond(response);
        return Err("deferred response capacity exhausted".to_owned());
    }
    let permit = request
        .take_response_admission()
        .expect("response reserved");
    let runtime = super::upstream::attempt_flow::transport::runtime::upstream_runtime()
        .map_err(|error| error.to_string())?;
    let (completion, completed) = tokio::sync::oneshot::channel();
    request.finish_body_after(completed);
    ACTIVE_DEFERRED_RESPONSES.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    let lifecycle = DeferredResponseGuard {
        _permit: permit,
        _request_guards: request.take_lifetime_guards(),
        completion: Some(completion),
    };
    let request_path = request_path.to_owned();
    let tool_name_restore_map = tool_name_restore_map.cloned();
    let trace_id = trace_id.map(str::to_owned);
    let fallback_model = fallback_model.map(str::to_owned);
    runtime.spawn(async move {
        let _lifecycle = lifecycle;
        let bridge = respond_with_upstream_async(
            request,
            upstream,
            inflight_guard,
            response_adapter,
            passthrough_sse_protocol,
            gemini_stream_output_mode,
            &request_path,
            tool_name_restore_map.as_ref(),
            is_stream,
            false,
            trace_id.as_deref(),
            fallback_model.as_deref(),
            request_started_at,
        );
        let result = bridge.await;
        let bridge = result.unwrap_or_else(|error| UpstreamResponseBridgeResult {
            delivery_error: Some(error),
            ..Default::default()
        });
        let Ok(permit) = FINALIZATION_WORKERS.acquire().await else {
            return;
        };
        if tokio::task::spawn_blocking(move || {
            let _permit = permit;
            finalize(bridge);
        })
        .await
        .is_err()
        {
            log::error!("event=gateway_response_finalization_panicked");
        }
    });
    Ok(())
}
/// 函数 `respond_with_upstream`
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
pub(super) async fn respond_with_upstream_async(
    request: Request,
    upstream: GatewayUpstreamResponse,
    inflight_guard: super::AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<super::GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&super::ToolNameRestoreMap>,
    is_stream: bool,
    allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    let upstream = match upstream {
        #[cfg(test)]
        GatewayUpstreamResponse::Blocking(upstream) => {
            crate::gateway::upstream::GatewayStreamResponse::from_blocking_response(upstream)
        }
        GatewayUpstreamResponse::Stream(upstream) => upstream,
    };
    delivery::respond_with_stream_upstream(
        request,
        upstream,
        inflight_guard,
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
    )
    .await
}

#[cfg(test)]
pub(super) fn respond_with_upstream(
    request: Request,
    upstream: GatewayUpstreamResponse,
    inflight_guard: super::AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<super::GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&super::ToolNameRestoreMap>,
    is_stream: bool,
    allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    crate::gateway::response_test_runtime()
        .map_err(|error| error.to_string())?
        .block_on(respond_with_upstream_async(
            request,
            upstream,
            inflight_guard,
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

pub(super) use stream_readers::{
    ChatCompletionsFromResponsesSseReader, ImagesFromResponsesSseReader,
    OpenAIResponsesPassthroughSseReader, PassthroughSseCollector, PassthroughSseUsageReader,
    ResponsesFromAnthropicSseReader, SseKeepAliveFrame,
};

pub(super) use stream_readers::{AnthropicSseReader, GeminiSseReader};

#[cfg(test)]
#[path = "../tests/http_bridge_tests.rs"]
mod tests;

#[cfg(test)]
mod native_async_tests;
