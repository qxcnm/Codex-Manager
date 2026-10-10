//! Account self-service writes backed by the listener-owned domain store.
use codexmanager_core::storage::{
    now_ts, ApiKeyOwner, AppWallet, AppWalletLedgerEntry, DomainStorage,
};

use crate::{ApiKeyOwnerResult, AppUserPublicResult, RpcActor};

fn actor_user_id(actor: &RpcActor, operation: &str) -> Result<String, String> {
    actor
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("permission_denied: {operation} requires user session"))
}

fn normalized_display_name(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalized_role(value: Option<&str>, current: &str) -> Result<String, String> {
    let role = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(current)
        .to_ascii_lowercase();
    match role.as_str() {
        "admin" | "member" => Ok(role),
        _ => Err("角色必须是 admin 或 member".to_owned()),
    }
}

fn normalized_status(value: Option<&str>, current: &str) -> Result<String, String> {
    let status = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(current)
        .to_ascii_lowercase();
    match status.as_str() {
        "active" | "disabled" => Ok(status),
        _ => Err("状态必须是 active 或 disabled".to_owned()),
    }
}

fn has_only_active_admin(users: &[codexmanager_core::storage::AppUser]) -> bool {
    users
        .iter()
        .filter(|user| user.role == "admin" && user.status == "active")
        .count()
        <= 1
}

fn normalized_owner_kind(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Ok("user"),
        "project" => Ok("project"),
        _ => Err("归属类型必须是 user 或 project".to_owned()),
    }
}

async fn wallet_owner(
    storage: &dyn DomainStorage,
    owner_kind: &str,
    owner_id: &str,
) -> Result<(String, String), String> {
    let kind = normalized_owner_kind(owner_kind)?.to_owned();
    let id = owner_id.trim();
    if id.is_empty() {
        return Err("钱包归属 ID 不能为空".to_owned());
    }
    if kind == "user" {
        let user = storage
            .user(id.to_owned())
            .await?
            .ok_or_else(|| "用户不存在".to_owned())?;
        if user.role == "admin" {
            return Err("管理员账号不参与额度分发".to_owned());
        }
        if user.status != "active" {
            return Err("用户已禁用".to_owned());
        }
    }
    Ok((kind, id.to_owned()))
}

fn wallet_result(wallet: AppWallet) -> crate::AppWalletResult {
    crate::AppWalletResult {
        available_credit_micros: (wallet.balance_credit_micros - wallet.frozen_credit_micros)
            .max(0),
        id: wallet.id,
        owner_kind: wallet.owner_kind,
        owner_id: wallet.owner_id,
        balance_credit_micros: wallet.balance_credit_micros,
        frozen_credit_micros: wallet.frozen_credit_micros,
        status: wallet.status,
        created_at: wallet.created_at,
        updated_at: wallet.updated_at,
    }
}

