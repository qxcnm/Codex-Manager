//! Account token metadata repository.
//!
//! Token values are kept behind this adapter and are returned as a domain
//! record.  The repository mirrors the SQLite token refresh schedule fields so
//! service code can migrate incrementally without exposing SeaORM entities.

use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountTokenRecord {
    pub account_id: String,
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub api_key_access_token: Option<String>,
    pub last_refresh: i64,
    pub access_token_exp: Option<i64>,
    pub next_refresh_at: Option<i64>,
    pub last_refresh_attempt_at: Option<i64>,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    account_id: String,
    #[sea_orm(column_type = "Text")]
    id_token: String,
    #[sea_orm(column_type = "Text")]
    access_token: String,
    #[sea_orm(column_type = "Text")]
    refresh_token: String,
    #[sea_orm(column_type = "Text", nullable)]
    api_key_access_token: Option<String>,
    last_refresh: i64,
    access_token_exp: Option<i64>,
    next_refresh_at: Option<i64>,
    last_refresh_attempt_at: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "crate::accounts::Entity",
        from = "Column::AccountId",
        to = "crate::accounts::Column::Id",
        on_delete = "Cascade"
    )]
    Account,
}
impl ActiveModelBehavior for ActiveModel {}

pub struct AccountTokensRepository;

#[cfg(test)]
#[path = "account_token_cas_tests.rs"]
mod cas_tests;

impl AccountTokensRepository {
    /// Match both credentials in the UPDATE so a stale refresh cannot replace
    /// credentials committed by another service instance.
    pub async fn compare_and_swap_token(
        db: &impl ConnectionTrait,
        expected: &codexmanager_core::storage::Token,
        next: &codexmanager_core::storage::Token,
    ) -> Result<bool, DbErr> {
        if expected.account_id != next.account_id {
            return Ok(false);
        }
        // OAuth credentials are case-sensitive even when a MySQL deployment
        // uses a case-insensitive default collation for the tokens table.
        let (access_matches, refresh_matches) =
            if db.get_database_backend() == sea_orm::DbBackend::MySql {
                (
                    Expr::cust_with_values(
                        "BINARY `access_token` = ?",
                        [expected.access_token.clone()],
                    ),
                    Expr::cust_with_values(
                        "BINARY `refresh_token` = ?",
                        [expected.refresh_token.clone()],
                    ),
                )
            } else {
                (
                    Column::AccessToken.eq(&expected.access_token),
                    Column::RefreshToken.eq(&expected.refresh_token),
                )
            };
        let id_matches = if db.get_database_backend() == sea_orm::DbBackend::MySql {
            Expr::cust_with_values("BINARY `id_token` = ?", [expected.id_token.clone()])
        } else {
            Column::IdToken.eq(&expected.id_token)
        };
        let api_key_matches = match &expected.api_key_access_token {
            Some(value) if db.get_database_backend() == sea_orm::DbBackend::MySql => {
                Expr::cust_with_values("BINARY `api_key_access_token` = ?", [value.clone()])
            }
            Some(value) => Column::ApiKeyAccessToken.eq(value),
            None => Column::ApiKeyAccessToken.is_null(),
        };
        let updated = Entity::update_many()
            .col_expr(Column::IdToken, Expr::value(next.id_token.clone()))
            .col_expr(Column::AccessToken, Expr::value(next.access_token.clone()))
            .col_expr(
                Column::RefreshToken,
                Expr::value(next.refresh_token.clone()),
            )
            .col_expr(
                Column::ApiKeyAccessToken,
                Expr::value(next.api_key_access_token.clone()),
            )
            .col_expr(Column::LastRefresh, Expr::value(next.last_refresh))
            .filter(Column::AccountId.eq(&expected.account_id))
            .filter(access_matches)
            .filter(refresh_matches)
            .filter(id_matches)
            .filter(api_key_matches)
            .filter(Column::LastRefresh.eq(expected.last_refresh))
            .exec(db)
            .await?;
        Ok(updated.rows_affected > 0)
    }

    pub async fn get(
        db: &impl ConnectionTrait,
        account_id: &str,
    ) -> Result<Option<AccountTokenRecord>, DbErr> {
        Entity::find_by_id(account_id)
            .one(db)
            .await
            .map(|row| row.map(Into::into))
    }

