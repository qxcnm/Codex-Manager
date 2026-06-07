use chrono::DateTime;
use codexmanager_core::usage::{accounts_check_endpoint, usage_endpoint};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::Client;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

use crate::account_plan::normalize_account_plan_value;

static USAGE_HTTP_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();
static SUBSCRIPTION_HTTP_CLIENT: OnceLock<RwLock<Client>> = OnceLock::new();
static USAGE_HTTP_RUNTIME: OnceLock<Runtime> = OnceLock::new();
const USAGE_HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const ENV_UPSTREAM_PROXY_URL: &str = "CODEXMANAGER_UPSTREAM_PROXY_URL";
const USAGE_HTTP_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
const REFRESH_TOKEN_EXPIRED_MESSAGE: &str =
    "Your access token could not be refreshed because your refresh token has expired. Please log out and sign in again.";
const REFRESH_TOKEN_REUSED_MESSAGE: &str =
    "Your access token could not be refreshed because your refresh token was already used. Please log out and sign in again.";
const REFRESH_TOKEN_INVALIDATED_MESSAGE: &str =
    "Your access token could not be refreshed because your refresh token was revoked. Please log out and sign in again.";
const REFRESH_TOKEN_INVALID_GRANT_MESSAGE: &str =
    "Your access token could not be refreshed because your refresh token is no longer valid. Please log out and sign in again.";
const REFRESH_TOKEN_UNKNOWN_MESSAGE: &str =
    "Your access token could not be refreshed. Please log out and sign in again.";
const REFRESH_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR: &str = "CODEX_REFRESH_TOKEN_URL_OVERRIDE";
const REFRESH_TOKEN_SCOPES: &str = "openid profile email";
const RESIDENCY_HEADER_NAME: &str = "x-openai-internal-codex-residency";
const CHATGPT_ACCOUNT_ID_HEADER_NAME: &str = "ChatGPT-Account-ID";
const REQUEST_ID_HEADER: &str = "x-request-id";
const OAI_REQUEST_ID_HEADER: &str = "x-oai-request-id";
const CF_RAY_HEADER: &str = "cf-ray";
const AUTH_ERROR_HEADER: &str = "x-openai-authorization-error";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshTokenAuthErrorReason {
    Expired,
    Reused,
    Invalidated,
    InvalidGrant,
    Unknown401,
}

impl RefreshTokenAuthErrorReason {
    /// 函数 `as_code`
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
    pub(crate) fn as_code(self) -> &'static str {
        match self {
            Self::Expired => "refresh_token_expired",
            Self::Reused => "refresh_token_reused",
            Self::Invalidated => "refresh_token_invalidated",
            Self::InvalidGrant => "invalid_grant",
            Self::Unknown401 => "refresh_token_unknown_401",
        }
    }

    /// 函数 `user_message`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn user_message(self) -> &'static str {
        match self {
            Self::Expired => REFRESH_TOKEN_EXPIRED_MESSAGE,
            Self::Reused => REFRESH_TOKEN_REUSED_MESSAGE,
            Self::Invalidated => REFRESH_TOKEN_INVALIDATED_MESSAGE,
            Self::InvalidGrant => REFRESH_TOKEN_INVALID_GRANT_MESSAGE,
            Self::Unknown401 => REFRESH_TOKEN_UNKNOWN_MESSAGE,
        }
    }

}
#[derive(serde::Deserialize)]
pub(crate) struct RefreshTokenResponse {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) id_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct AccountSubscriptionSnapshot {
    pub(crate) has_subscription: bool,
    pub(crate) account_plan_type: Option<String>,
    pub(crate) plan_type: Option<String>,
    pub(crate) expires_at: Option<i64>,
    pub(crate) renews_at: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct AccountsCheckResponse {
    #[serde(default)]
    accounts: HashMap<String, AccountsCheckEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct AccountsCheckEntry {
    #[serde(default)]
    account: Option<AccountsCheckAccount>,
    #[serde(default)]
    entitlement: Option<AccountsCheckEntitlement>,
}

#[derive(Debug, serde::Deserialize)]
struct AccountsCheckAccount {
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default)]
    is_default: Option<bool>,
    #[serde(default)]
    has_subscription: Option<bool>,
    #[serde(default)]
    has_active_subscription: Option<bool>,
    #[serde(default)]
    is_paid_subscription_active: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct AccountsCheckEntitlement {
    #[serde(default)]
    subscription_plan: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    renews_at: Option<String>,
    #[serde(default)]
    next_renewal_at: Option<String>,
    #[serde(default)]
    next_credit_grant_update: Option<String>,
    #[serde(default)]
    renewal_date: Option<String>,
    #[serde(default)]
    will_renew: Option<bool>,
    #[serde(default)]
    has_active_subscription: Option<bool>,
}

/// 函数 `usage_http_runtime`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn usage_http_runtime() -> &'static Runtime {
    USAGE_HTTP_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("usage-http")
            .build()
            .unwrap_or_else(|err| panic!("build usage http runtime failed: {err}"))
    })
}

