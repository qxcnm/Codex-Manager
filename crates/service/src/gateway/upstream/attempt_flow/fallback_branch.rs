use crate::gateway::upstream::support::failover_policy::{follow_up_action, FollowUpAction};
use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::header::HeaderMap;
use reqwest::header::HeaderValue;

use super::super::GatewayUpstreamResponse;

const REQUEST_ID_HEADER: &str = "x-request-id";
const OAI_REQUEST_ID_HEADER: &str = "x-oai-request-id";
const CF_RAY_HEADER: &str = "cf-ray";
const AUTH_ERROR_HEADER: &str = "x-openai-authorization-error";

pub(super) enum FallbackBranchResult {
    NotTriggered,
    RespondUpstream(GatewayUpstreamResponse),
    Failover,
    Terminal { status_code: u16, message: String },
}

/// 函数 `should_failover_after_fallback_non_success`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - has_more_candidates: 参数 has_more_candidates
///
/// # 返回
/// 返回函数执行结果
fn should_failover_after_fallback_non_success(status: u16, has_more_candidates: bool) -> bool {
    matches!(
        follow_up_action(
            crate::gateway::upstream::support::failover_policy::should_failover_after_fallback_non_success(status),
            has_more_candidates,
        ),
        FollowUpAction::Failover
    )
}

/// 函数 `extract_response_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn extract_response_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// 函数 `looks_like_blocked_marker`
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
fn looks_like_blocked_marker(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.contains("blocked")
        || normalized.contains("unsupported_country_region_territory")
        || normalized.contains("unsupported_country")
        || normalized.contains("region_restricted")
}

