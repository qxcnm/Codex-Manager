use rusqlite::{params, params_from_iter, types::Value, Result, Row};

use super::key_id_filters::KeyIdSqlFilter;
use super::request_log_filters::{
    account_join_clause, build_request_log_filters, token_stats_join_clause, RequestLogSqlFilters,
};
use super::{
    RequestLog, RequestLogQuerySummary, RequestLogTodaySummary, RequestTokenStat, Storage,
};

const DEFAULT_REQUEST_LOG_RETENTION_DAYS: i64 = 14;
const REQUEST_LOG_RETENTION_DAYS_ENV: &str = "CODEXMANAGER_REQUEST_LOG_RETENTION_DAYS";

fn request_log_retention_days() -> i64 {
    std::env::var(REQUEST_LOG_RETENTION_DAYS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_REQUEST_LOG_RETENTION_DAYS)
}

impl Storage {
    /// 函数 `ensure_request_logs_indexes`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn ensure_request_logs_indexes(&self) -> Result<()> {
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON request_logs(created_at DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_status_code_created_at_id ON request_logs(status_code, created_at DESC, id DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_method_created_at_id ON request_logs(method, created_at DESC, id DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_key_id_created_at_id ON request_logs(key_id, created_at DESC, id DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_account_id_created_at_id ON request_logs(account_id, created_at DESC, id DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_created_at_id ON request_logs(created_at DESC, id DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_trace_id_created_at_id ON request_logs(trace_id, created_at DESC, id DESC)",
            [],
        )?;
        self.ensure_request_logs_filter_indexes()?;
        Ok(())
    }

    fn ensure_request_logs_filter_indexes(&self) -> Result<()> {
        for (column, sql) in [
            (
                "model",
                "CREATE INDEX IF NOT EXISTS idx_request_logs_model_created_at_id ON request_logs(model, created_at DESC, id DESC)",
            ),
            (
                "request_type",
                "CREATE INDEX IF NOT EXISTS idx_request_logs_request_type_created_at_id ON request_logs(request_type, created_at DESC, id DESC)",
            ),
            (
                "gateway_mode",
                "CREATE INDEX IF NOT EXISTS idx_request_logs_gateway_mode_created_at_id ON request_logs(gateway_mode, created_at DESC, id DESC)",
            ),
            (
                "route_strategy",
                "CREATE INDEX IF NOT EXISTS idx_request_logs_route_strategy_created_at_id ON request_logs(route_strategy, created_at DESC, id DESC)",
            ),
            (
                "route_source",
                "CREATE INDEX IF NOT EXISTS idx_request_logs_route_source_created_at_id ON request_logs(route_source, created_at DESC, id DESC)",
            ),
            (
                "actual_source_id",
                "CREATE INDEX IF NOT EXISTS idx_request_logs_actual_source_id_created_at_id ON request_logs(actual_source_id, created_at DESC, id DESC)",
            ),
        ] {
            if self.has_column("request_logs", column)? {
                self.conn.execute(sql, [])?;
            }
        }
        Ok(())
    }

    /// 函数 `insert_request_log`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - log: 参数 log
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_request_log(&self, log: &RequestLog) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO request_logs (
                trace_id, key_id, account_id, initial_account_id, attempted_account_ids_json, initial_aggregate_api_id, attempted_aggregate_api_ids_json,
                request_path, original_path, adapted_path,
                method, request_type, gateway_mode, route_strategy, route_source, transparent_mode, enhanced_mode, client_model, model, model_source, upstream_model, actual_source_kind, actual_source_id, client_reasoning_effort, reasoning_effort, reasoning_source, service_tier, effective_service_tier, service_tier_source, response_adapter, upstream_url, aggregate_api_supplier_name, aggregate_api_url, status_code, duration_ms, first_response_ms, error, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38)",
            params![
                &log.trace_id,
                &log.key_id,
                &log.account_id,
                &log.initial_account_id,
                &log.attempted_account_ids_json,
                &log.initial_aggregate_api_id,
                &log.attempted_aggregate_api_ids_json,
                &log.request_path,
                &log.original_path,
                &log.adapted_path,
                &log.method,
                &log.request_type,
                &log.gateway_mode,
                &log.route_strategy,
                &log.route_source,
                log.transparent_mode,
                log.enhanced_mode,
                &log.client_model,
                &log.model,
                &log.model_source,
                &log.upstream_model,
                &log.actual_source_kind,
                &log.actual_source_id,
                &log.client_reasoning_effort,
                &log.reasoning_effort,
                &log.reasoning_source,
                &log.service_tier,
                &log.effective_service_tier,
                &log.service_tier_source,
                &log.response_adapter,
                &log.upstream_url,
                &log.aggregate_api_supplier_name,
                &log.aggregate_api_url,
                log.status_code,
                log.duration_ms,
                log.first_response_ms,
                &log.error,
                log.created_at,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 函数 `insert_request_log_with_token_stat`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - log: 参数 log
    /// - stat: 参数 stat
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_request_log_with_token_stat(
        &self,
        log: &RequestLog,
        stat: &RequestTokenStat,
    ) -> Result<(i64, Option<String>)> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO request_logs (
                trace_id, key_id, account_id, initial_account_id, attempted_account_ids_json, initial_aggregate_api_id, attempted_aggregate_api_ids_json,
                request_path, original_path, adapted_path,
                method, request_type, gateway_mode, route_strategy, route_source, transparent_mode, enhanced_mode, client_model, model, model_source, upstream_model, actual_source_kind, actual_source_id, client_reasoning_effort, reasoning_effort, reasoning_source, service_tier, effective_service_tier, service_tier_source, response_adapter, upstream_url, aggregate_api_supplier_name, aggregate_api_url, status_code, duration_ms, first_response_ms, error, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38)",
            params![
                &log.trace_id,
                &log.key_id,
                &log.account_id,
                &log.initial_account_id,
                &log.attempted_account_ids_json,
                &log.initial_aggregate_api_id,
                &log.attempted_aggregate_api_ids_json,
                &log.request_path,
                &log.original_path,
                &log.adapted_path,
                &log.method,
                &log.request_type,
                &log.gateway_mode,
                &log.route_strategy,
                &log.route_source,
                log.transparent_mode,
                log.enhanced_mode,
                &log.client_model,
                &log.model,
                &log.model_source,
                &log.upstream_model,
                &log.actual_source_kind,
                &log.actual_source_id,
                &log.client_reasoning_effort,
                &log.reasoning_effort,
                &log.reasoning_source,
                &log.service_tier,
                &log.effective_service_tier,
                &log.service_tier_source,
                &log.response_adapter,
                &log.upstream_url,
                &log.aggregate_api_supplier_name,
                &log.aggregate_api_url,
                log.status_code,
                log.duration_ms,
                log.first_response_ms,
                &log.error,
                log.created_at,
            ],
        )?;
        let request_log_id = tx.last_insert_rowid();

        // 中文注释：token 统计写入失败不应阻塞 request log 保留（例如 sqlite busy/锁竞争）。
        // 这里保持“单事务单提交”，但 stat 失败时仍 commit request log。
        let token_stat_error = tx
            .execute(
                "INSERT INTO request_token_stats (
                    request_log_id, key_id, account_id, model, actual_source_kind, actual_source_id,
                    input_tokens, cached_input_tokens, output_tokens, total_tokens, reasoning_output_tokens,
                    estimated_cost_usd, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                (
                    request_log_id,
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
            )
            .err()
            .map(|err| err.to_string());

        tx.commit()?;
        Ok((request_log_id, token_stat_error))
    }

    /// 函数 `list_request_logs`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - limit: 参数 limit
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_request_logs(&self, query: Option<&str>, limit: i64) -> Result<Vec<RequestLog>> {
        self.list_request_logs_paginated(query, None, None, None, 0, limit)
    }

    pub fn list_request_logs_for_keys(
        &self,
        query: Option<&str>,
        limit: i64,
        key_ids: &[String],
    ) -> Result<Vec<RequestLog>> {
        self.list_request_logs_paginated_for_keys(query, None, None, None, 0, limit, key_ids)
    }

    /// 函数 `list_request_logs_paginated`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - status_filter: 参数 status_filter
    /// - offset: 参数 offset
    /// - limit: 参数 limit
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_request_logs_paginated(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<RequestLog>> {
        let normalized_limit = normalize_request_log_limit(limit);
        if normalized_limit == 0 {
            return Ok(Vec::new());
        }
        let normalized_offset = offset.max(0);
        let include_account_lookup = self.has_table("accounts")?;
        let filters = build_request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            include_account_lookup,
            None,
            true,
        );
        self.list_request_logs_with_filter(filters, normalized_offset, normalized_limit)
    }

    pub fn list_request_logs_paginated_for_keys(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        offset: i64,
        limit: i64,
        key_ids: &[String],
    ) -> Result<Vec<RequestLog>> {
        let Some(key_filter) = KeyIdSqlFilter::create(self, "r.key_id", key_ids)? else {
            return Ok(Vec::new());
        };
        let normalized_limit = normalize_request_log_limit(limit);
        if normalized_limit == 0 {
            return Ok(Vec::new());
        }
        let normalized_offset = offset.max(0);
        let include_account_lookup = self.has_table("accounts")?;
        let filters = build_request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            include_account_lookup,
            Some(&key_filter),
            false,
        );
        self.list_request_logs_with_filter(filters, normalized_offset, normalized_limit)
    }

    fn list_request_logs_with_filter(
        &self,
        filters: RequestLogSqlFilters,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<RequestLog>> {
        let account_join = account_join_clause(filters.uses_account_lookup);
        let sql = if filters.uses_token_stats {
            format!(
                "SELECT
                r.trace_id, r.key_id, r.account_id, r.initial_account_id, r.attempted_account_ids_json, r.initial_aggregate_api_id, r.attempted_aggregate_api_ids_json,
                r.request_path, r.original_path, r.adapted_path,
                r.method, r.request_type, r.gateway_mode, r.route_strategy, r.route_source, r.transparent_mode, r.enhanced_mode, r.client_model, r.model, r.model_source, r.upstream_model, r.actual_source_kind, r.actual_source_id, r.client_reasoning_effort, r.reasoning_effort, r.reasoning_source, r.service_tier, r.effective_service_tier, r.service_tier_source, r.response_adapter, r.upstream_url, r.aggregate_api_supplier_name, r.aggregate_api_url, r.status_code, r.duration_ms, r.first_response_ms,
                t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                r.error, r.created_at
             FROM request_logs r
             {account_join}
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             {where_clause}
             ORDER BY r.created_at DESC, r.id DESC
             LIMIT ? OFFSET ?",
                account_join = account_join,
                where_clause = filters.where_clause
            )
        } else {
            format!(
                "WITH page_ids AS (
                    SELECT r.id
                    FROM request_logs r
                    {account_join}
                    {where_clause}
                    ORDER BY r.created_at DESC, r.id DESC
                    LIMIT ? OFFSET ?
                )
                SELECT
                    r.trace_id, r.key_id, r.account_id, r.initial_account_id, r.attempted_account_ids_json, r.initial_aggregate_api_id, r.attempted_aggregate_api_ids_json,
                    r.request_path, r.original_path, r.adapted_path,
                    r.method, r.request_type, r.gateway_mode, r.route_strategy, r.route_source, r.transparent_mode, r.enhanced_mode, r.client_model, r.model, r.model_source, r.upstream_model, r.actual_source_kind, r.actual_source_id, r.client_reasoning_effort, r.reasoning_effort, r.reasoning_source, r.service_tier, r.effective_service_tier, r.service_tier_source, r.response_adapter, r.upstream_url, r.aggregate_api_supplier_name, r.aggregate_api_url, r.status_code, r.duration_ms, r.first_response_ms,
                    t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                    r.error, r.created_at
                 FROM page_ids p
                 JOIN request_logs r ON r.id = p.id
                 LEFT JOIN request_token_stats t ON t.request_log_id = r.id
                 ORDER BY r.created_at DESC, r.id DESC",
                account_join = account_join,
                where_clause = filters.where_clause
            )
        };
        let mut params = filters.params;
        params.push(Value::Integer(limit));
        params.push(Value::Integer(offset));

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_request_log_row(row)?);
        }
        Ok(out)
    }

    /// 函数 `count_request_logs`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - status_filter: 参数 status_filter
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn count_request_logs(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    ) -> Result<i64> {
        let include_account_lookup = self.has_table("accounts")?;
        let filters = build_request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            include_account_lookup,
            None,
            true,
        );
        let sql = format!(
            "SELECT COUNT(1)
             FROM request_logs r
             {account_join}
             {token_stats_join}
             {where_clause}",
            account_join = account_join_clause(filters.uses_account_lookup),
            token_stats_join = token_stats_join_clause(filters.uses_token_stats),
            where_clause = filters.where_clause
        );
        self.conn
            .query_row(&sql, params_from_iter(filters.params.iter()), |row| {
                row.get(0)
            })
    }

