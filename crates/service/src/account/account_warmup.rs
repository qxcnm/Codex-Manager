use codexmanager_core::storage::{now_ts, Account, Event, RequestLog, Storage, Token};
use futures_util::TryStreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Client;
use serde::Serialize;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio_util::io::StreamReader;

use crate::account_status::mark_account_unavailable_for_auth_error;
use crate::storage_helpers::open_storage;
use crate::usage_account_meta::workspace_header_for_account;
use crate::usage_token_refresh::{
    refresh_and_persist_access_token_async, token_refresh_ahead_secs,
};

const DEFAULT_WARMUP_MESSAGE: &str = "hi";
const FALLBACK_WARMUP_MESSAGE: &str = "你好";
pub(crate) const WARMUP_UPSTREAM_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
const DEFAULT_WARMUP_MODEL: &str = "gpt-6-luna";
const RESET_WARMUP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const X_OPENAI_FEDRAMP_HEADER_NAME: &str = "x-openai-fedramp";

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

pub(crate) struct WarmupAuthorization {
    value: String,
    task_id: Option<String>,
    is_fedramp: bool,
    pub(crate) uses_agent_identity: bool,
    account_scope_id: Option<String>,
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
    crate::gateway::run_upstream_io(warmup_accounts_async(account_ids, message))?
}

pub(crate) async fn warmup_accounts_async(
    account_ids: Vec<String>,
    message: &str,
) -> Result<AccountWarmupResult, String> {
    let storage = open_storage()
        .map(|pooled| pooled.shared_handle())
        .ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let mut accounts = resolve_target_accounts(&storage, &account_ids)?;
    if accounts.is_empty() {
        return Err("no account available for warmup".to_string());
    }

    let warmup_message = normalize_warmup_message(message);
    let mut results = Vec::with_capacity(accounts.len());
    let mut succeeded = 0usize;

    for target in accounts.drain(..) {
        let warmup_model = match resolve_warmup_model_slug(&storage, &target) {
            Ok(model) => model,
            Err(err) => {
                results.push(AccountWarmupItemResult {
                    account_id: target.account.id,
                    account_name: target.account.label,
                    ok: false,
                    message: err,
                });
                continue;
            }
        };
        let client = match build_warmup_client_for_account(&target.account.id) {
            Ok(client) => client,
            Err(err) => {
                results.push(AccountWarmupItemResult {
                    account_id: target.account.id,
                    account_name: target.account.label,
                    ok: false,
                    message: err,
                });
                continue;
            }
        };
        let item = warmup_single_account(
            &storage,
            &client,
            target,
            warmup_model.as_str(),
            warmup_message.as_str(),
            true,
        )
        .await;
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
) -> Result<Vec<AccountWarmupTarget>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
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

/// Reset warmups must bypass the stale exhausted-quota gateway filter. The
/// scheduler has already atomically claimed a due, enabled cycle; account state
/// is checked again here before reusing the normal proxy/auth/logging pipeline.
pub(crate) async fn warmup_account_after_reset(
    storage: &Storage,
    account_id: &str,
) -> Result<AccountWarmupItemResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let target = resolve_reset_warmup_target(storage, account_id)?;
    let client = build_warmup_client_for_account(account_id)?;
    let model = resolve_warmup_model_slug(storage, &target)?;
    Ok(warmup_single_account(
        storage,
        &client,
        target,
        &model,
        DEFAULT_WARMUP_MESSAGE,
        false,
    )
    .await)
}

fn resolve_reset_warmup_target(
    storage: &Storage,
    account_id: &str,
) -> Result<AccountWarmupTarget, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let (account, token) = storage
        .find_account_with_token_by_id(account_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "reset warmup account or token missing".to_string())?;
    if matches!(
        account.status.trim().to_ascii_lowercase().as_str(),
        "inactive" | "disabled" | "unavailable" | "banned"
    ) {
        return Err("account unavailable for reset warmup".to_string());
    }
    Ok(AccountWarmupTarget { account, token })
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

fn build_warmup_client_for_account(account_id: &str) -> Result<Client, String> {
    let normalized = account_id.trim();
    if normalized.is_empty() {
        return Err("build warmup client failed: missing account id".to_string());
    }
    crate::gateway::fresh_async_upstream_client_for_account(normalized)
        .map_err(|err| format!("build warmup client failed: {err}"))
}

