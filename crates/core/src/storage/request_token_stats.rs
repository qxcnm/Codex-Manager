use rusqlite::{params, params_from_iter, types::Value, Result, Row};
use std::sync::atomic::{AtomicI64, Ordering};

use super::key_id_filters::{PairedKeyIdSqlFilter, TempKeyIdFilter};
use super::{
    now_ts, ApiKeyModelTokenUsageSummary, ApiKeyTokenUsageSummary, DailyTokenUsageRollup,
    MemberDashboardUsageBreakdownSnapshot, RequestLogQuerySummary, RequestLogTodaySummary,
    RequestTokenStat, SourceTokenUsageRollup, Storage, TokenUsageRollup, TokenUsageSummary,
    UserTokenUsageRollup,
};

const DEFAULT_REQUEST_TOKEN_STATS_RETAIN_DAYS: i64 = 14;
const DEFAULT_OBSERVABILITY_MAINTENANCE_INTERVAL_SECS: i64 = 900;
const HOUR_SECONDS: i64 = 3_600;
const REQUEST_TOKEN_STATS_RETAIN_DAYS_ENV: &str = "CODEXMANAGER_REQUEST_TOKEN_STATS_RETENTION_DAYS";
const OBSERVABILITY_MAINTENANCE_INTERVAL_SECS_ENV: &str =
    "CODEXMANAGER_OBSERVABILITY_MAINTENANCE_INTERVAL_SECS";

static LAST_OBSERVABILITY_MAINTENANCE_AT: AtomicI64 = AtomicI64::new(0);

pub(super) fn request_token_stats_retain_days() -> i64 {
    std::env::var(REQUEST_TOKEN_STATS_RETAIN_DAYS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_REQUEST_TOKEN_STATS_RETAIN_DAYS)
}

fn observability_maintenance_interval_secs() -> i64 {
    std::env::var(OBSERVABILITY_MAINTENANCE_INTERVAL_SECS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_OBSERVABILITY_MAINTENANCE_INTERVAL_SECS)
}

pub(super) fn retention_cutoff(now: i64, days: i64) -> Option<i64> {
    (days > 0).then(|| now.saturating_sub(days.saturating_mul(86_400)))
}

fn token_total_sql_expr() -> &'static str {
    "CASE
        WHEN total_tokens IS NOT NULL THEN
            CASE WHEN total_tokens > 0 THEN total_tokens ELSE 0 END
        ELSE
            CASE
                WHEN IFNULL(input_tokens, 0) - IFNULL(cached_input_tokens, 0) + IFNULL(output_tokens, 0) > 0
                    THEN IFNULL(input_tokens, 0) - IFNULL(cached_input_tokens, 0) + IFNULL(output_tokens, 0)
                ELSE 0
            END
     END"
}

