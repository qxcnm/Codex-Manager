//! Account routing and application-specific credentials stay independent of Codex tokens.
use crate::{AccountsRepository, UsersRepository};
use codexmanager_core::storage::{now_ts, AccountBpsSettings};
use sea_orm::entity::prelude::*;
use sea_orm::{sea_query::OnConflict, Set, TransactionTrait};

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "account_bps_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub account_id: String,
    pub enabled: bool,
    #[sea_orm(column_type = "Text", nullable)]
    pub access_token: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub refresh_token: Option<String>,
    pub upstream_account_id: Option<String>,
    pub expires_at: Option<i64>,
    pub updated_at: i64,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}

impl From<Model> for AccountBpsSettings {
    fn from(v: Model) -> Self {
        Self {
            account_id: v.account_id,
            enabled: v.enabled,
            access_token: v.access_token,
            refresh_token: v.refresh_token,
            upstream_account_id: v.upstream_account_id,
            expires_at: v.expires_at,
            updated_at: v.updated_at,
        }
    }
}

impl AccountsRepository {
    pub async fn any_account_bps_enabled(db: &impl ConnectionTrait) -> Result<bool, DbErr> {
        Ok(Entity::find()
            .filter(Column::Enabled.eq(true))
            .one(db)
            .await?
            .is_some())
    }
    pub async fn find_account_bps_settings(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<AccountBpsSettings>, DbErr> {
        Ok(Entity::find_by_id(id).one(db).await?.map(Into::into))
    }
    pub async fn list_account_bps_settings(
        db: &impl ConnectionTrait,
        ids: &[String],
    ) -> Result<Vec<AccountBpsSettings>, DbErr> {
        let mut rows = Vec::new();
        for chunk in ids.chunks(500) {
            rows.extend(
                Entity::find()
                    .filter(Column::AccountId.is_in(chunk.to_vec()))
                    .all(db)
                    .await?
                    .into_iter()
                    .map(Into::into),
            );
        }
        Ok(rows)
    }
    pub async fn save_account_bps_credentials(
        db: &DatabaseConnection,
        settings: &AccountBpsSettings,
    ) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "accounts").await?;
        let account = Self::get(&tx, &settings.account_id)
            .await?
            .ok_or_else(|| DbErr::RecordNotFound("account not found".into()))?;
        let expected = account
            .chatgpt_account_id
            .filter(|v| !v.is_empty())
            .or(account.workspace_id.filter(|v| !v.is_empty()));
        if expected.is_none()
            || expected != settings.upstream_account_id
            || !settings.authorized_at(now_ts())
        {
            return Err(DbErr::Custom(
                "BPS credentials do not match this account or are expired".into(),
            ));
        }
        Entity::insert(ActiveModel {
            account_id: Set(settings.account_id.clone()),
            enabled: Set(false),
            access_token: Set(settings.access_token.clone()),
            refresh_token: Set(settings.refresh_token.clone()),
            upstream_account_id: Set(settings.upstream_account_id.clone()),
            expires_at: Set(settings.expires_at),
            updated_at: Set(now_ts()),
        })
        .on_conflict(
            OnConflict::column(Column::AccountId)
                .update_columns([
                    Column::AccessToken,
                    Column::RefreshToken,
                    Column::UpstreamAccountId,
                    Column::ExpiresAt,
                    Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(&tx)
        .await?;
        tx.commit().await
    }
    pub async fn set_account_bps_enabled(
        db: &DatabaseConnection,
        ids: &[String],
        enabled: bool,
    ) -> Result<usize, DbErr> {
        if ids.is_empty() || ids.iter().any(|id| id.is_empty() || id.trim() != id) {
            return Err(DbErr::Custom(
                "accountIds must contain non-empty account IDs".into(),
            ));
        }
        let ids = ids.iter().collect::<std::collections::BTreeSet<_>>();
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "accounts").await?;
        for id in &ids {
            let account = Self::get(&tx, id)
                .await?
                .ok_or_else(|| DbErr::RecordNotFound("account not found".into()))?;
            if enabled {
                let expected = account
                    .chatgpt_account_id
                    .filter(|v| !v.is_empty())
                    .or(account.workspace_id.filter(|v| !v.is_empty()));
                if !Self::find_account_bps_settings(&tx, id)
                    .await?
                    .is_some_and(|s| {
                        s.authorized_at(now_ts())
                            && expected.is_some()
                            && expected == s.upstream_account_id
                    })
                {
                    return Err(DbErr::Custom(
                        "BPS authorization is required for every selected account".into(),
                    ));
                }
            }
        }
        for id in &ids {
            Entity::insert(ActiveModel {
                account_id: Set((*id).clone()),
                enabled: Set(enabled),
                access_token: Set(None),
                refresh_token: Set(None),
                upstream_account_id: Set(None),
                expires_at: Set(None),
                updated_at: Set(now_ts()),
            })
            .on_conflict(
                OnConflict::column(Column::AccountId)
                    .update_columns([Column::Enabled, Column::UpdatedAt])
                    .to_owned(),
            )
            .exec(&tx)
            .await?;
        }
        tx.commit().await?;
        Ok(ids.len())
    }
}

#[cfg(all(test, feature = "sqlite"))]
#[path = "accounts_bps_tests.rs"]
mod tests;
