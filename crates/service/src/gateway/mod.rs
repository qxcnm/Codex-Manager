use crate::storage_helpers::open_storage;

pub(crate) const MISSING_AUTH_JSON_OPENAI_API_KEY_ERROR: &str =
    "配置错误：未配置auth.json的OPENAI_API_KEY(invalid api key)";

pub(crate) fn bilingual_error(
    chinese_description: impl AsRef<str>,
    english_raw_message: impl AsRef<str>,
) -> String {
    format!(
        "{}({})",
        chinese_description.as_ref(),
        english_raw_message.as_ref()
    )
}

pub(crate) fn extract_raw_error_message(message: &str) -> Option<&str> {
    let (_, tail) = message.rsplit_once('(')?;
    let tail = tail.strip_suffix(')')?.trim();
    if tail.is_empty() || !tail.is_ascii() || !tail.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return None;
    }
    Some(tail)
}

fn is_codex_user_agent(value: &str) -> bool {
    value.to_ascii_lowercase().contains("codex_cli_rs")
}

fn is_codex_header_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("x-openai-subagent")
        || name.eq_ignore_ascii_case("x-client-request-id")
        || name.eq_ignore_ascii_case("session_id")
        || name.eq_ignore_ascii_case("conversation_id")
        || name.to_ascii_lowercase().starts_with("x-codex-")
}

pub(crate) fn prefers_raw_errors_for_http_headers(headers: &axum::http::HeaderMap) -> bool {
    headers.iter().any(|(name, value)| {
        is_codex_header_name(name.as_str())
            || (name.as_str().eq_ignore_ascii_case("User-Agent")
                && value.to_str().ok().is_some_and(is_codex_user_agent))
    })
}

pub(crate) fn prefers_raw_errors_for_tiny_http_request(request: &tiny_http::Request) -> bool {
    request.headers().iter().any(|header| {
        let name = header.field.as_str().as_str();
        is_codex_header_name(name)
            || (header.field.equiv("User-Agent") && is_codex_user_agent(header.value.as_str()))
    })
}

pub(crate) fn error_message_for_client(
    _prefers_raw_errors: bool,
    message: impl Into<String>,
) -> String {
    let message = message.into();
    if let Some(raw) = extract_raw_error_message(message.as_str()) {
        return raw.to_string();
    }
    message
}

mod anchor_fingerprint;
mod concurrency;
#[path = "routing/conversation_binding.rs"]
mod conversation_binding;
#[path = "routing/cooldown.rs"]
mod cooldown;
#[path = "observability/error_log.rs"]
mod error_log;
mod error_response;
#[path = "routing/failover.rs"]
mod failover;
#[path = "observability/http_bridge/mod.rs"]
mod http_bridge;
#[path = "request/incoming_headers.rs"]
mod incoming_headers;
#[path = "request/local_count_tokens.rs"]
mod local_count_tokens;
#[path = "request/local_models.rs"]
mod local_models;
#[path = "request/local_response.rs"]
mod local_response;
mod local_validation;
#[path = "observability/metrics.rs"]
mod metrics;
mod model_picker;
#[path = "request/official_responses_http.rs"]
mod official_responses_http;
#[path = "auth/openai_fallback.rs"]
mod openai_fallback;
mod protocol_adapter;
#[path = "request/request_entry.rs"]
mod request_entry;
#[path = "routing/request_gate.rs"]
mod request_gate;
#[path = "request/request_helpers.rs"]
mod request_helpers;
#[path = "observability/request_log.rs"]
mod request_log;
#[path = "request/request_rewrite.rs"]
mod request_rewrite;
#[path = "routing/route_hint.rs"]
mod route_hint;
#[path = "routing/route_quality.rs"]
mod route_quality;
#[path = "core/runtime_config.rs"]
mod runtime_config;
#[path = "routing/selection.rs"]
mod selection;
#[path = "request/session_affinity.rs"]
mod session_affinity;
#[path = "request/thread_anchor.rs"]
mod thread_anchor;
#[path = "auth/token_exchange.rs"]
mod token_exchange;
#[path = "observability/trace_log.rs"]
mod trace_log;
mod upstream;