/// 函数 `run_usage_future`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - future: 参数 future
///
/// # 返回
/// 返回函数执行结果
fn run_usage_future<F>(future: F) -> F::Output
where
    F: Future,
{
    usage_http_runtime().block_on(future)
}

/// 函数 `extract_refresh_token_error_code`
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
fn extract_refresh_token_error_code(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    value
        .get("error")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_ascii_lowercase())
        .or_else(|| {
            value
                .get("code")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.to_ascii_lowercase())
        })
        .or_else(|| {
            value
                .get("error")
                .and_then(|value| value.get("code"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.to_ascii_lowercase())
        })
}

/// 函数 `looks_like_refresh_token_blocked_marker`
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
fn looks_like_refresh_token_blocked_marker(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized.contains("blocked")
        || normalized.contains("unsupported_country_region_territory")
        || normalized.contains("unsupported_country")
        || normalized.contains("region_restricted")
}

pub(crate) fn is_region_blocked_error_message(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.contains("unsupported_country_region_territory")
        || normalized.contains("unsupported_country")
        || normalized.contains("region_restricted")
        || normalized.contains("kind=cloudflare_blocked")
        || normalized.contains("access blocked by cloudflare")
        || normalized.contains("country, region, or territory not supported")
}

pub(crate) fn is_refresh_token_region_blocked_error_message(message: &str) -> bool {
    let normalized = message.trim().to_ascii_lowercase();
    normalized.contains("refresh token failed") && is_region_blocked_error_message(message)
}

/// 函数 `classify_refresh_token_status_error_kind_with_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn classify_refresh_token_status_error_kind_with_headers(
    headers: Option<&HeaderMap>,
    body: &str,
) -> &'static str {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        if let Some(headers) = headers {
            if extract_response_header(headers, AUTH_ERROR_HEADER)
                .as_deref()
                .is_some_and(looks_like_refresh_token_blocked_marker)
                || crate::gateway::extract_identity_error_code_from_headers(headers)
                    .as_deref()
                    .is_some_and(looks_like_refresh_token_blocked_marker)
            {
                return "cloudflare_blocked";
            }
            if crate::gateway::extract_identity_error_code_from_headers(headers).is_some() {
                return "identity_error";
            }
            if extract_response_header(headers, AUTH_ERROR_HEADER).is_some() {
                return "auth_error";
            }
            if extract_response_header(headers, CF_RAY_HEADER).is_some() {
                return "cloudflare_edge";
            }
        }
        return "empty";
    }

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return "json";
    }

    let normalized = trimmed.to_ascii_lowercase();
    if normalized.contains("<html") || normalized.contains("<!doctype html") {
        if normalized.contains("cloudflare") && normalized.contains("blocked") {
            return "cloudflare_blocked";
        }
        if normalized.contains("cloudflare")
            || normalized.contains("just a moment")
            || normalized.contains("attention required")
        {
            return "cloudflare_challenge";
        }
        return "html";
    }

    "non_json"
}