fn body_as_trimmed_text(body: &[u8]) -> Option<&str> {
    std::str::from_utf8(body)
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn text_looks_like_html(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    normalized.contains("<html")
        || normalized.contains("<!doctype html")
        || normalized.contains("<body")
        || normalized.contains("</html>")
}

fn body_looks_like_html(body: &[u8]) -> bool {
    body_as_trimmed_text(body).is_some_and(text_looks_like_html)
}

fn body_looks_like_cloudflare_challenge(status_code: u16, body: &[u8]) -> bool {
    body_as_trimmed_text(body).is_some_and(|text| {
        let normalized = text.to_ascii_lowercase();
        let looks_like_challenge = normalized.contains("cloudflare")
            || normalized.contains("cf-chl")
            || normalized.contains("just a moment")
            || normalized.contains("attention required")
            || normalized.contains("captcha")
            || normalized.contains("security check")
            || normalized.contains("access denied")
            || normalized.contains("waf");
        looks_like_challenge || (text_looks_like_html(text) && matches!(status_code, 401 | 403))
    })
}

/// 函数 `classify_fallback_non_success_kind`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - fallback_status: 参数 fallback_status
/// - content_type: 参数 content_type
/// - body: 参数 body
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn classify_fallback_non_success_kind(
    fallback_status: u16,
    content_type: Option<&str>,
    body: &[u8],
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> &'static str {
    if fallback_status == 429 {
        return "rate_limited";
    }
    if fallback_status == 404 {
        return "not_found";
    }
    if auth_error.is_some_and(looks_like_blocked_marker)
        || identity_error_code.is_some_and(looks_like_blocked_marker)
    {
        return "cloudflare_blocked";
    }
    if body_looks_like_cloudflare_challenge(fallback_status, body) {
        return "cloudflare_challenge";
    }
    if body_looks_like_html(body) {
        return "html";
    }
    if content_type
        .map(crate::gateway::is_html_content_type)
        .unwrap_or(false)
    {
        return "html";
    }
    if identity_error_code.is_some() {
        return "identity_error";
    }
    if auth_error.is_some() {
        return "auth_error";
    }
    if cf_ray.is_some() {
        return "cloudflare_edge";
    }
    if fallback_status >= 500 {
        return "server_error";
    }
    if serde_json::from_slice::<serde_json::Value>(body).is_ok() {
        return "json_error";
    }
    if body.is_empty() {
        "empty"
    } else {
        "non_json"
    }
}

/// 函数 `summarize_fallback_non_success`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - primary_status: 参数 primary_status
/// - fallback_status: 参数 fallback_status
/// - headers: 参数 headers
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn summarize_fallback_non_success(
    primary_status: u16,
    fallback_status: u16,
    headers: &HeaderMap,
    body: &[u8],
) -> String {
    let request_id = extract_response_header(headers, REQUEST_ID_HEADER)
        .or_else(|| extract_response_header(headers, OAI_REQUEST_ID_HEADER));
    let cf_ray = extract_response_header(headers, CF_RAY_HEADER);
    let auth_error = extract_response_header(headers, AUTH_ERROR_HEADER);
    let identity_error_code = crate::gateway::extract_identity_error_code_from_headers(headers);
    let content_type = extract_response_header(headers, "content-type");
    let kind = classify_fallback_non_success_kind(
        fallback_status,
        content_type.as_deref(),
        body,
        cf_ray.as_deref(),
        auth_error.as_deref(),
        identity_error_code.as_deref(),
    );
    let body_hint = crate::gateway::summarize_upstream_error_hint_from_body(fallback_status, body)
        .or_else(|| {
            let trimmed = std::str::from_utf8(body).ok()?.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .unwrap_or_else(|| "unknown error".to_string());

    let mut details = vec![
        format!("kind={kind}"),
        format!("primary_status={primary_status}"),
    ];
    if let Some(request_id) = request_id {
        details.push(format!("request_id={request_id}"));
    }
    if let Some(cf_ray) = cf_ray {
        details.push(format!("cf_ray={cf_ray}"));
    }
    if let Some(auth_error) = auth_error {
        details.push(format!("auth_error={auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code {
        details.push(format!("identity_error_code={identity_error_code}"));
    }

    format!(
        "upstream fallback non-success(status={fallback_status}, body={body_hint}, {})",
        details.join(", ")
    )
}

/// 函数 `summarize_fallback_non_success_headers_only`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - primary_status: 参数 primary_status
/// - fallback_status: 参数 fallback_status
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn summarize_fallback_non_success_headers_only(
    primary_status: u16,
    fallback_status: u16,
    headers: &HeaderMap,
) -> String {
    let request_id = extract_response_header(headers, REQUEST_ID_HEADER)
        .or_else(|| extract_response_header(headers, OAI_REQUEST_ID_HEADER));
    let cf_ray = extract_response_header(headers, CF_RAY_HEADER);
    let auth_error = extract_response_header(headers, AUTH_ERROR_HEADER);
    let identity_error_code = crate::gateway::extract_identity_error_code_from_headers(headers);
    let content_type = extract_response_header(headers, "content-type");
    let kind = classify_fallback_non_success_kind(
        fallback_status,
        content_type.as_deref(),
        &[],
        cf_ray.as_deref(),
        auth_error.as_deref(),
        identity_error_code.as_deref(),
    );

    let mut details = vec![
        format!("kind={kind}"),
        format!("primary_status={primary_status}"),
    ];
    if let Some(request_id) = request_id {
        details.push(format!("request_id={request_id}"));
    }
    if let Some(cf_ray) = cf_ray {
        details.push(format!("cf_ray={cf_ray}"));
    }
    if let Some(auth_error) = auth_error {
        details.push(format!("auth_error={auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code {
        details.push(format!("identity_error_code={identity_error_code}"));
    }
    if let Some(content_type) = content_type {
        details.push(format!("content_type={content_type}"));
    }

    format!(
        "upstream fallback non-success(status={fallback_status}, {})",
        details.join(", ")
    )
}

/// 函数 `handle_openai_fallback_branch`
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
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_openai_fallback_branch<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    upstream_base: &str,
    path: &str,
    fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    status: reqwest::StatusCode,
    upstream_content_type: Option<&HeaderValue>,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> FallbackBranchResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    if !allow_openai_fallback || fallback_base.is_none() {
        return FallbackBranchResult::NotTriggered;
    }

    let should_fallback =
        super::super::super::should_try_openai_fallback(upstream_base, path, upstream_content_type)
            || super::super::super::should_try_openai_fallback_by_status(
                upstream_base,
                path,
                status.as_u16(),
            );
    if !should_fallback {
        return FallbackBranchResult::NotTriggered;
    }

    let fallback_base = fallback_base.expect("fallback base already checked");
    if debug {
        log::warn!(
            "event=gateway_upstream_fallback path={} status={} account_id={} from={} to={}",
            path,
            status.as_u16(),
            account.id,
            upstream_base,
            fallback_base
        );
    }
    match super::super::super::try_openai_fallback(
        client,
        storage,
        method,
        path,
        incoming_headers,
        body,
        is_stream,
        fallback_base,
        account,
        token,
        strip_session_affinity,
        debug,
    ) {
        Ok(Some(resp)) => {
            if resp.status().is_success() {
                super::super::super::clear_account_cooldown(&account.id);
                log_gateway_result(Some(fallback_base), resp.status().as_u16(), None);
                return FallbackBranchResult::RespondUpstream(resp.into());
            }
            let fallback_status = resp.status().as_u16();
            super::super::super::mark_account_cooldown_for_status(&account.id, fallback_status);
            // 候选已经被批次轮询限制在小集合内；5xx 在本批内切换下一个账号，
            // 避免一个抖动账号直接把整次请求拖成 502。
            if should_failover_after_fallback_non_success(fallback_status, has_more_candidates) {
                let headers = resp.headers().clone();
                let body = resp.bytes().unwrap_or_default();
                let fallback_error = summarize_fallback_non_success(
                    status.as_u16(),
                    fallback_status,
                    &headers,
                    &body,
                );
                log_gateway_result(
                    Some(fallback_base),
                    fallback_status,
                    Some(fallback_error.as_str()),
                );
                FallbackBranchResult::Failover
            } else {
                let headers = resp.headers().clone();
                let fallback_error = summarize_fallback_non_success_headers_only(
                    status.as_u16(),
                    fallback_status,
                    &headers,
                );
                log_gateway_result(
                    Some(fallback_base),
                    fallback_status,
                    Some(fallback_error.as_str()),
                );
                FallbackBranchResult::RespondUpstream(resp.into())
            }
        }
        Ok(None) => {
            super::super::super::mark_account_cooldown(
                &account.id,
                super::super::super::CooldownReason::Network,
            );
            log_gateway_result(
                Some(fallback_base),
                502,
                Some("upstream fallback unavailable"),
            );
            if has_more_candidates {
                FallbackBranchResult::Failover
            } else {
                FallbackBranchResult::Terminal {
                    status_code: 502,
                    message: "upstream blocked by Cloudflare".to_string(),
                }
            }
        }
        Err(err) => {
            super::super::super::mark_account_cooldown(
                &account.id,
                super::super::super::CooldownReason::Network,
            );
            log_gateway_result(Some(fallback_base), 502, Some(err.as_str()));
            if has_more_candidates {
                FallbackBranchResult::Failover
            } else {
                FallbackBranchResult::Terminal {
                    status_code: 502,
                    message: format!("upstream fallback error: {err}"),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/attempt_flow/fallback_branch_tests.rs"]
mod tests;
