use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use codexmanager_core::auth::extract_token_exp;
use codexmanager_core::storage::{now_ts, Account, Storage, Token};

use crate::account_status::mark_account_unavailable_for_auth_error;
use crate::auth_tokens;
use crate::usage_http::{
    log_account_data_route, refresh_access_token, refresh_access_token_with_explicit_proxy,
};

const ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS: i64 = 30 * 60;
const ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS: i64 = 60;
const API_KEY_ACCESS_TOKEN_REFRESH_AHEAD_SECS: i64 = 60;

struct AccountTokenExchangeLockEntry {
    lock: Arc<Mutex<()>>,
    last_seen_at: i64,
}

#[derive(Default)]
struct AccountTokenExchangeLockTable {
    entries: HashMap<String, AccountTokenExchangeLockEntry>,
    last_cleanup_at: i64,
}

static ACCOUNT_TOKEN_EXCHANGE_LOCKS: OnceLock<Mutex<AccountTokenExchangeLockTable>> =
    OnceLock::new();

/// 函数 `account_token_exchange_lock`
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
pub(super) fn account_token_exchange_lock(account_id: &str) -> Arc<Mutex<()>> {
    let lock = ACCOUNT_TOKEN_EXCHANGE_LOCKS
        .get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
    let mut table = crate::lock_utils::lock_recover(lock, "account_token_exchange_locks");
    let now = now_ts();
    maybe_cleanup_exchange_locks(&mut table, now);
    let entry = table
        .entries
        .entry(account_id.to_string())
        .or_insert_with(|| AccountTokenExchangeLockEntry {
            lock: Arc::new(Mutex::new(())),
            last_seen_at: now,
        });
    entry.last_seen_at = now;
    entry.lock.clone()
}

/// 函数 `maybe_cleanup_exchange_locks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - table: 参数 table
/// - now: 参数 now
///
/// # 返回
/// 无
fn maybe_cleanup_exchange_locks(table: &mut AccountTokenExchangeLockTable, now: i64) {
    if table.last_cleanup_at != 0
        && now.saturating_sub(table.last_cleanup_at)
            < ACCOUNT_TOKEN_EXCHANGE_LOCK_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    table.last_cleanup_at = now;
    table.entries.retain(|_, entry| {
        let stale = now.saturating_sub(entry.last_seen_at) > ACCOUNT_TOKEN_EXCHANGE_LOCK_TTL_SECS;
        !stale || Arc::strong_count(&entry.lock) > 1
    });
}

/// 函数 `find_cached_api_key_access_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn find_cached_api_key_access_token(storage: &Storage, account_id: &str) -> Option<String> {
    storage
        .find_token_by_account_id(account_id)
        .ok()?
        .and_then(|t| t.api_key_access_token)
        .and_then(|value| usable_api_key_access_token(&value))
}

fn usable_api_key_access_token(value: &str) -> Option<String> {
    let token = value.trim();
    if token.is_empty() {
        return None;
    }
    if access_token_expires_within(token, API_KEY_ACCESS_TOKEN_REFRESH_AHEAD_SECS) {
        return None;
    }
    Some(token.to_string())
}

fn access_token_expires_within(token: &str, ahead_secs: i64) -> bool {
    extract_token_exp(token)
        .map(|exp| exp <= now_ts().saturating_add(ahead_secs))
        .unwrap_or(false)
}

/// 函数 `exchange_and_persist_api_key_access_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - token: 参数 token
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
///
/// # 返回
/// 返回函数执行结果
fn exchange_and_persist_api_key_access_token(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
) -> Result<String, String> {
    let mut errors = Vec::new();
    for subject_token in api_key_exchange_subject_tokens(token) {
        match auth_tokens::obtain_api_key(issuer, client_id, &subject_token) {
            Ok(exchanged) => {
                token.api_key_access_token = Some(exchanged.clone());
                let _ = storage.insert_token(token);
                return Ok(exchanged);
            }
            Err(err) => errors.push(err),
        }
    }

    let exchange_error = errors
        .into_iter()
        .next()
        .unwrap_or_else(|| "api key exchange subject token is missing".to_string());
    Err(exchange_error)
}

fn api_key_exchange_subject_tokens(token: &Token) -> Vec<String> {
    let mut subjects = Vec::new();
    // 中文注释：直接导入 api/auth/session JSON 时通常只有 accessToken；
    // 登录授权路径已优先缓存 api_key_access_token，缓存缺失时也先按最新 access_token 兑换。
    push_unique_subject_token(&mut subjects, token.access_token.as_str());
    push_unique_subject_token(&mut subjects, token.id_token.as_str());
    subjects
}

fn push_unique_subject_token(subjects: &mut Vec<String>, candidate: &str) {
    let value = candidate.trim();
    if value.is_empty() || subjects.iter().any(|existing| existing == value) {
        return;
    }
    subjects.push(value.to_string());
}

