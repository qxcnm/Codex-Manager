use reqwest::header::HeaderValue;

pub(in super::super) fn normalize_upstream_base_url(base: &str) -> String {
    let mut normalized = base.trim().trim_end_matches('/').to_string();
    let lower = normalized.to_ascii_lowercase();
    if (lower.starts_with("https://chatgpt.com")
        || lower.starts_with("https://chat.openai.com"))
        && !lower.contains("/backend-api")
    {
        // 中文注释：对齐官方客户端的主机归一化，避免仅填域名时落到错误路径。
        normalized = format!("{normalized}/backend-api/codex");
    }
    normalized
}

pub(in super::super) fn resolve_upstream_base_url() -> String {
    let raw = std::env::var("GPTTOOLS_UPSTREAM_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://chatgpt.com/backend-api/codex".to_string());
    normalize_upstream_base_url(&raw)
}

pub(in super::super) fn resolve_upstream_fallback_base_url(primary_base: &str) -> Option<String> {
    std::env::var("GPTTOOLS_UPSTREAM_FALLBACK_BASE_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| normalize_upstream_base_url(&v))
        .or_else(|| {
            if is_chatgpt_backend_base(primary_base) {
                // 默认兜底到 OpenAI v1，避免 Cloudflare challenge 时模型列表不可用。
                Some("https://api.openai.com/v1".to_string())
            } else {
                None
            }
        })
}

pub(in super::super) fn is_openai_api_base(base: &str) -> bool {
    let normalized = base.trim().to_ascii_lowercase();
    normalized.contains("api.openai.com/v1")
}

pub(in super::super) fn is_chatgpt_backend_base(base: &str) -> bool {
    let normalized = base.trim().to_ascii_lowercase();
    normalized.contains("chatgpt.com/backend-api")
        || normalized.contains("chat.openai.com/backend-api")
}

pub(in super::super) fn should_try_openai_fallback(
    base: &str,
    request_path: &str,
    content_type: Option<&HeaderValue>,
) -> bool {
    if !is_chatgpt_backend_base(base) {
        return false;
    }
    let is_models_path = request_path == "/v1/models" || request_path.starts_with("/v1/models?");
    if is_models_path {
        // /models 需要与官方行为一致地直接透传，避免 fallback token-exchange 影响模型列表稳定性。
        return false;
    }
    let Some(content_type) = content_type else {
        return false;
    };
    let Ok(value) = content_type.to_str() else {
        return false;
    };
    super::super::is_html_content_type(value)
}

pub(in super::super) fn should_try_openai_fallback_by_status(
    base: &str,
    request_path: &str,
    status_code: u16,
) -> bool {
    if !is_chatgpt_backend_base(base) {
        return false;
    }
    let is_models_path = request_path == "/v1/models" || request_path.starts_with("/v1/models?");
    if is_models_path {
        return false;
    }
    matches!(status_code, 403 | 429)
}