pub(crate) use concurrency::current_gateway_concurrency_recommendation;
pub(crate) use error_log::write_gateway_error_log;
use metrics::{
    account_inflight_count, acquire_account_inflight, begin_gateway_request,
    record_gateway_candidate_skip, record_gateway_cooldown_mark, record_gateway_failover_attempt,
    record_gateway_request_outcome, AccountInFlightGuard,
};
pub(crate) use metrics::{
    begin_rpc_request, duration_to_millis, gateway_metrics_prometheus,
    record_usage_refresh_outcome, GatewayCandidateSkipReason,
};
pub(super) use official_responses_http::normalize_official_responses_http_body;
use protocol_adapter::build_gemini_error_body;
use protocol_adapter::{
    adapt_request_for_protocol, GeminiStreamOutputMode, ResponseAdapter, ToolNameRestoreMap,
};
pub(super) use request_helpers::{
    inspect_service_tier_for_log, inspect_service_tier_value, is_html_content_type,
    is_upstream_challenge_response, normalize_models_path, parse_request_metadata,
    validate_text_input_limit_for_path,
};
#[cfg(test)]
use request_helpers::{should_drop_incoming_header, should_drop_incoming_header_for_failover};
pub(crate) use request_log::{RequestLogTraceContext, RequestLogUsage};
#[cfg(test)]
use request_rewrite::apply_request_overrides_with_service_tier_and_prompt_cache_key;
use request_rewrite::{
    apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope,
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope, compute_upstream_url,
};
pub(super) use thread_anchor::{
    resolve_fallback_thread_anchor, resolve_local_conversation_id_with_sticky_fallback,
};
pub(crate) use trace_log::{
    log_client_service_tier, log_request_execution_plan, log_request_final, log_request_start,
    next_trace_id,
};
#[cfg(test)]
use upstream::config::normalize_upstream_base_url;
use upstream::config::{
    is_openai_api_base, resolve_upstream_base_url, resolve_upstream_fallback_base_url,
    should_try_openai_fallback, should_try_openai_fallback_by_status,
};
#[cfg(test)]
pub(super) use upstream::header_profile::{
    build_codex_compact_upstream_headers, build_codex_upstream_headers,
    CodexCompactUpstreamHeaderInput, CodexUpstreamHeaderInput,
};

// HTTP backend runtime metrics are exported via the gateway `/metrics` endpoint as well.
pub(crate) fn record_http_queue_capacity(normal_capacity: usize, stream_capacity: usize) {
    metrics::record_http_queue_capacity(normal_capacity, stream_capacity);
}

/// 函数 `record_http_queue_enqueue`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_http_queue_enqueue(is_stream_queue: bool) {
    metrics::record_http_queue_enqueue(is_stream_queue);
}

/// 函数 `record_http_queue_dequeue`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_http_queue_dequeue(is_stream_queue: bool) {
    metrics::record_http_queue_dequeue(is_stream_queue);
}

/// 函数 `record_http_queue_enqueue_failure`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_http_queue_enqueue_failure() {
    metrics::record_http_queue_enqueue_failure();
}
#[cfg(test)]
use cooldown::cooldown_reason_for_status;
use cooldown::{
    clear_account_cooldown, is_account_in_cooldown, mark_account_cooldown,
    mark_account_cooldown_for_status, CooldownReason,
};
#[cfg(test)]
pub(super) use failover::should_failover_after_refresh;
use failover::should_failover_from_cached_snapshot;
use failover::should_failover_from_low_quota_snapshot;
use http_bridge::respond_with_upstream;
pub(crate) use http_bridge::summarize_upstream_error_hint_from_body;
pub(crate) use http_bridge::PassthroughSseProtocol;
/// 函数 `extract_identity_error_code_from_headers`
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
pub(crate) fn extract_identity_error_code_from_headers(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    headers
        .get("x-error-json")
        .and_then(|value| value.to_str().ok())
        .and_then(extract_identity_error_code_from_header_value)
}

