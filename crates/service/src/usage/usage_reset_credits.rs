use codexmanager_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::storage::{
    now_ts, ResetCreditOperation, ResetCreditOperationClaim, ResetCreditOperationStatus,
    ResetCreditOperationUpdate, Storage, Token,
};
use codexmanager_core::usage::{ResetCreditConsumeResult, ResetCreditsSnapshot};
use rand::RngCore;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::storage_helpers::open_storage;
use crate::usage_account_meta::{
    clean_header_value, derive_account_meta, resolve_workspace_id_for_account,
};
use crate::usage_http::{
    consume_reset_credit_request_async, fetch_reset_credits_snapshot_async, log_account_data_route,
    UsageActionHttpError,
};
use crate::usage_token_refresh::{
    refresh_and_persist_access_token_async, token_refresh_ahead_secs,
};

#[derive(Default)]
struct ResetCreditState {
    gate: tokio::sync::Mutex<()>,
    interrupted: AtomicBool,
}

static RESET_CREDIT_LOCKS: OnceLock<Mutex<HashMap<String, Arc<ResetCreditState>>>> =
    OnceLock::new();
const RESET_CREDIT_LOCK_POISONED_MESSAGE: &str = "reset credit lock poisoned; restart CodexManager, verify the account's reset-credit balance and usage state upstream, then retry";
const RESET_CREDIT_RESULT_UNKNOWN_MESSAGE: &str = "reset credit operation result is unknown; refresh the reset-credit balance and usage state before trying again";
const RESET_CREDIT_PENDING_OPERATION_PREFIX: &str = "reset_credit_pending_operation:";
const RESET_CREDIT_TERMINAL_FAILURE_PREFIX: &str = "reset_credit_terminal_failure:";

fn reset_credit_lock(account_id: &str) -> Arc<ResetCreditState> {
    let locks = RESET_CREDIT_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(ResetCreditState::default()))
        .clone()
}

fn usage_base_url() -> String {
    std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string())
}

fn load_token(storage: &Storage, account_id: &str) -> Result<Token, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let token = storage
        .find_token_by_account_id(account_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("account has no OAuth token: {account_id}"))?;
    if token.access_token.trim().is_empty() {
        return Err(format!("account access token is empty: {account_id}"));
    }
    Ok(token)
}

fn resolve_account_header(storage: &Storage, token: &Token) -> Option<String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let (token_chatgpt_account_id, token_workspace_id) = derive_account_meta(token);
    clean_header_value(token_chatgpt_account_id)
        .or_else(|| {
            storage
                .find_account_workspace_identity_by_id(&token.account_id)
                .ok()
                .flatten()
                .and_then(|identity| clean_header_value(identity.chatgpt_account_id))
        })
        .or(token_workspace_id)
        .or_else(|| resolve_workspace_id_for_account(storage, &token.account_id))
}

async fn refresh_token_for_reset(storage: &Storage, token: &mut Token) -> Result<(), String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    if token.refresh_token.trim().is_empty() {
        return Err("account refresh token is empty; please sign in again".to_string());
    }
    let issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    refresh_and_persist_access_token_async(
        storage,
        token,
        &issuer,
        &client_id,
        token_refresh_ahead_secs(),
    )
    .await
}

fn account_proxy_error(message: &str) -> UsageActionHttpError {
    UsageActionHttpError {
        status: None,
        message: message.to_string(),
    }
}

async fn fetch_snapshot_for_account(
    account_id: &str,
    base_url: &str,
    bearer: &str,
    chatgpt_account_id: Option<&str>,
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
            fetch_reset_credits_snapshot_async(base_url, bearer, chatgpt_account_id, None).await
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            fetch_reset_credits_snapshot_async(
                base_url,
                bearer,
                chatgpt_account_id,
                Some(proxy_url),
            )
            .await
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
            Err(account_proxy_error(error))
        }
    }
}

async fn consume_for_account(
    account_id: &str,
    base_url: &str,
    bearer: &str,
    chatgpt_account_id: Option<&str>,
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
            consume_reset_credit_request_async(
                base_url,
                bearer,
                chatgpt_account_id,
                redeem_request_id,
                None,
            )
            .await
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            consume_reset_credit_request_async(
                base_url,
                bearer,
                chatgpt_account_id,
                redeem_request_id,
                Some(proxy_url),
            )
            .await
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
            Err(account_proxy_error(error))
        }
    }
}

