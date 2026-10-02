//! Compatibility boundary for request logging; remote storage is authoritative.
use super::list::NormalizedRequestLogParams;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::{RequestLog, RequestLogQuerySummary, RequestTokenStat, Storage};
use codexmanager_storage_seaorm::{RequestLogFilter, RequestLogsRepository};

fn database_error(error: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(error)))
}

pub(crate) fn maintain(storage: &Storage, now: i64) -> rusqlite::Result<()> {
    use std::sync::atomic::{AtomicI64, Ordering};
    static LAST_RUN: AtomicI64 = AtomicI64::new(0);
    if !seaorm_enabled() {
        return storage.maybe_run_observability_maintenance(now);
    }
    let configured = |name: &str, default: i64| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(default)
    };
    let interval = configured("CODEXMANAGER_OBSERVABILITY_MAINTENANCE_INTERVAL_SECS", 900).max(60);
    let previous = LAST_RUN.load(Ordering::Relaxed);
    if previous != 0 && now.saturating_sub(previous) < interval {
        return Ok(());
    }
    if LAST_RUN
        .compare_exchange(previous, now, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
    {
        return Ok(());
    }
    let log_days = configured("CODEXMANAGER_REQUEST_LOG_RETENTION_DAYS", 14);
    let stat_days = configured("CODEXMANAGER_REQUEST_TOKEN_STATS_RETENTION_DAYS", 14);
    let result = seaorm_block_on(move |remote| async move {
        RequestLogsRepository::maintain(remote.connection(), now, log_days, stat_days)
            .await
            .map_err(|err| format!("maintain SeaORM observability failed: {err}"))
    })
    .map_err(database_error);
    if result.is_err() {
        LAST_RUN.store(previous, Ordering::Relaxed);
    }
    result
}

pub(crate) fn insert_log(storage: &Storage, log: &RequestLog) -> rusqlite::Result<i64> {
    if !seaorm_enabled() {
        return storage.insert_request_log(log);
    }
    let stat = RequestTokenStat {
        key_id: log.key_id.clone(),
        account_id: log.account_id.clone(),
        model: log.model.clone(),
        actual_source_kind: log.actual_source_kind.clone(),
        actual_source_id: log.actual_source_id.clone(),
        input_tokens: log.input_tokens,
        cached_input_tokens: log.cached_input_tokens,
        output_tokens: log.output_tokens,
        total_tokens: log.total_tokens,
        reasoning_output_tokens: log.reasoning_output_tokens,
        estimated_cost_usd: log.estimated_cost_usd,
        created_at: log.created_at,
        ..Default::default()
    };
    insert_with_usage(storage, log, &stat).map(|(id, _)| id)
}

pub(crate) fn insert_with_usage(
    storage: &Storage,
    log: &RequestLog,
    stat: &RequestTokenStat,
) -> rusqlite::Result<(i64, Option<String>)> {
    if !seaorm_enabled() {
        return storage.insert_request_log_with_token_stat(log, stat);
    }
    let log = log.clone();
    let stat = stat.clone();
    seaorm_block_on(move |storage| async move {
        RequestLogsRepository::append_with_usage(storage.connection(), log, stat)
            .await
            .map(|id| (id, None))
            .map_err(|err| format!("append SeaORM request log failed: {err}"))
    })
    .map_err(database_error)
}

pub(crate) fn filter(
    params: &NormalizedRequestLogParams,
    keys: Option<&[String]>,
) -> RequestLogFilter {
    RequestLogFilter {
        query: params.query.clone(),
        status: params.status_filter.clone(),
        start_ts: params.start_ts,
        end_ts: params.end_ts,
        key_ids: keys.map(<[String]>::to_vec),
    }
}
pub(crate) fn list(
    filter: RequestLogFilter,
    offset: i64,
    limit: i64,
) -> Result<Vec<RequestLog>, String> {
    seaorm_block_on(move |storage| async move {
        RequestLogsRepository::list_filtered(
            storage.connection(),
            &filter,
            offset.max(0) as u64,
            limit.max(0) as u64,
        )
        .await
        .map_err(|e| format!("list SeaORM request logs failed: {e}"))
    })
}
pub(crate) fn count(filter: RequestLogFilter) -> Result<i64, String> {
    seaorm_block_on(move |storage| async move {
        RequestLogsRepository::count_filtered(storage.connection(), &filter)
            .await
            .map_err(|e| format!("count SeaORM request logs failed: {e}"))
    })
}
pub(crate) fn summary(filter: RequestLogFilter) -> Result<RequestLogQuerySummary, String> {
    seaorm_block_on(move |storage| async move {
        RequestLogsRepository::summarize_filtered(storage.connection(), &filter)
            .await
            .map_err(|e| format!("summarize SeaORM request logs failed: {e}"))
    })
}