/// 函数 `fallback_to_access_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
/// - exchange_error: 参数 exchange_error
///
/// # 返回
/// 返回函数执行结果
fn fallback_to_access_token(token: &Token, exchange_error: &str) -> Result<String, String> {
    let fallback = token.access_token.trim();
    if fallback.is_empty() {
        return Err(exchange_error.to_string());
    }
    log::warn!(
        "api_key_access_token exchange unavailable; fallback to access_token: {}",
        exchange_error
    );
    Ok(fallback.to_string())
}

fn should_mark_account_unavailable_after_refresh_failure_for_bearer_exchange(
    token: &Token,
) -> bool {
    let fallback = token.access_token.trim();
    if fallback.is_empty() {
        return true;
    }

    match extract_token_exp(fallback) {
        Some(exp) => exp <= now_ts(),
        None => false,
    }
}

/// 函数 `resolve_openai_bearer_token`
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
pub(super) fn resolve_openai_bearer_token(
    storage: &Storage,
    account: &Account,
    token: &mut Token,
) -> Result<String, String> {
    if let Some(existing) = token
        .api_key_access_token
        .as_deref()
        .and_then(usable_api_key_access_token)
    {
        return Ok(existing);
    }

    let exchange_lock = account_token_exchange_lock(&account.id);
    let _guard =
        crate::lock_utils::lock_recover(exchange_lock.as_ref(), "account_token_exchange_lock");

    if let Some(existing) = token
        .api_key_access_token
        .as_deref()
        .and_then(usable_api_key_access_token)
    {
        return Ok(existing);
    }

    if let Some(cached) = find_cached_api_key_access_token(storage, &account.id) {
        // 中文注释：并发下后到线程优先复用已落库的新 token，避免重复 token exchange 打上游。
        token.api_key_access_token = Some(cached.clone());
        return Ok(cached);
    }

    let fallback_client_id = super::runtime_config::token_exchange_client_id();
    let client_id = crate::usage_token_refresh::token_refresh_client_id(token, &fallback_client_id);
    let issuer_env = super::runtime_config::token_exchange_default_issuer();
    let issuer = if account.issuer.trim().is_empty() {
        issuer_env
    } else {
        account.issuer.clone()
    };

    match exchange_and_persist_api_key_access_token(storage, token, &issuer, &client_id) {
        Ok(token) => return Ok(token),
        Err(exchange_err) => {
            if !token.refresh_token.trim().is_empty() {
                let proxy_mode =
                    crate::account_proxy::resolve_account_proxy_mode(account.id.as_str());
                log_account_data_route(
                    "api_key_token_exchange",
                    account.id.as_str(),
                    &proxy_mode,
                    "refresh_token",
                    true,
                );
                let refresh_result = match &proxy_mode {
                    crate::account_proxy::AccountProxyMode::Disabled => {
                        refresh_access_token(&issuer, &client_id, &token.refresh_token)
                    }
                    crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
                        refresh_access_token_with_explicit_proxy(
                            &issuer,
                            &client_id,
                            &token.refresh_token,
                            proxy_url,
                        )
                    }
                    crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
                        Err(error.clone())
                    }
                };
                match refresh_result {
                    Ok(refreshed) => {
                        token.access_token = refreshed.access_token;
                        if let Some(refresh_token) = refreshed.refresh_token {
                            token.refresh_token = refresh_token;
                        }
                        if let Some(id_token) = refreshed.id_token {
                            token.id_token = id_token;
                        }
                        let _ = storage.insert_token(token);

                        if !token.id_token.trim().is_empty() {
                            let refreshed_client_id =
                                crate::usage_token_refresh::token_refresh_client_id(
                                    token, &client_id,
                                );
                            if let Ok(exchanged) = exchange_and_persist_api_key_access_token(
                                storage,
                                token,
                                &issuer,
                                &refreshed_client_id,
                            ) {
                                return Ok(exchanged);
                            }
                        }
                    }
                    Err(refresh_err) => {
                        if should_mark_account_unavailable_after_refresh_failure_for_bearer_exchange(
                            token,
                        ) && mark_account_unavailable_for_auth_error(
                            storage,
                            &account.id,
                            &refresh_err,
                        ) {
                            return Err(refresh_err);
                        }
                        log::warn!(
                            "refresh token before api_key_access_token exchange failed: {}",
                            refresh_err
                        );
                    }
                }
            }

            fallback_to_access_token(token, &exchange_err)
        }
    }
}

/// 函数 `clear_account_token_exchange_locks_for_tests`
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
#[cfg(test)]
fn clear_account_token_exchange_locks_for_tests() {
    let lock = ACCOUNT_TOKEN_EXCHANGE_LOCKS
        .get_or_init(|| Mutex::new(AccountTokenExchangeLockTable::default()));
    if let Ok(mut table) = lock.lock() {
        table.entries.clear();
        table.last_cleanup_at = 0;
    }
}

#[cfg(test)]
#[path = "tests/token_exchange_tests.rs"]
mod tests;