fn normalized_note(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

async fn read_wallet_result(
    storage: &dyn DomainStorage,
    owner_kind: String,
    owner_id: String,
) -> Result<crate::AppWalletResult, String> {
    storage
        .wallet(owner_kind, owner_id)
        .await?
        .map(wallet_result)
        .ok_or_else(|| "钱包不存在".to_owned())
}

pub(crate) async fn wallet_top_up(
    storage: &dyn DomainStorage,
    owner_kind: &str,
    owner_id: &str,
    amount_credit_micros: i64,
    note: Option<&str>,
    created_by_user_id: Option<&str>,
) -> Result<crate::AppWalletResult, String> {
    if amount_credit_micros <= 0 {
        return Err("充值金额必须大于 0".to_owned());
    }
    let (owner_kind, owner_id) = wallet_owner(storage, owner_kind, owner_id).await?;
    let wallet = storage
        .ensure_wallet(owner_kind.clone(), owner_id.clone())
        .await?;
    storage
        .adjust_wallet(AppWalletLedgerEntry {
            id: crate::generate_id("wl", 8),
            wallet_id: wallet.id,
            entry_kind: "manual_adjustment".to_owned(),
            amount_credit_micros,
            balance_after_credit_micros: 0,
            request_log_id: None,
            api_key_id: None,
            pricing_rule_id: None,
            raw_usage_json: None,
            note: normalized_note(note),
            created_by_user_id: normalized_note(created_by_user_id),
            created_at: now_ts(),
        })
        .await
        .map_err(|error| format!("adjust wallet failed: {error}"))?;
    read_wallet_result(storage, owner_kind, owner_id).await
}

pub(crate) async fn wallet_set_available(
    storage: &dyn DomainStorage,
    owner_kind: &str,
    owner_id: &str,
    available_credit_micros: i64,
    note: Option<&str>,
    created_by_user_id: Option<&str>,
) -> Result<crate::AppWalletResult, String> {
    if available_credit_micros < 0 {
        return Err("可用额度必须是非负数字".to_owned());
    }
    let (owner_kind, owner_id) = wallet_owner(storage, owner_kind, owner_id).await?;
    let wallet = storage
        .ensure_wallet(owner_kind.clone(), owner_id.clone())
        .await?;
    let target_balance = available_credit_micros.saturating_add(wallet.frozen_credit_micros);
    let delta = target_balance.saturating_sub(wallet.balance_credit_micros);
    if delta != 0 {
        storage
            .adjust_wallet(AppWalletLedgerEntry {
                id: crate::generate_id("wl", 8),
                wallet_id: wallet.id,
                entry_kind: "manual_adjustment".to_owned(),
                amount_credit_micros: delta,
                balance_after_credit_micros: 0,
                request_log_id: None,
                api_key_id: None,
                pricing_rule_id: None,
                raw_usage_json: None,
                note: normalized_note(note).or_else(|| Some("set available credit".to_owned())),
                created_by_user_id: normalized_note(created_by_user_id),
                created_at: now_ts(),
            })
            .await
            .map_err(|error| format!("set wallet credit failed: {error}"))?;
    }
    read_wallet_result(storage, owner_kind, owner_id).await
}

async fn public_user(
    storage: &dyn DomainStorage,
    user_id: String,
) -> Result<AppUserPublicResult, String> {
    let user = storage
        .user(user_id.clone())
        .await?
        .ok_or_else(|| "当前用户不存在".to_owned())?;
    let wallet = if user.role != "admin" {
        storage.wallet("user".to_owned(), user.id.clone()).await?
    } else {
        None
    };
    Ok(crate::public_user(user, wallet))
}

pub(crate) async fn update_profile(
    storage: &dyn DomainStorage,
    actor: &RpcActor,
    display_name: Option<&str>,
) -> Result<AppUserPublicResult, String> {
    let user_id = actor_user_id(actor, "profile")?;
    storage
        .user(user_id.clone())
        .await?
        .ok_or_else(|| "当前用户不存在".to_owned())?;
    storage
        .update_user_profile(user_id.clone(), normalized_display_name(display_name))
        .await
        .map_err(|error| format!("update app user profile failed: {error}"))?;
    public_user(storage, user_id).await
}

pub(crate) async fn create_user(
    storage: &dyn DomainStorage,
    input: crate::AppUserCreateInput,
) -> Result<AppUserPublicResult, String> {
    let username = crate::normalize_username(&input.username)?;
    crate::validate_password(&input.password)?;
    if storage
        .users()
        .await?
        .into_iter()
        .any(|user| user.username.eq_ignore_ascii_case(&username))
    {
        return Err("用户名已存在".to_owned());
    }
    let role = crate::normalize_role(input.role.as_deref())?;
    let initial_balance = input.initial_balance_credit_micros.unwrap_or(0);
    if role == "admin" && initial_balance > 0 {
        return Err("管理员账号不参与额度分发".to_owned());
    }
    let now = now_ts();
    let user = codexmanager_core::storage::AppUser {
        id: crate::generate_id("usr", 8),
        username,
        display_name: normalized_display_name(input.display_name.as_deref()),
        password_hash: crate::hash_password(&input.password),
        role,
        status: "active".to_owned(),
        created_at: now,
        updated_at: now,
        last_login_at: None,
    };
    storage
        .create_user(user.clone(), initial_balance)
        .await
        .map_err(|error| format!("create app user failed: {error}"))?;
    let created = storage
        .user(user.id.clone())
        .await?
        .ok_or_else(|| "用户创建失败".to_owned())?;
    let wallet = if created.role != "admin" {
        storage
            .wallet("user".to_owned(), created.id.clone())
            .await?
    } else {
        None
    };
    Ok(crate::public_user(created, wallet))
}

pub(crate) async fn update_user(
    storage: &dyn DomainStorage,
    input: crate::AppUserUpdateInput,
) -> Result<AppUserPublicResult, String> {
    let user_id = input.id.trim();
    if user_id.is_empty() {
        return Err("用户 ID 不能为空".to_owned());
    }
    let current = storage
        .user(user_id.to_owned())
        .await?
        .ok_or_else(|| "用户不存在".to_owned())?;
    let role = normalized_role(input.role.as_deref(), &current.role)?;
    let status = normalized_status(input.status.as_deref(), &current.status)?;
    if current.role == "admin"
        && current.status == "active"
        && (role != "admin" || status != "active")
        && has_only_active_admin(&storage.users().await?)
    {
        return Err("至少需要保留一个启用的管理员账号".to_owned());
    }
    let password_hash = match normalized_display_name(input.password.as_deref()) {
        Some(password) => {
            crate::validate_password(&password)?;
            crate::hash_password(&password)
        }
        None => current.password_hash.clone(),
    };
    let next = codexmanager_core::storage::AppUser {
        id: current.id.clone(),
        username: current.username,
        display_name: normalized_display_name(input.display_name.as_deref()),
        password_hash,
        role,
        status,
        created_at: current.created_at,
        updated_at: now_ts(),
        last_login_at: current.last_login_at,
    };
    storage
        .update_user(next.clone())
        .await
        .map_err(|error| format!("update app user failed: {error}"))?;
    let updated = storage
        .user(next.id.clone())
        .await?
        .ok_or_else(|| "用户不存在".to_owned())?;
    let wallet = if updated.role != "admin" {
        storage
            .wallet("user".to_owned(), updated.id.clone())
            .await?
    } else {
        None
    };
    Ok(crate::public_user(updated, wallet))
}

pub(crate) async fn delete_user(storage: &dyn DomainStorage, user_id: &str) -> Result<(), String> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Err("用户 ID 不能为空".to_owned());
    }
    let user = storage
        .user(user_id.to_owned())
        .await?
        .ok_or_else(|| "用户不存在".to_owned())?;
    if user.role == "admin"
        && user.status == "active"
        && has_only_active_admin(&storage.users().await?)
    {
        return Err("至少需要保留一个启用的管理员账号".to_owned());
    }
    storage
        .delete_user(user_id.to_owned())
        .await
        .map_err(|error| format!("delete app user failed: {error}"))
}

