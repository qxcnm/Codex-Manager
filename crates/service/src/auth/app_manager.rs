use codexmanager_core::storage::{
    now_ts, ApiKeyOwner, AppUser, AppUserSession, AppWallet, AppWalletLedgerEntry, BillingRule,
    Storage,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::app_settings::{
    get_persisted_app_setting, normalize_optional_text, parse_bool_with_default,
    save_persisted_app_setting, APP_SETTING_DISTRIBUTION_ENABLED_KEY,
    APP_SETTING_WEB_AUTH_MODE_KEY,
};
use crate::storage_helpers::open_storage;
use crate::RpcActor;

pub const WEB_AUTH_MODE_NONE: &str = "none";
pub const WEB_AUTH_MODE_PASSWORD: &str = "password";
pub const WEB_AUTH_MODE_ACCOUNTS: &str = "accounts";
const SESSION_TTL_SECONDS: i64 = 60 * 60 * 24 * 14;
const CREDIT_MICROS_PER_USD: f64 = 1_000_000.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUserPublicResult {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_login_at: Option<i64>,
    pub wallet: Option<AppWalletResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppWalletResult {
    pub id: String,
    pub owner_kind: String,
    pub owner_id: String,
    pub balance_credit_micros: i64,
    pub frozen_credit_micros: i64,
    pub available_credit_micros: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppLoginResult {
    pub token: String,
    pub expires_at: i64,
    pub user: AppUserPublicResult,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionUserResult {
    pub session_id: String,
    pub expires_at: i64,
    pub user: AppUserPublicResult,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyOwnerResult {
    pub key_id: String,
    pub owner_kind: String,
    pub owner_user_id: Option<String>,
    pub project_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSessionResult {
    pub mode: String,
    pub current_user: Option<AppUserPublicResult>,
    pub role: String,
    pub permissions: Vec<String>,
    pub distribution_enabled: bool,
    pub billing_mode_lock: BillingModeLockResult,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingModeLockResult {
    pub account_mode_locked: bool,
    pub distribution_locked: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUserCreateInput {
    pub username: String,
    pub password: String,
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub initial_balance_credit_micros: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUserUpdateInput {
    pub id: String,
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub password: Option<String>,
}

pub fn current_web_auth_mode() -> String {
    if let Some(raw) = get_persisted_app_setting(APP_SETTING_WEB_AUTH_MODE_KEY) {
        let mode = normalize_web_auth_mode(Some(&raw));
        if mode == WEB_AUTH_MODE_PASSWORD && !super::web_access::web_access_password_configured() {
            return WEB_AUTH_MODE_NONE.to_string();
        }
        return mode.to_string();
    }
    if super::web_access::web_access_password_configured() {
        WEB_AUTH_MODE_PASSWORD.to_string()
    } else {
        WEB_AUTH_MODE_NONE.to_string()
    }
}

pub fn set_web_auth_mode(mode: &str) -> Result<String, String> {
    let normalized = normalize_web_auth_mode(Some(mode));
    let current = current_web_auth_mode();
    if current == WEB_AUTH_MODE_ACCOUNTS && normalized != WEB_AUTH_MODE_ACCOUNTS {
        let lock = billing_mode_lock_status()?;
        if lock.account_mode_locked {
            return Err("account_billing_mode_locked".to_string());
        }
    }
    if normalized == WEB_AUTH_MODE_PASSWORD && !super::web_access::web_access_password_configured()
    {
        return Err("启用访问密码模式前需要先设置访问密码".to_string());
    }
    save_persisted_app_setting(APP_SETTING_WEB_AUTH_MODE_KEY, Some(normalized))?;
    Ok(normalized.to_string())
}

pub fn distribution_enabled() -> bool {
    get_persisted_app_setting(APP_SETTING_DISTRIBUTION_ENABLED_KEY)
        .as_deref()
        .map(|raw| parse_bool_with_default(raw, false))
        .unwrap_or(false)
}

fn distribution_enabled_for_storage(storage: &Storage) -> bool {
    let raw = storage
        .get_app_setting(APP_SETTING_DISTRIBUTION_ENABLED_KEY)
        .ok()
        .flatten();
    normalize_optional_text(raw.as_deref())
        .as_deref()
        .map(|raw| parse_bool_with_default(raw, false))
        .unwrap_or(false)
}

pub fn set_distribution_enabled(enabled: bool) -> Result<bool, String> {
    if enabled && current_web_auth_mode() != WEB_AUTH_MODE_ACCOUNTS {
        return Err("distribution_requires_accounts_mode".to_string());
    }
    if !enabled && distribution_enabled() {
        let lock = billing_mode_lock_status()?;
        if lock.distribution_locked {
            return Err("distribution_mode_locked".to_string());
        }
    }
    save_persisted_app_setting(
        APP_SETTING_DISTRIBUTION_ENABLED_KEY,
        Some(if enabled { "true" } else { "false" }),
    )?;
    Ok(enabled)
}

pub fn billing_mode_lock_status() -> Result<BillingModeLockResult, String> {
    let storage = open_storage_or_error()?;
    billing_mode_lock_status_for_storage(&storage)
}

fn billing_mode_lock_status_for_storage(
    storage: &Storage,
) -> Result<BillingModeLockResult, String> {
    let reasons = billing_mode_lock_reasons(storage)?;
    let has_reasons = !reasons.is_empty();
    Ok(BillingModeLockResult {
        account_mode_locked: has_reasons,
        distribution_locked: has_reasons,
        reasons,
    })
}

fn billing_mode_lock_reasons(storage: &Storage) -> Result<Vec<String>, String> {
    let mut reasons = Vec::new();
    if storage
        .member_app_user_count()
        .map_err(|err| format!("read member users failed: {err}"))?
        > 0
    {
        reasons.push("member_users".to_string());
    }
    if storage
        .api_key_owner_count()
        .map_err(|err| format!("read api key owners failed: {err}"))?
        > 0
    {
        reasons.push("api_key_owners".to_string());
    }
    if storage
        .nonzero_wallet_count()
        .map_err(|err| format!("read wallets failed: {err}"))?
        > 0
    {
        reasons.push("wallet_balance".to_string());
    }
    if storage
        .wallet_ledger_entry_count()
        .map_err(|err| format!("read wallet ledger failed: {err}"))?
        > 0
    {
        reasons.push("wallet_ledger".to_string());
    }
    if storage
        .user_model_group_assignment_count()
        .map_err(|err| format!("read model group assignments failed: {err}"))?
        > 0
    {
        reasons.push("model_group_assignments".to_string());
    }
    if storage
        .request_charge_ledger_entry_count()
        .map_err(|err| format!("read wallet request charges failed: {err}"))?
        > 0
    {
        reasons.push("request_charges".to_string());
    }
    Ok(reasons)
}

pub fn app_auth_status_value() -> Result<Value, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let user_count = storage
        .app_user_count()
        .map_err(|err| format!("read app users failed: {err}"))?;
    let active_admin_count = storage
        .active_admin_count()
        .map_err(|err| format!("read app admins failed: {err}"))?;
    let billing_mode_lock = billing_mode_lock_status_for_storage(&storage)?;
    Ok(serde_json::json!({
        "mode": current_web_auth_mode(),
        "modeOptions": [
            WEB_AUTH_MODE_NONE,
            WEB_AUTH_MODE_PASSWORD,
            WEB_AUTH_MODE_ACCOUNTS
        ],
        "passwordConfigured": super::web_access::web_access_password_configured(),
        "appUsersConfigured": active_admin_count > 0,
        "appUserCount": user_count,
        "activeAdminCount": active_admin_count,
        "distributionEnabled": distribution_enabled(),
        "billingModeLock": billing_mode_lock,
    }))
}

pub fn app_session_result(actor: &RpcActor) -> Result<AppSessionResult, String> {
    crate::initialize_storage_if_needed()?;
    let current_user = actor
        .user_id
        .as_deref()
        .map(|user_id| {
            let storage = open_storage_or_error()?;
            let user = storage
                .find_app_user_by_id(user_id)
                .map_err(|err| format!("read app user failed: {err}"))?
                .ok_or_else(|| "当前用户不存在".to_string())?;
            let wallet = if app_user_can_own_wallet(&user) {
                storage
                    .find_wallet_by_owner("user", &user.id)
                    .map_err(|err| format!("read app wallet failed: {err}"))?
            } else {
                None
            };
            Ok::<_, String>(public_user(user, wallet))
        })
        .transpose()?;
    Ok(AppSessionResult {
        mode: current_web_auth_mode(),
        current_user,
        role: actor.role.clone(),
        permissions: actor
            .permissions()
            .into_iter()
            .map(str::to_string)
            .collect(),
        distribution_enabled: distribution_enabled(),
        billing_mode_lock: billing_mode_lock_status()?,
    })
}

pub fn bootstrap_app_admin(
    username: &str,
    password: &str,
    display_name: Option<&str>,
) -> Result<AppLoginResult, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let active_admin_count = storage
        .active_admin_count()
        .map_err(|err| format!("read app admins failed: {err}"))?;
    if active_admin_count > 0 {
        return Err("管理员已初始化".to_string());
    }
    let input = AppUserCreateInput {
        username: username.to_string(),
        password: password.to_string(),
        display_name: display_name.map(str::to_string),
        role: Some("admin".to_string()),
        initial_balance_credit_micros: Some(0),
    };
    let public = create_app_user_with_storage(&storage, input)?;
    let user = storage
        .find_app_user_by_id(&public.id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "管理员创建失败".to_string())?;
    create_session_with_storage(&storage, user)
}

pub fn login_app_user(username: &str, password: &str) -> Result<AppLoginResult, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let username = normalize_username(username)?;
    let Some(user) = storage
        .find_app_user_by_username(&username)
        .map_err(|err| format!("read app user failed: {err}"))?
    else {
        return Err("用户名或密码错误".to_string());
    };
    if user.status != "active" || !verify_password_hash(password, &user.password_hash) {
        return Err("用户名或密码错误".to_string());
    }
    let now = now_ts();
    storage
        .update_app_user_last_login(&user.id, now)
        .map_err(|err| format!("update app user login failed: {err}"))?;
    let mut next = user;
    next.last_login_at = Some(now);
    next.updated_at = now;
    create_session_with_storage(&storage, next)
}

pub fn resolve_app_user_session(token: &str) -> Result<Option<AppSessionUserResult>, String> {
    let token = token.trim();
    if token.is_empty() {
        return Ok(None);
    }
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let now = now_ts();
    let token_hash = token_hash(token);
    let Some(session) = storage
        .find_active_app_session_by_token_hash(&token_hash, now)
        .map_err(|err| format!("read app session failed: {err}"))?
    else {
        return Ok(None);
    };
    let Some(user) = storage
        .find_app_user_by_id(&session.user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
    else {
        return Ok(None);
    };
    if user.status != "active" {
        return Ok(None);
    }
    let _ = storage.touch_app_user_session(&session.id, now);
    let wallet = storage
        .find_wallet_by_owner("user", &user.id)
        .map_err(|err| format!("read app wallet failed: {err}"))?;
    Ok(Some(AppSessionUserResult {
        session_id: session.id,
        expires_at: session.expires_at,
        user: public_user(user, wallet),
    }))
}

pub fn logout_app_user_session(token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Ok(());
    }
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    storage
        .revoke_app_user_session_by_token_hash(&token_hash(token), now_ts())
        .map_err(|err| format!("revoke app session failed: {err}"))?;
    Ok(())
}

pub fn create_app_user(input: AppUserCreateInput) -> Result<AppUserPublicResult, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    create_app_user_with_storage(&storage, input)
}

pub fn list_app_users() -> Result<Vec<AppUserPublicResult>, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let users = storage
        .list_app_users()
        .map_err(|err| format!("list app users failed: {err}"))?;
    users
        .into_iter()
        .map(|user| {
            let wallet = if app_user_can_own_wallet(&user) {
                storage
                    .find_wallet_by_owner("user", &user.id)
                    .map_err(|err| format!("read app wallet failed: {err}"))?
            } else {
                None
            };
            Ok(public_user(user, wallet))
        })
        .collect()
}

pub fn update_app_user(input: AppUserUpdateInput) -> Result<AppUserPublicResult, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let user_id = input.id.trim();
    if user_id.is_empty() {
        return Err("用户 ID 不能为空".to_string());
    }
    let current = storage
        .find_app_user_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "用户不存在".to_string())?;
    let next_role = input
        .role
        .as_deref()
        .map(|value| normalize_role(Some(value)))
        .transpose()?
        .unwrap_or_else(|| current.role.clone());
    let next_status = input
        .status
        .as_deref()
        .map(normalize_status)
        .transpose()?
        .unwrap_or_else(|| current.status.clone());

    if current.role == "admin"
        && current.status == "active"
        && (next_role != "admin" || next_status != "active")
    {
        let active_admin_count = storage
            .active_admin_count()
            .map_err(|err| format!("read app admins failed: {err}"))?;
        if active_admin_count <= 1 {
            return Err("至少需要保留一个启用的管理员账号".to_string());
        }
    }

    storage
        .update_app_user_display_name(
            user_id,
            normalize_optional_text(input.display_name.as_deref()),
        )
        .map_err(|err| format!("update app user display name failed: {err}"))?;
    if current.role != next_role {
        storage
            .update_app_user_role(user_id, &next_role)
            .map_err(|err| format!("update app user role failed: {err}"))?;
    }
    if current.status != next_status {
        storage
            .update_app_user_status(user_id, &next_status)
            .map_err(|err| format!("update app user status failed: {err}"))?;
    }
    if let Some(password) = normalize_optional_text(input.password.as_deref()) {
        validate_password(&password)?;
        storage
            .update_app_user_password_hash(user_id, &hash_password(&password))
            .map_err(|err| format!("update app user password failed: {err}"))?;
    }

    let updated = storage
        .find_app_user_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "用户不存在".to_string())?;
    let wallet = if app_user_can_own_wallet(&updated) {
        Some(ensure_wallet(&storage, "user", &updated.id)?)
    } else {
        None
    };
    Ok(public_user(updated, wallet))
}

pub fn delete_app_user(user_id: &str) -> Result<(), String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Err("用户 ID 不能为空".to_string());
    }
    let user = storage
        .find_app_user_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "用户不存在".to_string())?;
    if user.role == "admin" && user.status == "active" {
        let active_admin_count = storage
            .active_admin_count()
            .map_err(|err| format!("read app admins failed: {err}"))?;
        if active_admin_count <= 1 {
            return Err("至少需要保留一个启用的管理员账号".to_string());
        }
    }
    let deleted = storage
        .delete_app_user(user_id)
        .map_err(|err| format!("delete app user failed: {err}"))?;
    if deleted == 0 {
        return Err("用户不存在".to_string());
    }
    Ok(())
}