async fn fetch_snapshot_with_retry(
    storage: &Storage,
    token: &mut Token,
) -> Result<ResetCreditsSnapshot, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let base_url = usage_base_url();
    let mut chatgpt_account_id = resolve_account_header(storage, token);
    match fetch_snapshot_for_account(
        token.account_id.as_str(),
        &base_url,
        &token.access_token,
        chatgpt_account_id.as_deref(),
    )
    .await
    {
        Ok(snapshot) => Ok(snapshot),
        Err(error) if error.is_unauthorized() => {
            refresh_token_for_reset(storage, token).await?;
            chatgpt_account_id = resolve_account_header(storage, token);
            fetch_snapshot_for_account(
                token.account_id.as_str(),
                &base_url,
                &token.access_token,
                chatgpt_account_id.as_deref(),
            )
            .await
            .map_err(|retry_error| retry_error.message)
        }
        Err(error) => Err(error.message),
    }
}

async fn consume_with_retry(
    storage: &Storage,
    token: &mut Token,
    redeem_request_id: &str,
) -> Result<(), UsageActionHttpError> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let base_url = usage_base_url();
    let mut chatgpt_account_id = resolve_account_header(storage, token);
    match consume_for_account(
        token.account_id.as_str(),
        &base_url,
        &token.access_token,
        chatgpt_account_id.as_deref(),
        redeem_request_id,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(error) if error.is_unauthorized() => {
            refresh_token_for_reset(storage, token)
                .await
                .map_err(|message| UsageActionHttpError {
                    // The first request was rejected with a definitive 401;
                    // a refresh failure cannot make that request ambiguous.
                    status: Some(reqwest::StatusCode::UNAUTHORIZED.as_u16()),
                    message,
                })?;
            chatgpt_account_id = resolve_account_header(storage, token);
            consume_for_account(
                token.account_id.as_str(),
                &base_url,
                &token.access_token,
                chatgpt_account_id.as_deref(),
                redeem_request_id,
            )
            .await
        }
        Err(error) => Err(error),
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

fn is_uuid_v4(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36
        || [8, 13, 18, 23]
            .into_iter()
            .any(|index| bytes[index] != b'-')
        || bytes[14] != b'4'
        || !matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
    {
        return false;
    }
    bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}

fn stored_operation_result(
    operation: &ResetCreditOperation,
) -> Result<ResetCreditConsumeResult, String> {
    match operation.status {
        ResetCreditOperationStatus::Completed => operation
            .result_json
            .as_deref()
            .ok_or_else(|| "completed reset credit operation has no saved result".to_string())
            .and_then(|value| {
                serde_json::from_str(value)
                    .map_err(|error| format!("read saved reset credit result failed: {error}"))
            }),
        ResetCreditOperationStatus::Failed => Err(format!(
            "{RESET_CREDIT_TERMINAL_FAILURE_PREFIX}{}",
            operation
                .error
                .as_deref()
                .unwrap_or("reset credit operation failed")
        )),
        ResetCreditOperationStatus::Pending => Err(format!(
            "{RESET_CREDIT_PENDING_OPERATION_PREFIX}{}; {RESET_CREDIT_RESULT_UNKNOWN_MESSAGE}",
            operation.operation_id
        )),
    }
}

fn fail_operation(
    storage: &crate::account::remote_storage::AccountStorage<'_>,
    operation_id: &str,
    account_id: &str,
    error: String,
) -> Result<ResetCreditConsumeResult, String> {
    match storage.fail_reset_credit_operation(operation_id, account_id, &error, now_ts()) {
        Ok(ResetCreditOperationUpdate::Updated(operation))
        | Ok(ResetCreditOperationUpdate::Existing(operation)) => {
            stored_operation_result(&operation)
        }
        Ok(ResetCreditOperationUpdate::AccountConflict(_)) => Err(format!(
            "{RESET_CREDIT_TERMINAL_FAILURE_PREFIX}{error}; operationId belongs to a different account"
        )),
        Ok(ResetCreditOperationUpdate::NotFound) => Err(format!(
            "{RESET_CREDIT_TERMINAL_FAILURE_PREFIX}{error}; reset credit operation record is missing"
        )),
        Err(persist_error) => Err(format!(
            "{error}; persist reset credit failure failed: {persist_error}; {RESET_CREDIT_RESULT_UNKNOWN_MESSAGE}"
        )),
    }
}

fn complete_operation(
    storage: &crate::account::remote_storage::AccountStorage<'_>,
    operation_id: &str,
    account_id: &str,
    result: &ResetCreditConsumeResult,
) -> Result<(), String> {
    let result_json = serde_json::to_string(result)
        .map_err(|error| format!("serialize reset credit result failed: {error}"))?;
    match storage.complete_reset_credit_operation(operation_id, account_id, &result_json, now_ts())
    {
        Ok(ResetCreditOperationUpdate::Updated(_)) => Ok(()),
        Ok(ResetCreditOperationUpdate::Existing(operation))
            if operation.status == ResetCreditOperationStatus::Completed =>
        {
            Ok(())
        }
        Ok(ResetCreditOperationUpdate::Existing(_)) => {
            Err("reset credit operation reached an unexpected terminal state".to_string())
        }
        Ok(ResetCreditOperationUpdate::AccountConflict(_)) => {
            Err("operationId belongs to a different account".to_string())
        }
        Ok(ResetCreditOperationUpdate::NotFound) => {
            Err("reset credit operation record is missing".to_string())
        }
        Err(error) => Err(format!("persist reset credit completion failed: {error}")),
    }
}

pub(crate) fn read_reset_credits(account_id: &str) -> Result<ResetCreditsSnapshot, String> {
    crate::gateway::run_upstream_io(read_reset_credits_async(account_id))?
}

pub(crate) async fn read_reset_credits_async(
    account_id: &str,
) -> Result<ResetCreditsSnapshot, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let storage = open_storage()
        .map(|storage| storage.shared_handle())
        .ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let mut token = load_token(&storage, account_id)?;
    fetch_snapshot_with_retry(&storage, &mut token).await
}

