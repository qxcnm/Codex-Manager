use serde_json::json;
use serde_json::Value;

pub(crate) const ERROR_CODE_HEADER_NAME: &str = "X-CodexManager-Error-Code";
pub(crate) const TRACE_ID_HEADER_NAME: &str = "X-CodexManager-Trace-Id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorCode {
    UnknownMethod,
    UnknownError,
    PermissionDenied,
    InvalidSettingsPayload,
    InvalidRequestPayload,
    InputTooLarge,
    ProtocolMappingError,
    RequestBodyTooLarge,
    BackendProxyError,
    BuildResponseFailed,
    UpstreamTimeout,
    UpstreamChallengeBlocked,
    UpstreamRateLimited,
    UpstreamNotFound,
    UpstreamNonSuccess,
    NoAvailableAccount,
    CandidateResolveFailed,
    ResponseWriteFailed,
    StreamInterrupted,
}

impl ErrorCode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownMethod => "unknown_method",
            Self::UnknownError => "unknown_error",
            Self::PermissionDenied => "permission_denied",
            Self::InvalidSettingsPayload => "invalid_settings_payload",
            Self::InvalidRequestPayload => "invalid_request_payload",
            Self::InputTooLarge => "input_too_large",
            Self::ProtocolMappingError => "protocol_mapping_error",
            Self::RequestBodyTooLarge => "request_body_too_large",
            Self::BackendProxyError => "backend_proxy_error",
            Self::BuildResponseFailed => "build_response_failed",
            Self::UpstreamTimeout => "upstream_timeout",
            Self::UpstreamChallengeBlocked => "upstream_challenge_blocked",
            Self::UpstreamRateLimited => "upstream_rate_limited",
            Self::UpstreamNotFound => "upstream_not_found",
            Self::UpstreamNonSuccess => "upstream_non_success",
            Self::NoAvailableAccount => "no_available_account",
            Self::CandidateResolveFailed => "candidate_resolve_failed",
            Self::ResponseWriteFailed => "response_write_failed",
            Self::StreamInterrupted => "stream_interrupted",
        }
    }
}