pub fn list_api_key_owners() -> Result<Vec<ApiKeyOwnerResult>, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let mut owners = storage
        .list_api_key_owners()
        .map_err(|err| format!("list api key owners failed: {err}"))?
        .into_values()
        .map(api_key_owner_result)
        .collect::<Vec<_>>();
    owners.sort_by(|a, b| a.key_id.cmp(&b.key_id));
    Ok(owners)
}

pub fn list_api_key_ids_for_user(user_id: &str) -> Result<Vec<String>, String> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Ok(Vec::new());
    }
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    storage
        .list_api_key_ids_for_user(user_id)
        .map_err(|err| format!("list api key ids for user failed: {err}"))
}

pub fn api_key_belongs_to_user(key_id: &str, user_id: &str) -> Result<bool, String> {
    let key_id = key_id.trim();
    let user_id = user_id.trim();
    if key_id.is_empty() || user_id.is_empty() {
        return Ok(false);
    }
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let owner = storage
        .find_api_key_owner(key_id)
        .map_err(|err| format!("read api key owner failed: {err}"))?;
    Ok(owner.is_some_and(|owner| {
        owner.owner_kind == "user" && owner.owner_user_id.as_deref().map(str::trim) == Some(user_id)
    }))
}

pub fn update_app_user_profile(
    actor: &RpcActor,
    display_name: Option<&str>,
) -> Result<AppUserPublicResult, String> {
    let Some(user_id) = actor
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Err("permission_denied: profile requires user session".to_string());
    };
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    storage
        .update_app_user_display_name(user_id, normalize_optional_text(display_name))
        .map_err(|err| format!("update app user profile failed: {err}"))?;
    let user = storage
        .find_app_user_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "当前用户不存在".to_string())?;
    let wallet = if app_user_can_own_wallet(&user) {
        storage
            .find_wallet_by_owner("user", &user.id)
            .map_err(|err| format!("read app wallet failed: {err}"))?
    } else {
        None
    };
    Ok(public_user(user, wallet))
}

