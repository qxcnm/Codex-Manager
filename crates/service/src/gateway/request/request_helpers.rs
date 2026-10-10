use reqwest::header::HeaderValue;
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) const DEFAULT_MAX_TEXT_INPUT_CHARS: usize = 1_048_576;
const ENV_MAX_TEXT_INPUT_CHARS: &str = "CODEXMANAGER_MAX_TEXT_INPUT_CHARS";

/// Effective local text-input cap. Defaults to [`DEFAULT_MAX_TEXT_INPUT_CHARS`]; deployments with
/// legitimately larger text payloads (e.g. long agent transcripts and their summarization
/// requests) can raise it explicitly with `CODEXMANAGER_MAX_TEXT_INPUT_CHARS`.
pub(crate) fn max_text_input_chars() -> usize {
    std::env::var(ENV_MAX_TEXT_INPUT_CHARS)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_TEXT_INPUT_CHARS)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedRequestMetadata {
    pub(crate) model: Option<String>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) service_tier: Option<String>,
    pub(crate) is_stream: bool,
    pub(crate) stream_specified: bool,
    pub(crate) request_shape: Option<String>,
    pub(crate) has_prompt_cache_key: bool,
    pub(crate) prompt_cache_key: Option<String>,
    pub(crate) has_previous_response_id: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ServiceTierLogDiagnostic {
    pub(crate) has_field: bool,
    pub(crate) raw_value: Option<String>,
    pub(crate) normalized_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputSizeLimitError {
    pub(crate) actual_chars: usize,
    pub(crate) max_chars: usize,
}

impl InputSizeLimitError {
    pub(crate) fn message(&self) -> String {
        crate::gateway::bilingual_error(
            format!("输入超过最大长度 {} 个字符", self.max_chars),
            format!(
                "Input exceeds the maximum length of {} characters.",
                self.max_chars
            ),
        )
    }
}

/// 函数 `parse_request_metadata`
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
pub(crate) fn parse_request_metadata(body: &[u8]) -> ParsedRequestMetadata {
    let Some(value) = parse_request_json_value(body) else {
        return ParsedRequestMetadata::default();
    };
    parse_request_metadata_from_value(&value)
}

pub(crate) fn parse_request_json_value(body: &[u8]) -> Option<Value> {
    if body.is_empty() {
        return None;
    }
    serde_json::from_slice::<Value>(body).ok()
}

pub(crate) fn parse_request_metadata_from_value(value: &Value) -> ParsedRequestMetadata {
    let Some(object) = value.as_object() else {
        return ParsedRequestMetadata::default();
    };

    let model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());

    let reasoning_effort = value
        .get("reasoning")
        .and_then(|v| v.get("effort"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .or_else(|| {
            value
                .get("reasoning_effort")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string())
        });

    let request_shape = Some(summarize_request_shape_from_object(object));
    let has_prompt_cache_key = value
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|v| !v.is_empty());
    let prompt_cache_key = value
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());
    let has_previous_response_id = value
        .get("previous_response_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|v| !v.is_empty());
    let service_tier = inspect_service_tier_value(value.get("service_tier")).normalized_value;

    ParsedRequestMetadata {
        model,
        reasoning_effort,
        service_tier,
        is_stream: value
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        stream_specified: object.contains_key("stream"),
        request_shape,
        has_prompt_cache_key,
        prompt_cache_key,
        has_previous_response_id,
    }
}

pub(crate) fn validate_text_input_limit_for_path(
    path: &str,
    body: &[u8],
) -> Result<(), InputSizeLimitError> {
    if body.is_empty() || !is_text_input_limit_path(path) {
        return Ok(());
    }
    let Some(value) = parse_request_json_value(body) else {
        return Ok(());
    };
    validate_text_input_limit_for_value(path, &value)
}

pub(crate) fn validate_text_input_limit_for_value(
    path: &str,
    value: &Value,
) -> Result<(), InputSizeLimitError> {
    if !is_text_input_limit_path(path) {
        return Ok(());
    }
    let actual_chars = count_path_text_input_chars(path, &value);
    let max_chars = max_text_input_chars();
    if actual_chars > max_chars {
        return Err(InputSizeLimitError {
            actual_chars,
            max_chars,
        });
    }
    Ok(())
}

fn is_text_input_limit_path(path: &str) -> bool {
    path.starts_with("/v1/responses")
        || path.starts_with("/v1/chat/completions")
        || path.starts_with("/v1/messages")
}

fn count_path_text_input_chars(path: &str, value: &Value) -> usize {
    let Some(object) = value.as_object() else {
        return 0;
    };
    let mut total = 0;
    if let Some(instructions) = object.get("instructions") {
        total += count_stringish_chars(instructions);
    }
    if path.starts_with("/v1/responses") {
        if let Some(input) = object.get("input") {
            total += count_response_input_chars(input);
        }
        return total;
    }
    if path.starts_with("/v1/chat/completions") || path.starts_with("/v1/messages") {
        if let Some(messages) = object.get("messages") {
            total += count_message_list_chars(messages);
        }
        return total;
    }
    total
}

fn count_response_input_chars(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.iter().map(count_response_input_chars).sum(),
        Value::Object(object) => {
            let mut total = 0;
            if let Some(text) = object.get("text") {
                total += count_stringish_chars(text);
            }
            if let Some(input_text) = object.get("input_text") {
                total += count_stringish_chars(input_text);
            }
            if let Some(content) = object.get("content") {
                total += count_response_input_chars(content);
            }
            if let Some(input) = object.get("input") {
                total += count_response_input_chars(input);
            }
            total
        }
        _ => 0,
    }
}

