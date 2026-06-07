use codexmanager_core::auth::{
    device_redirect_uri, device_token_url, device_usercode_url, extract_chatgpt_account_id,
    extract_workspace_id, normalize_chatgpt_account_id, normalize_workspace_id,
    parse_id_token_claims, token_exchange_body_authorization_code,
    token_exchange_body_token_exchange, IdTokenClaims, DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
};
use codexmanager_core::storage::{now_ts, Account, Token};
use reqwest::header::HeaderMap;
use reqwest::Client;
use reqwest::Error as ReqwestError;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::future::Future;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

use crate::account_identity::{
    build_account_storage_id, build_fallback_subject_key, clean_value,
    pick_existing_account_id_by_identity,
};
use crate::auth_callback::resolve_redirect_uri;
use crate::storage_helpers::open_storage;

static OPENAI_AUTH_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static OPENAI_AUTH_RUNTIME: OnceLock<Runtime> = OnceLock::new();
const OPENAI_AUTH_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const OPENAI_AUTH_READ_TIMEOUT: Duration = Duration::from_secs(30);
const OPENAI_AUTH_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
const ACCOUNT_SORT_STEP: i64 = 5;
const REQUEST_ID_HEADER: &str = "x-request-id";
const OAI_REQUEST_ID_HEADER: &str = "x-oai-request-id";
const CF_RAY_HEADER: &str = "cf-ray";
const AUTH_ERROR_HEADER: &str = "x-openai-authorization-error";
const CLOUDFLARE_BLOCKED_MESSAGE: &str =
    "Access blocked by Cloudflare. This usually happens when connecting from a restricted region";

/// 函数 `auth_runtime`
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
fn auth_runtime() -> &'static Runtime {
    OPENAI_AUTH_RUNTIME.get_or_init(|| {
        Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("auth-http")
            .build()
            .unwrap_or_else(|err| panic!("build auth runtime failed: {err}"))
    })
}

/// 函数 `run_auth_future`
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
fn run_auth_future<F>(future: F) -> F::Output
where
    F: Future,
{
    auth_runtime().block_on(future)
}

/// 函数 `read_json_with_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
/// - read_timeout: 参数 read_timeout
///
/// # 返回
/// 返回函数执行结果
async fn read_json_with_timeout<T>(
    resp: reqwest::Response,
    read_timeout: Duration,
) -> Result<T, String>
where
    T: DeserializeOwned + Send + 'static,
{
    match tokio::time::timeout(read_timeout, resp.json::<T>()).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "response read timed out after {}ms",
            read_timeout.as_millis()
        )),
    }
}

/// 函数 `read_text_with_timeout`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
/// - read_timeout: 参数 read_timeout
///
/// # 返回
/// 返回函数执行结果
async fn read_text_with_timeout(
    resp: reqwest::Response,
    read_timeout: Duration,
) -> Result<String, String> {
    match tokio::time::timeout(read_timeout, resp.text()).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!(
            "response read timed out after {}ms",
            read_timeout.as_millis()
        )),
    }
}

/// 函数 `summarize_token_endpoint_error_body`
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
fn summarize_token_endpoint_error_body(body: &str) -> String {
    parse_token_endpoint_error(body).to_string()
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

/// 函数 `build_token_endpoint_debug_suffix`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn build_token_endpoint_debug_suffix(headers: &HeaderMap) -> String {
    let request_id = extract_response_header(headers, REQUEST_ID_HEADER)
        .or_else(|| extract_response_header(headers, OAI_REQUEST_ID_HEADER));
    let cf_ray = extract_response_header(headers, CF_RAY_HEADER);
    let auth_error = extract_response_header(headers, AUTH_ERROR_HEADER);
    let identity_error_code = crate::gateway::extract_identity_error_code_from_headers(headers);

    let mut details = Vec::new();
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

    if details.is_empty() {
        String::new()
    } else {
        format!(" [{}]", details.join(", "))
    }
}