pub fn change_app_user_password(
    actor: &RpcActor,
    current_password: &str,
    new_password: &str,
) -> Result<(), String> {
    let Some(user_id) = actor
        .user_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return Err("permission_denied: password change requires user session".to_string());
    };
    validate_password(new_password)?;
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let user = storage
        .find_app_user_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "当前用户不存在".to_string())?;
    if !verify_password_hash(current_password, &user.password_hash) {
        return Err("当前密码不正确".to_string());
    }
    storage
        .update_app_user_password_hash(user_id, &hash_password(new_password))
        .map_err(|err| format!("update app user password failed: {err}"))?;
    Ok(())
}

pub fn wallet_top_up(
    owner_kind: &str,
    owner_id: &str,
    amount_credit_micros: i64,
    note: Option<&str>,
    created_by_user_id: Option<&str>,
) -> Result<AppWalletResult, String> {
    if amount_credit_micros <= 0 {
        return Err("充值金额必须大于 0".to_string());
    }
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let owner_kind = normalize_owner_kind(owner_kind)?;
    let owner_id = owner_id.trim();
    if owner_kind == "user" {
        let _ = ensure_user_can_own_wallet(&storage, owner_id)?;
    }
    let wallet = ensure_wallet(&storage, owner_kind, owner_id)?;
    let ledger = AppWalletLedgerEntry {
        id: generate_id("wl", 8),
        wallet_id: wallet.id.clone(),
        entry_kind: "manual_adjustment".to_string(),
        amount_credit_micros,
        balance_after_credit_micros: 0,
        request_log_id: None,
        api_key_id: None,
        pricing_rule_id: None,
        raw_usage_json: None,
        note: normalize_optional_text(note),
        created_by_user_id: normalize_optional_text(created_by_user_id),
        created_at: now_ts(),
    };
    let entry = storage
        .adjust_wallet_balance(&ledger)
        .map_err(|err| format!("adjust wallet failed: {err}"))?;
    let next = storage
        .find_wallet_by_owner(&wallet.owner_kind, &wallet.owner_id)
        .map_err(|err| format!("read app wallet failed: {err}"))?
        .ok_or_else(|| "钱包不存在".to_string())?;
    log::info!(
        "event=app_wallet_top_up wallet_id={} amount={} balance_after={}",
        entry.wallet_id,
        entry.amount_credit_micros,
        entry.balance_after_credit_micros
    );
    Ok(wallet_result(next))
}