async fn warmup_single_account(
    storage: &Storage,
    client: &Client,
    target: AccountWarmupTarget,
    model_slug: &str,
    message: &str,
    allow_message_fallback: bool,
) -> AccountWarmupItemResult {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let AccountWarmupTarget { account, mut token } = target;
    let started_at = Instant::now();
    let authorization = resolve_warmup_authorization(storage, client, &account, &token).await;
    let uses_agent_identity = authorization
        .as_ref()
        .map(|authorization| authorization.uses_agent_identity)
        .unwrap_or(false);
    let failed_agent_task_id = authorization
        .as_ref()
        .ok()
        .and_then(|authorization| authorization.task_id.clone());
    let mut outcome = match authorization {
        Ok(authorization) => {
            send_warmup_request_with_fallback(
                client,
                &account,
                &authorization,
                model_slug,
                message,
                allow_message_fallback,
            )
            .await
        }
        Err(error) => Err(error),
    };

    if let Err(err) = outcome.as_ref() {
        if uses_agent_identity && crate::agent_identity::is_agent_identity_task_invalid_error(err) {
            outcome = recover_warmup_agent_identity_task(
                storage,
                client,
                &account,
                &token,
                model_slug,
                message,
                failed_agent_task_id.as_deref(),
                (!allow_message_fallback).then_some(RESET_WARMUP_REQUEST_TIMEOUT),
            )
            .await;
        } else if !uses_agent_identity && should_retry_warmup_with_refresh(&token, err) {
            let issuer = std::env::var("CODEXMANAGER_ISSUER")
                .unwrap_or_else(|_| codexmanager_core::auth::DEFAULT_ISSUER.to_string());
            let client_id = std::env::var("CODEXMANAGER_CLIENT_ID")
                .unwrap_or_else(|_| codexmanager_core::auth::DEFAULT_CLIENT_ID.to_string());
            outcome = match refresh_and_persist_access_token_async(
                storage,
                &mut token,
                &issuer,
                &client_id,
                token_refresh_ahead_secs(),
            )
            .await
            {
                Ok(_) => {
                    match resolve_warmup_authorization(storage, client, &account, &token).await {
                        Ok(authorization) => {
                            send_warmup_request_with_fallback(
                                client,
                                &account,
                                &authorization,
                                model_slug,
                                message,
                                allow_message_fallback,
                            )
                            .await
                        }
                        Err(error) => Err(error),
                    }
                }
                Err(error) => Err(error),
            };
        }
    }

    finish_warmup_attempt(
        storage,
        account,
        model_slug,
        started_at.elapsed().as_millis() as i64,
        outcome,
        crate::usage_refresh::enqueue_usage_refresh_for_account,
    )
}

