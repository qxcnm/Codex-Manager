use codexmanager_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, normalize_chatgpt_account_id,
    normalize_workspace_id, parse_id_token_claims, DEFAULT_CLIENT_ID, DEFAULT_ISSUER,
};
use codexmanager_core::rpc::types::LoginStartResult;
use codexmanager_core::storage::{
    now_ts, Account, AccountAuthRefreshTarget, AccountWorkspaceIdentity, Storage, Token,
};
use serde::Serialize;
use std::collections::HashMap;

use crate::account_identity::{build_account_storage_id, build_fallback_subject_key, clean_value};
use crate::account_plan::resolve_effective_account_plan;
use crate::account_status::mark_account_unavailable_for_auth_error;
use crate::app_settings::{get_persisted_app_setting, save_persisted_app_setting};
use crate::storage_helpers::open_storage;
use crate::usage_http::{
    fetch_account_subscription, fetch_account_subscription_with_explicit_proxy,
    log_account_data_route,
};
use crate::usage_token_refresh::{refresh_and_persist_access_token, token_refresh_ahead_secs};

const CURRENT_AUTH_ACCOUNT_ID_KEY: &str = "auth.current_account_id";
const CURRENT_AUTH_MODE_KEY: &str = "auth.current_auth_mode";
const AUTH_MODE_CHATGPT: &str = "chatgpt";
const AUTH_MODE_CHATGPT_AUTH_TOKENS: &str = "chatgptAuthTokens";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountReadResponse {
    pub(crate) account: Option<CurrentAuthAccount>,
    pub(crate) requires_openai_auth: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CurrentAuthAccount {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    pub(crate) account_id: String,
    pub(crate) email: String,
    pub(crate) plan_type: String,
    pub(crate) plan_type_raw: Option<String>,
    pub(crate) has_subscription: Option<bool>,
    pub(crate) subscription_plan: Option<String>,
    pub(crate) subscription_expires_at: Option<i64>,
    pub(crate) subscription_renews_at: Option<i64>,
    pub(crate) chatgpt_account_id: Option<String>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatgptAuthTokensRefreshResponse {
    pub(crate) access_token: String,
    pub(crate) chatgpt_account_id: String,
    pub(crate) chatgpt_plan_type: Option<String>,
    pub(crate) has_subscription: Option<bool>,
    pub(crate) subscription_plan: Option<String>,
    pub(crate) subscription_expires_at: Option<i64>,
    pub(crate) subscription_renews_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatgptAuthTokensRefreshAllItem {
    pub(crate) account_id: String,
    pub(crate) account_name: String,
    pub(crate) ok: bool,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatgptAuthTokensRefreshAllResponse {
    pub(crate) requested: usize,
    pub(crate) succeeded: usize,
    pub(crate) failed: usize,
    pub(crate) skipped: usize,
    pub(crate) results: Vec<ChatgptAuthTokensRefreshAllItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct ChatgptAuthTokensLoginInput {
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) id_token: Option<String>,
    pub(crate) chatgpt_account_id: Option<String>,
    pub(crate) workspace_id: Option<String>,
    pub(crate) chatgpt_plan_type: Option<String>,
}

/// 函数 `login_with_chatgpt_auth_tokens`
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
pub(crate) fn login_with_chatgpt_auth_tokens(
    input: ChatgptAuthTokensLoginInput,
) -> Result<LoginStartResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let access_token = input.access_token.trim();
    if access_token.is_empty() {
        return Err("accessToken is required".to_string());
    }
    let _requested_plan_type = input
        .chatgpt_plan_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let claims = parse_id_token_claims(access_token)
        .map_err(|err| format!("invalid access token jwt: {err}"))?;
    let subject_account_id = claims.sub.trim();
    if subject_account_id.is_empty() {
        return Err("access token missing subject".to_string());
    }
    let claim_chatgpt_account_id = claims
        .auth
        .as_ref()
        .and_then(|auth| normalize_chatgpt_account_id(auth.chatgpt_account_id.as_deref()));
    let claim_workspace_id = normalize_workspace_id(claims.workspace_id.as_deref());

    let chatgpt_account_id = clean_value(
        input
            .chatgpt_account_id
            .as_deref()
            .and_then(|value| normalize_chatgpt_account_id(Some(value)))
            .or_else(|| extract_chatgpt_account_id(access_token))
            .or(claim_chatgpt_account_id),
    );
    let workspace_id = clean_value(
        input
            .workspace_id
            .as_deref()
            .and_then(|value| normalize_workspace_id(Some(value)))
            .or_else(|| extract_workspace_id(access_token))
            .or(claim_workspace_id)
            .or_else(|| chatgpt_account_id.clone()),
    );
    let resolved_scope_id = workspace_id
        .clone()
        .or_else(|| chatgpt_account_id.clone())
        .ok_or_else(|| "chatgptAccountId/workspaceId is required".to_string())?;

    let fallback_subject_key = build_fallback_subject_key(Some(subject_account_id), None);
    let account_storage_id = build_account_storage_id(
        subject_account_id,
        chatgpt_account_id.as_deref(),
        workspace_id.as_deref(),
        None,
    );
    let account_id = storage
        .find_account_id_by_identity(
            fallback_subject_key.as_deref(),
            chatgpt_account_id.as_deref(),
            workspace_id.as_deref(),
        )
        .map_err(|err| err.to_string())?
        .unwrap_or(account_storage_id);

    let existing_state = storage
        .find_account_upsert_state_by_id(&account_id)
        .map_err(|err| err.to_string())?;
    let now = now_ts();
    let account = Account {
        id: account_id.clone(),
        label: claims
            .email
            .clone()
            .unwrap_or_else(|| resolved_scope_id.clone()),
        issuer: std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string()),
        chatgpt_account_id: chatgpt_account_id.clone(),
        workspace_id: workspace_id.clone(),
        group_name: existing_state
            .as_ref()
            .and_then(|state| state.group_name.clone()),
        sort: existing_state
            .as_ref()
            .map(|state| state.sort)
            .unwrap_or_else(|| super::tokens::next_account_sort(&storage)),
        status: "active".to_string(),
        created_at: existing_state
            .as_ref()
            .map(|state| state.created_at)
            .unwrap_or(now),
        updated_at: now,
    };
    storage
        .insert_account(&account)
        .map_err(|err| err.to_string())?;

    let mut token = Token {
        account_id: account_id.clone(),
        id_token: input.id_token.unwrap_or_default(),
        access_token: access_token.to_string(),
        refresh_token: input.refresh_token.unwrap_or_default(),
        api_key_access_token: None,
        last_refresh: now,
    };
    if token.id_token.trim().is_empty() {
        token.id_token = token.access_token.clone();
    }
    storage
        .insert_token(&token)
        .map_err(|err| err.to_string())?;

    set_current_auth_account_id(Some(&account_id))?;
    set_current_auth_mode(Some(AUTH_MODE_CHATGPT_AUTH_TOKENS))?;
    let _ = crate::usage_refresh::enqueue_usage_refresh_after_account_add(&account_id);

    Ok(LoginStartResult::ChatgptAuthTokens {})
}

/// 函数 `read_current_account`
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
pub(crate) fn read_current_account(refresh_token: bool) -> Result<AccountReadResponse, String> {
    let Some(storage) = open_storage() else {
        return Ok(AccountReadResponse {
            account: None,
            requires_openai_auth: true,
        });
    };
    let Some((account, token)) = resolve_current_account_with_token(&storage)? else {
        return Ok(AccountReadResponse {
            account: None,
            requires_openai_auth: true,
        });
    };

    let mut token = token;
    if refresh_token && !token.refresh_token.trim().is_empty() {
        let issuer =
            std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
        let client_id = std::env::var("CODEXMANAGER_CLIENT_ID")
            .unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
        if let Err(err) = refresh_and_persist_access_token(
            &storage,
            &mut token,
            &issuer,
            &client_id,
            token_refresh_ahead_secs(),
        ) {
            let _ = mark_account_unavailable_for_auth_error(&storage, &account.id, &err);
            return Err(err);
        }
    }

    let auth_mode = resolve_current_auth_mode(&token);
    Ok(AccountReadResponse {
        account: Some(current_account_payload(
            &storage,
            &account,
            &token,
            auth_mode.as_str(),
        )),
        requires_openai_auth: true,
    })
}

/// 函数 `refresh_current_chatgpt_auth_tokens`
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
pub(crate) fn refresh_current_chatgpt_auth_tokens(
    target_account_id: Option<&str>,
) -> Result<ChatgptAuthTokensRefreshResponse, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let (account, mut token) = resolve_refresh_target(&storage, target_account_id)?
        .ok_or_else(|| "no current chatgptAuthTokens account".to_string())?;
    if token.refresh_token.trim().is_empty() {
        return Err("target account does not have refresh_token".to_string());
    }

    let issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    if let Err(err) = refresh_and_persist_access_token(
        &storage,
        &mut token,
        &issuer,
        &client_id,
        token_refresh_ahead_secs(),
    ) {
        let _ = mark_account_unavailable_for_auth_error(&storage, &account.id, &err);
        return Err(err);
    }

    let refreshed_account = storage
        .find_account_workspace_identity_by_id(&account.id)
        .map_err(|err| err.to_string())?
        .unwrap_or_else(|| AccountWorkspaceIdentity {
            id: account.id,
            chatgpt_account_id: account.chatgpt_account_id,
            workspace_id: account.workspace_id,
        });
    let stored_chatgpt_account_id =
        normalize_chatgpt_account_id(refreshed_account.chatgpt_account_id.as_deref());
    let stored_workspace_id = normalize_workspace_id(refreshed_account.workspace_id.as_deref());
    let chatgpt_account_id = stored_chatgpt_account_id
        .clone()
        .or_else(|| extract_chatgpt_account_id(&token.access_token))
        .or_else(|| stored_workspace_id.clone())
        .or_else(|| extract_workspace_id(&token.access_token))
        .ok_or_else(|| "refreshed token missing chatgptAccountId".to_string())?;
    let workspace_id = stored_workspace_id
        .clone()
        .or_else(|| extract_workspace_id(&token.access_token))
        .or_else(|| stored_chatgpt_account_id.clone())
        .or_else(|| extract_chatgpt_account_id(&token.access_token));
    let access_claims = parse_id_token_claims(&token.access_token).ok();
    let plan_type_resolution = resolve_plan_type_resolution(&token, access_claims.as_ref());
    let base_url = std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string());
    let proxy_mode =
        crate::account_proxy::resolve_account_proxy_mode(refreshed_account.id.as_str());
    log_account_data_route(
        "subscription",
        refreshed_account.id.as_str(),
        &proxy_mode,
        "accounts_check",
        false,
    );
    let subscription = match &proxy_mode {
        crate::account_proxy::AccountProxyMode::Disabled => fetch_account_subscription(
            &base_url,
            &token.access_token,
            &chatgpt_account_id,
            workspace_id.as_deref(),
        )?,
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            fetch_account_subscription_with_explicit_proxy(
                &base_url,
                &token.access_token,
                &chatgpt_account_id,
                workspace_id.as_deref(),
                proxy_url,
            )?
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
            return Err(error.clone());
        }
    };
    storage
        .upsert_account_subscription(
            &refreshed_account.id,
            subscription.has_subscription,
            subscription.account_plan_type.as_deref(),
            subscription.plan_type.as_deref(),
            subscription.expires_at,
            subscription.renews_at,
        )
        .map_err(|err| format!("store account subscription failed: {err}"))?;
    let chatgpt_plan_type = subscription.account_plan_type.clone().or_else(|| {
        plan_type_resolution
            .as_ref()
            .map(|plan| plan.normalized.clone())
    });

    Ok(ChatgptAuthTokensRefreshResponse {
        access_token: token.access_token,
        chatgpt_account_id,
        chatgpt_plan_type,
        has_subscription: Some(subscription.has_subscription),
        subscription_plan: subscription.plan_type,
        subscription_expires_at: subscription.expires_at,
        subscription_renews_at: subscription.renews_at,
    })
}

