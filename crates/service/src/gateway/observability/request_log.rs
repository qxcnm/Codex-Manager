use codexmanager_core::storage::{now_ts, RequestLog, RequestTokenStat, Storage};

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RequestLogUsage {
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub first_response_ms: Option<i64>,
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

#[allow(dead_code)]
const MODEL_PRICE_PER_1K_TOKENS: &[(&str, f64, f64, f64)] = &[
    // OpenAI 官方价格（单位：USD / 1K tokens）。按模型前缀匹配，越具体越靠前。
    // GPT-5.5 官方价格。
    ("gpt-5.5-pro", 0.03, 0.03, 0.18),
    ("gpt-5.5", 0.005, 0.0005, 0.03),
    // GPT-5.4 mini 官方价格。
    ("gpt-5.4-mini", 0.00075, 0.000075, 0.0045),
    ("gpt-5.4-nano", 0.0002, 0.00002, 0.00125),
    // GPT-5.4 pro 官方未提供 cached input 单价，这里按普通输入价计算，避免低估费用。
    ("gpt-5.4-pro", 0.03, 0.03, 0.18),
    ("gpt-5.4", 0.0025, 0.00025, 0.015),
    // gpt-5.3-codex 暂按官方当前最接近的 gpt-5.2-codex 价格估算。
    ("gpt-5.3-codex", 0.00175, 0.000175, 0.014),
    // GPT-5.2 / GPT-5.2 pro 官方价格。
    ("gpt-5.2-pro", 0.021, 0.021, 0.168),
    ("gpt-5.2-chat-latest", 0.00175, 0.000175, 0.014),
    ("gpt-5.2-codex", 0.00175, 0.000175, 0.014),
    ("gpt-5.2", 0.00175, 0.000175, 0.014),
    // GPT-5.1 Codex mini / gpt-5-codex-mini 同价。
    ("gpt-5.1-codex-mini", 0.00025, 0.000025, 0.002),
    ("gpt-5-codex-mini", 0.00025, 0.000025, 0.002),
    ("gpt-5.1-codex-max", 0.00125, 0.000125, 0.01),
    ("gpt-5.1-chat-latest", 0.00125, 0.000125, 0.01),
    ("gpt-5.1-codex", 0.00125, 0.000125, 0.01),
    ("gpt-5.1", 0.00125, 0.000125, 0.01),
    ("gpt-5-mini", 0.00025, 0.000025, 0.002),
    ("gpt-5-nano", 0.00005, 0.000005, 0.0004),
    // gpt-5-pro 官方未提供 cached input 单价，这里按普通输入价计算，避免低估费用。
    ("gpt-5-pro", 0.015, 0.015, 0.12),
    ("gpt-5-chat-latest", 0.00125, 0.000125, 0.01),
    ("gpt-5-codex", 0.00125, 0.000125, 0.01),
    ("gpt-5", 0.00125, 0.000125, 0.01),
    ("gpt-4.1-nano", 0.0001, 0.000025, 0.0004),
    ("gpt-4.1-mini", 0.0004, 0.0001, 0.0016),
    ("gpt-4.1", 0.002, 0.0005, 0.008),
    ("gpt-4o-mini", 0.00015, 0.000075, 0.0006),
    // 2024-05-13 版本没有公开 cached input 单价，这里按输入同价处理，避免低估费用。
    ("gpt-4o-2024-05-13", 0.005, 0.005, 0.015),
    ("gpt-4o", 0.0025, 0.00125, 0.01),
    ("gpt-realtime-mini", 0.0006, 0.00006, 0.0024),
    ("gpt-realtime", 0.004, 0.0004, 0.016),
    ("gpt-4o-mini-realtime-preview", 0.0006, 0.0003, 0.0024),
    ("gpt-4o-realtime-preview", 0.005, 0.0025, 0.02),
    // 音频模型官方未提供 cached input 单价，这里按普通输入价计算，避免低估费用。
    ("gpt-audio-mini", 0.0006, 0.0006, 0.0024),
    ("gpt-audio", 0.0025, 0.0025, 0.01),
    ("gpt-4o-mini-audio-preview", 0.00015, 0.00015, 0.0006),
    ("gpt-4o-audio-preview", 0.0025, 0.0025, 0.01),
    // 图片模型官方区分 Text / Image modality。当前日志 usage 未携带 modality 分桶，
    // 因此统一按 Image token 价格估算，避免把混合图片请求低估。
    ("gpt-image-2", 0.008, 0.002, 0.03),
    ("gpt-image-1.5", 0.008, 0.002, 0.032),
    ("gpt-image-1-mini", 0.0025, 0.00025, 0.008),
    // 兼容旧模型：缓存输入按输入同价处理，保持历史口径稳定。
    ("gpt-4", 0.03, 0.03, 0.06),
    // o3 / o3-mini / o3-pro / o3-deep-research 官方价格。
    ("o4-mini-deep-research", 0.002, 0.0005, 0.008),
    ("o4-mini", 0.0011, 0.000275, 0.0044),
    ("o3-deep-research", 0.01, 0.0025, 0.04),
    ("o3-pro", 0.02, 0.02, 0.08),
    ("o3-mini", 0.0011, 0.00055, 0.0044),
    ("o3", 0.002, 0.0005, 0.008),
    // o1 / o1-pro 官方未提供 cached input 单价，这里按普通输入价计算，避免低估费用。
    ("o1-pro", 0.15, 0.15, 0.6),
    ("o1-mini", 0.0011, 0.00055, 0.0044),
    ("o1", 0.015, 0.0075, 0.06),
    ("claude-3-7", 0.003, 0.003, 0.015),
    ("claude-3-5", 0.003, 0.003, 0.015),
    ("claude-3", 0.003, 0.003, 0.015),
];

/// 函数 `resolve_model_price_per_1k`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - normalized: 参数 normalized
/// - input_tokens_total: 参数 input_tokens_total
///
/// # 返回
/// 返回函数执行结果
#[allow(dead_code)]
fn resolve_model_price_per_1k(
    normalized: &str,
    input_tokens_total: i64,
) -> Option<(f64, f64, f64)> {
    // OpenAI 官方定价：gpt-5.5 / gpt-5.4 在输入达到 270K 时切换到更高档位。
    if normalized.starts_with("gpt-5.5-pro") {
        if input_tokens_total >= 270_000 {
            return Some((0.06, 0.06, 0.27));
        }
        return Some((0.03, 0.03, 0.18));
    }
    if normalized == "gpt-5.5" {
        if input_tokens_total >= 270_000 {
            return Some((0.01, 0.001, 0.045));
        }
        return Some((0.005, 0.0005, 0.03));
    }
    if normalized.starts_with("gpt-5.4-pro") {
        if input_tokens_total >= 270_000 {
            return Some((0.06, 0.06, 0.27));
        }
        return Some((0.03, 0.03, 0.18));
    }
    if normalized == "gpt-5.4" {
        if input_tokens_total >= 270_000 {
            return Some((0.005, 0.0005, 0.0225));
        }
        return Some((0.0025, 0.00025, 0.015));
    }
    MODEL_PRICE_PER_1K_TOKENS
        .iter()
        .find(|(prefix, _, _, _)| normalized.starts_with(prefix))
        .map(|(_, input, cached_input, output)| (*input, *cached_input, *output))
}

/// 函数 `estimate_cost_usd`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - model: 参数 model
/// - input_tokens: 参数 input_tokens
/// - cached_input_tokens: 参数 cached_input_tokens
/// - output_tokens: 参数 output_tokens
///
/// # 返回
/// 返回函数执行结果
#[allow(dead_code)]
fn estimate_cost_usd(
    model: Option<&str>,
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    output_tokens: Option<i64>,
) -> f64 {
    let normalized = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let Some(normalized) = normalized else {
        return 0.0;
    };
    let input_tokens_total = input_tokens.unwrap_or(0).max(0);
    let Some((in_per_1k, cached_in_per_1k, out_per_1k)) =
        resolve_model_price_per_1k(&normalized, input_tokens_total)
    else {
        return 0.0;
    };
    let in_tokens_total = input_tokens_total as f64;
    let cached_in_tokens = (cached_input_tokens.unwrap_or(0).max(0) as f64).min(in_tokens_total);
    let billable_in_tokens = (in_tokens_total - cached_in_tokens).max(0.0);
    let out_tokens = output_tokens.unwrap_or(0).max(0) as f64;
    (billable_in_tokens / 1000.0) * in_per_1k
        + (cached_in_tokens / 1000.0) * cached_in_per_1k
        + (out_tokens / 1000.0) * out_per_1k
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
    path.starts_with("/v1/responses")
        || path.starts_with("/v1/chat/completions")
        || path.starts_with("/v1/messages")
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
fn resolve_route_details(
    storage: &Storage,
    trace_context: &RequestLogTraceContext<'_>,
    account_id: Option<&str>,
    model: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    let actual_source_kind = normalize_log_text(trace_context.actual_source_kind).or_else(|| {
        account_id
            .and_then(|value| normalize_log_text(Some(value)))
            .map(|_| "openai_account".to_string())
    });
    let actual_source_id = normalize_log_text(trace_context.actual_source_id)
        .or_else(|| normalize_log_text(account_id));
    let upstream_model = normalize_log_text(trace_context.upstream_model).or_else(|| {
        let platform_model = model.map(str::trim).filter(|value| !value.is_empty())?;
        let source_kind = actual_source_kind.as_deref()?;
        let source_id = actual_source_id.as_deref()?;
        storage
            .find_enabled_model_source_mapping(platform_model, source_kind, source_id)
            .ok()
            .flatten()
            .map(|mapping| mapping.upstream_model)
    });
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
    let input_tokens = normalize_token(usage.input_tokens);
    let cached_input_tokens = normalize_token(usage.cached_input_tokens);
    let output_tokens = normalize_token(usage.output_tokens);
    let total_tokens = normalize_token(usage.total_tokens);
    let reasoning_output_tokens = normalize_token(usage.reasoning_output_tokens);
    let duration_ms = normalize_duration_ms(duration_ms);
    let first_response_ms = usage.first_response_ms.map(|value| value.max(0));
    let created_at = now_ts();
    let estimated_cost_usd = crate::quota::model_pricing::estimate_cost_usd_for_log(
        storage,
        model,
        input_tokens,
        cached_input_tokens,
        output_tokens,
    );
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
    let reasoning_source = resolve_value_source(
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
    let input_zero_or_missing = input_tokens.unwrap_or(0) == 0;
    let cached_zero_or_missing = cached_input_tokens.unwrap_or(0) == 0;
    let output_zero_or_missing = output_tokens.unwrap_or(0) == 0;
    let total_zero_or_missing = total_tokens.unwrap_or(0) == 0;
    let reasoning_zero_or_missing = reasoning_output_tokens.unwrap_or(0) == 0;
    if success
        && is_inference_path(request_path)
        && input_zero_or_missing
        && cached_zero_or_missing
        && output_zero_or_missing
        && total_zero_or_missing
        && reasoning_zero_or_missing
    {
        log::warn!(
            "event=gateway_token_usage_missing path={} status={} account_id={} key_id={} model={}",
            request_path,
            status_code.unwrap_or(0),
            account_id.unwrap_or("-"),
            key_id.unwrap_or("-"),
            model.unwrap_or("-"),
        );
    }
    // 记录请求最终结果（而非内部重试明细），保证 UI 一次请求只展示一条记录。
    let (request_log_id, token_stat_error) = match storage.insert_request_log_with_token_stat(
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
            estimated_cost_usd: Some(estimated_cost_usd),
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
        let raw_usage_json = serde_json::to_string(&serde_json::json!({
            "model": model,
            "inputTokens": input_tokens,
            "cachedInputTokens": cached_input_tokens,
            "outputTokens": output_tokens,
            "totalTokens": total_tokens,
            "reasoningOutputTokens": reasoning_output_tokens,
            "estimatedCostUsd": estimated_cost_usd,
        }))
        .ok();
        if let Err(err) = crate::wallet_charge_for_request(
            storage,
            key_id,
            request_log_id,
            estimated_cost_usd,
            model,
            effective_service_tier.or(service_tier),
            raw_usage_json,
        ) {
            log::warn!(
                "event=app_wallet_charge_failed key_id={} request_log_id={} estimated_cost_usd={} err={}",
                key_id.unwrap_or("-"),
                request_log_id,
                estimated_cost_usd,
                err
            );
        }
    }

    if let Err(err) = storage.maybe_run_observability_maintenance(created_at) {
        let err_text = err.to_string();
        super::metrics::record_db_error(err_text.as_str());
        log::warn!(
            "event=gateway_observability_maintenance_failed request_log_id={} err={}",
            request_log_id,
            err_text
        );
    }
}

#[cfg(test)]
#[path = "tests/request_log_tests.rs"]
mod tests;