/// 函数 `classify_refresh_token_auth_error_reason_from_code`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - code: 参数 code
///
/// # 返回
/// 返回函数执行结果
fn classify_refresh_token_auth_error_reason_from_code(
    code: Option<&str>,
) -> RefreshTokenAuthErrorReason {
    match code {
        Some("refresh_token_expired") => RefreshTokenAuthErrorReason::Expired,
        Some("refresh_token_reused") => RefreshTokenAuthErrorReason::Reused,
        Some("refresh_token_invalidated") => RefreshTokenAuthErrorReason::Invalidated,
        Some("invalid_grant") => RefreshTokenAuthErrorReason::InvalidGrant,
        _ => RefreshTokenAuthErrorReason::Unknown401,
    }
}

/// 函数 `classify_refresh_token_auth_error_reason`
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
pub(crate) fn classify_refresh_token_auth_error_reason(
    status: reqwest::StatusCode,
    body: &str,
) -> Option<RefreshTokenAuthErrorReason> {
    classify_refresh_token_auth_error_reason_with_headers(status, None, body)
}

/// 函数 `classify_refresh_token_auth_error_reason_with_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - _headers: 参数 _headers
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn classify_refresh_token_auth_error_reason_with_headers(
    status: reqwest::StatusCode,
    _headers: Option<&HeaderMap>,
    body: &str,
) -> Option<RefreshTokenAuthErrorReason> {
    if status != reqwest::StatusCode::UNAUTHORIZED {
        return None;
    }
    Some(classify_refresh_token_auth_error_reason_from_code(
        extract_refresh_token_error_code(body).as_deref(),
    ))
}

/// 函数 `refresh_token_auth_error_reason_from_message`
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
pub(crate) fn refresh_token_auth_error_reason_from_message(
    message: &str,
) -> Option<RefreshTokenAuthErrorReason> {
    let normalized = message.trim();
    let is_401 = normalized.contains("refresh token failed with status 401");
    let is_400_invalid_grant = normalized.contains("refresh token failed with status 400")
        && normalized.contains("invalid_grant");
    if !is_401 && !is_400_invalid_grant {
        return None;
    }
    if is_400_invalid_grant {
        return Some(RefreshTokenAuthErrorReason::InvalidGrant);
    }
    if normalized.contains(REFRESH_TOKEN_EXPIRED_MESSAGE) {
        return Some(RefreshTokenAuthErrorReason::Expired);
    }
    if normalized.contains(REFRESH_TOKEN_REUSED_MESSAGE) {
        return Some(RefreshTokenAuthErrorReason::Reused);
    }
    if normalized.contains(REFRESH_TOKEN_INVALIDATED_MESSAGE) {
        return Some(RefreshTokenAuthErrorReason::Invalidated);
    }
    if normalized.contains(REFRESH_TOKEN_INVALID_GRANT_MESSAGE) {
        return Some(RefreshTokenAuthErrorReason::InvalidGrant);
    }
    Some(RefreshTokenAuthErrorReason::Unknown401)
}

/// 函数 `format_refresh_token_status_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn format_refresh_token_status_error(status: reqwest::StatusCode, body: &str) -> String {
    format_refresh_token_status_error_with_headers(status, None, body)
}