pub fn wallet_set_available_credit(
    owner_kind: &str,
    owner_id: &str,
    available_credit_micros: i64,
    note: Option<&str>,
    created_by_user_id: Option<&str>,
) -> Result<AppWalletResult, String> {
    if available_credit_micros < 0 {
        return Err("可用额度必须是非负数字".to_string());
    }
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let owner_kind = normalize_owner_kind(owner_kind)?;
    let owner_id = owner_id.trim();
    if owner_kind == "user" {
        let _ = ensure_user_can_own_wallet(&storage, owner_id)?;
    }
    let wallet = ensure_wallet(&storage, owner_kind, owner_id)?;
    let target_balance = available_credit_micros.saturating_add(wallet.frozen_credit_micros);
    let delta = target_balance.saturating_sub(wallet.balance_credit_micros);
    if delta == 0 {
        return Ok(wallet_result(wallet));
    }
    let ledger = AppWalletLedgerEntry {
        id: generate_id("wl", 8),
        wallet_id: wallet.id.clone(),
        entry_kind: "manual_adjustment".to_string(),
        amount_credit_micros: delta,
        balance_after_credit_micros: 0,
        request_log_id: None,
        api_key_id: None,
        pricing_rule_id: None,
        raw_usage_json: None,
        note: normalize_optional_text(note).or_else(|| Some("set available credit".to_string())),
        created_by_user_id: normalize_optional_text(created_by_user_id),
        created_at: now_ts(),
    };
    let entry = storage
        .adjust_wallet_balance(&ledger)
        .map_err(|err| format!("set wallet credit failed: {err}"))?;
    let next = storage
        .find_wallet_by_owner(&wallet.owner_kind, &wallet.owner_id)
        .map_err(|err| format!("read app wallet failed: {err}"))?
        .ok_or_else(|| "钱包不存在".to_string())?;
    log::info!(
        "event=app_wallet_set_available wallet_id={} delta={} balance_after={}",
        entry.wallet_id,
        entry.amount_credit_micros,
        entry.balance_after_credit_micros
    );
    Ok(wallet_result(next))
}

