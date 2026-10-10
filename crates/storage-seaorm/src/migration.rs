//! DDL for the optional adapter only; the desktop SQLite migration chain is unchanged.

use sea_orm::sea_query::{Index, IndexCreateStatement, OnConflict};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Schema, Set, Statement,
};

pub(crate) async fn migrate(db: &DatabaseConnection) -> Result<(), DbErr> {
    crate::desktop_history::migrate(db).await?;
    let backend = db.get_database_backend();
    let schema = Schema::new(backend);
    // Let SeaQuery quote identifiers (notably MySQL's reserved `key`) and
    // select backend-specific types from the private entity definitions.
    for mut table in [
        schema.create_table_from_entity(crate::aggregate_apis::providers::Entity),
        schema.create_table_from_entity(crate::aggregate_apis::suppliers::Entity),
        schema.create_table_from_entity(crate::aggregate_apis::secrets::Entity),
        schema.create_table_from_entity(crate::aggregate_apis::balance_secrets::Entity),
        schema.create_table_from_entity(crate::settings::Entity),
        schema.create_table_from_entity(crate::plugins::installs::Entity),
        schema.create_table_from_entity(crate::plugins::tasks::Entity),
        schema.create_table_from_entity(crate::plugins::logs::Entity),
        schema.create_table_from_entity(crate::proxy_history::proxy_profile_url_test::Entity),
        schema.create_table_from_entity(crate::proxy_history::proxy_speed_test::Entity),
        schema.create_table_from_entity(crate::proxy_history::proxy_diagnostic_test::Entity),
        schema.create_table_from_entity(crate::proxy_history::account_proxy_url_test::Entity),
        schema.create_table_from_entity(crate::accounts::Entity),
        schema.create_table_from_entity(crate::account_details::metadata::Entity),
        schema.create_table_from_entity(crate::account_details::subscriptions::Entity),
        schema.create_table_from_entity(crate::account_details::proxy_settings::Entity),
        schema.create_table_from_entity(crate::account_details::proxy_profiles::Entity),
        schema.create_table_from_entity(crate::account_details::quota_overrides::Entity),
        schema.create_table_from_entity(crate::account_details::agent_identities::Entity),
        schema.create_table_from_entity(crate::account_details::login_sessions::Entity),
        schema.create_table_from_entity(crate::account_details::events::Entity),
        schema.create_table_from_entity(crate::account_details::warmups::Entity),
        schema.create_table_from_entity(crate::account_tokens::Entity),
        schema.create_table_from_entity(crate::api_keys::Entity),
        schema.create_table_from_entity(crate::api_key_details::secrets::Entity),
        schema.create_table_from_entity(crate::api_key_details::quotas::Entity),
        schema.create_table_from_entity(crate::api_key_rollups::hourly::Entity),
        schema.create_table_from_entity(crate::api_key_rollups::legacy::Entity),
        schema.create_table_from_entity(crate::request_token_stats::Entity),
        schema.create_table_from_entity(crate::request_logs::Entity),
        schema.create_table_from_entity(crate::usage_snapshots::Entity),
        schema.create_table_from_entity(crate::reset_credit_operations::Entity),
        schema.create_table_from_entity(crate::reset_credit_operations::pending_accounts::Entity),
        schema.create_table_from_entity(crate::model_catalog::models::Entity),
        schema.create_table_from_entity(crate::model_catalog::prices::Entity),
        schema.create_table_from_entity(crate::model_catalog::price_tiers::Entity),
        schema.create_table_from_entity(crate::model_catalog::routes::Entity),
        schema.create_table_from_entity(crate::model_groups::groups::Entity),
        schema.create_table_from_entity(crate::model_groups::group_models::Entity),
        schema.create_table_from_entity(crate::model_groups::users::Entity),
        schema.create_table_from_entity(crate::model_groups::group_models_v2::Entity),
        schema.create_table_from_entity(crate::users::locks::Entity),
        schema.create_table_from_entity(crate::users::users::Entity),
        schema.create_table_from_entity(crate::users::sessions::Entity),
        schema.create_table_from_entity(crate::users::owners::Entity),
        schema.create_table_from_entity(crate::users::rules::Entity),
        schema.create_table_from_entity(crate::billing::wallets::Entity),
        schema.create_table_from_entity(crate::billing::ledger::Entity),
        schema.create_table_from_entity(crate::billing::snapshots::Entity),
    ] {
        table.if_not_exists();
        db.execute(backend.build(&table)).await?;
    }

    ensure_request_log_clear_column(db).await?;
    ensure_request_stat_legacy_id_column(db).await?;
    ensure_account_preferred_column(db).await?;
    crate::users::initialize_domain_locks(db).await?;
    let mut index = Index::create();
    index
        .name("idx_api_keys_key_hash")
        .table(crate::api_keys::Entity)
        .col(crate::api_keys::Column::KeyHash);
    ensure_index(db, "api_keys", "idx_api_keys_key_hash", index).await?;
    let mut index = Index::create();
    index
        .name("idx_usage_snapshots_account_captured_id")
        .table(crate::usage_snapshots::Entity)
        .col(crate::usage_snapshots::Column::AccountId)
        .col(crate::usage_snapshots::Column::CapturedAt)
        .col(crate::usage_snapshots::Column::Id);
    ensure_index(
        db,
        "usage_snapshots",
        "idx_usage_snapshots_account_captured_id",
        index,
    )
    .await?;
    let mut index = Index::create();
    index
        .name("idx_request_token_stats_created_log")
        .table(crate::request_token_stats::Entity)
        .col(crate::request_token_stats::Column::CreatedAt)
        .col(crate::request_token_stats::Column::RequestLogId);
    ensure_index(
        db,
        "request_token_stats",
        "idx_request_token_stats_created_log",
        index,
    )
    .await?;
    let mut index = Index::create();
    index
        .name("idx_request_logs_created_id")
        .table(crate::request_logs::Entity)
        .col(crate::request_logs::Column::CreatedAt)
        .col(crate::request_logs::Column::Id);
    ensure_index(db, "request_logs", "idx_request_logs_created_id", index).await?;
    let mut index = Index::create();
    index
        .name("idx_reset_credit_operations_account_created")
        .table(crate::reset_credit_operations::Entity)
        .col(crate::reset_credit_operations::Column::AccountId)
        .col(crate::reset_credit_operations::Column::CreatedAt)
        .col(crate::reset_credit_operations::Column::OperationId);
    ensure_index(
        db,
        "reset_credit_operations",
        "idx_reset_credit_operations_account_created",
        index,
    )
    .await?;
    let mut index = Index::create();
    index
        .name("idx_reset_credit_operation_accounts_operation")
        .table(crate::reset_credit_operations::pending_accounts::Entity)
        .col(crate::reset_credit_operations::pending_accounts::Column::OperationId)
        .unique();
    ensure_index(
        db,
        "reset_credit_operation_accounts",
        "idx_reset_credit_operation_accounts_operation",
        index,
    )
    .await?;
    let mut index = Index::create();
    index
        .name("idx_model_groups_status_sort")
        .table(crate::model_groups::groups::Entity)
        .col(crate::model_groups::groups::Column::Status)
        .col(crate::model_groups::groups::Column::Sort);
    ensure_index(db, "model_groups", "idx_model_groups_status_sort", index).await?;
    let mut index = Index::create();
    index
        .name("idx_model_group_models_model")
        .table(crate::model_groups::group_models::Entity)
        .col(crate::model_groups::group_models::Column::PlatformModelSlug)
        .col(crate::model_groups::group_models::Column::Enabled);
    ensure_index(
        db,
        "model_group_models",
        "idx_model_group_models_model",
        index,
    )
    .await?;
    let mut index = Index::create();
    index
        .name("idx_user_model_groups_user_status")
        .table(crate::model_groups::users::Entity)
        .col(crate::model_groups::users::Column::UserId)
        .col(crate::model_groups::users::Column::Status)
        .col(crate::model_groups::users::Column::ExpiresAt);
    ensure_index(
        db,
        "user_model_groups",
        "idx_user_model_groups_user_status",
        index,
    )
    .await?;
    let mut index = Index::create();
    index
        .name("idx_wallet_owner_unique")
        .table(crate::billing::wallets::Entity)
        .col(crate::billing::wallets::Column::OwnerKind)
        .col(crate::billing::wallets::Column::OwnerId)
        .unique();
    ensure_index(db, "app_wallets", "idx_wallet_owner_unique", index).await?;
    let mut index = Index::create();
    index
        .name("idx_ledger_request_kind_unique")
        .table(crate::billing::ledger::Entity)
        .col(crate::billing::ledger::Column::RequestLogId)
        .col(crate::billing::ledger::Column::EntryKind)
        .unique();
    ensure_index(
        db,
        "app_wallet_ledger_entries",
        "idx_ledger_request_kind_unique",
        index,
    )
    .await?;
    backfill_reset_credit_operation_guards(db).await?;
    Ok(())
}