/// 函数 `format_refresh_token_status_error_with_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - headers: 参数 headers
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn format_refresh_token_status_error_with_headers(
    status: reqwest::StatusCode,
    headers: Option<&HeaderMap>,
    body: &str,
) -> String {
    if let Some(reason) =
        classify_refresh_token_auth_error_reason_with_headers(status, headers, body)
    {
        let message = reason.user_message();
        return format!("refresh token failed with status {status}: {message}");
    }

    let body_hint =
        crate::gateway::summarize_upstream_error_hint_from_body(status.as_u16(), body.as_bytes())
            .or_else(|| {
                let snippet = body
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(256)
                    .collect::<String>();
                (!snippet.is_empty()).then_some(snippet)
            });
    let debug_suffix = headers
        .map(|headers| {
            let mut details = Vec::new();
            let kind = classify_refresh_token_status_error_kind_with_headers(Some(headers), body);
            if kind != "json" {
                details.push(format!("kind={kind}"));
            }
            if let Some(request_id) = extract_response_header(headers, REQUEST_ID_HEADER)
                .or_else(|| extract_response_header(headers, OAI_REQUEST_ID_HEADER))
            {
                details.push(format!("request_id={request_id}"));
            }
            if let Some(cf_ray) = extract_response_header(headers, CF_RAY_HEADER) {
                details.push(format!("cf_ray={cf_ray}"));
            }
            if let Some(auth_error) = extract_response_header(headers, AUTH_ERROR_HEADER) {
                details.push(format!("auth_error={auth_error}"));
            }
            if let Some(identity_error_code) =
                crate::gateway::extract_identity_error_code_from_headers(headers)
            {
                details.push(format!("identity_error_code={identity_error_code}"));
            }
            if details.is_empty() {
                String::new()
            } else {
                format!(" [{}]", details.join(", "))
            }
        })
        .unwrap_or_default();
    if let Some(body_hint) = body_hint {
        format!("refresh token failed with status {status}: {body_hint}{debug_suffix}")
    } else if debug_suffix.is_empty() {
        format!("refresh token failed with status {status}")
    } else {
        format!("refresh token failed with status {status}{debug_suffix}")
    }
}

/// 函数 `build_usage_http_client`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn build_usage_http_client() -> Client {
    let default_headers = build_usage_http_default_headers();
    let builder = Client::builder()
        // 中文注释：轮询链路复用连接池可降低握手开销；不复用会在多账号刷新时放大短连接抖动。
        .connect_timeout(USAGE_HTTP_CONNECT_TIMEOUT)
        .timeout(USAGE_HTTP_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(8)
        .pool_idle_timeout(Some(Duration::from_secs(60)))
        .user_agent(crate::gateway::current_codex_user_agent())
        .default_headers(default_headers);
    let builder = crate::gateway::apply_async_upstream_proxy(
        builder,
        current_upstream_proxy_url().as_deref(),
        "usage_http_proxy_invalid",
    );
    builder.build().unwrap_or_else(|_| Client::new())
}
fn build_subscription_http_client() -> Client {
    let builder = Client::builder()
        .connect_timeout(USAGE_HTTP_CONNECT_TIMEOUT)
        .timeout(USAGE_HTTP_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Some(Duration::from_secs(60)));
    let builder = crate::gateway::apply_async_upstream_proxy(
        builder,
        current_upstream_proxy_url().as_deref(),
        "subscription_http_proxy_invalid",
    );
    builder.build().unwrap_or_else(|_| Client::new())
}
/// 函数 `build_usage_http_default_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn build_usage_http_default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&crate::gateway::current_wire_originator()) {
        headers.insert(HeaderName::from_static("originator"), value);
    }
    if let Some(residency_requirement) = crate::gateway::current_residency_requirement() {
        if let Ok(value) = HeaderValue::from_str(&residency_requirement) {
            headers.insert(HeaderName::from_static(RESIDENCY_HEADER_NAME), value);
        }
    }
    headers
}

/// 函数 `build_usage_request_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
fn build_usage_request_headers(workspace_id: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(workspace_id) = workspace_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Ok(value) = HeaderValue::from_str(workspace_id) {
            if let Ok(name) = HeaderName::from_bytes(CHATGPT_ACCOUNT_ID_HEADER_NAME.as_bytes()) {
                headers.insert(name, value);
            }
        }
    }
    headers
}