    pub fn count_request_logs_for_keys(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
    ) -> Result<i64> {
        let Some(key_filter) = KeyIdSqlFilter::create(self, "r.key_id", key_ids)? else {
            return Ok(0);
        };
        let include_account_lookup = self.has_table("accounts")?;
        let filters = build_request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            include_account_lookup,
            Some(&key_filter),
            false,
        );
        let sql = format!(
            "SELECT COUNT(1)
             FROM request_logs r
             {account_join}
             {token_stats_join}
             {where_clause}",
            account_join = account_join_clause(filters.uses_account_lookup),
            token_stats_join = token_stats_join_clause(filters.uses_token_stats),
            where_clause = filters.where_clause
        );
        self.conn
            .query_row(&sql, params_from_iter(filters.params.iter()), |row| {
                row.get(0)
            })
    }

    /// 函数 `summarize_request_logs_filtered`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - status_filter: 参数 status_filter
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn summarize_request_logs_filtered(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
    ) -> Result<RequestLogQuerySummary> {
        let include_account_lookup = self.has_table("accounts")?;
        let filters = build_request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            include_account_lookup,
            None,
            true,
        );
        let sql = format!(
            "SELECT
                COUNT(1),
                IFNULL(SUM(CASE WHEN r.status_code >= 200 AND r.status_code <= 299 THEN 1 ELSE 0 END), 0),
                IFNULL(SUM(CASE WHEN IFNULL(r.status_code, 0) >= 400 OR TRIM(IFNULL(r.error, '')) <> '' THEN 1 ELSE 0 END), 0),
                IFNULL(SUM(
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
                ), 0),
                IFNULL(SUM(IFNULL(t.estimated_cost_usd, 0.0)), 0.0)
             FROM request_logs r
             {account_join}
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             {where_clause}",
            account_join = account_join_clause(filters.uses_account_lookup),
            where_clause = filters.where_clause
        );
        self.conn
            .query_row(&sql, params_from_iter(filters.params.iter()), |row| {
                Ok(RequestLogQuerySummary {
                    count: row.get(0)?,
                    success_count: row.get(1)?,
                    error_count: row.get(2)?,
                    total_tokens: row.get(3)?,
                    estimated_cost_usd: row.get(4)?,
                })
            })
    }

    pub fn summarize_request_logs_filtered_for_keys(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
    ) -> Result<RequestLogQuerySummary> {
        let Some(key_filter) = KeyIdSqlFilter::create(self, "r.key_id", key_ids)? else {
            return Ok(empty_request_log_query_summary());
        };
        let include_account_lookup = self.has_table("accounts")?;
        let filters = build_request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            include_account_lookup,
            Some(&key_filter),
            false,
        );
        let sql = format!(
            "SELECT
                COUNT(1),
                IFNULL(SUM(CASE WHEN r.status_code >= 200 AND r.status_code <= 299 THEN 1 ELSE 0 END), 0),
                IFNULL(SUM(CASE WHEN IFNULL(r.status_code, 0) >= 400 OR TRIM(IFNULL(r.error, '')) <> '' THEN 1 ELSE 0 END), 0),
                IFNULL(SUM(
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
                ), 0),
                IFNULL(SUM(IFNULL(t.estimated_cost_usd, 0.0)), 0.0)
             FROM request_logs r
             {account_join}
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             {where_clause}",
            account_join = account_join_clause(filters.uses_account_lookup),
            where_clause = filters.where_clause
        );
        self.conn
            .query_row(&sql, params_from_iter(filters.params.iter()), |row| {
                Ok(RequestLogQuerySummary {
                    count: row.get(0)?,
                    success_count: row.get(1)?,
                    error_count: row.get(2)?,
                    total_tokens: row.get(3)?,
                    estimated_cost_usd: row.get(4)?,
                })
            })
    }

    /// 函数 `clear_request_logs`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn clear_request_logs(&self) -> Result<()> {
        // 中文注释：先把状态计数写入 hourly rollup，再移除可浏览请求明细，避免清日志后仪表盘成功率丢失。
        self.rollup_all_request_token_stats()?;
        self.conn.execute("DELETE FROM request_logs", [])?;
        let _ = self
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        Ok(())
    }

    pub fn prune_request_logs_before(&self, cutoff_ts: i64) -> Result<usize> {
        self.rollup_request_token_stats_before(cutoff_ts)?;
        self.conn.execute(
            "DELETE FROM request_logs WHERE created_at < ?1",
            [cutoff_ts],
        )
    }

    pub fn prune_request_logs_by_retention(&self, now: i64) -> Result<usize> {
        let days = request_log_retention_days();
        if days <= 0 {
            return Ok(0);
        }
        let cutoff = now.saturating_sub(days.saturating_mul(86_400));
        self.prune_request_logs_before(cutoff)
    }

    /// 函数 `summarize_request_logs_between`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - start_ts: 参数 start_ts
    /// - end_ts: 参数 end_ts
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn summarize_request_logs_between(
        &self,
        start_ts: i64,
        end_ts: i64,
    ) -> Result<RequestLogTodaySummary> {
        self.summarize_request_token_stats_between(start_ts, end_ts)
    }

    pub fn summarize_request_logs_between_for_keys(
        &self,
        start_ts: i64,
        end_ts: i64,
        key_ids: &[String],
    ) -> Result<RequestLogTodaySummary> {
        let Some(raw_key_filter) = KeyIdSqlFilter::create(self, "s.key_id", key_ids)? else {
            return Ok(empty_request_log_today_summary());
        };
        let Some(hourly_key_filter) = KeyIdSqlFilter::create(self, "h.key_id", key_ids)? else {
            return Ok(empty_request_log_today_summary());
        };
        let sql = format!(
            "WITH combined AS (
                SELECT
                    IFNULL(SUM(s.input_tokens), 0) AS input_tokens,
                    IFNULL(SUM(s.cached_input_tokens), 0) AS cached_input_tokens,
                    IFNULL(SUM(s.output_tokens), 0) AS output_tokens,
                    IFNULL(SUM(s.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                    IFNULL(SUM(s.estimated_cost_usd), 0.0) AS estimated_cost_usd
                FROM request_token_stats s
                WHERE s.created_at >= ?
                  AND s.created_at < ?
                  AND {raw_key_condition}
                UNION ALL
                SELECT
                    IFNULL(SUM(h.input_tokens), 0) AS input_tokens,
                    IFNULL(SUM(h.cached_input_tokens), 0) AS cached_input_tokens,
                    IFNULL(SUM(h.output_tokens), 0) AS output_tokens,
                    IFNULL(SUM(h.reasoning_output_tokens), 0) AS reasoning_output_tokens,
                    IFNULL(SUM(h.estimated_cost_usd), 0.0) AS estimated_cost_usd
                FROM request_token_stat_hourly_rollups h
                WHERE h.bucket_start >= ?
                  AND h.bucket_end <= ?
                  AND {hourly_key_condition}
             )
             SELECT
                IFNULL(SUM(input_tokens), 0),
                IFNULL(SUM(cached_input_tokens), 0),
                IFNULL(SUM(output_tokens), 0),
                IFNULL(SUM(reasoning_output_tokens), 0),
                IFNULL(SUM(estimated_cost_usd), 0.0)
             FROM combined",
            raw_key_condition = raw_key_filter.condition(),
            hourly_key_condition = hourly_key_filter.condition()
        );
        let mut params = Vec::with_capacity(
            raw_key_filter.params().len() + hourly_key_filter.params().len() + 4,
        );
        params.push(Value::Integer(start_ts));
        params.push(Value::Integer(end_ts));
        params.extend_from_slice(raw_key_filter.params());
        params.push(Value::Integer(start_ts));
        params.push(Value::Integer(end_ts));
        params.extend_from_slice(hourly_key_filter.params());
        self.conn.query_row(&sql, params_from_iter(params), |row| {
            Ok(RequestLogTodaySummary {
                input_tokens: row.get(0)?,
                cached_input_tokens: row.get(1)?,
                output_tokens: row.get(2)?,
                reasoning_output_tokens: row.get(3)?,
                estimated_cost_usd: row.get(4)?,
            })
        })
    }

    /// 函数 `ensure_request_logs_table`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_logs_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id TEXT,
                key_id TEXT,
                account_id TEXT,
                initial_account_id TEXT,
                attempted_account_ids_json TEXT,
                initial_aggregate_api_id TEXT,
                attempted_aggregate_api_ids_json TEXT,
                request_path TEXT NOT NULL,
                original_path TEXT,
                adapted_path TEXT,
                method TEXT NOT NULL,
                request_type TEXT,
                gateway_mode TEXT,
                route_strategy TEXT,
                route_source TEXT,
                transparent_mode INTEGER,
                enhanced_mode INTEGER,
                client_model TEXT,
                model TEXT,
                model_source TEXT,
                upstream_model TEXT,
                actual_source_kind TEXT,
                actual_source_id TEXT,
                client_reasoning_effort TEXT,
                reasoning_effort TEXT,
                reasoning_source TEXT,
                service_tier TEXT,
                effective_service_tier TEXT,
                service_tier_source TEXT,
                response_adapter TEXT,
                upstream_url TEXT,
                aggregate_api_supplier_name TEXT,
                aggregate_api_url TEXT,
                status_code INTEGER,
                duration_ms INTEGER,
                first_response_ms INTEGER,
                error TEXT,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.ensure_request_logs_indexes()?;
        Ok(())
    }

    pub(super) fn ensure_request_log_route_detail_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "upstream_model", "TEXT")?;
        self.ensure_column("request_logs", "actual_source_kind", "TEXT")?;
        self.ensure_column("request_logs", "actual_source_id", "TEXT")?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_actual_source_created_at ON request_logs(actual_source_kind, actual_source_id, created_at DESC)",
            [],
        )?;
        Ok(())
    }

    /// 函数 `ensure_request_log_reasoning_column`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_reasoning_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "reasoning_effort", "TEXT")?;
        Ok(())
    }

    /// 函数 `ensure_request_log_account_tokens_cost_columns`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_account_tokens_cost_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "account_id", "TEXT")?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_account_id_created_at_id ON request_logs(account_id, created_at DESC, id DESC)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_created_at_id ON request_logs(created_at DESC, id DESC)",
            [],
        )?;
        Ok(())
    }

    /// 函数 `ensure_request_log_cached_reasoning_columns`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_cached_reasoning_columns(&self) -> Result<()> {
        Ok(())
    }

    /// 函数 `ensure_request_log_trace_context_columns`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_trace_context_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "trace_id", "TEXT")?;
        self.ensure_column("request_logs", "original_path", "TEXT")?;
        self.ensure_column("request_logs", "adapted_path", "TEXT")?;
        self.ensure_column("request_logs", "response_adapter", "TEXT")?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_request_logs_trace_id_created_at_id ON request_logs(trace_id, created_at DESC, id DESC)",
            [],
        )?;
        Ok(())
    }

    /// 函数 `ensure_request_log_aggregate_api_context_columns`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_aggregate_api_context_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "aggregate_api_supplier_name", "TEXT")?;
        self.ensure_column("request_logs", "aggregate_api_url", "TEXT")?;
        Ok(())
    }

    /// 函数 `ensure_request_log_attempt_chain_columns`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_attempt_chain_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "initial_account_id", "TEXT")?;
        self.ensure_column("request_logs", "attempted_account_ids_json", "TEXT")?;
        Ok(())
    }

    /// 函数 `ensure_request_log_aggregate_api_attempt_chain_columns`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_aggregate_api_attempt_chain_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "initial_aggregate_api_id", "TEXT")?;
        self.ensure_column("request_logs", "attempted_aggregate_api_ids_json", "TEXT")?;
        Ok(())
    }

    /// 函数 `ensure_request_log_duration_column`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_request_log_duration_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "duration_ms", "INTEGER")?;
        Ok(())
    }

    pub(super) fn ensure_request_log_first_response_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "first_response_ms", "INTEGER")?;
        Ok(())
    }

    pub(super) fn ensure_request_log_request_type_and_service_tier_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "request_type", "TEXT")?;
        self.ensure_column("request_logs", "gateway_mode", "TEXT")?;
        self.ensure_column("request_logs", "transparent_mode", "INTEGER")?;
        self.ensure_column("request_logs", "enhanced_mode", "INTEGER")?;
        self.ensure_column("request_logs", "service_tier", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_request_log_route_strategy_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "route_strategy", "TEXT")?;
        self.ensure_column("request_logs", "route_source", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_request_log_effective_service_tier_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "effective_service_tier", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_request_log_service_tier_source_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "service_tier_source", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_request_log_model_reasoning_source_columns(&self) -> Result<()> {
        self.ensure_column("request_logs", "client_model", "TEXT")?;
        self.ensure_column("request_logs", "model_source", "TEXT")?;
        self.ensure_column("request_logs", "client_reasoning_effort", "TEXT")?;
        self.ensure_column("request_logs", "reasoning_source", "TEXT")?;
        Ok(())
    }

    /// 函数 `compact_request_logs_legacy_usage_columns`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - super: 参数 super
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn compact_request_logs_legacy_usage_columns(&self) -> Result<()> {
        self.ensure_request_logs_table()?;
        self.ensure_request_log_reasoning_column()?;
        self.ensure_request_log_account_tokens_cost_columns()?;
        self.ensure_request_log_trace_context_columns()?;

        let legacy_columns = [
            "input_tokens",
            "output_tokens",
            "estimated_cost_usd",
            "cached_input_tokens",
            "reasoning_output_tokens",
        ];
        let mut has_legacy_columns = false;
        for column in legacy_columns {
            if self.has_column("request_logs", column)? {
                has_legacy_columns = true;
                break;
            }
        }
        if !has_legacy_columns {
            return Ok(());
        }

        let tx = self.conn.unchecked_transaction()?;
        tx.execute_batch(
            "ALTER TABLE request_logs RENAME TO request_logs_legacy_028;
             CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id TEXT,
                key_id TEXT,
                account_id TEXT,
                initial_account_id TEXT,
                attempted_account_ids_json TEXT,
                initial_aggregate_api_id TEXT,
                attempted_aggregate_api_ids_json TEXT,
                request_path TEXT NOT NULL,
                original_path TEXT,
                adapted_path TEXT,
                method TEXT NOT NULL,
                request_type TEXT,
                gateway_mode TEXT,
                route_strategy TEXT,
                route_source TEXT,
                transparent_mode INTEGER,
                enhanced_mode INTEGER,
                client_model TEXT,
                model TEXT,
                model_source TEXT,
                upstream_model TEXT,
                actual_source_kind TEXT,
                actual_source_id TEXT,
                client_reasoning_effort TEXT,
                reasoning_effort TEXT,
                reasoning_source TEXT,
                service_tier TEXT,
                effective_service_tier TEXT,
                service_tier_source TEXT,
                response_adapter TEXT,
                upstream_url TEXT,
                aggregate_api_supplier_name TEXT,
                aggregate_api_url TEXT,
                status_code INTEGER,
                duration_ms INTEGER,
                first_response_ms INTEGER,
                error TEXT,
                created_at INTEGER NOT NULL
             );
             INSERT INTO request_logs (
                id, trace_id, key_id, account_id, initial_account_id, attempted_account_ids_json, initial_aggregate_api_id, attempted_aggregate_api_ids_json,
                request_path, original_path, adapted_path,
                method, request_type, gateway_mode, route_strategy, route_source, transparent_mode, enhanced_mode, client_model, model, model_source, upstream_model, actual_source_kind, actual_source_id, client_reasoning_effort, reasoning_effort, reasoning_source, service_tier, effective_service_tier, service_tier_source, response_adapter, upstream_url, aggregate_api_supplier_name, aggregate_api_url, status_code, duration_ms, first_response_ms, error, created_at
             )
             SELECT
                id, trace_id, key_id, account_id, NULL, NULL, NULL, NULL, request_path, original_path, adapted_path,
                method, NULL, NULL, NULL, NULL, NULL, NULL, NULL, model, NULL, NULL, NULL, NULL, NULL, reasoning_effort, NULL, NULL, NULL, NULL, response_adapter, upstream_url, NULL, NULL, status_code, NULL, NULL, error, created_at
             FROM request_logs_legacy_028;
             DROP TABLE request_logs_legacy_028;",
        )?;
        tx.commit()?;

        self.ensure_request_logs_indexes()?;
        Ok(())
    }
}

