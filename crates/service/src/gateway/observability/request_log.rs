use codexmanager_core::storage::{now_ts, RequestLog, RequestTokenStat, Storage};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const API_KEY_LAST_USED_TOUCH_MIN_INTERVAL_SECS: i64 = 60;
static API_KEY_LAST_USED_TOUCH_CACHE: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RequestLogUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub first_response_ms: Option<i64>,
    pub estimated_input_tokens: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedChargeUsage {
    usage_source: &'static str,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RequestLogTraceContext<'a> {
    pub trace_id: Option<&'a str>,
    pub original_path: Option<&'a str>,
    pub adapted_path: Option<&'a str>,
    pub request_type: Option<&'a str>,
    pub gateway_mode: Option<&'a str>,
    pub route_strategy: Option<&'a str>,
    pub route_source: Option<&'a str>,
    pub client_model: Option<&'a str>,
    pub model_source: Option<&'a str>,
    pub client_reasoning_effort: Option<&'a str>,
    pub reasoning_source: Option<&'a str>,
    pub service_tier: Option<&'a str>,
    pub effective_service_tier: Option<&'a str>,
    pub service_tier_source: Option<&'a str>,
    pub response_adapter: Option<super::ResponseAdapter>,
    pub aggregate_api_supplier_name: Option<&'a str>,
    pub aggregate_api_url: Option<&'a str>,
    pub attempted_aggregate_api_ids: Option<&'a [String]>,
    pub upstream_model: Option<&'a str>,
    pub actual_source_kind: Option<&'a str>,
    pub actual_source_id: Option<&'a str>,
}

/// 函数 `normalize_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_token(value: Option<i64>) -> Option<i64> {
    value.map(|v| v.max(0))
}

pub(crate) fn estimate_input_tokens_from_body(body: &[u8]) -> i64 {
    let char_count = String::from_utf8_lossy(body).chars().count();
    let estimate = char_count / 4 + usize::from(char_count % 4 != 0);
    i64::try_from(estimate.max(1)).unwrap_or(i64::MAX)
}

fn resolve_charge_usage(usage: RequestLogUsage) -> ResolvedChargeUsage {
    let has_actual_usage = usage.input_tokens.is_some()
        || usage.cached_input_tokens.is_some()
        || usage.cache_write_tokens.is_some()
        || usage.output_tokens.is_some()
        || usage.total_tokens.is_some();
    if !has_actual_usage {
        let input_tokens = usage.estimated_input_tokens.unwrap_or(1).max(1);
        return ResolvedChargeUsage {
            usage_source: "estimated",
            input_tokens,
            cached_input_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 0,
            total_tokens: input_tokens,
        };
    }
    let output_tokens = usage.output_tokens.unwrap_or(0).max(0);
    let cached_input_tokens = usage.cached_input_tokens.unwrap_or(0).max(0);
    let cache_write_tokens = usage.cache_write_tokens.unwrap_or(0).max(0);
    let input_tokens = usage
        .input_tokens
        .map(|value| value.max(0))
        .or_else(|| {
            usage
                .total_tokens
                .map(|total| total.max(0).saturating_sub(output_tokens))
        })
        .unwrap_or_else(|| cached_input_tokens.saturating_add(cache_write_tokens));
    let cached_input_tokens = cached_input_tokens.min(input_tokens);
    let cache_write_tokens =
        cache_write_tokens.min(input_tokens.saturating_sub(cached_input_tokens));
    ResolvedChargeUsage {
        usage_source: "actual",
        input_tokens,
        cached_input_tokens,
        cache_write_tokens,
        output_tokens,
        total_tokens: usage
            .total_tokens
            .map(|value| value.max(0))
            .unwrap_or_else(|| input_tokens.saturating_add(output_tokens)),
    }
}

/// 函数 `normalize_duration_ms`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_duration_ms(value: Option<u128>) -> Option<i64> {
    value.map(|duration| duration.min(i64::MAX as u128) as i64)
}