/// 函数 `refresh_all_chatgpt_auth_tokens`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-03
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn refresh_all_chatgpt_auth_tokens(
) -> Result<ChatgptAuthTokensRefreshAllResponse, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let accounts = storage
        .list_account_auth_refresh_targets()
        .map_err(|err| err.to_string())?;
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let mut tokens_by_account = storage
        .list_tokens_for_accounts(&account_ids)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|token| (token.account_id.clone(), token))
        .collect::<HashMap<_, _>>();
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let default_issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());

    let mut results = Vec::with_capacity(accounts.len());
    let mut requested = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for account in accounts {
        let account_name = account.label.clone();
        let Some(mut token) = tokens_by_account.remove(&account.id) else {
            skipped = skipped.saturating_add(1);
            results.push(ChatgptAuthTokensRefreshAllItem {
                account_id: account.id,
                account_name,
                ok: false,
                message: Some("missing token".to_string()),
            });
            continue;
        };
        if token.refresh_token.trim().is_empty() {
            skipped = skipped.saturating_add(1);
            results.push(ChatgptAuthTokensRefreshAllItem {
                account_id: account.id,
                account_name,
                ok: false,
                message: Some("missing refresh_token".to_string()),
            });
            continue;
        }

        requested = requested.saturating_add(1);
        let issuer = refresh_target_issuer(&account, &default_issuer);
        match refresh_and_persist_access_token(
            &storage,
            &mut token,
            issuer,
            &client_id,
            token_refresh_ahead_secs(),
        ) {
            Ok(()) => {
                succeeded = succeeded.saturating_add(1);
                results.push(ChatgptAuthTokensRefreshAllItem {
                    account_id: account.id,
                    account_name,
                    ok: true,
                    message: None,
                });
            }
            Err(err) => {
                failed = failed.saturating_add(1);
                let _ = mark_account_unavailable_for_auth_error(&storage, &account.id, &err);
                results.push(ChatgptAuthTokensRefreshAllItem {
                    account_id: account.id,
                    account_name,
                    ok: false,
                    message: Some(err),
                });
            }
        }
    }

    Ok(ChatgptAuthTokensRefreshAllResponse {
        requested,
        succeeded,
        failed,
        skipped,
        results,
    })
}

