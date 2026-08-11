use codexmanager_core::storage::{now_ts, Account, Event, RequestLog, Storage, Token};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde_json::json;
use rand::RngCore;
use std::io::{BufRead, BufReader, Read};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;

use crate::account_status::mark_account_unavailable_for_auth_error;
use crate::storage_helpers::open_storage;
use crate::usage_account_meta::workspace_header_for_account;
use crate::usage_token_refresh::{refresh_and_persist_access_token, token_refresh_ahead_secs};

const DEFAULT_WARMUP_MESSAGE: &str = "hi";
const FALLBACK_WARMUP_MESSAGE: &str = "你好";
const WARMUP_UPSTREAM_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_WARMUP_MODEL: &str = "gpt-5.3-codex";
const WARMUP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const WARMUP_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const BATCH_ADMISSION_PROBE_TIMEOUT: Duration = Duration::from_secs(20);
static WARMUP_HTTP_CLIENT: OnceLock<Mutex<Option<WarmupClientCacheEntry>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
struct WarmupClientConfig {
    user_agent: String,
    proxy_url: Option<String>,
}

#[derive(Clone)]
struct WarmupClientCacheEntry {
    config: WarmupClientConfig,
    client: Client,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountWarmupResult {
    pub(crate) requested: usize,
    pub(crate) succeeded: usize,
    pub(crate) failed: usize,
    pub(crate) results: Vec<AccountWarmupItemResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountWarmupItemResult {
    pub(crate) account_id: String,
    pub(crate) account_name: String,
    pub(crate) ok: bool,
    pub(crate) message: String,
}

struct AccountWarmupTarget {
    account: Account,
    token: Token,
}

/// 函数 `warmup_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-14
///
/// # 参数
/// - account_ids: 参数 account_ids
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn warmup_accounts(
    account_ids: Vec<String>,
    message: &str,
) -> Result<AccountWarmupResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut accounts = resolve_target_accounts(&storage, &account_ids)?;
    if accounts.is_empty() {
        return Err("no account available for warmup".to_string());
    }

    let client = warmup_client()?;
    let warmup_message = normalize_warmup_message(message);
    let warmup_model = resolve_warmup_model_slug(&storage);
    let mut results = Vec::with_capacity(accounts.len());
    let mut succeeded = 0usize;

    for account in accounts.drain(..) {
        let item = warmup_single_account(
            &storage,
            &client,
            account,
            warmup_model.as_str(),
            warmup_message.as_str(),
            WARMUP_TOTAL_TIMEOUT,
        );
        if item.ok {
            succeeded += 1;
            crate::usage_refresh::record_codex_responses_verified(&storage, &item.account_id);
        } else {
            crate::usage_refresh::record_codex_admission_probe_failure(
                &storage,
                &item.account_id,
                &item.message,
            );
        }
        results.push(item);
    }

    Ok(AccountWarmupResult {
        requested: results.len(),
        succeeded,
        failed: results.len().saturating_sub(succeeded),
        results,
    })
}

/// A batch admission probe uses the real Codex Responses endpoint with a tiny
/// internal prompt. It is intentionally separate from import/model discovery:
/// only this proves that the exact account, workspace, route and requested
/// model can execute before the account is admitted into a live batch.
pub(crate) fn probe_account_responses_for_batch(
    account_id: &str,
    model_slug: Option<&str>,
) -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut targets = resolve_target_accounts(&storage, &[account_id.to_string()])?;
    let target = targets
        .pop()
        .ok_or_else(|| "account is not gateway eligible for admission probe".to_string())?;
    let client = warmup_client()?;
    let model = model_slug
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| resolve_warmup_model_slug(&storage));
    let result = warmup_single_account(
        &storage,
        &client,
        target,
        model.as_str(),
        ".",
        BATCH_ADMISSION_PROBE_TIMEOUT,
    );
    if result.ok {
        Ok(())
    } else {
        Err(result.message)
    }
}

fn resolve_target_accounts(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<AccountWarmupTarget>, String> {
    if account_ids.is_empty() {
        return storage
            .list_gateway_candidates()
            .map_err(|err| err.to_string())
            .map(gateway_candidate_warmup_targets);
    }

    storage
        .list_gateway_candidates_for_accounts(account_ids)
        .map_err(|err| err.to_string())
        .map(gateway_candidate_warmup_targets)
}

fn gateway_candidate_warmup_targets(candidates: Vec<(Account, Token)>) -> Vec<AccountWarmupTarget> {
    candidates
        .into_iter()
        .map(|(account, token)| AccountWarmupTarget { account, token })
        .collect()
}

fn normalize_warmup_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        DEFAULT_WARMUP_MESSAGE.to_string()
    } else {
        trimmed.to_string()
    }
}