/// 函数 `resolve_refresh_token_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
///
/// # 返回
/// 返回函数执行结果
fn resolve_refresh_token_url(issuer: &str) -> String {
    if let Some(override_url) = std::env::var(REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return override_url;
    }

    let normalized_issuer = issuer.trim().trim_end_matches('/');
    if normalized_issuer.is_empty()
        || normalized_issuer.eq_ignore_ascii_case("https://auth.openai.com")
    {
        return REFRESH_TOKEN_URL.to_string();
    }

    format!("{normalized_issuer}/oauth/token")
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

/// 函数 `summarize_usage_error_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - headers: 参数 headers
/// - body: 参数 body
/// - force_html_error: 参数 force_html_error
///
/// # 返回
/// 返回函数执行结果
fn summarize_endpoint_error_response(
    endpoint_name: &str,
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: &str,
    force_html_error: bool,
) -> String {
    let request_id = extract_response_header(headers, REQUEST_ID_HEADER)
        .or_else(|| extract_response_header(headers, OAI_REQUEST_ID_HEADER));
    let cf_ray = extract_response_header(headers, CF_RAY_HEADER);
    let auth_error = extract_response_header(headers, AUTH_ERROR_HEADER);
    let identity_error_code = crate::gateway::extract_identity_error_code_from_headers(headers);
    let body_hint = if force_html_error {
        crate::gateway::summarize_upstream_error_hint_from_body(403, body.as_bytes())
    } else {
        crate::gateway::summarize_upstream_error_hint_from_body(status.as_u16(), body.as_bytes())
    }
    .or_else(|| {
        let trimmed = body.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
    .unwrap_or_else(|| "unknown error".to_string());

    let mut details = Vec::new();
    if let Some(request_id) = request_id {
        details.push(format!("request id: {request_id}"));
    }
    if let Some(cf_ray) = cf_ray {
        details.push(format!("cf-ray: {cf_ray}"));
    }
    if let Some(auth_error) = auth_error {
        details.push(format!("auth error: {auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code {
        details.push(format!("identity error code: {identity_error_code}"));
    }

    if details.is_empty() {
        format!(
            "{endpoint_name} endpoint failed: status={} body={body_hint}",
            status
        )
    } else {
        format!(
            "{endpoint_name} endpoint failed: status={} body={body_hint}, {}",
            status,
            details.join(", ")
        )
    }
}

/// 函数 `summarize_usage_error_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - headers: 参数 headers
/// - body: 参数 body
/// - force_html_error: 参数 force_html_error
///
/// # 返回
/// 返回函数执行结果
fn summarize_usage_error_response(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: &str,
    force_html_error: bool,
) -> String {
    summarize_endpoint_error_response("usage", status, headers, body, force_html_error)
}

/// 函数 `summarize_subscription_error_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// - status: 参数 status
/// - headers: 参数 headers
/// - body: 参数 body
/// - force_html_error: 参数 force_html_error
///
/// # 返回
/// 返回函数执行结果
fn summarize_subscription_error_response(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: &str,
    force_html_error: bool,
) -> String {
    summarize_endpoint_error_response("subscription", status, headers, body, force_html_error)
}

fn extract_accounts_check_plan_type(entry: &AccountsCheckEntry) -> Option<String> {
    entry
        .account
        .as_ref()
        .and_then(|value| value.plan_type.as_deref())
        .or_else(|| {
            entry.entitlement.as_ref().and_then(|value| {
                value
                    .subscription_plan
                    .as_deref()
                    .filter(|plan| !plan.trim().is_empty())
            })
        })
        .and_then(normalize_account_plan_value)
}

fn extract_accounts_check_subscription_plan(entry: &AccountsCheckEntry) -> Option<String> {
    entry
        .entitlement
        .as_ref()
        .and_then(|value| value.subscription_plan.as_deref())
        .or_else(|| {
            entry
                .account
                .as_ref()
                .and_then(|value| value.plan_type.as_deref())
        })
        .and_then(normalize_account_plan_value)
}

fn extract_accounts_check_expires_at(entry: &AccountsCheckEntry) -> Option<i64> {
    parse_subscription_timestamp(
        entry
            .entitlement
            .as_ref()
            .and_then(|value| value.expires_at.as_deref()),
    )
}

fn extract_accounts_check_renews_at(entry: &AccountsCheckEntry) -> Option<i64> {
    let entitlement = entry.entitlement.as_ref()?;
    parse_subscription_timestamp(entitlement.renews_at.as_deref())
        .or_else(|| parse_subscription_timestamp(entitlement.next_renewal_at.as_deref()))
        .or_else(|| parse_subscription_timestamp(entitlement.next_credit_grant_update.as_deref()))
        .or_else(|| parse_subscription_timestamp(entitlement.renewal_date.as_deref()))
}

fn extract_accounts_check_has_subscription(entry: &AccountsCheckEntry) -> Option<bool> {
    entry
        .entitlement
        .as_ref()
        .and_then(|value| value.has_active_subscription)
        .or_else(|| {
            entry.account.as_ref().and_then(|value| {
                value
                    .has_subscription
                    .or(value.has_active_subscription)
                    .or(value.is_paid_subscription_active)
            })
        })
}

fn build_accounts_check_snapshot(entry: &AccountsCheckEntry) -> AccountSubscriptionSnapshot {
    let account_plan_type = extract_accounts_check_plan_type(entry);
    let plan_type =
        extract_accounts_check_subscription_plan(entry).or_else(|| account_plan_type.clone());
    let expires_at = extract_accounts_check_expires_at(entry);
    let renews_at = extract_accounts_check_renews_at(entry).or_else(|| {
        if entry
            .entitlement
            .as_ref()
            .and_then(|value| value.will_renew)
            .unwrap_or(false)
        {
            expires_at
        } else {
            None
        }
    });
    let has_subscription = extract_accounts_check_has_subscription(entry).unwrap_or_else(|| {
        account_plan_type
            .as_deref()
            .is_some_and(|value| value != "free")
            || plan_type.as_deref().is_some_and(|value| value != "free")
            || expires_at.is_some()
            || renews_at.is_some()
    });

    AccountSubscriptionSnapshot {
        has_subscription,
        account_plan_type,
        plan_type,
        expires_at,
        renews_at,
    }
}

fn parse_subscription_timestamp(value: Option<&str>) -> Option<i64> {
    let text = value?.trim();
    if text.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|timestamp| timestamp.timestamp())
}

/// 函数 `usage_http_client`
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
pub(crate) fn usage_http_client() -> Client {
    let lock = USAGE_HTTP_CLIENT.get_or_init(|| RwLock::new(build_usage_http_client()));
    crate::lock_utils::read_recover(lock, "usage_http_client").clone()
}

fn subscription_http_client() -> Client {
    let lock =
        SUBSCRIPTION_HTTP_CLIENT.get_or_init(|| RwLock::new(build_subscription_http_client()));
    crate::lock_utils::read_recover(lock, "subscription_http_client").clone()
}

/// 函数 `rebuild_usage_http_client`
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
fn rebuild_usage_http_client() {
    let next = build_usage_http_client();
    let lock = USAGE_HTTP_CLIENT.get_or_init(|| RwLock::new(next.clone()));
    let mut current = crate::lock_utils::write_recover(lock, "usage_http_client");
    *current = next;
}

fn rebuild_subscription_http_client() {
    let next = build_subscription_http_client();
    let lock = SUBSCRIPTION_HTTP_CLIENT.get_or_init(|| RwLock::new(next.clone()));
    let mut current = crate::lock_utils::write_recover(lock, "subscription_http_client");
    *current = next;
}

/// 函数 `reload_usage_http_client_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn reload_usage_http_client_from_env() {
    rebuild_usage_http_client();
    rebuild_subscription_http_client();
}

/// 函数 `current_upstream_proxy_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn current_upstream_proxy_url() -> Option<String> {
    std::env::var(ENV_UPSTREAM_PROXY_URL)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 函数 `fetch_usage_snapshot`
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
pub(crate) fn fetch_usage_snapshot(
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    run_usage_future(fetch_usage_snapshot_async(base_url, bearer, workspace_id))
}

/// 函数 `fetch_account_subscription`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// - base_url: 参数 base_url
/// - bearer: 参数 bearer
/// - account_id: 参数 account_id
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn fetch_account_subscription(
    base_url: &str,
    bearer: &str,
    account_id: &str,
    workspace_id: Option<&str>,
) -> Result<AccountSubscriptionSnapshot, String> {
    run_usage_future(fetch_account_subscription_async(
        base_url,
        bearer,
        account_id,
        workspace_id,
    ))
}

/// 函数 `fetch_usage_snapshot_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - base_url: 参数 base_url
/// - bearer: 参数 bearer
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
async fn fetch_usage_snapshot_async(
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    // 调用上游用量接口
    let url = usage_endpoint(base_url);
    let build_request = || {
        let client = usage_http_client();
        let mut req = client
            .get(&url)
            .header("Authorization", format!("Bearer {bearer}"));
        let request_headers = build_usage_request_headers(workspace_id);
        if !request_headers.is_empty() {
            req = req.headers(request_headers);
        }
        req
    };
    let resp = match build_request().send().await {
        Ok(resp) => resp,
        Err(first_err) => {
            // 中文注释：代理在程序启动后才开启时，旧 client 可能沿用旧网络状态；这里自动重建并重试一次。
            rebuild_usage_http_client();
            let retried = build_request().send().await;
            match retried {
                Ok(resp) => resp,
                Err(second_err) => {
                    return Err(format!(
                        "{}; retry_after_client_rebuild: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_response_text(resp, USAGE_HTTP_TOTAL_TIMEOUT).await?;
        return Err(summarize_usage_error_response(
            status, &headers, &body, false,
        ));
    }
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if crate::gateway::is_html_content_type(content_type) {
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_response_text(resp, USAGE_HTTP_TOTAL_TIMEOUT).await?;
        return Err(summarize_usage_error_response(
            status, &headers, &body, true,
        ));
    }
    read_response_json(resp, USAGE_HTTP_TOTAL_TIMEOUT)
        .await
        .map_err(|e| format!("read usage endpoint json failed: {e}"))
}

async fn fetch_accounts_check_response_async(
    base_url: &str,
    bearer: &str,
) -> Result<AccountsCheckResponse, String> {
    let url = accounts_check_endpoint(base_url);
    let build_request = || {
        let client = subscription_http_client();
        client
            .get(&url)
            .header("Authorization", format!("Bearer {bearer}"))
            .header("Origin", "https://chatgpt.com")
            .header("Referer", "https://chatgpt.com/")
            .header("Accept", "application/json")
    };
    let resp = match build_request().send().await {
        Ok(resp) => resp,
        Err(first_err) => {
            rebuild_subscription_http_client();
            let retried = build_request().send().await;
            match retried {
                Ok(resp) => resp,
                Err(second_err) => {
                    return Err(format!(
                        "{}; retry_after_client_rebuild: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(AccountsCheckResponse {
            accounts: HashMap::new(),
        });
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_response_text(resp, USAGE_HTTP_TOTAL_TIMEOUT).await?;
        return Err(summarize_subscription_error_response(
            status, &headers, &body, false,
        ));
    }
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if crate::gateway::is_html_content_type(content_type) {
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_response_text(resp, USAGE_HTTP_TOTAL_TIMEOUT).await?;
        return Err(summarize_subscription_error_response(
            status, &headers, &body, true,
        ));
    }

    read_response_json(resp, USAGE_HTTP_TOTAL_TIMEOUT)
        .await
        .map_err(|e| format!("read accounts/check json failed: {e}"))
}

/// 函数 `fetch_account_subscription_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// - base_url: 参数 base_url
/// - bearer: 参数 bearer
/// - account_id: 参数 account_id
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
async fn fetch_account_subscription_async(
    base_url: &str,
    bearer: &str,
    account_id: &str,
    _workspace_id: Option<&str>,
) -> Result<AccountSubscriptionSnapshot, String> {
    let normalized_account_id = account_id.trim();
    if normalized_account_id.is_empty() {
        return Ok(AccountSubscriptionSnapshot::default());
    }
    let response = fetch_accounts_check_response_async(base_url, bearer).await?;

    if let Some(entry) = response.accounts.get(normalized_account_id) {
        return Ok(build_accounts_check_snapshot(entry));
    }

    let mut default_snapshot = None;
    let mut paid_snapshot = None;
    let mut any_snapshot = None;
    for entry in response.accounts.values() {
        let snapshot = build_accounts_check_snapshot(entry);
        if any_snapshot.is_none() {
            any_snapshot = Some(snapshot.clone());
        }
        if default_snapshot.is_none()
            && entry
                .account
                .as_ref()
                .and_then(|value| value.is_default)
                .unwrap_or(false)
        {
            default_snapshot = Some(snapshot.clone());
        }
        if paid_snapshot.is_none()
            && snapshot
                .account_plan_type
                .as_deref()
                .is_some_and(|value| value != "free")
        {
            paid_snapshot = Some(snapshot);
        }
    }

    Ok(default_snapshot
        .or(paid_snapshot)
        .or(any_snapshot)
        .unwrap_or_default())
}

/// 函数 `refresh_access_token`
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
pub(crate) fn refresh_access_token(
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<RefreshTokenResponse, String> {
    run_usage_future(refresh_access_token_async(issuer, client_id, refresh_token))
}

/// 函数 `refresh_access_token_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
/// - refresh_token: 参数 refresh_token
///
/// # 返回
/// 返回函数执行结果
async fn refresh_access_token_async(
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<RefreshTokenResponse, String> {
    let refresh_token_url = resolve_refresh_token_url(issuer);
    let body = build_refresh_token_body(client_id, refresh_token);
    let build_request = || {
        let client = usage_http_client();
        client
            .post(refresh_token_url.clone())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.clone())
    };
    let resp = match build_request().send().await {
        Ok(resp) => resp,
        Err(first_err) => {
            rebuild_usage_http_client();
            let retried = build_request().send().await;
            match retried {
                Ok(resp) => resp,
                Err(second_err) => {
                    return Err(format!(
                        "{}; retry_after_client_rebuild: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_response_text(resp, USAGE_HTTP_TOTAL_TIMEOUT).await?;
        return Err(format_refresh_token_status_error_with_headers(
            status,
            Some(&headers),
            body.as_str(),
        ));
    }
    read_response_json(resp, USAGE_HTTP_TOTAL_TIMEOUT)
        .await
        .map_err(|e| format!("read refresh token response json failed: {e}"))
}

/// 函数 `read_response_text`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
/// - timeout: 参数 timeout
///
/// # 返回
/// 返回函数执行结果
async fn read_response_text(resp: reqwest::Response, timeout: Duration) -> Result<String, String> {
    match tokio::time::timeout(timeout, resp.text()).await {
        Ok(Ok(body)) => Ok(body),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "response read timed out after {}ms",
            timeout.as_millis()
        )),
    }
}

/// 函数 `read_response_json`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
/// - timeout: 参数 timeout
///
/// # 返回
/// 返回函数执行结果
async fn read_response_json<T>(resp: reqwest::Response, timeout: Duration) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    match tokio::time::timeout(timeout, resp.json::<T>()).await {
        Ok(Ok(body)) => Ok(body),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "response read timed out after {}ms",
            timeout.as_millis()
        )),
    }
}

/// 函数 `build_refresh_token_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - client_id: 参数 client_id
/// - refresh_token: 参数 refresh_token
///
/// # 返回
/// 返回函数执行结果
fn build_refresh_token_body(client_id: &str, refresh_token: &str) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("client_id", client_id);
    serializer.append_pair("grant_type", "refresh_token");
    serializer.append_pair("refresh_token", refresh_token);
    serializer.append_pair("scope", REFRESH_TOKEN_SCOPES);
    serializer.finish()
}

#[cfg(test)]
#[path = "tests/usage_http_tests.rs"]
mod tests;
