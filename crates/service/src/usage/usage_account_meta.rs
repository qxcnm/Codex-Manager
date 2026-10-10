use codexmanager_core::auth::{
    extract_chatgpt_account_id, extract_workspace_id, normalize_chatgpt_account_id,
    normalize_workspace_id, parse_id_token_claims,
};
use codexmanager_core::storage::{now_ts, Account, AccountWorkspaceIdentity, Storage, Token};
use std::collections::HashMap;

/// 函数 `clean_header_value`
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
pub(crate) fn clean_header_value(value: Option<String>) -> Option<String> {
    match value {
        Some(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => None,
    }
}

/// 函数 `resolve_workspace_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - workspace_id: 参数 workspace_id
/// - chatgpt_account_id: 参数 chatgpt_account_id
///
/// # 返回
/// 返回函数执行结果
fn resolve_workspace_header(
    workspace_id: Option<String>,
    chatgpt_account_id: Option<String>,
) -> Option<String> {
    normalize_workspace_id(workspace_id.as_deref())
        .or_else(|| normalize_chatgpt_account_id(chatgpt_account_id.as_deref()))
}

/// 函数 `workspace_header_for_account`
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
pub(crate) fn workspace_header_for_account(account: &Account) -> Option<String> {
    resolve_workspace_header(
        account.workspace_id.clone(),
        account.chatgpt_account_id.clone(),
    )
}

/// 函数 `build_workspace_map_from_accounts`
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
#[cfg(test)]
pub(crate) fn build_workspace_map_from_accounts(
    accounts: &[Account],
) -> HashMap<String, Option<String>> {
    let mut workspace_map = HashMap::with_capacity(accounts.len());
    for account in accounts {
        let workspace_id = workspace_header_for_account(account);
        workspace_map.insert(account.id.clone(), workspace_id);
    }
    workspace_map
}

/// 函数 `resolve_workspace_id_for_account`
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
pub(crate) fn resolve_workspace_id_for_account(
    storage: &Storage,
    account_id: &str,
) -> Option<String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .find_account_workspace_identity_by_id(account_id)
        .ok()
        .flatten()
        .and_then(|identity| {
            resolve_workspace_header(identity.workspace_id, identity.chatgpt_account_id)
        })
}

/// 函数 `derive_account_meta`
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
pub(crate) fn derive_account_meta(token: &Token) -> (Option<String>, Option<String>) {
    let mut chatgpt_account_id = None;
    let mut workspace_id = None;

    if let Ok(claims) = parse_id_token_claims(&token.id_token) {
        if let Some(auth) = claims.auth {
            if chatgpt_account_id.is_none() {
                chatgpt_account_id =
                    normalize_chatgpt_account_id(auth.chatgpt_account_id.as_deref());
            }
        }
        if workspace_id.is_none() {
            workspace_id = normalize_workspace_id(claims.workspace_id.as_deref());
        }
    }

    if workspace_id.is_none() {
        workspace_id = clean_header_value(
            extract_workspace_id(&token.id_token)
                .or_else(|| extract_workspace_id(&token.access_token)),
        );
    }
    if chatgpt_account_id.is_none() {
        chatgpt_account_id = clean_header_value(
            extract_chatgpt_account_id(&token.id_token)
                .or_else(|| extract_chatgpt_account_id(&token.access_token)),
        );
    }
    if workspace_id.is_none() {
        workspace_id = chatgpt_account_id.clone();
    }

    (chatgpt_account_id, workspace_id)
}

/// 函数 `patch_account_meta`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn patch_account_meta(
    storage: &Storage,
    account_id: &str,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let Ok(account) = storage.find_account_workspace_identity_by_id(account_id) else {
        return;
    };
    let Some(mut account) = account else {
        return;
    };

    if apply_account_identity_patch(&mut account, chatgpt_account_id, workspace_id) {
        let _ = storage.update_account_workspace_identity(
            &account.id,
            account.chatgpt_account_id.as_deref(),
            account.workspace_id.as_deref(),
            now_ts(),
        );
    }
}

/// 函数 `patch_account_meta_cached`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn patch_account_meta_cached(
    storage: &Storage,
    accounts: &mut HashMap<String, Account>,
    account_id: &str,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    if let Some(account) = accounts.get_mut(account_id) {
        if apply_account_meta_patch(account, chatgpt_account_id, workspace_id) {
            account.updated_at = now_ts();
            let _ = storage.update_account_workspace_identity(
                &account.id,
                account.chatgpt_account_id.as_deref(),
                account.workspace_id.as_deref(),
                account.updated_at,
            );
        }
        return;
    }

    patch_account_meta(storage, account_id, chatgpt_account_id, workspace_id);
}

/// 函数 `patch_account_meta_in_place`
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
pub(crate) fn patch_account_meta_in_place(
    account: &mut Account,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) -> bool {
    apply_account_meta_patch(account, chatgpt_account_id, workspace_id)
}

/// 函数 `is_invalid_upstream_scope_value`
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
fn is_invalid_upstream_scope_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    // `auth0|...` / `google-oauth2|...` 等 subject 不能作为 ChatGPT workspace/account header。
    trimmed.contains('|') || trimmed.starts_with("import-sub-")
}

/// 函数 `apply_account_meta_patch`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account: 参数 account
/// - chatgpt_account_id: 参数 chatgpt_account_id
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
fn apply_account_meta_patch(
    account: &mut Account,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) -> bool {
    let mut identity = AccountWorkspaceIdentity {
        id: account.id.clone(),
        chatgpt_account_id: account.chatgpt_account_id.clone(),
        workspace_id: account.workspace_id.clone(),
    };
    let changed = apply_account_identity_patch(&mut identity, chatgpt_account_id, workspace_id);
    if changed {
        account.chatgpt_account_id = identity.chatgpt_account_id;
        account.workspace_id = identity.workspace_id;
    }
    changed
}

fn apply_account_identity_patch(
    account: &mut AccountWorkspaceIdentity,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
) -> bool {
    let mut changed = false;
    let next_chatgpt_account_id = normalize_chatgpt_account_id(chatgpt_account_id.as_deref());
    let next_workspace_id = normalize_workspace_id(workspace_id.as_deref());

    if let Some(next) = next_chatgpt_account_id.clone() {
        if is_invalid_upstream_scope_value(&next) {
            log::debug!(
                "event=account_meta_patch_skip_invalid_scope field=chatgpt_account_id account_id={} value={}",
                account.id,
                next
            );
        } else {
            let current = account.chatgpt_account_id.as_deref().unwrap_or("").trim();
            if current != next {
                account.chatgpt_account_id = Some(next);
                changed = true;
            }
        }
    }

    let desired_workspace = next_workspace_id.or_else(|| next_chatgpt_account_id.clone());
    if let Some(next) = desired_workspace {
        if is_invalid_upstream_scope_value(&next) {
            log::debug!(
                "event=account_meta_patch_skip_invalid_scope field=workspace_id account_id={} value={}",
                account.id,
                next
            );
        } else {
            let current = account.workspace_id.as_deref().unwrap_or("").trim();
            if current != next {
                account.workspace_id = Some(next);
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
#[path = "tests/usage_account_meta_tests.rs"]
mod tests;
