//! Atomic raw usage compaction and request-log retention across SQL backends.
use crate::usage_analytics::{OWNER, OWNER_JOINS};
use crate::{
    api_key_rollups::hourly, request_logs, request_token_stats, users::locks,
    RequestLogsRepository, UsersRepository,
};
use sea_orm::{
    entity::prelude::*, ActiveModelTrait, DatabaseConnection, DatabaseTransaction, FromQueryResult,
    QueryOrder, Statement, TransactionTrait,
};

pub(crate) async fn id_floor(tx: &impl ConnectionTrait) -> Result<i64, DbErr> {
    let stored = locks::Entity::find_by_id("request_logs")
        .one(tx)
        .await?
        .ok_or_else(|| DbErr::Custom("request log allocation lock missing".into()))?;
    let max = request_logs::Entity::find()
        .order_by_desc(request_logs::Column::Id)
        .one(tx)
        .await?
        .map(|row| row.id_value())
        .unwrap_or(0);
    let floor = stored.version.max(max);
    if floor != stored.version {
        locks::Entity::update_many()
            .col_expr(locks::Column::Version, Expr::value(floor))
            .filter(locks::Column::Id.eq("request_logs"))
            .exec(tx)
            .await?;
    }
    Ok(floor)
}
pub(crate) async fn allocate_id(tx: &impl ConnectionTrait) -> Result<i64, DbErr> {
    let id = id_floor(tx)
        .await?
        .checked_add(1)
        .ok_or_else(|| DbErr::Custom("request log ID exhausted".into()))?;
    locks::Entity::update_many()
        .col_expr(locks::Column::Version, Expr::value(id))
        .filter(locks::Column::Id.eq("request_logs"))
        .exec(tx)
        .await?;
    Ok(id)
}
fn aligned(ts: i64) -> i64 {
    ts - ts.rem_euclid(3600)
}
pub(crate) async fn compact(tx: &DatabaseTransaction, cutoff: i64, now: i64) -> Result<(), DbErr> {
    let backend = tx.get_database_backend();
    let integer = if backend == sea_orm::DatabaseBackend::MySql {
        "SIGNED"
    } else {
        "BIGINT"
    };
    let float = if backend == sea_orm::DatabaseBackend::Postgres {
        "DOUBLE PRECISION"
    } else {
        "DOUBLE"
    };
    let param = if backend == sea_orm::DatabaseBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    let div = if backend == sea_orm::DatabaseBackend::MySql {
        "(t.created_at DIV 3600)"
    } else {
        "CAST(t.created_at / 3600 AS BIGINT)"
    };
    let fallback =
        "COALESCE(t.input_tokens,0)-COALESCE(t.cached_input_tokens,0)+COALESCE(t.output_tokens,0)";
    let tokens=format!("CASE WHEN t.usage_included<>TRUE THEN 0 WHEN t.total_tokens IS NOT NULL THEN CASE WHEN t.total_tokens>0 THEN t.total_tokens ELSE 0 END WHEN {fallback}>0 THEN {fallback} ELSE 0 END");
    let counters=["input_tokens","cached_input_tokens","output_tokens","reasoning_output_tokens"].iter().map(|field|format!("CAST(COALESCE(SUM(CASE WHEN t.usage_included=TRUE AND t.{field}>0 THEN t.{field} ELSE 0 END),0) AS {integer}) AS {field}")).collect::<Vec<_>>().join(",");
    let sql=format!("SELECT CAST({div}*3600 AS {integer}) AS bucket_start,CAST({div}*3600+3600 AS {integer}) AS bucket_end,COALESCE(NULLIF(TRIM(t.key_id),''),'') AS key_id,COALESCE(NULLIF(TRIM(t.account_id),''),'') AS account_id,COALESCE(NULLIF(TRIM(t.model),''),'') AS model,COALESCE(NULLIF(TRIM(t.actual_source_kind),''),'') AS actual_source_kind,COALESCE(NULLIF(TRIM(t.actual_source_id),''),'') AS actual_source_id,COALESCE({OWNER},'') AS owner_user_id,{counters},CAST(COALESCE(SUM({tokens}),0) AS {integer}) AS total_tokens,CAST(COALESCE(SUM(CASE WHEN t.usage_included=TRUE AND t.estimated_cost_usd>0 THEN t.estimated_cost_usd ELSE 0 END),0) AS {float}) AS estimated_cost_usd,COUNT(*) AS request_count,CAST(COALESCE(SUM(CASE WHEN r.status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END),0) AS {integer}) AS success_count,CAST(COALESCE(SUM(CASE WHEN r.status_code>=400 OR TRIM(COALESCE(r.error,''))<>'' THEN 1 ELSE 0 END),0) AS {integer}) AS error_count,CAST({now} AS {integer}) AS updated_at FROM request_token_stats t LEFT JOIN request_logs r ON r.id=t.request_log_id {OWNER_JOINS} WHERE t.created_at<{param} GROUP BY 1,2,3,4,5,6,7,8");
    let rows = tx
        .query_all(Statement::from_sql_and_values(
            backend,
            sql,
            [cutoff.into()],
        ))
        .await?;
    for row in rows {
        let mut total = hourly::Model::from_query_result(&row, "")?;
        let key = (
            total.bucket_start,
            total.key_id.clone(),
            total.account_id.clone(),
            total.model.clone(),
            total.actual_source_kind.clone(),
            total.actual_source_id.clone(),
            total.owner_user_id.clone(),
        );
        let exists = hourly::Entity::find_by_id(key).one(tx).await?;
        if let Some(old) = &exists {
            total.input_tokens = total.input_tokens.saturating_add(old.input_tokens);
            total.cached_input_tokens = total
                .cached_input_tokens
                .saturating_add(old.cached_input_tokens);
            total.output_tokens = total.output_tokens.saturating_add(old.output_tokens);
            total.total_tokens = total.total_tokens.saturating_add(old.total_tokens);
            total.reasoning_output_tokens = total
                .reasoning_output_tokens
                .saturating_add(old.reasoning_output_tokens);
            total.estimated_cost_usd += old.estimated_cost_usd;
            total.request_count = total.request_count.saturating_add(old.request_count);
            total.success_count = total.success_count.saturating_add(old.success_count);
            total.error_count = total.error_count.saturating_add(old.error_count);
            total.bucket_end = total.bucket_end.max(old.bucket_end);
        }
        let active: hourly::ActiveModel = total.into();
        let active = active.reset_all();
        if exists.is_some() {
            active.update(tx).await?;
        } else {
            active.insert(tx).await?;
        }
    }
    request_token_stats::Entity::delete_many()
        .filter(request_token_stats::Column::CreatedAt.lt(cutoff))
        .exec(tx)
        .await?;
    Ok(())
}
pub(crate) async fn prune(tx: &DatabaseTransaction, cutoff: i64, now: i64) -> Result<(), DbErr> {
    id_floor(tx).await?;
    let backend = tx.get_database_backend();
    let cutoff_param = if backend == sea_orm::DatabaseBackend::Postgres {
        "$1"
    } else {
        "?"
    };
    // Preserve every immutable billing reference, including older ledger rows
    // that predate the charge snapshot schema.
    let billed="EXISTS(SELECT 1 FROM request_charge_snapshots s WHERE s.request_log_id=request_logs.id) OR EXISTS(SELECT 1 FROM app_wallet_ledger_entries l WHERE l.request_log_id=request_logs.id)";
    tx.execute(Statement::from_sql_and_values(backend,format!("UPDATE request_logs SET cleared_at={now} WHERE created_at<{cutoff_param} AND cleared_at IS NULL AND ({billed})"),[cutoff.into()])).await?;
    tx.execute(Statement::from_sql_and_values(
        backend,
        format!("DELETE FROM request_logs WHERE created_at<{cutoff_param} AND NOT ({billed})"),
        [cutoff.into()],
    ))
    .await?;
    Ok(())
}
impl RequestLogsRepository {
    pub async fn maintain(
        db: &DatabaseConnection,
        now: i64,
        log_days: i64,
        stat_days: i64,
    ) -> Result<(), DbErr> {
        let logs =
            (log_days > 0).then(|| aligned(now.saturating_sub(log_days.saturating_mul(86400))));
        let stats =
            (stat_days > 0).then(|| aligned(now.saturating_sub(stat_days.saturating_mul(86400))));
        let Some(cutoff) = logs
            .into_iter()
            .chain(stats)
            .max()
            .filter(|cutoff| *cutoff > 0)
        else {
            return Ok(());
        };
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "request_logs").await?;
        compact(&tx, cutoff, now).await?;
        if let Some(log_cutoff) = logs.filter(|cutoff| *cutoff > 0) {
            prune(&tx, log_cutoff, now).await?;
        }
        tx.commit().await
    }
    pub async fn clear(db: &DatabaseConnection, now: i64) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        UsersRepository::lock(&tx, "request_logs").await?;
        compact(&tx, i64::MAX, now).await?;
        prune(&tx, i64::MAX, now).await?;
        tx.commit().await
    }
}