pub fn set_api_key_owner(
    key_id: &str,
    owner_kind: &str,
    owner_user_id: Option<&str>,
    project_id: Option<&str>,
) -> Result<ApiKeyOwnerResult, String> {
    crate::initialize_storage_if_needed()?;
    let storage = open_storage_or_error()?;
    let key_id = key_id.trim();
    if key_id.is_empty() {
        return Err("API Key ID 不能为空".to_string());
    }
    if storage
        .find_api_key_by_id(key_id)
        .map_err(|err| format!("read api key failed: {err}"))?
        .is_none()
    {
        return Err("API Key 不存在".to_string());
    }
    let owner_kind = normalize_owner_kind(owner_kind)?;
    let owner = match owner_kind {
        "user" => {
            let user_id = normalize_optional_text(owner_user_id)
                .ok_or_else(|| "用户归属需要 userId".to_string())?;
            let _ = ensure_user_can_own_wallet(&storage, &user_id)?;
            let _ = ensure_wallet(&storage, "user", &user_id)?;
            ApiKeyOwner {
                key_id: key_id.to_string(),
                owner_kind: owner_kind.to_string(),
                owner_user_id: Some(user_id),
                project_id: None,
                updated_at: now_ts(),
            }
        }
        "project" => {
            let project_id = normalize_optional_text(project_id)
                .ok_or_else(|| "项目归属需要 projectId".to_string())?;
            let _ = ensure_wallet(&storage, "project", &project_id)?;
            ApiKeyOwner {
                key_id: key_id.to_string(),
                owner_kind: owner_kind.to_string(),
                owner_user_id: None,
                project_id: Some(project_id),
                updated_at: now_ts(),
            }
        }
        _ => return Err("不支持的归属类型".to_string()),
    };
    storage
        .upsert_api_key_owner(&owner)
        .map_err(|err| format!("save api key owner failed: {err}"))?;
    Ok(api_key_owner_result(owner))
}

pub fn wallet_precheck_for_api_key(storage: &Storage, key_id: &str) -> Result<(), String> {
    if !distribution_enabled_for_storage(storage) {
        return Ok(());
    }
    let Some(owner) = storage
        .find_api_key_owner(key_id)
        .map_err(|err| format!("read api key owner failed: {err}"))?
    else {
        return Ok(());
    };
    let (owner_kind, owner_id) = owner_identity(&owner)?;
    if owner_kind == "user" {
        let _ = ensure_user_can_own_wallet(storage, owner_id)?;
    }
    let wallet = storage
        .find_wallet_by_owner(owner_kind, owner_id)
        .map_err(|err| format!("read app wallet failed: {err}"))?
        .ok_or_else(|| "归属钱包不存在".to_string())?;
    if wallet.status != "active" {
        return Err("归属钱包已停用".to_string());
    }
    if wallet.balance_credit_micros <= wallet.frozen_credit_micros {
        return Err("归属钱包余额不足".to_string());
    }
    Ok(())
}

pub fn wallet_charge_for_request(
    storage: &Storage,
    key_id: Option<&str>,
    request_log_id: i64,
    estimated_cost_usd: f64,
    model: Option<&str>,
    service_tier: Option<&str>,
    raw_usage_json: Option<String>,
) -> Result<Option<AppWalletLedgerEntry>, String> {
    if !distribution_enabled_for_storage(storage) {
        return Ok(None);
    }
    let Some(key_id) = key_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let now = now_ts();
    let Some(owner) = storage
        .find_api_key_owner(key_id)
        .map_err(|err| format!("read api key owner failed: {err}"))?
    else {
        return Ok(None);
    };
    let (owner_kind, owner_id) = owner_identity(&owner)?;
    if owner_kind == "user" {
        let _ = ensure_user_can_own_wallet(storage, owner_id)?;
    }
    let wallet = storage
        .find_wallet_by_owner(owner_kind, owner_id)
        .map_err(|err| format!("read app wallet failed: {err}"))?
        .ok_or_else(|| "归属钱包不存在".to_string())?;
    let model_group_access = match model.map(str::trim).filter(|value| !value.is_empty()) {
        Some(platform_model) => {
            crate::resolve_api_key_model_group_access(storage, key_id, platform_model)?
        }
        None => None,
    };
    let billing_rule = if model_group_access.is_some() {
        None
    } else {
        resolve_billing_rule_for_request(storage, key_id, &owner, model, service_tier, now)?
    };
    let multiplier_millis = model_group_access
        .as_ref()
        .map(|access| access.rate_multiplier_millis.max(0))
        .or_else(|| {
            billing_rule
                .as_ref()
                .map(|rule| rule.multiplier_millis.max(0))
        })
        .unwrap_or(1_000);
    let base_cost_usd = model_group_base_cost_usd(
        storage,
        model_group_access.as_ref(),
        estimated_cost_usd,
        raw_usage_json.as_deref(),
    );
    if base_cost_usd <= 0.0 {
        return Ok(None);
    }
    let charged_cost_usd = base_cost_usd * multiplier_millis as f64 / 1_000.0;
    let charge = (charged_cost_usd * CREDIT_MICROS_PER_USD).round() as i64;
    if charge <= 0 {
        return Ok(None);
    }
    let ledger = AppWalletLedgerEntry {
        id: generate_id("wl", 8),
        wallet_id: wallet.id,
        entry_kind: "request_charge".to_string(),
        amount_credit_micros: -charge,
        balance_after_credit_micros: 0,
        request_log_id: Some(request_log_id),
        api_key_id: Some(key_id.to_string()),
        pricing_rule_id: billing_rule.as_ref().map(|rule| rule.id.clone()),
        raw_usage_json: usage_json_with_billing(
            raw_usage_json,
            estimated_cost_usd,
            base_cost_usd,
            charged_cost_usd,
            multiplier_millis,
            billing_rule.as_ref(),
            model_group_access.as_ref(),
        ),
        note: billing_rule
            .as_ref()
            .map(|rule| format!("billing_rule={}", rule.name)),
        created_by_user_id: None,
        created_at: now,
    };
    let entry = storage
        .adjust_wallet_balance(&ledger)
        .map_err(|err| format!("charge wallet failed: {err}"))?;
    Ok(Some(entry))
}