/// 函数 `classify_token_endpoint_error_kind`
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
fn classify_token_endpoint_error_kind(body: &str) -> &'static str {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "empty";
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return "json";
    }
    let normalized = trimmed.to_ascii_lowercase();
    if normalized.contains("<html") || normalized.contains("<!doctype html") {
        if normalized.contains("cloudflare") && normalized.contains("blocked") {
            "cloudflare_blocked"
        } else if normalized.contains("cloudflare")
            || normalized.contains("just a moment")
            || normalized.contains("attention required")
        {
            "cloudflare_challenge"
        } else {
            "html"
        }
    } else {
        "non_json"
    }
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

/// 函数 `classify_token_endpoint_error_kind_with_headers`
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
fn classify_token_endpoint_error_kind_with_headers(
    headers: &HeaderMap,
    body: &str,
) -> &'static str {
    let body_kind = classify_token_endpoint_error_kind(body);
    if !matches!(body_kind, "empty" | "non_json") {
        return body_kind;
    }

    if extract_response_header(headers, AUTH_ERROR_HEADER)
        .as_deref()
        .is_some_and(looks_like_blocked_marker)
        || crate::gateway::extract_identity_error_code_from_headers(headers)
            .as_deref()
            .is_some_and(looks_like_blocked_marker)
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

    body_kind
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenEndpointErrorDetail {
    error_code: Option<String>,
    error_message: Option<String>,
    display_message: String,
}

impl std::fmt::Display for TokenEndpointErrorDetail {
    /// 函数 `fmt`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - f: 参数 f
    ///
    /// # 返回
    /// 返回函数执行结果
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.display_message.fmt(f)
    }
}

const REDACTED_URL_VALUE: &str = "<redacted>";
const SENSITIVE_URL_QUERY_KEYS: &[&str] = &[
    "access_token",
    "api_key",
    "client_secret",
    "code",
    "code_verifier",
    "id_token",
    "key",
    "refresh_token",
    "requested_token",
    "state",
    "subject_token",
    "token",
];

/// 函数 `extract_html_title`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn extract_html_title(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let start = lower.find("<title>")?;
    let end = lower[start + 7..].find("</title>")? + start + 7;
    let title = raw.get(start + 7..end)?.trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

/// 函数 `summarize_html_error_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn summarize_html_error_body(raw: &str) -> String {
    let normalized = raw.to_ascii_lowercase();
    let looks_like_blocked = normalized.contains("cloudflare") && normalized.contains("blocked");
    let looks_like_challenge = normalized.contains("cloudflare")
        || normalized.contains("just a moment")
        || normalized.contains("attention required");
    let looks_like_html = normalized.contains("<html")
        || normalized.contains("<!doctype html")
        || normalized.contains("</html>");
    if !looks_like_html {
        return raw.trim().to_string();
    }

    if looks_like_blocked {
        return CLOUDFLARE_BLOCKED_MESSAGE.to_string();
    }

    let title = extract_html_title(raw);
    if looks_like_challenge {
        return match title {
            Some(title) => format!("Cloudflare 安全验证页（title={title}）"),
            None => "Cloudflare 安全验证页".to_string(),
        };
    }

    match title {
        Some(title) => format!("上游返回 HTML 错误页（title={title}）"),
        None => "上游返回 HTML 错误页".to_string(),
    }
}

/// 函数 `redact_sensitive_query_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - key: 参数 key
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn redact_sensitive_query_value(key: &str, value: &str) -> String {
    if SENSITIVE_URL_QUERY_KEYS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(key))
    {
        REDACTED_URL_VALUE.to_string()
    } else {
        value.to_string()
    }
}

