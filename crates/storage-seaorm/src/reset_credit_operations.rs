//! Durable idempotency records for reset-credit consumption.

use codexmanager_core::storage::{
    ResetCreditOperation, ResetCreditOperationClaim, ResetCreditOperationStatus,
    ResetCreditOperationUpdate,
};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{IsolationLevel, QueryFilter, QueryOrder, Set, TransactionTrait};

pub(crate) mod pending_accounts {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "reset_credit_operation_accounts")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub account_id: String,
        pub operation_id: String,
        pub created_at: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "reset_credit_operations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub operation_id: String,
    pub account_id: String,
    pub redeem_request_id: String,
    pub status: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub result_json: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl TryFrom<Model> for ResetCreditOperation {
    type Error = DbErr;

    fn try_from(row: Model) -> Result<Self, Self::Error> {
        let status = ResetCreditOperationStatus::from_storage(&row.status)
            .ok_or_else(|| DbErr::Custom("invalid stored reset credit operation status".into()))?;
        Ok(Self {
            operation_id: row.operation_id,
            account_id: row.account_id,
            redeem_request_id: row.redeem_request_id,
            status,
            result_json: row.result_json,
            error: row.error,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

impl From<ResetCreditOperation> for ActiveModel {
    fn from(operation: ResetCreditOperation) -> Self {
        Self {
            operation_id: Set(operation.operation_id),
            account_id: Set(operation.account_id),
            redeem_request_id: Set(operation.redeem_request_id),
            status: Set(operation.status.as_str().to_owned()),
            result_json: Set(operation.result_json),
            error: Set(operation.error),
            created_at: Set(operation.created_at),
            updated_at: Set(operation.updated_at),
        }
    }
}

fn validate_identifier(name: &str, value: &str) -> Result<(), DbErr> {
    if value.is_empty() || value.trim() != value {
        return Err(DbErr::Custom(format!(
            "{name} must be non-empty and trimmed"
        )));
    }
    Ok(())
}

fn is_retryable_lock_error(error: &DbErr) -> bool {
    let message = format!("{error:?}").to_ascii_lowercase();
    message.contains("deadlock")
        || message.contains("database is locked")
        || message.contains("database is deadlocked")
        || message.contains("sqlite_busy")
}

pub struct ResetCreditOperationsRepository;

impl ResetCreditOperationsRepository {
    pub async fn get(
        db: &impl ConnectionTrait,
        operation_id: &str,
    ) -> Result<Option<ResetCreditOperation>, DbErr> {
        validate_identifier("operationId", operation_id)?;
        Entity::find_by_id(operation_id)
            .one(db)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    pub async fn find_pending(
        db: &impl ConnectionTrait,
        account_id: &str,
    ) -> Result<Option<ResetCreditOperation>, DbErr> {
        validate_identifier("accountId", account_id)?;
        Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .filter(Column::Status.eq(ResetCreditOperationStatus::Pending.as_str()))
            .order_by_asc(Column::CreatedAt)
            .order_by_asc(Column::OperationId)
            .one(db)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    pub async fn claim<C>(
        db: &C,
        operation_id: &str,
        account_id: &str,
        redeem_request_id: &str,
        now: i64,
    ) -> Result<ResetCreditOperationClaim, DbErr>
    where
        C: ConnectionTrait + TransactionTrait,
    {
        // SQLite reports an immediate deadlock when two deferred transactions
        // both read and then try to upgrade to a writer. The guard still
        // serializes the claim; bounded retries let the losing transaction
        // reopen after the winner commits. The same retry is harmless for
        // transient lock/deadlock errors on server backends.
        for _ in 0..32 {
            match Self::claim_once(db, operation_id, account_id, redeem_request_id, now).await {
                Err(error) if is_retryable_lock_error(&error) => {
                    futures_lite::future::yield_now().await;
                }
                result => return result,
            }
        }
        Self::claim_once(db, operation_id, account_id, redeem_request_id, now).await
    }

    async fn claim_once<C>(
        db: &C,
        operation_id: &str,
        account_id: &str,
        redeem_request_id: &str,
        now: i64,
    ) -> Result<ResetCreditOperationClaim, DbErr>
    where
        C: ConnectionTrait + TransactionTrait,
    {
        validate_identifier("operationId", operation_id)?;
        validate_identifier("accountId", account_id)?;
        validate_identifier("redeemRequestId", redeem_request_id)?;
        // Keep the account guard and operation row in one transaction. The
        // account primary key makes competing claims across service
        // processes converge on one pending operation for every backend.
        // Read-committed keeps the post-conflict guard lookup fresh on MySQL,
        // whose default repeatable-read snapshot would otherwise hide the row
        // that won the account-key race while this transaction was open.
        let tx = db
            .begin_with_config(Some(IsolationLevel::ReadCommitted), None)
            .await?;
        if let Some(operation) = Self::get(&tx, operation_id).await? {
            if operation.account_id != account_id {
                tx.commit().await?;
                return Ok(ResetCreditOperationClaim::AccountConflict(operation));
            }
            if operation.status == ResetCreditOperationStatus::Pending {
                if let Some(guard) = pending_accounts::Entity::find_by_id(account_id)
                    .one(&tx)
                    .await?
                {
                    if guard.operation_id != operation_id {
                        let blocking = Self::get(&tx, &guard.operation_id).await?;
                        if let Some(blocking) = blocking.filter(|value| {
                            value.status == ResetCreditOperationStatus::Pending
                                && value.account_id == account_id
                        }) {
                            tx.commit().await?;
                            return Ok(ResetCreditOperationClaim::PendingAccount(blocking));
                        }
                        pending_accounts::Entity::delete_by_id(account_id)
                            .exec(&tx)
                            .await?;
                    }
                }
                let guard_insert =
                    pending_accounts::Entity::insert(pending_accounts::ActiveModel {
                        account_id: Set(account_id.to_owned()),
                        operation_id: Set(operation_id.to_owned()),
                        created_at: Set(operation.created_at),
                    })
                    .on_conflict(
                        OnConflict::column(pending_accounts::Column::AccountId)
                            .do_nothing()
                            .to_owned(),
                    )
                    .exec_without_returning(&tx)
                    .await?;
                if guard_insert == 0 {
                    if let Some(guard) = pending_accounts::Entity::find_by_id(account_id)
                        .one(&tx)
                        .await?
                    {
                        if guard.operation_id != operation_id {
                            if let Some(blocking) =
                                Self::get(&tx, &guard.operation_id).await?.filter(|value| {
                                    value.status == ResetCreditOperationStatus::Pending
                                        && value.account_id == account_id
                                })
                            {
                                tx.commit().await?;
                                return Ok(ResetCreditOperationClaim::PendingAccount(blocking));
                            }
                            pending_accounts::Entity::delete_by_id(account_id)
                                .exec(&tx)
                                .await?;
                        }
                    }
                }
            } else {
                // Self-heal a stale guard left by an interrupted upgrade.
                if let Some(guard) = pending_accounts::Entity::find_by_id(account_id)
                    .one(&tx)
                    .await?
                {
                    if Self::get(&tx, &guard.operation_id)
                        .await?
                        .map_or(true, |value| {
                            value.status != ResetCreditOperationStatus::Pending
                        })
                    {
                        pending_accounts::Entity::delete_by_id(account_id)
                            .exec(&tx)
                            .await?;
                    }
                }
            }
            tx.commit().await?;
            return Ok(ResetCreditOperationClaim::Existing(operation));
        }
        if let Some(operation) = pending_accounts::Entity::find_by_id(account_id)
            .one(&tx)
            .await?
        {
            if let Some(blocking) = Self::get(&tx, &operation.operation_id)
                .await?
                .filter(|value| {
                    value.status == ResetCreditOperationStatus::Pending
                        && value.account_id == account_id
                })
            {
                tx.commit().await?;
                return Ok(ResetCreditOperationClaim::PendingAccount(blocking));
            }
            pending_accounts::Entity::delete_by_id(account_id)
                .exec(&tx)
                .await?;
        }
        /*
         * The guard insert is the cross-process serialization point. The
         * initial read above is only a fast path; a competing transaction may
         * win between that read and this insert, so every conflict is resolved
         * by reading the guard again inside the same transaction.
         */
        let guard = pending_accounts::ActiveModel {
            account_id: Set(account_id.to_owned()),
            operation_id: Set(operation_id.to_owned()),
            created_at: Set(now),
        };
        let guard_insert = pending_accounts::Entity::insert(guard)
            .on_conflict(
                OnConflict::column(pending_accounts::Column::AccountId)
                    .do_nothing()
                    .to_owned(),
            )
            .exec_without_returning(&tx)
            .await?;
        if guard_insert == 0 {
            let Some(guard) = pending_accounts::Entity::find_by_id(account_id)
                .one(&tx)
                .await?
            else {
                return Err(DbErr::Custom(
                    "reset credit account guard disappeared".into(),
                ));
            };
            let Some(blocking) = Self::get(&tx, &guard.operation_id).await? else {
                pending_accounts::Entity::delete_by_id(account_id)
                    .exec(&tx)
                    .await?;
                return Err(DbErr::Custom(
                    "reset credit account guard has no operation".into(),
                ));
            };
            if blocking.status == ResetCreditOperationStatus::Pending
                && blocking.account_id == account_id
            {
                tx.commit().await?;
                return Ok(ResetCreditOperationClaim::PendingAccount(blocking));
            }
            pending_accounts::Entity::delete_by_id(account_id)
                .exec(&tx)
                .await?;
            let retry_guard = pending_accounts::ActiveModel {
                account_id: Set(account_id.to_owned()),
                operation_id: Set(operation_id.to_owned()),
                created_at: Set(now),
            };
            let retry = pending_accounts::Entity::insert(retry_guard)
                .on_conflict(
                    OnConflict::column(pending_accounts::Column::AccountId)
                        .do_nothing()
                        .to_owned(),
                )
                .exec_without_returning(&tx)
                .await?;
            if retry == 0 {
                let Some(guard) = pending_accounts::Entity::find_by_id(account_id)
                    .one(&tx)
                    .await?
                else {
                    return Err(DbErr::Custom(
                        "reset credit account guard disappeared".into(),
                    ));
                };
                let Some(blocking) = Self::get(&tx, &guard.operation_id).await? else {
                    return Err(DbErr::Custom(
                        "reset credit account guard has no operation".into(),
                    ));
                };
                tx.commit().await?;
                return Ok(ResetCreditOperationClaim::PendingAccount(blocking));
            }
        }

        let candidate = ResetCreditOperation {
            operation_id: operation_id.to_owned(),
            account_id: account_id.to_owned(),
            redeem_request_id: redeem_request_id.to_owned(),
            status: ResetCreditOperationStatus::Pending,
            result_json: None,
            error: None,
            created_at: now,
            updated_at: now,
        };
        let active: ActiveModel = candidate.clone().into();
        active.insert(&tx).await?;
        tx.commit().await?;
        Ok(ResetCreditOperationClaim::Created(candidate))
    }

    pub async fn complete<C>(
        db: &C,
        operation_id: &str,
        account_id: &str,
        result_json: &str,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate, DbErr>
    where
        C: ConnectionTrait + TransactionTrait,
    {
        Self::finish(
            db,
            operation_id,
            account_id,
            ResetCreditOperationStatus::Completed,
            Some(result_json),
            None,
            now,
        )
        .await
    }

    pub async fn fail<C>(
        db: &C,
        operation_id: &str,
        account_id: &str,
        error: &str,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate, DbErr>
    where
        C: ConnectionTrait + TransactionTrait,
    {
        Self::finish(
            db,
            operation_id,
            account_id,
            ResetCreditOperationStatus::Failed,
            None,
            Some(error),
            now,
        )
        .await
    }

    pub async fn update_completed_result<C>(
        db: &C,
        operation_id: &str,
        account_id: &str,
        result_json: &str,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate, DbErr>
    where
        C: ConnectionTrait + TransactionTrait,
    {
        validate_identifier("operationId", operation_id)?;
        validate_identifier("accountId", account_id)?;
        let tx = db.begin().await?;
        let update = Entity::update_many()
            .col_expr(Column::ResultJson, Expr::value(result_json))
            .col_expr(Column::UpdatedAt, Expr::value(now))
            .filter(Column::OperationId.eq(operation_id))
            .filter(Column::AccountId.eq(account_id))
            .filter(Column::Status.eq(ResetCreditOperationStatus::Completed.as_str()))
            .exec(&tx)
            .await?;
        let Some(operation) = Self::get(&tx, operation_id).await? else {
            tx.commit().await?;
            return Ok(ResetCreditOperationUpdate::NotFound);
        };
        if operation.account_id != account_id {
            tx.commit().await?;
            Ok(ResetCreditOperationUpdate::AccountConflict(operation))
        } else {
            pending_accounts::Entity::delete_many()
                .filter(pending_accounts::Column::AccountId.eq(account_id))
                .filter(pending_accounts::Column::OperationId.eq(operation_id))
                .exec(&tx)
                .await?;
            tx.commit().await?;
            if update.rows_affected == 1 {
                Ok(ResetCreditOperationUpdate::Updated(operation))
            } else {
                Ok(ResetCreditOperationUpdate::Existing(operation))
            }
        }
    }

    async fn finish<C>(
        db: &C,
        operation_id: &str,
        account_id: &str,
        status: ResetCreditOperationStatus,
        result_json: Option<&str>,
        error: Option<&str>,
        now: i64,
    ) -> Result<ResetCreditOperationUpdate, DbErr>
    where
        C: ConnectionTrait + TransactionTrait,
    {
        validate_identifier("operationId", operation_id)?;
        validate_identifier("accountId", account_id)?;
        let tx = db.begin().await?;
        let update = Entity::update_many()
            .col_expr(Column::Status, Expr::value(status.as_str()))
            .col_expr(
                Column::ResultJson,
                Expr::value(result_json.map(str::to_owned)),
            )
            .col_expr(Column::Error, Expr::value(error.map(str::to_owned)))
            .col_expr(Column::UpdatedAt, Expr::value(now))
            .filter(Column::OperationId.eq(operation_id))
            .filter(Column::AccountId.eq(account_id))
            .filter(Column::Status.eq(ResetCreditOperationStatus::Pending.as_str()))
            .exec(&tx)
            .await?;
        let Some(operation) = Self::get(&tx, operation_id).await? else {
            tx.commit().await?;
            return Ok(ResetCreditOperationUpdate::NotFound);
        };
        if operation.account_id != account_id {
            tx.commit().await?;
            Ok(ResetCreditOperationUpdate::AccountConflict(operation))
        } else {
            pending_accounts::Entity::delete_many()
                .filter(pending_accounts::Column::AccountId.eq(account_id))
                .filter(pending_accounts::Column::OperationId.eq(operation_id))
                .exec(&tx)
                .await?;
            tx.commit().await?;
            if update.rows_affected == 1 {
                Ok(ResetCreditOperationUpdate::Updated(operation))
            } else {
                Ok(ResetCreditOperationUpdate::Existing(operation))
            }
        }
    }
}

#[cfg(test)]
#[path = "reset_credit_operations_tests.rs"]
mod tests;