const TOKEN_ROLLUP_COLUMNS: &str = "
    IFNULL(SUM(IFNULL(t.input_tokens, 0)), 0) AS input_tokens,
    IFNULL(SUM(IFNULL(t.cached_input_tokens, 0)), 0) AS cached_input_tokens,
    IFNULL(SUM(IFNULL(t.output_tokens, 0)), 0) AS output_tokens,
    IFNULL(SUM(IFNULL(t.reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
    IFNULL(
        SUM(
            CASE
                WHEN t.total_tokens IS NOT NULL THEN
                    CASE WHEN t.total_tokens > 0 THEN t.total_tokens ELSE 0 END
                ELSE
                    CASE
                        WHEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0) + IFNULL(t.output_tokens, 0) > 0
                            THEN IFNULL(t.input_tokens, 0) - IFNULL(t.cached_input_tokens, 0) + IFNULL(t.output_tokens, 0)
                        ELSE 0
                    END
            END
        ),
        0
    ) AS total_tokens,
    IFNULL(SUM(IFNULL(t.estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd,
    COUNT(DISTINCT t.request_log_id) AS request_count,
    COUNT(DISTINCT CASE WHEN r.status_code >= 200 AND r.status_code <= 299 THEN t.request_log_id END) AS success_count,
    COUNT(DISTINCT CASE WHEN IFNULL(r.status_code, 0) >= 400 OR TRIM(IFNULL(r.error, '')) <> '' THEN t.request_log_id END) AS error_count";

const HOURLY_ROLLUP_COLUMNS: &str = "
    IFNULL(SUM(IFNULL(h.input_tokens, 0)), 0) AS input_tokens,
    IFNULL(SUM(IFNULL(h.cached_input_tokens, 0)), 0) AS cached_input_tokens,
    IFNULL(SUM(IFNULL(h.output_tokens, 0)), 0) AS output_tokens,
    IFNULL(SUM(IFNULL(h.reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
    IFNULL(SUM(IFNULL(h.total_tokens, 0)), 0) AS total_tokens,
    IFNULL(SUM(IFNULL(h.estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd,
    IFNULL(SUM(IFNULL(h.request_count, 0)), 0) AS request_count,
    IFNULL(SUM(IFNULL(h.success_count, 0)), 0) AS success_count,
    IFNULL(SUM(IFNULL(h.error_count, 0)), 0) AS error_count";

const COMBINED_ROLLUP_COLUMNS: &str = "
    IFNULL(SUM(IFNULL(input_tokens, 0)), 0) AS input_tokens,
    IFNULL(SUM(IFNULL(cached_input_tokens, 0)), 0) AS cached_input_tokens,
    IFNULL(SUM(IFNULL(output_tokens, 0)), 0) AS output_tokens,
    IFNULL(SUM(IFNULL(reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
    IFNULL(SUM(IFNULL(total_tokens, 0)), 0) AS total_tokens,
    IFNULL(SUM(IFNULL(estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd,
    IFNULL(SUM(IFNULL(request_count, 0)), 0) AS request_count,
    IFNULL(SUM(IFNULL(success_count, 0)), 0) AS success_count,
    IFNULL(SUM(IFNULL(error_count, 0)), 0) AS error_count";

const USER_OWNER_EXPR: &str = "COALESCE(
    (
        SELECT MIN(NULLIF(TRIM(w.owner_id), ''))
        FROM app_wallet_ledger_entries l
        JOIN app_wallets w ON w.id = l.wallet_id
        WHERE l.request_log_id = t.request_log_id
          AND l.entry_kind = 'request_charge'
          AND w.owner_kind = 'user'
    ),
    NULLIF(TRIM(owner.owner_user_id), ''),
    NULLIF(TRIM(stat_owner.owner_user_id), '')
)";

// User attribution prefers the request_charge wallet owner. Use a correlated
// lookup so dashboard range queries do not pre-aggregate the entire ledger table.
// The api_key_owners fallback is current-owner based, so old uncharged logs are
// approximate.
const USER_OWNER_JOINS: &str = "
    LEFT JOIN api_key_owners owner ON owner.key_id = r.key_id AND owner.owner_kind = 'user'
    LEFT JOIN api_key_owners stat_owner ON stat_owner.key_id = t.key_id AND stat_owner.owner_kind = 'user'";

fn token_usage_rollup_from_row(row: &Row<'_>, offset: usize) -> Result<TokenUsageRollup> {
    Ok(TokenUsageRollup {
        input_tokens: row.get::<_, i64>(offset)?.max(0),
        cached_input_tokens: row.get::<_, i64>(offset + 1)?.max(0),
        output_tokens: row.get::<_, i64>(offset + 2)?.max(0),
        reasoning_output_tokens: row.get::<_, i64>(offset + 3)?.max(0),
        total_tokens: row.get::<_, i64>(offset + 4)?.max(0),
        estimated_cost_usd: row.get::<_, f64>(offset + 5)?.max(0.0),
        request_count: row.get::<_, i64>(offset + 6)?.max(0),
        success_count: row.get::<_, i64>(offset + 7)?.max(0),
        error_count: row.get::<_, i64>(offset + 8)?.max(0),
    })
}

fn request_log_query_summary_from_usage(usage: TokenUsageRollup) -> RequestLogQuerySummary {
    RequestLogQuerySummary {
        count: usage.request_count,
        success_count: usage.success_count,
        error_count: usage.error_count,
        total_tokens: usage.total_tokens,
        estimated_cost_usd: usage.estimated_cost_usd,
    }
}

fn empty_request_log_today_summary() -> RequestLogTodaySummary {
    RequestLogTodaySummary {
        input_tokens: 0,
        cached_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        estimated_cost_usd: 0.0,
    }
}

fn request_log_today_summary_from_row(row: &Row<'_>) -> Result<RequestLogTodaySummary> {
    Ok(RequestLogTodaySummary {
        input_tokens: row.get(0)?,
        cached_input_tokens: row.get(1)?,
        output_tokens: row.get(2)?,
        reasoning_output_tokens: row.get(3)?,
        estimated_cost_usd: row.get(4)?,
    })
}

pub(super) fn request_token_stats_pending_rollup_exists_sql() -> &'static str {
    "SELECT EXISTS(SELECT 1 FROM request_token_stats WHERE created_at < ?1)"
}

pub(super) fn delete_request_token_stats_before_sql() -> &'static str {
    "DELETE FROM request_token_stats WHERE created_at < ?1"
}

fn request_log_today_summary_sql(
    raw_key_condition: Option<&str>,
    hourly_key_condition: Option<&str>,
) -> String {
    let raw_key_clause = raw_key_condition
        .map(|condition| format!(" AND {condition}"))
        .unwrap_or_default();
    let hourly_key_clause = hourly_key_condition
        .map(|condition| format!(" AND {condition}"))
        .unwrap_or_default();

    format!(
        "WITH combined AS (
            SELECT
                IFNULL(SUM(IFNULL(s.input_tokens, 0)), 0) AS input_tokens,
                IFNULL(SUM(IFNULL(s.cached_input_tokens, 0)), 0) AS cached_input_tokens,
                IFNULL(SUM(IFNULL(s.output_tokens, 0)), 0) AS output_tokens,
                IFNULL(SUM(IFNULL(s.reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
                IFNULL(SUM(IFNULL(s.estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd
            FROM request_token_stats s
            WHERE s.created_at >= ? AND s.created_at < ?{raw_key_clause}
            UNION ALL
            SELECT
                IFNULL(SUM(IFNULL(h.input_tokens, 0)), 0) AS input_tokens,
                IFNULL(SUM(IFNULL(h.cached_input_tokens, 0)), 0) AS cached_input_tokens,
                IFNULL(SUM(IFNULL(h.output_tokens, 0)), 0) AS output_tokens,
                IFNULL(SUM(IFNULL(h.reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
                IFNULL(SUM(IFNULL(h.estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd
            FROM request_token_stat_hourly_rollups h
            WHERE h.bucket_start >= ? AND h.bucket_end <= ?{hourly_key_clause}
         )
         SELECT
            IFNULL(SUM(input_tokens), 0),
            IFNULL(SUM(cached_input_tokens), 0),
            IFNULL(SUM(output_tokens), 0),
            IFNULL(SUM(reasoning_output_tokens), 0),
            IFNULL(SUM(estimated_cost_usd), 0.0)
         FROM combined"
    )
}

fn source_id_expr(source_kind: &str) -> Option<&'static str> {
    match source_kind {
        "openai_account" => Some(
            // Prefer actual_source_* written by routing. Legacy account_id is only
            // used when actual source metadata was not captured.
            "CASE
                WHEN t.actual_source_kind = 'openai_account'
                    THEN COALESCE(NULLIF(TRIM(t.actual_source_id), ''), NULLIF(TRIM(t.account_id), ''))
                WHEN r.actual_source_kind = 'openai_account'
                    THEN COALESCE(NULLIF(TRIM(r.actual_source_id), ''), NULLIF(TRIM(r.account_id), ''), NULLIF(TRIM(t.account_id), ''))
                WHEN (t.actual_source_kind IS NULL OR TRIM(t.actual_source_kind) = '')
                    AND (r.actual_source_kind IS NULL OR TRIM(r.actual_source_kind) = '')
                    THEN COALESCE(NULLIF(TRIM(r.account_id), ''), NULLIF(TRIM(t.account_id), ''))
                ELSE NULL
             END",
        ),
        "aggregate_api" => Some(
            // Prefer actual_source_* written by routing. Legacy aggregate API
            // context is only used when actual source metadata was not captured.
            "CASE
                WHEN t.actual_source_kind = 'aggregate_api'
                    THEN NULLIF(TRIM(t.actual_source_id), '')
                WHEN r.actual_source_kind = 'aggregate_api'
                    THEN COALESCE(NULLIF(TRIM(r.actual_source_id), ''), NULLIF(TRIM(r.initial_aggregate_api_id), ''))
                WHEN (t.actual_source_kind IS NULL OR TRIM(t.actual_source_kind) = '')
                    AND (r.actual_source_kind IS NULL OR TRIM(r.actual_source_kind) = '')
                    THEN NULLIF(TRIM(r.initial_aggregate_api_id), '')
                ELSE NULL
             END",
        ),
        _ => None,
    }
}

fn hourly_source_id_expr(source_kind: &str) -> Option<&'static str> {
    match source_kind {
        "openai_account" => Some(
            "CASE
                WHEN h.actual_source_kind = 'openai_account'
                    THEN COALESCE(NULLIF(TRIM(h.actual_source_id), ''), NULLIF(TRIM(h.account_id), ''))
                WHEN TRIM(IFNULL(h.actual_source_kind, '')) = ''
                    THEN NULLIF(TRIM(h.account_id), '')
                ELSE NULL
             END",
        ),
        "aggregate_api" => Some(
            "CASE
                WHEN h.actual_source_kind = 'aggregate_api'
                    THEN NULLIF(TRIM(h.actual_source_id), '')
                ELSE NULL
             END",
        ),
        _ => None,
    }
}

fn hourly_rollup_range_clause() -> &'static str {
    "h.bucket_start >= ?1 AND h.bucket_end <= ?2"
}

fn sql_limit_clause(limit: Option<usize>) -> String {
    limit
        .map(|value| format!("\n             LIMIT {}", value))
        .unwrap_or_default()
}

fn union_all_selects(selects: impl IntoIterator<Item = String>) -> String {
    selects
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n                UNION ALL\n                ")
}

struct KeyFilterSqlClauses {
    raw: String,
    hourly: String,
    legacy: String,
    outer: String,
}

fn key_filter_sql_clauses(key_filter: Option<&TempKeyIdFilter<'_>>) -> KeyFilterSqlClauses {
    KeyFilterSqlClauses {
        raw: key_filter
            .map(|filter| filter.exists_clause("t.key_id"))
            .unwrap_or_default(),
        hourly: key_filter
            .map(|filter| filter.exists_clause("NULLIF(TRIM(h.key_id), '')"))
            .unwrap_or_default(),
        legacy: key_filter
            .map(|filter| filter.exists_clause("NULLIF(TRIM(r.key_id), '')"))
            .unwrap_or_default(),
        outer: key_filter
            .map(|filter| filter.exists_clause("s.key_id"))
            .unwrap_or_default(),
    }
}

fn request_token_stats_by_key_sql(
    combined_selects: &str,
    key_filter_clauses: &KeyFilterSqlClauses,
) -> String {
    format!(
        "WITH combined AS (
            {combined_selects}
         )
         SELECT
            s.key_id,
            IFNULL(SUM(IFNULL(s.total_tokens, 0)), 0) AS total_tokens,
            IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
         FROM combined s
         WHERE s.key_id IS NOT NULL AND TRIM(s.key_id) <> ''{outer_key_filter_clause}
         GROUP BY s.key_id
         ORDER BY total_tokens DESC, s.key_id ASC",
        outer_key_filter_clause = key_filter_clauses.outer,
    )
}

fn raw_token_rollup_select(
    select_prefix: &str,
    where_clause: &str,
    group_by: &str,
    include_owner_joins: bool,
) -> String {
    let owner_joins = include_owner_joins
        .then_some(USER_OWNER_JOINS)
        .unwrap_or("");
    format!(
        "SELECT
            {select_prefix}
            {TOKEN_ROLLUP_COLUMNS}
         FROM request_token_stats t
         LEFT JOIN request_logs r ON r.id = t.request_log_id
         {owner_joins}
         WHERE {where_clause}
         {group_by}"
    )
}

fn hourly_token_rollup_select(select_prefix: &str, where_clause: &str, group_by: &str) -> String {
    format!(
        "SELECT
            {select_prefix}
            {HOURLY_ROLLUP_COLUMNS}
         FROM request_token_stat_hourly_rollups h
         WHERE {where_clause}
         {group_by}"
    )
}

fn raw_key_usage_select(select_prefix: &str, where_clause: &str, group_by: &str) -> String {
    format!(
        "SELECT
            {select_prefix}
            t.key_id,
            COALESCE(NULLIF(TRIM(t.model), ''), 'unknown') AS normalized_model,
            IFNULL(SUM(IFNULL(t.input_tokens, 0)), 0) AS input_tokens,
            IFNULL(SUM(IFNULL(t.cached_input_tokens, 0)), 0) AS cached_input_tokens,
            IFNULL(SUM(IFNULL(t.output_tokens, 0)), 0) AS output_tokens,
            IFNULL(SUM(IFNULL(t.reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
            IFNULL(SUM({token_total}), 0) AS total_tokens,
            IFNULL(SUM(IFNULL(t.estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd
         FROM request_token_stats t
         WHERE {where_clause}
         {group_by}",
        token_total = token_total_sql_expr(),
    )
}

fn hourly_key_usage_select(select_prefix: &str, where_clause: &str, group_by: &str) -> String {
    format!(
        "SELECT
            {select_prefix}
            NULLIF(TRIM(h.key_id), '') AS key_id,
            COALESCE(NULLIF(TRIM(h.model), ''), 'unknown') AS normalized_model,
            IFNULL(SUM(IFNULL(h.input_tokens, 0)), 0) AS input_tokens,
            IFNULL(SUM(IFNULL(h.cached_input_tokens, 0)), 0) AS cached_input_tokens,
            IFNULL(SUM(IFNULL(h.output_tokens, 0)), 0) AS output_tokens,
            IFNULL(SUM(IFNULL(h.reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
            IFNULL(SUM(IFNULL(h.total_tokens, 0)), 0) AS total_tokens,
            IFNULL(SUM(IFNULL(h.estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd
         FROM request_token_stat_hourly_rollups h
         WHERE {where_clause}
         {group_by}"
    )
}

fn legacy_key_usage_select(select_prefix: &str, where_clause: &str, group_by: &str) -> String {
    format!(
        "SELECT
            {select_prefix}
            NULLIF(TRIM(r.key_id), '') AS key_id,
            COALESCE(NULLIF(TRIM(r.model), ''), 'unknown') AS normalized_model,
            IFNULL(SUM(IFNULL(r.input_tokens, 0)), 0) AS input_tokens,
            IFNULL(SUM(IFNULL(r.cached_input_tokens, 0)), 0) AS cached_input_tokens,
            IFNULL(SUM(IFNULL(r.output_tokens, 0)), 0) AS output_tokens,
            IFNULL(SUM(IFNULL(r.reasoning_output_tokens, 0)), 0) AS reasoning_output_tokens,
            IFNULL(SUM(IFNULL(r.total_tokens, 0)), 0) AS total_tokens,
            IFNULL(SUM(IFNULL(r.estimated_cost_usd, 0.0)), 0.0) AS estimated_cost_usd
         FROM request_token_stat_rollups r
         WHERE {where_clause}
         {group_by}"
    )
}

fn optional_raw_stats_range_clause(start_ts: Option<i64>, end_ts: Option<i64>) -> &'static str {
    match (start_ts, end_ts) {
        (None, None) => "1 = 1",
        _ => "(?1 IS NULL OR t.created_at >= ?1) AND (?2 IS NULL OR t.created_at < ?2)",
    }
}

fn optional_hourly_rollup_range_clause(start_ts: Option<i64>, end_ts: Option<i64>) -> &'static str {
    match (start_ts, end_ts) {
        (None, None) => "1 = 1",
        _ => "(?1 IS NULL OR h.bucket_start >= ?1) AND (?2 IS NULL OR h.bucket_end <= ?2)",
    }
}

fn optional_range_params(start_ts: Option<i64>, end_ts: Option<i64>) -> Option<[Value; 2]> {
    if start_ts.is_none() && end_ts.is_none() {
        None
    } else {
        Some([
            start_ts.map(Value::Integer).unwrap_or(Value::Null),
            end_ts.map(Value::Integer).unwrap_or(Value::Null),
        ])
    }
}

fn is_empty_optional_range(start_ts: Option<i64>, end_ts: Option<i64>) -> bool {
    matches!((start_ts, end_ts), (Some(start), Some(end)) if end <= start)
}

fn map_api_key_token_usage_summary(row: &Row<'_>) -> Result<ApiKeyTokenUsageSummary> {
    Ok(ApiKeyTokenUsageSummary {
        key_id: row.get(0)?,
        total_tokens: row.get(1)?,
        estimated_cost_usd: row.get(2)?,
    })
}

fn map_token_usage_summary(row: &Row<'_>) -> Result<TokenUsageSummary> {
    Ok(TokenUsageSummary {
        model: row.get(0)?,
        input_tokens: row.get::<_, i64>(1)?.max(0),
        cached_input_tokens: row.get::<_, i64>(2)?.max(0),
        output_tokens: row.get::<_, i64>(3)?.max(0),
        reasoning_output_tokens: row.get::<_, i64>(4)?.max(0),
        total_tokens: row.get::<_, i64>(5)?.max(0),
        estimated_cost_usd: row.get::<_, f64>(6)?.max(0.0),
    })
}

fn map_api_key_model_token_usage_summary(row: &Row<'_>) -> Result<ApiKeyModelTokenUsageSummary> {
    Ok(ApiKeyModelTokenUsageSummary {
        key_id: row.get(0)?,
        model: row.get(1)?,
        input_tokens: row.get::<_, i64>(2)?.max(0),
        cached_input_tokens: row.get::<_, i64>(3)?.max(0),
        output_tokens: row.get::<_, i64>(4)?.max(0),
        reasoning_output_tokens: row.get::<_, i64>(5)?.max(0),
        total_tokens: row.get::<_, i64>(6)?.max(0),
        estimated_cost_usd: row.get::<_, f64>(7)?.max(0.0),
    })
}

impl Storage {
    /// 函数 `insert_request_token_stat`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - stat: 参数 stat
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_request_token_stat(&self, stat: &RequestTokenStat) -> Result<()> {
        self.conn.execute(
            "INSERT INTO request_token_stats (
                request_log_id, key_id, account_id, model, actual_source_kind, actual_source_id,
                input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                estimated_cost_usd, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            (
                stat.request_log_id,
                &stat.key_id,
                &stat.account_id,
                &stat.model,
                &stat.actual_source_kind,
                &stat.actual_source_id,
                stat.input_tokens,
                stat.cached_input_tokens,
                stat.output_tokens,
                stat.total_tokens,
                stat.reasoning_output_tokens,
                stat.estimated_cost_usd,
                stat.created_at,
            ),
        )?;
        Ok(())
    }

    pub fn maybe_run_observability_maintenance(&self, now: i64) -> Result<()> {
        let interval = observability_maintenance_interval_secs().max(60);
        let last = LAST_OBSERVABILITY_MAINTENANCE_AT.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < interval {
            return Ok(());
        }
        if LAST_OBSERVABILITY_MAINTENANCE_AT
            .compare_exchange(last, now, Ordering::SeqCst, Ordering::Relaxed)
            .is_err()
        {
            return Ok(());
        }

        if let Err(err) = self.prune_observability_history(now) {
            LAST_OBSERVABILITY_MAINTENANCE_AT.store(last, Ordering::Relaxed);
            return Err(err);
        }
        Ok(())
    }

    pub fn prune_observability_history(&self, now: i64) -> Result<()> {
        let mut touched = 0_usize;
        if let Some(cutoff) = retention_cutoff(now, request_token_stats_retain_days()) {
            touched = touched.saturating_add(self.rollup_request_token_stats_before(cutoff)?);
        }
        touched = touched.saturating_add(self.prune_request_logs_by_retention(now)?);
        if touched > 0 {
            let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
        }
        Ok(())
    }

    pub fn rollup_all_request_token_stats(&self) -> Result<usize> {
        self.rollup_request_token_stats_before(i64::MAX)
    }

    pub fn rollup_request_token_stats_before(&self, cutoff_ts: i64) -> Result<usize> {
        let cutoff_ts = cutoff_ts - cutoff_ts.rem_euclid(HOUR_SECONDS);
        if cutoff_ts <= 0 {
            return Ok(0);
        }
        let pending_count: i64 = self.conn.query_row(
            request_token_stats_pending_rollup_exists_sql(),
            [cutoff_ts],
            |row| row.get(0),
        )?;
        if pending_count == 0 {
            return Ok(0);
        }
        let now = now_ts();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!(
                "INSERT INTO request_token_stat_hourly_rollups (
                    bucket_start, bucket_end, key_id, account_id, model, actual_source_kind, actual_source_id,
                    owner_user_id, input_tokens, cached_input_tokens, output_tokens, total_tokens,
                    reasoning_output_tokens, estimated_cost_usd, request_count, success_count,
                    error_count, updated_at
                 )
                 SELECT
                    CAST(t.created_at / {HOUR_SECONDS} AS INTEGER) * {HOUR_SECONDS},
                    CAST(t.created_at / {HOUR_SECONDS} AS INTEGER) * {HOUR_SECONDS} + {HOUR_SECONDS},
                    COALESCE(NULLIF(TRIM(t.key_id), ''), ''),
                    COALESCE(NULLIF(TRIM(t.account_id), ''), ''),
                    COALESCE(NULLIF(TRIM(t.model), ''), ''),
                    COALESCE(NULLIF(TRIM(t.actual_source_kind), ''), ''),
                    COALESCE(NULLIF(TRIM(t.actual_source_id), ''), ''),
                    COALESCE({USER_OWNER_EXPR}, ''),
                    IFNULL(SUM(CASE WHEN t.input_tokens > 0 THEN t.input_tokens ELSE 0 END), 0),
                    IFNULL(SUM(CASE WHEN t.cached_input_tokens > 0 THEN t.cached_input_tokens ELSE 0 END), 0),
                    IFNULL(SUM(CASE WHEN t.output_tokens > 0 THEN t.output_tokens ELSE 0 END), 0),
                    IFNULL(SUM({token_total}), 0),
                    IFNULL(SUM(CASE WHEN t.reasoning_output_tokens > 0 THEN t.reasoning_output_tokens ELSE 0 END), 0),
                    IFNULL(SUM(CASE WHEN t.estimated_cost_usd > 0 THEN t.estimated_cost_usd ELSE 0 END), 0.0),
                    COUNT(DISTINCT t.request_log_id),
                    COUNT(DISTINCT CASE WHEN r.status_code >= 200 AND r.status_code <= 299 THEN t.request_log_id END),
                    COUNT(DISTINCT CASE WHEN IFNULL(r.status_code, 0) >= 400 OR TRIM(IFNULL(r.error, '')) <> '' THEN t.request_log_id END),
                    ?2
                 FROM request_token_stats t
                 LEFT JOIN request_logs r ON r.id = t.request_log_id
                 {USER_OWNER_JOINS}
                 WHERE t.created_at < ?1
                 GROUP BY
                    CAST(t.created_at / {HOUR_SECONDS} AS INTEGER) * {HOUR_SECONDS},
                    COALESCE(NULLIF(TRIM(t.key_id), ''), ''),
                    COALESCE(NULLIF(TRIM(t.account_id), ''), ''),
                    COALESCE(NULLIF(TRIM(t.model), ''), ''),
                    COALESCE(NULLIF(TRIM(t.actual_source_kind), ''), ''),
                    COALESCE(NULLIF(TRIM(t.actual_source_id), ''), ''),
                    COALESCE({USER_OWNER_EXPR}, '')
                 ON CONFLICT(bucket_start, key_id, account_id, model, actual_source_kind, actual_source_id, owner_user_id)
                 DO UPDATE SET
                    bucket_end = CASE
                        WHEN request_token_stat_hourly_rollups.bucket_end > excluded.bucket_end
                            THEN request_token_stat_hourly_rollups.bucket_end
                        ELSE excluded.bucket_end
                    END,
                    input_tokens = request_token_stat_hourly_rollups.input_tokens + excluded.input_tokens,
                    cached_input_tokens = request_token_stat_hourly_rollups.cached_input_tokens + excluded.cached_input_tokens,
                    output_tokens = request_token_stat_hourly_rollups.output_tokens + excluded.output_tokens,
                    total_tokens = request_token_stat_hourly_rollups.total_tokens + excluded.total_tokens,
                    reasoning_output_tokens = request_token_stat_hourly_rollups.reasoning_output_tokens + excluded.reasoning_output_tokens,
                    estimated_cost_usd = request_token_stat_hourly_rollups.estimated_cost_usd + excluded.estimated_cost_usd,
                    request_count = request_token_stat_hourly_rollups.request_count + excluded.request_count,
                    success_count = request_token_stat_hourly_rollups.success_count + excluded.success_count,
                    error_count = request_token_stat_hourly_rollups.error_count + excluded.error_count,
                    updated_at = excluded.updated_at",
                token_total = token_total_sql_expr(),
            ),
            (cutoff_ts, now),
        )?;
        let deleted = tx.execute(delete_request_token_stats_before_sql(), [cutoff_ts])?;
        tx.commit()?;
        Ok(deleted)
    }

    pub fn summarize_request_token_stats_between(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<RequestLogTodaySummary> {
        if end_ts <= start_ts {
            return Ok(empty_request_log_today_summary());
        }
        let sql = request_log_today_summary_sql(None, None);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![start_ts, end_ts, start_ts, end_ts])?;
        if let Some(row) = rows.next()? {
            return request_log_today_summary_from_row(row);
        }
        Ok(empty_request_log_today_summary())
    }

    pub fn summarize_request_token_stats_between_for_keys(
        &self,
        start_ts: i64,
        end_ts: i64,
        key_ids: &[String],
    ) -> Result<RequestLogTodaySummary> {
        if end_ts <= start_ts {
            return Ok(empty_request_log_today_summary());
        }
        let Some(key_filter) = PairedKeyIdSqlFilter::create(self, "s.key_id", "h.key_id", key_ids)?
        else {
            return Ok(empty_request_log_today_summary());
        };

        let sql = request_log_today_summary_sql(
            Some(key_filter.first_condition()),
            Some(key_filter.second_condition()),
        );
        let paired_params = key_filter.params();
        let split_at = paired_params.len() / 2;
        let (raw_key_params, hourly_key_params) = paired_params.split_at(split_at);
        let mut params = Vec::with_capacity(paired_params.len() + 4);
        params.push(Value::Integer(start_ts));
        params.push(Value::Integer(end_ts));
        params.extend_from_slice(raw_key_params);
        params.push(Value::Integer(start_ts));
        params.push(Value::Integer(end_ts));
        params.extend_from_slice(hourly_key_params);

        self.conn.query_row(&sql, params_from_iter(params), |row| {
            request_log_today_summary_from_row(row)
        })
    }

    pub fn summarize_request_token_stats_query_between(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    ) -> Result<RequestLogQuerySummary> {
        if is_empty_optional_range(start_ts, end_ts) {
            return Ok(RequestLogQuerySummary::default());
        }
        self.query_request_token_stats_query_between(start_ts, end_ts, None)
    }

    pub fn summarize_request_token_stats_query_for_keys_between(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
    ) -> Result<RequestLogQuerySummary> {
        if is_empty_optional_range(start_ts, end_ts) {
            return Ok(RequestLogQuerySummary::default());
        }
        let Some(key_filter) = TempKeyIdFilter::create(self, key_ids)? else {
            return Ok(RequestLogQuerySummary::default());
        };
        self.query_request_token_stats_query_between(start_ts, end_ts, Some(&key_filter))
    }

    fn query_request_token_stats_query_between(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_filter: Option<&TempKeyIdFilter<'_>>,
    ) -> Result<RequestLogQuerySummary> {
        let key_filter_clauses = key_filter_sql_clauses(key_filter);
        let raw = raw_token_rollup_select(
            "",
            &format!(
                "{}{}",
                optional_raw_stats_range_clause(start_ts, end_ts),
                key_filter_clauses.raw,
            ),
            "",
            false,
        );
        let hourly = hourly_token_rollup_select(
            "",
            &format!(
                "{}{}",
                optional_hourly_rollup_range_clause(start_ts, end_ts),
                key_filter_clauses.hourly,
            ),
            "",
        );
        let sql = format!(
            "WITH combined AS (
                {raw}
                UNION ALL
                {hourly}
             )
             SELECT
                {COMBINED_ROLLUP_COLUMNS}
             FROM combined"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let usage = if let Some(params) = optional_range_params(start_ts, end_ts) {
            stmt.query_row(params_from_iter(params), |row| {
                token_usage_rollup_from_row(row, 0)
            })?
        } else {
            stmt.query_row([], |row| token_usage_rollup_from_row(row, 0))?
        };
        Ok(request_log_query_summary_from_usage(usage))
    }

    pub fn summarize_request_token_stats_by_key(&self) -> Result<Vec<ApiKeyTokenUsageSummary>> {
        self.summarize_request_token_stats_by_key_filtered(None)
    }

    pub fn summarize_request_token_stats_by_key_for_keys(
        &self,
        key_ids: &[String],
    ) -> Result<Vec<ApiKeyTokenUsageSummary>> {
        self.summarize_request_token_stats_by_key_filtered(Some(key_ids))
    }

    pub fn summarize_request_token_stats_by_key_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<ApiKeyTokenUsageSummary>> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(Vec::new());
        }
        self.query_request_token_stats_by_key_for_user(user_id)
    }

    fn summarize_request_token_stats_by_key_filtered(
        &self,
        key_ids: Option<&[String]>,
    ) -> Result<Vec<ApiKeyTokenUsageSummary>> {
        let Some(key_ids) = key_ids else {
            return self.query_request_token_stats_by_key(None);
        };
        let Some(key_filter) = TempKeyIdFilter::create(self, key_ids)? else {
            return Ok(Vec::new());
        };
        self.query_request_token_stats_by_key(Some(&key_filter))
    }

    fn query_request_token_stats_by_key(
        &self,
        key_filter: Option<&TempKeyIdFilter<'_>>,
    ) -> Result<Vec<ApiKeyTokenUsageSummary>> {
        let key_filter_clauses = key_filter_sql_clauses(key_filter);
        let raw = raw_key_usage_select(
            "",
            &format!(
                "t.key_id IS NOT NULL AND TRIM(t.key_id) <> ''{}",
                key_filter_clauses.raw
            ),
            "GROUP BY t.key_id",
        );
        let hourly = hourly_key_usage_select(
            "",
            &format!(
                "NULLIF(TRIM(h.key_id), '') IS NOT NULL{}",
                key_filter_clauses.hourly
            ),
            "GROUP BY key_id",
        );
        let legacy = legacy_key_usage_select(
            "",
            &format!(
                "NULLIF(TRIM(r.key_id), '') IS NOT NULL{}",
                key_filter_clauses.legacy
            ),
            "GROUP BY key_id",
        );
        let combined_selects = union_all_selects([raw, hourly, legacy]);
        let sql = request_token_stats_by_key_sql(&combined_selects, &key_filter_clauses);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_api_key_token_usage_summary(row)?);
        }
        Ok(items)
    }

    fn query_request_token_stats_by_key_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<ApiKeyTokenUsageSummary>> {
        let raw = raw_key_usage_select(
            "",
            "t.key_id IS NOT NULL AND TRIM(t.key_id) <> ''",
            "GROUP BY t.key_id",
        );
        let hourly = hourly_key_usage_select(
            "",
            "NULLIF(TRIM(h.key_id), '') IS NOT NULL",
            "GROUP BY key_id",
        );
        let legacy = legacy_key_usage_select(
            "",
            "NULLIF(TRIM(r.key_id), '') IS NOT NULL",
            "GROUP BY key_id",
        );
        let combined_selects = union_all_selects([raw, hourly, legacy]);
        let sql = format!(
            "WITH combined AS (
                {combined_selects}
             )
             SELECT
                s.key_id,
                IFNULL(SUM(IFNULL(s.total_tokens, 0)), 0) AS total_tokens,
                IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
             FROM combined s
             INNER JOIN api_key_owners owner
                ON owner.key_id = s.key_id
               AND owner.owner_kind = 'user'
               AND owner.owner_user_id = ?1
             WHERE s.key_id IS NOT NULL AND TRIM(s.key_id) <> ''
             GROUP BY s.key_id
             ORDER BY total_tokens DESC, s.key_id ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([user_id])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_api_key_token_usage_summary(row)?);
        }
        Ok(items)
    }

    pub fn summarize_request_token_stats_by_model(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    ) -> Result<Vec<TokenUsageSummary>> {
        self.summarize_request_token_stats_by_model_filtered(start_ts, end_ts, None, None)
    }

    pub fn summarize_request_token_stats_by_model_for_keys(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
    ) -> Result<Vec<TokenUsageSummary>> {
        self.summarize_request_token_stats_by_model_for_keys_limited(
            start_ts, end_ts, key_ids, None,
        )
    }

    pub fn summarize_request_token_stats_by_model_for_keys_limited(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
        limit: Option<usize>,
    ) -> Result<Vec<TokenUsageSummary>> {
        self.summarize_request_token_stats_by_model_filtered(start_ts, end_ts, Some(key_ids), limit)
    }

    fn summarize_request_token_stats_by_model_filtered(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: Option<&[String]>,
        limit: Option<usize>,
    ) -> Result<Vec<TokenUsageSummary>> {
        if is_empty_optional_range(start_ts, end_ts) {
            return Ok(Vec::new());
        }
        if limit == Some(0) {
            return Ok(Vec::new());
        }
        let Some(key_ids) = key_ids else {
            return self.query_request_token_stats_by_model(start_ts, end_ts, None, limit);
        };
        let Some(key_filter) = TempKeyIdFilter::create(self, key_ids)? else {
            return Ok(Vec::new());
        };
        self.query_request_token_stats_by_model(start_ts, end_ts, Some(&key_filter), limit)
    }

    fn query_request_token_stats_by_model(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_filter: Option<&TempKeyIdFilter<'_>>,
        limit: Option<usize>,
    ) -> Result<Vec<TokenUsageSummary>> {
        let key_filter_clauses = key_filter_sql_clauses(key_filter);
        let limit_clause = sql_limit_clause(limit);
        let raw = raw_key_usage_select(
            "",
            &format!(
                "{}{}",
                optional_raw_stats_range_clause(start_ts, end_ts),
                key_filter_clauses.raw
            ),
            "GROUP BY normalized_model",
        );
        let hourly = hourly_key_usage_select(
            "",
            &format!(
                "{}{}",
                optional_hourly_rollup_range_clause(start_ts, end_ts),
                key_filter_clauses.hourly
            ),
            "GROUP BY normalized_model",
        );
        let mut selects = vec![raw, hourly];
        if start_ts.is_none() && end_ts.is_none() {
            let legacy = legacy_key_usage_select(
                "",
                &format!("1 = 1{}", key_filter_clauses.legacy),
                "GROUP BY normalized_model",
            );
            selects.push(legacy);
        }
        let combined_selects = union_all_selects(selects);
        let sql = format!(
            "WITH combined AS (
                {combined_selects}
             )
             SELECT
                s.normalized_model,
                IFNULL(SUM(s.input_tokens), 0) AS input_tokens,
                IFNULL(SUM(s.cached_input_tokens), 0) AS cached_input_tokens,
                IFNULL(SUM(s.output_tokens), 0) AS output_tokens,
                IFNULL(SUM(s.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                IFNULL(SUM(IFNULL(s.total_tokens, 0)), 0) AS total_tokens,
                IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
             FROM combined s
             WHERE 1 = 1
             GROUP BY s.normalized_model
             ORDER BY total_tokens DESC, s.normalized_model ASC{limit_clause}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = if let Some(params) = optional_range_params(start_ts, end_ts) {
            stmt.query(params_from_iter(params))?
        } else {
            stmt.query([])?
        };
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_token_usage_summary(row)?);
        }
        Ok(items)
    }

    pub fn summarize_request_token_stats_by_key_and_model(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    ) -> Result<Vec<ApiKeyModelTokenUsageSummary>> {
        self.summarize_request_token_stats_by_key_and_model_filtered(start_ts, end_ts, None)
    }

    pub fn summarize_request_token_stats_by_key_and_model_for_keys(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
    ) -> Result<Vec<ApiKeyModelTokenUsageSummary>> {
        self.summarize_request_token_stats_by_key_and_model_filtered(
            start_ts,
            end_ts,
            Some(key_ids),
        )
    }

    pub fn load_member_dashboard_usage_breakdown_snapshot(
        &self,
        key_ids: &[String],
        day_start: i64,
        day_end: i64,
        trend_days: i64,
        top_model_limit: usize,
    ) -> Result<MemberDashboardUsageBreakdownSnapshot> {
        if key_ids.is_empty() {
            return Ok(MemberDashboardUsageBreakdownSnapshot::default());
        }
        let day_span = day_end.saturating_sub(day_start).max(1);
        let trend_days = trend_days.max(1);
        let trend_start = day_start.saturating_sub((trend_days - 1).saturating_mul(day_span));
        Ok(MemberDashboardUsageBreakdownSnapshot {
            today_key_model_usage: self.summarize_request_token_stats_by_key_and_model_for_keys(
                Some(day_start),
                Some(day_end),
                key_ids,
            )?,
            total_key_usage: self.summarize_request_token_stats_by_key_for_keys(key_ids)?,
            top_model_usage: self.summarize_request_token_stats_by_model_for_keys_limited(
                Some(trend_start),
                Some(day_end),
                key_ids,
                Some(top_model_limit),
            )?,
        })
    }

    fn summarize_request_token_stats_by_key_and_model_filtered(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: Option<&[String]>,
    ) -> Result<Vec<ApiKeyModelTokenUsageSummary>> {
        if is_empty_optional_range(start_ts, end_ts) {
            return Ok(Vec::new());
        }
        let Some(key_ids) = key_ids else {
            return self.query_request_token_stats_by_key_and_model(start_ts, end_ts, None);
        };
        let Some(key_filter) = TempKeyIdFilter::create(self, key_ids)? else {
            return Ok(Vec::new());
        };
        self.query_request_token_stats_by_key_and_model(start_ts, end_ts, Some(&key_filter))
    }

    fn query_request_token_stats_by_key_and_model(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_filter: Option<&TempKeyIdFilter<'_>>,
    ) -> Result<Vec<ApiKeyModelTokenUsageSummary>> {
        let key_filter_clauses = key_filter_sql_clauses(key_filter);
        let raw = raw_key_usage_select(
            "",
            &format!(
                "{} AND t.key_id IS NOT NULL AND TRIM(t.key_id) <> ''{}",
                optional_raw_stats_range_clause(start_ts, end_ts),
                key_filter_clauses.raw
            ),
            "GROUP BY t.key_id, normalized_model",
        );
        let hourly = hourly_key_usage_select(
            "",
            &format!(
                "{} AND NULLIF(TRIM(h.key_id), '') IS NOT NULL{}",
                optional_hourly_rollup_range_clause(start_ts, end_ts),
                key_filter_clauses.hourly
            ),
            "GROUP BY key_id, normalized_model",
        );
        let mut selects = vec![raw, hourly];
        if start_ts.is_none() && end_ts.is_none() {
            let legacy = legacy_key_usage_select(
                "",
                &format!(
                    "NULLIF(TRIM(r.key_id), '') IS NOT NULL{}",
                    key_filter_clauses.legacy
                ),
                "GROUP BY key_id, normalized_model",
            );
            selects.push(legacy);
        }
        let combined_selects = union_all_selects(selects);
        let sql = format!(
            "WITH combined AS (
                {combined_selects}
             )
             SELECT
                s.key_id,
                s.normalized_model,
                IFNULL(SUM(s.input_tokens), 0) AS input_tokens,
                IFNULL(SUM(s.cached_input_tokens), 0) AS cached_input_tokens,
                IFNULL(SUM(s.output_tokens), 0) AS output_tokens,
                IFNULL(SUM(s.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                IFNULL(SUM(IFNULL(s.total_tokens, 0)), 0) AS total_tokens,
                IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
             FROM combined s
             WHERE s.key_id IS NOT NULL AND TRIM(s.key_id) <> ''
             GROUP BY s.key_id, s.normalized_model
             ORDER BY total_tokens DESC, s.key_id ASC, s.normalized_model ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = if let Some(params) = optional_range_params(start_ts, end_ts) {
            stmt.query(params_from_iter(params))?
        } else {
            stmt.query([])?
        };
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(map_api_key_model_token_usage_summary(row)?);
        }
        Ok(items)
    }

    pub fn summarize_request_token_stats_daily(
        &self,
        start_ts: i64,
        end_ts: i64,
        bucket_seconds: i64,
    ) -> Result<Vec<DailyTokenUsageRollup>> {
        if end_ts <= start_ts {
            return Ok(Vec::new());
        }
        let bucket_seconds = bucket_seconds.max(1);
        let raw = raw_token_rollup_select(
            "?1 + CAST((t.created_at - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,",
            "t.created_at >= ?1 AND t.created_at < ?2",
            "GROUP BY bucket_start",
            false,
        );
        let hourly = hourly_token_rollup_select(
            "?1 + CAST((h.bucket_start - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,",
            hourly_rollup_range_clause(),
            "GROUP BY bucket_start",
        );
        let sql = format!(
            "WITH combined AS (
                {raw}
                UNION ALL
                {hourly}
             )
             SELECT
                bucket_start,
                MIN(bucket_start + ?3, ?2) AS bucket_end,
                {COMBINED_ROLLUP_COLUMNS}
             FROM combined
             GROUP BY bucket_start
             ORDER BY bucket_start ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![start_ts, end_ts, bucket_seconds])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(DailyTokenUsageRollup {
                day_start_ts: row.get(0)?,
                day_end_ts: row.get(1)?,
                usage: token_usage_rollup_from_row(row, 2)?,
            });
        }
        Ok(items)
    }

    pub fn summarize_request_token_stats_by_user_between(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<UserTokenUsageRollup>> {
        self.summarize_request_token_stats_by_user_between_limited(start_ts, end_ts, None)
    }

    pub fn summarize_request_token_stats_by_user_between_limited(
        &self,
        start_ts: i64,
        end_ts: i64,
        limit: Option<usize>,
    ) -> Result<Vec<UserTokenUsageRollup>> {
        if end_ts <= start_ts {
            return Ok(Vec::new());
        }
        if limit == Some(0) {
            return Ok(Vec::new());
        }
        let limit_clause = sql_limit_clause(limit);
        let raw = raw_token_rollup_select(
            &format!("{USER_OWNER_EXPR} AS user_id,"),
            &format!("t.created_at >= ?1 AND t.created_at < ?2 AND {USER_OWNER_EXPR} IS NOT NULL"),
            "GROUP BY user_id",
            true,
        );
        let hourly = hourly_token_rollup_select(
            "NULLIF(TRIM(h.owner_user_id), '') AS user_id,",
            &format!(
                "{} AND NULLIF(TRIM(h.owner_user_id), '') IS NOT NULL",
                hourly_rollup_range_clause()
            ),
            "GROUP BY user_id",
        );
        let sql = format!(
            "WITH combined AS (
                {raw}
                UNION ALL
                {hourly}
             )
             SELECT
                user_id,
                {COMBINED_ROLLUP_COLUMNS}
             FROM combined
             GROUP BY user_id
             ORDER BY total_tokens DESC, user_id ASC{limit_clause}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![start_ts, end_ts])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(UserTokenUsageRollup {
                user_id: row.get(0)?,
                usage: token_usage_rollup_from_row(row, 1)?,
            });
        }
        Ok(items)
    }

    pub fn summarize_request_token_stats_for_user_between(
        &self,
        user_id: &str,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<TokenUsageRollup> {
        if end_ts <= start_ts || user_id.trim().is_empty() {
            return Ok(TokenUsageRollup::default());
        }
        let raw = raw_token_rollup_select(
            "",
            &format!("t.created_at >= ?1 AND t.created_at < ?2 AND {USER_OWNER_EXPR} = ?3"),
            "",
            true,
        );
        let hourly = hourly_token_rollup_select(
            "",
            &format!("{} AND h.owner_user_id = ?3", hourly_rollup_range_clause()),
            "",
        );
        let sql = format!(
            "WITH combined AS (
                {raw}
                UNION ALL
                {hourly}
             )
             SELECT
                {COMBINED_ROLLUP_COLUMNS}
             FROM combined"
        );
        self.conn
            .query_row(&sql, params![start_ts, end_ts, user_id.trim()], |row| {
                token_usage_rollup_from_row(row, 0)
            })
    }

    pub fn summarize_request_token_stats_daily_for_user(
        &self,
        user_id: &str,
        start_ts: i64,
        end_ts: i64,
        bucket_seconds: i64,
    ) -> Result<Vec<DailyTokenUsageRollup>> {
        if end_ts <= start_ts || user_id.trim().is_empty() {
            return Ok(Vec::new());
        }
        let bucket_seconds = bucket_seconds.max(1);
        let raw = raw_token_rollup_select(
            "?1 + CAST((t.created_at - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,",
            &format!("t.created_at >= ?1 AND t.created_at < ?2 AND {USER_OWNER_EXPR} = ?4"),
            "GROUP BY bucket_start",
            true,
        );
        let hourly = hourly_token_rollup_select(
            "?1 + CAST((h.bucket_start - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,",
            &format!("{} AND h.owner_user_id = ?4", hourly_rollup_range_clause()),
            "GROUP BY bucket_start",
        );
        let sql = format!(
            "WITH combined AS (
                {raw}
                UNION ALL
                {hourly}
             )
             SELECT
                bucket_start,
                MIN(bucket_start + ?3, ?2) AS bucket_end,
                {COMBINED_ROLLUP_COLUMNS}
             FROM combined
             GROUP BY bucket_start
             ORDER BY bucket_start ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![start_ts, end_ts, bucket_seconds, user_id.trim()])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(DailyTokenUsageRollup {
                day_start_ts: row.get(0)?,
                day_end_ts: row.get(1)?,
                usage: token_usage_rollup_from_row(row, 2)?,
            });
        }
        Ok(items)
    }

    pub fn summarize_request_token_stats_by_source_between(
        &self,
        source_kind: &str,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<SourceTokenUsageRollup>> {
        self.summarize_request_token_stats_by_sources_between(&[source_kind], start_ts, end_ts)
    }

    pub fn summarize_request_token_stats_by_sources_between(
        &self,
        source_kinds: &[&str],
        start_ts: i64,
        end_ts: i64,
    ) -> Result<Vec<SourceTokenUsageRollup>> {
        self.summarize_request_token_stats_by_sources_between_limited(
            source_kinds,
            start_ts,
            end_ts,
            None,
        )
    }

    pub fn summarize_request_token_stats_by_sources_between_limited(
        &self,
        source_kinds: &[&str],
        start_ts: i64,
        end_ts: i64,
        limit_per_source_kind: Option<usize>,
    ) -> Result<Vec<SourceTokenUsageRollup>> {
        if end_ts <= start_ts {
            return Ok(Vec::new());
        }
        if limit_per_source_kind == Some(0) {
            return Ok(Vec::new());
        }
        let mut normalized_source_kinds = source_kinds
            .iter()
            .map(|source_kind| source_kind.trim())
            .filter(|source_kind| !source_kind.is_empty())
            .collect::<Vec<_>>();
        normalized_source_kinds.sort_unstable();
        normalized_source_kinds.dedup();
        if normalized_source_kinds.is_empty() {
            return Ok(Vec::new());
        }
        let mut raw_parts = Vec::new();
        let mut hourly_parts = Vec::new();
        for source_kind in normalized_source_kinds {
            let Some(source_id_expr) = source_id_expr(source_kind) else {
                continue;
            };
            let Some(hourly_source_id_expr) = hourly_source_id_expr(source_kind) else {
                continue;
            };
            raw_parts.push(raw_token_rollup_select(
                &format!("'{source_kind}' AS source_kind, {source_id_expr} AS source_id,"),
                &format!(
                    "t.created_at >= ?1 AND t.created_at < ?2 AND {source_id_expr} IS NOT NULL"
                ),
                "GROUP BY source_kind, source_id",
                false,
            ));
            hourly_parts.push(hourly_token_rollup_select(
                &format!("'{source_kind}' AS source_kind, {hourly_source_id_expr} AS source_id,"),
                &format!(
                    "{} AND {hourly_source_id_expr} IS NOT NULL",
                    hourly_rollup_range_clause()
                ),
                "GROUP BY source_kind, source_id",
            ));
        }
        let selects = raw_parts
            .into_iter()
            .chain(hourly_parts)
            .collect::<Vec<_>>();
        if selects.is_empty() {
            return Ok(Vec::new());
        }
        let union_sql = selects.join("\nUNION ALL\n");
        let sql = if let Some(limit) = limit_per_source_kind {
            format!(
                "WITH combined AS (
                    {union_sql}
                 ),
                 ranked AS (
                    SELECT
                        source_kind,
                        source_id,
                        {COMBINED_ROLLUP_COLUMNS},
                        ROW_NUMBER() OVER (
                            PARTITION BY source_kind
                            ORDER BY SUM(IFNULL(total_tokens, 0)) DESC, source_id ASC
                        ) AS source_rank
                    FROM combined
                    GROUP BY source_kind, source_id
                 )
                 SELECT
                    source_kind,
                    source_id,
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                    reasoning_output_tokens,
                    total_tokens,
                    estimated_cost_usd,
                    request_count,
                    success_count,
                    error_count
                 FROM ranked
                 WHERE source_rank <= {limit}
                 ORDER BY source_kind ASC, total_tokens DESC, source_id ASC"
            )
        } else {
            format!(
                "WITH combined AS (
                    {union_sql}
                 )
                 SELECT
                    source_kind,
                    source_id,
                    {COMBINED_ROLLUP_COLUMNS}
                 FROM combined
                 GROUP BY source_kind, source_id
                 ORDER BY source_kind ASC, total_tokens DESC, source_id ASC"
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![start_ts, end_ts])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(SourceTokenUsageRollup {
                source_kind: row.get(0)?,
                source_id: row.get(1)?,
                usage: token_usage_rollup_from_row(row, 2)?,
            });
        }
        Ok(items)
    }

    pub(super) fn ensure_request_token_stats_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS request_token_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                request_log_id INTEGER NOT NULL,
                key_id TEXT,
                account_id TEXT,
                model TEXT,
                actual_source_kind TEXT,
                actual_source_id TEXT,
                input_tokens INTEGER,
                cached_input_tokens INTEGER,
                output_tokens INTEGER,
                total_tokens INTEGER,
                reasoning_output_tokens INTEGER,
                estimated_cost_usd REAL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_request_token_stats_request_log_id
             ON request_token_stats(request_log_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_created_at
             ON request_token_stats(created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_account_id_created_at
             ON request_token_stats(account_id, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_key_id_created_at
             ON request_token_stats(key_id, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_model_created_at
             ON request_token_stats(model, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_key_model_created_at
             ON request_token_stats(key_id, model, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_account_model_created_at
             ON request_token_stats(account_id, model, created_at DESC)",
            [],
        )?;
        self.ensure_column("request_token_stats", "total_tokens", "INTEGER")?;
        self.ensure_column("request_token_stats", "actual_source_kind", "TEXT")?;
        self.ensure_column("request_token_stats", "actual_source_id", "TEXT")?;
        if self.has_column("request_logs", "actual_source_kind")?
            && self.has_column("request_logs", "actual_source_id")?
        {
            self.conn.execute(
                "UPDATE request_token_stats
                 SET
                    actual_source_kind = (
                        SELECT request_logs.actual_source_kind
                        FROM request_logs
                        WHERE request_logs.id = request_token_stats.request_log_id
                    ),
                    actual_source_id = (
                        SELECT request_logs.actual_source_id
                        FROM request_logs
                        WHERE request_logs.id = request_token_stats.request_log_id
                    )
                 WHERE (actual_source_kind IS NULL OR TRIM(actual_source_kind) = '')
                   AND (actual_source_id IS NULL OR TRIM(actual_source_id) = '')
                   AND request_log_id IS NOT NULL
                   AND EXISTS (
                        SELECT 1
                        FROM request_logs
                        WHERE request_logs.id = request_token_stats.request_log_id
                          AND (
                            request_logs.actual_source_kind IS NOT NULL
                            OR request_logs.actual_source_id IS NOT NULL
                          )
                   )",
                [],
            )?;
        }
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stats_actual_source_created_at
             ON request_token_stats(actual_source_kind, actual_source_id, created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS request_token_stat_rollups (
                key_id TEXT NOT NULL DEFAULT '',
                account_id TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL DEFAULT '',
                input_tokens INTEGER NOT NULL DEFAULT 0,
                cached_input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_output_tokens INTEGER NOT NULL DEFAULT 0,
                estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
                source_rows INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (key_id, account_id, model)
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stat_rollups_key_id
             ON request_token_stat_rollups(key_id)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stat_rollups_model
             ON request_token_stat_rollups(model)",
            [],
        )?;
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS request_token_stat_hourly_rollups (
                bucket_start INTEGER NOT NULL,
                bucket_end INTEGER NOT NULL,
                key_id TEXT NOT NULL DEFAULT '',
                account_id TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL DEFAULT '',
                actual_source_kind TEXT NOT NULL DEFAULT '',
                actual_source_id TEXT NOT NULL DEFAULT '',
                owner_user_id TEXT NOT NULL DEFAULT '',
                input_tokens INTEGER NOT NULL DEFAULT 0,
                cached_input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_output_tokens INTEGER NOT NULL DEFAULT 0,
                estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
                request_count INTEGER NOT NULL DEFAULT 0,
                success_count INTEGER NOT NULL DEFAULT 0,
                error_count INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY(bucket_start, key_id, account_id, model, actual_source_kind, actual_source_id, owner_user_id)
             )",
            [],
        )?;
        self.ensure_column("request_token_stat_hourly_rollups", "bucket_end", "INTEGER")?;
        self.conn.execute(
            "UPDATE request_token_stat_hourly_rollups
             SET bucket_end = bucket_start + 3600
             WHERE bucket_end IS NULL OR bucket_end <= bucket_start",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stat_hourly_rollups_bucket_start
             ON request_token_stat_hourly_rollups(bucket_start)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stat_hourly_rollups_key_bucket
             ON request_token_stat_hourly_rollups(key_id, bucket_start)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stat_hourly_rollups_owner_bucket
             ON request_token_stat_hourly_rollups(owner_user_id, bucket_start)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_token_stat_hourly_rollups_source_bucket
             ON request_token_stat_hourly_rollups(actual_source_kind, actual_source_id, bucket_start)",
            [],
        )?;
        if self.has_column("request_logs", "input_tokens")? {
            let actual_source_kind_expr =
                if self.has_column("request_logs", "actual_source_kind")? {
                    "actual_source_kind"
                } else {
                    "NULL"
                };
            let actual_source_id_expr = if self.has_column("request_logs", "actual_source_id")? {
                "actual_source_id"
            } else {
                "NULL"
            };
            let backfill_sql = format!(
                "INSERT OR IGNORE INTO request_token_stats (
                    request_log_id, key_id, account_id, model, actual_source_kind, actual_source_id,
                    input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 )
                 SELECT
                    id, key_id, account_id, model, {actual_source_kind_expr}, {actual_source_id_expr},
                    input_tokens, cached_input_tokens, output_tokens, NULL, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 FROM request_logs
                 WHERE input_tokens IS NOT NULL
                    OR cached_input_tokens IS NOT NULL
                    OR output_tokens IS NOT NULL
                    OR reasoning_output_tokens IS NOT NULL
                    OR estimated_cost_usd IS NOT NULL"
            );
            self.conn.execute(&backfill_sql, [])?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/request_token_stats_tests.rs"]
mod tests;