/// 函数 `is_inference_path`
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
fn is_inference_path(path: &str) -> bool {
    let path = path.split_once('?').map(|(path, _)| path).unwrap_or(path);
    path.starts_with("/v1/responses")
        || path.starts_with("/v1/chat/completions")
        || (path.starts_with("/v1/messages") && !path.starts_with("/v1/messages/count_tokens"))
}

fn normalize_log_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn resolve_service_tier_source<'a>(
    service_tier: Option<&'a str>,
    effective_service_tier: Option<&'a str>,
    explicit_source: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(source) = explicit_source
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(source);
    }
    match (service_tier, effective_service_tier) {
        (Some(client), Some(effective)) if client.eq_ignore_ascii_case(effective) => {
            Some("client_request")
        }
        (Some(_), Some(_)) => Some("gateway_override"),
        (None, Some(_)) => Some("gateway_config"),
        (Some(_), None) => Some("client_request"),
        (None, None) => Some("unset"),
    }
}

fn resolve_value_source<'a>(
    client_value: Option<&'a str>,
    effective_value: Option<&'a str>,
    explicit_source: Option<&'a str>,
) -> Option<&'a str> {
    if let Some(source) = explicit_source
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(source);
    }
    match (client_value, effective_value) {
        (Some(client), Some(effective)) if client.eq_ignore_ascii_case(effective) => {
            Some("client_request")
        }
        (Some(_), Some(_)) => Some("gateway_override"),
        (None, Some(_)) => Some("gateway_config"),
        (Some(_), None) => Some("client_request"),
        (None, None) => Some("unset"),
    }
}

fn resolve_reasoning_value_source<'a>(
    client_value: Option<&'a str>,
    effective_value: Option<&'a str>,
    explicit_source: Option<&'a str>,
) -> Option<&'a str> {
    if explicit_source.map(str::trim).is_none_or(str::is_empty)
        && crate::reasoning_effort::is_ultra_to_max_normalization(client_value, effective_value)
    {
        return Some("client_request_normalized");
    }
    resolve_value_source(client_value, effective_value, explicit_source)
}

fn resolve_route_details(
    _storage: &Storage,
    trace_context: &RequestLogTraceContext<'_>,
    account_id: Option<&str>,
    _model: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    let actual_source_kind = normalize_log_text(trace_context.actual_source_kind).or_else(|| {
        account_id
            .and_then(|value| normalize_log_text(Some(value)))
            .map(|_| "openai_account".to_string())
    });
    let actual_source_id = normalize_log_text(trace_context.actual_source_id)
        .or_else(|| normalize_log_text(account_id));
    let upstream_model = normalize_log_text(trace_context.upstream_model);
    (upstream_model, actual_source_kind, actual_source_id)
}

/// 函数 `response_adapter_label`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn response_adapter_label(value: super::ResponseAdapter) -> &'static str {
    match value {
        super::ResponseAdapter::Passthrough => "Passthrough",
        super::ResponseAdapter::AnthropicMessagesFromResponses => "AnthropicMessagesFromResponses",
        super::ResponseAdapter::ResponsesFromAnthropicMessages => "ResponsesFromAnthropicMessages",
        super::ResponseAdapter::ChatCompletionsFromResponses => "ChatCompletionsFromResponses",
        super::ResponseAdapter::CompactFromChatCompletions => "CompactFromChatCompletions",
        super::ResponseAdapter::ImagesB64JsonFromResponses => "ImagesB64JsonFromResponses",
        super::ResponseAdapter::ImagesUrlFromResponses => "ImagesUrlFromResponses",
        super::ResponseAdapter::GeminiJson => "GeminiJson",
        super::ResponseAdapter::GeminiSse => "GeminiSse",
        super::ResponseAdapter::GeminiCliJson => "GeminiCliJson",
        super::ResponseAdapter::GeminiCliSse => "GeminiCliSse",
    }
}

