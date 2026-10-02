use rusqlite::{params, params_from_iter, types::Value, OptionalExtension, Result, Row};

use super::key_id_filters::KeyIdSqlFilter;
use super::request_log_filters::{
    account_join_clause, build_request_log_filters, token_stats_join_clause, RequestLogSqlFilters,
};
use super::{
    now_ts, RequestLog, RequestLogPayload, RequestLogQuerySummary, RequestLogTodaySummary,
    RequestTokenStat, Storage,
};

const DEFAULT_REQUEST_LOG_RETENTION_DAYS: i64 = 14;
const REQUEST_LOG_RETENTION_DAYS_ENV: &str = "CODEXMANAGER_REQUEST_LOG_RETENTION_DAYS";
const REQUEST_LOG_LIST_SELECT_COLUMNS: &str = "r.trace_id, r.key_id, r.account_id, r.initial_account_id, r.attempted_account_ids_json, r.initial_aggregate_api_id, r.attempted_aggregate_api_ids_json,
                r.request_path, r.original_path, r.adapted_path,
                r.method, r.request_type, r.gateway_mode, r.route_strategy, r.route_source, r.transparent_mode, r.enhanced_mode, r.client_model, r.model, r.model_source, r.upstream_model, r.actual_source_kind, r.actual_source_id, r.client_reasoning_effort, r.reasoning_effort, r.reasoning_source, r.service_tier, r.effective_service_tier, r.service_tier_source, r.response_adapter, r.upstream_url, r.aggregate_api_supplier_name, r.aggregate_api_url, r.status_code, r.duration_ms, r.first_response_ms,
                t.input_tokens, t.cached_input_tokens, t.output_tokens, t.total_tokens, t.reasoning_output_tokens, t.estimated_cost_usd,
                r.error, r.created_at";

fn request_log_retention_days() -> i64 {
    std::env::var(REQUEST_LOG_RETENTION_DAYS_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_REQUEST_LOG_RETENTION_DAYS)
}

fn empty_optional_range(start_ts: Option<i64>, end_ts: Option<i64>) -> bool {
    matches!((start_ts, end_ts), (Some(start), Some(end)) if end <= start)
}

fn clear_request_logs_sql() -> &'static str {
    "DELETE FROM request_logs
     WHERE cleared_at IS NULL
       AND NOT EXISTS (
       SELECT 1 FROM request_charge_snapshots snapshots
       WHERE snapshots.request_log_id=request_logs.id
     )"
}

fn hide_billed_request_logs_sql() -> &'static str {
    "UPDATE request_logs
     SET cleared_at = ?1
     WHERE cleared_at IS NULL
       AND EXISTS (
         SELECT 1 FROM request_charge_snapshots snapshots
         WHERE snapshots.request_log_id=request_logs.id
       )"
}

fn prune_request_logs_before_sql() -> &'static str {
    "DELETE FROM request_logs
     WHERE cleared_at IS NULL
       AND created_at < ?1
       AND NOT EXISTS (
         SELECT 1 FROM request_charge_snapshots snapshots
         WHERE snapshots.request_log_id=request_logs.id
       )"
}