/// 函数 `redact_sensitive_url_parts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - url: 参数 url
///
/// # 返回
/// 无
fn redact_sensitive_url_parts(url: &mut url::Url) {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_fragment(None);

    let query_pairs = url
        .query_pairs()
        .map(|(key, value)| {
            let key = key.into_owned();
            let value = value.into_owned();
            (key.clone(), redact_sensitive_query_value(&key, &value))
        })
        .collect::<Vec<_>>();

    if query_pairs.is_empty() {
        url.set_query(None);
        return;
    }

    let redacted_query = query_pairs
        .into_iter()
        .fold(
            url::form_urlencoded::Serializer::new(String::new()),
            |mut serializer, (key, value)| {
                serializer.append_pair(&key, &value);
                serializer
            },
        )
        .finish();
    url.set_query(Some(&redacted_query));
}

/// 函数 `redact_sensitive_error_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
fn redact_sensitive_error_url(mut err: ReqwestError) -> ReqwestError {
    if let Some(url) = err.url_mut() {
        redact_sensitive_url_parts(url);
    }
    err
}

/// 函数 `parse_token_endpoint_error`
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
pub(crate) fn parse_token_endpoint_error(body: &str) -> TokenEndpointErrorDetail {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return TokenEndpointErrorDetail {
            error_code: None,
            error_message: None,
            display_message: "unknown error".to_string(),
        };
    }

    let parsed = serde_json::from_str::<serde_json::Value>(trimmed).ok();
    if let Some(json) = parsed {
        let error_code = json
            .get("error")
            .and_then(serde_json::Value::as_str)
            .filter(|error_code| !error_code.trim().is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                json.get("error")
                    .and_then(serde_json::Value::as_object)
                    .and_then(|error_obj| error_obj.get("code"))
                    .and_then(serde_json::Value::as_str)
                    .filter(|code| !code.trim().is_empty())
                    .map(ToString::to_string)
            });
        if let Some(description) = json
            .get("error_description")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return TokenEndpointErrorDetail {
                error_code,
                error_message: Some(description.to_string()),
                display_message: description.to_string(),
            };
        }
        if let Some(message) = json
            .get("error")
            .and_then(serde_json::Value::as_object)
            .and_then(|error_obj| error_obj.get("message"))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return TokenEndpointErrorDetail {
                error_code,
                error_message: Some(message.to_string()),
                display_message: message.to_string(),
            };
        }
        if let Some(message) = json
            .get("message")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            return TokenEndpointErrorDetail {
                error_code,
                error_message: Some(message.to_string()),
                display_message: message.to_string(),
            };
        }
        if let Some(error_code) = error_code {
            return TokenEndpointErrorDetail {
                display_message: error_code.clone(),
                error_code: Some(error_code),
                error_message: None,
            };
        }
    }

    TokenEndpointErrorDetail {
        error_code: None,
        error_message: None,
        display_message: summarize_html_error_body(trimmed),
    }
}

/// 函数 `summarize_header_only_token_endpoint_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn summarize_header_only_token_endpoint_error(headers: &HeaderMap) -> Option<String> {
    if let Some(auth_error) = extract_response_header(headers, AUTH_ERROR_HEADER) {
        if looks_like_blocked_marker(&auth_error) {
            return Some(CLOUDFLARE_BLOCKED_MESSAGE.to_string());
        }
        return Some(format!("authorization error: {auth_error}"));
    }

    if let Some(identity_error_code) =
        crate::gateway::extract_identity_error_code_from_headers(headers)
    {
        if looks_like_blocked_marker(&identity_error_code) {
            return Some(CLOUDFLARE_BLOCKED_MESSAGE.to_string());
        }
        return Some(format!("identity error: {identity_error_code}"));
    }

    None
}

/// 函数 `resolve_token_endpoint_error_detail`
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
fn resolve_token_endpoint_error_detail(headers: &HeaderMap, body: &str) -> String {
    if !body.trim().is_empty() {
        return parse_token_endpoint_error(body).to_string();
    }

    summarize_header_only_token_endpoint_error(headers)
        .unwrap_or_else(|| parse_token_endpoint_error(body).to_string())
}

