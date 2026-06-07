use codexmanager_core::storage::{now_ts, Account, Event, RequestLog, Storage, Token};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde_json::json;
use std::io::{BufRead, BufReader, Read};
use std::time::Duration;
use std::time::Instant;

use crate::account_status::mark_account_unavailable_for_auth_error;
use crate::apikey_models::read_managed_model_catalog_from_storage;
use crate::storage_helpers::open_storage;
use crate::usage_account_meta::workspace_header_for_account;
use crate::usage_token_refresh::{refresh_and_persist_access_token, token_refresh_ahead_secs};

const DEFAULT_WARMUP_MESSAGE: &str = "hi";
const FALLBACK_WARMUP_MESSAGE: &str = "你好";
const WARMUP_UPSTREAM_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_WARMUP_MODEL: &str = "gpt-5.3-codex";
const WARMUP_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const WARMUP_TOTAL_TIMEOUT: Duration = Duration::from_secs(90);

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

    let client = build_warmup_client()?;
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
        );
        if item.ok {
            succeeded += 1;
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

fn resolve_target_accounts(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<Account>, String> {
    let accounts = storage
        .list_gateway_candidates()
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|(account, _token)| account)
        .collect::<Vec<_>>();

    if account_ids.is_empty() {
        return Ok(accounts);
    }

    let mut selected = Vec::new();
    for account_id in account_ids {
        let normalized = account_id.trim();
        if normalized.is_empty() {
            continue;
        }
        if let Some(account) = accounts.iter().find(|item| item.id == normalized) {
            selected.push(account.clone());
        }
    }
    Ok(selected)
}

fn normalize_warmup_message(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        DEFAULT_WARMUP_MESSAGE.to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_warmup_client() -> Result<Client, String> {
    let builder = Client::builder()
        .connect_timeout(WARMUP_CONNECT_TIMEOUT)
        .timeout(WARMUP_TOTAL_TIMEOUT)
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Some(Duration::from_secs(60)))
        .user_agent(crate::gateway::current_codex_user_agent());
    let builder = crate::gateway::apply_blocking_upstream_proxy(
        builder,
        crate::gateway::current_upstream_proxy_url().as_deref(),
        "warmup_http_proxy_invalid",
    );
    builder
        .build()
        .map_err(|err| format!("build warmup client failed: {err}"))
}

fn warmup_single_account(
    storage: &Storage,
    client: &Client,
    account: Account,
    model_slug: &str,
    message: &str,
) -> AccountWarmupItemResult {
    let account_name = account.label.clone();
    let started_at = Instant::now();
    match load_account_token(storage, &account) {
        Ok(mut token) => {
            let mut outcome =
                send_warmup_request_with_fallback(client, &account, &token, model_slug, message);

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
                            client, &account, &token, model_slug, message,
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

fn load_account_token(storage: &Storage, account: &Account) -> Result<Token, String> {
    storage
        .find_token_by_account_id(&account.id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "missing token".to_string())
}

fn resolve_warmup_model_slug(storage: &Storage) -> String {
    read_managed_model_catalog_from_storage(storage)
        .ok()
        .and_then(|catalog| {
            catalog
                .items
                .into_iter()
                .find(|item| item.model.supported_in_api)
                .map(|item| item.model.slug)
        })
        .filter(|slug| !slug.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_WARMUP_MODEL.to_string())
}

fn send_warmup_request_with_fallback(
    client: &Client,
    account: &Account,
    token: &Token,
    model_slug: &str,
    message: &str,
) -> Result<String, String> {
    let primary = send_warmup_request(client, account, token, model_slug, message);
    match primary {
        Ok(()) => Ok("已发送预热消息".to_string()),
        Err(primary_err) if message == DEFAULT_WARMUP_MESSAGE => {
            send_warmup_request(client, account, token, model_slug, FALLBACK_WARMUP_MESSAGE)
                .map(|_| "已发送预热消息".to_string())
                .map_err(|fallback_err| format!("{primary_err}; fallback={fallback_err}"))
        }
        Err(err) => Err(err),
    }
}

fn should_retry_warmup_with_refresh(token: &Token, err: &str) -> bool {
    if token.refresh_token.trim().is_empty() {
        return false;
    }
    let normalized = err.to_ascii_lowercase();
    normalized.contains("status=401")
        || normalized.contains("status=403")
        || normalized.contains("auth error")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
}

fn send_warmup_request(
    client: &Client,
    account: &Account,
    token: &Token,
    model_slug: &str,
    message: &str,
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
        return Ok(true);
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
    matches!(value.trim(), "response.completed" | "response.done")
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
mod tests {
    use super::{
        build_warmup_headers, consume_warmup_stream, resolve_target_accounts,
        resolve_warmup_model_slug, should_retry_warmup_with_refresh, DEFAULT_WARMUP_MODEL,
    };
    use crate::apikey_models::save_managed_model_catalog_with_storage;
    use codexmanager_core::rpc::types::{
        ManagedModelCatalogEntry, ManagedModelCatalogResult, ModelInfo,
    };
    use codexmanager_core::storage::{now_ts, Account, Storage, Token};
    use std::io::Cursor;

    fn make_model(slug: &str, sort_index: i64, supported_in_api: bool) -> ManagedModelCatalogEntry {
        ManagedModelCatalogEntry {
            model: ModelInfo {
                slug: slug.to_string(),
                display_name: slug.to_string(),
                supported_in_api,
                ..ModelInfo::default()
            },
            sort_index,
            ..ManagedModelCatalogEntry::default()
        }
    }

    #[test]
    fn resolve_warmup_model_slug_uses_first_supported_model_from_catalog_order() {
        let storage = Storage::open_in_memory().expect("open in-memory storage");
        storage.init().expect("init in-memory storage");
        save_managed_model_catalog_with_storage(
            &storage,
            &ManagedModelCatalogResult {
                items: vec![
                    make_model("gpt-hidden", 0, false),
                    make_model("gpt-latest", 1, true),
                    make_model("gpt-older", 2, true),
                ],
                ..ManagedModelCatalogResult::default()
            },
        )
        .expect("save model catalog");

        assert_eq!(resolve_warmup_model_slug(&storage), "gpt-latest");
    }

    #[test]
    fn resolve_warmup_model_slug_falls_back_when_catalog_missing() {
        let storage = Storage::open_in_memory().expect("open in-memory storage");
        storage.init().expect("init in-memory storage");
        assert_eq!(resolve_warmup_model_slug(&storage), DEFAULT_WARMUP_MODEL);
    }

    #[test]
    fn should_retry_warmup_with_refresh_only_for_auth_errors_with_refresh_token() {
        let mut token = Token {
            account_id: "account-1".to_string(),
            id_token: String::new(),
            access_token: String::new(),
            refresh_token: "refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: 0,
        };

        assert!(should_retry_warmup_with_refresh(
            &token,
            "status=401 body=Unauthorized"
        ));
        assert!(!should_retry_warmup_with_refresh(
            &token,
            "status=500 body=server error"
        ));

        token.refresh_token.clear();
        assert!(!should_retry_warmup_with_refresh(
            &token,
            "status=401 body=Unauthorized"
        ));
    }

    #[test]
    fn resolve_target_accounts_only_returns_gateway_available_accounts() {
        let storage = Storage::open_in_memory().expect("open in-memory storage");
        storage.init().expect("init in-memory storage");
        let now = now_ts();

        for (id, status) in [
            ("acc-active", "active"),
            ("acc-unavailable", "unavailable"),
            ("acc-disabled", "disabled"),
            ("acc-banned", "banned"),
            ("acc-inactive", "inactive"),
        ] {
            storage
                .insert_account(&Account {
                    id: id.to_string(),
                    label: id.to_string(),
                    issuer: "issuer".to_string(),
                    chatgpt_account_id: None,
                    workspace_id: None,
                    group_name: None,
                    sort: 0,
                    status: status.to_string(),
                    created_at: now,
                    updated_at: now,
                })
                .expect("insert account");
            storage
                .insert_token(&Token {
                    account_id: id.to_string(),
                    id_token: "id-token".to_string(),
                    access_token: "access-token".to_string(),
                    refresh_token: "refresh-token".to_string(),
                    api_key_access_token: None,
                    last_refresh: now,
                })
                .expect("insert token");
        }

        let all_targets = resolve_target_accounts(&storage, &[]).expect("resolve all targets");
        assert_eq!(all_targets.len(), 1);
        assert_eq!(all_targets[0].id, "acc-active");

        let selected_targets = resolve_target_accounts(
            &storage,
            &[
                "acc-unavailable".to_string(),
                "acc-active".to_string(),
                "acc-disabled".to_string(),
            ],
        )
        .expect("resolve selected targets");
        assert_eq!(selected_targets.len(), 1);
        assert_eq!(selected_targets[0].id, "acc-active");
    }

    #[test]
    fn build_warmup_headers_omits_non_codex_headers() {
        let account = Account {
            id: "acc-1".to_string(),
            label: "acc-1".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
        };

        let headers = build_warmup_headers(&account, "bearer-token").expect("build warmup headers");

        assert!(headers.get("version").is_none());
        assert!(headers.get("openai-organization").is_none());
        assert!(headers.get("openai-project").is_none());
        assert!(headers.get("client_version").is_none());
    }

    #[test]
    fn consume_warmup_stream_waits_for_response_completed() {
        let stream = Cursor::new(
            "event: response.created\n\
             data: {\"type\":\"response.created\"}\n\n\
             event: response.completed\n\
             data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\"}}\n\n",
        );

        assert!(consume_warmup_stream(stream).is_ok());
    }

    #[test]
    fn consume_warmup_stream_rejects_incomplete_stream() {
        let stream = Cursor::new(
            "event: response.created\n\
             data: {\"type\":\"response.created\"}\n\n",
        );

        let err = consume_warmup_stream(stream).expect_err("stream should be incomplete");
        assert!(err.contains("before response.completed"));
    }

    #[test]
    fn consume_warmup_stream_reports_error_event() {
        let stream = Cursor::new(
            "event: response.failed\n\
             data: {\"type\":\"response.failed\",\"error\":{\"message\":\"quota exceeded\"}}\n\n",
        );

        let err = consume_warmup_stream(stream).expect_err("stream should fail");
        assert!(err.contains("quota exceeded"));
    }
}

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
    if err.to_ascii_lowercase().contains("auth error")
        || err.to_ascii_lowercase().contains("status=401")
        || err.to_ascii_lowercase().contains("status=403")
    {
        let _ = mark_account_unavailable_for_auth_error(storage, account_id, err);
    }
    Ok(())
}