/// 函数 `extract_identity_error_code_from_header_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn extract_identity_error_code_from_header_value(raw: &str) -> Option<String> {
    if let Some(code) = extract_identity_error_code_from_error_json(raw) {
        return Some(code);
    }

    let decoded = decode_base64_header_value(raw.as_bytes())?;
    let decoded = String::from_utf8(decoded).ok()?;
    extract_identity_error_code_from_error_json(&decoded)
}

/// 函数 `extract_identity_error_code_from_error_json`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn extract_identity_error_code_from_error_json(raw: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    value
        .get("identity_error_code")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            value
                .get("error")
                .and_then(serde_json::Value::as_object)
                .and_then(|error| error.get("code"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            value
                .get("error")
                .and_then(serde_json::Value::as_object)
                .and_then(|error| error.get("identity_error_code"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            value
                .get("details")
                .and_then(serde_json::Value::as_object)
                .and_then(|details| details.get("identity_error_code"))
                .and_then(serde_json::Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// 函数 `decode_base64_header_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - input: 参数 input
///
/// # 返回
/// 返回函数执行结果
fn decode_base64_header_value(input: &[u8]) -> Option<Vec<u8>> {
    /// 函数 `decode_char`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - byte: 参数 byte
    ///
    /// # 返回
    /// 返回函数执行结果
    fn decode_char(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }

    let filtered = input
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    if filtered.is_empty() || filtered.len() % 4 != 0 {
        return None;
    }

    let mut output = Vec::with_capacity(filtered.len() / 4 * 3);
    for chunk in filtered.chunks(4) {
        let a = decode_char(chunk[0])?;
        let b = decode_char(chunk[1])?;
        let c_pad = chunk[2] == b'=';
        let d_pad = chunk[3] == b'=';
        let c = if c_pad { 0 } else { decode_char(chunk[2])? };
        let d = if d_pad { 0 } else { decode_char(chunk[3])? };

        output.push((a << 2) | (b >> 4));
        if !c_pad {
            output.push((b << 4) | (c >> 2));
        }
        if !d_pad {
            output.push((c << 6) | d);
        }
    }

    Some(output)
}
pub(super) use incoming_headers::IncomingHeaderSnapshot;
use local_count_tokens::maybe_respond_local_count_tokens;
use local_models::maybe_respond_local_models;
pub(crate) use model_picker::fetch_models_for_picker;
use openai_fallback::try_openai_fallback;
pub(crate) use request_entry::handle_gateway_request;
use request_gate::{request_gate_lock, RequestGateAcquireError};
pub(crate) use request_log::write_request_log;
use route_hint::apply_route_strategy;
use route_quality::record_route_quality;
pub(crate) use runtime_config::fresh_upstream_client;
pub(crate) use runtime_config::front_proxy_max_body_bytes;
pub(crate) use runtime_config::{account_max_inflight_limit, set_account_max_inflight_limit};
use runtime_config::{
    async_upstream_client_for_account, fresh_async_upstream_client_for_account,
    fresh_upstream_client_for_account, request_gate_wait_timeout, trace_body_preview_max_bytes,
    upstream_client_for_account, upstream_stream_timeout, upstream_total_timeout,
    DEFAULT_GATEWAY_DEBUG,
};
use selection::collect_gateway_candidates;
pub(crate) use selection::{
    current_quota_guard_config, invalidate_candidate_cache, set_quota_guard_config,
    QuotaGuardConfig,
};
#[cfg(test)]
use token_exchange::account_token_exchange_lock;
use token_exchange::resolve_openai_bearer_token;
use upstream::proxy::proxy_validated_request;

/// 函数 `reload_runtime_config_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn reload_runtime_config_from_env() {
    runtime_config::reload_from_env();
    selection::reload_from_env();
    request_gate::clear_runtime_state();
    cooldown::clear_runtime_state();
    route_quality::clear_runtime_state();
    route_hint::reload_from_env();
    upstream::config::reload_from_env();
    trace_log::reload_from_env();
    http_bridge::reload_from_env();
}

/// 函数 `current_route_strategy`
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
pub(crate) fn current_route_strategy() -> &'static str {
    route_hint::current_route_strategy()
}

/// 函数 `set_route_strategy`
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
pub(crate) fn set_route_strategy(strategy: &str) -> Result<&'static str, String> {
    let applied = route_hint::set_route_strategy(strategy)?;
    std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", applied);
    Ok(applied)
}

/// 函数 `current_free_account_max_model`
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
pub(crate) fn current_free_account_max_model() -> String {
    runtime_config::current_free_account_max_model()
}

/// 函数 `current_compact_model_override`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_compact_model_override() -> Option<String> {
    runtime_config::current_compact_model_override()
}

/// 函数 `current_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn current_model_forward_rules() -> String {
    runtime_config::current_model_forward_rules()
}

