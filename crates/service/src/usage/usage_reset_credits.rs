use codexmanager_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::storage::{Storage, Token};
use codexmanager_core::usage::{ResetCreditConsumeResult, ResetCreditsSnapshot};
use rand::RngCore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::storage_helpers::open_storage;
use crate::usage_account_meta::{derive_account_meta, resolve_workspace_id_for_account};
use crate::usage_http::{
    consume_reset_credit_request, consume_reset_credit_request_with_explicit_proxy,
    fetch_reset_credits_snapshot, fetch_reset_credits_snapshot_with_explicit_proxy,
    log_account_data_route, UsageActionHttpError,
};
use crate::usage_token_refresh::{refresh_and_persist_access_token, token_refresh_ahead_secs};

static RESET_CREDIT_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
const RESET_CREDIT_LOCK_POISONED_MESSAGE: &str = "reset credit lock poisoned; restart CodexManager, verify the account's reset-credit balance and usage state upstream, then retry";

fn reset_credit_lock(account_id: &str) -> Arc<Mutex<()>> {
    let locks = RESET_CREDIT_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks.lock().unwrap_or_else(|poisoned| {
        log::warn!("event=lock_poisoned lock=reset_credit_locks action=recover");
        poisoned.into_inner()
    });
    locks
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

fn usage_base_url() -> String {
    std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string())
}

fn load_token(storage: &Storage, account_id: &str) -> Result<Token, String> {
    let token = storage
        .find_token_by_account_id(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("account has no OAuth token: {account_id}"))?;
    if token.access_token.trim().is_empty() {
        return Err(format!("account access token is empty: {account_id}"));
    }
    Ok(token)
}

fn resolve_workspace_header(storage: &Storage, token: &Token) -> Option<String> {
    resolve_workspace_id_for_account(storage, &token.account_id).or_else(|| {
        let (chatgpt_account_id, workspace_id) = derive_account_meta(token);
        workspace_id.or(chatgpt_account_id)
    })
}

fn refresh_token_for_reset(storage: &Storage, token: &mut Token) -> Result<(), String> {
    if token.refresh_token.trim().is_empty() {
        return Err("account refresh token is empty; please sign in again".to_string());
    }
    let issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    refresh_and_persist_access_token(
        storage,
        token,
        &issuer,
        &client_id,
        token_refresh_ahead_secs(),
    )
}

fn account_proxy_error(message: &str) -> UsageActionHttpError {
    UsageActionHttpError {
        status: None,
        message: message.to_string(),
    }
}

fn fetch_snapshot_for_account(
    account_id: &str,
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
) -> Result<ResetCreditsSnapshot, UsageActionHttpError> {
    let proxy_mode = crate::account_proxy::resolve_account_proxy_mode(account_id);
    log_account_data_route(
        "reset_credit_read",
        account_id,
        &proxy_mode,
        "rate_limit_reset_credits",
        true,
    );
    match &proxy_mode {
        crate::account_proxy::AccountProxyMode::Disabled => {
            fetch_reset_credits_snapshot(base_url, bearer, workspace_id)
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            fetch_reset_credits_snapshot_with_explicit_proxy(
                base_url,
                bearer,
                workspace_id,
                proxy_url,
            )
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
            Err(account_proxy_error(error))
        }
    }
}

fn consume_for_account(
    account_id: &str,
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
    redeem_request_id: &str,
) -> Result<(), UsageActionHttpError> {
    let proxy_mode = crate::account_proxy::resolve_account_proxy_mode(account_id);
    log_account_data_route(
        "reset_credit_consume",
        account_id,
        &proxy_mode,
        "rate_limit_reset_credits_consume",
        true,
    );
    match &proxy_mode {
        crate::account_proxy::AccountProxyMode::Disabled => {
            consume_reset_credit_request(base_url, bearer, workspace_id, redeem_request_id)
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            consume_reset_credit_request_with_explicit_proxy(
                base_url,
                bearer,
                workspace_id,
                redeem_request_id,
                proxy_url,
            )
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
            Err(account_proxy_error(error))
        }
    }
}