fn refresh_target_issuer<'a>(
    account: &'a AccountAuthRefreshTarget,
    default_issuer: &'a str,
) -> &'a str {
    if account.issuer.trim().is_empty() {
        default_issuer
    } else {
        account.issuer.as_str()
    }
}

/// 函数 `logout_current_account`
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
pub(crate) fn logout_current_account() -> Result<serde_json::Value, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let current_account_id = get_persisted_app_setting(CURRENT_AUTH_ACCOUNT_ID_KEY);
    if let Some(account_id) = current_account_id.as_deref() {
        if storage
            .account_exists(account_id)
            .map_err(|err| err.to_string())?
        {
            storage
                .update_account_status(account_id, "inactive")
                .map_err(|err| format!("update account status failed: {err}"))?;
        }
    }
    set_current_auth_account_id(None)?;
    set_current_auth_mode(None)?;
    Ok(serde_json::json!({}))
}

/// 函数 `resolve_current_account_with_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
///
/// # 返回
/// 返回函数执行结果
fn resolve_current_account_with_token(
    storage: &Storage,
) -> Result<Option<(Account, Token)>, String> {
    let Some(account_id) = get_persisted_app_setting(CURRENT_AUTH_ACCOUNT_ID_KEY) else {
        return Ok(None);
    };
    if let Some(pair) = storage
        .find_account_with_token_by_id(&account_id)
        .map_err(|err| err.to_string())?
    {
        return Ok(Some(pair));
    }
    set_current_auth_account_id(None)?;
    set_current_auth_mode(None)?;
    Ok(None)
}