pub(crate) fn consume_reset_credit(
    account_id: &str,
    operation_id: &str,
) -> Result<ResetCreditConsumeResult, String> {
    crate::gateway::run_upstream_io(consume_reset_credit_async(account_id, operation_id))?
}

pub(crate) async fn consume_reset_credit_async(
    account_id: &str,
    operation_id: &str,
) -> Result<ResetCreditConsumeResult, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let operation_id = operation_id.trim();
    if !is_uuid_v4(operation_id) {
        return Err("operationId must be a UUID v4".to_string());
    }

    let account_lock = reset_credit_lock(account_id);
    let _guard = account_lock.gate.lock().await;
    let storage = open_storage()
        .map(|storage| storage.shared_handle())
        .ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let candidate_redeem_request_id = random_uuid_v4();
    let resume_redeem_request_id = match storage
        .claim_reset_credit_operation(
            operation_id,
            account_id,
            &candidate_redeem_request_id,
            now_ts(),
        )
        .map_err(|error| format!("claim reset credit operation failed: {error}"))?
    {
        ResetCreditOperationClaim::Created(_) => None,
        ResetCreditOperationClaim::Existing(operation) => {
            if operation.status == ResetCreditOperationStatus::Pending {
                Some(operation.redeem_request_id)
            } else {
                return stored_operation_result(&operation);
            }
        }
        ResetCreditOperationClaim::PendingAccount(operation) => {
            return stored_operation_result(&operation);
        }
        ResetCreditOperationClaim::AccountConflict(_) => {
            return Err("operationId belongs to a different account".to_string());
        }
    };
    let is_resume = resume_redeem_request_id.is_some();
    let redeem_request_id = resume_redeem_request_id.unwrap_or(candidate_redeem_request_id);
    if !is_resume && account_lock.interrupted.load(Ordering::Acquire) {
        return fail_operation(
            storage,
            operation_id,
            account_id,
            RESET_CREDIT_LOCK_POISONED_MESSAGE.to_string(),
        );
    }
    let mut token = match load_token(&storage, account_id) {
        Ok(token) => token,
        Err(error) => {
            return fail_operation(storage, operation_id, account_id, error);
        }
    };

    if !is_resume {
        let before = match fetch_snapshot_with_retry(&storage, &mut token).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return fail_operation(storage, operation_id, account_id, error);
            }
        };
        if before.available_count.unwrap_or(0) <= 0 {
            return fail_operation(
                storage,
                operation_id,
                account_id,
                "no reset credits are currently available".to_string(),
            );
        }
    }

    // A replay of the same operation reuses the durable redeem id. If the first
    // request reached the provider, its idempotency key lets the provider return
    // the original outcome instead of charging a second credit.
    account_lock.interrupted.store(true, Ordering::Release);
    let consume_result = consume_with_retry(&storage, &mut token, &redeem_request_id).await;
    account_lock.interrupted.store(false, Ordering::Release);
    if let Err(error) = consume_result {
        if error.is_ambiguous_non_idempotent() {
            return Err(format!(
                "{}; {}",
                error.message, RESET_CREDIT_RESULT_UNKNOWN_MESSAGE
            ));
        }
        return fail_operation(storage, operation_id, account_id, error.message);
    }

    // Commit the provider-confirmed result before any follow-up query. A slow
    // usage refresh must never turn a completed redemption into a replay.
    let committed_result = ResetCreditConsumeResult {
        consumed: true,
        usage_refreshed: false,
        snapshot: None,
        warning: Some("usage refresh is still pending; refresh manually if needed".to_string()),
    };
    if let Err(error) = complete_operation(storage, operation_id, account_id, &committed_result) {
        account_lock.interrupted.store(true, Ordering::Release);
        return Err(format!(
            "reset credit was accepted upstream but its local result could not be recorded; {error}; {}",
            RESET_CREDIT_RESULT_UNKNOWN_MESSAGE
        ));
    }

    let usage_refresh_error = crate::usage_refresh::refresh_usage_for_account_async(account_id)
        .await
        .err();
    let snapshot_result = read_reset_credits_async(account_id).await;
    let snapshot_error = snapshot_result.as_ref().err().cloned();
    let warning = [usage_refresh_error.clone(), snapshot_error]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");

    let result = ResetCreditConsumeResult {
        consumed: true,
        usage_refreshed: usage_refresh_error.is_none(),
        snapshot: snapshot_result.ok(),
        warning: (!warning.is_empty()).then_some(warning),
    };
    if let Err(error) = storage.update_completed_reset_credit_operation_result(
        operation_id,
        account_id,
        &serde_json::to_string(&result).map_err(|serialize_error| {
            format!("serialize reset credit result failed: {serialize_error}")
        })?,
        now_ts(),
    ) {
        log::warn!(
            "event=reset_credit_operation_result_update_failed operation_id={} error={}",
            operation_id,
            error
        );
    }
    Ok(result)
}

