//! Identity, session, ownership and pricing policy persistence.
pub(crate) mod users {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "app_users")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        #[sea_orm(unique)]
        pub username: String,
        pub display_name: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub password_hash: String,
        pub role: String,
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub last_login_at: Option<i64>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<users::Model> for codexmanager_core::storage::AppUser {
    fn from(row: users::Model) -> Self {
        Self {
            id: row.id,
            username: row.username,
            display_name: row.display_name,
            password_hash: row.password_hash,
            role: row.role,
            status: row.status,
            created_at: row.created_at,
            updated_at: row.updated_at,
            last_login_at: row.last_login_at,
        }
    }
}
impl From<codexmanager_core::storage::AppUser> for users::ActiveModel {
    fn from(row: codexmanager_core::storage::AppUser) -> Self {
        Self {
            id: sea_orm::Set(row.id),
            username: sea_orm::Set(row.username),
            display_name: sea_orm::Set(row.display_name),
            password_hash: sea_orm::Set(row.password_hash),
            role: sea_orm::Set(row.role),
            status: sea_orm::Set(row.status),
            created_at: sea_orm::Set(row.created_at),
            updated_at: sea_orm::Set(row.updated_at),
            last_login_at: sea_orm::Set(row.last_login_at),
        }
    }
}
pub(crate) mod sessions {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "app_user_sessions")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub user_id: String,
        #[sea_orm(unique)]
        pub token_hash: String,
        pub expires_at: i64,
        pub created_at: i64,
        pub last_seen_at: Option<i64>,
        pub revoked_at: Option<i64>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<sessions::Model> for codexmanager_core::storage::AppUserSession {
    fn from(row: sessions::Model) -> Self {
        Self {
            id: row.id,
            user_id: row.user_id,
            token_hash: row.token_hash,
            expires_at: row.expires_at,
            created_at: row.created_at,
            last_seen_at: row.last_seen_at,
            revoked_at: row.revoked_at,
        }
    }
}
impl From<codexmanager_core::storage::AppUserSession> for sessions::ActiveModel {
    fn from(row: codexmanager_core::storage::AppUserSession) -> Self {
        Self {
            id: sea_orm::Set(row.id),
            user_id: sea_orm::Set(row.user_id),
            token_hash: sea_orm::Set(row.token_hash),
            expires_at: sea_orm::Set(row.expires_at),
            created_at: sea_orm::Set(row.created_at),
            last_seen_at: sea_orm::Set(row.last_seen_at),
            revoked_at: sea_orm::Set(row.revoked_at),
        }
    }
}
pub(crate) mod owners {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "api_key_owners")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub key_id: String,
        pub owner_kind: String,
        pub owner_user_id: Option<String>,
        pub project_id: Option<String>,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<owners::Model> for codexmanager_core::storage::ApiKeyOwner {
    fn from(row: owners::Model) -> Self {
        Self {
            key_id: row.key_id,
            owner_kind: row.owner_kind,
            owner_user_id: row.owner_user_id,
            project_id: row.project_id,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::ApiKeyOwner> for owners::ActiveModel {
    fn from(row: codexmanager_core::storage::ApiKeyOwner) -> Self {
        Self {
            key_id: sea_orm::Set(row.key_id),
            owner_kind: sea_orm::Set(row.owner_kind),
            owner_user_id: sea_orm::Set(row.owner_user_id),
            project_id: sea_orm::Set(row.project_id),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
pub(crate) mod rules {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "billing_rules")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: String,
        pub status: String,
        pub priority: i64,
        pub multiplier_millis: i64,
        pub model_pattern: Option<String>,
        pub service_tier: Option<String>,
        pub user_id: Option<String>,
        pub project_id: Option<String>,
        pub api_key_id: Option<String>,
        pub starts_at: Option<i64>,
        pub ends_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
impl From<rules::Model> for codexmanager_core::storage::BillingRule {
    fn from(row: rules::Model) -> Self {
        Self {
            id: row.id,
            name: row.name,
            status: row.status,
            priority: row.priority,
            multiplier_millis: row.multiplier_millis,
            model_pattern: row.model_pattern,
            service_tier: row.service_tier,
            user_id: row.user_id,
            project_id: row.project_id,
            api_key_id: row.api_key_id,
            starts_at: row.starts_at,
            ends_at: row.ends_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}
impl From<codexmanager_core::storage::BillingRule> for rules::ActiveModel {
    fn from(row: codexmanager_core::storage::BillingRule) -> Self {
        Self {
            id: sea_orm::Set(row.id),
            name: sea_orm::Set(row.name),
            status: sea_orm::Set(row.status),
            priority: sea_orm::Set(row.priority),
            multiplier_millis: sea_orm::Set(row.multiplier_millis),
            model_pattern: sea_orm::Set(row.model_pattern),
            service_tier: sea_orm::Set(row.service_tier),
            user_id: sea_orm::Set(row.user_id),
            project_id: sea_orm::Set(row.project_id),
            api_key_id: sea_orm::Set(row.api_key_id),
            starts_at: sea_orm::Set(row.starts_at),
            ends_at: sea_orm::Set(row.ends_at),
            created_at: sea_orm::Set(row.created_at),
            updated_at: sea_orm::Set(row.updated_at),
        }
    }
}
use codexmanager_core::storage::{ApiKeyOwner, AppUser, AppUserSession, BillingRule};
use sea_orm::{entity::prelude::*, ActiveModelTrait, Condition, QueryOrder, Set};

pub struct UsersRepository;
impl UsersRepository {
    pub async fn get(db: &impl ConnectionTrait, id: &str) -> Result<Option<AppUser>, DbErr> {
        Ok(users::Entity::find_by_id(id).one(db).await?.map(Into::into))
    }
    pub async fn find_by_username(
        db: &impl ConnectionTrait,
        username: &str,
    ) -> Result<Option<AppUser>, DbErr> {
        Ok(users::Entity::find()
            .filter(users::Column::Username.eq(username))
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn list(db: &impl ConnectionTrait) -> Result<Vec<AppUser>, DbErr> {
        Ok(users::Entity::find()
            .order_by_asc(users::Column::CreatedAt)
            .order_by_asc(users::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn put(db: &impl ConnectionTrait, user: AppUser) -> Result<(), DbErr> {
        if user.id.trim().is_empty()
            || user.username.trim().is_empty()
            || user.password_hash.is_empty()
            || !matches!(user.role.as_str(), "admin" | "member")
            || !matches!(user.status.as_str(), "active" | "disabled")
        {
            return Err(DbErr::Custom("invalid app user".into()));
        }
        let existing = users::Entity::find_by_id(&user.id).one(db).await?;
        let mut model: users::ActiveModel = user.into();
        if let Some(existing) = existing {
            model.created_at = Set(existing.created_at);
            model.update(db).await?;
        } else {
            model.insert(db).await?;
        }
        Ok(())
    }
    pub async fn owner(
        db: &impl ConnectionTrait,
        key_id: &str,
    ) -> Result<Option<ApiKeyOwner>, DbErr> {
        Ok(owners::Entity::find_by_id(key_id)
            .one(db)
            .await?
            .map(Into::into))
    }
    pub async fn put_owner(db: &impl ConnectionTrait, owner: ApiKeyOwner) -> Result<(), DbErr> {
        let valid = match owner.owner_kind.as_str() {
            "user" => {
                owner
                    .owner_user_id
                    .as_deref()
                    .is_some_and(|v| !v.trim().is_empty())
                    && owner.project_id.is_none()
            }
            "project" => owner
                .project_id
                .as_deref()
                .is_some_and(|v| !v.trim().is_empty()),
            _ => false,
        };
        if !valid || owner.key_id.trim().is_empty() {
            return Err(DbErr::Custom("invalid API key owner".into()));
        }
        if owner.owner_kind == "user"
            && Self::get(db, owner.owner_user_id.as_deref().unwrap_or_default())
                .await?
                .is_none()
        {
            return Err(DbErr::Custom("API key owner user does not exist".into()));
        }
        let exists = owners::Entity::find_by_id(&owner.key_id)
            .one(db)
            .await?
            .is_some();
        let model: owners::ActiveModel = owner.into();
        if exists {
            model.update(db).await?;
        } else {
            model.insert(db).await?;
        }
        Ok(())
    }
    pub async fn create_session(
        db: &impl ConnectionTrait,
        session: AppUserSession,
    ) -> Result<(), DbErr> {
        let user = Self::get(db, &session.user_id)
            .await?
            .ok_or_else(|| DbErr::Custom("session user does not exist".into()))?;
        if user.status != "active"
            || session.expires_at <= session.created_at
            || session.token_hash.is_empty()
        {
            return Err(DbErr::Custom("invalid session".into()));
        }
        let model: sessions::ActiveModel = session.into();
        model.insert(db).await?;
        Ok(())
    }
    pub async fn active_session(
        db: &impl ConnectionTrait,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<(AppUserSession, AppUser)>, DbErr> {
        let Some(session) = sessions::Entity::find()
            .filter(sessions::Column::TokenHash.eq(token_hash))
            .filter(sessions::Column::ExpiresAt.gt(now))
            .filter(sessions::Column::RevokedAt.is_null())
            .one(db)
            .await?
        else {
            return Ok(None);
        };
        let Some(user) = Self::get(db, &session.user_id).await? else {
            return Ok(None);
        };
        if user.status != "active" {
            return Ok(None);
        }
        Ok(Some((session.into(), user)))
    }
    pub async fn revoke_session(
        db: &impl ConnectionTrait,
        token_hash: &str,
        now: i64,
    ) -> Result<(), DbErr> {
        sessions::Entity::update_many()
            .col_expr(sessions::Column::RevokedAt, Expr::value(now))
            .filter(sessions::Column::TokenHash.eq(token_hash))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn active_billing_rules(
        db: &impl ConnectionTrait,
        now: i64,
    ) -> Result<Vec<BillingRule>, DbErr> {
        Ok(rules::Entity::find()
            .filter(rules::Column::Status.eq("active"))
            .filter(
                Condition::any()
                    .add(rules::Column::StartsAt.is_null())
                    .add(rules::Column::StartsAt.lte(now)),
            )
            .filter(
                Condition::any()
                    .add(rules::Column::EndsAt.is_null())
                    .add(rules::Column::EndsAt.gt(now)),
            )
            .order_by_desc(rules::Column::Priority)
            .order_by_asc(rules::Column::CreatedAt)
            .order_by_asc(rules::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn put_billing_rule(
        db: &impl ConnectionTrait,
        rule: BillingRule,
    ) -> Result<(), DbErr> {
        if rule.id.trim().is_empty()
            || rule.name.trim().is_empty()
            || rule.multiplier_millis < 0
            || !matches!(rule.status.as_str(), "active" | "disabled")
            || matches!((rule.starts_at, rule.ends_at), (Some(start), Some(end)) if start >= end)
        {
            return Err(DbErr::Custom("invalid billing rule".into()));
        }
        let previous = rules::Entity::find_by_id(&rule.id).one(db).await?;
        let mut model: rules::ActiveModel = rule.into();
        if let Some(previous) = previous {
            model.created_at = Set(previous.created_at);
            model.update(db).await?;
        } else {
            model.insert(db).await?;
        }
        Ok(())
    }
}
pub(crate) mod locks {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "app_domain_locks")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub version: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) async fn initialize_domain_locks(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    for id in [
        "users",
        "model_groups",
        "accounts",
        "usage_snapshots",
        "api_keys",
        "request_logs",
        "conversation_bindings",
        "skill_repositories",
    ] {
        locks::Entity::insert(locks::ActiveModel {
            id: Set(id.into()),
            version: Set(0),
        })
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(locks::Column::Id)
                .do_nothing_on([locks::Column::Id])
                .to_owned(),
        )
        .do_nothing()
        .exec(db)
        .await?;
    }
    Ok(())
}
impl UsersRepository {
    pub async fn lock(db: &impl ConnectionTrait, domain: &str) -> Result<(), DbErr> {
        locks::Entity::update_many()
            .col_expr(
                locks::Column::Version,
                Expr::col(locks::Column::Version).into(),
            )
            .filter(locks::Column::Id.eq(domain))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn list_owners(db: &impl ConnectionTrait) -> Result<Vec<ApiKeyOwner>, DbErr> {
        Ok(owners::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn delete(db: &impl ConnectionTrait, id: &str) -> Result<(), DbErr> {
        // Preserve ledger history while revoking sessions and all owned API keys.
        let keys = owners::Entity::find()
            .filter(owners::Column::OwnerUserId.eq(id))
            .all(db)
            .await?;
        for key in keys {
            crate::ApiKeysRepository::update_status(db, &key.key_id, "disabled").await?;
        }
        sessions::Entity::delete_many()
            .filter(sessions::Column::UserId.eq(id))
            .exec(db)
            .await?;
        crate::model_groups::users::Entity::delete_many()
            .filter(crate::model_groups::users::Column::UserId.eq(id))
            .exec(db)
            .await?;
        users::Entity::delete_by_id(id).exec(db).await?;
        Ok(())
    }
    pub async fn touch_session(db: &impl ConnectionTrait, id: &str, now: i64) -> Result<(), DbErr> {
        sessions::Entity::update_many()
            .col_expr(sessions::Column::LastSeenAt, Expr::value(now))
            .filter(sessions::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn billing_lock_reasons(db: &impl ConnectionTrait) -> Result<Vec<String>, DbErr> {
        use sea_orm::PaginatorTrait;
        let mut reasons = Vec::new();
        if users::Entity::find()
            .filter(users::Column::Role.eq("member"))
            .count(db)
            .await?
            > 0
        {
            reasons.push("member_users".into());
        }
        if owners::Entity::find().count(db).await? > 0 {
            reasons.push("api_key_owners".into());
        }
        if crate::billing::wallets::Entity::find()
            .filter(
                Condition::any()
                    .add(crate::billing::wallets::Column::BalanceCreditMicros.ne(0))
                    .add(crate::billing::wallets::Column::FrozenCreditMicros.ne(0)),
            )
            .count(db)
            .await?
            > 0
        {
            reasons.push("wallet_balance".into());
        }
        if crate::billing::ledger::Entity::find().count(db).await? > 0 {
            reasons.push("wallet_ledger".into());
        }
        if crate::model_groups::users::Entity::find().count(db).await? > 0 {
            reasons.push("model_group_assignments".into());
        }
        if crate::billing::ledger::Entity::find()
            .filter(crate::billing::ledger::Column::EntryKind.eq("request_charge"))
            .count(db)
            .await?
            > 0
        {
            reasons.push("request_charges".into());
        }
        Ok(reasons)
    }
}
impl UsersRepository {
    pub async fn create_user(
        db: &sea_orm::DatabaseConnection,
        user: AppUser,
        initial_balance: i64,
        bootstrap: bool,
    ) -> Result<(), DbErr> {
        use sea_orm::TransactionTrait;
        let tx = db.begin().await?;
        Self::lock(&tx, "users").await?;
        if bootstrap
            && Self::list(&tx)
                .await?
                .iter()
                .any(|u| u.role == "admin" && u.status == "active")
        {
            return Err(DbErr::Custom("管理员已初始化".into()));
        }
        if Self::get(&tx, &user.id).await?.is_some()
            || Self::find_by_username(&tx, &user.username).await?.is_some()
        {
            return Err(DbErr::Custom("用户名已存在".into()));
        }
        if user.role == "admin" && initial_balance > 0 {
            return Err(DbErr::Custom("管理员账号不参与额度分发".into()));
        }
        Self::put(&tx, user.clone()).await?;
        if user.role == "member" {
            let wallet = crate::BillingRepository::ensure_wallet(
                &tx,
                &format!("wlt_{}", user.id),
                "user",
                &user.id,
            )
            .await?;
            crate::ModelGroupsRepository::assign_default(&tx, &user.id, user.created_at).await?;
            if initial_balance > 0 {
                crate::BillingRepository::adjust_in_transaction(
                    &tx,
                    codexmanager_core::storage::AppWalletLedgerEntry {
                        id: format!("wl_initial_{}", user.id),
                        wallet_id: wallet.id,
                        entry_kind: "initial_grant".into(),
                        amount_credit_micros: initial_balance,
                        balance_after_credit_micros: 0,
                        request_log_id: None,
                        api_key_id: None,
                        pricing_rule_id: None,
                        raw_usage_json: None,
                        note: Some("initial balance".into()),
                        created_by_user_id: None,
                        created_at: user.created_at,
                    },
                )
                .await?;
            }
        }
        tx.commit().await
    }
    pub async fn update_user(db: &sea_orm::DatabaseConnection, user: AppUser) -> Result<(), DbErr> {
        use sea_orm::TransactionTrait;
        let tx = db.begin().await?;
        Self::lock(&tx, "users").await?;
        let current = Self::get(&tx, &user.id)
            .await?
            .ok_or_else(|| DbErr::Custom("用户不存在".into()))?;
        if current.role == "admin"
            && current.status == "active"
            && (user.role != "admin" || user.status != "active")
            && Self::list(&tx)
                .await?
                .iter()
                .filter(|u| u.role == "admin" && u.status == "active")
                .count()
                <= 1
        {
            return Err(DbErr::Custom("至少需要保留一个启用的管理员账号".into()));
        }
        Self::put(&tx, user.clone()).await?;
        if user.role == "member" {
            crate::BillingRepository::ensure_wallet(
                &tx,
                &format!("wlt_{}", user.id),
                "user",
                &user.id,
            )
            .await?;
        }
        tx.commit().await
    }
    pub async fn delete_user(db: &sea_orm::DatabaseConnection, id: &str) -> Result<(), DbErr> {
        use sea_orm::TransactionTrait;
        let tx = db.begin().await?;
        Self::lock(&tx, "users").await?;
        let current = Self::get(&tx, id)
            .await?
            .ok_or_else(|| DbErr::Custom("用户不存在".into()))?;
        if current.role == "admin"
            && current.status == "active"
            && Self::list(&tx)
                .await?
                .iter()
                .filter(|u| u.role == "admin" && u.status == "active")
                .count()
                <= 1
        {
            return Err(DbErr::Custom("至少需要保留一个启用的管理员账号".into()));
        }
        Self::delete(&tx, id).await?;
        tx.commit().await
    }
}
impl UsersRepository {
    pub async fn billing_rules(db: &impl ConnectionTrait) -> Result<Vec<BillingRule>, DbErr> {
        Ok(rules::Entity::find()
            .order_by_desc(rules::Column::Priority)
            .order_by_desc(rules::Column::UpdatedAt)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn delete_billing_rule(db: &impl ConnectionTrait, id: &str) -> Result<(), DbErr> {
        rules::Entity::delete_by_id(id).exec(db).await?;
        Ok(())
    }
}
impl UsersRepository {
    pub async fn touch_login(db: &impl ConnectionTrait, id: &str, now: i64) -> Result<(), DbErr> {
        users::Entity::update_many()
            .col_expr(users::Column::LastLoginAt, Expr::value(now))
            .col_expr(users::Column::UpdatedAt, Expr::value(now))
            .filter(users::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_profile(
        db: &impl ConnectionTrait,
        id: &str,
        name: Option<String>,
        now: i64,
    ) -> Result<(), DbErr> {
        users::Entity::update_many()
            .col_expr(users::Column::DisplayName, Expr::value(name))
            .col_expr(users::Column::UpdatedAt, Expr::value(now))
            .filter(users::Column::Id.eq(id))
            .exec(db)
            .await?;
        Ok(())
    }
    pub async fn update_password(
        db: &impl ConnectionTrait,
        id: &str,
        old_hash: &str,
        new_hash: String,
        now: i64,
    ) -> Result<(), DbErr> {
        let updated = users::Entity::update_many()
            .col_expr(users::Column::PasswordHash, Expr::value(new_hash))
            .col_expr(users::Column::UpdatedAt, Expr::value(now))
            .filter(users::Column::Id.eq(id))
            .filter(users::Column::PasswordHash.eq(old_hash))
            .exec(db)
            .await?;
        if updated.rows_affected != 1 {
            return Err(DbErr::Custom("password changed concurrently".into()));
        }
        Ok(())
    }
}