fn model_group_base_cost_usd(
    storage: &Storage,
    model_group_access: Option<&codexmanager_core::storage::ModelGroupAccess>,
    estimated_cost_usd: f64,
    raw_usage_json: Option<&str>,
) -> f64 {
    model_group_access
        .and_then(|access| access.billing_model_slug.as_deref())
        .and_then(|billing_model| {
            estimate_billing_model_cost_usd(storage, billing_model, raw_usage_json)
        })
        .unwrap_or(estimated_cost_usd)
        .max(0.0)
}

fn estimate_billing_model_cost_usd(
    storage: &Storage,
    billing_model: &str,
    raw_usage_json: Option<&str>,
) -> Option<f64> {
    let usage = raw_usage_json.and_then(|raw| serde_json::from_str::<Value>(raw).ok())?;
    let input_tokens = extract_usage_token(
        &usage,
        &["inputTokens", "input_tokens", "prompt_tokens"],
        &[&["usage", "input_tokens"], &["usage", "prompt_tokens"]],
    );
    let cached_input_tokens = extract_usage_token(
        &usage,
        &[
            "cachedInputTokens",
            "cached_input_tokens",
            "cache_read_input_tokens",
        ],
        &[
            &["input_tokens_details", "cached_tokens"],
            &["prompt_tokens_details", "cached_tokens"],
            &["usage", "input_tokens_details", "cached_tokens"],
            &["usage", "prompt_tokens_details", "cached_tokens"],
            &["usage", "cache_read_input_tokens"],
        ],
    );
    let output_tokens = extract_usage_token(
        &usage,
        &["outputTokens", "output_tokens", "completion_tokens"],
        &[&["usage", "output_tokens"], &["usage", "completion_tokens"]],
    );
    if input_tokens.is_none() && cached_input_tokens.is_none() && output_tokens.is_none() {
        return None;
    }
    let cost = crate::quota::model_pricing::estimate_cost_usd_for_log(
        storage,
        Some(billing_model),
        input_tokens,
        cached_input_tokens,
        output_tokens,
    );
    if cost > 0.0 {
        Some(cost)
    } else {
        None
    }
}

fn extract_usage_token(value: &Value, keys: &[&str], nested_paths: &[&[&str]]) -> Option<i64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(value_as_i64))
        .or_else(|| {
            nested_paths
                .iter()
                .find_map(|path| value_at_path(value, path).and_then(value_as_i64))
        })
        .map(|value| value.max(0))
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    Some(cursor)
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| {
            value.as_f64().and_then(|value| {
                if value.is_finite() && value >= 0.0 {
                    Some(value.round() as i64)
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| value.trim().parse::<i64>().ok())
        })
}

fn resolve_billing_rule_for_request(
    storage: &Storage,
    key_id: &str,
    owner: &ApiKeyOwner,
    model: Option<&str>,
    service_tier: Option<&str>,
    now: i64,
) -> Result<Option<BillingRule>, String> {
    let rules = storage
        .list_active_billing_rules(now)
        .map_err(|err| format!("list billing rules failed: {err}"))?;
    Ok(rules
        .into_iter()
        .filter(|rule| billing_rule_matches(rule, key_id, owner, model, service_tier))
        .max_by_key(|rule| {
            (
                rule.priority,
                billing_rule_scope_score(rule),
                rule.model_pattern
                    .as_deref()
                    .map(str::len)
                    .unwrap_or_default() as i64,
                rule.updated_at,
            )
        }))
}

fn billing_rule_matches(
    rule: &BillingRule,
    key_id: &str,
    owner: &ApiKeyOwner,
    model: Option<&str>,
    service_tier: Option<&str>,
) -> bool {
    if !matches_optional_text(rule.api_key_id.as_deref(), Some(key_id)) {
        return false;
    }
    if !matches_optional_text(rule.user_id.as_deref(), owner.owner_user_id.as_deref()) {
        return false;
    }
    if !matches_optional_text(rule.project_id.as_deref(), owner.project_id.as_deref()) {
        return false;
    }
    if !matches_optional_text(rule.service_tier.as_deref(), service_tier) {
        return false;
    }
    billing_model_matches(rule.model_pattern.as_deref(), model)
}

fn matches_optional_text(rule_value: Option<&str>, context_value: Option<&str>) -> bool {
    let Some(rule_value) = rule_value.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    context_value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value.eq_ignore_ascii_case(rule_value))
}

fn billing_model_matches(rule_pattern: Option<&str>, model: Option<&str>) -> bool {
    let Some(pattern) = rule_pattern
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "*")
    else {
        return true;
    };
    let Some(model) = model
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("unknown"))
    else {
        return false;
    };
    let pattern = pattern.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    if pattern.contains('*') {
        crate::quota::model_pricing::wildcard_matches(&pattern, &model)
    } else {
        model.starts_with(&pattern)
    }
}

fn billing_rule_scope_score(rule: &BillingRule) -> i64 {
    [
        rule.api_key_id.as_deref(),
        rule.user_id.as_deref(),
        rule.project_id.as_deref(),
        rule.service_tier.as_deref(),
        rule.model_pattern.as_deref(),
    ]
    .into_iter()
    .filter(|value| value.map(str::trim).is_some_and(|text| !text.is_empty()))
    .count() as i64
}