fn fetch_snapshot_with_retry(
    storage: &Storage,
    token: &mut Token,
) -> Result<ResetCreditsSnapshot, String> {
    let base_url = usage_base_url();
    let mut workspace_id = resolve_workspace_header(storage, token);
    match fetch_snapshot_for_account(
        token.account_id.as_str(),
        &base_url,
        &token.access_token,
        workspace_id.as_deref(),
    ) {
        Ok(snapshot) => Ok(snapshot),
        Err(error) if error.is_unauthorized() => {
            refresh_token_for_reset(storage, token)?;
            workspace_id = resolve_workspace_header(storage, token);
            fetch_snapshot_for_account(
                token.account_id.as_str(),
                &base_url,
                &token.access_token,
                workspace_id.as_deref(),
            )
            .map_err(|retry_error| retry_error.message)
        }
        Err(error) => Err(error.message),
    }
}

fn consume_with_retry(
    storage: &Storage,
    token: &mut Token,
    redeem_request_id: &str,
) -> Result<(), String> {
    let base_url = usage_base_url();
    let mut workspace_id = resolve_workspace_header(storage, token);
    match consume_for_account(
        token.account_id.as_str(),
        &base_url,
        &token.access_token,
        workspace_id.as_deref(),
        redeem_request_id,
    ) {
        Ok(()) => Ok(()),
        Err(error) if error.is_unauthorized() => {
            refresh_token_for_reset(storage, token)?;
            workspace_id = resolve_workspace_header(storage, token);
            consume_for_account(
                token.account_id.as_str(),
                &base_url,
                &token.access_token,
                workspace_id.as_deref(),
                redeem_request_id,
            )
            .map_err(|retry_error| retry_error.message)
        }
        Err(error) => Err(error.message),
    }
}

fn random_uuid_v4() -> String {
    let mut bytes = [0_u8; 16];
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

pub(crate) fn read_reset_credits(account_id: &str) -> Result<ResetCreditsSnapshot, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut token = load_token(&storage, account_id)?;
    fetch_snapshot_with_retry(&storage, &mut token)
}

pub(crate) fn consume_reset_credit(account_id: &str) -> Result<ResetCreditConsumeResult, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("accountId is required".to_string());
    }

    let account_lock = reset_credit_lock(account_id);
    let _guard = account_lock
        .lock()
        .map_err(|_| RESET_CREDIT_LOCK_POISONED_MESSAGE.to_string())?;
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut token = load_token(&storage, account_id)?;

    let before = fetch_snapshot_with_retry(&storage, &mut token)?;
    if before.available_count.unwrap_or(0) <= 0 {
        return Err("no reset credits are currently available".to_string());
    }

    let redeem_request_id = random_uuid_v4();
    consume_with_retry(&storage, &mut token, &redeem_request_id)?;

    let usage_refresh_error = crate::usage_refresh::refresh_usage_for_account(account_id).err();
    let snapshot_result = read_reset_credits(account_id);
    let snapshot_error = snapshot_result.as_ref().err().cloned();
    let warning = [usage_refresh_error.clone(), snapshot_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");

    Ok(ResetCreditConsumeResult {
        consumed: true,
        usage_refreshed: usage_refresh_error.is_none(),
        snapshot: snapshot_result.ok(),
        warning: (!warning.is_empty()).then_some(warning),
    })
}

#[cfg(test)]
mod tests {
    use super::{random_uuid_v4, RESET_CREDIT_LOCK_POISONED_MESSAGE};

    #[test]
    fn generated_redeem_request_id_is_uuid_v4() {
        let value = random_uuid_v4();
        assert_eq!(value.len(), 36);
        assert_eq!(&value[14..15], "4");
        assert!(matches!(&value[19..20], "8" | "9" | "a" | "b"));
        assert_eq!(
            value.chars().filter(|character| *character == '-').count(),
            4
        );
    }

    #[test]
    fn poisoned_lock_message_is_actionable() {
        assert!(RESET_CREDIT_LOCK_POISONED_MESSAGE.contains("restart CodexManager"));
        assert!(RESET_CREDIT_LOCK_POISONED_MESSAGE.contains("verify"));
        assert!(RESET_CREDIT_LOCK_POISONED_MESSAGE.contains("then retry"));
    }
}