/// 函数 `format_token_endpoint_status_error`
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
fn format_token_endpoint_status_error(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> String {
    let detail = resolve_token_endpoint_error_detail(headers, body);
    let suffix = {
        let mut suffix = build_token_endpoint_debug_suffix(headers);
        let kind = classify_token_endpoint_error_kind_with_headers(headers, body);
        if kind != "json" {
            let addition = format!("kind={kind}");
            if suffix.is_empty() {
                suffix = format!(" [{addition}]");
            } else {
                suffix.insert_str(suffix.len() - 1, &format!(", {addition}"));
            }
        }
        suffix
    };
    format!("token endpoint returned status {status}: {detail}{suffix}")
}

/// 函数 `format_api_key_exchange_status_error`
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
fn format_api_key_exchange_status_error(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: &str,
) -> String {
    let detail = if body.trim().is_empty() {
        summarize_header_only_token_endpoint_error(headers)
            .unwrap_or_else(|| summarize_token_endpoint_error_body(body))
    } else {
        summarize_token_endpoint_error_body(body)
    };
    let suffix = {
        let mut suffix = build_token_endpoint_debug_suffix(headers);
        let kind = classify_token_endpoint_error_kind_with_headers(headers, body);
        if kind != "json" {
            let addition = format!("kind={kind}");
            if suffix.is_empty() {
                suffix = format!(" [{addition}]");
            } else {
                suffix.insert_str(suffix.len() - 1, &format!(", {addition}"));
            }
        }
        suffix
    };
    format!("api key exchange failed with status {status}: {detail}{suffix}")
}

/// 函数 `next_account_sort`
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
pub(crate) fn next_account_sort(storage: &codexmanager_core::storage::Storage) -> i64 {
    storage
        .list_accounts()
        .ok()
        .and_then(|accounts| accounts.into_iter().map(|account| account.sort).max())
        .map(|sort| sort.saturating_add(ACCOUNT_SORT_STEP))
        .unwrap_or(0)
}

/// 函数 `openai_auth_http_client`
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
fn openai_auth_http_client() -> &'static Client {
    OPENAI_AUTH_HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(OPENAI_AUTH_CONNECT_TIMEOUT)
            .timeout(OPENAI_AUTH_TOTAL_TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

/// 函数 `issuer_uses_loopback_host`
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
pub(crate) fn issuer_uses_loopback_host(issuer: &str) -> bool {
    url::Url::parse(issuer)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
}

/// 函数 `auth_http_client_for_issuer`
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
fn auth_http_client_for_issuer(issuer: &str) -> Client {
    if issuer_uses_loopback_host(issuer) {
        return Client::builder()
            .connect_timeout(OPENAI_AUTH_CONNECT_TIMEOUT)
            .timeout(OPENAI_AUTH_TOTAL_TIMEOUT)
            .no_proxy()
            .build()
            .unwrap_or_else(|_| Client::new());
    }

    openai_auth_http_client().clone()
}

#[derive(serde::Deserialize)]
struct DeviceUserCodeResponse {
    device_auth_id: String,
    #[serde(alias = "user_code", alias = "usercode")]
    user_code: String,
    #[serde(
        default = "default_device_poll_interval",
        deserialize_with = "deserialize_interval"
    )]
    interval: u64,
}

/// 函数 `default_device_poll_interval`
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
fn default_device_poll_interval() -> u64 {
    5
}

/// 函数 `deserialize_interval`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - deserializer: 参数 deserializer
///
/// # 返回
/// 返回函数执行结果
fn deserialize_interval<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("invalid interval value")),
        serde_json::Value::String(value) => value
            .trim()
            .parse::<u64>()
            .map_err(serde::de::Error::custom),
        serde_json::Value::Null => Ok(default_device_poll_interval()),
        other => Err(serde::de::Error::custom(format!(
            "invalid interval value: {other}"
        ))),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DeviceCodeStartResult {
    pub(crate) verification_url: String,
    pub(crate) user_code: String,
    pub(crate) device_auth_id: String,
    pub(crate) interval: u64,
}