fn usage_json_with_billing(
    raw_usage_json: Option<String>,
    platform_estimated_cost_usd: f64,
    base_cost_usd: f64,
    charged_cost_usd: f64,
    multiplier_millis: i64,
    billing_rule: Option<&BillingRule>,
    model_group_access: Option<&codexmanager_core::storage::ModelGroupAccess>,
) -> Option<String> {
    let mut value = raw_usage_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !value.is_object() {
        value = serde_json::json!({ "raw": value });
    }
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "platformEstimatedCostUsd".to_string(),
            serde_json::json!(platform_estimated_cost_usd.max(0.0)),
        );
        object.insert(
            "baseEstimatedCostUsd".to_string(),
            serde_json::json!(base_cost_usd.max(0.0)),
        );
        object.insert(
            "chargedCostUsd".to_string(),
            serde_json::json!(charged_cost_usd.max(0.0)),
        );
        object.insert(
            "billingMultiplierMillis".to_string(),
            serde_json::json!(multiplier_millis.max(0)),
        );
        if let Some(rule) = billing_rule {
            object.insert("billingRuleId".to_string(), serde_json::json!(rule.id));
            object.insert("billingRuleName".to_string(), serde_json::json!(rule.name));
        }
        if let Some(access) = model_group_access {
            object.insert(
                "modelGroupId".to_string(),
                serde_json::json!(access.group_id),
            );
            object.insert(
                "modelGroupName".to_string(),
                serde_json::json!(access.group_name),
            );
            object.insert(
                "platformModelSlug".to_string(),
                serde_json::json!(access.platform_model_slug),
            );
            if let Some(billing_model_slug) = access.billing_model_slug.as_deref() {
                object.insert(
                    "billingModelSlug".to_string(),
                    serde_json::json!(billing_model_slug),
                );
            }
        }
    }
    serde_json::to_string(&value).ok()
}

fn open_storage_or_error() -> Result<crate::storage_helpers::StorageHandle, String> {
    open_storage().ok_or_else(|| "存储不可用".to_string())
}

fn create_app_user_with_storage(
    storage: &Storage,
    input: AppUserCreateInput,
) -> Result<AppUserPublicResult, String> {
    let username = normalize_username(&input.username)?;
    validate_password(&input.password)?;
    if storage
        .find_app_user_by_username(&username)
        .map_err(|err| format!("read app user failed: {err}"))?
        .is_some()
    {
        return Err("用户名已存在".to_string());
    }
    let role = normalize_role(input.role.as_deref())?;
    if role == "admin" && input.initial_balance_credit_micros.unwrap_or(0) > 0 {
        return Err("管理员账号不参与额度分发".to_string());
    }
    let now = now_ts();
    let user = AppUser {
        id: generate_id("usr", 8),
        username,
        display_name: normalize_optional_text(input.display_name.as_deref()),
        password_hash: hash_password(&input.password),
        role,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_login_at: None,
    };
    storage
        .insert_app_user(&user)
        .map_err(|err| format!("create app user failed: {err}"))?;
    let wallet = if app_user_can_own_wallet(&user) {
        storage
            .assign_default_model_group_to_user(&user.id)
            .map_err(|err| format!("assign default model group failed: {err}"))?;
        Some(ensure_wallet(storage, "user", &user.id)?)
    } else {
        None
    };
    if let Some(initial_balance) = input
        .initial_balance_credit_micros
        .filter(|value| *value > 0)
    {
        let wallet = wallet
            .as_ref()
            .ok_or_else(|| "管理员账号不参与额度分发".to_string())?;
        let ledger = AppWalletLedgerEntry {
            id: generate_id("wl", 8),
            wallet_id: wallet.id.clone(),
            entry_kind: "initial_grant".to_string(),
            amount_credit_micros: initial_balance,
            balance_after_credit_micros: 0,
            request_log_id: None,
            api_key_id: None,
            pricing_rule_id: None,
            raw_usage_json: None,
            note: Some("initial balance".to_string()),
            created_by_user_id: None,
            created_at: now_ts(),
        };
        let _ = storage
            .adjust_wallet_balance(&ledger)
            .map_err(|err| format!("grant app wallet failed: {err}"))?;
    }
    let wallet = if app_user_can_own_wallet(&user) {
        storage
            .find_wallet_by_owner("user", &user.id)
            .map_err(|err| format!("read app wallet failed: {err}"))?
    } else {
        None
    };
    Ok(public_user(user, wallet))
}

fn create_session_with_storage(storage: &Storage, user: AppUser) -> Result<AppLoginResult, String> {
    let now = now_ts();
    let token = generate_session_token();
    let session = AppUserSession {
        id: generate_id("sess", 8),
        user_id: user.id.clone(),
        token_hash: token_hash(&token),
        expires_at: now.saturating_add(SESSION_TTL_SECONDS),
        created_at: now,
        last_seen_at: Some(now),
        revoked_at: None,
    };
    storage
        .insert_app_user_session(&session)
        .map_err(|err| format!("create app session failed: {err}"))?;
    let wallet = if app_user_can_own_wallet(&user) {
        storage
            .find_wallet_by_owner("user", &user.id)
            .map_err(|err| format!("read app wallet failed: {err}"))?
    } else {
        None
    };
    Ok(AppLoginResult {
        token,
        expires_at: session.expires_at,
        user: public_user(user, wallet),
    })
}