#[cfg(test)]
#[path = "tests/reset_credits_async_tests.rs"]
mod async_tests;

#[cfg(test)]
mod tests {
    use super::{
        is_uuid_v4, random_uuid_v4, resolve_account_header, stored_operation_result,
        RESET_CREDIT_LOCK_POISONED_MESSAGE, RESET_CREDIT_PENDING_OPERATION_PREFIX,
        RESET_CREDIT_TERMINAL_FAILURE_PREFIX,
    };
    use codexmanager_core::storage::{
        now_ts, Account, ResetCreditOperation, ResetCreditOperationStatus, Storage, Token,
    };

    #[test]
    fn generated_redeem_request_id_is_uuid_v4() {
        let value = random_uuid_v4();
        assert!(is_uuid_v4(&value));
        assert_eq!(value.len(), 36);
        assert_eq!(&value[14..15], "4");
        assert!(matches!(&value[19..20], "8" | "9" | "a" | "b"));
        assert_eq!(
            value.chars().filter(|character| *character == '-').count(),
            4
        );
    }

    #[test]
    fn operation_id_validation_requires_uuid_v4() {
        assert!(is_uuid_v4("01234567-89ab-4def-8abc-0123456789ab"));
        assert!(!is_uuid_v4("01234567-89ab-1def-8abc-0123456789ab"));
        assert!(!is_uuid_v4("not-a-uuid"));
    }

    #[test]
    fn poisoned_lock_message_is_actionable() {
        assert!(RESET_CREDIT_LOCK_POISONED_MESSAGE.contains("restart CodexManager"));
        assert!(RESET_CREDIT_LOCK_POISONED_MESSAGE.contains("verify"));
        assert!(RESET_CREDIT_LOCK_POISONED_MESSAGE.contains("then retry"));
    }

    #[test]
    fn stored_pending_operation_exposes_recoverable_operation_id() {
        let operation = ResetCreditOperation {
            operation_id: "01234567-89ab-4def-8abc-0123456789ab".to_string(),
            account_id: "account-1".to_string(),
            redeem_request_id: "redeem-1".to_string(),
            status: ResetCreditOperationStatus::Pending,
            result_json: None,
            error: None,
            created_at: 1,
            updated_at: 1,
        };

        let error = stored_operation_result(&operation).expect_err("pending operation");
        assert!(error.contains(&format!(
            "{RESET_CREDIT_PENDING_OPERATION_PREFIX}{}",
            operation.operation_id
        )));
    }

    #[test]
    fn stored_failed_operation_is_marked_as_terminal() {
        let operation = ResetCreditOperation {
            operation_id: "01234567-89ab-4def-8abc-0123456789ab".to_string(),
            account_id: "account-1".to_string(),
            redeem_request_id: "redeem-1".to_string(),
            status: ResetCreditOperationStatus::Failed,
            result_json: None,
            error: Some("provider rejected".to_string()),
            created_at: 1,
            updated_at: 2,
        };

        assert_eq!(
            stored_operation_result(&operation).expect_err("failed operation"),
            format!("{RESET_CREDIT_TERMINAL_FAILURE_PREFIX}provider rejected")
        );
    }

    #[test]
    fn reset_credit_header_prefers_stored_chatgpt_account_id() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        storage
            .insert_account(&Account {
                id: "acc-reset-header".to_string(),
                label: "reset-header".to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: Some("chatgpt-account".to_string()),
                workspace_id: Some("workspace".to_string()),
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");

        let header = resolve_account_header(
            &storage,
            &Token {
                account_id: "acc-reset-header".to_string(),
                id_token: String::new(),
                access_token: String::new(),
                refresh_token: String::new(),
                api_key_access_token: None,
                last_refresh: now,
            },
        );

        assert_eq!(header.as_deref(), Some("chatgpt-account"));
    }
}