#[derive(serde::Deserialize)]
struct DeviceAuthTokenResponse {
    authorization_code: String,
    #[serde(rename = "code_challenge")]
    _code_challenge: String,
    code_verifier: String,
}

/// 函数 `request_device_code`
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
pub(crate) fn request_device_code(
    issuer: &str,
    client_id: &str,
) -> Result<DeviceCodeStartResult, String> {
    run_auth_future(request_device_code_async(issuer, client_id))
}

/// 函数 `request_device_code_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
///
/// # 返回
/// 返回函数执行结果
async fn request_device_code_async(
    issuer: &str,
    client_id: &str,
) -> Result<DeviceCodeStartResult, String> {
    let client = auth_http_client_for_issuer(issuer);
    let url = device_usercode_url(issuer);
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(serde_json::json!({ "client_id": client_id }).to_string())
        .send()
        .await
        .map_err(|err| err.to_string())?;

    if !resp.status().is_success() {
        if resp.status() == StatusCode::NOT_FOUND {
            return Err(
                "device code login is not enabled for this Codex server. Use the browser login or verify the server URL."
                    .to_string(),
            );
        }
        return Err(format!(
            "device code request failed with status {}",
            resp.status()
        ));
    }

    let body: DeviceUserCodeResponse =
        read_json_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT).await?;
    Ok(DeviceCodeStartResult {
        verification_url: codexmanager_core::auth::device_verification_url(issuer),
        user_code: body.user_code,
        device_auth_id: body.device_auth_id,
        interval: body.interval,
    })
}

/// 函数 `spawn_device_code_login_completion`
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
pub(crate) fn spawn_device_code_login_completion(
    issuer: String,
    login_id: String,
    device_code: DeviceCodeStartResult,
) {
    let suffix = login_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>();
    let thread_name = if suffix.is_empty() {
        "device-login".to_string()
    } else {
        format!("device-login-{suffix}")
    };
    let _ = thread::Builder::new().name(thread_name).spawn(move || {
        if let Err(err) = complete_device_code_login(issuer, login_id.clone(), device_code) {
            if let Some(storage) = open_storage() {
                let _ = storage.update_login_session_status(&login_id, "failed", Some(&err));
            }
        }
    });
}

/// 函数 `poll_device_auth_token_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
/// - device_auth_id: 参数 device_auth_id
/// - user_code: 参数 user_code
/// - interval: 参数 interval
///
/// # 返回
/// 返回函数执行结果
async fn poll_device_auth_token_async(
    issuer: &str,
    device_auth_id: &str,
    user_code: &str,
    interval: u64,
) -> Result<DeviceAuthTokenResponse, String> {
    let client = auth_http_client_for_issuer(issuer);
    let url = device_token_url(issuer);
    let max_wait = Duration::from_secs(15 * 60);
    let started_at = tokio::time::Instant::now();

    loop {
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(
                serde_json::json!({
                    "device_auth_id": device_auth_id,
                    "user_code": user_code,
                })
                .to_string(),
            )
            .send()
            .await
            .map_err(|err| err.to_string())?;

        if resp.status().is_success() {
            return read_json_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT).await;
        }

        if matches!(resp.status(), StatusCode::FORBIDDEN | StatusCode::NOT_FOUND) {
            let elapsed = started_at.elapsed();
            if elapsed >= max_wait {
                return Err("device auth timed out after 15 minutes".to_string());
            }
            let remaining = max_wait.saturating_sub(elapsed);
            let sleep_for = Duration::from_secs(interval).min(remaining);
            tokio::time::sleep(sleep_for).await;
            continue;
        }

        let status = resp.status();
        let headers = resp.headers().clone();
        let body = read_text_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT)
            .await
            .unwrap_or_default();
        return Err(format_token_endpoint_status_error(status, &headers, &body));
    }
}

