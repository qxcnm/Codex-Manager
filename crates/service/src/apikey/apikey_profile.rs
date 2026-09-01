pub(crate) const CLIENT_CODEX: &str = "codex";
pub(crate) const PROTOCOL_OPENAI_COMPAT: &str = "openai_compat";
pub(crate) const PROTOCOL_ANTHROPIC_NATIVE: &str = "anthropic_native";
pub(crate) const PROTOCOL_GEMINI_NATIVE: &str = "gemini_native";
pub(crate) const AUTH_BEARER: &str = "authorization_bearer";
pub(crate) const AUTH_X_API_KEY: &str = "x_api_key";
pub(crate) const ROTATION_ACCOUNT: &str = "account_rotation";
pub(crate) const ROTATION_AGGREGATE_API: &str = "aggregate_api_rotation";
pub(crate) const ROTATION_HYBRID: &str = "hybrid_rotation";
pub(crate) const ROTATION_HYBRID_AGGREGATE_FIRST: &str = "hybrid_aggregate_first_rotation";

/// 函数 `normalize_key`
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
fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

/// 函数 `is_anthropic_request_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn is_anthropic_request_path(path: &str) -> bool {
    path == "/v1/messages" || path.starts_with("/v1/messages/") || path.starts_with("/v1/messages?")
}

fn normalized_request_path(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

fn is_gemini_internal_generate_content_request_path(path: &str) -> bool {
    matches!(
        normalized_request_path(path),
        "/v1internal:generateContent" | "/v1internal:streamGenerateContent"
    )
}

pub(crate) fn is_gemini_generate_content_request_path(path: &str) -> bool {
    let normalized = normalized_request_path(path);
    if is_gemini_internal_generate_content_request_path(normalized) {
        return true;
    }
    ["/v1/models/", "/v1beta/models/", "/v1alpha/models/"]
        .iter()
        .any(|prefix| {
            normalized.starts_with(prefix)
                && (normalized.contains(":generateContent")
                    || normalized.contains(":streamGenerateContent"))
        })
}

pub(crate) fn is_gemini_count_tokens_request_path(path: &str) -> bool {
    let normalized = normalized_request_path(path);
    if normalized == "/v1internal:countTokens" {
        return true;
    }
    ["/v1/models/", "/v1beta/models/", "/v1alpha/models/"]
        .iter()
        .any(|prefix| normalized.starts_with(prefix) && normalized.contains(":countTokens"))
}

pub(crate) fn is_gemini_request_path(path: &str) -> bool {
    is_gemini_generate_content_request_path(path) || is_gemini_count_tokens_request_path(path)
}

/// 函数 `resolve_gateway_protocol_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - protocol_type: 参数 protocol_type
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_gateway_protocol_type(protocol_type: &str, path: &str) -> &'static str {
    match normalize_key(protocol_type).as_str() {
        _ if is_gemini_request_path(path) => PROTOCOL_GEMINI_NATIVE,
        // 中文注释：平台 Key 对 Codex / Claude Code 默认按路径通配；
        // `/v1/messages*` 走 Claude 语义，Gemini 原生路径走 Gemini 语义，其余标准路径走 OpenAI/Codex 语义。
        _ if is_anthropic_request_path(path) => PROTOCOL_ANTHROPIC_NATIVE,
        _ => PROTOCOL_OPENAI_COMPAT,
    }
}

/// 函数 `normalize_protocol_type`
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
pub(crate) fn normalize_protocol_type(value: Option<String>) -> Result<String, String> {
    match value {
        Some(raw) => match normalize_key(&raw).as_str() {
            "openai" | "openai_compat" => Ok(PROTOCOL_OPENAI_COMPAT.to_string()),
            "anthropic" | "anthropic_native" => Ok(PROTOCOL_ANTHROPIC_NATIVE.to_string()),
            "gemini" | "gemini_native" => Ok(PROTOCOL_GEMINI_NATIVE.to_string()),
            other => Err(format!("unsupported protocol type: {other}")),
        },
        None => Ok(PROTOCOL_OPENAI_COMPAT.to_string()),
    }
}

/// 函数 `profile_from_protocol`
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
pub(crate) fn profile_from_protocol(
    protocol_type: &str,
) -> Result<(String, String, String), String> {
    let protocol = normalize_protocol_type(Some(protocol_type.to_string()))?;
    let auth_scheme = if protocol == PROTOCOL_ANTHROPIC_NATIVE {
        AUTH_X_API_KEY.to_string()
    } else {
        AUTH_BEARER.to_string()
    };
    Ok((CLIENT_CODEX.to_string(), protocol, auth_scheme))
}

/// 函数 `normalize_rotation_strategy`
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
pub(crate) fn normalize_rotation_strategy(value: Option<String>) -> Result<String, String> {
    match value {
        Some(raw) => match normalize_key(&raw).as_str() {
            "account" | "account_rotation" | "account_rotate" | "账号轮转" => {
                Ok(ROTATION_ACCOUNT.to_string())
            }
            "aggregateapi"
            | "aggregate_api"
            | "aggregate_api_rotation"
            | "aggregateapirotation"
            | "聚合api"
            | "聚合api轮转" => Ok(ROTATION_AGGREGATE_API.to_string()),
            "hybrid"
            | "mixed"
            | "hybrid_rotation"
            | "mixed_rotation"
            | "混合轮转"
            | "账号优先聚合兜底" => Ok(ROTATION_HYBRID.to_string()),
            "hybrid_aggregate_first"
            | "hybrid_aggregate_first_rotation"
            | "mixed_aggregate_first"
            | "aggregate_first"
            | "聚合优先"
            | "聚合优先账号兜底" => Ok(ROTATION_HYBRID_AGGREGATE_FIRST.to_string()),
            other => Err(format!("unsupported rotation strategy: {other}")),
        },
        None => Ok(ROTATION_ACCOUNT.to_string()),
    }
}

/// 函数 `normalize_upstream_base_url`
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
pub(crate) fn normalize_upstream_base_url(value: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed =
        reqwest::Url::parse(trimmed.as_str()).map_err(|_| "invalid upstreamBaseUrl".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("invalid upstreamBaseUrl scheme".to_string());
    }
    Ok(Some(trimmed))
}

/// 函数 `normalize_static_headers_json`
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
pub(crate) fn normalize_static_headers_json(
    value: Option<String>,
) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|_| "invalid staticHeadersJson: must be a JSON object".to_string())?;
    let obj = parsed
        .as_object()
        .ok_or_else(|| "invalid staticHeadersJson: must be a JSON object".to_string())?;
    for (name, value) in obj {
        if name.trim().is_empty() {
            return Err("invalid staticHeadersJson: header name is empty".to_string());
        }
        if !value.is_string() {
            return Err(format!(
                "invalid staticHeadersJson: header {name} value must be string"
            ));
        }
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
#[path = "apikey_profile_tests.rs"]
mod tests;