/// 函数 `resolve_refresh_target`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - previous_account_id: 参数 previous_account_id
///
/// # 返回
/// 返回函数执行结果
fn resolve_refresh_target(
    storage: &Storage,
    target_account_id: Option<&str>,
) -> Result<Option<(Account, Token)>, String> {
    let Some(target_account_id) = target_account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return resolve_current_account_with_token(storage);
    };

    let chatgpt_account_id = normalize_chatgpt_account_id(Some(target_account_id));
    let workspace_id = normalize_workspace_id(Some(target_account_id));
    storage
        .find_account_with_token_by_identity(
            Some(target_account_id),
            chatgpt_account_id.as_deref(),
            workspace_id.as_deref(),
        )
        .map_err(|err| err.to_string())
}

/// 函数 `current_account_payload`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account: 参数 account
/// - token: 参数 token
/// - auth_mode: 参数 auth_mode
///
/// # 返回
/// 返回函数执行结果
fn current_account_payload(
    storage: &Storage,
    account: &Account,
    token: &Token,
    auth_mode: &str,
) -> CurrentAuthAccount {
    let claims = parse_id_token_claims(&token.access_token).ok();
    let subscription = storage
        .find_account_subscription(&account.id)
        .ok()
        .flatten();
    let token_plan = crate::account_plan::token_plan_from_token(token);
    let resolved_plan =
        resolve_effective_account_plan(Some(&token_plan), None, subscription.as_ref());
    let plan_type = resolved_plan
        .as_ref()
        .map(|plan| plan.normalized.clone())
        .unwrap_or_else(|| "unknown".to_string());
    CurrentAuthAccount {
        kind: auth_mode.to_string(),
        account_id: account.id.clone(),
        email: claims
            .as_ref()
            .and_then(|claims| claims.email.clone())
            .unwrap_or_else(|| account.label.clone()),
        plan_type,
        plan_type_raw: resolved_plan.and_then(|plan| plan.raw),
        has_subscription: subscription.as_ref().map(|value| value.has_subscription),
        subscription_plan: subscription
            .as_ref()
            .and_then(|value| value.plan_type.clone()),
        subscription_expires_at: subscription.as_ref().and_then(|value| value.expires_at),
        subscription_renews_at: subscription.as_ref().and_then(|value| value.renews_at),
        chatgpt_account_id: normalize_chatgpt_account_id(account.chatgpt_account_id.as_deref()),
        workspace_id: normalize_workspace_id(account.workspace_id.as_deref()),
        status: account.status.clone(),
    }
}