/// 函数 `complete_device_code_login`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
/// - login_id: 参数 login_id
/// - device_code: 参数 device_code
///
/// # 返回
/// 返回函数执行结果
fn complete_device_code_login(
    issuer: String,
    login_id: String,
    device_code: DeviceCodeStartResult,
) -> Result<(), String> {
    let code = run_auth_future(poll_device_auth_token_async(
        &issuer,
        &device_code.device_auth_id,
        &device_code.user_code,
        device_code.interval,
    ))?;

    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .update_login_session_code_verifier(&login_id, &code.code_verifier)
        .map_err(|err| err.to_string())?;
    complete_login_with_redirect(
        &login_id,
        &code.authorization_code,
        Some(&device_redirect_uri(&issuer)),
    )
}

/// 函数 `complete_login`
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
pub(crate) fn complete_login(state: &str, code: &str) -> Result<(), String> {
    complete_login_with_redirect(state, code, None)
}

/// 函数 `complete_login_with_redirect`
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
pub(crate) fn complete_login_with_redirect(
    state: &str,
    code: &str,
    redirect_uri: Option<&str>,
) -> Result<(), String> {
    // 读取登录会话
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let session = storage
        .get_login_session(state)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "unknown login session".to_string())?;

    // 读取 OAuth 配置
    let issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let redirect_uri = redirect_uri
        .map(|value| value.to_string())
        .or_else(|| resolve_redirect_uri())
        .unwrap_or_else(|| "http://localhost:1455/auth/callback".to_string());

    // 交换授权码获取 token
    let tokens = exchange_code_for_tokens(
        &issuer,
        &client_id,
        &redirect_uri,
        &session.code_verifier,
        code,
    )
    .map_err(|e| {
        let _ = storage.update_login_session_status(state, "failed", Some(&e));
        e
    })?;

    // 可选兑换平台 key
    let api_key_access_token = obtain_api_key(&issuer, &client_id, &tokens.id_token).ok();
    let claims = parse_id_token_claims(&tokens.id_token).map_err(|e| {
        let _ = storage.update_login_session_status(state, "failed", Some(&e));
        e
    })?;
    if let Err(e) = ensure_workspace_allowed(
        session.workspace_id.as_deref(),
        &claims,
        &tokens.id_token,
        &tokens.access_token,
    ) {
        let _ = storage.update_login_session_status(state, "failed", Some(&e));
        return Err(e);
    }

    // 生成账户记录
    let subject_account_id = claims.sub.clone();
    let label = claims
        .email
        .clone()
        .unwrap_or_else(|| subject_account_id.clone());
    let claim_chatgpt_account_id = claims
        .auth
        .as_ref()
        .and_then(|auth| normalize_chatgpt_account_id(auth.chatgpt_account_id.as_deref()));
    let claim_workspace_id = normalize_workspace_id(claims.workspace_id.as_deref());
    let chatgpt_account_id = clean_value(
        claim_chatgpt_account_id
            .or_else(|| extract_chatgpt_account_id(&tokens.id_token))
            .or_else(|| extract_chatgpt_account_id(&tokens.access_token)),
    );
    let workspace_id = clean_value(
        claim_workspace_id
            .or_else(|| extract_workspace_id(&tokens.id_token))
            .or_else(|| extract_workspace_id(&tokens.access_token))
            .or_else(|| chatgpt_account_id.clone()),
    );
    let fallback_subject_key =
        build_fallback_subject_key(Some(&subject_account_id), session.tags.as_deref());
    let account_storage_id = build_account_storage_id(
        &subject_account_id,
        chatgpt_account_id.as_deref(),
        workspace_id.as_deref(),
        session.tags.as_deref(),
    );
    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let account_key = pick_existing_account_id_by_identity(
        accounts.iter(),
        chatgpt_account_id.as_deref(),
        workspace_id.as_deref(),
        fallback_subject_key.as_deref(),
        None,
    )
    .unwrap_or(account_storage_id);
    let now = now_ts();
    let existing_account = storage
        .find_account_by_id(&account_key)
        .map_err(|e| e.to_string())?;
    let sort = existing_account
        .as_ref()
        .map(|account| account.sort)
        .unwrap_or_else(|| next_account_sort(&storage));
    let created_at = existing_account
        .as_ref()
        .map(|account| account.created_at)
        .unwrap_or(now);
    let workspace_id_for_log = workspace_id.clone();
    let chatgpt_account_id_for_log = chatgpt_account_id.clone();
    let account = Account {
        id: account_key.clone(),
        label,
        issuer: issuer.clone(),
        chatgpt_account_id,
        workspace_id,
        group_name: session.group_name.clone(),
        sort,
        status: "active".to_string(),
        created_at,
        updated_at: now,
    };
    storage
        .insert_account(&account)
        .map_err(|e| e.to_string())?;
    storage
        .upsert_account_metadata(
            &account_key,
            session.note.as_deref(),
            session.tags.as_deref(),
        )
        .map_err(|e| e.to_string())?;

    // 写入 token
    let token = Token {
        account_id: account_key.clone(),
        id_token: tokens.id_token,
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        api_key_access_token,
        last_refresh: now,
    };
    storage.insert_token(&token).map_err(|e| e.to_string())?;

    let db_path = std::env::var("CODEXMANAGER_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    log::info!(
        "oauth login persisted account: db_path={} login_id={} account_id={} workspace_id={} chatgpt_account_id={} redirect_uri={}",
        db_path,
        state,
        account_key,
        workspace_id_for_log.as_deref().unwrap_or("-"),
        chatgpt_account_id_for_log.as_deref().unwrap_or("-"),
        redirect_uri
    );

    storage
        .update_login_session_status(state, "success", None)
        .map_err(|e| e.to_string())?;
    crate::auth_account::set_current_auth_account_id(Some(&account_key))?;
    crate::auth_account::set_current_auth_mode(Some("chatgpt"))?;
    let _ = crate::usage_refresh::enqueue_usage_refresh_after_account_add(&account_key);
    Ok(())
}

