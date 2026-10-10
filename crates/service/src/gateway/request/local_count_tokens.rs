use crate::apikey_profile::{
    is_gemini_count_tokens_request_path, PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_GEMINI_NATIVE,
};
use serde_json::{json, Value};

/// 函数 `accumulate_text_len`
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
fn accumulate_text_len(value: &Value) -> usize {
    match value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.iter().map(accumulate_text_len).sum(),
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return text.chars().count();
            }
            if let Some(content) = map.get("content") {
                return accumulate_text_len(content);
            }
            if let Some(input) = map.get("input") {
                return accumulate_text_len(input);
            }
            map.values().map(accumulate_text_len).sum()
        }
        _ => 0,
    }
}

/// 函数 `estimate_input_tokens_from_anthropic_messages`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn estimate_input_tokens_from_anthropic_messages(body: &[u8]) -> Result<u64, String> {
    let payload: Value = serde_json::from_slice(body).map_err(|_| {
        crate::gateway::bilingual_error("Claude 请求 JSON 无效", "invalid claude request json")
    })?;
    let Some(object) = payload.as_object() else {
        return Err(crate::gateway::bilingual_error(
            "Claude 请求体必须是对象",
            "claude request body must be an object",
        ));
    };

    let mut char_count = 0usize;
    if let Some(system) = object.get("system") {
        char_count += accumulate_text_len(system);
    }
    if let Some(messages) = object.get("messages").and_then(Value::as_array) {
        for message in messages {
            if let Some(content) = message.get("content") {
                char_count += accumulate_text_len(content);
            }
        }
    }

    // 中文注释：count_tokens 仅用于本地预算估计，采用稳定的轻量估算（约 4 chars/token）。
    let estimated = ((char_count as u64) / 4).max(1);
    Ok(estimated)
}

fn estimate_input_tokens_from_gemini_request(body: &[u8]) -> Result<u64, String> {
    let payload: Value = serde_json::from_slice(body).map_err(|_| {
        crate::gateway::bilingual_error("Gemini 请求 JSON 无效", "invalid gemini request json")
    })?;
    let Some(root) = payload.as_object() else {
        return Err(crate::gateway::bilingual_error(
            "Gemini 请求体必须是对象",
            "gemini request body must be an object",
        ));
    };
    let object = root
        .get("request")
        .and_then(Value::as_object)
        .unwrap_or(root);

    let mut char_count = 0usize;
    if let Some(system_instruction) = object
        .get("systemInstruction")
        .or_else(|| object.get("system_instruction"))
    {
        char_count += accumulate_text_len(system_instruction);
    }
    if let Some(contents) = object.get("contents").and_then(Value::as_array) {
        for content in contents {
            if let Some(parts) = content.get("parts") {
                char_count += accumulate_text_len(parts);
            }
        }
    }

    Ok(((char_count as u64) / 4).max(1))
}

fn is_anthropic_count_tokens_request_path(path: &str) -> bool {
    path == "/v1/messages/count_tokens" || path.starts_with("/v1/messages/count_tokens?")
}

/// 函数 `maybe_respond_local_count_tokens`
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
pub(super) fn maybe_respond_local_count_tokens(
    request: crate::http::gateway_request::GatewayRequest,
    trace_id: &str,
    key_id: &str,
    protocol_type: &str,
    original_path: &str,
    path: &str,
    _response_adapter: super::ResponseAdapter,
    request_method: &str,
    body: &[u8],
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    storage: &codexmanager_core::storage::Storage,
) -> Result<Option<crate::http::gateway_request::GatewayRequest>, String> {
    let is_anthropic_count_tokens = protocol_type == PROTOCOL_ANTHROPIC_NATIVE
        && request_method.eq_ignore_ascii_case("POST")
        && (is_anthropic_count_tokens_request_path(original_path)
            || is_anthropic_count_tokens_request_path(path));
    let is_gemini_count_tokens = protocol_type == PROTOCOL_GEMINI_NATIVE
        && request_method.eq_ignore_ascii_case("POST")
        && (is_gemini_count_tokens_request_path(original_path)
            || is_gemini_count_tokens_request_path(path));
    if !is_anthropic_count_tokens && !is_gemini_count_tokens {
        return Ok(Some(request));
    }
    let local_path = if is_anthropic_count_tokens_request_path(original_path)
        || is_gemini_count_tokens_request_path(original_path)
    {
        original_path
    } else {
        path
    };
    let context = super::local_response::LocalResponseContext {
        trace_id,
        key_id,
        protocol_type,
        original_path,
        path: local_path,
        response_adapter: super::ResponseAdapter::Passthrough,
        request_method,
        model_for_log,
        reasoning_for_log,
        storage,
    };

    let estimate_result = if is_gemini_count_tokens {
        estimate_input_tokens_from_gemini_request(body)
    } else {
        estimate_input_tokens_from_anthropic_messages(body)
    };
    match estimate_result {
        Ok(input_tokens) => {
            let output = if is_gemini_count_tokens {
                json!({ "totalTokens": input_tokens }).to_string()
            } else {
                json!({ "input_tokens": input_tokens }).to_string()
            };
            super::local_response::respond_local_json(
                request,
                &context,
                output,
                super::request_log::RequestLogUsage {
                    input_tokens: Some(input_tokens.min(i64::MAX as u64) as i64),
                    cached_input_tokens: Some(0),
                    cache_write_tokens: Some(0),
                    output_tokens: Some(0),
                    total_tokens: Some(input_tokens.min(i64::MAX as u64) as i64),
                    reasoning_output_tokens: Some(0),
                    first_response_ms: None,
                    estimated_input_tokens: None,
                },
            )?;
            Ok(None)
        }
        Err(err) => {
            super::local_response::respond_local_terminal_error(request, &context, 400, err)?;
            Ok(None)
        }
    }
}

#[cfg(test)]
#[path = "tests/local_count_tokens_tests.rs"]
mod tests;