async fn backfill_reset_credit_operation_guards(db: &DatabaseConnection) -> Result<(), DbErr> {
    let operations = crate::reset_credit_operations::Entity::find()
        .filter(
            crate::reset_credit_operations::Column::Status
                .eq(codexmanager_core::storage::ResetCreditOperationStatus::Pending.as_str()),
        )
        .order_by_asc(crate::reset_credit_operations::Column::CreatedAt)
        .order_by_asc(crate::reset_credit_operations::Column::OperationId)
        .all(db)
        .await?;
    for operation in operations {
        crate::reset_credit_operations::pending_accounts::Entity::insert(
            crate::reset_credit_operations::pending_accounts::ActiveModel {
                account_id: Set(operation.account_id),
                operation_id: Set(operation.operation_id),
                created_at: Set(operation.created_at),
            },
        )
        .on_conflict(
            OnConflict::column(crate::reset_credit_operations::pending_accounts::Column::AccountId)
                .do_nothing()
                .to_owned(),
        )
        .exec(db)
        .await?;
    }
    Ok(())
}

async fn ensure_index(
    db: &DatabaseConnection,
    table: &str,
    name: &str,
    mut index: IndexCreateStatement,
) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    if backend == DatabaseBackend::MySql {
        // MySQL does not support CREATE INDEX IF NOT EXISTS. Its catalog is
        // scoped to DATABASE(), so rerunning migration never scans another DB.
        let exists = db
            .query_one(Statement::from_sql_and_values(
                backend,
                "SELECT COUNT(*) AS index_count FROM information_schema.statistics WHERE table_schema = DATABASE() AND table_name = ? AND index_name = ?",
                [table.into(), name.into()],
            ))
            .await?
            .ok_or_else(|| DbErr::Custom("index catalog returned no row".into()))?
            .try_get::<i64>("", "index_count")?;
        if exists == 0 {
            db.execute(backend.build(&index)).await?;
        }
    } else {
        index.if_not_exists();
        db.execute(backend.build(&index)).await?;
    }
    Ok(())
}