fn hide_billed_request_logs_before_sql() -> &'static str {
    "UPDATE request_logs
     SET cleared_at = ?2
     WHERE cleared_at IS NULL
       AND created_at < ?1
       AND EXISTS (
         SELECT 1 FROM request_charge_snapshots snapshots
         WHERE snapshots.request_log_id=request_logs.id
       )"
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
    /// Upsert a request payload preview for a gateway trace and stage.
    /// Payloads are pure diagnostics: a missing `request_log_payloads` table
    /// (pre-migration database) or a write failure must never break the
    /// request hot path, so callers treat errors as best-effort.
    pub fn insert_request_log_payload(&self, payload: &RequestLogPayload) -> Result<()> {
        self.conn.execute(
            "INSERT INTO request_log_payloads (
                trace_id, stage, payload, payload_bytes, payload_truncated, redacted,
                body_hash, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(trace_id, stage) DO UPDATE SET
                payload=excluded.payload,
                payload_bytes=excluded.payload_bytes,
                payload_truncated=excluded.payload_truncated,
                redacted=excluded.redacted,
                body_hash=excluded.body_hash,
                created_at=excluded.created_at",
            params![
                payload.trace_id,
                payload.stage,
                payload.payload,
                payload.payload_bytes,
                payload.payload_truncated,
                payload.redacted,
                payload.body_hash,
                payload.created_at,
            ],
        )?;
        Ok(())
    }

    /// Load the stored payload preview for a trace. Returns `None` when the
    /// table has not been created yet or no preview was captured.
    pub fn find_request_log_payload_by_trace_id(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<RequestLogPayload>> {
        if !self.has_table("request_log_payloads")? {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT trace_id, stage, payload, payload_bytes, payload_truncated, redacted,
                        body_hash, created_at
                 FROM request_log_payloads WHERE trace_id = ?1 AND stage = ?2",
                (trace_id, stage),
                |row| {
                    Ok(RequestLogPayload {
                        trace_id: row.get(0)?,
                        stage: row.get(1)?,
                        payload: row.get(2)?,
                        payload_bytes: row.get(3)?,
                        payload_truncated: row.get(4)?,
                        redacted: row.get(5)?,
                        body_hash: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )
            .optional()
    }

    /// Stages that have a stored preview for this trace, in capture order
    /// (client before upstream).
    pub fn list_request_log_payload_stages(&self, trace_id: &str) -> Result<Vec<String>> {
        if !self.has_table("request_log_payloads")? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT stage FROM request_log_payloads WHERE trace_id = ?1 ORDER BY stage ASC",
        )?;
        let rows = stmt.query_map([trace_id], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    /// Stored body hash of one stage, used to skip the upstream capture when
    /// the gateway forwarded the client body unchanged.
    pub fn find_request_log_payload_body_hash(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<String>> {
        if !self.has_table("request_log_payloads")? {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT body_hash FROM request_log_payloads WHERE trace_id = ?1 AND stage = ?2",
                (trace_id, stage),
                |row| row.get::<_, String>(0),
            )
            .optional()
    }

    /// Resolve the owning API key of a logged request for member-scope
    /// authorization. `Ok(None)` means no log row exists for the trace.
    pub fn find_request_log_key_id_by_trace_id(
        &self,
        trace_id: &str,
    ) -> Result<Option<Option<String>>> {
        self.conn
            .query_row(
                "SELECT key_id FROM request_logs WHERE trace_id = ?1 ORDER BY id DESC LIMIT 1",
                [trace_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
    }

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
                    estimated_cost_usd, usage_included, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                    i64::from(
                        log.status_code
                            .is_some_and(|status| (200..=299).contains(&status)),
                    ),
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
        if empty_optional_range(start_ts, end_ts) {
            return Ok(Vec::new());
        }
        let normalized_offset = offset.max(0);
        let filters =
            self.request_log_filters(query, status_filter, start_ts, end_ts, None, true)?;
        self.list_request_logs_with_filter(filters, normalized_offset, normalized_limit)
    }

    #[allow(clippy::too_many_arguments)]
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
        let normalized_limit = normalize_request_log_limit(limit);
        if normalized_limit == 0 {
            return Ok(Vec::new());
        }
        if empty_optional_range(start_ts, end_ts) {
            return Ok(Vec::new());
        }
        let Some(key_filter) = KeyIdSqlFilter::create(self, "r.key_id", key_ids)? else {
            return Ok(Vec::new());
        };
        let normalized_offset = offset.max(0);
        let filters = self.request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            Some(&key_filter),
            false,
        )?;
        self.list_request_logs_with_filter(filters, normalized_offset, normalized_limit)
    }

    fn list_request_logs_with_filter(
        &self,
        filters: RequestLogSqlFilters,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<RequestLog>> {
        let sql = request_log_list_sql(&filters);
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
        if empty_optional_range(start_ts, end_ts) {
            return Ok(0);
        }
        let filters =
            self.request_log_filters(query, status_filter, start_ts, end_ts, None, true)?;
        let sql = request_log_count_sql(&filters);
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
        if empty_optional_range(start_ts, end_ts) {
            return Ok(0);
        }
        let Some(key_filter) = KeyIdSqlFilter::create(self, "r.key_id", key_ids)? else {
            return Ok(0);
        };
        let filters = self.request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            Some(&key_filter),
            false,
        )?;
        let sql = request_log_count_sql(&filters);
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
        if empty_optional_range(start_ts, end_ts) {
            return Ok(empty_request_log_query_summary());
        }
        let filters =
            self.request_log_filters(query, status_filter, start_ts, end_ts, None, true)?;
        self.summarize_request_logs_with_filter(filters)
    }

    pub fn summarize_request_logs_filtered_for_keys(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_ids: &[String],
    ) -> Result<RequestLogQuerySummary> {
        if empty_optional_range(start_ts, end_ts) {
            return Ok(empty_request_log_query_summary());
        }
        let Some(key_filter) = KeyIdSqlFilter::create(self, "r.key_id", key_ids)? else {
            return Ok(empty_request_log_query_summary());
        };
        let filters = self.request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            Some(&key_filter),
            false,
        )?;
        self.summarize_request_logs_with_filter(filters)
    }

    fn request_log_filters(
        &self,
        query: Option<&str>,
        status_filter: Option<&str>,
        start_ts: Option<i64>,
        end_ts: Option<i64>,
        key_filter: Option<&KeyIdSqlFilter<'_>>,
        include_route_detail_fields: bool,
    ) -> Result<RequestLogSqlFilters> {
        let include_account_lookup = self.has_table("accounts")?;
        Ok(build_request_log_filters(
            query,
            status_filter,
            start_ts,
            end_ts,
            include_account_lookup,
            key_filter,
            include_route_detail_fields,
        ))
    }

    fn summarize_request_logs_with_filter(
        &self,
        filters: RequestLogSqlFilters,
    ) -> Result<RequestLogQuerySummary> {
        let sql = request_log_summary_sql(&filters);
        self.conn
            .query_row(&sql, params_from_iter(filters.params.iter()), |row| {
                map_request_log_query_summary_row(row)
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
        let rolled_up = self.rollup_all_request_token_stats()?;
        // Migration 062 runs before the V2 charge snapshot table is created. Keep that
        // fresh/legacy migration path valid while preserving immutable billed logs once
        // the V2 schema exists.
        let affected_logs = if self.has_table("request_charge_snapshots")? {
            let cleared_at = now_ts();
            let hidden_logs = self
                .conn
                .execute(hide_billed_request_logs_sql(), [cleared_at])?;
            let deleted_logs = self.conn.execute(clear_request_logs_sql(), [])?;
            hidden_logs.saturating_add(deleted_logs)
        } else {
            self.conn.execute("DELETE FROM request_logs", [])?
        };
        self.clear_request_log_payloads()?;
        if rolled_up.saturating_add(affected_logs) > 0 {
            let _ = self
                .conn
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        }
        Ok(())
    }

    /// Remove every stored request payload preview. Payloads are plain
    /// diagnostics without billing semantics, so clearing never preserves
    /// rows the way billed request logs are hidden instead of deleted.
    pub fn clear_request_log_payloads(&self) -> Result<usize> {
        let mut removed = self.clear_request_log_payload_store()?;
        if self.has_table("request_log_payloads")? {
            removed =
                removed.saturating_add(self.conn.execute("DELETE FROM request_log_payloads", [])?);
        }
        Ok(removed)
    }

    pub fn prune_request_logs_before(&self, cutoff_ts: i64) -> Result<usize> {
        if cutoff_ts <= 0 {
            return Ok(0);
        }
        self.rollup_request_token_stats_before(cutoff_ts)?;
        if self.has_table("request_log_payloads")? {
            self.conn.execute(
                "DELETE FROM request_log_payloads WHERE created_at < ?1",
                [cutoff_ts],
            )?;
        }
        self.prune_request_log_payload_store_before(cutoff_ts)?;
        if self.has_table("request_charge_snapshots")? {
            let hidden_logs = self
                .conn
                .execute(hide_billed_request_logs_before_sql(), [cutoff_ts, now_ts()])?;
            let deleted_logs = self
                .conn
                .execute(prune_request_logs_before_sql(), [cutoff_ts])?;
            Ok(hidden_logs.saturating_add(deleted_logs))
        } else {
            self.conn.execute(
                "DELETE FROM request_logs WHERE created_at < ?1",
                [cutoff_ts],
            )
        }
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
        self.summarize_request_token_stats_between_for_keys(start_ts, end_ts, key_ids)
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
                cleared_at INTEGER,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.ensure_request_logs_indexes()?;
        Ok(())
    }

    pub(super) fn ensure_request_log_visibility_column(&self) -> Result<()> {
        self.ensure_column("request_logs", "cleared_at", "INTEGER")?;
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

fn request_log_list_sql(filters: &RequestLogSqlFilters) -> String {
    let account_join = account_join_clause(filters.uses_account_lookup);
    if filters.uses_token_stats {
        return format!(
            "SELECT
                {select_columns}
             FROM request_logs r
             {account_join}
             LEFT JOIN request_token_stats t ON t.request_log_id = r.id
             {where_clause}
             ORDER BY r.created_at DESC, r.id DESC
             LIMIT ? OFFSET ?",
            select_columns = REQUEST_LOG_LIST_SELECT_COLUMNS,
            account_join = account_join,
            where_clause = filters.where_clause
        );
    }

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
            {select_columns}
         FROM page_ids p
         JOIN request_logs r ON r.id = p.id
         LEFT JOIN request_token_stats t ON t.request_log_id = r.id
         ORDER BY r.created_at DESC, r.id DESC",
        select_columns = REQUEST_LOG_LIST_SELECT_COLUMNS,
        account_join = account_join,
        where_clause = filters.where_clause
    )
}

fn request_log_count_sql(filters: &RequestLogSqlFilters) -> String {
    format!(
        "SELECT COUNT(1)
         FROM request_logs r
         {account_join}
         {token_stats_join}
         {where_clause}",
        account_join = account_join_clause(filters.uses_account_lookup),
        token_stats_join = token_stats_join_clause(filters.uses_token_stats),
        where_clause = filters.where_clause
    )
}

fn request_log_summary_sql(filters: &RequestLogSqlFilters) -> String {
    format!(
        "SELECT
            COUNT(1),
            IFNULL(SUM(CASE WHEN r.status_code >= 200 AND r.status_code <= 299 THEN 1 ELSE 0 END), 0),
            IFNULL(SUM(CASE WHEN IFNULL(r.status_code, 0) >= 400 OR TRIM(IFNULL(r.error, '')) <> '' THEN 1 ELSE 0 END), 0),
            IFNULL(SUM(
                CASE
                    WHEN t.usage_included <> 1 THEN 0
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
            IFNULL(SUM(
                CASE
                    WHEN t.usage_included = 1
                        THEN IFNULL(t.estimated_cost_usd, 0.0)
                    ELSE 0.0
                END
            ), 0.0)
         FROM request_logs r
         {account_join}
         LEFT JOIN request_token_stats t ON t.request_log_id = r.id
         {where_clause}",
        account_join = account_join_clause(filters.uses_account_lookup),
        where_clause = filters.where_clause,
    )
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

fn map_request_log_query_summary_row(row: &Row<'_>) -> Result<RequestLogQuerySummary> {
    Ok(RequestLogQuerySummary {
        count: row.get(0)?,
        success_count: row.get(1)?,
        error_count: row.get(2)?,
        total_tokens: row.get(3)?,
        estimated_cost_usd: row.get(4)?,
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
fn empty_request_log_query_summary() -> RequestLogQuerySummary {
    RequestLogQuerySummary::default()
}

#[cfg(test)]
#[path = "tests/request_logs_tests.rs"]
mod tests;