fn current_warmup_client_config() -> WarmupClientConfig {
    WarmupClientConfig {
        user_agent: crate::gateway::current_codex_user_agent(),
        proxy_url: crate::gateway::current_upstream_proxy_url(),
    }
}

fn warmup_client() -> Result<Client, String> {
    let config = current_warmup_client_config();
    let cache = WARMUP_HTTP_CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .map_err(|_| "warmup client cache lock poisoned".to_string())?;
    if let Some(entry) = guard.as_ref() {
        if entry.config == config {
            return Ok(entry.client.clone());
        }
    }
    let client = build_warmup_client_for_config(&config)?;
    *guard = Some(WarmupClientCacheEntry {
        config,
        client: client.clone(),
    });
    Ok(client)
}

pub(crate) fn reload_warmup_client() {
    if let Some(cache) = WARMUP_HTTP_CLIENT.get() {
        let mut guard = crate::lock_utils::lock_recover(cache, "warmup_http_client");
        *guard = None;
    }
}

fn build_warmup_client_for_config(config: &WarmupClientConfig) -> Result<Client, String> {
    #[cfg(test)]
    WARMUP_CLIENT_BUILD_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let builder = Client::builder()
        .connect_timeout(WARMUP_CONNECT_TIMEOUT)
        .timeout(WARMUP_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Some(Duration::from_secs(60)))
        .user_agent(config.user_agent.as_str());
    let builder = crate::gateway::apply_blocking_upstream_proxy(
        builder,
        config.proxy_url.as_deref(),
        "warmup_http_proxy_invalid",
    );
    builder
        .build()
        .map_err(|err| format!("build warmup client failed: {err}"))
}

