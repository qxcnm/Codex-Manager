use codexmanager_core::auth::{DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::storage::{Storage, Token};
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use serde_json::Value;

use crate::storage_helpers::{open_storage, StorageHandle};
use crate::usage_account_meta::{
    clean_header_value, derive_account_meta, patch_account_meta, resolve_workspace_id_for_account,
};
use crate::usage_http::{consume_rate_limit_reset_credit, fetch_rate_limit_reset_credits};
use crate::usage_refresh::refresh_usage_for_account;
use crate::usage_token_refresh::{refresh_and_persist_access_token, token_refresh_ahead_secs};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitResetCreditsResult {
    pub available_count: i64,
    pub total_earned_count: i64,
    pub credits: Vec<Value>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitResetConsumeResult {
    pub consumed: bool,
    pub credit_id: String,
    pub redeem_request_id: String,
    pub response: Value,
}

struct ResetAccountContext {
    storage: StorageHandle,
    token: Token,
    base_url: String,
    workspace_id: Option<String>,
}

pub(crate) fn read_rate_limit_reset_credits(
    account_id: &str,
) -> Result<RateLimitResetCreditsResult, String> {
    let context = load_reset_account_context(account_id)?;
    fetch_rate_limit_reset_credits_for_context(&context)
}

pub(crate) fn consume_rate_limit_reset_credits(
    account_id: &str,
) -> Result<RateLimitResetConsumeResult, String> {
    let context = load_reset_account_context(account_id)?;
    let credits = fetch_rate_limit_reset_credits_for_context(&context)?;
    if credits.available_count <= 0 {
        return Err("no available rate limit reset credits".to_string());
    }
    let credit_id = select_credit_id(&credits.credits)
        .ok_or_else(|| "available rate limit reset credit id not found".to_string())?;
    let redeem_request_id = generate_redeem_request_id();
    let token = context
        .storage
        .find_token_by_account_id(account_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| context.token.clone());
    let response = consume_rate_limit_reset_credit(
        &context.base_url,
        &token.access_token,
        context.workspace_id.as_deref(),
        &credit_id,
        &redeem_request_id,
    )?;
    refresh_usage_for_account(account_id)?;
    Ok(RateLimitResetConsumeResult {
        consumed: true,
        credit_id,
        redeem_request_id,
        response,
    })
}

fn load_reset_account_context(account_id: &str) -> Result<ResetAccountContext, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut token = storage
        .find_token_by_account_id(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "account token unavailable".to_string())?;
    let workspace_id = resolve_reset_workspace_id(&storage, &mut token)?;
    let base_url = std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string());
    Ok(ResetAccountContext {
        storage,
        token,
        base_url,
        workspace_id,
    })
}

fn resolve_reset_workspace_id(
    storage: &Storage,
    token: &mut Token,
) -> Result<Option<String>, String> {
    let mut resolved_workspace_id = resolve_workspace_id_for_account(storage, &token.account_id);
    let (derived_chatgpt_id, derived_workspace_id) = derive_account_meta(token);
    if resolved_workspace_id.is_none() {
        resolved_workspace_id = derived_workspace_id
            .clone()
            .or_else(|| derived_chatgpt_id.clone());
    }
    patch_account_meta(
        storage,
        &token.account_id,
        derived_chatgpt_id,
        derived_workspace_id,
    );
    Ok(clean_header_value(resolved_workspace_id))
}

fn fetch_rate_limit_reset_credits_for_context(
    context: &ResetAccountContext,
) -> Result<RateLimitResetCreditsResult, String> {
    match fetch_rate_limit_reset_credits(
        &context.base_url,
        &context.token.access_token,
        context.workspace_id.as_deref(),
    ) {
        Ok(value) => Ok(parse_rate_limit_reset_credits(value)),
        Err(err) if should_retry_reset_request_with_token(&err) => {
            let mut token = context.token.clone();
            let issuer =
                std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
            let client_id = std::env::var("CODEXMANAGER_CLIENT_ID")
                .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
            refresh_and_persist_access_token(
                &context.storage,
                &mut token,
                &issuer,
                &client_id,
                token_refresh_ahead_secs(),
            )?;
            let value = fetch_rate_limit_reset_credits(
                &context.base_url,
                &token.access_token,
                context.workspace_id.as_deref(),
            )?;
            Ok(parse_rate_limit_reset_credits(value))
        }
        Err(err) => Err(err),
    }
}

fn parse_rate_limit_reset_credits(value: Value) -> RateLimitResetCreditsResult {
    let available_count = value
        .get("available_count")
        .or_else(|| value.get("availableCount"))
        .and_then(Value::as_i64)
        .unwrap_or_else(|| {
            value
                .get("credits")
                .and_then(Value::as_array)
                .map(|items| items.len() as i64)
                .unwrap_or(0)
        });
    let total_earned_count = value
        .get("total_earned_count")
        .or_else(|| value.get("totalEarnedCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let credits = value
        .get("credits")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    RateLimitResetCreditsResult {
        available_count,
        total_earned_count,
        credits,
        raw: value,
    }
}

fn select_credit_id(credits: &[Value]) -> Option<String> {
    credits
        .iter()
        .filter_map(Value::as_object)
        .find(|credit| {
            !matches!(
                credit.get("status").and_then(Value::as_str),
                Some("redeemed") | Some("expired")
            )
        })
        .and_then(|credit| credit.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn should_retry_reset_request_with_token(err: &str) -> bool {
    err.contains("status=401")
        || err.contains("401 Unauthorized")
        || err.contains("token_expired")
        || err.contains("identity error code: token_expired")
}

fn generate_redeem_request_id() -> String {
    let suffix = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect::<String>();
    format!("codexmanager-{suffix}")
}