/// 函数 `write_request_log`
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
pub(crate) fn write_request_log(
    storage: &Storage,
    trace_context: RequestLogTraceContext<'_>,
    key_id: Option<&str>,
    account_id: Option<&str>,
    request_path: &str,
    method: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_url: Option<&str>,
    status_code: Option<u16>,
    usage: RequestLogUsage,
    error: Option<&str>,
    duration_ms: Option<u128>,
) {
    write_request_log_with_attempts(
        storage,
        trace_context,
        key_id,
        account_id,
        request_path,
        method,
        model,
        reasoning_effort,
        upstream_url,
        status_code,
        usage,
        error,
        duration_ms,
        None,
    );
}

/// 函数 `write_request_log_with_attempts`
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_request_log_with_attempts(
    storage: &Storage,
    trace_context: RequestLogTraceContext<'_>,
    key_id: Option<&str>,
    account_id: Option<&str>,
    request_path: &str,
    method: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_url: Option<&str>,
    status_code: Option<u16>,
    usage: RequestLogUsage,
    error: Option<&str>,
    duration_ms: Option<u128>,
    attempted_account_ids: Option<&[String]>,
) {
    let original_path = trace_context.original_path.unwrap_or(request_path);
    let adapted_path = trace_context.adapted_path.unwrap_or(request_path);
    let initial_account_id = attempted_account_ids
        .and_then(|items| items.first())
        .map(String::as_str);
    let attempted_account_ids_json = attempted_account_ids
        .filter(|items| !items.is_empty())
        .and_then(|items| serde_json::to_string(items).ok());
    let initial_aggregate_api_id = trace_context
        .attempted_aggregate_api_ids
        .and_then(|items| items.first())
        .map(String::as_str);
    let attempted_aggregate_api_ids_json = trace_context
        .attempted_aggregate_api_ids
        .filter(|items| !items.is_empty())
        .and_then(|items| serde_json::to_string(items).ok());
    let raw_input_tokens = normalize_token(usage.input_tokens);
    let raw_cached_input_tokens = normalize_token(usage.cached_input_tokens);
    let raw_output_tokens = normalize_token(usage.output_tokens);
    let raw_total_tokens = normalize_token(usage.total_tokens);
    let charge_usage = resolve_charge_usage(usage);
    let inference_path = is_inference_path(request_path);
    let use_charge_usage =
        inference_path && (charge_usage.usage_source == "actual" || upstream_url.is_some());
    let input_tokens = use_charge_usage
        .then_some(charge_usage.input_tokens)
        .or(raw_input_tokens);
    let cached_input_tokens = use_charge_usage
        .then_some(charge_usage.cached_input_tokens)
        .or(raw_cached_input_tokens);
    let output_tokens = use_charge_usage
        .then_some(charge_usage.output_tokens)
        .or(raw_output_tokens);
    let total_tokens = use_charge_usage
        .then_some(charge_usage.total_tokens)
        .or(raw_total_tokens);
    let reasoning_output_tokens = normalize_token(usage.reasoning_output_tokens);
    let duration_ms = normalize_duration_ms(duration_ms);
    let first_response_ms = usage.first_response_ms.map(|value| value.max(0));
    let created_at = now_ts();
    let request_type = trace_context
        .request_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    let service_tier = trace_context
        .service_tier
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let effective_service_tier = trace_context
        .effective_service_tier
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let service_tier_source = resolve_service_tier_source(
        service_tier,
        effective_service_tier,
        trace_context.service_tier_source,
    );
    let client_model = trace_context
        .client_model
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model_source = resolve_value_source(client_model, model, trace_context.model_source);
    let client_reasoning_effort = trace_context
        .client_reasoning_effort
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let reasoning_source = resolve_reasoning_value_source(
        client_reasoning_effort,
        reasoning_effort,
        trace_context.reasoning_source,
    );
    let (upstream_model, actual_source_kind, actual_source_id) =
        resolve_route_details(storage, &trace_context, account_id, model);
    super::trace_log::log_failed_request(super::trace_log::FailedRequestLog {
        ts: created_at,
        trace_id: trace_context.trace_id,
        key_id,
        account_id,
        method,
        request_path,
        original_path: Some(original_path),
        adapted_path: Some(adapted_path),
        request_type: Some(request_type),
        model,
        reasoning_effort,
        service_tier,
        upstream_url,
        status_code,
        error,
        duration_ms,
    });
    let success = status_code
        .map(|status| (200..300).contains(&status))
        .unwrap_or(false);
    if inference_path && upstream_url.is_some() && charge_usage.usage_source == "estimated" {
        log::warn!(
            "event=gateway_token_usage_estimated path={} status={} account_id={} key_id={} model={} input_tokens={}",
            request_path,
            status_code.unwrap_or(0),
            account_id.unwrap_or("-"),
            key_id.unwrap_or("-"),
            model.unwrap_or("-"),
            charge_usage.input_tokens,
        );
    }
    // 记录请求最终结果（而非内部重试明细），保证 UI 一次请求只展示一条记录。
    let (request_log_id, token_stat_error) = match crate::requestlog::seaorm::insert_with_usage(
        storage,
        &RequestLog {
            trace_id: trace_context.trace_id.map(|v| v.to_string()),
            key_id: key_id.map(|v| v.to_string()),
            account_id: account_id.map(|v| v.to_string()),
            initial_account_id: initial_account_id.map(str::to_string),
            attempted_account_ids_json,
            initial_aggregate_api_id: initial_aggregate_api_id.map(str::to_string),
            attempted_aggregate_api_ids_json,
            request_path: request_path.to_string(),
            original_path: Some(original_path.to_string()),
            adapted_path: Some(adapted_path.to_string()),
            method: method.to_string(),
            request_type: Some(request_type.to_string()),
            gateway_mode: trace_context.gateway_mode.map(str::to_string),
            route_strategy: normalize_log_text(trace_context.route_strategy),
            route_source: normalize_log_text(trace_context.route_source),
            transparent_mode: None,
            enhanced_mode: None,
            client_model: client_model.map(str::to_string),
            model: model.map(|v| v.to_string()),
            model_source: model_source.map(str::to_string),
            upstream_model,
            actual_source_kind: actual_source_kind.clone(),
            actual_source_id: actual_source_id.clone(),
            client_reasoning_effort: client_reasoning_effort.map(str::to_string),
            reasoning_effort: reasoning_effort.map(|v| v.to_string()),
            reasoning_source: reasoning_source.map(str::to_string),
            service_tier: service_tier.map(str::to_string),
            effective_service_tier: effective_service_tier.map(str::to_string),
            service_tier_source: service_tier_source.map(str::to_string),
            response_adapter: trace_context
                .response_adapter
                .map(response_adapter_label)
                .map(str::to_string),
            upstream_url: upstream_url.map(|v| v.to_string()),
            aggregate_api_supplier_name: trace_context
                .aggregate_api_supplier_name
                .map(str::to_string),
            aggregate_api_url: trace_context.aggregate_api_url.map(str::to_string),
            status_code: status_code.map(|v| i64::from(v)),
            duration_ms,
            first_response_ms,
            input_tokens: None,
            cached_input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            reasoning_output_tokens: None,
            estimated_cost_usd: None,
            error: error.map(|v| v.to_string()),
            created_at,
        },
        &RequestTokenStat {
            request_log_id: 0,
            key_id: key_id.map(|v| v.to_string()),
            account_id: account_id.map(|v| v.to_string()),
            model: model.map(|v| v.to_string()),
            actual_source_kind,
            actual_source_id,
            input_tokens,
            cached_input_tokens,
            output_tokens,
            total_tokens,
            reasoning_output_tokens,
            estimated_cost_usd: None,
            created_at,
        },
    ) {
        Ok(result) => result,
        Err(err) => {
            let err_text = err.to_string();
            super::metrics::record_db_error(err_text.as_str());
            log::error!(
                "event=gateway_request_log_insert_failed path={} status={} account_id={} key_id={} err={}",
                request_path,
                status_code.unwrap_or(0),
                account_id.unwrap_or("-"),
                key_id.unwrap_or("-"),
                err_text
            );
            return;
        }
    };

    if let Some(err) = token_stat_error {
        let err_text = err.to_string();
        super::metrics::record_db_error(err_text.as_str());
        log::error!(
            "event=gateway_request_token_stat_insert_failed path={} status={} account_id={} key_id={} request_log_id={} err={}",
            request_path,
            status_code.unwrap_or(0),
            account_id.unwrap_or("-"),
            key_id.unwrap_or("-"),
            request_log_id,
            err_text
        );
    }

    if success {
        touch_api_key_last_used_after_success(storage, key_id, created_at);
    }

    if inference_path && upstream_url.is_some() {
        if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
            let raw_usage_json = serde_json::to_string(&serde_json::json!({
                "model": model,
                "usageSource": charge_usage.usage_source,
                "inputTokens": charge_usage.input_tokens,
                "cachedInputTokens": charge_usage.cached_input_tokens,
                "cacheWriteTokens": charge_usage.cache_write_tokens,
                "outputTokens": charge_usage.output_tokens,
                "totalTokens": charge_usage.total_tokens,
                "reasoningOutputTokens": reasoning_output_tokens,
            }))
            .ok();
            let charge_wallet = success || status_code == Some(499);
            if let Err(err) = crate::auth::app_manager::record_request_charge_v2(
                storage,
                key_id,
                request_log_id,
                model,
                effective_service_tier.or(service_tier),
                charge_usage.usage_source,
                charge_usage.input_tokens,
                charge_usage.cached_input_tokens,
                charge_usage.cache_write_tokens,
                charge_usage.output_tokens,
                raw_usage_json,
                charge_wallet,
            ) {
                log::warn!(
                    "event=model_catalog_v2_charge_failed key_id={} request_log_id={} usage_source={} charge_wallet={} err={}",
                    key_id.unwrap_or("-"),
                    request_log_id,
                    charge_usage.usage_source,
                    charge_wallet,
                    err
                );
            }
        } else {
            log::warn!(
                "event=model_catalog_v2_charge_skipped request_log_id={} reason=model_missing",
                request_log_id
            );
        }
    }

    if let Err(err) = crate::requestlog::seaorm::maintain(storage, created_at) {
        let err_text = err.to_string();
        super::metrics::record_db_error(err_text.as_str());
        log::warn!(
            "event=gateway_observability_maintenance_failed request_log_id={} err={}",
            request_log_id,
            err_text
        );
    }
}