fn count_message_list_chars(value: &Value) -> usize {
    match value {
        Value::Array(items) => items.iter().map(count_message_list_chars).sum(),
        Value::Object(object) => object
            .get("content")
            .map(count_message_content_chars)
            .unwrap_or(0),
        _ => 0,
    }
}

fn count_message_content_chars(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.iter().map(count_message_content_chars).sum(),
        Value::Object(object) => {
            let mut total = 0;
            if let Some(text) = object.get("text") {
                total += count_stringish_chars(text);
            }
            if let Some(content) = object.get("content") {
                total += count_message_content_chars(content);
            }
            total
        }
        _ => 0,
    }
}

fn count_stringish_chars(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.iter().map(count_stringish_chars).sum(),
        Value::Object(object) => {
            let mut total = 0;
            if let Some(text) = object.get("text") {
                total += count_stringish_chars(text);
            }
            if let Some(content) = object.get("content") {
                total += count_stringish_chars(content);
            }
            total
        }
        _ => 0,
    }
}

pub(crate) fn inspect_service_tier_value(value: Option<&Value>) -> ServiceTierLogDiagnostic {
    let Some(value) = value else {
        return ServiceTierLogDiagnostic::default();
    };

    let raw_value = Some(match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "<serialize_failed>".to_string()),
    });
    let normalized_value = value
        .as_str()
        .and_then(crate::apikey::service_tier::normalize_service_tier_for_log)
        .map(str::to_string);

    ServiceTierLogDiagnostic {
        has_field: true,
        raw_value,
        normalized_value,
    }
}

/// 函数 `summarize_request_shape_from_object`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - object: 参数 object
///
/// # 返回
/// 返回函数执行结果
fn summarize_request_shape_from_object(object: &serde_json::Map<String, Value>) -> String {
    let mut keys = object.keys().cloned().collect::<Vec<_>>();
    keys.sort_unstable();
    let keys_joined = if keys.is_empty() {
        "-".to_string()
    } else {
        keys.join("+")
    };

    let input_count = object
        .get("input")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let messages_count = object
        .get("messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let tools_count = object
        .get("tools")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let stream_flag = match object.get("stream").and_then(Value::as_bool) {
        Some(true) => "1",
        Some(false) => "0",
        None => "-",
    };
    let has_reasoning = if object.get("reasoning").is_some() {
        1
    } else {
        0
    };
    let has_instructions = if object
        .get("instructions")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.trim().is_empty())
    {
        1
    } else {
        0
    };

    let shape = format!(
        "k={keys_joined};i={input_count};m={messages_count};t={tools_count};s={stream_flag};r={has_reasoning};ins={has_instructions}"
    );
    let digest = Sha256::digest(shape.as_bytes());
    let fingerprint = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("fp={fingerprint};{shape}")
}

/// 函数 `should_drop_incoming_header`
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
pub(crate) fn should_drop_incoming_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    name.eq_ignore_ascii_case("Authorization")
        || name.eq_ignore_ascii_case("x-api-key")
        || name.eq_ignore_ascii_case("x-goog-api-key")
        || name.eq_ignore_ascii_case("Host")
        || name.eq_ignore_ascii_case("Content-Length")
        // 中文注释：Claude SDK/CLI 会附带 anthropic/x-stainless 指纹头；
        // 直接透传到 ChatGPT upstream 会提高 challenge 概率，这里统一剔除。
        || lower.starts_with("anthropic-")
        || lower.starts_with("x-stainless-")
        // 中文注释：resume 会携带旧会话的账号头；若不剔除会把请求强行绑定到过期/耗尽账号，导致无法切换候选账号。
        || name.eq_ignore_ascii_case("ChatGPT-Account-Id")
}

/// 函数 `should_drop_session_affinity_header`
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
pub(crate) fn should_drop_session_affinity_header(name: &str) -> bool {
    // 中文注释：session_id / turn-state 属于会话粘性信号，正常直连时应保留；
    // 仅在 failover 到其他账号时剔除，避免继续命中旧账号会话路由导致“切换无效”。
    name.eq_ignore_ascii_case("session_id") || name.eq_ignore_ascii_case("x-codex-turn-state")
}

/// 函数 `should_drop_incoming_header_for_failover`
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
pub(crate) fn should_drop_incoming_header_for_failover(name: &str) -> bool {
    should_drop_incoming_header(name) || should_drop_session_affinity_header(name)
}

/// 函数 `is_upstream_challenge_response`
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
pub(crate) fn is_upstream_challenge_response(
    status_code: u16,
    content_type: Option<&HeaderValue>,
) -> bool {
    let is_html = content_type
        .and_then(|v| v.to_str().ok())
        .map(is_html_content_type)
        .unwrap_or(false);
    // 中文注释：429 常见于业务限流/额度，不应统一映射成 Cloudflare challenge；
    // 仅在明确 HTML challenge 时按 challenge 处理，避免误报成 WAF 拦截。
    let _ = status_code;
    is_html
}

/// 函数 `is_html_content_type`
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
pub(crate) fn is_html_content_type(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("text/html")
}

/// 函数 `normalize_models_path`
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
pub(crate) fn normalize_models_path(path: &str) -> String {
    let is_models_path = path == "/v1/models" || path.starts_with("/v1/models?");
    if !is_models_path {
        return path.to_string();
    }
    path.to_string()
}

#[cfg(test)]
#[path = "tests/request_helpers_tests.rs"]
mod tests;