#[cfg(test)]
static WARMUP_CLIENT_BUILD_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
fn reset_warmup_client_cache_for_test() {
    if let Some(cache) = WARMUP_HTTP_CLIENT.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
    WARMUP_CLIENT_BUILD_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
fn warmup_client_build_count_for_test() -> usize {
    WARMUP_CLIENT_BUILD_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

fn warmup_single_account(
    storage: &Storage,
    client: &Client,
    target: AccountWarmupTarget,
    model_slug: &str,
    message: &str,
    request_timeout: Duration,
) -> AccountWarmupItemResult {
    let AccountWarmupTarget { account, mut token } = target;
    let account_name = account.label.clone();
    let started_at = Instant::now();
    let mut outcome = send_warmup_request_with_fallback(
        client,
        &account,
        &token,
        model_slug,
        message,
        request_timeout,
    );

    if let Err(err) = outcome.as_ref() {
        if should_retry_warmup_with_refresh(&token, err) {
            let issuer = std::env::var("CODEXMANAGER_ISSUER")
                .unwrap_or_else(|_| codexmanager_core::auth::DEFAULT_ISSUER.to_string());
            let client_id = std::env::var("CODEXMANAGER_CLIENT_ID")
                .unwrap_or_else(|_| codexmanager_core::auth::DEFAULT_CLIENT_ID.to_string());
            outcome = refresh_and_persist_access_token(
                storage,
                &mut token,
                &issuer,
                &client_id,
                token_refresh_ahead_secs(),
            )
            .and_then(|_| {
                send_warmup_request_with_fallback(
                    client,
                    &account,
                    &token,
                    model_slug,
                    message,
                    request_timeout,
                )
            });
        }
    }

    match outcome {
        Ok(ok_message) => {
            persist_warmup_observability(
                storage,
                &account,
                200,
                None,
                model_slug,
                started_at.elapsed().as_millis() as i64,
                ok_message.as_str(),
            );
            let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(&account.id);
            AccountWarmupItemResult {
                account_id: account.id,
                account_name,
                ok: true,
                message: ok_message,
            }
        }
        Err(err) => {
            let _ = maybe_mark_account_auth_error(storage, &account.id, &err);
            let status_code = extract_status_code_from_message(&err);
            persist_warmup_observability(
                storage,
                &account,
                status_code,
                Some(err.as_str()),
                model_slug,
                started_at.elapsed().as_millis() as i64,
                "预热失败",
            );
            AccountWarmupItemResult {
                account_id: account.id,
                account_name,
                ok: false,
                message: err,
            }
        }
    }
}

fn persist_warmup_observability(
    storage: &Storage,
    account: &Account,
    status_code: i64,
    error: Option<&str>,
    model_slug: &str,
    duration_ms: i64,
    event_message: &str,
) {
    let created_at = now_ts();
    let trace_id = format!("warmup-{}-{created_at}", account.id);
    let _ = storage.insert_request_log(&RequestLog {
        trace_id: Some(trace_id),
        account_id: Some(account.id.clone()),
        initial_account_id: Some(account.id.clone()),
        attempted_account_ids_json: Some(format!(r#"["{}"]"#, account.id)),
        request_path: "/internal/account/warmup".to_string(),
        original_path: Some("/internal/account/warmup".to_string()),
        adapted_path: Some("/internal/account/warmup".to_string()),
        method: "POST".to_string(),
        request_type: Some("account_warmup".to_string()),
        gateway_mode: None,
        transparent_mode: None,
        enhanced_mode: None,
        model: Some(model_slug.to_string()),
        upstream_url: Some(WARMUP_UPSTREAM_URL.to_string()),
        status_code: Some(status_code),
        duration_ms: Some(duration_ms.max(0)),
        first_response_ms: None,
        error: error.map(str::to_string),
        created_at,
        ..RequestLog::default()
    });
    let _ = storage.insert_event(&Event {
        account_id: Some(account.id.clone()),
        event_type: "account_warmup".to_string(),
        message: match error {
            Some(err) => {
                format!("{event_message}; model={model_slug}; status={status_code}; error={err}")
            }
            None => format!("{event_message}; model={model_slug}; status={status_code}"),
        },
        created_at,
    });
}

fn extract_status_code_from_message(message: &str) -> i64 {
    let marker = "status=";
    let Some(index) = message.find(marker) else {
        return 500;
    };
    let digits: String = message[index + marker.len()..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse::<i64>().unwrap_or(500)
}

fn resolve_warmup_model_slug(storage: &Storage) -> String {
    storage
        .find_first_api_available_model_catalog_slug("default")
        .ok()
        .flatten()
        .filter(|slug| !slug.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_WARMUP_MODEL.to_string())
}

fn send_warmup_request_with_fallback(
    client: &Client,
    account: &Account,
    token: &Token,
    model_slug: &str,
    message: &str,
    request_timeout: Duration,
) -> Result<String, String> {
    let primary = send_warmup_request(client, account, token, model_slug, message, request_timeout);
    match primary {
        Ok(()) => Ok("已发送预热消息".to_string()),
        Err(primary_err) if message == DEFAULT_WARMUP_MESSAGE => send_warmup_request(
            client,
            account,
            token,
            model_slug,
            FALLBACK_WARMUP_MESSAGE,
            request_timeout,
        )
        .map(|_| "已发送预热消息".to_string())
        .map_err(|fallback_err| format!("{primary_err}; fallback={fallback_err}")),
        Err(err) => Err(err),
    }
}

fn should_retry_warmup_with_refresh(token: &Token, err: &str) -> bool {
    if token.refresh_token.trim().is_empty() {
        return false;
    }
    let normalized = err.to_ascii_lowercase();
    if crate::usage_http::is_cloudflare_challenge_error_message(err)
        || crate::usage_http::is_region_blocked_error_message(err)
    {
        return false;
    }
    normalized.contains("status=401")
        || normalized.contains("auth error")
        || normalized.contains("unauthorized")
}

fn send_warmup_request(
    client: &Client,
    account: &Account,
    token: &Token,
    model_slug: &str,
    message: &str,
    request_timeout: Duration,
) -> Result<(), String> {
    let body = json!({
        "model": model_slug,
        "instructions": "",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": message
            }]
        }],
        "stream": true,
        "store": false
    });

    let headers = build_warmup_headers(account, token.access_token.as_str())?;
    let response = client
        .post(WARMUP_UPSTREAM_URL)
        .timeout(request_timeout)
        .headers(headers)
        .json(&body)
        .send()
        .map_err(|err| format!("warmup request failed: {err}"))?;

    let status = response.status();
    let headers = response.headers().clone();
    if status.is_success() {
        return consume_warmup_stream(response);
    }

    let body_text = response.text().unwrap_or_default();
    Err(summarize_warmup_error(
        status.as_u16(),
        &headers,
        &body_text,
    ))
}

fn consume_warmup_stream<R: Read>(reader: R) -> Result<(), String> {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut event_name: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|err| format!("warmup stream read failed: {err}"))?;
        if bytes == 0 {
            if process_warmup_sse_event(event_name.as_deref(), &data_lines)? {
                return Ok(());
            }
            return Err("warmup stream ended before response.completed".to_string());
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if process_warmup_sse_event(event_name.as_deref(), &data_lines)? {
                return Ok(());
            }
            event_name = None;
            data_lines.clear();
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("event:") {
            event_name = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("data:") {
            data_lines.push(value.trim().to_string());
        }
    }
}

fn process_warmup_sse_event(
    event_name: Option<&str>,
    data_lines: &[String],
) -> Result<bool, String> {
    let event_name = event_name.map(str::trim).filter(|value| !value.is_empty());
    if let Some(event) = event_name {
        if is_warmup_terminal_event(event) {
            return Ok(true);
        }
    }

    if data_lines.is_empty() {
        if let Some(event) = event_name {
            if is_warmup_error_event(event) {
                return Err(format!("warmup stream error event: {event}"));
            }
        }
        return Ok(false);
    }
    let data = data_lines.join("\n");
    let trimmed = data.trim();
    if trimmed == "[DONE]" {
        // The transport terminator alone does not prove that Codex completed
        // the response; admission requires an explicit response.completed.
        return Ok(false);
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Ok(false);
    };
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .or(event_name);
    if let Some(event_type) = event_type {
        if is_warmup_terminal_event(event_type) {
            return Ok(true);
        }
        if is_warmup_error_event(event_type) {
            return Err(format!(
                "warmup stream error event: {}; {}",
                event_type,
                summarize_warmup_stream_error(&value)
            ));
        }
    }
    if let Some(event) = event_name {
        if is_warmup_error_event(event) {
            return Err(format!("warmup stream error event: {event}"));
        }
    }
    Ok(false)
}

fn is_warmup_terminal_event(value: &str) -> bool {
    value.trim() == "response.completed"
}

fn is_warmup_error_event(value: &str) -> bool {
    matches!(
        value.trim(),
        "error" | "response.failed" | "response.incomplete"
    )
}

fn summarize_warmup_stream_error(value: &serde_json::Value) -> String {
    value
        .get("error")
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("error"))
        })
        .and_then(|error| {
            error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .or_else(|| error.as_str())
        })
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .unwrap_or("unknown stream error")
        .to_string()
}

