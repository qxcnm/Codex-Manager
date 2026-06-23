use codexmanager_core::auth::{extract_client_id_claim, extract_token_exp, DEFAULT_CLIENT_ID};
use codexmanager_core::storage::{now_ts, Storage, Token};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::auth_tokens::obtain_api_key;
use crate::usage_http::{
    log_account_data_route, refresh_access_token, refresh_access_token_with_explicit_proxy,
    refresh_token_auth_error_reason_from_message, RefreshTokenAuthErrorReason,
};

pub(crate) const DEFAULT_TOKEN_REFRESH_AHEAD_SECS: i64 = 3600;
pub(crate) const ENV_TOKEN_REFRESH_AHEAD_SECS: &str = "CODEXMANAGER_TOKEN_REFRESH_AHEAD_SECS";

static TOKEN_REFRESH_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

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
pub(crate) fn refresh_and_persist_access_token(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
    refresh_ahead_secs: i64,
) -> Result<(), String> {
    let original_access_token = token.access_token.clone();
    let original_refresh_token = token.refresh_token.clone();
    let refresh_lock = token_refresh_lock_for_account(&token.account_id);
    let _refresh_guard = refresh_lock
        .lock()
        .map_err(|_| "token refresh lock poisoned".to_string())?;

    if let Some(latest) = storage
        .find_token_by_account_id(&token.account_id)
        .map_err(|err| err.to_string())?
    {
        if latest.access_token != original_access_token
            || latest.refresh_token != original_refresh_token
        {
            *token = latest;
            return Ok(());
        }
        *token = latest;
    }

    let refresh_client_id = token_refresh_client_id(token, client_id);
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
            refresh_access_token(issuer, &refresh_client_id, &token.refresh_token)
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            refresh_access_token_with_explicit_proxy(
                issuer,
                &refresh_client_id,
                &token.refresh_token,
                proxy_url,
            )
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => Err(error.clone()),
    };
    let refreshed = match refreshed {
        Ok(refreshed) => refreshed,
        Err(err) => {
            if recover_refresh_race_from_latest_token(
                storage,
                token,
                &original_refresh_token,
                err.as_str(),
            )? {
                return Ok(());
            }
            return Err(err);
        }
    };
    token.access_token = refreshed.access_token;

    if let Some(refresh_token) = refreshed.refresh_token {
        token.refresh_token = refresh_token;
    }

    if let Some(id_token) = refreshed.id_token {
        token.id_token = id_token.clone();
        let exchange_client_id = token_refresh_client_id(token, refresh_client_id.as_str());
        if let Ok(api_key) = obtain_api_key(issuer, &exchange_client_id, &id_token) {
            token.api_key_access_token = Some(api_key);
        }
    }

    token.last_refresh = now_ts();
    storage.insert_token(token).map_err(|err| err.to_string())?;
    let access_exp = extract_token_exp(&token.access_token);
    let next_refresh_at = next_refresh_at_from_token(token, refresh_ahead_secs);
    let _ = storage.update_token_refresh_schedule(&token.account_id, access_exp, next_refresh_at);
    Ok(())
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

fn token_refresh_lock_for_account(account_id: &str) -> Arc<Mutex<()>> {
    let locks = TOKEN_REFRESH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
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