/// 函数 `classify_message`
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
pub(crate) fn classify_message(message: &str) -> ErrorCode {
    let normalized = message.trim().to_ascii_lowercase();
    let normalized_english_tail = message
        .trim()
        .rsplit_once('(')
        .and_then(|(_, tail)| tail.strip_suffix(')'))
        .map(str::trim)
        .filter(|tail| !tail.is_empty())
        .map(|tail| tail.to_ascii_lowercase());
    let eq = |expected: &str| {
        normalized == expected || normalized_english_tail.as_deref() == Some(expected)
    };
    let starts_with = |prefix: &str| {
        normalized.starts_with(prefix)
            || normalized_english_tail
                .as_deref()
                .is_some_and(|tail| tail.starts_with(prefix))
    };
    let contains = |needle: &str| {
        normalized.contains(needle)
            || normalized_english_tail
                .as_deref()
                .is_some_and(|tail| tail.contains(needle))
    };
    if normalized.is_empty() {
        return ErrorCode::UnknownError;
    }

    if eq("unknown_method") {
        return ErrorCode::UnknownMethod;
    }
    if eq("permission_denied") || starts_with("permission_denied:") {
        return ErrorCode::PermissionDenied;
    }
    if starts_with("invalid app settings payload:") {
        return ErrorCode::InvalidSettingsPayload;
    }
    if starts_with("input exceeds the maximum length of")
        || starts_with("输入超过最大长度")
        || starts_with("输入过大")
    {
        return ErrorCode::InputTooLarge;
    }
    if starts_with("request body too large") {
        return ErrorCode::RequestBodyTooLarge;
    }
    if starts_with("backend proxy error:") {
        return ErrorCode::BackendProxyError;
    }
    if starts_with("build response failed:") {
        return ErrorCode::BuildResponseFailed;
    }
    if eq("upstream total timeout exceeded") || eq("upstream request timed out") {
        return ErrorCode::UpstreamTimeout;
    }
    if eq("上游请求超时") || contains("连接超时") {
        return ErrorCode::UpstreamTimeout;
    }
    if eq("stream idle timeout") || eq("上游流式空闲超时") || contains("stream_timeout") {
        return ErrorCode::UpstreamTimeout;
    }
    if starts_with("upstream blocked by cloudflare/waf") || eq("upstream challenge blocked") {
        return ErrorCode::UpstreamChallengeBlocked;
    }
    if contains("cloudflare/waf") || contains("安全验证拦截") || contains("验证/拦截页面")
    {
        return ErrorCode::UpstreamChallengeBlocked;
    }
    if eq("upstream rate-limited") {
        return ErrorCode::UpstreamRateLimited;
    }
    if eq("upstream not-found failover") {
        return ErrorCode::UpstreamNotFound;
    }
    if eq("upstream non-success") {
        return ErrorCode::UpstreamNonSuccess;
    }
    if eq("no available account") {
        return ErrorCode::NoAvailableAccount;
    }
    if starts_with("candidate resolve failed:") {
        return ErrorCode::CandidateResolveFailed;
    }
    if starts_with("response write failed:") {
        return ErrorCode::ResponseWriteFailed;
    }
    if eq("stream disconnected before completion")
        || eq("request or response body error")
        || eq("stream read failed")
        || eq("response.incomplete")
        || eq("上游中途断开，未返回具体错误信息")
        || eq("网络抖动")
        || eq("连接中断（可能是网络波动或客户端主动取消）")
    {
        return ErrorCode::StreamInterrupted;
    }
    if starts_with("upstream stream terminated unexpectedly")
        || starts_with("upstream stream read failed: connection interrupted")
    {
        return ErrorCode::StreamInterrupted;
    }
    if starts_with("上游流中途中断")
        || starts_with("上游流读取失败（连接中断）")
        || contains("上游连接中断")
    {
        return ErrorCode::StreamInterrupted;
    }
    if starts_with("upstream returned non-api content") {
        return ErrorCode::UpstreamNonSuccess;
    }
    if starts_with("上游返回的不是正常接口数据") || starts_with("上游返回了网页内容而不是接口数据")
    {
        return ErrorCode::UpstreamNonSuccess;
    }
    if contains("model_not_found")
        || contains("model not found")
        || contains("unsupported model")
        || contains("not supported")
        || contains("does not exist")
    {
        return ErrorCode::UpstreamNonSuccess;
    }
    if starts_with("模型不支持") {
        return ErrorCode::UpstreamNonSuccess;
    }
    if eq("response.failed") || eq("上游请求失败，未返回具体错误信息") {
        return ErrorCode::UpstreamNonSuccess;
    }
    if starts_with("invalid upstream ")
        || ((contains("serialize") || contains("serialized")) && contains("json"))
        || contains("sse bytes")
    {
        return ErrorCode::ProtocolMappingError;
    }
    if eq("invalid claude request json")
        || eq("claude request body must be an object")
        || eq("invalid gemini request json")
        || eq("gemini request body must be an object")
    {
        return ErrorCode::InvalidRequestPayload;
    }

    ErrorCode::UnknownError
}

/// 函数 `code_or_dash`
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
pub(crate) fn code_or_dash(message: Option<&str>) -> &'static str {
    message
        .map(classify_message)
        .map(ErrorCode::as_str)
        .unwrap_or("-")
}

/// 函数 `code_for_message`
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
pub(crate) fn code_for_message(message: &str) -> &'static str {
    classify_message(message).as_str()
}

/// 函数 `rpc_error_payload`
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
pub(crate) fn rpc_error_payload(message: String) -> Value {
    let code = classify_message(message.as_str()).as_str();
    json!({
        "error": message,
        "errorCode": code,
        "errorDetail": {
            "code": code,
            "message": message,
        }
    })
}

/// 函数 `rpc_action_error_payload`
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
pub(crate) fn rpc_action_error_payload(message: String) -> Value {
    let code = classify_message(message.as_str()).as_str();
    json!({
        "ok": false,
        "error": message,
        "errorCode": code,
        "errorDetail": {
            "code": code,
            "message": message,
        }
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
