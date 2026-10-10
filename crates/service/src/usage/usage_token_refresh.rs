use codexmanager_core::auth::{extract_client_id_claim, extract_token_exp, DEFAULT_CLIENT_ID};
use codexmanager_core::storage::{now_ts, Storage, Token};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::auth_tokens::obtain_api_key_async;
use crate::usage_http::{
    log_account_data_route, refresh_access_token_async,
    refresh_token_auth_error_reason_from_message, RefreshTokenAuthErrorReason,
};

pub(crate) const DEFAULT_TOKEN_REFRESH_AHEAD_SECS: i64 = 3600;
pub(crate) const ENV_TOKEN_REFRESH_AHEAD_SECS: &str = "CODEXMANAGER_TOKEN_REFRESH_AHEAD_SECS";

static TOKEN_REFRESH_LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();

/// 函数 `refresh_and_persist_access_token`
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
// Refresh grants may rotate on the provider before the response is delivered.
// Once admitted, completion owns the grant and persists it even if the request
// waiting for it is cancelled. Admission and per-account lock waits remain cancellable.
static REFRESH_COMPLETION_REGISTRATION: Mutex<()> = Mutex::new(());
static REFRESH_COMPLETION_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);
static ACTIVE_REFRESH_COMPLETIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static REFRESH_COMPLETIONS_CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();

struct RefreshCompletionGuard;
impl Drop for RefreshCompletionGuard {
    fn drop(&mut self) {
        ACTIVE_REFRESH_COMPLETIONS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        REFRESH_COMPLETIONS_CHANGED.notify_waiters();
    }
}

pub(crate) async fn drain_token_refresh_tasks() {
    loop {
        let changed = REFRESH_COMPLETIONS_CHANGED.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let active = {
            let _registration = crate::lock_utils::lock_recover(
                &REFRESH_COMPLETION_REGISTRATION,
                "token_refresh_completion_registration",
            );
            ACTIVE_REFRESH_COMPLETIONS.load(std::sync::atomic::Ordering::Acquire)
        };
        if active == 0 {
            return;
        }
        changed.await;
    }
}

pub(crate) async fn refresh_and_persist_access_token_async(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
    refresh_ahead_secs: i64,
) -> Result<(), String> {
    if crate::shutdown_requested() {
        return Err("token refresh cancelled during shutdown".to_owned());
    }
    let refresh_lock = token_refresh_lock_for_account(&token.account_id);
    let refresh_guard =
        crate::http::gateway_request::with_response_cancellation(refresh_lock.lock_owned())
            .await
            .map_err(|_| "token refresh cancelled".to_owned())?;
    let permit = crate::http::gateway_request::with_response_cancellation(
        REFRESH_COMPLETION_SLOTS.acquire(),
    )
    .await
    .map_err(|_| "token refresh cancelled".to_owned())?
    .map_err(|_| "token refresh completion unavailable".to_owned())?;
    if crate::shutdown_requested() {
        return Err("token refresh cancelled during shutdown".to_owned());
    }
    let runtime = crate::account::background::runtime()?;
    let storage = storage.shared_handle();
    let input = token.clone();
    let issuer = issuer.to_owned();
    let client_id = client_id.to_owned();
    let completion_guard = {
        let _registration = crate::lock_utils::lock_recover(
            &REFRESH_COMPLETION_REGISTRATION,
            "token_refresh_completion_registration",
        );
        if crate::shutdown_requested() {
            return Err("token refresh cancelled during shutdown".to_owned());
        }
        ACTIVE_REFRESH_COMPLETIONS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        RefreshCompletionGuard
    };
    let task = runtime.spawn(async move {
        let (_permit, _refresh_guard, _completion_guard) =
            (permit, refresh_guard, completion_guard);
        complete_token_refresh(&storage, input, &issuer, &client_id, refresh_ahead_secs).await
    });
    *token = crate::http::gateway_request::with_response_cancellation(task)
        .await
        .map_err(|_| "token refresh caller cancelled; credential persistence continues".to_owned())?
        .map_err(|_| "token refresh completion interrupted".to_owned())??;
    Ok(())
}