pub(crate) async fn change_password(
    storage: &dyn DomainStorage,
    actor: &RpcActor,
    current_password: &str,
    new_password: &str,
) -> Result<(), String> {
    let user_id = actor_user_id(actor, "password change")?;
    crate::validate_password(new_password)?;
    let user = storage
        .user(user_id.clone())
        .await?
        .ok_or_else(|| "当前用户不存在".to_owned())?;
    if !crate::verify_password_hash(current_password, &user.password_hash) {
        return Err("当前密码不正确".to_owned());
    }
    storage
        .update_user_password(
            user_id,
            user.password_hash,
            crate::hash_password(new_password),
        )
        .await
        .map_err(|error| format!("update app user password failed: {error}"))
}

pub(crate) async fn set_api_key_owner(
    storage: &dyn DomainStorage,
    key_id: &str,
    owner_kind: &str,
    owner_user_id: Option<&str>,
    project_id: Option<&str>,
) -> Result<ApiKeyOwnerResult, String> {
    let key_id = key_id.trim();
    if key_id.is_empty() {
        return Err("API Key ID 不能为空".to_owned());
    }
    if !storage
        .api_keys()
        .await?
        .into_iter()
        .any(|key| key.id == key_id)
    {
        return Err("API Key 不存在".to_owned());
    }
    let owner_kind = match owner_kind.trim().to_ascii_lowercase().as_str() {
        "user" => "user",
        "project" => "project",
        _ => return Err("归属类型必须是 user 或 project".to_owned()),
    };
    let (owner_user_id, project_id, owner_id) = if owner_kind == "user" {
        let user_id = owner_user_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("用户归属需要 userId")?;
        let user = storage
            .user(user_id.to_owned())
            .await?
            .ok_or("用户不存在")?;
        if user.role == "admin" {
            return Err("管理员账号不参与额度分发".to_owned());
        }
        if user.status != "active" {
            return Err("用户已禁用".to_owned());
        }
        (Some(user_id.to_owned()), None, user_id.to_owned())
    } else {
        let project_id = project_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or("项目归属需要 projectId")?;
        (None, Some(project_id.to_owned()), project_id.to_owned())
    };
    storage
        .ensure_wallet(owner_kind.to_owned(), owner_id)
        .await
        .map_err(|error| format!("ensure app wallet failed: {error}"))?;
    let owner = ApiKeyOwner {
        key_id: key_id.to_owned(),
        owner_kind: owner_kind.to_owned(),
        owner_user_id,
        project_id,
        updated_at: now_ts(),
    };
    storage
        .save_api_key_owner(owner.clone())
        .await
        .map_err(|error| format!("save api key owner failed: {error}"))?;
    Ok(ApiKeyOwnerResult {
        key_id: owner.key_id,
        owner_kind: owner.owner_kind,
        owner_user_id: owner.owner_user_id,
        project_id: owner.project_id,
        updated_at: owner.updated_at,
    })
}