/// 函数 `resolve_plan_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
/// - parsed_claims: 参数 parsed_claims
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn resolve_plan_type(
    token: &Token,
    parsed_claims: Option<&codexmanager_core::auth::IdTokenClaims>,
) -> Option<String> {
    resolve_plan_type_resolution(token, parsed_claims).map(|plan| plan.normalized)
}

/// 函数 `resolve_plan_type_raw`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
/// - parsed_claims: 参数 parsed_claims
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn resolve_plan_type_raw(
    token: &Token,
    parsed_claims: Option<&codexmanager_core::auth::IdTokenClaims>,
) -> Option<String> {
    resolve_plan_type_resolution(token, parsed_claims).and_then(|plan| plan.raw)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedPlanType {
    normalized: String,
    raw: Option<String>,
}

/// 函数 `resolve_plan_type_resolution`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
/// - parsed_claims: 参数 parsed_claims
///
/// # 返回
/// 返回函数执行结果
fn resolve_plan_type_resolution(
    token: &Token,
    parsed_claims: Option<&codexmanager_core::auth::IdTokenClaims>,
) -> Option<ResolvedPlanType> {
    if let Some(claims) = parsed_claims {
        if let Some(plan_type) = claims
            .auth
            .as_ref()
            .and_then(|auth| auth.chatgpt_plan_type.clone())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return normalize_plan_type(plan_type);
        }
    }
    if let Some(plan_type) = parse_id_token_claims(&token.access_token)
        .ok()
        .and_then(|claims| claims.auth.and_then(|auth| auth.chatgpt_plan_type))
        .and_then(normalize_plan_type)
    {
        return Some(plan_type);
    }
    parse_id_token_claims(&token.id_token)
        .ok()
        .and_then(|claims| claims.auth.and_then(|auth| auth.chatgpt_plan_type))
        .and_then(normalize_plan_type)
}

/// 函数 `normalize_plan_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_plan_type(value: String) -> Option<ResolvedPlanType> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "free" | "go" | "plus" | "pro" | "team" | "business" | "enterprise" | "edu"
        | "education" => Some(ResolvedPlanType {
            normalized: match normalized.as_str() {
                "education" => "edu".to_string(),
                _ => normalized,
            },
            raw: None,
        }),
        "" => None,
        _ => Some(ResolvedPlanType {
            normalized: "unknown".to_string(),
            raw: Some(value.trim().to_string()),
        }),
    }
}

/// 函数 `set_current_auth_account_id`
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
pub(crate) fn set_current_auth_account_id(account_id: Option<&str>) -> Result<(), String> {
    save_persisted_app_setting(CURRENT_AUTH_ACCOUNT_ID_KEY, account_id)
}

/// 函数 `set_current_auth_mode`
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
pub(crate) fn set_current_auth_mode(auth_mode: Option<&str>) -> Result<(), String> {
    save_persisted_app_setting(CURRENT_AUTH_MODE_KEY, auth_mode)
}

/// 函数 `resolve_current_auth_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
fn resolve_current_auth_mode(token: &Token) -> String {
    get_persisted_app_setting(CURRENT_AUTH_MODE_KEY)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| infer_auth_mode_from_token(token).to_string())
}

/// 函数 `infer_auth_mode_from_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
fn infer_auth_mode_from_token(token: &Token) -> &'static str {
    if token.id_token.trim() == token.access_token.trim() {
        AUTH_MODE_CHATGPT_AUTH_TOKENS
    } else {
        AUTH_MODE_CHATGPT
    }
}

#[cfg(test)]
#[path = "tests/auth_account_tests.rs"]
mod tests;