async fn complete_token_refresh(
    storage: &Storage,
    mut token: Token,
    issuer: &str,
    client_id: &str,
    refresh_ahead_secs: i64,
) -> Result<Token, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let original_access_token = token.access_token.clone();
    let original_refresh_token = token.refresh_token.clone();
    let latest = storage
        .find_token_by_account_id(&token.account_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "token was removed before refresh could start".to_owned())?;
    if latest.access_token != original_access_token
        || latest.refresh_token != original_refresh_token
    {
        return Ok(latest);
    }
    token = latest;
    let expected = token.clone();
    let refresh_client_id = token_refresh_client_id(&token, client_id);
    let proxy_mode = crate::account_proxy::resolve_account_proxy_mode(token.account_id.as_str());
    log_account_data_route(
        "token_refresh",
        token.account_id.as_str(),
        &proxy_mode,
        "refresh_token",
        true,
    );
    let refreshed = match &proxy_mode {
        crate::account_proxy::AccountProxyMode::Disabled => {
            refresh_access_token_async(issuer, &refresh_client_id, &token.refresh_token, None).await
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            refresh_access_token_async(
                issuer,
                &refresh_client_id,
                &token.refresh_token,
                Some(proxy_url),
            )
            .await
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => Err(error.clone()),
    };
    let refreshed = match refreshed {
        Ok(refreshed) => refreshed,
        Err(error) => {
            if recover_refresh_race_from_latest_token(
                storage,
                &mut token,
                &original_refresh_token,
                &error,
            )? {
                return Ok(token);
            }
            return Err(error);
        }
    };
    token.access_token = refreshed.access_token;
    if let Some(refresh_token) = refreshed.refresh_token {
        token.refresh_token = refresh_token;
    }
    let new_id_token = refreshed.id_token;
    if let Some(id_token) = &new_id_token {
        token.id_token = id_token.clone();
    }
    token.last_refresh = now_ts();
    // Persist the rotated grant before optional follow-up HTTP. The second CAS
    // below cannot overwrite a concurrent import, another refresh, or deletion.
    if !storage
        .compare_and_swap_token(&expected, &token)
        .map_err(|err| err.to_string())?
    {
        return storage
            .find_token_by_account_id(&expected.account_id)
            .map_err(|err| err.to_string())?
            .ok_or_else(|| {
                "token was removed before refreshed credentials could be persisted".to_owned()
            });
    }
    let access_exp = extract_token_exp(&token.access_token);
    let next_refresh_at = next_refresh_at_from_token(&token, refresh_ahead_secs);
    let _ = storage.update_token_refresh_schedule(&token.account_id, access_exp, next_refresh_at);
    if let Some(id_token) = new_id_token {
        let exchange_client_id =
            crate::gateway::api_key_exchange_client_id(&token, &refresh_client_id);
        if let Ok(api_key) = obtain_api_key_async(issuer, &exchange_client_id, &id_token).await {
            let granted = token.clone();
            token.api_key_access_token = Some(api_key);
            if !storage
                .compare_and_swap_token(&granted, &token)
                .map_err(|err| err.to_string())?
            {
                return storage
                    .find_token_by_account_id(&granted.account_id)
                    .map_err(|err| err.to_string())?
                    .ok_or_else(|| {
                        "token was removed before API key credentials could be persisted".to_owned()
                    });
            }
        }
    }
    Ok(token)
}

pub(crate) fn token_refresh_ahead_secs() -> i64 {
    std::env::var(ENV_TOKEN_REFRESH_AHEAD_SECS)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_TOKEN_REFRESH_AHEAD_SECS)
}

pub(crate) fn token_refresh_client_id(token: &Token, fallback_client_id: &str) -> String {
    extract_client_id_claim(&token.access_token)
        .or_else(|| extract_client_id_claim(&token.id_token))
        .or_else(|| {
            let fallback = fallback_client_id.trim();
            (!fallback.is_empty()).then(|| fallback.to_string())
        })
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
}

fn token_refresh_lock_for_account(account_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let locks = TOKEN_REFRESH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn next_refresh_at_from_token(token: &Token, ahead_secs: i64) -> Option<i64> {
    let access_refresh_at =
        extract_token_exp(&token.access_token).map(|exp| exp.saturating_sub(ahead_secs));
    let refresh_refresh_at =
        extract_token_exp(&token.refresh_token).map(|exp| exp.saturating_sub(ahead_secs));

    match (access_refresh_at, refresh_refresh_at) {
        (Some(access_at), Some(refresh_at)) => Some(access_at.min(refresh_at)),
        (Some(access_at), None) => Some(access_at),
        (None, Some(refresh_at)) => Some(refresh_at),
        (None, None) => None,
    }
}

fn recover_refresh_race_from_latest_token(
    storage: &Storage,
    token: &mut Token,
    original_refresh_token: &str,
    err: &str,
) -> Result<bool, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    if !is_refresh_race_recoverable_error(err) {
        return Ok(false);
    }

    let Some(latest) = storage
        .find_token_by_account_id(&token.account_id)
        .map_err(|err| err.to_string())?
    else {
        return Ok(false);
    };

    if latest.refresh_token.trim().is_empty() || latest.refresh_token == original_refresh_token {
        return Ok(false);
    }

    *token = latest;
    Ok(true)
}

fn is_refresh_race_recoverable_error(err: &str) -> bool {
    matches!(
        refresh_token_auth_error_reason_from_message(err),
        Some(RefreshTokenAuthErrorReason::InvalidGrant | RefreshTokenAuthErrorReason::Reused)
    )
}

#[cfg(test)]
#[path = "usage_token_refresh_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn refresh_and_persist_access_token(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
    refresh_ahead_secs: i64,
) -> Result<(), String> {
    crate::gateway::run_upstream_io(refresh_and_persist_access_token_async(
        storage,
        token,
        issuer,
        client_id,
        refresh_ahead_secs,
    ))?
}
