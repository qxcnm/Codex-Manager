use gpttools_core::storage::{now_ts, Account, RequestLog, Storage, Token};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::blocking::Client;
use reqwest::Method;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tiny_http::{Header, Request, Response, StatusCode};

use crate::auth_tokens;
use crate::storage_helpers::open_storage;
use gpttools_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};

mod local_validation;
mod upstream_proxy;
mod request_helpers;
mod request_rewrite;
mod metrics;
mod selection;
mod failover;
mod model_picker;
mod upstream_config;

pub(super) use request_helpers::{
    extract_request_model, extract_request_reasoning_effort, is_html_content_type,
    is_upstream_challenge_response, normalize_models_path, should_drop_incoming_header,
    should_drop_incoming_header_for_failover,
};
use request_rewrite::{apply_request_overrides, compute_upstream_url};
use upstream_config::{
    is_openai_api_base,
    resolve_upstream_base_url, resolve_upstream_fallback_base_url, should_try_openai_fallback,
    should_try_openai_fallback_by_status,
};
#[cfg(test)]
use upstream_config::normalize_upstream_base_url;
pub(crate) use metrics::{
    AccountInFlightGuard, acquire_account_inflight,
    begin_gateway_request, gateway_metrics_prometheus,
    account_inflight_count, record_gateway_cooldown_mark, record_gateway_failover_attempt,
};
pub(crate) use selection::{collect_gateway_candidates, rotate_candidates_for_fairness};
pub(crate) use failover::should_failover_after_refresh;
pub(crate) use model_picker::fetch_models_for_picker;

static UPSTREAM_CLIENT: OnceLock<Client> = OnceLock::new();
static ACCOUNT_COOLDOWN_UNTIL: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();
static ACCOUNT_TOKEN_EXCHANGE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> =
    OnceLock::new();

const DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_ACCOUNT_MAX_INFLIGHT: usize = 0;
const DEFAULT_ACCOUNT_COOLDOWN_SECS: i64 = 20;
const DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
const DEFAULT_ACCOUNT_COOLDOWN_429_SECS: i64 = 45;
const DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS: i64 = 30;
const DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
const DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS: i64 = 60;
const DEFAULT_MODELS_CLIENT_VERSION: &str = "0.98.0";
const DEFAULT_GATEWAY_DEBUG: bool = false;

fn upstream_client() -> &'static Client {
    UPSTREAM_CLIENT.get_or_init(|| {
        Client::builder()
            // 中文注释：显式关闭总超时，避免长时流式响应在客户端层被误判超时中断。
            .timeout(None::<Duration>)
            // 中文注释：连接阶段设置超时，避免网络异常时线程长期卡死占满并发槽位。
            .connect_timeout(upstream_connect_timeout())
            .pool_max_idle_per_host(32)
            .pool_idle_timeout(Some(Duration::from_secs(90)))
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

fn upstream_connect_timeout() -> Duration {
    Duration::from_secs(DEFAULT_UPSTREAM_CONNECT_TIMEOUT_SECS)
}


fn account_max_inflight_limit() -> usize {
    DEFAULT_ACCOUNT_MAX_INFLIGHT
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CooldownReason {
    Default,
    Network,
    RateLimited,
    Upstream5xx,
    Upstream4xx,
    Challenge,
}

fn cooldown_secs_for_reason(reason: CooldownReason) -> i64 {
    match reason {
        CooldownReason::Default => DEFAULT_ACCOUNT_COOLDOWN_SECS,
        CooldownReason::Network => DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS,
        CooldownReason::RateLimited => DEFAULT_ACCOUNT_COOLDOWN_429_SECS,
        CooldownReason::Upstream5xx => DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS,
        CooldownReason::Upstream4xx => DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS,
        CooldownReason::Challenge => DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS,
    }
}

fn cooldown_reason_for_status(status: u16) -> CooldownReason {
    match status {
        429 => CooldownReason::RateLimited,
        500..=599 => CooldownReason::Upstream5xx,
        401 | 403 => CooldownReason::Challenge,
        400..=499 => CooldownReason::Upstream4xx,
        _ => CooldownReason::Default,
    }
}

fn is_account_in_cooldown(account_id: &str) -> bool {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut map) = lock.lock() else {
        return false;
    };
    let now = now_ts();
    map.retain(|_, until| *until > now);
    map.get(account_id).copied().unwrap_or(0) > now
}

fn mark_account_cooldown(account_id: &str, reason: CooldownReason) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        record_gateway_cooldown_mark();
        let cooldown_until = now_ts() + cooldown_secs_for_reason(reason);
        // 中文注释：同账号短时间内可能触发不同失败类型；保留更晚的 until 可避免被较短冷却覆盖。
        match map.get_mut(account_id) {
            Some(until) => {
                if cooldown_until > *until {
                    *until = cooldown_until;
                }
            }
            None => {
                map.insert(account_id.to_string(), cooldown_until);
            }
        }
    }
}

