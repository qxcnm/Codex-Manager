use codexmanager_core::storage::{Account, AccountWorkspaceIdentity};

use crate::storage_helpers::account_key;

/// Codex workspace switching retains the previous ID token until refresh. Only
/// use the access-token workspace when the selected account and both user claims
/// agree; an arbitrary import hint must not select another user's workspace.
pub(crate) fn selected_workspace_from_access_token(
    id_token: &codexmanager_core::auth::IdTokenClaims,
    access_token: &codexmanager_core::auth::IdTokenClaims,
    selected_account_id: Option<&str>,
) -> Option<String> {
    let selected = selected_account_id.filter(|value| !value.is_empty())?;
    let previous = id_token.auth.as_ref()?;
    let current = access_token.auth.as_ref()?;
    let user_id = previous
        .chatgpt_user_id
        .as_deref()
        .filter(|value| !value.is_empty())?;
    if current.chatgpt_user_id.as_deref() != Some(user_id)
        || current.chatgpt_account_id.as_deref() != Some(selected)
        || previous.chatgpt_account_id.as_deref() == Some(selected)
    {
        return None;
    }
    Some(selected.to_string())
}

/// 函数 `clean_value`
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
pub(crate) fn clean_value(value: Option<String>) -> Option<String> {
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

/// 函数 `normalize_non_empty`
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
fn normalize_non_empty<'a>(value: Option<&'a str>) -> Option<&'a str> {
    value.map(str::trim).filter(|v| !v.is_empty())
}

/// 函数 `normalize_id_part`
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
fn normalize_id_part(value: Option<&str>) -> Option<String> {
    let raw = normalize_non_empty(value)?;
    Some(raw.replace("::", "_"))
}

/// 函数 `same_normalized`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - lhs: 参数 lhs
/// - rhs: 参数 rhs
///
/// # 返回
/// 返回函数执行结果
fn same_normalized(lhs: Option<&str>, rhs: Option<&str>) -> bool {
    normalize_non_empty(lhs) == normalize_non_empty(rhs)
}

pub(crate) trait AccountIdentityView {
    fn account_id(&self) -> &str;
    fn chatgpt_account_id(&self) -> Option<&str>;
    fn workspace_id(&self) -> Option<&str>;
}

impl AccountIdentityView for Account {
    fn account_id(&self) -> &str {
        self.id.as_str()
    }

    fn chatgpt_account_id(&self) -> Option<&str> {
        self.chatgpt_account_id.as_deref()
    }

    fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }
}

impl AccountIdentityView for AccountWorkspaceIdentity {
    fn account_id(&self) -> &str {
        self.id.as_str()
    }

    fn chatgpt_account_id(&self) -> Option<&str> {
        self.chatgpt_account_id.as_deref()
    }

    fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }
}

impl<T> AccountIdentityView for &T
where
    T: AccountIdentityView + ?Sized,
{
    fn account_id(&self) -> &str {
        (*self).account_id()
    }

    fn chatgpt_account_id(&self) -> Option<&str> {
        (*self).chatgpt_account_id()
    }

    fn workspace_id(&self) -> Option<&str> {
        (*self).workspace_id()
    }
}

/// 函数 `build_scope_identity_hint`
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
pub(crate) fn build_scope_identity_hint(
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
) -> Option<String> {
    let chatgpt = normalize_id_part(chatgpt_account_id);
    let workspace = normalize_id_part(workspace_id);
    match (chatgpt, workspace) {
        (Some(chatgpt), Some(workspace)) if chatgpt != workspace => {
            Some(format!("cgpt={chatgpt}|ws={workspace}"))
        }
        (Some(chatgpt), _) => Some(format!("cgpt={chatgpt}")),
        (None, Some(workspace)) => Some(format!("ws={workspace}")),
        (None, None) => None,
    }
}

/// 函数 `build_account_storage_id`
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
pub(crate) fn build_account_storage_id(
    subject_account_id: &str,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
    tags: Option<&str>,
) -> String {
    let base = subject_account_id.trim();
    let mut suffix_parts: Vec<String> = Vec::new();
    if let Some(hint) = build_scope_identity_hint(chatgpt_account_id, workspace_id) {
        suffix_parts.push(hint);
    }
    if let Some(tag) = normalize_id_part(tags) {
        suffix_parts.push(tag);
    }
    if suffix_parts.is_empty() {
        return base.to_string();
    }
    format!("{base}::{}", suffix_parts.join("|"))
}

/// 函数 `build_fallback_subject_key`
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
pub(crate) fn build_fallback_subject_key(
    subject_account_id: Option<&str>,
    tags: Option<&str>,
) -> Option<String> {
    normalize_non_empty(subject_account_id).map(|subject| account_key(subject, tags))
}

/// 函数 `pick_existing_account_id_by_identity`
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
pub(crate) fn pick_existing_account_id_by_identity<'a, I>(
    accounts: I,
    chatgpt_account_id: Option<&str>,
    workspace_id: Option<&str>,
    fallback_subject_key: Option<&str>,
    account_id_hint: Option<&str>,
) -> Option<String>
where
    I: IntoIterator,
    I::Item: AccountIdentityView,
{
    let accounts = accounts.into_iter().collect::<Vec<_>>();
    let preferred_chatgpt = normalize_non_empty(chatgpt_account_id).map(str::to_string);
    let preferred_workspace = normalize_non_empty(workspace_id).map(str::to_string);

    if let (Some(chatgpt_id), Some(workspace_id)) =
        (preferred_chatgpt.as_ref(), preferred_workspace.as_ref())
    {
        if let Some(found) = accounts.iter().find(|acc| {
            same_normalized(acc.chatgpt_account_id(), Some(chatgpt_id.as_str()))
                && same_normalized(acc.workspace_id(), Some(workspace_id.as_str()))
        }) {
            return Some(found.account_id().to_string());
        }
        return None;
    }

    if let Some(chatgpt_id) = preferred_chatgpt.as_ref() {
        let mut matched = accounts
            .iter()
            .filter(|acc| same_normalized(acc.chatgpt_account_id(), Some(chatgpt_id.as_str())));
        if let Some(found) = matched.next() {
            if matched.next().is_none() {
                return Some(found.account_id().to_string());
            }
        }
        if let Some(found) = accounts.iter().find(|acc| {
            same_normalized(acc.chatgpt_account_id(), Some(chatgpt_id.as_str()))
                && normalize_non_empty(acc.workspace_id()).is_none()
        }) {
            return Some(found.account_id().to_string());
        }
    }

    if let Some(workspace) = preferred_workspace.as_ref() {
        if let Some(found) = accounts
            .iter()
            .find(|acc| same_normalized(acc.workspace_id(), Some(workspace.as_str())))
        {
            return Some(found.account_id().to_string());
        }
    }

    if let Some(account_id_hint) = normalize_non_empty(account_id_hint) {
        if let Some(found) = accounts
            .iter()
            .find(|acc| acc.account_id() == account_id_hint)
        {
            return Some(found.account_id().to_string());
        }
    }

    if let Some(fallback_subject_key) = normalize_non_empty(fallback_subject_key) {
        if let Some(found) = accounts
            .iter()
            .find(|acc| acc.account_id() == fallback_subject_key)
        {
            return Some(found.account_id().to_string());
        }
    }

    None
}