fn touch_api_key_last_used_after_success(storage: &Storage, key_id: Option<&str>, now: i64) {
    let Some(key_id) = key_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if !should_touch_api_key_last_used(key_id, now) {
        return;
    }
    let result = if crate::storage_helpers::seaorm_enabled() {
        let key_id = key_id.to_owned();
        crate::storage_helpers::seaorm_block_on(move |storage| async move {
            codexmanager_storage_seaorm::ApiKeysRepository::touch_last_used(
                storage.connection(),
                &key_id,
                now,
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
        })
    } else {
        storage
            .update_api_key_last_used_at_by_id(key_id, now)
            .map_err(|error| error.to_string())
    };
    if let Err(err) = result {
        log::warn!(
            "event=api_key_last_used_touch_failed key_id={} err={}",
            key_id,
            err
        );
    }
}

fn should_touch_api_key_last_used(key_id: &str, now: i64) -> bool {
    let cache = API_KEY_LAST_USED_TOUCH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return true;
    };
    let previous = cache.get(key_id).copied().unwrap_or(0);
    if now.saturating_sub(previous) < API_KEY_LAST_USED_TOUCH_MIN_INTERVAL_SECS {
        return false;
    }
    cache.insert(key_id.to_string(), now);
    true
}

#[cfg(test)]
#[path = "tests/request_log_tests.rs"]
mod tests;