    pub async fn upsert(db: &impl ConnectionTrait, token: AccountTokenRecord) -> Result<(), DbErr> {
        let model = ActiveModel {
            account_id: Set(token.account_id),
            id_token: Set(token.id_token),
            access_token: Set(token.access_token),
            refresh_token: Set(token.refresh_token),
            api_key_access_token: Set(token.api_key_access_token),
            last_refresh: Set(token.last_refresh),
            access_token_exp: Set(token.access_token_exp),
            next_refresh_at: Set(token.next_refresh_at),
            last_refresh_attempt_at: Set(token.last_refresh_attempt_at),
        };
        Entity::insert(model)
            .on_conflict(
                OnConflict::column(Column::AccountId)
                    .update_columns([
                        Column::IdToken,
                        Column::AccessToken,
                        Column::RefreshToken,
                        Column::ApiKeyAccessToken,
                        Column::LastRefresh,
                        Column::AccessTokenExp,
                        Column::NextRefreshAt,
                        Column::LastRefreshAttemptAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map(|_| ())
    }

    pub async fn update_refresh_schedule(
        db: &impl ConnectionTrait,
        account_id: &str,
        access_token_exp: Option<i64>,
        next_refresh_at: Option<i64>,
    ) -> Result<bool, DbErr> {
        let result = Entity::update_many()
            .col_expr(Column::AccessTokenExp, Expr::value(access_token_exp))
            .col_expr(Column::NextRefreshAt, Expr::value(next_refresh_at))
            .filter(Column::AccountId.eq(account_id))
            .exec(db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    pub async fn touch_refresh_attempt(
        db: &impl ConnectionTrait,
        account_id: &str,
        attempt_at: i64,
    ) -> Result<bool, DbErr> {
        let result = Entity::update_many()
            .col_expr(Column::LastRefreshAttemptAt, Expr::value(attempt_at))
            .filter(Column::AccountId.eq(account_id))
            .exec(db)
            .await?;
        Ok(result.rows_affected > 0)
    }

    /// Return tokens whose scheduled refresh or access expiry is approaching.
    pub async fn list_due(
        db: &impl ConnectionTrait,
        refresh_due_cutoff: i64,
        access_exp_cutoff: i64,
        limit: u64,
    ) -> Result<Vec<AccountTokenRecord>, DbErr> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Entity::find()
            .filter(
                Condition::any()
                    .add(
                        Column::NextRefreshAt
                            .is_null()
                            .or(Column::NextRefreshAt.lte(refresh_due_cutoff)),
                    )
                    .add(
                        Column::AccessTokenExp
                            .is_null()
                            .or(Column::AccessTokenExp.lte(access_exp_cutoff)),
                    ),
            )
            .order_by_asc(Column::NextRefreshAt)
            .order_by_asc(Column::AccountId)
            .limit(limit)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    pub async fn delete(db: &impl ConnectionTrait, account_id: &str) -> Result<bool, DbErr> {
        Entity::delete_by_id(account_id)
            .exec(db)
            .await
            .map(|r| r.rows_affected > 0)
    }
}

impl From<Model> for AccountTokenRecord {
    fn from(model: Model) -> Self {
        Self {
            account_id: model.account_id,
            id_token: model.id_token,
            access_token: model.access_token,
            refresh_token: model.refresh_token,
            api_key_access_token: model.api_key_access_token,
            last_refresh: model.last_refresh,
            access_token_exp: model.access_token_exp,
            next_refresh_at: model.next_refresh_at,
            last_refresh_attempt_at: model.last_refresh_attempt_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, Statement};

    #[tokio::test]
    async fn token_metadata_round_trip_schedule_and_due() {
        let db = Database::connect("sqlite::memory:").await.unwrap();
        db.execute(Statement::from_string(db.get_database_backend(), "CREATE TABLE tokens (account_id VARCHAR(255) PRIMARY KEY, id_token TEXT NOT NULL, access_token TEXT NOT NULL, refresh_token TEXT NOT NULL, api_key_access_token TEXT, last_refresh BIGINT NOT NULL, access_token_exp BIGINT, next_refresh_at BIGINT, last_refresh_attempt_at BIGINT)"))
            .await
            .unwrap();
        let token = AccountTokenRecord {
            account_id: "a1".into(),
            id_token: "id".into(),
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            api_key_access_token: Some("api".into()),
            last_refresh: 1,
            access_token_exp: Some(100),
            next_refresh_at: Some(90),
            last_refresh_attempt_at: None,
        };
        AccountTokensRepository::upsert(&db, token.clone())
            .await
            .unwrap();
        assert_eq!(
            AccountTokensRepository::get(&db, "a1").await.unwrap(),
            Some(token)
        );
        assert_eq!(
            AccountTokensRepository::list_due(&db, 90, 100, 10)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(AccountTokensRepository::touch_refresh_attempt(&db, "a1", 7)
            .await
            .unwrap());
        assert!(
            AccountTokensRepository::update_refresh_schedule(&db, "a1", Some(200), Some(190))
                .await
                .unwrap()
        );
        assert!(AccountTokensRepository::list_due(&db, 100, 150, 10)
            .await
            .unwrap()
            .is_empty());
        assert!(AccountTokensRepository::delete(&db, "a1").await.unwrap());
    }
}