/// 函数 `build_exchange_code_request`
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
pub(crate) fn build_exchange_code_request(
    client: &Client,
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<reqwest::Request, String> {
    client
        .post(format!("{issuer}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(token_exchange_body_authorization_code(
            code,
            redirect_uri,
            client_id,
            code_verifier,
        ))
        .build()
        .map_err(|e| redact_sensitive_error_url(e).to_string())
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    id_token: String,
    access_token: String,
    refresh_token: String,
}

/// 函数 `exchange_code_for_tokens`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
/// - redirect_uri: 参数 redirect_uri
/// - code_verifier: 参数 code_verifier
/// - code: 参数 code
///
/// # 返回
/// 返回函数执行结果
fn exchange_code_for_tokens(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<TokenResponse, String> {
    run_auth_future(exchange_code_for_tokens_async(
        issuer,
        client_id,
        redirect_uri,
        code_verifier,
        code,
    ))
}

/// 函数 `exchange_code_for_tokens_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
/// - redirect_uri: 参数 redirect_uri
/// - code_verifier: 参数 code_verifier
/// - code: 参数 code
///
/// # 返回
/// 返回函数执行结果
async fn exchange_code_for_tokens_async(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> Result<TokenResponse, String> {
    // 请求 token 接口
    let client = auth_http_client_for_issuer(issuer);
    let request = build_exchange_code_request(
        &client,
        issuer,
        client_id,
        redirect_uri,
        code_verifier,
        code,
    )?;
    let resp = client
        .execute(request)
        .await
        .map_err(|e| redact_sensitive_error_url(e).to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let headers = resp.headers().clone();
        let message = read_text_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT)
            .await
            .map(|body| format_token_endpoint_status_error(status, &headers, &body))
            .unwrap_or_else(|_| {
                let suffix = build_token_endpoint_debug_suffix(&headers);
                format!("token endpoint returned status {status}: unknown error{suffix}")
            });
        return Err(message);
    }
    read_json_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT).await
}

/// 函数 `obtain_api_key`
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
pub(crate) fn obtain_api_key(
    issuer: &str,
    client_id: &str,
    id_token: &str,
) -> Result<String, String> {
    run_auth_future(obtain_api_key_async(issuer, client_id, id_token))
}

/// 函数 `obtain_api_key_async`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
/// - id_token: 参数 id_token
///
/// # 返回
/// 返回函数执行结果
async fn obtain_api_key_async(
    issuer: &str,
    client_id: &str,
    id_token: &str,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct ExchangeResp {
        access_token: String,
    }

    // 兑换平台 API Key
    let client = auth_http_client_for_issuer(issuer);
    let request = build_api_key_exchange_request(&client, issuer, client_id, id_token)?;
    let resp = client
        .execute(request)
        .await
        .map_err(|e| redact_sensitive_error_url(e).to_string())?;
    if !resp.status().is_success() {
        let status = resp.status();
        let headers = resp.headers().clone();
        let message = read_text_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT)
            .await
            .map(|body| format_api_key_exchange_status_error(status, &headers, &body))
            .unwrap_or_else(|_| {
                let suffix = build_token_endpoint_debug_suffix(&headers);
                format!("api key exchange failed with status {status}: unknown error{suffix}")
            });
        return Err(message);
    }
    let body: ExchangeResp = read_json_with_timeout(resp, OPENAI_AUTH_READ_TIMEOUT).await?;
    Ok(body.access_token)
}

