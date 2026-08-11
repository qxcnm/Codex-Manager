use super::{
    should_failover_after_fallback_non_success, summarize_fallback_non_success,
    summarize_fallback_non_success_headers_only,
};
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;

/// 函数 `fallback_non_success_5xx_does_not_failover_even_with_more_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_5xx_failovers_when_more_candidates_remain() {
    assert!(should_failover_after_fallback_non_success(500, true));
    assert!(should_failover_after_fallback_non_success(503, true));
}

/// 函数 `fallback_non_success_auth_and_rate_limit_can_failover_when_candidates_remain`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_auth_and_rate_limit_can_failover_when_candidates_remain() {
    assert!(should_failover_after_fallback_non_success(401, true));
    assert!(should_failover_after_fallback_non_success(403, true));
    assert!(should_failover_after_fallback_non_success(404, true));
    assert!(should_failover_after_fallback_non_success(429, true));
}

/// 函数 `fallback_non_success_never_failover_without_more_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_never_failover_without_more_candidates() {
    assert!(!should_failover_after_fallback_non_success(401, false));
    assert!(!should_failover_after_fallback_non_success(429, false));
    assert!(!should_failover_after_fallback_non_success(500, false));
}

/// 函数 `fallback_non_success_summary_includes_debug_headers_and_body_hint`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_summary_includes_debug_headers_and_body_hint() {
    let mut headers = HeaderMap::new();
    headers.insert("x-oai-request-id", HeaderValue::from_static("req_fallback"));
    headers.insert("cf-ray", HeaderValue::from_static("ray_fallback"));
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("expired_session"),
    );
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("{\"identity_error_code\":\"org_membership_required\"}"),
    );

    let message = summarize_fallback_non_success(
        403,
        403,
        &headers,
        b"<html><title>Just a moment...</title><body>Cloudflare</body></html>",
    );

    assert!(
        message.contains("kind=cloudflare_challenge"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("status=403"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("Cloudflare 安全验证页"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("primary_status=403"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("request_id=req_fallback"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("cf_ray=ray_fallback"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("auth_error=expired_session"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("identity_error_code=org_membership_required"),
        "unexpected summary: {message}"
    );
}

/// 函数 `fallback_non_success_summary_uses_plain_body_when_no_structured_hint_exists`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_summary_uses_plain_body_when_no_structured_hint_exists() {
    let headers = HeaderMap::new();
    let message = summarize_fallback_non_success(404, 404, &headers, b"plain upstream error");

    assert!(
        message.contains("body=plain upstream error"),
        "unexpected summary: {message}"
    );
}

/// 函数 `fallback_non_success_headers_only_summary_includes_debug_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_headers_only_summary_includes_debug_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-oai-request-id",
        HeaderValue::from_static("req_fallback_headers"),
    );
    headers.insert("cf-ray", HeaderValue::from_static("ray_fallback_headers"));
    headers.insert(
        "x-openai-authorization-error",
        HeaderValue::from_static("blocked_by_policy"),
    );
    headers.insert(
        "x-error-json",
        HeaderValue::from_static("{\"identity_error_code\":\"org_membership_required\"}"),
    );
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/html; charset=utf-8"),
    );

    let message = summarize_fallback_non_success_headers_only(500, 500, &headers);

    assert!(
        message.contains("kind=cloudflare_blocked"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("status=500"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("primary_status=500"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("request_id=req_fallback_headers"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("cf_ray=ray_fallback_headers"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("auth_error=blocked_by_policy"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("identity_error_code=org_membership_required"),
        "unexpected summary: {message}"
    );
    assert!(
        message.contains("content_type=text/html; charset=utf-8"),
        "unexpected summary: {message}"
    );
}

/// 函数 `fallback_non_success_headers_only_summary_marks_server_error_without_debug_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_headers_only_summary_marks_server_error_without_debug_headers() {
    let headers = HeaderMap::new();

    let message = summarize_fallback_non_success_headers_only(502, 502, &headers);

    assert!(
        message.contains("kind=server_error"),
        "unexpected summary: {message}"
    );
}
