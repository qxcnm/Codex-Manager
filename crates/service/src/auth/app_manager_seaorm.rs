use super::*;
use codexmanager_storage_seaorm::{
    ApiKeysRepository as Keys, BillingRepository as Billing, ModelGroupsRepository as Groups,
    SeaOrmStorage, SettingsRepository, UsersRepository as Users,
};

fn failure(error: impl std::fmt::Display) -> String {
    format!("account storage failed: {error}")
}
pub(super) fn run<T, F, Fut>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(SeaOrmStorage) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, String>> + Send + 'static,
{
    crate::storage_helpers::seaorm_block_on(operation)
}
pub(super) fn enabled() -> bool {
    crate::storage_helpers::seaorm_enabled()
}
async fn public(storage: &SeaOrmStorage, user: AppUser) -> Result<AppUserPublicResult, String> {
    let wallet = if app_user_can_own_wallet(&user) {
        Billing::wallet_by_owner(storage.connection(), "user", &user.id)
            .await
            .map_err(failure)?
    } else {
        None
    };
    Ok(public_user(user, wallet))
}
pub(super) async fn public_by_id(
    storage: SeaOrmStorage,
    id: String,
) -> Result<AppUserPublicResult, String> {
    let user = Users::get(storage.connection(), &id)
        .await
        .map_err(failure)?
        .ok_or("当前用户不存在")?;
    public(&storage, user).await
}
pub(super) async fn lock_status(storage: SeaOrmStorage) -> Result<BillingModeLockResult, String> {
    let reasons = Users::billing_lock_reasons(storage.connection())
        .await
        .map_err(failure)?;
    Ok(BillingModeLockResult {
        account_mode_locked: !reasons.is_empty(),
        distribution_locked: !reasons.is_empty(),
        reasons,
    })
}
pub(super) async fn counts(storage: SeaOrmStorage) -> Result<(usize, usize), String> {
    let users = Users::list(storage.connection()).await.map_err(failure)?;
    Ok((
        users.len(),
        users
            .iter()
            .filter(|u| u.role == "admin" && u.status == "active")
            .count(),
    ))
}
async fn create(
    storage: &SeaOrmStorage,
    input: AppUserCreateInput,
    bootstrap: bool,
) -> Result<AppUser, String> {
    let username = normalize_username(&input.username)?;
    validate_password(&input.password)?;
    let now = now_ts();
    let user = AppUser {
        id: generate_id("usr", 8),
        username,
        display_name: normalize_optional_text(input.display_name.as_deref()),
        password_hash: hash_password(&input.password),
        role: normalize_role(input.role.as_deref())?,
        status: "active".into(),
        created_at: now,
        updated_at: now,
        last_login_at: None,
    };
    Users::create_user(
        storage.connection(),
        user.clone(),
        input.initial_balance_credit_micros.unwrap_or(0),
        bootstrap,
    )
    .await
    .map_err(failure)?;
    Ok(user)
}
async fn session(storage: &SeaOrmStorage, user: AppUser) -> Result<AppLoginResult, String> {
    let now = now_ts();
    let token = generate_session_token();
    let expires = now.saturating_add(SESSION_TTL_SECONDS);
    Users::create_session(
        storage.connection(),
        AppUserSession {
            id: generate_id("sess", 8),
            user_id: user.id.clone(),
            token_hash: token_hash(&token),
            expires_at: expires,
            created_at: now,
            last_seen_at: Some(now),
            revoked_at: None,
        },
    )
    .await
    .map_err(failure)?;
    Ok(AppLoginResult {
        token,
        expires_at: expires,
        user: public(storage, user).await?,
    })
}
pub(super) async fn bootstrap(
    storage: SeaOrmStorage,
    input: AppUserCreateInput,
) -> Result<AppLoginResult, String> {
    let user = create(&storage, input, true).await?;
    session(&storage, user).await
}
pub(super) async fn create_user(
    storage: SeaOrmStorage,
    input: AppUserCreateInput,
) -> Result<AppUserPublicResult, String> {
    let user = create(&storage, input, false).await?;
    public(&storage, user).await
}
pub(super) async fn login(
    storage: SeaOrmStorage,
    username: String,
    password: String,
) -> Result<AppLoginResult, String> {
    let username = normalize_username(&username)?;
    let mut user = Users::find_by_username(storage.connection(), &username)
        .await
        .map_err(failure)?
        .ok_or("用户名或密码错误")?;
    if user.status != "active" || !verify_password_hash(&password, &user.password_hash) {
        return Err("用户名或密码错误".into());
    }
    user.last_login_at = Some(now_ts());
    user.updated_at = now_ts();
    Users::touch_login(storage.connection(), &user.id, user.updated_at)
        .await
        .map_err(failure)?;
    session(&storage, user).await
}
pub(super) async fn resolve(
    storage: SeaOrmStorage,
    token: String,
) -> Result<Option<AppSessionUserResult>, String> {
    if token.trim().is_empty() {
        return Ok(None);
    }
    let Some((session, user)) =
        Users::active_session(storage.connection(), &token_hash(token.trim()), now_ts())
            .await
            .map_err(failure)?
    else {
        return Ok(None);
    };
    Users::touch_session(storage.connection(), &session.id, now_ts())
        .await
        .map_err(failure)?;
    Ok(Some(AppSessionUserResult {
        session_id: session.id,
        expires_at: session.expires_at,
        user: public(&storage, user).await?,
    }))
}
pub(super) async fn logout(storage: SeaOrmStorage, token: String) -> Result<(), String> {
    Users::revoke_session(storage.connection(), &token_hash(token.trim()), now_ts())
        .await
        .map_err(failure)
}
pub(super) async fn list(storage: SeaOrmStorage) -> Result<Vec<AppUserPublicResult>, String> {
    let mut result = Vec::new();
    for user in Users::list(storage.connection()).await.map_err(failure)? {
        result.push(public(&storage, user).await?);
    }
    Ok(result)
}
pub(super) async fn update(
    storage: SeaOrmStorage,
    input: AppUserUpdateInput,
) -> Result<AppUserPublicResult, String> {
    let mut user = Users::get(storage.connection(), input.id.trim())
        .await
        .map_err(failure)?
        .ok_or("用户不存在")?;
    if let Some(role) = input.role {
        user.role = normalize_role(Some(&role))?;
    }
    if let Some(status) = input.status {
        user.status = normalize_status(&status)?;
    }
    user.display_name = normalize_optional_text(input.display_name.as_deref());
    if let Some(password) = normalize_optional_text(input.password.as_deref()) {
        validate_password(&password)?;
        user.password_hash = hash_password(&password);
    }
    user.updated_at = now_ts();
    Users::update_user(storage.connection(), user.clone())
        .await
        .map_err(failure)?;
    public(&storage, user).await
}
pub(super) async fn delete(storage: SeaOrmStorage, id: String) -> Result<(), String> {
    Users::delete_user(storage.connection(), id.trim())
        .await
        .map_err(failure)
}
pub(super) async fn owners(storage: SeaOrmStorage) -> Result<Vec<ApiKeyOwnerResult>, String> {
    Ok(Users::list_owners(storage.connection())
        .await
        .map_err(failure)?
        .into_iter()
        .map(api_key_owner_result)
        .collect())
}
pub(super) async fn keys_for_user(
    storage: SeaOrmStorage,
    id: String,
) -> Result<Vec<String>, String> {
    Ok(Users::list_owners(storage.connection())
        .await
        .map_err(failure)?
        .into_iter()
        .filter(|v| v.owner_kind == "user" && v.owner_user_id.as_deref() == Some(id.trim()))
        .map(|v| v.key_id)
        .collect())
}
pub(super) async fn belongs(
    storage: SeaOrmStorage,
    key: String,
    user: String,
) -> Result<bool, String> {
    Ok(Users::owner(storage.connection(), key.trim())
        .await
        .map_err(failure)?
        .is_some_and(|v| v.owner_kind == "user" && v.owner_user_id.as_deref() == Some(user.trim())))
}
pub(super) async fn profile(
    storage: SeaOrmStorage,
    id: String,
    name: Option<String>,
) -> Result<AppUserPublicResult, String> {
    let mut user = Users::get(storage.connection(), &id)
        .await
        .map_err(failure)?
        .ok_or("当前用户不存在")?;
    user.display_name = normalize_optional_text(name.as_deref());
    user.updated_at = now_ts();
    Users::update_profile(
        storage.connection(),
        &user.id,
        user.display_name.clone(),
        user.updated_at,
    )
    .await
    .map_err(failure)?;
    public(&storage, user).await
}
pub(super) async fn password(
    storage: SeaOrmStorage,
    id: String,
    current: String,
    next: String,
) -> Result<(), String> {
    validate_password(&next)?;
    let user = Users::get(storage.connection(), &id)
        .await
        .map_err(failure)?
        .ok_or("当前用户不存在")?;
    if !verify_password_hash(&current, &user.password_hash) {
        return Err("当前密码不正确".into());
    }
    Users::update_password(
        storage.connection(),
        &user.id,
        &user.password_hash,
        hash_password(&next),
        now_ts(),
    )
    .await
    .map_err(failure)
}
async fn check_user(storage: &SeaOrmStorage, id: &str) -> Result<(), String> {
    let user = Users::get(storage.connection(), id)
        .await
        .map_err(failure)?
        .ok_or("用户不存在")?;
    if user.role == "admin" {
        return Err("管理员账号不参与额度分发".into());
    }
    if user.status != "active" {
        return Err("用户已禁用".into());
    }
    Ok(())
}
pub(super) async fn adjust(
    storage: SeaOrmStorage,
    kind: String,
    id: String,
    amount: i64,
    note: Option<String>,
    actor: Option<String>,
    set: bool,
) -> Result<AppWalletResult, String> {
    if (!set && amount <= 0) || (set && amount < 0) {
        return Err("额度必须是非负数字，充值必须大于 0".into());
    }
    let kind = normalize_owner_kind(&kind)?;
    let id = id.trim();
    if id.is_empty() {
        return Err("钱包归属 ID 不能为空".into());
    }
    if kind == "user" {
        check_user(&storage, id).await?;
    }
    let wallet = Billing::ensure_wallet(storage.connection(), &generate_id("wlt", 8), kind, id)
        .await
        .map_err(failure)?;
    let entry = AppWalletLedgerEntry {
        id: generate_id("wl", 8),
        wallet_id: wallet.id.clone(),
        entry_kind: "manual_adjustment".into(),
        amount_credit_micros: amount,
        balance_after_credit_micros: 0,
        request_log_id: None,
        api_key_id: None,
        pricing_rule_id: None,
        raw_usage_json: None,
        note: normalize_optional_text(note.as_deref()),
        created_by_user_id: normalize_optional_text(actor.as_deref()),
        created_at: now_ts(),
    };
    if set {
        return Billing::set_available(storage.connection(), entry, amount)
            .await
            .map(wallet_result)
            .map_err(failure);
    }
    Billing::adjust_balance(storage.connection(), entry)
        .await
        .map_err(failure)?;
    Billing::wallet(storage.connection(), &wallet.id)
        .await
        .map_err(failure)?
        .map(wallet_result)
        .ok_or_else(|| "钱包不存在".into())
}
pub(super) async fn set_owner(
    storage: SeaOrmStorage,
    key: String,
    kind: String,
    user: Option<String>,
    project: Option<String>,
) -> Result<ApiKeyOwnerResult, String> {
    let key = key.trim();
    if Keys::get(storage.connection(), key)
        .await
        .map_err(failure)?
        .is_none()
    {
        return Err("API Key 不存在".into());
    }
    let kind = normalize_owner_kind(&kind)?;
    let (user, project, id) = if kind == "user" {
        let uid = normalize_optional_text(user.as_deref()).ok_or("用户归属需要 userId")?;
        check_user(&storage, &uid).await?;
        (Some(uid.clone()), None, uid)
    } else {
        let pid = normalize_optional_text(project.as_deref()).ok_or("项目归属需要 projectId")?;
        (None, Some(pid.clone()), pid)
    };
    Billing::ensure_wallet(storage.connection(), &generate_id("wlt", 8), kind, &id)
        .await
        .map_err(failure)?;
    let owner = ApiKeyOwner {
        key_id: key.into(),
        owner_kind: kind.into(),
        owner_user_id: user,
        project_id: project,
        updated_at: now_ts(),
    };
    Users::put_owner(storage.connection(), owner.clone())
        .await
        .map_err(failure)?;
    Ok(api_key_owner_result(owner))
}
async fn distributed(storage: &SeaOrmStorage) -> Result<bool, String> {
    Ok(
        SettingsRepository::get(storage.connection(), APP_SETTING_DISTRIBUTION_ENABLED_KEY)
            .await
            .map_err(failure)?
            .is_some_and(|v| parse_bool_with_default(&v.value, false)),
    )
}
pub(super) async fn precheck(
    storage: SeaOrmStorage,
    key: String,
    rate: Option<i64>,
) -> Result<(), String> {
    if !distributed(&storage).await? {
        return Ok(());
    }
    let Some(owner) = Users::owner(storage.connection(), &key)
        .await
        .map_err(failure)?
    else {
        return Ok(());
    };
    let (kind, id) = owner_identity(&owner)?;
    if kind == "user" {
        check_user(&storage, id).await?;
    }
    let wallet = Billing::wallet_by_owner(storage.connection(), kind, id)
        .await
        .map_err(failure)?
        .ok_or("归属钱包不存在")?;
    if wallet.status != "active" {
        return Err("归属钱包已停用".into());
    }
    if rate != Some(0) && wallet.balance_credit_micros <= wallet.frozen_credit_micros {
        return Err("归属钱包余额不足".into());
    }
    Ok(())
}
pub(super) async fn charge(
    storage: SeaOrmStorage,
    mut input: codexmanager_core::storage::ChargeSnapshotInputV2,
    key: Option<String>,
    service_tier: Option<String>,
    charge_wallet: bool,
) -> Result<codexmanager_core::storage::ChargeSnapshotV2, String> {
    let model = input.model_slug.trim().to_owned();
    if model.is_empty() {
        return Err("model_slug_required".into());
    }
    let catalog = crate::models_v2::policy_catalog_slug(&model);
    input.pricing_model_slug = (catalog != model).then(|| catalog.into());
    let mut rule = None;
    let mut group = None;
    if charge_wallet && distributed(&storage).await? {
        if let Some(key) = key.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            if let Some(owner) = Users::owner(storage.connection(), key)
                .await
                .map_err(failure)?
            {
                let (kind, id) = owner_identity(&owner)?;
                if kind == "user" {
                    check_user(&storage, id).await?;
                }
                let wallet = Billing::wallet_by_owner(storage.connection(), kind, id)
                    .await
                    .map_err(failure)?
                    .ok_or("归属钱包不存在")?;
                if wallet.status != "active" {
                    return Err("归属钱包已停用".into());
                }
                if kind == "user" {
                    group = Groups::resolve_access_v2(storage.connection(), id, catalog, now_ts())
                        .await
                        .map_err(failure)?;
                    if group.is_none() {
                        return Err(format!("model_not_allowed: {model}"));
                    }
                }
                if group.is_none() {
                    rule = Users::active_billing_rules(storage.connection(), now_ts())
                        .await
                        .map_err(failure)?
                        .into_iter()
                        .filter(|r| {
                            billing_rule_matches(
                                r,
                                key,
                                &owner,
                                Some(&model),
                                service_tier.as_deref(),
                            )
                        })
                        .max_by_key(|r| {
                            (
                                r.priority,
                                billing_rule_scope_score(r),
                                r.model_pattern.as_deref().map(str::len).unwrap_or(0),
                                r.updated_at,
                            )
                        });
                }
                input.rate_multiplier_millis = group
                    .as_ref()
                    .map(|g| g.rate_multiplier_millis)
                    .or_else(|| rule.as_ref().map(|r| r.multiplier_millis))
                    .unwrap_or(1000)
                    .max(0);
                input.wallet_id = Some(wallet.id);
                input.api_key_id = Some(key.into());
            }
        }
    }
    input.raw_usage_json = usage_json_with_billing_context(
        input.raw_usage_json,
        rule.as_ref(),
        group.as_ref(),
        input.rate_multiplier_millis,
    );
    input.pricing_rule_id = rule.as_ref().map(|r| r.id.clone());
    input.ledger_note = rule
        .map(|r| format!("billing_rule={}", r.name))
        .or_else(|| group.map(|g| format!("model_group={}", g.group_id)));
    Billing::record_charge(storage.connection(), &input)
        .await
        .map_err(failure)
}
#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::StorageBackendKind;

    async fn fixture() -> SeaOrmStorage {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        storage
    }
    async fn exercise_auth_wallet(storage: SeaOrmStorage) {
        let username = format!(
            "remote-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros()
        );
        let input = AppUserCreateInput {
            username: username.clone(),
            password: "test-password".into(),
            display_name: Some("remote member".into()),
            role: Some("member".into()),
            initial_balance_credit_micros: Some(400),
        };
        let member = create_user(storage.clone(), input).await.unwrap();
        assert_eq!(member.wallet.as_ref().unwrap().balance_credit_micros, 400);
        assert!(login(storage.clone(), username.clone(), "incorrect".into())
            .await
            .is_err());
        let result = login(storage.clone(), username.clone(), "test-password".into())
            .await
            .unwrap();
        assert_eq!(
            resolve(storage.clone(), result.token.clone())
                .await
                .unwrap()
                .unwrap()
                .user
                .id,
            member.id
        );
        let updated = adjust(
            storage.clone(),
            "user".into(),
            member.id.clone(),
            100,
            None,
            None,
            false,
        )
        .await
        .unwrap();
        assert_eq!(updated.balance_credit_micros, 500);
        let updated = adjust(
            storage.clone(),
            "user".into(),
            member.id.clone(),
            120,
            None,
            None,
            true,
        )
        .await
        .unwrap();
        assert_eq!(updated.available_credit_micros, 120);
        password(
            storage.clone(),
            member.id.clone(),
            "test-password".into(),
            "next-password".into(),
        )
        .await
        .unwrap();
        assert!(
            login(storage.clone(), username.clone(), "test-password".into())
                .await
                .is_err()
        );
        assert!(
            login(storage.clone(), username.clone(), "next-password".into())
                .await
                .is_ok()
        );
        update(
            storage.clone(),
            AppUserUpdateInput {
                id: member.id.clone(),
                status: Some("disabled".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(resolve(storage.clone(), result.token.clone())
            .await
            .unwrap()
            .is_none());
        assert!(adjust(
            storage.clone(),
            "user".into(),
            member.id.clone(),
            1,
            None,
            None,
            false
        )
        .await
        .is_err());
        logout(storage.clone(), result.token).await.unwrap();
        let reasons = lock_status(storage).await.unwrap();
        assert!(reasons.account_mode_locked);
        assert!(reasons.reasons.contains(&"wallet_ledger".into()));
    }
    #[tokio::test]
    async fn remote_auth_wallet_and_revocation_use_one_authoritative_store() {
        exercise_auth_wallet(fixture().await).await;
    }
    #[tokio::test]
    #[ignore = "requires isolated MySQL URL"]
    async fn mysql_remote_auth_wallet_and_revocation() {
        let storage = SeaOrmStorage::connect(
            StorageBackendKind::Mysql,
            &std::env::var("CODEXMANAGER_TEST_MYSQL_URL").unwrap(),
        )
        .await
        .unwrap();
        storage.migrate().await.unwrap();
        exercise_auth_wallet(storage).await;
    }
    #[tokio::test]
    #[ignore = "requires isolated PostgreSQL URL"]
    async fn postgres_remote_auth_wallet_and_revocation() {
        let storage = SeaOrmStorage::connect(
            StorageBackendKind::Postgres,
            &std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap(),
        )
        .await
        .unwrap();
        storage.migrate().await.unwrap();
        exercise_auth_wallet(storage).await;
    }
    #[tokio::test]
    async fn remote_bootstrap_is_atomic_and_last_admin_cannot_be_removed() {
        let storage = fixture().await;
        let mut tasks = Vec::new();
        for n in 0..4 {
            let storage = storage.clone();
            tasks.push(tokio::spawn(async move {
                bootstrap(
                    storage,
                    AppUserCreateInput {
                        username: format!("admin-{n}"),
                        password: "test-password".into(),
                        role: Some("admin".into()),
                        ..Default::default()
                    },
                )
                .await
            }));
        }
        let mut success = Vec::new();
        for task in tasks {
            match task.await.unwrap() {
                Ok(result) => success.push(result),
                Err(err) => assert!(err.contains("管理员已初始化"), "{err}"),
            }
        }
        assert_eq!(success.len(), 1);
        let id = success.pop().unwrap().user.id;
        assert!(delete(storage.clone(), id.clone())
            .await
            .unwrap_err()
            .contains("至少需要保留"));
        assert!(update(
            storage,
            AppUserUpdateInput {
                id,
                status: Some("disabled".into()),
                ..Default::default()
            }
        )
        .await
        .unwrap_err()
        .contains("至少需要保留"));
    }
}