/// 函数 `build_api_key_exchange_request`
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
pub(crate) fn build_api_key_exchange_request(
    client: &Client,
    issuer: &str,
    client_id: &str,
    id_token: &str,
) -> Result<reqwest::Request, String> {
    client
        .post(format!("{issuer}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(token_exchange_body_token_exchange(id_token, client_id))
        .build()
        .map_err(|e| redact_sensitive_error_url(e).to_string())
}

/// 函数 `ensure_workspace_allowed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - expected: 参数 expected
/// - claims: 参数 claims
/// - id_token: 参数 id_token
/// - access_token: 参数 access_token
///
/// # 返回
/// 返回函数执行结果
fn ensure_workspace_allowed(
    expected: Option<&str>,
    claims: &IdTokenClaims,
    id_token: &str,
    access_token: &str,
) -> Result<(), String> {
    let Some(expected_raw) = expected.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };

    let expected_workspace = normalize_workspace_id(Some(expected_raw));
    let expected_chatgpt = normalize_chatgpt_account_id(Some(expected_raw));
    let expected = expected_workspace
        .clone()
        .or(expected_chatgpt.clone())
        .ok_or_else(|| {
            "Login is restricted to a specific workspace, but the workspace id was invalid."
                .to_string()
        })?;

    let actual_workspace = normalize_workspace_id(claims.workspace_id.as_deref())
        .or_else(|| extract_workspace_id(id_token))
        .or_else(|| extract_workspace_id(access_token));
    let actual_chatgpt = claims
        .auth
        .as_ref()
        .and_then(|auth| normalize_chatgpt_account_id(auth.chatgpt_account_id.as_deref()))
        .or_else(|| extract_chatgpt_account_id(id_token))
        .or_else(|| extract_chatgpt_account_id(access_token));

    if actual_workspace.is_none() && actual_chatgpt.is_none() {
        return Err("Login is restricted to a specific workspace, but the token did not include a workspace claim.".to_string());
    }

    let workspace_matches = expected_workspace
        .as_ref()
        .zip(actual_workspace.as_ref())
        .is_some_and(|(expected, actual)| expected == actual);
    let chatgpt_matches = expected_chatgpt
        .as_ref()
        .zip(actual_chatgpt.as_ref())
        .is_some_and(|(expected, actual)| expected == actual);

    if workspace_matches || chatgpt_matches {
        Ok(())
    } else {
        Err(format!("Login is restricted to workspace id {expected}."))
    }
}

#[cfg(test)]
#[path = "tests/auth_tokens_tests.rs"]
mod tests;