fn ensure_wallet(storage: &Storage, owner_kind: &str, owner_id: &str) -> Result<AppWallet, String> {
    let owner_kind = normalize_owner_kind(owner_kind)?;
    let owner_id = owner_id.trim();
    if owner_id.is_empty() {
        return Err("钱包归属 ID 不能为空".to_string());
    }
    storage
        .ensure_wallet_for_owner(&generate_id("wlt", 8), owner_kind, owner_id)
        .map_err(|err| format!("ensure app wallet failed: {err}"))
}

fn app_user_can_own_wallet(user: &AppUser) -> bool {
    user.role != "admin"
}

fn ensure_user_can_own_wallet(storage: &Storage, user_id: &str) -> Result<AppUser, String> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Err("用户归属需要 userId".to_string());
    }
    let user = storage
        .find_app_user_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "用户不存在".to_string())?;
    if !app_user_can_own_wallet(&user) {
        return Err("管理员账号不参与额度分发".to_string());
    }
    if user.status != "active" {
        return Err("用户已禁用".to_string());
    }
    Ok(user)
}

fn public_user(user: AppUser, wallet: Option<AppWallet>) -> AppUserPublicResult {
    let can_own_wallet = app_user_can_own_wallet(&user);
    AppUserPublicResult {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        role: user.role,
        status: user.status,
        created_at: user.created_at,
        updated_at: user.updated_at,
        last_login_at: user.last_login_at,
        wallet: if can_own_wallet {
            wallet.map(wallet_result)
        } else {
            None
        },
    }
}

fn wallet_result(wallet: AppWallet) -> AppWalletResult {
    AppWalletResult {
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

fn api_key_owner_result(owner: ApiKeyOwner) -> ApiKeyOwnerResult {
    ApiKeyOwnerResult {
        key_id: owner.key_id,
        owner_kind: owner.owner_kind,
        owner_user_id: owner.owner_user_id,
        project_id: owner.project_id,
        updated_at: owner.updated_at,
    }
}

fn owner_identity(owner: &ApiKeyOwner) -> Result<(&str, &str), String> {
    match owner.owner_kind.as_str() {
        "user" => owner
            .owner_user_id
            .as_deref()
            .map(|id| ("user", id))
            .ok_or_else(|| "API Key 用户归属缺失".to_string()),
        "project" => owner
            .project_id
            .as_deref()
            .map(|id| ("project", id))
            .ok_or_else(|| "API Key 项目归属缺失".to_string()),
        _ => Err("API Key 归属类型无效".to_string()),
    }
}

fn normalize_web_auth_mode(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some(WEB_AUTH_MODE_PASSWORD) => WEB_AUTH_MODE_PASSWORD,
        Some(WEB_AUTH_MODE_ACCOUNTS) => WEB_AUTH_MODE_ACCOUNTS,
        _ => WEB_AUTH_MODE_NONE,
    }
}

fn normalize_owner_kind(raw: &str) -> Result<&'static str, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "user" => Ok("user"),
        "project" => Ok("project"),
        _ => Err("归属类型必须是 user 或 project".to_string()),
    }
}

fn normalize_role(raw: Option<&str>) -> Result<String, String> {
    let role = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("member")
        .to_ascii_lowercase();
    match role.as_str() {
        "admin" | "member" => Ok(role),
        _ => Err("角色必须是 admin 或 member".to_string()),
    }
}

fn normalize_status(raw: &str) -> Result<String, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "active" => Ok("active".to_string()),
        "disabled" => Ok("disabled".to_string()),
        _ => Err("状态必须是 active 或 disabled".to_string()),
    }
}

fn normalize_username(raw: &str) -> Result<String, String> {
    let value = raw.trim().to_ascii_lowercase();
    if value.len() < 3 || value.len() > 64 {
        return Err("用户名长度需要在 3 到 64 之间".to_string());
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err("用户名仅支持字母、数字、点、下划线和短横线".to_string());
    }
    Ok(value)
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.len() < 8 {
        return Err("密码至少需要 8 位".to_string());
    }
    Ok(())
}

fn hash_password(password: &str) -> String {
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let salt_hex = hex_encode(&salt);
    let digest = hex_sha256(format!("{salt_hex}:{password}").as_bytes());
    format!("sha256${salt_hex}${digest}")
}

fn verify_password_hash(password: &str, stored_hash: &str) -> bool {
    let mut parts = stored_hash.split('$');
    let Some(kind) = parts.next() else {
        return false;
    };
    let Some(salt_hex) = parts.next() else {
        return false;
    };
    let Some(expected_hash) = parts.next() else {
        return false;
    };
    if kind != "sha256" || parts.next().is_some() {
        return false;
    }
    super::rpc::constant_time_eq(
        hex_sha256(format!("{salt_hex}:{password}").as_bytes()).as_bytes(),
        expected_hash.as_bytes(),
    )
}

fn token_hash(token: &str) -> String {
    hex_sha256(format!("codexmanager-app-session:{token}").as_bytes())
}

fn generate_session_token() -> String {
    format!("cms_{}", random_hex(32))
}

fn generate_id(prefix: &str, bytes_len: usize) -> String {
    format!("{prefix}_{}", random_hex(bytes_len))
}

fn random_hex(bytes_len: usize) -> String {
    let mut bytes = vec![0u8; bytes_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex_encode(&bytes)
}

fn hex_sha256(bytes: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes.as_ref());
    let digest = hasher.finalize();
    hex_encode(digest.as_slice())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