fn finish_warmup_attempt<F>(
    storage: &Storage,
    account: Account,
    model_slug: &str,
    duration_ms: i64,
    outcome: Result<String, String>,
    refresh_usage: F,
) -> AccountWarmupItemResult
where
    F: FnOnce(&str) -> bool,
{
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let account_name = account.label.clone();
    match outcome {
        Ok(ok_message) => {
            persist_warmup_observability(
                storage,
                &account,
                200,
                None,
                model_slug,
                duration_ms,
                ok_message.as_str(),
            );
            let _ = refresh_usage(&account.id);
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
                duration_ms,
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
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
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

fn resolve_warmup_model_slug(
    storage: &Storage,
    target: &AccountWarmupTarget,
) -> Result<String, String> {
    let configured_ceiling = crate::gateway::current_free_account_max_model();
    resolve_warmup_model_slug_with_ceiling(storage, target, &configured_ceiling)
}

fn resolve_warmup_model_slug_with_ceiling(
    storage: &Storage,
    target: &AccountWarmupTarget,
    configured_ceiling: &str,
) -> Result<String, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let models = storage
        .list_api_models_v2()
        .map_err(|err| format!("list warmup models failed: {err}"))?;
    let first_text_model = models
        .iter()
        .find(|model| crate::models_v2::supports_text_generation(model));
    let ceiling = configured_ceiling.trim();
    if ceiling.is_empty() || ceiling.eq_ignore_ascii_case("auto") {
        return Ok(first_text_model
            .map(|model| model.slug.clone())
            .filter(|slug| !slug.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_WARMUP_MODEL.to_string()));
    }

    let snapshot = storage
        .latest_usage_snapshot_for_account(&target.account.id)
        .map_err(|err| format!("read warmup account usage failed: {err}"))?;
    let subscription = storage
        .find_account_subscription(&target.account.id)
        .map_err(|err| format!("read warmup account subscription failed: {err}"))?;
    let token_plan = crate::account_plan::token_plan_from_token(&target.token);
    let is_free_or_unknown = match crate::account_plan::resolve_effective_account_plan(
        Some(&token_plan),
        snapshot.as_ref(),
        subscription.as_ref(),
    ) {
        Some(plan) => matches!(plan.normalized.as_str(), "free" | "unknown"),
        // Missing plan metadata is not proof of a paid account. Apply the
        // conservative Free ceiling until a refresh provides a plan signal.
        None => true,
    };
    if !is_free_or_unknown {
        return Ok(first_text_model
            .map(|model| model.slug.clone())
            .filter(|slug| !slug.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_WARMUP_MODEL.to_string()));
    }

    for model in models
        .iter()
        .filter(|model| crate::models_v2::supports_text_generation(model))
    {
        let exceeds =
            crate::models_v2::request_exceeds_model_ceiling(storage, Some(&model.slug), ceiling)
                .map_err(|err| format!("read warmup model ceiling failed: {err}"))?;
        if !exceeds {
            return Ok(model.slug.clone());
        }
    }

    Err(format!(
        "no enabled text-generation model is available at or below the Free account ceiling: {ceiling}"
    ))
}

pub(crate) async fn resolve_warmup_authorization(
    storage: &Storage,
    client: &Client,
    account: &Account,
    token: &Token,
) -> Result<WarmupAuthorization, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    match crate::agent_identity::resolve_or_bootstrap_account_agent_identity_authorization_async(
        storage, client, account, token,
    )
    .await
    {
        Ok(Some(authorization)) => {
            return Ok(WarmupAuthorization {
                value: authorization.value,
                task_id: Some(authorization.task_id),
                is_fedramp: authorization.is_fedramp,
                uses_agent_identity: true,
                account_scope_id: authorization.account_scope_id,
            });
        }
        Ok(None) => {}
        Err(err) => {
            if token.access_token.trim().is_empty() {
                return Err(err);
            }
            log::warn!(
                "event=account_warmup_agent_identity_resolution_failed account_id={} error={}",
                account.id,
                err
            );
        }
    }

    let access_token = token.access_token.trim();
    if access_token.is_empty() {
        return Err("missing chatgpt access token".to_string());
    }
    Ok(WarmupAuthorization {
        value: access_token.to_string(),
        task_id: None,
        is_fedramp: false,
        uses_agent_identity: false,
        account_scope_id: None,
    })
}

async fn recover_warmup_agent_identity_task(
    storage: &Storage,
    client: &Client,
    account: &Account,
    token: &Token,
    model_slug: &str,
    message: &str,
    failed_task_id: Option<&str>,
    request_timeout: Option<Duration>,
) -> Result<String, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let failed_task_id = failed_task_id
        .map(str::trim)
        .filter(|task_id| !task_id.is_empty())
        .ok_or_else(|| "agent identity task_id is missing during warmup recovery".to_string())?;
    let authorization = crate::agent_identity::recover_account_agent_identity_authorization_async(
        storage,
        client,
        account,
        token,
        failed_task_id,
    )
    .await?
    .ok_or_else(|| "agent identity disappeared during warmup task recovery".to_string())?;
    let authorization = WarmupAuthorization {
        value: authorization.value,
        task_id: Some(authorization.task_id),
        is_fedramp: authorization.is_fedramp,
        uses_agent_identity: true,
        account_scope_id: authorization.account_scope_id,
    };
    send_warmup_request(
        client,
        account,
        &authorization,
        model_slug,
        message,
        request_timeout,
    )
    .await
    .map(|_| "已发送预热消息".to_string())
}

