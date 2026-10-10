use axum::http::{HeaderName, HeaderValue};

/// 函数 `is_hop_by_hop_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn is_hop_by_hop_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("connection")
        || name.eq_ignore_ascii_case("keep-alive")
        || name.eq_ignore_ascii_case("proxy-authenticate")
        || name.eq_ignore_ascii_case("proxy-authorization")
        || name.eq_ignore_ascii_case("te")
        || name.eq_ignore_ascii_case("trailer")
        || name.eq_ignore_ascii_case("transfer-encoding")
        || name.eq_ignore_ascii_case("upgrade")
}

/// 函数 `should_skip_request_header`
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
pub(crate) fn should_skip_request_header(name: &HeaderName, value: &HeaderValue) -> bool {
    let lower = name.as_str();
    if is_hop_by_hop_header(lower)
        || lower.eq_ignore_ascii_case("host")
        || lower.eq_ignore_ascii_case("content-length")
    {
        return true;
    }
    // The routing/auth metadata parser consumes textual header values. Keep
    // its input compatible with HTTP HeaderValue::to_str validation.
    value.to_str().is_err()
}

/// 函数 `should_skip_response_header`
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
pub(crate) fn should_skip_response_header(name: &HeaderName) -> bool {
    let lower = name.as_str();
    is_hop_by_hop_header(lower) || lower.eq_ignore_ascii_case("content-length")
}

#[cfg(test)]
#[path = "tests/header_filter_tests.rs"]
mod tests;