#[cfg(test)]
#[path = "account_warmup_tests.rs"]
mod tests;

fn build_warmup_headers(account: &Account, bearer: &str) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        header_value(&format!("Bearer {bearer}"))?,
    );
    headers.insert(
        reqwest::header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        header_value(&crate::gateway::current_codex_user_agent())?,
    );
    headers.insert(
        HeaderName::from_static("originator"),
        header_value(&crate::gateway::current_wire_originator())?,
    );
    headers.insert(
        HeaderName::from_static("version"),
        header_value(&crate::gateway::current_codex_user_agent_version())?,
    );
    headers.insert(
        HeaderName::from_static("openai-beta"),
        HeaderValue::from_static("responses=experimental"),
    );
    headers.insert(
        HeaderName::from_static("x-codex-window-id"),
        header_value(&new_probe_window_id())?,
    );

    if let Some(residency_requirement) = crate::gateway::current_residency_requirement() {
        headers.insert(
            HeaderName::from_static("x-openai-internal-codex-residency"),
            header_value(&residency_requirement)?,
        );
    }
    if let Some(account_header) = workspace_header_for_account(account) {
        headers.insert(
            HeaderName::from_static("chatgpt-account-id"),
            header_value(&account_header)?,
        );
    }

    Ok(headers)
}

fn new_probe_window_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn header_value(value: &str) -> Result<HeaderValue, String> {
    HeaderValue::from_str(value).map_err(|err| format!("invalid header value: {err}"))
}

fn summarize_warmup_error(status: u16, headers: &HeaderMap, body: &str) -> String {
    let body_hint =
        crate::gateway::summarize_upstream_error_hint_from_body(status, body.as_bytes())
            .or_else(|| {
                let trimmed = body.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .unwrap_or_else(|| "unknown error".to_string());

    let request_id = first_header(headers, &["x-request-id", "x-oai-request-id"]);
    let auth_error = first_header(headers, &["x-openai-authorization-error"]);
    let cf_ray = first_header(headers, &["cf-ray"]);

    let mut details = Vec::new();
    if let Some(value) = request_id {
        details.push(format!("request id: {value}"));
    }
    if let Some(value) = auth_error {
        details.push(format!("auth error: {value}"));
    }
    if let Some(value) = cf_ray {
        details.push(format!("cf-ray: {value}"));
    }

    if details.is_empty() {
        format!("status={status} body={body_hint}")
    } else {
        format!("status={status} body={body_hint}, {}", details.join(", "))
    }
}

fn first_header(headers: &HeaderMap, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn maybe_mark_account_auth_error(
    storage: &Storage,
    account_id: &str,
    err: &str,
) -> Result<(), String> {
    let normalized = err.to_ascii_lowercase();
    let cloudflare_or_region = crate::usage_http::is_cloudflare_challenge_error_message(err)
        || crate::usage_http::is_region_blocked_error_message(err);
    if !cloudflare_or_region
        && (normalized.contains("auth error") || normalized.contains("status=401"))
    {
        let _ = mark_account_unavailable_for_auth_error(storage, account_id, err);
    }
    Ok(())
}