fn mark_account_cooldown_for_status(account_id: &str, status: u16) {
    mark_account_cooldown(account_id, cooldown_reason_for_status(status));
}

fn clear_account_cooldown(account_id: &str) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        map.remove(account_id);
    }
}

fn account_token_exchange_lock(account_id: &str) -> Arc<Mutex<()>> {
    let lock = ACCOUNT_TOKEN_EXCHANGE_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut map) = lock.lock() else {
        return Arc::new(Mutex::new(()));
    };
    map.entry(account_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn find_cached_api_key_access_token(storage: &Storage, account_id: &str) -> Option<String> {
    storage
        .list_tokens()
        .ok()?
        .into_iter()
        .find(|t| t.account_id == account_id)
        .and_then(|t| t.api_key_access_token)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn resolve_openai_bearer_token(
    storage: &Storage,
    account: &Account,
    token: &mut Token,
) -> Result<String, String> {
    if let Some(existing) = token
        .api_key_access_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Ok(existing.to_string());
    }

    let exchange_lock = account_token_exchange_lock(&account.id);
    let _guard = exchange_lock
        .lock()
        .map_err(|_| "token exchange lock poisoned".to_string())?;

    if let Some(existing) = token
        .api_key_access_token
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Ok(existing.to_string());
    }

    if let Some(cached) = find_cached_api_key_access_token(storage, &account.id) {
        // 中文注释：并发下后到线程优先复用已落库的新 token，避免重复 token exchange 打上游。
        token.api_key_access_token = Some(cached.clone());
        return Ok(cached);
    }

    let client_id = std::env::var("GPTTOOLS_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let issuer_env = std::env::var("GPTTOOLS_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let issuer = if account.issuer.trim().is_empty() {
        issuer_env
    } else {
        account.issuer.clone()
    };
    let exchanged = auth_tokens::obtain_api_key(&issuer, &client_id, &token.id_token)?;
    token.api_key_access_token = Some(exchanged.clone());
    let _ = storage.insert_token(token);
    Ok(exchanged)
}

fn write_request_log(
    storage: &Storage,
    key_id: Option<&str>,
    request_path: &str,
    method: &str,
    model: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_url: Option<&str>,
    status_code: Option<u16>,
    error: Option<&str>,
) {
    // 记录每次网关转发结果，便于在 UI 里按模型/错误检索问题。
    let _ = storage.insert_request_log(&RequestLog {
        key_id: key_id.map(|v| v.to_string()),
        request_path: request_path.to_string(),
        method: method.to_string(),
        model: model.map(|v| v.to_string()),
        reasoning_effort: reasoning_effort.map(|v| v.to_string()),
        upstream_url: upstream_url.map(|v| v.to_string()),
        status_code: status_code.map(|v| i64::from(v)),
        error: error.map(|v| v.to_string()),
        created_at: now_ts(),
    });
}

pub(crate) fn handle_gateway_request(mut request: Request) -> Result<(), String> {
    // 处理代理请求（鉴权后转发到上游）
    let debug = DEFAULT_GATEWAY_DEBUG;
    if request.method().as_str() == "OPTIONS" {
        let response = Response::empty(204);
        let _ = request.respond(response);
        return Ok(());
    }

    if request.url() == "/health" {
        let response = Response::from_string("ok");
        let _ = request.respond(response);
        return Ok(());
    }

    let _request_guard = begin_gateway_request();
    let request_path_for_log = normalize_models_path(request.url());
    let request_method_for_log = request.method().as_str().to_string();
    let validated = match local_validation::prepare_local_request(&mut request, debug) {
        Ok(v) => v,
        Err(err) => {
            if let Some(storage) = open_storage() {
                write_request_log(
                    &storage,
                    None,
                    &request_path_for_log,
                    &request_method_for_log,
                    None,
                    None,
                    None,
                    Some(err.status_code),
                    Some(err.message.as_str()),
                );
            }
            let response = Response::from_string(err.message).with_status_code(err.status_code);
            let _ = request.respond(response);
            return Ok(());
        }
    };

    upstream_proxy::proxy_validated_request(request, validated, debug)
}

fn try_openai_fallback(
    client: &Client,
    storage: &gpttools_core::storage::Storage,
    method: &Method,
    request: &Request,
    body: &[u8],
    upstream_base: &str,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
) -> Result<Option<reqwest::blocking::Response>, String> {
    let path = normalize_models_path(request.url());
    let (url, _url_alt) = compute_upstream_url(upstream_base, &path);
    let bearer = resolve_openai_bearer_token(storage, account, token)?;

    let mut builder = client.request(method.clone(), &url);
    let mut has_user_agent = false;
    for header in request.headers() {
        let name = header.field.as_str().as_str();
        if if strip_session_affinity {
            should_drop_incoming_header_for_failover(name)
        } else {
            should_drop_incoming_header(name)
        } {
            continue;
        }
        if header.field.equiv("User-Agent") {
            has_user_agent = true;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.field.as_str().as_bytes()),
            HeaderValue::from_str(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    if !has_user_agent {
        builder = builder.header("User-Agent", "codex-cli");
    }
    if let Some(cookie) = upstream_cookie {
        if !cookie.trim().is_empty() {
            builder = builder.header("Cookie", cookie);
        }
    }
    if debug {
        eprintln!("gateway upstream: base={}, token_source=api_key_access_token", upstream_base);
    }
    builder = builder.header("Authorization", format!("Bearer {}", bearer));
    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }
    let resp = builder.send().map_err(|e| e.to_string())?;
    Ok(Some(resp))
}

fn extract_platform_key(request: &Request) -> Option<String> {
    // 从请求头提取平台 Key
    for header in request.headers() {
        if header.field.equiv("Authorization") {
            let value = header.value.as_str();
            if let Some(rest) = value.strip_prefix("Bearer ") {
                return Some(rest.trim().to_string());
            }
        }
        if header.field.equiv("x-api-key") {
            return Some(header.value.as_str().trim().to_string());
        }
    }
    None
}

fn respond_with_upstream(
    request: Request,
    upstream: reqwest::blocking::Response,
    _inflight_guard: AccountInFlightGuard,
) -> Result<(), String> {
    let status = StatusCode(upstream.status().as_u16());
    let mut headers = Vec::new();
    for (name, value) in upstream.headers().iter() {
        let name_str = name.as_str();
        if name_str.eq_ignore_ascii_case("transfer-encoding")
            || name_str.eq_ignore_ascii_case("content-length")
            || name_str.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let Ok(header) = Header::from_bytes(name_str.as_bytes(), value.as_bytes()) {
            headers.push(header);
        }
    }
    let len = upstream.content_length().map(|v| v as usize);
    let response = Response::new(status, headers, upstream, len, None);
    let _ = request.respond(response);
    Ok(())
}

#[cfg(test)]
mod availability_tests {
    use super::should_failover_after_refresh;
    use super::request_rewrite::{apply_request_overrides, compute_upstream_url};
    use super::{
        account_token_exchange_lock,
        cooldown_reason_for_status, gateway_metrics_prometheus, is_html_content_type,
        is_upstream_challenge_response, normalize_models_path, normalize_upstream_base_url,
        resolve_openai_bearer_token, should_drop_incoming_header,
        should_drop_incoming_header_for_failover, should_try_openai_fallback, CooldownReason,
    };
    use gpttools_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};
    use reqwest::header::HeaderValue;
    use std::sync::Arc;

    #[test]
    fn failover_on_missing_usage() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let account = Account {
            id: "acc-1".to_string(),
            label: "main".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            workspace_name: None,
            note: None,
            tags: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        };
        storage.insert_account(&account).expect("insert");
        let record = UsageSnapshotRecord {
            account_id: "acc-1".to_string(),
            used_percent: None,
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(10.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now_ts(),
        };
        storage.insert_usage_snapshot(&record).expect("insert usage");

        let should_failover = should_failover_after_refresh(&storage, "acc-1", Ok(()));
        assert!(should_failover);
    }

    #[test]
    fn html_content_type_detection() {
        assert!(is_html_content_type("text/html; charset=utf-8"));
        assert!(is_html_content_type("TEXT/HTML"));
        assert!(!is_html_content_type("application/json"));
    }

    #[test]
    fn compute_url_keeps_v1_for_models_on_codex_backend() {
        let (url, alt) = compute_upstream_url("https://chatgpt.com/backend-api/codex", "/v1/models");
        assert_eq!(url, "https://chatgpt.com/backend-api/codex/models");
        assert_eq!(
            alt.as_deref(),
            Some("https://chatgpt.com/backend-api/codex/v1/models")
        );
        let (url, alt) = compute_upstream_url("https://api.openai.com/v1", "/v1/models");
        assert_eq!(url, "https://api.openai.com/v1/models");
        assert!(alt.is_none());
    }

    #[test]
    fn normalize_upstream_base_url_for_chatgpt_host() {
        assert_eq!(
            normalize_upstream_base_url("https://chatgpt.com"),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            normalize_upstream_base_url("https://chat.openai.com/"),
            "https://chat.openai.com/backend-api/codex"
        );
    }

    #[test]
    fn normalize_upstream_base_url_keeps_existing_backend_path() {
        assert_eq!(
            normalize_upstream_base_url("https://chatgpt.com/backend-api/codex/"),
            "https://chatgpt.com/backend-api/codex"
        );
        assert_eq!(
            normalize_upstream_base_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn apply_request_overrides_accepts_xhigh() {
        let body = br#"{"model":"gpt-5.3-codex","reasoning":{"effort":"medium"}}"#.to_vec();
        let updated = apply_request_overrides("/v1/responses", body, None, Some("xhigh"));
        let value: serde_json::Value = serde_json::from_slice(&updated).expect("json");
        assert_eq!(value["reasoning"]["effort"], "xhigh");
    }

    #[test]
    fn apply_request_overrides_maps_extra_high_to_xhigh() {
        let body = br#"{"model":"gpt-5.3-codex"}"#.to_vec();
        let updated = apply_request_overrides("/v1/responses", body, None, Some("extra_high"));
        let value: serde_json::Value = serde_json::from_slice(&updated).expect("json");
        assert_eq!(value["reasoning"]["effort"], "xhigh");
    }

    #[test]
    fn normalize_models_path_appends_client_version_when_missing() {
        assert_eq!(
            normalize_models_path("/v1/models"),
            "/v1/models?client_version=0.98.0"
        );
        assert_eq!(
            normalize_models_path("/v1/models?foo=1"),
            "/v1/models?foo=1&client_version=0.98.0"
        );
    }

    #[test]
    fn normalize_models_path_keeps_existing_client_version() {
        assert_eq!(
            normalize_models_path("/v1/models?client_version=1.2.3"),
            "/v1/models?client_version=1.2.3"
        );
        assert_eq!(normalize_models_path("/v1/responses"), "/v1/responses");
    }

    #[test]
    fn models_path_does_not_try_openai_fallback() {
        let content_type = HeaderValue::from_str("text/html; charset=utf-8").ok();
        assert!(!should_try_openai_fallback(
            "https://chatgpt.com/backend-api/codex",
            "/v1/models?client_version=0.98.0",
            content_type.as_ref()
        ));
        assert!(should_try_openai_fallback(
            "https://chatgpt.com/backend-api/codex",
            "/v1/responses",
            content_type.as_ref()
        ));
    }

    #[test]
    fn cooldown_reason_maps_status() {
        assert_eq!(cooldown_reason_for_status(429), CooldownReason::RateLimited);
        assert_eq!(cooldown_reason_for_status(503), CooldownReason::Upstream5xx);
        assert_eq!(cooldown_reason_for_status(403), CooldownReason::Challenge);
        assert_eq!(cooldown_reason_for_status(400), CooldownReason::Upstream4xx);
        assert_eq!(cooldown_reason_for_status(200), CooldownReason::Default);
    }

    #[test]
    fn token_exchange_lock_reuses_same_account_lock() {
        let first = account_token_exchange_lock("acc-1");
        let second = account_token_exchange_lock("acc-1");
        let third = account_token_exchange_lock("acc-2");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &third));
    }

    #[test]
    fn resolve_openai_bearer_token_uses_cached_storage_value() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let account = Account {
            id: "acc-1".to_string(),
            label: "main".to_string(),
            issuer: "".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            workspace_name: None,
            note: None,
            tags: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        };
        storage.insert_account(&account).expect("insert account");
        storage
            .insert_token(&Token {
                account_id: "acc-1".to_string(),
                id_token: "id-token".to_string(),
                access_token: "access-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                api_key_access_token: Some("cached-api-key-token".to_string()),
                last_refresh: now_ts(),
            })
            .expect("insert token");
        let mut runtime_token = Token {
            account_id: "acc-1".to_string(),
            id_token: "runtime-id-token".to_string(),
            access_token: "runtime-access-token".to_string(),
            refresh_token: "runtime-refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        };

        let bearer =
            resolve_openai_bearer_token(&storage, &account, &mut runtime_token).expect("resolve");
        assert_eq!(bearer, "cached-api-key-token");
        assert_eq!(
            runtime_token.api_key_access_token.as_deref(),
            Some("cached-api-key-token")
        );
    }

    #[test]
    fn metrics_prometheus_contains_expected_series() {
        let text = gateway_metrics_prometheus();
        assert!(text.contains("gpttools_gateway_requests_total "));
        assert!(text.contains("gpttools_gateway_requests_active "));
        assert!(text.contains("gpttools_gateway_account_inflight_total "));
        assert!(text.contains("gpttools_gateway_failover_attempts_total "));
        assert!(text.contains("gpttools_gateway_cooldown_marks_total "));
    }

    #[test]
    fn challenge_detection_requires_html_or_429() {
        let html = HeaderValue::from_str("text/html; charset=utf-8").ok();
        let json = HeaderValue::from_str("application/json").ok();
        assert!(is_upstream_challenge_response(403, html.as_ref()));
        assert!(!is_upstream_challenge_response(403, json.as_ref()));
        assert!(is_upstream_challenge_response(429, json.as_ref()));
    }

    #[test]
    fn drop_incoming_header_keeps_session_affinity_for_primary_attempt() {
        assert!(should_drop_incoming_header("ChatGPT-Account-Id"));
        assert!(should_drop_incoming_header("authorization"));
        assert!(should_drop_incoming_header("x-api-key"));
        assert!(!should_drop_incoming_header("session_id"));
        assert!(!should_drop_incoming_header("x-codex-turn-state"));
        assert!(!should_drop_incoming_header("Content-Type"));
    }

    #[test]
    fn drop_incoming_header_for_failover_strips_session_affinity() {
        assert!(should_drop_incoming_header_for_failover("ChatGPT-Account-Id"));
        assert!(should_drop_incoming_header_for_failover("session_id"));
        assert!(should_drop_incoming_header_for_failover("x-codex-turn-state"));
        assert!(!should_drop_incoming_header_for_failover("Content-Type"));
    }


}