async fn ensure_request_log_clear_column(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let exists = match backend {
        DatabaseBackend::Sqlite => db
            .query_all(Statement::from_string(
                backend,
                "PRAGMA table_info(request_logs)".to_string(),
            ))
            .await?
            .iter()
            .any(|row| {
                row.try_get::<String>("", "name")
                    .is_ok_and(|name| name == "cleared_at")
            }),
        DatabaseBackend::MySql | DatabaseBackend::Postgres => {
            let schema = if backend == DatabaseBackend::MySql {
                "DATABASE()"
            } else {
                "current_schema()"
            };
            let row = db.query_one(Statement::from_string(backend, format!("SELECT COUNT(*) AS count FROM information_schema.columns WHERE table_schema={schema} AND table_name='request_logs' AND column_name='cleared_at'"))).await?.ok_or_else(|| DbErr::Custom("column catalog returned no row".into()))?;
            row.try_get::<i64>("", "count")? > 0
        }
    };
    if !exists {
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE request_logs ADD COLUMN cleared_at BIGINT NULL".to_string(),
        ))
        .await?;
    }
    Ok(())
}

async fn ensure_account_preferred_column(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let exists = match backend {
        DatabaseBackend::Sqlite => db
            .query_all(Statement::from_string(
                backend,
                "PRAGMA table_info(accounts)".to_string(),
            ))
            .await?
            .iter()
            .any(|r| {
                r.try_get::<String>("", "name")
                    .is_ok_and(|v| v == "preferred")
            }),
        _ => {
            let schema = if backend == DatabaseBackend::MySql {
                "DATABASE()"
            } else {
                "current_schema()"
            };
            db.query_one(Statement::from_string(backend,format!("SELECT COUNT(*) AS count FROM information_schema.columns WHERE table_schema={schema} AND table_name='accounts' AND column_name='preferred'"))).await?.ok_or_else(||DbErr::Custom("column catalog returned no row".into()))?.try_get::<i64>("","count")?>0
        }
    };
    if !exists {
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE accounts ADD COLUMN preferred BOOLEAN NOT NULL DEFAULT FALSE".to_string(),
        ))
        .await?;
    }
    Ok(())
}

async fn ensure_request_stat_legacy_id_column(db: &DatabaseConnection) -> Result<(), DbErr> {
    let backend = db.get_database_backend();
    let exists = match backend {
        DatabaseBackend::Sqlite => db
            .query_all(Statement::from_string(
                backend,
                "PRAGMA table_info(request_token_stats)".to_string(),
            ))
            .await?
            .iter()
            .any(|row| {
                row.try_get::<String>("", "name")
                    .is_ok_and(|name| name == "id")
            }),
        DatabaseBackend::MySql | DatabaseBackend::Postgres => {
            let schema = if backend == DatabaseBackend::MySql {
                "DATABASE()"
            } else {
                "current_schema()"
            };
            let row = db.query_one(Statement::from_string(backend, format!("SELECT COUNT(*) AS count FROM information_schema.columns WHERE table_schema={schema} AND table_name='request_token_stats' AND column_name='id'"))).await?.ok_or_else(|| DbErr::Custom("column catalog returned no row".into()))?;
            row.try_get::<i64>("", "count")? > 0
        }
    };
    if !exists {
        db.execute(Statement::from_string(
            backend,
            "ALTER TABLE request_token_stats ADD COLUMN id BIGINT NULL".to_string(),
        ))
        .await?;
    }
    Ok(())
}