async fn send_warmup_request_with_fallback(
    client: &Client,
    account: &Account,
    authorization: &WarmupAuthorization,
    model_slug: &str,
    message: &str,
    allow_message_fallback: bool,
) -> Result<String, String> {
    let timeout = (!allow_message_fallback).then_some(RESET_WARMUP_REQUEST_TIMEOUT);
    match send_warmup_request(client, account, authorization, model_slug, message, timeout).await {
        Ok(()) => Ok("已发送预热消息".to_string()),
        Err(primary_err)
            if allow_message_fallback
                && message == DEFAULT_WARMUP_MESSAGE
                && !crate::agent_identity::is_agent_identity_task_invalid_error(&primary_err) =>
        {
            send_warmup_request(
                client,
                account,
                authorization,
                model_slug,
                FALLBACK_WARMUP_MESSAGE,
                timeout,
            )
            .await
            .map(|_| "已发送预热消息".to_string())
            .map_err(|fallback_err| format!("{primary_err}; fallback={fallback_err}"))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]

fn warmup_request_with_message_fallback<F>(
    message: &str,
    allow_message_fallback: bool,
    mut send: F,
) -> Result<String, String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let primary = send(message);
    match primary {
        Ok(()) => Ok("已发送预热消息".to_string()),
        Err(primary_err)
            if allow_message_fallback
                && message == DEFAULT_WARMUP_MESSAGE
                && !crate::agent_identity::is_agent_identity_task_invalid_error(&primary_err) =>
        {
            send(FALLBACK_WARMUP_MESSAGE)
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

async fn send_warmup_request(
    client: &Client,
    account: &Account,
    authorization: &WarmupAuthorization,
    model_slug: &str,
    message: &str,
    request_timeout: Option<Duration>,
) -> Result<(), String> {
    send_warmup_request_at_url(
        client,
        account,
        authorization,
        model_slug,
        message,
        request_timeout,
        WARMUP_UPSTREAM_URL,
    )
    .await
}

async fn send_warmup_request_at_url(
    client: &Client,
    account: &Account,
    authorization: &WarmupAuthorization,
    model_slug: &str,
    message: &str,
    request_timeout: Option<Duration>,
    upstream_url: &str,
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

    let headers = build_warmup_headers(account, authorization)?;
    let request = client.post(upstream_url).headers(headers).json(&body);
    let request = match request_timeout {
        Some(timeout) => request.timeout(timeout),
        None => request,
    };
    let response = request
        .send()
        .await
        .map_err(|err| format!("warmup request failed: {err}"))?;

    let status = response.status();
    let headers = response.headers().clone();
    if status.is_success() {
        return consume_warmup_stream_async(StreamReader::new(
            response.bytes_stream().map_err(std::io::Error::other),
        ))
        .await;
    }

    let body_text = response.text().await.unwrap_or_default();
    Err(summarize_warmup_error(
        status.as_u16(),
        &headers,
        &body_text,
    ))
}

async fn consume_warmup_stream_async<R: AsyncRead + Unpin>(reader: R) -> Result<(), String> {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    let mut event_name: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .await
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
#[path = "account_warmup_tests.rs"]
mod tests;

pub(crate) fn build_warmup_headers(
    account: &Account,
    authorization: &WarmupAuthorization,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        header_value(&crate::agent_identity::format_upstream_authorization(
            &authorization.value,
        ))?,
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
        header_value(&crate::gateway::current_gateway_user_agent())?,
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
    if let Some(account_header) = authorization
        .account_scope_id
        .clone()
        .or_else(|| workspace_header_for_account(account))
    {
        headers.insert(
            HeaderName::from_static("chatgpt-account-id"),
            header_value(&account_header)?,
        );
    }
    if authorization.is_fedramp {
        headers.insert(
            HeaderName::from_static(X_OPENAI_FEDRAMP_HEADER_NAME),
            HeaderValue::from_static("true"),
        );
    }

    Ok(headers)
}

fn header_value(value: &str) -> Result<HeaderValue, String> {
    HeaderValue::from_str(value).map_err(|err| format!("invalid header value: {err}"))
}

pub(crate) fn summarize_warmup_error(status: u16, headers: &HeaderMap, body: &str) -> String {
    let invalid_agent_task =
        crate::agent_identity::is_agent_identity_task_invalid_response(status, body.as_bytes());
    let body_hint = if invalid_agent_task {
        "invalid_task_id".to_string()
    } else {
        crate::gateway::summarize_upstream_error_hint_from_body(status, body.as_bytes())
            .or_else(|| {
                let trimmed = body.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .unwrap_or_else(|| "unknown error".to_string())
    };

    let request_id = first_header(headers, &["x-request-id", "x-oai-request-id"]);
    let auth_error = first_header(headers, &["x-openai-authorization-error"]);
    let cf_ray = first_header(headers, &["cf-ray"]);

    let mut details = Vec::new();
    if let Some(value) = request_id {
        details.push(format!("request id: {value}"));
    }
    if !invalid_agent_task {
        if let Some(value) = auth_error {
            details.push(format!("auth error: {value}"));
        }
    }
    if let Some(value) = cf_ray {
        details.push(format!("cf-ray: {value}"));
    }
    if invalid_agent_task {
        details.push("agent identity task error: invalid_task_id".to_string());
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
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    if err.to_ascii_lowercase().contains("auth error")
        || err.to_ascii_lowercase().contains("status=401")
        || err.to_ascii_lowercase().contains("status=403")
    {
        let _ = mark_account_unavailable_for_auth_error(storage, account_id, err);
    }
    Ok(())
}

#[cfg(test)]
fn consume_warmup_stream<R: std::io::Read>(mut reader: R) -> Result<(), String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    crate::gateway::run_upstream_io(consume_warmup_stream_async(bytes.as_slice()))?
}

pub(crate) fn schedule_cron_warmup() -> Result<(), String> {
    static SLOT: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    let permit = SLOT
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| "previous account cron warmup still running".to_string())?;
    crate::account::background::spawn("account-cron-warmup", async move {
        let _permit = permit;
        let shutdown = async {
            while !crate::shutdown_requested() {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        };
        tokio::select! {
            biased;
            _ = shutdown => {}
            result = warmup_accounts_async(Vec::new(), "") => match result {
                Ok(result) => log::info!("account warmup cron finished: requested={} succeeded={} failed={}", result.requested, result.succeeded, result.failed),
                Err(error) => log::warn!("account warmup cron error: {error}"),
            }
        }
    })?;
    Ok(())
}

#[cfg(test)]
#[path = "account_warmup_async_tests.rs"]
mod async_network_tests;
