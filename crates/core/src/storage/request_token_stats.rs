use rusqlite::{params, params_from_iter, types::Value, Result, Row};
use std::sync::atomic::{AtomicI64, Ordering};

use super::key_id_filters::TempKeyIdFilter;
use super::{
    now_ts, ApiKeyModelTokenUsageSummary, ApiKeyTokenUsageSummary, DailyTokenUsageRollup,
    RequestLogTodaySummary, RequestTokenStat, SourceTokenUsageRollup, Storage, TokenUsageRollup,
    TokenUsageSummary, UserTokenUsageRollup,
};

const DEFAULT_REQUEST_TOKEN_STATS_RETAIN_DAYS: i64 = 14;
const DEFAULT_OBSERVABILITY_MAINTENANCE_INTERVAL_SECS: i64 = 900;
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
    COUNT(DISTINCT r.id) AS request_count,
    COUNT(DISTINCT CASE WHEN r.status_code >= 200 AND r.status_code <= 299 THEN r.id END) AS success_count,
    COUNT(DISTINCT CASE WHEN IFNULL(r.status_code, 0) >= 400 OR TRIM(IFNULL(r.error, '')) <> '' THEN r.id END) AS error_count";

const USER_OWNER_EXPR: &str = "COALESCE(
    (
        SELECT MIN(NULLIF(TRIM(w.owner_id), ''))
        FROM app_wallet_ledger_entries l
        JOIN app_wallets w ON w.id = l.wallet_id
        WHERE l.request_log_id = r.id
          AND l.entry_kind = 'request_charge'
          AND w.owner_kind = 'user'
    ),
    NULLIF(TRIM(owner.owner_user_id), '')
)";

// User attribution prefers the request_charge wallet owner. Use a correlated
// lookup so dashboard range queries do not pre-aggregate the entire ledger table.
// The api_key_owners fallback is current-owner based, so old uncharged logs are
// approximate.
const USER_OWNER_JOINS: &str = "
    LEFT JOIN api_key_owners owner ON owner.key_id = r.key_id AND owner.owner_kind = 'user'";

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