/// 函数 `request_compression_enabled`
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
pub(crate) fn request_compression_enabled() -> bool {
    runtime_config::request_compression_enabled()
}

/// 函数 `current_originator`
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
pub(crate) fn current_originator() -> String {
    runtime_config::current_originator()
}

/// 函数 `default_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 返回 Codex 默认 originator
pub(crate) fn default_originator() -> &'static str {
    runtime_config::default_originator()
}

/// 函数 `current_wire_originator`
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
pub(crate) fn current_wire_originator() -> String {
    runtime_config::current_wire_originator()
}

/// 函数 `current_codex_user_agent_version`
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
pub(crate) fn current_codex_user_agent_version() -> String {
    runtime_config::current_codex_user_agent_version()
}

/// 函数 `default_codex_user_agent_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 返回 Codex 默认 User-Agent 版本
pub(crate) fn default_codex_user_agent_version() -> &'static str {
    runtime_config::default_codex_user_agent_version()
}

/// 函数 `set_originator`
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
pub(crate) fn set_originator(originator: &str) -> Result<String, String> {
    runtime_config::set_originator(originator)
}

/// 函数 `set_codex_user_agent_version`
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
pub(crate) fn set_codex_user_agent_version(version: &str) -> Result<String, String> {
    runtime_config::set_codex_user_agent_version(version)
}

/// 函数 `current_residency_requirement`
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
pub(crate) fn current_residency_requirement() -> Option<String> {
    runtime_config::current_residency_requirement()
}

/// 函数 `set_residency_requirement`
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
pub(crate) fn set_residency_requirement(value: Option<&str>) -> Result<Option<String>, String> {
    runtime_config::set_residency_requirement(value)
}

/// 函数 `current_codex_user_agent`
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
pub(crate) fn current_codex_user_agent() -> String {
    runtime_config::current_codex_user_agent()
}

/// 函数 `set_free_account_max_model`
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
pub(crate) fn set_free_account_max_model(model: &str) -> Result<String, String> {
    runtime_config::set_free_account_max_model(model)
}

/// 函数 `set_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn set_model_forward_rules(raw: &str) -> Result<String, String> {
    runtime_config::set_model_forward_rules(raw)
}

/// 函数 `resolve_forwarded_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_forwarded_model(model: &str) -> Option<String> {
    runtime_config::resolve_forwarded_model(model)
}

/// 函数 `resolve_builtin_forwarded_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-16
///
/// # 参数
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_builtin_forwarded_model(model: &str) -> Option<String> {
    runtime_config::resolve_builtin_forwarded_model(model)
}

/// 函数 `set_request_compression_enabled`
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
pub(crate) fn set_request_compression_enabled(enabled: bool) -> bool {
    runtime_config::set_request_compression_enabled(enabled)
}