/// 函数 `map_request_log_row`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - row: 参数 row
///
/// # 返回
/// 返回函数执行结果
fn map_request_log_row(row: &Row<'_>) -> Result<RequestLog> {
    Ok(RequestLog {
        trace_id: row.get(0)?,
        key_id: row.get(1)?,
        account_id: row.get(2)?,
        initial_account_id: row.get(3)?,
        attempted_account_ids_json: row.get(4)?,
        initial_aggregate_api_id: row.get(5)?,
        attempted_aggregate_api_ids_json: row.get(6)?,
        request_path: row.get(7)?,
        original_path: row.get(8)?,
        adapted_path: row.get(9)?,
        method: row.get(10)?,
        request_type: row.get(11)?,
        gateway_mode: row.get(12)?,
        route_strategy: row.get(13)?,
        route_source: row.get(14)?,
        transparent_mode: row.get(15)?,
        enhanced_mode: row.get(16)?,
        client_model: row.get(17)?,
        model: row.get(18)?,
        model_source: row.get(19)?,
        upstream_model: row.get(20)?,
        actual_source_kind: row.get(21)?,
        actual_source_id: row.get(22)?,
        client_reasoning_effort: row.get(23)?,
        reasoning_effort: row.get(24)?,
        reasoning_source: row.get(25)?,
        service_tier: row.get(26)?,
        effective_service_tier: row.get(27)?,
        service_tier_source: row.get(28)?,
        response_adapter: row.get(29)?,
        upstream_url: row.get(30)?,
        aggregate_api_supplier_name: row.get(31)?,
        aggregate_api_url: row.get(32)?,
        status_code: row.get(33)?,
        duration_ms: row.get(34)?,
        first_response_ms: row.get(35)?,
        input_tokens: row.get(36)?,
        cached_input_tokens: row.get(37)?,
        output_tokens: row.get(38)?,
        total_tokens: row.get(39)?,
        reasoning_output_tokens: row.get(40)?,
        estimated_cost_usd: row.get(41)?,
        error: row.get(42)?,
        created_at: row.get(43)?,
    })
}

/// 函数 `normalize_request_log_limit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_request_log_limit(value: i64) -> i64 {
    if value < 0 {
        200
    } else if value == 0 {
        0
    } else {
        value.min(1000)
    }
}

/// 函数 `build_request_log_filters`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - query: 参数 query
/// - status_filter: 参数 status_filter
///
/// # 返回
/// 返回函数执行结果
fn empty_request_log_today_summary() -> RequestLogTodaySummary {
    RequestLogTodaySummary {
        input_tokens: 0,
        cached_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        estimated_cost_usd: 0.0,
    }
}

fn empty_request_log_query_summary() -> RequestLogQuerySummary {
    RequestLogQuerySummary {
        count: 0,
        success_count: 0,
        error_count: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
    }
}

#[cfg(test)]
#[path = "tests/request_logs_tests.rs"]
mod tests;