fn source_id_expr(source_kind: &str) -> Option<&'static str> {
    match source_kind {
        "openai_account" => Some(
            // Prefer actual_source_* written by routing. Legacy account_id is only
            // used when actual source metadata was not captured.
            "CASE
                WHEN r.actual_source_kind = 'openai_account'
                    THEN COALESCE(NULLIF(TRIM(r.actual_source_id), ''), NULLIF(TRIM(r.account_id), ''))
                WHEN r.actual_source_kind IS NULL OR TRIM(r.actual_source_kind) = ''
                    THEN NULLIF(TRIM(r.account_id), '')
                ELSE NULL
             END",
        ),
        "aggregate_api" => Some(
            // Prefer actual_source_* written by routing. Legacy aggregate API
            // context is only used when actual source metadata was not captured.
            "CASE
                WHEN r.actual_source_kind = 'aggregate_api'
                    THEN COALESCE(NULLIF(TRIM(r.actual_source_id), ''), NULLIF(TRIM(r.initial_aggregate_api_id), ''))
                WHEN r.actual_source_kind IS NULL OR TRIM(r.actual_source_kind) = ''
                    THEN NULLIF(TRIM(r.initial_aggregate_api_id), '')
                ELSE NULL
             END",
        ),
        _ => None,
    }
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
                request_log_id, key_id, account_id, model,
                input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                estimated_cost_usd, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            (
                stat.request_log_id,
                &stat.key_id,
                &stat.account_id,
                &stat.model,
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
        let now = now_ts();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            &format!(
                "INSERT INTO request_token_stat_rollups (
                    key_id, account_id, model,
                    input_tokens, cached_input_tokens, output_tokens, total_tokens,
                    reasoning_output_tokens, estimated_cost_usd, source_rows, updated_at
                 )
                 SELECT
                    COALESCE(NULLIF(TRIM(key_id), ''), ''),
                    COALESCE(NULLIF(TRIM(account_id), ''), ''),
                    COALESCE(NULLIF(TRIM(model), ''), ''),
                    IFNULL(SUM(CASE WHEN input_tokens > 0 THEN input_tokens ELSE 0 END), 0),
                    IFNULL(SUM(CASE WHEN cached_input_tokens > 0 THEN cached_input_tokens ELSE 0 END), 0),
                    IFNULL(SUM(CASE WHEN output_tokens > 0 THEN output_tokens ELSE 0 END), 0),
                    IFNULL(SUM({token_total}), 0),
                    IFNULL(SUM(CASE WHEN reasoning_output_tokens > 0 THEN reasoning_output_tokens ELSE 0 END), 0),
                    IFNULL(SUM(CASE WHEN estimated_cost_usd > 0 THEN estimated_cost_usd ELSE 0 END), 0.0),
                    COUNT(1),
                    ?2
                 FROM request_token_stats
                 WHERE created_at < ?1
                 GROUP BY
                    COALESCE(NULLIF(TRIM(key_id), ''), ''),
                    COALESCE(NULLIF(TRIM(account_id), ''), ''),
                    COALESCE(NULLIF(TRIM(model), ''), '')
                 ON CONFLICT(key_id, account_id, model) DO UPDATE SET
                    input_tokens = request_token_stat_rollups.input_tokens + excluded.input_tokens,
                    cached_input_tokens = request_token_stat_rollups.cached_input_tokens + excluded.cached_input_tokens,
                    output_tokens = request_token_stat_rollups.output_tokens + excluded.output_tokens,
                    total_tokens = request_token_stat_rollups.total_tokens + excluded.total_tokens,
                    reasoning_output_tokens = request_token_stat_rollups.reasoning_output_tokens + excluded.reasoning_output_tokens,
                    estimated_cost_usd = request_token_stat_rollups.estimated_cost_usd + excluded.estimated_cost_usd,
                    source_rows = request_token_stat_rollups.source_rows + excluded.source_rows,
                    updated_at = excluded.updated_at",
                token_total = token_total_sql_expr(),
            ),
            (cutoff_ts, now),
        )?;
        let deleted = tx.execute(
            "DELETE FROM request_token_stats WHERE created_at < ?1",
            [cutoff_ts],
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    pub fn summarize_request_token_stats_between(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<RequestLogTodaySummary> {
        let mut stmt = self.conn.prepare(
            "SELECT
                IFNULL(SUM(input_tokens), 0),
                IFNULL(SUM(cached_input_tokens), 0),
                IFNULL(SUM(output_tokens), 0),
                IFNULL(SUM(reasoning_output_tokens), 0),
                IFNULL(SUM(estimated_cost_usd), 0.0)
             FROM request_token_stats
             WHERE created_at >= ?1 AND created_at < ?2",
        )?;
        let mut rows = stmt.query((start_ts, end_ts))?;
        if let Some(row) = rows.next()? {
            return Ok(RequestLogTodaySummary {
                input_tokens: row.get(0)?,
                cached_input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                reasoning_output_tokens: row.get(3)?,
                estimated_cost_usd: row.get(4)?,
            });
        }
        Ok(RequestLogTodaySummary {
            input_tokens: 0,
            cached_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            estimated_cost_usd: 0.0,
        })
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
        let key_filter_clause = key_filter
            .map(|filter| filter.exists_clause("s.key_id"))
            .unwrap_or_default();
        let mut stmt = self.conn.prepare(&format!(
            "WITH all_stats AS (
                SELECT
                    key_id,
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                    total_tokens,
                    estimated_cost_usd
                FROM request_token_stats
                UNION ALL
                SELECT
                    NULLIF(key_id, '') AS key_id,
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                    total_tokens,
                    estimated_cost_usd
                FROM request_token_stat_rollups
             )
             SELECT
                s.key_id,
                IFNULL(SUM({token_total}), 0) AS total_tokens,
                IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
             FROM all_stats s
             WHERE s.key_id IS NOT NULL AND TRIM(s.key_id) <> ''{key_filter_clause}
             GROUP BY s.key_id
             ORDER BY total_tokens DESC, s.key_id ASC",
            token_total = token_total_sql_expr(),
        ))?;
        let mut rows = stmt.query([])?;
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
        self.summarize_request_token_stats_by_model_filtered(start_ts, end_ts, None)
    }

    pub fn summarize_request_token_stats_by_model_for_keys(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
    ) -> Result<Vec<TokenUsageSummary>> {
        self.summarize_request_token_stats_by_model_filtered(start_ts, end_ts, Some(key_ids))
    }

    fn summarize_request_token_stats_by_model_filtered(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: Option<&[String]>,
    ) -> Result<Vec<TokenUsageSummary>> {
        let Some(key_ids) = key_ids else {
            return self.query_request_token_stats_by_model(start_ts, end_ts, None);
        };
        let Some(key_filter) = TempKeyIdFilter::create(self, key_ids)? else {
            return Ok(Vec::new());
        };
        self.query_request_token_stats_by_model(start_ts, end_ts, Some(&key_filter))
    }

    fn query_request_token_stats_by_model(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_filter: Option<&TempKeyIdFilter<'_>>,
    ) -> Result<Vec<TokenUsageSummary>> {
        let include_rollups = start_ts.is_none() && end_ts.is_none();
        let key_filter_clause = key_filter
            .map(|filter| filter.exists_clause("s.key_id"))
            .unwrap_or_default();
        let sql = if include_rollups {
            format!(
                "WITH all_stats AS (
                    SELECT
                        key_id,
                        model,
                        input_tokens,
                        cached_input_tokens,
                        output_tokens,
                        reasoning_output_tokens,
                        total_tokens,
                        estimated_cost_usd
                    FROM request_token_stats
                    UNION ALL
                    SELECT
                        NULLIF(key_id, '') AS key_id,
                        NULLIF(model, '') AS model,
                        input_tokens,
                        cached_input_tokens,
                        output_tokens,
                        reasoning_output_tokens,
                        total_tokens,
                        estimated_cost_usd
                    FROM request_token_stat_rollups
                 )
                 SELECT
                    COALESCE(NULLIF(TRIM(s.model), ''), 'unknown') AS normalized_model,
                    IFNULL(SUM(s.input_tokens), 0) AS input_tokens,
                    IFNULL(SUM(s.cached_input_tokens), 0) AS cached_input_tokens,
                    IFNULL(SUM(s.output_tokens), 0) AS output_tokens,
                    IFNULL(SUM(s.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                    IFNULL(SUM({token_total}), 0) AS total_tokens,
                    IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
                 FROM all_stats s
                 WHERE 1 = 1{key_filter_clause}
                 GROUP BY normalized_model
                 ORDER BY total_tokens DESC, normalized_model ASC",
                token_total = token_total_sql_expr(),
            )
        } else {
            format!(
                "SELECT
                    COALESCE(NULLIF(TRIM(s.model), ''), 'unknown') AS normalized_model,
                    IFNULL(SUM(s.input_tokens), 0) AS input_tokens,
                    IFNULL(SUM(s.cached_input_tokens), 0) AS cached_input_tokens,
                    IFNULL(SUM(s.output_tokens), 0) AS output_tokens,
                    IFNULL(SUM(s.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                    IFNULL(SUM({token_total}), 0) AS total_tokens,
                    IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
                 FROM request_token_stats s
                 WHERE (?1 IS NULL OR s.created_at >= ?1)
                   AND (?2 IS NULL OR s.created_at < ?2){key_filter_clause}
                 GROUP BY normalized_model
                 ORDER BY total_tokens DESC, normalized_model ASC",
                token_total = token_total_sql_expr(),
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = if include_rollups {
            stmt.query([])?
        } else {
            let params = [
                start_ts.map(Value::Integer).unwrap_or(Value::Null),
                end_ts.map(Value::Integer).unwrap_or(Value::Null),
            ];
            stmt.query(params_from_iter(params.iter()))?
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

    fn summarize_request_token_stats_by_key_and_model_filtered(
        &self,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: Option<&[String]>,
    ) -> Result<Vec<ApiKeyModelTokenUsageSummary>> {
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
        let include_rollups = start_ts.is_none() && end_ts.is_none();
        let key_filter_clause = key_filter
            .map(|filter| filter.exists_clause("s.key_id"))
            .unwrap_or_default();
        let sql = if include_rollups {
            format!(
                "WITH all_stats AS (
                    SELECT
                        key_id,
                        model,
                        input_tokens,
                        cached_input_tokens,
                        output_tokens,
                        reasoning_output_tokens,
                        total_tokens,
                        estimated_cost_usd
                    FROM request_token_stats
                    UNION ALL
                    SELECT
                        NULLIF(key_id, '') AS key_id,
                        NULLIF(model, '') AS model,
                        input_tokens,
                        cached_input_tokens,
                        output_tokens,
                        reasoning_output_tokens,
                        total_tokens,
                        estimated_cost_usd
                    FROM request_token_stat_rollups
                 )
                 SELECT
                    s.key_id,
                    COALESCE(NULLIF(TRIM(s.model), ''), 'unknown') AS normalized_model,
                    IFNULL(SUM(s.input_tokens), 0) AS input_tokens,
                    IFNULL(SUM(s.cached_input_tokens), 0) AS cached_input_tokens,
                    IFNULL(SUM(s.output_tokens), 0) AS output_tokens,
                    IFNULL(SUM(s.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                    IFNULL(SUM({token_total}), 0) AS total_tokens,
                    IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
                 FROM all_stats s
                 WHERE s.key_id IS NOT NULL AND TRIM(s.key_id) <> ''{key_filter_clause}
                 GROUP BY s.key_id, normalized_model
                 ORDER BY total_tokens DESC, s.key_id ASC, normalized_model ASC",
                token_total = token_total_sql_expr(),
            )
        } else {
            format!(
                "SELECT
                    s.key_id,
                    COALESCE(NULLIF(TRIM(s.model), ''), 'unknown') AS normalized_model,
                    IFNULL(SUM(s.input_tokens), 0) AS input_tokens,
                    IFNULL(SUM(s.cached_input_tokens), 0) AS cached_input_tokens,
                    IFNULL(SUM(s.output_tokens), 0) AS output_tokens,
                    IFNULL(SUM(s.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                    IFNULL(SUM({token_total}), 0) AS total_tokens,
                    IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
                 FROM request_token_stats s
                 WHERE s.key_id IS NOT NULL AND TRIM(s.key_id) <> ''
                   AND (?1 IS NULL OR s.created_at >= ?1)
                   AND (?2 IS NULL OR s.created_at < ?2){key_filter_clause}
                 GROUP BY s.key_id, normalized_model
                 ORDER BY total_tokens DESC, s.key_id ASC, normalized_model ASC",
                token_total = token_total_sql_expr(),
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = if include_rollups {
            stmt.query([])?
        } else {
            let params = [
                start_ts.map(Value::Integer).unwrap_or(Value::Null),
                end_ts.map(Value::Integer).unwrap_or(Value::Null),
            ];
            stmt.query(params_from_iter(params.iter()))?
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
        let created_at_expr = "COALESCE(r.created_at, t.created_at)";
        let sql = format!(
            "SELECT
                ?1 + CAST(({created_at_expr} - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,
                MIN(?1 + (CAST(({created_at_expr} - ?1) / ?3 AS INTEGER) + 1) * ?3, ?2) AS bucket_end,
                {TOKEN_ROLLUP_COLUMNS}
             FROM request_token_stats t
             LEFT JOIN request_logs r ON r.id = t.request_log_id
             WHERE {created_at_expr} >= ?1 AND {created_at_expr} < ?2
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
        if end_ts <= start_ts {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT
                {USER_OWNER_EXPR} AS user_id,
                {TOKEN_ROLLUP_COLUMNS}
             FROM request_logs r
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             {USER_OWNER_JOINS}
             WHERE r.created_at >= ?1 AND r.created_at < ?2
               AND {USER_OWNER_EXPR} IS NOT NULL
             GROUP BY user_id
             ORDER BY total_tokens DESC, user_id ASC"
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
        let sql = format!(
            "SELECT
                {TOKEN_ROLLUP_COLUMNS}
             FROM request_logs r
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             {USER_OWNER_JOINS}
             WHERE r.created_at >= ?1 AND r.created_at < ?2
               AND {USER_OWNER_EXPR} = ?3"
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
        let sql = format!(
            "SELECT
                ?1 + CAST((r.created_at - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,
                MIN(?1 + (CAST((r.created_at - ?1) / ?3 AS INTEGER) + 1) * ?3, ?2) AS bucket_end,
                {TOKEN_ROLLUP_COLUMNS}
             FROM request_logs r
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             {USER_OWNER_JOINS}
             WHERE r.created_at >= ?1 AND r.created_at < ?2
               AND {USER_OWNER_EXPR} = ?4
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
        if end_ts <= start_ts {
            return Ok(Vec::new());
        }
        let Some(source_id_expr) = source_id_expr(source_kind) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT
                {source_id_expr} AS source_id,
                {TOKEN_ROLLUP_COLUMNS}
             FROM request_logs r
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             WHERE r.created_at >= ?1 AND r.created_at < ?2
               AND {source_id_expr} IS NOT NULL
             GROUP BY source_id
             ORDER BY total_tokens DESC, source_id ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![start_ts, end_ts])?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(SourceTokenUsageRollup {
                source_kind: source_kind.to_string(),
                source_id: row.get(0)?,
                usage: token_usage_rollup_from_row(row, 1)?,
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
        self.ensure_column("request_token_stats", "total_tokens", "INTEGER")?;

        if self.has_column("request_logs", "input_tokens")? {
            self.conn.execute(
                "INSERT OR IGNORE INTO request_token_stats (
                    request_log_id, key_id, account_id, model,
                    input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 )
                 SELECT
                    id, key_id, account_id, model,
                    input_tokens, cached_input_tokens, output_tokens, NULL, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 FROM request_logs
                 WHERE input_tokens IS NOT NULL
                    OR cached_input_tokens IS NOT NULL
                    OR output_tokens IS NOT NULL
                    OR reasoning_output_tokens IS NOT NULL
                    OR estimated_cost_usd IS NOT NULL",
                [],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/request_token_stats_tests.rs"]
mod tests;