/// 函数 `strict_request_param_allowlist_enabled`
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
pub(crate) fn strict_request_param_allowlist_enabled() -> bool {
    runtime_config::strict_request_param_allowlist_enabled()
}

/// 函数 `current_upstream_proxy_url`
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
pub(crate) fn current_upstream_proxy_url() -> Option<String> {
    runtime_config::upstream_proxy_url()
}

pub(crate) fn current_upstream_proxy_url_for_account(account_id: &str) -> Option<String> {
    runtime_config::upstream_proxy_url_for_account(account_id)
}

/// 函数 `set_upstream_proxy_url`
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
pub(crate) fn set_upstream_proxy_url(proxy_url: Option<&str>) -> Result<Option<String>, String> {
    let applied = runtime_config::set_upstream_proxy_url(proxy_url)?;
    // 中文注释：用量轮询和 token 刷新复用独立 HTTP client，代理变更后同步重建，避免继续走旧网络路径。
    crate::usage_http::reload_usage_http_client_from_env();
    Ok(applied)
}

/// 函数 `current_upstream_stream_timeout_ms`
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
pub(crate) fn current_upstream_stream_timeout_ms() -> u64 {
    runtime_config::current_upstream_stream_timeout_ms()
}

pub(crate) fn current_upstream_total_timeout_ms() -> u64 {
    runtime_config::current_upstream_total_timeout_ms()
}

/// 函数 `set_upstream_stream_timeout_ms`
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
pub(crate) fn set_upstream_stream_timeout_ms(timeout_ms: u64) -> u64 {
    runtime_config::set_upstream_stream_timeout_ms(timeout_ms)
}

pub(crate) fn set_upstream_total_timeout_ms(timeout_ms: u64) -> u64 {
    runtime_config::set_upstream_total_timeout_ms(timeout_ms)
}

/// 函数 `current_sse_keepalive_interval_ms`
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
pub(crate) fn current_sse_keepalive_interval_ms() -> u64 {
    http_bridge::current_sse_keepalive_interval_ms()
}

/// 函数 `set_sse_keepalive_interval_ms`
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
pub(crate) fn set_sse_keepalive_interval_ms(interval_ms: u64) -> Result<u64, String> {
    http_bridge::set_sse_keepalive_interval_ms(interval_ms)
}

/// 函数 `manual_preferred_account`
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
pub(crate) fn manual_preferred_account() -> Option<String> {
    route_hint::get_manual_preferred_account()
}

/// 函数 `set_manual_preferred_account`
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
pub(crate) fn set_manual_preferred_account(account_id: &str) -> Result<(), String> {
    let id = account_id.trim();
    if id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage not initialized".to_string())?;
    let found = storage
        .find_account_by_id(id)
        .map_err(|err| err.to_string())?
        .is_some();
    if !found {
        return Err("account not found".to_string());
    }
    route_hint::set_manual_preferred_account(id)
}

/// 函数 `clear_manual_preferred_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn clear_manual_preferred_account() {
    route_hint::clear_manual_preferred_account();
}

/// 函数 `gateway_resolve_effective_upstream_base`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - api_key: 参数 api_key
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn gateway_resolve_effective_upstream_base(
    api_key: &codexmanager_core::storage::ApiKey,
) -> String {
    api_key
        .upstream_base_url
        .as_deref()
        .map(upstream::config::normalize_upstream_base_url)
        .unwrap_or_else(resolve_upstream_base_url)
}

/// 函数 `gateway_supports_official_responses_websocket`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - api_key: 参数 api_key
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn gateway_supports_official_responses_websocket(
    api_key: &codexmanager_core::storage::ApiKey,
) -> bool {
    if crate::apikey_profile::resolve_gateway_protocol_type(
        api_key.protocol_type.as_str(),
        "/v1/responses",
    ) != crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
    {
        return false;
    }
    if api_key.rotation_strategy == crate::apikey_profile::ROTATION_AGGREGATE_API
        || api_key.rotation_strategy == crate::apikey_profile::ROTATION_HYBRID
    {
        return false;
    }
    upstream::config::is_chatgpt_backend_base(&gateway_resolve_effective_upstream_base(api_key))
}

/// 函数 `gateway_collect_routed_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - storage: 参数 storage
/// - key_id: 参数 key_id
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn gateway_collect_routed_candidates(
    storage: &codexmanager_core::storage::Storage,
    key_id: &str,
    model: Option<&str>,
) -> Result<
    Vec<(
        codexmanager_core::storage::Account,
        codexmanager_core::storage::Token,
    )>,
    String,
> {
    let mut candidates = collect_gateway_candidates(storage)?;
    apply_route_strategy(&mut candidates, key_id, model);
    Ok(candidates)
}

/// 函数 `gateway_record_failover_attempt`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// 无
///
/// # 返回
/// 无
pub(crate) fn gateway_record_failover_attempt() {
    record_gateway_failover_attempt();
}

/// 函数 `gateway_mark_account_cooldown_for_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - account_id: 参数 account_id
/// - status: 参数 status
///
/// # 返回
/// 无
pub(crate) fn gateway_mark_account_cooldown_for_status(account_id: &str, status: u16) {
    mark_account_cooldown_for_status(account_id, status);
}

/// 函数 `gateway_resolve_openai_bearer_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - storage: 参数 storage
/// - account: 参数 account
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn gateway_resolve_openai_bearer_token(
    storage: &codexmanager_core::storage::Storage,
    account: &codexmanager_core::storage::Account,
    token: &mut codexmanager_core::storage::Token,
) -> Result<String, String> {
    resolve_openai_bearer_token(storage, account, token)
}

pub(crate) fn gateway_resolve_ws_prompt_cache_key(
    storage: &codexmanager_core::storage::Storage,
    api_key: &codexmanager_core::storage::ApiKey,
    incoming_headers: &IncomingHeaderSnapshot,
) -> Result<(IncomingHeaderSnapshot, Option<String>), String> {
    let local_conversation_id =
        resolve_local_conversation_id_with_sticky_fallback(incoming_headers, true);
    let conversation_binding = conversation_binding::load_conversation_binding(
        storage,
        api_key.key_hash.as_str(),
        local_conversation_id.as_deref(),
    )?;
    let incoming_headers =
        incoming_headers.with_conversation_id_override(local_conversation_id.as_deref());
    let prompt_cache_key = resolve_fallback_thread_anchor(
        &incoming_headers,
        local_conversation_id.as_deref(),
        conversation_binding.as_ref(),
    );
    Ok((incoming_headers, prompt_cache_key))
}

/// 函数 `gateway_rewrite_ws_responses_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - path: 参数 path
/// - body: 参数 body
/// - api_key: 参数 api_key
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn gateway_rewrite_ws_responses_body(
    path: &str,
    body: Vec<u8>,
    api_key: &codexmanager_core::storage::ApiKey,
    prompt_cache_key: Option<&str>,
) -> Vec<u8> {
    let normalized_model = api_key
        .model_slug
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized_reasoning = api_key
        .reasoning_effort
        .as_deref()
        .and_then(crate::reasoning_effort::normalize_reasoning_effort);
    let normalized_service_tier = api_key
        .service_tier
        .as_deref()
        .and_then(crate::apikey::service_tier::normalize_service_tier);
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        path,
        body,
        normalized_model,
        normalized_reasoning,
        normalized_service_tier,
        api_key.upstream_base_url.as_deref(),
        prompt_cache_key,
        false,
    )
}

/// 函数 `gateway_compute_upstream_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - upstream_base_url: 参数 upstream_base_url
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn gateway_compute_upstream_url(
    upstream_base_url: &str,
    path: &str,
) -> (String, Option<String>) {
    compute_upstream_url(upstream_base_url, path)
}

#[cfg(test)]
#[path = "../../tests/gateway/availability/mod.rs"]
mod availability_tests;
