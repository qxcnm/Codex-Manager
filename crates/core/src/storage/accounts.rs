use rusqlite::{params_from_iter, types::Value, Result, Row};

use super::account_metadata::delete_account_metadata_for_account_sql;
use super::account_subscriptions::delete_account_subscription_for_account_sql;
use super::conversation_bindings::delete_conversation_bindings_for_account_sql;
use super::events::delete_events_for_account_sql;
use super::key_id_filters::{normalize_text_ids, text_id_in_clause, SQLITE_IN_CLAUSE_BATCH_SIZE};
use super::model_sources::{
    delete_model_source_mapping_preferences_for_source_sql,
    delete_model_source_mappings_for_source_sql, delete_model_source_models_for_source_sql,
};
use super::tokens::delete_token_for_account_sql;
use super::usage::delete_usage_snapshots_for_account_sql;

use super::{
    now_ts, Account, AccountAuthRefreshTarget, AccountCleanupCandidate,
    AccountCodexProfileCandidate, AccountDashboardSourceMetadata, AccountDirectAuthProfile,
    AccountImportSnapshot, AccountListSummaryRow, AccountQuotaOverviewStats,
    AccountQuotaPoolSource, AccountQuotaSourceSummary, AccountStatusCount,
    AccountSummaryStorageSnapshot, AccountSummaryStorageSnapshotOptions, AccountTokenRefreshIssuer,
    AccountUpsertState, AccountUsageRefreshTarget, AccountUsageRefreshTokenTarget,
    AccountWorkspaceIdentity, Storage, Token,
};

const ACCOUNT_MODEL_SOURCE_KIND: &str = "openai_account";

impl Storage {
    /// 函数 `insert_account`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account: 参数 account
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_account(&self, account: &Account) -> Result<()> {
        self.conn.execute(
            "INSERT INTO accounts (
                id,
                label,
                issuer,
                chatgpt_account_id,
                workspace_id,
                group_name,
                sort,
                status,
                created_at,
                updated_at,
                preferred
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                ?7,
                ?8,
                ?9,
                ?10,
                0
            )
             ON CONFLICT(id) DO UPDATE SET
                label = excluded.label,
                issuer = excluded.issuer,
                chatgpt_account_id = excluded.chatgpt_account_id,
                workspace_id = excluded.workspace_id,
                group_name = excluded.group_name,
                sort = excluded.sort,
                status = excluded.status,
                updated_at = excluded.updated_at",
            (
                &account.id,
                &account.label,
                &account.issuer,
                &account.chatgpt_account_id,
                &account.workspace_id,
                &account.group_name,
                account.sort,
                &account.status,
                account.created_at,
                account.updated_at,
            ),
        )?;
        Ok(())
    }

    /// 函数 `account_count`
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
    pub fn account_count(&self) -> Result<i64> {
        self.conn
            .query_row(account_count_sql(), [], |row| row.get(0))
    }

    pub fn account_status_counts(&self) -> Result<Vec<AccountStatusCount>> {
        let mut stmt = self.conn.prepare(account_status_counts_sql())?;
        let rows = stmt.query_map([], |row| {
            Ok(AccountStatusCount {
                status: row.get(0)?,
                count: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    pub fn account_quota_overview_stats(&self) -> Result<AccountQuotaOverviewStats> {
        let sql = format!(
            "{latest_usage_cte}
             SELECT
                COUNT(1) AS account_count,
                IFNULL(SUM(CASE WHEN {available_status_clause} THEN 1 ELSE 0 END), 0) AS available_count,
                IFNULL(SUM(
                    CASE
                        WHEN {available_status_clause}
                         AND (
                            ({primary_remain_expr} > 0.0 AND {primary_remain_expr} <= 20.0)
                            OR ({secondary_remain_expr} > 0.0 AND {secondary_remain_expr} <= 20.0)
                         )
                        THEN 1
                        ELSE 0
                    END
                ), 0) AS low_quota_count,
                AVG(CASE WHEN {available_status_clause} THEN {primary_remain_expr} END) AS primary_remain_percent_avg,
                AVG(CASE WHEN {available_status_clause} THEN {secondary_remain_expr} END) AS secondary_remain_percent_avg,
                MAX(CASE WHEN {available_status_clause} THEN lu.captured_at END) AS last_refreshed_at
             FROM accounts a
             LEFT JOIN latest_usage lu
               ON lu.account_id = a.id
              AND lu.rn = 1",
            latest_usage_cte = latest_usage_cte_sql(),
            available_status_clause = available_account_status_clause("a"),
            primary_remain_expr = remaining_percent_sql("lu.used_percent"),
            secondary_remain_expr = remaining_percent_sql("lu.secondary_used_percent"),
        );
        self.conn.query_row(&sql, [], |row| {
            Ok(AccountQuotaOverviewStats {
                account_count: row.get(0)?,
                available_count: row.get(1)?,
                low_quota_count: row.get(2)?,
                primary_remain_percent_avg: row.get(3)?,
                secondary_remain_percent_avg: row.get(4)?,
                last_refreshed_at: row.get(5)?,
            })
        })
    }

    pub fn max_account_sort(&self) -> Result<Option<i64>> {
        self.conn
            .query_row(max_account_sort_sql(), [], |row| row.get(0))
    }

    /// 函数 `account_count_filtered`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - group_name: 参数 group_name
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn account_count_filtered(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
    ) -> Result<i64> {
        let mut params = Vec::new();
        let where_clause = build_account_where_clause(query, group_name, &mut params, "accounts");
        let sql = format!("SELECT COUNT(1) FROM accounts{where_clause}");
        self.conn
            .query_row(&sql, params_from_iter(params), |row| row.get(0))
    }

    /// 函数 `list_accounts`
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
    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        self.list_accounts_filtered(None, None)
    }

    /// 函数 `list_accounts_filtered`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - group_name: 参数 group_name
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_accounts_filtered(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
    ) -> Result<Vec<Account>> {
        self.query_accounts(query, group_name, None)
    }

    pub fn list_accounts_by_statuses(&self, statuses: &[String]) -> Result<Vec<Account>> {
        let statuses = normalize_text_ids(statuses);
        if statuses.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in statuses.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_accounts_by_statuses_chunk(self, chunk)?);
        }
        out.sort_by(|left, right| {
            left.sort
                .cmp(&right.sort)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(out)
    }

    pub fn list_account_cleanup_candidates_by_statuses(
        &self,
        statuses: &[String],
    ) -> Result<Vec<AccountCleanupCandidate>> {
        let statuses = normalize_text_ids(statuses);
        if statuses.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in statuses.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_cleanup_candidates_by_statuses_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_accounts_for_ids(&self, account_ids: &[String]) -> Result<Vec<Account>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_accounts_for_ids_chunk(self, chunk)?);
        }
        out.sort_by(|left, right| {
            left.sort
                .cmp(&right.sort)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(out)
    }

    pub fn list_account_dashboard_source_metadata_for_ids(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<AccountDashboardSourceMetadata>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_dashboard_source_metadata_for_ids_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_account_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(account_ids_list_sql())?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    pub fn list_account_ids_by_statuses(&self, statuses: &[String]) -> Result<Vec<String>> {
        let statuses = normalize_text_ids(statuses);
        if statuses.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in statuses.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_ids_by_statuses_chunk(self, chunk)?);
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.cmp(&right.0))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_account_usage_refresh_targets_by_statuses(
        &self,
        statuses: &[String],
    ) -> Result<Vec<AccountUsageRefreshTarget>> {
        let statuses = normalize_text_ids(statuses);
        if statuses.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in statuses.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_usage_refresh_targets_by_statuses_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_account_usage_refresh_targets_with_usable_tokens_by_statuses(
        &self,
        statuses: &[String],
    ) -> Result<Vec<AccountUsageRefreshTarget>> {
        let statuses = normalize_text_ids(statuses);
        if statuses.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in statuses.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(
                list_account_usage_refresh_targets_with_usable_tokens_by_statuses_chunk(
                    self, chunk,
                )?,
            );
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_account_usage_refresh_token_targets_by_statuses(
        &self,
        statuses: &[String],
    ) -> Result<Vec<AccountUsageRefreshTokenTarget>> {
        let statuses = normalize_text_ids(statuses);
        if statuses.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in statuses.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_usage_refresh_token_targets_by_statuses_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.account_id.cmp(&right.0.account_id))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_active_account_codex_profile_candidates_for_ids(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<AccountCodexProfileCandidate>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_active_account_codex_profile_candidates_for_ids_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_account_auth_refresh_targets(&self) -> Result<Vec<AccountAuthRefreshTarget>> {
        let mut stmt = self.conn.prepare(account_auth_refresh_targets_list_sql())?;
        let rows = stmt.query_map([], |row| {
            Ok(AccountAuthRefreshTarget {
                id: row.get(0)?,
                label: row.get(1)?,
                issuer: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn find_account_direct_auth_profile_by_id(
        &self,
        account_id: &str,
    ) -> Result<Option<AccountDirectAuthProfile>> {
        let mut stmt = self.conn.prepare(account_direct_auth_profile_by_id_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(AccountDirectAuthProfile {
                id: row.get(0)?,
                issuer: row.get(1)?,
                chatgpt_account_id: row.get(2)?,
                status: row.get(3)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_account_quota_source_summaries(&self) -> Result<Vec<AccountQuotaSourceSummary>> {
        let mut stmt = self
            .conn
            .prepare(account_quota_source_summaries_list_sql())?;
        let rows = stmt.query_map([], |row| {
            Ok(AccountQuotaSourceSummary {
                id: row.get(0)?,
                label: row.get(1)?,
                status: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_available_account_quota_pool_sources(&self) -> Result<Vec<AccountQuotaPoolSource>> {
        let sql = format!(
            "SELECT id, label
             FROM accounts
             WHERE {available_status_clause}
             ORDER BY sort ASC, updated_at DESC, id ASC",
            available_status_clause = available_account_status_clause("accounts"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(AccountQuotaPoolSource {
                id: row.get(0)?,
                label: row.get(1)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_account_import_snapshots(&self) -> Result<Vec<AccountImportSnapshot>> {
        let mut stmt = self.conn.prepare(account_import_snapshots_list_sql())?;
        let rows = stmt.query_map([], |row| {
            Ok(AccountImportSnapshot {
                id: row.get(0)?,
                label: row.get(1)?,
                issuer: row.get(2)?,
                chatgpt_account_id: row.get(3)?,
                workspace_id: row.get(4)?,
                sort: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn list_account_summary_rows(&self) -> Result<Vec<AccountListSummaryRow>> {
        let mut stmt = self.conn.prepare(account_summary_rows_list_sql())?;
        let rows = stmt.query_map([], |row| {
            Ok(AccountListSummaryRow {
                id: row.get(0)?,
                label: row.get(1)?,
                group_name: row.get(2)?,
                sort: row.get(3)?,
                status: row.get(4)?,
            })
        })?;
        rows.collect()
    }

    pub fn load_account_summary_storage_snapshot(
        &self,
        account_ids: &[String],
    ) -> Result<AccountSummaryStorageSnapshot> {
        self.load_account_summary_storage_snapshot_with_options(
            account_ids,
            AccountSummaryStorageSnapshotOptions::default(),
        )
    }

    pub fn load_account_summary_storage_snapshot_with_options(
        &self,
        account_ids: &[String],
        options: AccountSummaryStorageSnapshotOptions,
    ) -> Result<AccountSummaryStorageSnapshot> {
        if account_ids.is_empty() {
            return Ok(AccountSummaryStorageSnapshot::default());
        }
        let (metadata, subscriptions, model_assignments, quota_overrides) =
            if options.include_details {
                (
                    self.list_account_metadata_for_accounts(account_ids)?,
                    self.list_account_subscriptions_for_accounts(account_ids)?,
                    self.list_quota_source_model_assignments_for_sources(
                        ACCOUNT_MODEL_SOURCE_KIND,
                        account_ids,
                    )?,
                    self.list_account_quota_capacity_overrides_for_accounts(account_ids)?,
                )
            } else {
                Default::default()
            };
        Ok(AccountSummaryStorageSnapshot {
            preferred_account_id: if options.include_preferred {
                self.preferred_account_id()?
            } else {
                None
            },
            status_reasons: if options.include_status_reasons {
                self.latest_account_status_reasons(account_ids)?
            } else {
                Default::default()
            },
            tokens: if options.include_tokens {
                self.list_account_token_plans_for_accounts(account_ids)?
            } else {
                Vec::new()
            },
            usage_snapshots: self.latest_usage_snapshots_for_accounts(account_ids)?,
            metadata,
            subscriptions,
            model_assignments,
            quota_overrides,
        })
    }

    pub fn list_account_ids_for_ids(&self, account_ids: &[String]) -> Result<Vec<String>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_ids_for_ids_chunk(self, chunk)?);
        }
        out.sort_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.cmp(&right.0))
        });
        Ok(out.into_iter().map(|item| item.0).collect())
    }

    pub fn list_account_token_refresh_issuers_for_ids(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<AccountTokenRefreshIssuer>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_token_refresh_issuers_for_ids_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(out)
    }

    pub fn list_accounts_matching_identity(
        &self,
        account_ids: &[String],
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> Result<Vec<Account>> {
        let mut clauses = Vec::new();
        let mut params = Vec::new();

        let account_ids = normalize_text_ids(account_ids);
        if !account_ids.is_empty() {
            let Some((condition, values)) = text_id_in_clause("a.id", &account_ids) else {
                return Ok(Vec::new());
            };
            clauses.push(condition);
            params.extend(values);
        }
        if let Some(value) = normalize_optional_filter(chatgpt_account_id) {
            clauses.push("a.chatgpt_account_id = ?".to_string());
            params.push(Value::Text(value));
        }
        if let Some(value) = normalize_optional_filter(workspace_id) {
            clauses.push("a.workspace_id = ?".to_string());
            params.push(Value::Text(value));
        }
        if clauses.is_empty() {
            return Ok(Vec::new());
        }

        let sql = format!(
            "SELECT {}
             FROM accounts a
             WHERE {}
             ORDER BY a.updated_at DESC, a.id ASC",
            account_select_columns("a"),
            clauses.join(" OR ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_account_row(row)?);
        }
        Ok(out)
    }

    pub fn list_account_workspace_identities_matching_identity(
        &self,
        account_ids: &[String],
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> Result<Vec<AccountWorkspaceIdentity>> {
        let mut clauses = Vec::new();
        let mut params = Vec::new();

        let account_ids = normalize_text_ids(account_ids);
        if !account_ids.is_empty() {
            let Some((condition, values)) = text_id_in_clause("a.id", &account_ids) else {
                return Ok(Vec::new());
            };
            clauses.push(condition);
            params.extend(values);
        }
        if let Some(value) = normalize_optional_filter(chatgpt_account_id) {
            clauses.push("a.chatgpt_account_id = ?".to_string());
            params.push(Value::Text(value));
        }
        if let Some(value) = normalize_optional_filter(workspace_id) {
            clauses.push("a.workspace_id = ?".to_string());
            params.push(Value::Text(value));
        }
        if clauses.is_empty() {
            return Ok(Vec::new());
        }

        let sql = format!(
            "SELECT a.id, a.chatgpt_account_id, a.workspace_id
             FROM accounts a
             WHERE {}
             ORDER BY a.updated_at DESC, a.id ASC",
            clauses.join(" OR ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params), |row| {
            Ok(AccountWorkspaceIdentity {
                id: row.get(0)?,
                chatgpt_account_id: row.get(1)?,
                workspace_id: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// 函数 `list_accounts_paginated`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - group_name: 参数 group_name
    /// - offset: 参数 offset
    /// - limit: 参数 limit
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_accounts_paginated(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Account>> {
        self.query_accounts(query, group_name, Some((offset, limit)))
    }

    /// 函数 `list_gateway_candidates`
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
    pub fn list_gateway_candidates(&self) -> Result<Vec<(Account, Token)>> {
        list_gateway_candidates_filtered(self, None)
    }

    pub fn list_gateway_candidates_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<(Account, Token)>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_gateway_candidates_filtered(self, Some(chunk))?);
        }
        out.sort_by(|(left, _), (right, _)| {
            left.sort
                .cmp(&right.sort)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(out)
    }

    pub fn find_account_with_token_by_id(
        &self,
        account_id: &str,
    ) -> Result<Option<(Account, Token)>> {
        self.find_account_with_token_by_column("a.id", account_id)
    }

    pub fn find_account_with_token_by_identity(
        &self,
        account_id: Option<&str>,
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> Result<Option<(Account, Token)>> {
        if let Some(account_id) = normalize_optional_filter(account_id) {
            if let Some(found) = self.find_account_with_token_by_column("a.id", &account_id)? {
                return Ok(Some(found));
            }
        }
        if let Some(chatgpt_account_id) = normalize_optional_filter(chatgpt_account_id) {
            if let Some(found) =
                self.find_account_with_token_by_column("a.chatgpt_account_id", &chatgpt_account_id)?
            {
                return Ok(Some(found));
            }
        }
        if let Some(workspace_id) = normalize_optional_filter(workspace_id) {
            if let Some(found) =
                self.find_account_with_token_by_column("a.workspace_id", &workspace_id)?
            {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// 函数 `find_account_by_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn find_account_by_id(&self, account_id: &str) -> Result<Option<Account>> {
        let mut stmt = self.conn.prepare(account_by_id_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_account_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn find_account_status_by_id(&self, account_id: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(account_status_by_id_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn find_account_workspace_identity_by_id(
        &self,
        account_id: &str,
    ) -> Result<Option<AccountWorkspaceIdentity>> {
        let mut stmt = self.conn.prepare(account_workspace_identity_by_id_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(AccountWorkspaceIdentity {
                id: row.get(0)?,
                chatgpt_account_id: row.get(1)?,
                workspace_id: row.get(2)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn find_account_upsert_state_by_id(
        &self,
        account_id: &str,
    ) -> Result<Option<AccountUpsertState>> {
        let mut stmt = self.conn.prepare(account_upsert_state_by_id_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(AccountUpsertState {
                group_name: row.get(0)?,
                sort: row.get(1)?,
                created_at: row.get(2)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn account_exists(&self, account_id: &str) -> Result<bool> {
        self.conn
            .query_row(account_exists_sql(), [account_id], |row| row.get(0))
    }

    pub fn find_account_by_identity(
        &self,
        account_id: Option<&str>,
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> Result<Option<Account>> {
        if let Some(account_id) = normalize_optional_filter(account_id) {
            if let Some(account) = self.find_account_by_id(&account_id)? {
                return Ok(Some(account));
            }
        }
        if let Some(chatgpt_account_id) = normalize_optional_filter(chatgpt_account_id) {
            if let Some(account) =
                self.find_account_by_identity_column("chatgpt_account_id", &chatgpt_account_id)?
            {
                return Ok(Some(account));
            }
        }
        if let Some(workspace_id) = normalize_optional_filter(workspace_id) {
            if let Some(account) =
                self.find_account_by_identity_column("workspace_id", &workspace_id)?
            {
                return Ok(Some(account));
            }
        }
        Ok(None)
    }

    pub fn find_account_id_by_identity(
        &self,
        account_id: Option<&str>,
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
    ) -> Result<Option<String>> {
        if let Some(account_id) = normalize_optional_filter(account_id) {
            if let Some(found) = self.find_account_id_by_column("id", &account_id)? {
                return Ok(Some(found));
            }
        }
        if let Some(chatgpt_account_id) = normalize_optional_filter(chatgpt_account_id) {
            if let Some(found) =
                self.find_account_id_by_column("chatgpt_account_id", &chatgpt_account_id)?
            {
                return Ok(Some(found));
            }
        }
        if let Some(workspace_id) = normalize_optional_filter(workspace_id) {
            if let Some(found) = self.find_account_id_by_column("workspace_id", &workspace_id)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// 函数 `update_account_sort`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    /// - sort: 参数 sort
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_account_sort(&self, account_id: &str, sort: i64) -> Result<()> {
        self.conn
            .execute(update_account_sort_sql(), (sort, now_ts(), account_id))?;
        Ok(())
    }

    /// 函数 `update_account_label`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    /// - label: 参数 label
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_account_label(&self, account_id: &str, label: &str) -> Result<()> {
        self.conn
            .execute(update_account_label_sql(), (label, now_ts(), account_id))?;
        Ok(())
    }

    pub fn update_account_workspace_identity(
        &self,
        account_id: &str,
        chatgpt_account_id: Option<&str>,
        workspace_id: Option<&str>,
        updated_at: i64,
    ) -> Result<bool> {
        let updated = self.conn.execute(
            update_account_workspace_identity_sql(),
            (chatgpt_account_id, workspace_id, updated_at, account_id),
        )?;
        Ok(updated > 0)
    }

    /// 函数 `touch_account_updated_at`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn touch_account_updated_at(&self, account_id: &str) -> Result<()> {
        self.conn
            .execute(touch_account_updated_at_sql(), (now_ts(), account_id))?;
        Ok(())
    }

    /// 函数 `update_account_status`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    /// - status: 参数 status
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_account_status(&self, account_id: &str, status: &str) -> Result<()> {
        self.conn
            .execute(update_account_status_sql(), (status, now_ts(), account_id))?;
        Ok(())
    }

    /// 函数 `update_account_status_if_changed`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    /// - status: 参数 status
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_account_status_if_changed(&self, account_id: &str, status: &str) -> Result<bool> {
        let updated = self.conn.execute(
            update_account_status_if_changed_sql(),
            (status, now_ts(), account_id),
        )?;
        Ok(updated > 0)
    }

    pub fn update_account_status_if_changed_with_existence(
        &self,
        account_id: &str,
        status: &str,
    ) -> Result<(bool, bool)> {
        let changed = self.update_account_status_if_changed(account_id, status)?;
        if changed {
            return Ok((true, true));
        }
        Ok((self.account_exists(account_id)?, false))
    }

    /// 函数 `delete_account`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn delete_account(&mut self, account_id: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(delete_account_metadata_for_account_sql(), [account_id])?;
        tx.execute(delete_account_subscription_for_account_sql(), [account_id])?;
        tx.execute(delete_token_for_account_sql(), [account_id])?;
        tx.execute(delete_usage_snapshots_for_account_sql(), [account_id])?;
        tx.execute(delete_events_for_account_sql(), [account_id])?;
        tx.execute(delete_conversation_bindings_for_account_sql(), [account_id])?;
        tx.execute(
            delete_model_source_mappings_for_source_sql(),
            (ACCOUNT_MODEL_SOURCE_KIND, account_id),
        )?;
        tx.execute(
            delete_model_source_models_for_source_sql(),
            (ACCOUNT_MODEL_SOURCE_KIND, account_id),
        )?;
        tx.execute(
            delete_model_source_mapping_preferences_for_source_sql(),
            (ACCOUNT_MODEL_SOURCE_KIND, account_id),
        )?;
        tx.execute(delete_account_by_id_sql(), [account_id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_accounts(&mut self, account_ids: &[String]) -> Result<usize> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(0);
        }

        let tx = self.conn.transaction()?;
        let mut deleted = 0usize;
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            delete_accounts_from_table(&tx, "account_metadata", "account_id", chunk)?;
            delete_accounts_from_table(&tx, "account_subscriptions", "account_id", chunk)?;
            delete_accounts_from_table(&tx, "tokens", "account_id", chunk)?;
            delete_accounts_from_table(&tx, "usage_snapshots", "account_id", chunk)?;
            delete_accounts_from_table(&tx, "events", "account_id", chunk)?;
            delete_accounts_from_table(&tx, "conversation_bindings", "account_id", chunk)?;
            delete_model_source_rows_for_accounts(&tx, "model_source_mappings", chunk)?;
            delete_model_source_rows_for_accounts(&tx, "model_source_models", chunk)?;
            delete_model_source_rows_for_accounts(&tx, "model_source_mapping_preferences", chunk)?;
            deleted += delete_accounts_from_table(&tx, "accounts", "id", chunk)?;
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// 函数 `ensure_account_meta_columns`
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
    pub(super) fn ensure_account_meta_columns(&self) -> Result<()> {
        self.ensure_column("accounts", "chatgpt_account_id", "TEXT")?;
        self.ensure_column("accounts", "group_name", "TEXT")?;
        self.ensure_column("accounts", "sort", "INTEGER DEFAULT 0")?;
        self.ensure_column("accounts", "preferred", "INTEGER NOT NULL DEFAULT 0")?;
        self.ensure_column("login_sessions", "note", "TEXT")?;
        self.ensure_column("login_sessions", "tags", "TEXT")?;
        self.ensure_column("login_sessions", "group_name", "TEXT")?;
        Ok(())
    }

    pub(super) fn ensure_account_group_name_filter_index(&self) -> Result<()> {
        self.ensure_column("accounts", "group_name", "TEXT")?;
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_accounts_group_name_sort_updated_at
               ON accounts(group_name, sort ASC, updated_at DESC);",
        )?;
        Ok(())
    }

    pub(super) fn ensure_accounts_list_order_index(&self) -> Result<()> {
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_accounts_list_order
             ON accounts(sort ASC, updated_at DESC, id ASC)",
            [],
        )?;
        Ok(())
    }

    pub(super) fn ensure_accounts_identity_lookup_indexes(&self) -> Result<()> {
        self.ensure_column("accounts", "chatgpt_account_id", "TEXT")?;
        self.ensure_column("accounts", "workspace_id", "TEXT")?;
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_accounts_chatgpt_account_id_updated_at
               ON accounts(chatgpt_account_id, updated_at DESC, id ASC);
             CREATE INDEX IF NOT EXISTS idx_accounts_workspace_id_updated_at
               ON accounts(workspace_id, updated_at DESC, id ASC);",
        )?;
        Ok(())
    }

    /// 函数 `preferred_account_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-10
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn preferred_account_id(&self) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(preferred_account_id_sql())?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `set_preferred_account`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-10
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn set_preferred_account(&mut self, account_id: Option<&str>) -> Result<()> {
        let now = now_ts();
        let tx = self.conn.transaction()?;
        tx.execute(clear_preferred_accounts_sql(), [])?;
        if let Some(account_id) = account_id {
            let normalized_account_id = account_id.trim();
            if !normalized_account_id.is_empty() {
                tx.execute(set_preferred_account_sql(), (now, normalized_account_id))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 函数 `clear_preferred_account_if`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-10
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn clear_preferred_account_if(&self, account_id: &str) -> Result<bool> {
        let normalized_account_id = account_id.trim();
        if normalized_account_id.is_empty() {
            return Ok(false);
        }
        let updated = self.conn.execute(
            clear_preferred_account_by_id_sql(),
            (now_ts(), normalized_account_id),
        )?;
        Ok(updated > 0)
    }

    /// 函数 `ensure_login_session_workspace_column`
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
    pub(super) fn ensure_login_session_workspace_column(&self) -> Result<()> {
        self.ensure_column("login_sessions", "workspace_id", "TEXT")?;
        Ok(())
    }

    /// 函数 `query_accounts`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - query: 参数 query
    /// - group_name: 参数 group_name
    /// - pagination: 参数 pagination
    ///
    /// # 返回
    /// 返回函数执行结果
    fn query_accounts(
        &self,
        query: Option<&str>,
        group_name: Option<&str>,
        pagination: Option<(i64, i64)>,
    ) -> Result<Vec<Account>> {
        let mut params = Vec::new();
        let where_clause = build_account_where_clause(query, group_name, &mut params, "a");
        let sql = account_query_sql(&where_clause, pagination.is_some());
        if let Some((offset, limit)) = pagination {
            params.push(Value::Integer(limit.max(1)));
            params.push(Value::Integer(offset.max(0)));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_account_row(row)?);
        }
        Ok(out)
    }

    fn find_account_by_identity_column(
        &self,
        column: &str,
        identity: &str,
    ) -> Result<Option<Account>> {
        debug_assert!(matches!(column, "chatgpt_account_id" | "workspace_id"));
        let sql = account_identity_lookup_sql(column);
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([identity])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_account_row(row)?))
        } else {
            Ok(None)
        }
    }

    fn find_account_id_by_column(&self, column: &str, identity: &str) -> Result<Option<String>> {
        debug_assert!(matches!(
            column,
            "id" | "chatgpt_account_id" | "workspace_id"
        ));
        let sql = format!(
            "SELECT id
             FROM accounts
             WHERE {column} = ?1
             ORDER BY updated_at DESC, id ASC
             LIMIT 1",
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([identity])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    fn find_account_with_token_by_column(
        &self,
        column: &str,
        identity: &str,
    ) -> Result<Option<(Account, Token)>> {
        debug_assert!(matches!(
            column,
            "a.id" | "a.chatgpt_account_id" | "a.workspace_id"
        ));
        let sql = format!(
            "SELECT
               {account_select},
               {token_select}
             FROM accounts a
             JOIN tokens t
               ON t.account_id = a.id
             WHERE {column} = ?1
             ORDER BY a.updated_at DESC, a.id ASC
             LIMIT 1",
            account_select = account_select_columns("a"),
            token_select = token_select_columns("t"),
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([identity])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_gateway_candidate_row(row)?))
        } else {
            Ok(None)
        }
    }
}

/// 函数 `normalize_optional_filter`
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
fn normalize_optional_filter(value: Option<&str>) -> Option<String> {
    let trimmed = value.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// 函数 `build_account_where_clause`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - query: 参数 query
/// - group_name: 参数 group_name
/// - params: 参数 params
/// - table_name: 参数 table_name
///
/// # 返回
/// 返回函数执行结果
fn build_account_where_clause(
    query: Option<&str>,
    group_name: Option<&str>,
    params: &mut Vec<Value>,
    table_name: &str,
) -> String {
    let mut clauses = Vec::new();

    if let Some(keyword) = normalize_optional_filter(query) {
        let pattern = format!("%{keyword}%");
        let label_column = qualified_column(table_name, "label");
        let id_column = qualified_column(table_name, "id");
        clauses.push(format!(
            "(LOWER({label_column}) LIKE LOWER(?) OR LOWER({id_column}) LIKE LOWER(?))"
        ));
        params.push(Value::Text(pattern.clone()));
        params.push(Value::Text(pattern));
    }

    if let Some(group_name) = normalize_optional_filter(group_name) {
        let group_column = qualified_column(table_name, "group_name");
        clauses.push(format!("{group_column} = ?"));
        params.push(Value::Text(group_name));
    }

    if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    }
}

/// 函数 `qualified_column`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - table_name: 参数 table_name
/// - column: 参数 column
///
/// # 返回
/// 返回函数执行结果
fn qualified_column(table_name: &str, column: &str) -> String {
    format!("{table_name}.{column}")
}

fn account_count_sql() -> &'static str {
    "SELECT COUNT(1) FROM accounts"
}

fn max_account_sort_sql() -> &'static str {
    "SELECT MAX(sort) FROM accounts"
}

fn account_status_counts_sql() -> &'static str {
    "SELECT LOWER(TRIM(COALESCE(status, ''))), COUNT(1)
     FROM accounts
     GROUP BY LOWER(TRIM(COALESCE(status, '')))
     ORDER BY COUNT(1) DESC, LOWER(TRIM(COALESCE(status, ''))) ASC"
}

fn account_direct_auth_profile_by_id_sql() -> &'static str {
    "SELECT id, issuer, chatgpt_account_id, status
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

fn account_by_id_sql() -> &'static str {
    "SELECT id, label, issuer, chatgpt_account_id, workspace_id, group_name, sort, status, created_at, updated_at
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

fn account_status_by_id_sql() -> &'static str {
    "SELECT status
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

fn account_workspace_identity_by_id_sql() -> &'static str {
    "SELECT id, chatgpt_account_id, workspace_id
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

fn account_upsert_state_by_id_sql() -> &'static str {
    "SELECT group_name, sort, created_at
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

fn account_exists_sql() -> &'static str {
    "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = ?1)"
}

fn preferred_account_id_sql() -> &'static str {
    "SELECT id
     FROM accounts
     WHERE preferred = 1
     ORDER BY updated_at DESC, id ASC
     LIMIT 1"
}

fn update_account_sort_sql() -> &'static str {
    "UPDATE accounts SET sort = ?1, updated_at = ?2 WHERE id = ?3"
}

fn update_account_label_sql() -> &'static str {
    "UPDATE accounts SET label = ?1, updated_at = ?2 WHERE id = ?3"
}

fn update_account_workspace_identity_sql() -> &'static str {
    "UPDATE accounts
     SET chatgpt_account_id = ?1,
         workspace_id = ?2,
         updated_at = ?3
     WHERE id = ?4"
}

fn touch_account_updated_at_sql() -> &'static str {
    "UPDATE accounts SET updated_at = ?1 WHERE id = ?2"
}

fn update_account_status_sql() -> &'static str {
    "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3"
}

fn update_account_status_if_changed_sql() -> &'static str {
    "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3 AND status != ?1"
}

fn delete_account_by_id_sql() -> &'static str {
    "DELETE FROM accounts WHERE id = ?1"
}

fn clear_preferred_accounts_sql() -> &'static str {
    "UPDATE accounts SET preferred = 0 WHERE preferred != 0"
}

fn set_preferred_account_sql() -> &'static str {
    "UPDATE accounts
     SET preferred = 1, updated_at = ?1
     WHERE id = ?2"
}

fn clear_preferred_account_by_id_sql() -> &'static str {
    "UPDATE accounts SET preferred = 0, updated_at = ?1 WHERE id = ?2 AND preferred = 1"
}
fn account_ids_list_sql() -> &'static str {
    "SELECT id FROM accounts ORDER BY sort ASC, updated_at DESC, id ASC"
}
fn account_auth_refresh_targets_list_sql() -> &'static str {
    "SELECT id, label, issuer
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}

fn account_quota_source_summaries_list_sql() -> &'static str {
    "SELECT id, label, status
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}

fn account_import_snapshots_list_sql() -> &'static str {
    "SELECT id, label, issuer, chatgpt_account_id, workspace_id, sort, created_at
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}

fn account_summary_rows_list_sql() -> &'static str {
    "SELECT id, label, group_name, sort, status
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}

fn account_query_sql(where_clause: &str, include_pagination: bool) -> String {
    let mut sql = format!(
        "SELECT {} FROM accounts a{where_clause} ORDER BY a.sort ASC, a.updated_at DESC",
        account_select_columns("a"),
    );
    if include_pagination {
        sql.push_str(" LIMIT ? OFFSET ?");
    }
    sql
}

fn account_identity_lookup_sql(column: &str) -> String {
    debug_assert!(matches!(column, "chatgpt_account_id" | "workspace_id"));
    format!(
        "SELECT {}
         FROM accounts
         WHERE {column} = ?1
         ORDER BY updated_at DESC, id ASC
         LIMIT 1",
        account_select_columns("accounts"),
    )
}

fn account_ids_by_statuses_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT id, sort, updated_at
         FROM accounts
         WHERE {condition}"
    )
}

fn account_usage_refresh_targets_by_statuses_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT id, status, workspace_id, sort, updated_at
         FROM accounts
         WHERE {condition}"
    )
}

fn account_cleanup_candidates_by_statuses_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT id, status, sort, updated_at
         FROM accounts
         WHERE {condition}"
    )
}

fn account_ids_for_ids_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT id, sort, updated_at
         FROM accounts
         WHERE {condition}"
    )
}

fn accounts_by_statuses_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT {}
         FROM accounts a
         WHERE {condition}",
        account_select_columns("a"),
    )
}

fn accounts_for_ids_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT {}
         FROM accounts a
         WHERE {condition}",
        account_select_columns("a"),
    )
}

fn active_account_codex_profile_candidates_for_ids_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT id, label, issuer, chatgpt_account_id, workspace_id, group_name, status, sort, updated_at
         FROM accounts
         WHERE {condition}
           AND LOWER(TRIM(COALESCE(status, ''))) = 'active'"
    )
}

fn account_token_refresh_issuers_for_ids_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT id, issuer
         FROM accounts
         WHERE {condition}"
    )
}

fn account_dashboard_source_metadata_for_ids_chunk_sql(condition: &str) -> String {
    format!(
        "SELECT id, label, status, sort, updated_at
         FROM accounts
         WHERE {condition}"
    )
}

fn list_accounts_by_statuses_chunk(storage: &Storage, statuses: &[String]) -> Result<Vec<Account>> {
    let Some((condition, params)) =
        text_id_in_clause("LOWER(TRIM(COALESCE(a.status, '')))", statuses)
    else {
        return Ok(Vec::new());
    };
    let sql = accounts_by_statuses_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_account_row(row)?);
    }
    Ok(out)
}

fn list_account_ids_by_statuses_chunk(
    storage: &Storage,
    statuses: &[String],
) -> Result<Vec<(String, i64, i64)>> {
    let Some((condition, params)) =
        text_id_in_clause("LOWER(TRIM(COALESCE(status, '')))", statuses)
    else {
        return Ok(Vec::new());
    };
    let sql = account_ids_by_statuses_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

fn list_account_usage_refresh_targets_by_statuses_chunk(
    storage: &Storage,
    statuses: &[String],
) -> Result<Vec<(AccountUsageRefreshTarget, i64, i64)>> {
    let Some((condition, params)) =
        text_id_in_clause("LOWER(TRIM(COALESCE(status, '')))", statuses)
    else {
        return Ok(Vec::new());
    };
    let sql = account_usage_refresh_targets_by_statuses_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((
            AccountUsageRefreshTarget {
                id: row.get(0)?,
                status: row.get(1)?,
                workspace_id: row.get(2)?,
            },
            row.get(3)?,
            row.get(4)?,
        ))
    })?;
    rows.collect()
}

fn list_account_usage_refresh_targets_with_usable_tokens_by_statuses_chunk(
    storage: &Storage,
    statuses: &[String],
) -> Result<Vec<(AccountUsageRefreshTarget, i64, i64)>> {
    let Some((condition, params)) =
        text_id_in_clause("LOWER(TRIM(COALESCE(a.status, '')))", statuses)
    else {
        return Ok(Vec::new());
    };
    let sql = account_usage_refresh_targets_with_usable_tokens_by_statuses_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((
            AccountUsageRefreshTarget {
                id: row.get(0)?,
                status: row.get(1)?,
                workspace_id: row.get(2)?,
            },
            row.get(3)?,
            row.get(4)?,
        ))
    })?;
    rows.collect()
}

fn account_usage_refresh_targets_with_usable_tokens_by_statuses_chunk_sql(
    condition: &str,
) -> String {
    format!(
        "SELECT a.id, a.status, a.workspace_id, a.sort, a.updated_at
         FROM accounts a
         INNER JOIN tokens t ON t.account_id = a.id
         WHERE {condition}
           AND TRIM(COALESCE(t.access_token, '')) <> ''
           AND TRIM(COALESCE(t.refresh_token, '')) <> ''"
    )
}

fn list_account_usage_refresh_token_targets_by_statuses_chunk(
    storage: &Storage,
    statuses: &[String],
) -> Result<Vec<(AccountUsageRefreshTokenTarget, i64, i64)>> {
    let Some((condition, params)) =
        text_id_in_clause("LOWER(TRIM(COALESCE(a.status, '')))", statuses)
    else {
        return Ok(Vec::new());
    };
    let sql = usage_refresh_token_targets_by_status_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        let account_id: String = row.get(0)?;
        Ok((
            AccountUsageRefreshTokenTarget {
                account_id,
                workspace_id: row.get(1)?,
                token: Token {
                    account_id: row.get(2)?,
                    id_token: row.get(3)?,
                    access_token: row.get(4)?,
                    refresh_token: row.get(5)?,
                    api_key_access_token: row.get(6)?,
                    last_refresh: row.get(7)?,
                },
            },
            row.get(8)?,
            row.get(9)?,
        ))
    })?;
    rows.collect()
}

fn usage_refresh_token_targets_by_status_sql(status_condition: &str) -> String {
    format!(
        "WITH target_accounts AS (
            SELECT
                a.id,
                a.workspace_id,
                a.sort,
                a.updated_at,
                t.account_id AS token_account_id,
                t.id_token,
                t.access_token,
                t.refresh_token,
                t.api_key_access_token,
                t.last_refresh
            FROM accounts a
            INNER JOIN tokens t ON t.account_id = a.id
            WHERE {status_condition}
              AND TRIM(COALESCE(t.access_token, '')) <> ''
              AND TRIM(COALESCE(t.refresh_token, '')) <> ''
        ),
        latest_status AS (
            SELECT
                e.account_id,
                LOWER(TRIM(SUBSTR(e.message, INSTR(e.message, ' reason=') + LENGTH(' reason=')))) AS reason,
                ROW_NUMBER() OVER (
                    PARTITION BY e.account_id
                    ORDER BY e.created_at DESC, e.id DESC
                ) AS rn
            FROM events e
            INNER JOIN target_accounts ta
              ON ta.id = e.account_id
            WHERE e.type = 'account_status_update'
              AND INSTR(e.message, ' reason=') > 0
        )
        SELECT
            ta.id,
            ta.workspace_id,
            ta.token_account_id,
            ta.id_token,
            ta.access_token,
            ta.refresh_token,
            ta.api_key_access_token,
            ta.last_refresh,
            ta.sort,
            ta.updated_at
         FROM target_accounts ta
         LEFT JOIN latest_status ls
           ON ls.account_id = ta.id
          AND ls.rn = 1
         WHERE (
                ls.reason IS NULL
                OR ls.reason NOT IN (
                    'account_deactivated',
                    'workspace_deactivated',
                    'deactivated_workspace',
                    'refresh_token_region_blocked'
                )
           )"
    )
}

fn list_active_account_codex_profile_candidates_for_ids_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<(AccountCodexProfileCandidate, i64, i64)>> {
    let Some((condition, params)) = text_id_in_clause("id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = active_account_codex_profile_candidates_for_ids_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((
            AccountCodexProfileCandidate {
                id: row.get(0)?,
                label: row.get(1)?,
                issuer: row.get(2)?,
                chatgpt_account_id: row.get(3)?,
                workspace_id: row.get(4)?,
                group_name: row.get(5)?,
                status: row.get(6)?,
            },
            row.get(7)?,
            row.get(8)?,
        ))
    })?;
    rows.collect()
}

fn list_account_cleanup_candidates_by_statuses_chunk(
    storage: &Storage,
    statuses: &[String],
) -> Result<Vec<(AccountCleanupCandidate, i64, i64)>> {
    let Some((condition, params)) =
        text_id_in_clause("LOWER(TRIM(COALESCE(status, '')))", statuses)
    else {
        return Ok(Vec::new());
    };
    let sql = account_cleanup_candidates_by_statuses_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((
            AccountCleanupCandidate {
                id: row.get(0)?,
                status: row.get(1)?,
            },
            row.get(2)?,
            row.get(3)?,
        ))
    })?;
    rows.collect()
}

fn delete_accounts_from_table(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    column: &str,
    account_ids: &[String],
) -> Result<usize> {
    let Some((condition, params)) = text_id_in_clause(column, account_ids) else {
        return Ok(0);
    };
    let sql = format!("DELETE FROM {table} WHERE {condition}");
    tx.execute(&sql, params_from_iter(params))
}

fn delete_model_source_rows_for_accounts(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    account_ids: &[String],
) -> Result<usize> {
    let Some((condition, params)) = text_id_in_clause("source_id", account_ids) else {
        return Ok(0);
    };
    let sql = format!(
        "DELETE FROM {table}
         WHERE source_kind = 'openai_account'
           AND {condition}"
    );
    tx.execute(&sql, params_from_iter(params))
}

fn list_accounts_for_ids_chunk(storage: &Storage, account_ids: &[String]) -> Result<Vec<Account>> {
    let Some((condition, params)) = text_id_in_clause("a.id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = accounts_for_ids_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_account_row(row)?);
    }
    Ok(out)
}

fn list_account_ids_for_ids_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<(String, i64, i64)>> {
    let Some((condition, params)) = text_id_in_clause("id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = account_ids_for_ids_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    rows.collect()
}

fn list_account_token_refresh_issuers_for_ids_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<AccountTokenRefreshIssuer>> {
    let Some((condition, params)) = text_id_in_clause("id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = account_token_refresh_issuers_for_ids_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok(AccountTokenRefreshIssuer {
            id: row.get(0)?,
            issuer: row.get(1)?,
        })
    })?;
    rows.collect()
}

fn list_account_dashboard_source_metadata_for_ids_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<(AccountDashboardSourceMetadata, i64, i64)>> {
    let Some((condition, params)) = text_id_in_clause("id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = account_dashboard_source_metadata_for_ids_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((
            AccountDashboardSourceMetadata {
                id: row.get(0)?,
                label: row.get(1)?,
                status: row.get(2)?,
            },
            row.get(3)?,
            row.get(4)?,
        ))
    })?;
    rows.collect()
}

fn list_gateway_candidates_filtered(
    storage: &Storage,
    account_ids: Option<&[String]>,
) -> Result<Vec<(Account, Token)>> {
    let availability_clause = gateway_account_usage_filter_clause("a", "lu");
    let mut usage_cte_params = Vec::new();
    let latest_usage_cte = if let Some(account_ids) = account_ids {
        let Some((usage_condition, usage_params)) = text_id_in_clause("account_id", account_ids)
        else {
            return Ok(Vec::new());
        };
        usage_cte_params.extend(usage_params);
        latest_usage_cte_sql_for_condition(&usage_condition)
    } else {
        latest_usage_cte_sql().to_string()
    };
    let mut where_clauses = vec![availability_clause];
    let mut params = Vec::new();
    if let Some(account_ids) = account_ids {
        let Some((condition, condition_params)) = text_id_in_clause("a.id", account_ids) else {
            return Ok(Vec::new());
        };
        where_clauses.push(condition);
        params.extend(condition_params);
    }
    let where_clause = where_clauses.join(" AND ");
    let sql = gateway_candidates_filtered_sql(&latest_usage_cte, &where_clause);
    if !usage_cte_params.is_empty() {
        usage_cte_params.extend(params);
        params = usage_cte_params;
    }

    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_gateway_candidate_row(row)?);
    }
    Ok(out)
}

fn gateway_candidates_filtered_sql(latest_usage_cte: &str, where_clause: &str) -> String {
    format!(
        "{latest_usage_cte}
         SELECT
           {account_select},
           {token_select}
         FROM accounts a
         JOIN tokens t
           ON t.account_id = a.id
         LEFT JOIN latest_usage lu
           ON lu.account_id = a.id
          AND lu.rn = 1
         WHERE {where_clause}
         ORDER BY a.sort ASC, a.updated_at DESC",
        account_select = account_select_columns("a"),
        token_select = token_select_columns("t"),
    )
}

/// 函数 `latest_usage_cte_sql`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn latest_usage_cte_sql() -> &'static str {
    "WITH latest_usage AS (
        SELECT
            account_id,
            used_percent,
            window_minutes,
            captured_at,
            secondary_used_percent,
            secondary_window_minutes,
            ROW_NUMBER() OVER (
                PARTITION BY account_id
                ORDER BY captured_at DESC, id DESC
            ) AS rn
        FROM usage_snapshots
    )"
}

fn latest_usage_cte_sql_for_condition(where_condition: &str) -> String {
    format!(
        "WITH latest_usage AS (
        SELECT
            account_id,
            used_percent,
            window_minutes,
            captured_at,
            secondary_used_percent,
            secondary_window_minutes,
            ROW_NUMBER() OVER (
                PARTITION BY account_id
                ORDER BY captured_at DESC, id DESC
            ) AS rn
        FROM usage_snapshots
        WHERE {where_condition}
    )"
    )
}

fn available_account_status_clause(account_alias: &str) -> String {
    format!("LOWER(TRIM(COALESCE({account_alias}.status, ''))) IN ('active', 'available')")
}

fn remaining_percent_sql(percent_expr: &str) -> String {
    format!(
        "CASE
            WHEN {percent_expr} IS NULL THEN NULL
            WHEN {percent_expr} < 0.0 THEN 100.0
            WHEN {percent_expr} > 100.0 THEN 0.0
            ELSE 100.0 - {percent_expr}
         END"
    )
}

/// 函数 `available_usage_clause`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - usage_alias: 参数 usage_alias
///
/// # 返回
/// 返回函数执行结果
fn available_usage_clause(usage_alias: &str) -> String {
    format!(
        "{usage_alias}.used_percent IS NOT NULL
         AND {usage_alias}.window_minutes IS NOT NULL
         AND (
            ({usage_alias}.secondary_used_percent IS NULL AND {usage_alias}.secondary_window_minutes IS NULL)
            OR ({usage_alias}.secondary_used_percent IS NOT NULL AND {usage_alias}.secondary_window_minutes IS NOT NULL)
         )
         AND {usage_alias}.used_percent < 100
         AND ({usage_alias}.secondary_used_percent IS NULL OR {usage_alias}.secondary_used_percent < 100)"
    )
}

/// 函数 `gateway_account_usage_filter_clause`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_alias: 参数 account_alias
/// - usage_alias: 参数 usage_alias
///
/// # 返回
/// 返回函数执行结果
fn gateway_account_usage_filter_clause(account_alias: &str, usage_alias: &str) -> String {
    format!(
        "LOWER(TRIM(COALESCE({account_alias}.status, ''))) NOT IN ('inactive', 'disabled', 'unavailable', 'limited', 'banned')
         AND ({usage_alias}.account_id IS NULL OR ({}))",
        available_usage_clause(usage_alias)
    )
}

/// 函数 `account_select_columns`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - table_name: 参数 table_name
///
/// # 返回
/// 返回函数执行结果
fn account_select_columns(table_name: &str) -> String {
    [
        "id",
        "label",
        "issuer",
        "chatgpt_account_id",
        "workspace_id",
        "group_name",
        "sort",
        "status",
        "created_at",
        "updated_at",
    ]
    .into_iter()
    .map(|column| qualified_column(table_name, column))
    .collect::<Vec<_>>()
    .join(", ")
}

/// 函数 `token_select_columns`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - table_name: 参数 table_name
///
/// # 返回
/// 返回函数执行结果
fn token_select_columns(table_name: &str) -> String {
    [
        "account_id",
        "id_token",
        "access_token",
        "refresh_token",
        "api_key_access_token",
        "last_refresh",
    ]
    .into_iter()
    .map(|column| qualified_column(table_name, column))
    .collect::<Vec<_>>()
    .join(", ")
}

/// 函数 `map_account_row`
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
fn map_account_row(row: &Row<'_>) -> Result<Account> {
    map_account_row_from_offset(row, 0)
}

/// 函数 `map_account_row_from_offset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - row: 参数 row
/// - offset: 参数 offset
///
/// # 返回
/// 返回函数执行结果
fn map_account_row_from_offset(row: &Row<'_>, offset: usize) -> Result<Account> {
    Ok(Account {
        id: row.get(offset)?,
        label: row.get(offset + 1)?,
        issuer: row.get(offset + 2)?,
        chatgpt_account_id: row.get(offset + 3)?,
        workspace_id: row.get(offset + 4)?,
        group_name: row.get(offset + 5)?,
        sort: row.get(offset + 6)?,
        status: row.get(offset + 7)?,
        created_at: row.get(offset + 8)?,
        updated_at: row.get(offset + 9)?,
    })
}

/// 函数 `map_token_row_from_offset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - row: 参数 row
/// - offset: 参数 offset
///
/// # 返回
/// 返回函数执行结果
fn map_token_row_from_offset(row: &Row<'_>, offset: usize) -> Result<Token> {
    Ok(Token {
        account_id: row.get(offset)?,
        id_token: row.get(offset + 1)?,
        access_token: row.get(offset + 2)?,
        refresh_token: row.get(offset + 3)?,
        api_key_access_token: row.get(offset + 4)?,
        last_refresh: row.get(offset + 5)?,
    })
}

/// 函数 `map_gateway_candidate_row`
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
fn map_gateway_candidate_row(row: &Row<'_>) -> Result<(Account, Token)> {
    let account = map_account_row_from_offset(row, 0)?;
    let token = map_token_row_from_offset(row, 10)?;
    Ok((account, token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        ConversationBinding, Event, ModelSourceMapping, ModelSourceModel, UsageSnapshotRecord,
    };

    fn sample_account(id: &str, status: &str, now: i64) -> Account {
        Account {
            id: id.to_string(),
            label: id.to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: status.to_string(),
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_token(account_id: &str, now: i64) -> Token {
        Token {
            account_id: account_id.to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        }
    }

    fn dependent_row_count(storage: &Storage, table: &str, column: &str, account_id: &str) -> i64 {
        let sql = format!("SELECT COUNT(1) FROM {table} WHERE {column} = ?1");
        storage
            .conn
            .query_row(&sql, [account_id], |row| row.get(0))
            .expect("count dependent rows")
    }

    fn model_source_account_row_count(storage: &Storage, table: &str, account_id: &str) -> i64 {
        let sql = format!(
            "SELECT COUNT(1) FROM {table} WHERE source_kind = 'openai_account' AND source_id = ?1"
        );
        storage
            .conn
            .query_row(&sql, [account_id], |row| row.get(0))
            .expect("count model source account rows")
    }

    fn collect_query_plan(storage: &Storage, sql: &str) -> String {
        collect_query_plan_with_params(storage, sql, [])
    }

    fn collect_query_plan_with_params<P>(storage: &Storage, sql: &str, params: P) -> String
    where
        P: rusqlite::Params,
    {
        let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
        let rows = stmt
            .query_map(params, |row| row.get::<_, String>(3))
            .expect("query explain");
        rows.collect::<Result<Vec<_>>>()
            .expect("collect explain")
            .join("\n")
    }

    #[test]
    fn insert_account_update_preserves_existing_token() {
        let mut storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-upsert", "active", now);
        account.chatgpt_account_id = Some("cgpt-old".to_string());
        account.group_name = Some("team-a".to_string());
        storage.insert_account(&account).expect("insert account");
        storage
            .insert_token(&sample_token(account.id.as_str(), now))
            .expect("insert token");
        storage
            .set_preferred_account(Some(account.id.as_str()))
            .expect("set preferred");

        let mut updated = account.clone();
        updated.label = "updated label".to_string();
        updated.chatgpt_account_id = Some("cgpt-new".to_string());
        updated.workspace_id = Some("ws-new".to_string());
        updated.created_at = now.saturating_add(100);
        updated.updated_at = now.saturating_add(1);
        storage
            .insert_account(&updated)
            .expect("update account without replacing row");

        let found = storage
            .find_account_by_id(account.id.as_str())
            .expect("find updated account")
            .expect("updated account exists");
        assert_eq!(found.label, "updated label");
        assert_eq!(found.chatgpt_account_id.as_deref(), Some("cgpt-new"));
        assert_eq!(found.workspace_id.as_deref(), Some("ws-new"));
        assert_eq!(found.group_name.as_deref(), Some("team-a"));
        assert_eq!(found.created_at, now);
        assert_eq!(found.updated_at, now.saturating_add(1));
        assert_eq!(
            storage.preferred_account_id().expect("preferred account"),
            Some(account.id.clone())
        );

        let plan = collect_query_plan_with_params(
            &storage,
            &format!("EXPLAIN QUERY PLAN {}", account_by_id_sql()),
            rusqlite::params![account.id.as_str()],
        );
        assert!(
            plan.contains("sqlite_autoindex_accounts_1"),
            "expected account lookup to use account primary-key index, got {plan}"
        );

        let token = storage
            .find_token_by_account_id(account.id.as_str())
            .expect("find token")
            .expect("token still exists");
        assert_eq!(token.access_token, "access");
        assert_eq!(token.refresh_token, "refresh");
    }

    #[test]
    fn account_count_reads_account_cardinality_sql() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        assert_eq!(storage.account_count().expect("empty count"), 0);
        storage
            .insert_account(&sample_account("acc-count-first", "active", now))
            .expect("insert first account");
        storage
            .insert_account(&sample_account("acc-count-second", "disabled", now))
            .expect("insert second account");

        assert_eq!(storage.account_count().expect("account count"), 2);

        let plan = collect_query_plan(
            &storage,
            &format!("EXPLAIN QUERY PLAN {}", account_count_sql()),
        );
        assert!(
            plan.contains("SCAN accounts") || plan.contains("USING COVERING INDEX"),
            "expected account count to stay a direct accounts cardinality scan, got {plan}"
        );
    }

    #[test]
    fn max_account_sort_reads_largest_sort_without_loading_accounts() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        assert_eq!(storage.max_account_sort().expect("max empty sort"), None);

        let mut low = sample_account("acc-low-sort", "active", now);
        low.sort = 2;
        let mut high = sample_account("acc-high-sort", "active", now);
        high.sort = 11;
        storage.insert_account(&low).expect("insert low sort");
        storage.insert_account(&high).expect("insert high sort");

        assert_eq!(storage.max_account_sort().expect("max sort"), Some(11));

        let plan = collect_query_plan(
            &storage,
            &format!("EXPLAIN QUERY PLAN {}", max_account_sort_sql()),
        );
        assert!(
            plan.contains("USING COVERING INDEX")
                && (plan.contains("idx_accounts_sort_updated_at")
                    || plan.contains("idx_accounts_list_order")),
            "expected max sort lookup to use an account sort covering index, got {plan}"
        );
    }

    #[test]
    fn account_quota_overview_stats_aggregates_latest_usage_in_sql() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        assert_eq!(
            storage
                .account_quota_overview_stats()
                .expect("empty stats")
                .account_count,
            0
        );

        for (account_id, status) in [
            ("acc-active-low", "active"),
            ("acc-available-ok", "available"),
            ("acc-disabled-low", "disabled"),
        ] {
            storage
                .insert_account(&sample_account(account_id, status, now))
                .expect("insert account");
        }

        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-active-low".to_string(),
                used_percent: Some(10.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: Some(10.0),
                secondary_window_minutes: Some(10_080),
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now - 60,
            })
            .expect("insert old active usage");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-active-low".to_string(),
                used_percent: Some(90.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: Some(10.0),
                secondary_window_minutes: Some(10_080),
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert latest active usage");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-available-ok".to_string(),
                used_percent: Some(20.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: Some(40.0),
                secondary_window_minutes: Some(10_080),
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now + 10,
            })
            .expect("insert available usage");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-disabled-low".to_string(),
                used_percent: Some(99.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now + 20,
            })
            .expect("insert disabled usage");

        let stats = storage
            .account_quota_overview_stats()
            .expect("quota overview stats");

        assert_eq!(stats.account_count, 3);
        assert_eq!(stats.available_count, 2);
        assert_eq!(stats.low_quota_count, 1);
        assert_eq!(stats.primary_remain_percent_avg, Some(45.0));
        assert_eq!(stats.secondary_remain_percent_avg, Some(75.0));
        assert_eq!(stats.last_refreshed_at, Some(now + 10));
    }

    #[test]
    fn list_accounts_for_ids_filters_and_preserves_account_order() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first", "active", now);
        first.sort = 1;
        first.updated_at = now;
        let mut second = sample_account("acc-second", "active", now);
        second.sort = 0;
        second.updated_at = now.saturating_sub(10);
        let mut ignored = sample_account("acc-ignored", "active", now);
        ignored.sort = -1;

        for account in [&first, &second, &ignored] {
            storage.insert_account(account).expect("insert account");
        }

        let requested = vec![
            "acc-first".to_string(),
            "acc-missing".to_string(),
            "acc-second".to_string(),
            "acc-first".to_string(),
        ];
        let accounts = storage
            .list_accounts_for_ids(&requested)
            .expect("list accounts for ids");

        assert_eq!(
            accounts
                .into_iter()
                .map(|account| account.id)
                .collect::<Vec<_>>(),
            vec!["acc-second".to_string(), "acc-first".to_string()]
        );
    }

    #[test]
    fn list_account_dashboard_source_metadata_for_ids_reads_dashboard_fields_only() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-dashboard-first", "active", now);
        first.label = "First Dashboard".to_string();
        first.issuer = "ignored-issuer".to_string();
        first.chatgpt_account_id = Some("ignored-chatgpt".to_string());
        first.workspace_id = Some("ignored-workspace".to_string());
        first.sort = 1;
        first.updated_at = now;
        let mut second = sample_account("acc-dashboard-second", "disabled", now);
        second.label = "Second Dashboard".to_string();
        second.sort = 0;
        second.updated_at = now.saturating_sub(10);
        let mut ignored = sample_account("acc-dashboard-ignored", "active", now);
        ignored.label = "Ignored Dashboard".to_string();
        ignored.sort = -1;

        for account in [&first, &second, &ignored] {
            storage.insert_account(account).expect("insert account");
        }

        let metadata = storage
            .list_account_dashboard_source_metadata_for_ids(&[
                "acc-dashboard-first".to_string(),
                "acc-dashboard-missing".to_string(),
                "acc-dashboard-second".to_string(),
                "acc-dashboard-first".to_string(),
            ])
            .expect("list dashboard account metadata");

        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].id, "acc-dashboard-second");
        assert_eq!(metadata[0].label, "Second Dashboard");
        assert_eq!(metadata[0].status, "disabled");
        assert_eq!(metadata[1].id, "acc-dashboard-first");
        assert_eq!(metadata[1].label, "First Dashboard");
        assert_eq!(metadata[1].status, "active");
    }

    #[test]
    fn account_status_and_exists_helpers_read_minimal_account_state() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-status-helper", " Limited ", now);
        account.label = "ignored label".to_string();
        account.issuer = "ignored issuer".to_string();
        account.chatgpt_account_id = Some("ignored-chatgpt".to_string());
        account.workspace_id = Some("ignored-workspace".to_string());
        storage.insert_account(&account).expect("insert account");

        assert_eq!(
            storage
                .find_account_status_by_id("acc-status-helper")
                .expect("find account status")
                .as_deref(),
            Some(" Limited ")
        );
        assert_eq!(
            storage
                .find_account_status_by_id("acc-status-missing")
                .expect("find missing account status"),
            None
        );
        assert!(storage
            .account_exists("acc-status-helper")
            .expect("account exists"));
        assert!(!storage
            .account_exists("acc-status-missing")
            .expect("missing account exists"));

        let plan = collect_query_plan_with_params(
            &storage,
            &format!("EXPLAIN QUERY PLAN {}", account_status_by_id_sql()),
            rusqlite::params!["acc-status-helper"],
        );
        assert!(
            plan.contains("sqlite_autoindex_accounts_1"),
            "expected account status lookup to use account primary-key index, got {plan}"
        );
    }

    #[test]
    fn find_account_workspace_identity_by_id_reads_scope_fields_only() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-workspace-helper", "active", now);
        account.label = "ignored label".to_string();
        account.issuer = "ignored issuer".to_string();
        account.chatgpt_account_id = Some("chatgpt-workspace-helper".to_string());
        account.workspace_id = Some("workspace-helper".to_string());
        account.group_name = Some("ignored group".to_string());
        storage.insert_account(&account).expect("insert account");

        let identity = storage
            .find_account_workspace_identity_by_id("acc-workspace-helper")
            .expect("find workspace identity")
            .expect("identity exists");

        assert_eq!(identity.id, "acc-workspace-helper");
        assert_eq!(
            identity.chatgpt_account_id.as_deref(),
            Some("chatgpt-workspace-helper")
        );
        assert_eq!(identity.workspace_id.as_deref(), Some("workspace-helper"));
        assert!(storage
            .find_account_workspace_identity_by_id("acc-workspace-missing")
            .expect("find missing workspace identity")
            .is_none());

        let plan = collect_query_plan_with_params(
            &storage,
            &format!(
                "EXPLAIN QUERY PLAN {}",
                account_workspace_identity_by_id_sql()
            ),
            rusqlite::params!["acc-workspace-helper"],
        );
        assert!(
            plan.contains("sqlite_autoindex_accounts_1"),
            "expected workspace identity lookup to use account primary-key index, got {plan}"
        );
    }

    #[test]
    fn find_account_upsert_state_by_id_reads_upsert_fields_only() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-upsert-state", "limited", now);
        account.label = "ignored label".to_string();
        account.issuer = "ignored issuer".to_string();
        account.chatgpt_account_id = Some("ignored-chatgpt".to_string());
        account.workspace_id = Some("ignored-workspace".to_string());
        account.group_name = Some("keep group".to_string());
        account.sort = 37;
        account.created_at = now.saturating_sub(20);
        storage.insert_account(&account).expect("insert account");

        let state = storage
            .find_account_upsert_state_by_id("acc-upsert-state")
            .expect("find upsert state")
            .expect("state exists");

        assert_eq!(state.group_name.as_deref(), Some("keep group"));
        assert_eq!(state.sort, 37);
        assert_eq!(state.created_at, now.saturating_sub(20));
        assert!(storage
            .find_account_upsert_state_by_id("acc-upsert-missing")
            .expect("find missing upsert state")
            .is_none());

        let plan = collect_query_plan_with_params(
            &storage,
            &format!("EXPLAIN QUERY PLAN {}", account_upsert_state_by_id_sql()),
            rusqlite::params!["acc-upsert-state"],
        );
        assert!(
            plan.contains("sqlite_autoindex_accounts_1"),
            "expected upsert state lookup to use account primary-key index, got {plan}"
        );
    }

    #[test]
    fn update_account_workspace_identity_only_updates_identity_columns() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-identity-update", "limited", now);
        account.label = "original label".to_string();
        account.issuer = "original issuer".to_string();
        account.chatgpt_account_id = Some("old-chatgpt".to_string());
        account.workspace_id = Some("old-workspace".to_string());
        account.group_name = Some("original group".to_string());
        account.sort = 42;
        storage.insert_account(&account).expect("insert account");

        let changed = storage
            .update_account_workspace_identity(
                "acc-identity-update",
                Some("new-chatgpt"),
                Some("new-workspace"),
                now.saturating_add(5),
            )
            .expect("update identity");

        assert!(changed);
        let updated = storage
            .find_account_by_id("acc-identity-update")
            .expect("find account")
            .expect("account exists");
        assert_eq!(updated.label, "original label");
        assert_eq!(updated.issuer, "original issuer");
        assert_eq!(updated.chatgpt_account_id.as_deref(), Some("new-chatgpt"));
        assert_eq!(updated.workspace_id.as_deref(), Some("new-workspace"));
        assert_eq!(updated.group_name.as_deref(), Some("original group"));
        assert_eq!(updated.sort, 42);
        assert_eq!(updated.status, "limited");
        assert_eq!(updated.created_at, now);
        assert_eq!(updated.updated_at, now.saturating_add(5));
        assert!(!storage
            .update_account_workspace_identity(
                "acc-identity-missing",
                Some("ignored-chatgpt"),
                Some("ignored-workspace"),
                now.saturating_add(6),
            )
            .expect("update missing identity"));
    }

    #[test]
    fn list_account_ids_for_ids_filters_and_reads_only_ids_in_account_order() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-id", "active", now);
        first.sort = 1;
        first.updated_at = now;
        let mut second = sample_account("acc-second-id", "active", now);
        second.sort = 0;
        second.updated_at = now.saturating_sub(10);
        let mut ignored = sample_account("acc-ignored-id", "active", now);
        ignored.sort = -1;

        for account in [&first, &second, &ignored] {
            storage.insert_account(account).expect("insert account");
        }

        let requested = vec![
            "acc-first-id".to_string(),
            "acc-missing-id".to_string(),
            "acc-second-id".to_string(),
            "acc-first-id".to_string(),
        ];
        assert_eq!(
            storage
                .list_account_ids_for_ids(&requested)
                .expect("list account ids for ids"),
            vec!["acc-second-id".to_string(), "acc-first-id".to_string()]
        );
    }

    #[test]
    fn list_account_token_refresh_issuers_for_ids_reads_only_issuer_fields() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-refresh-first", "active", now);
        first.issuer = "https://issuer.first".to_string();
        first.label = "ignored label".to_string();
        first.chatgpt_account_id = Some("ignored-chatgpt".to_string());
        first.workspace_id = Some("ignored-workspace".to_string());
        let mut second = sample_account("acc-refresh-second", "limited", now);
        second.issuer = "https://issuer.second".to_string();
        let mut ignored = sample_account("acc-refresh-ignored", "active", now);
        ignored.issuer = "https://issuer.ignored".to_string();

        for account in [&first, &second, &ignored] {
            storage.insert_account(account).expect("insert account");
        }

        let issuers = storage
            .list_account_token_refresh_issuers_for_ids(&[
                second.id.clone(),
                "acc-refresh-missing".to_string(),
                first.id.clone(),
                second.id.clone(),
            ])
            .expect("list token refresh issuers");

        assert_eq!(
            issuers
                .into_iter()
                .map(|issuer| (issuer.id, issuer.issuer))
                .collect::<Vec<_>>(),
            vec![
                (
                    "acc-refresh-first".to_string(),
                    "https://issuer.first".to_string()
                ),
                (
                    "acc-refresh-second".to_string(),
                    "https://issuer.second".to_string()
                ),
            ]
        );
    }

    #[test]
    fn list_account_ids_reads_only_ids_in_account_order() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-list-id", "active", now);
        first.sort = 1;
        first.updated_at = now;
        let mut second = sample_account("acc-second-list-id", "active", now);
        second.sort = 0;
        second.updated_at = now.saturating_sub(10);

        for account in [&first, &second] {
            storage.insert_account(account).expect("insert account");
        }

        assert_eq!(
            storage.list_account_ids().expect("list account ids"),
            vec![
                "acc-second-list-id".to_string(),
                "acc-first-list-id".to_string()
            ]
        );
    }

    #[test]
    fn list_account_auth_refresh_targets_reads_only_refresh_fields() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-refresh-target", "active", now);
        first.label = "First".to_string();
        first.issuer = "issuer-first".to_string();
        first.sort = 1;
        first.group_name = Some("ignored-group".to_string());
        let mut second = sample_account("acc-second-refresh-target", "disabled", now);
        second.label = "Second".to_string();
        second.issuer = "issuer-second".to_string();
        second.sort = 0;
        second.workspace_id = Some("ignored-workspace".to_string());

        for account in [&first, &second] {
            storage.insert_account(account).expect("insert account");
        }

        let targets = storage
            .list_account_auth_refresh_targets()
            .expect("list account auth refresh targets");

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].id, "acc-second-refresh-target");
        assert_eq!(targets[0].label, "Second");
        assert_eq!(targets[0].issuer, "issuer-second");
        assert_eq!(targets[1].id, "acc-first-refresh-target");
        assert_eq!(targets[1].label, "First");
        assert_eq!(targets[1].issuer, "issuer-first");
    }

    #[test]
    fn list_account_cleanup_candidates_by_statuses_reads_only_id_and_status() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-cleanup", "banned", now);
        first.sort = 2;
        first.label = "ignored label".to_string();
        let mut second = sample_account("acc-second-cleanup", "limited", now);
        second.sort = 1;
        second.chatgpt_account_id = Some("ignored-chatgpt-id".to_string());
        let mut ignored = sample_account("acc-ignored-cleanup", "active", now);
        ignored.sort = 0;

        for account in [&first, &second, &ignored] {
            storage.insert_account(account).expect("insert account");
        }

        let candidates = storage
            .list_account_cleanup_candidates_by_statuses(&[
                "banned".to_string(),
                "limited".to_string(),
            ])
            .expect("list cleanup candidates");

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].id, "acc-second-cleanup");
        assert_eq!(candidates[0].status, "limited");
        assert_eq!(candidates[1].id, "acc-first-cleanup");
        assert_eq!(candidates[1].status, "banned");
    }

    #[test]
    fn list_account_quota_source_summaries_reads_only_source_fields() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-quota-source", "active", now);
        first.label = "First Quota".to_string();
        first.sort = 1;
        first.issuer = "ignored-issuer".to_string();
        let mut second = sample_account("acc-second-quota-source", "limited", now);
        second.label = "Second Quota".to_string();
        second.sort = 0;
        second.workspace_id = Some("ignored-workspace".to_string());

        for account in [&first, &second] {
            storage.insert_account(account).expect("insert account");
        }

        let summaries = storage
            .list_account_quota_source_summaries()
            .expect("list account quota source summaries");

        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, "acc-second-quota-source");
        assert_eq!(summaries[0].label, "Second Quota");
        assert_eq!(summaries[0].status, "limited");
        assert_eq!(summaries[1].id, "acc-first-quota-source");
        assert_eq!(summaries[1].label, "First Quota");
        assert_eq!(summaries[1].status, "active");
    }

    #[test]
    fn list_available_account_quota_pool_sources_filters_and_reads_only_id_label() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-pool-source", "active", now);
        first.label = "First Pool".to_string();
        first.sort = 1;
        first.issuer = "ignored-issuer".to_string();
        let mut second = sample_account("acc-second-pool-source", " AVAILABLE ", now);
        second.label = "Second Pool".to_string();
        second.sort = 0;
        second.workspace_id = Some("ignored-workspace".to_string());
        let mut disabled = sample_account("acc-disabled-pool-source", "disabled", now);
        disabled.label = "Disabled Pool".to_string();
        disabled.sort = -1;

        for account in [&first, &second, &disabled] {
            storage.insert_account(account).expect("insert account");
        }

        let sources = storage
            .list_available_account_quota_pool_sources()
            .expect("list account quota pool sources");

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].id, "acc-second-pool-source");
        assert_eq!(sources[0].label, "Second Pool");
        assert_eq!(sources[1].id, "acc-first-pool-source");
        assert_eq!(sources[1].label, "First Pool");
    }

    #[test]
    fn list_account_import_snapshots_reads_only_import_index_fields() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-import-snapshot", "disabled", now);
        first.label = "First Import".to_string();
        first.issuer = "issuer-first".to_string();
        first.chatgpt_account_id = Some("cgpt-first".to_string());
        first.workspace_id = Some("ws-first".to_string());
        first.group_name = Some("ignored-group".to_string());
        first.sort = 1;
        first.created_at = now.saturating_sub(10);
        let mut second = sample_account("acc-second-import-snapshot", "active", now);
        second.label = "Second Import".to_string();
        second.issuer = "issuer-second".to_string();
        second.sort = 0;

        for account in [&first, &second] {
            storage.insert_account(account).expect("insert account");
        }

        let snapshots = storage
            .list_account_import_snapshots()
            .expect("list account import snapshots");

        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].id, "acc-second-import-snapshot");
        assert_eq!(snapshots[0].label, "Second Import");
        assert_eq!(snapshots[0].issuer, "issuer-second");
        assert_eq!(snapshots[0].sort, 0);
        assert_eq!(snapshots[1].id, "acc-first-import-snapshot");
        assert_eq!(
            snapshots[1].chatgpt_account_id.as_deref(),
            Some("cgpt-first")
        );
        assert_eq!(snapshots[1].workspace_id.as_deref(), Some("ws-first"));
        assert_eq!(snapshots[1].created_at, now.saturating_sub(10));
    }

    #[test]
    fn list_account_summary_rows_reads_only_list_fields() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-summary-row", "active", now);
        first.label = "First Summary".to_string();
        first.group_name = Some("team-a".to_string());
        first.sort = 1;
        first.issuer = "ignored-issuer".to_string();
        let mut second = sample_account("acc-second-summary-row", "disabled", now);
        second.label = "Second Summary".to_string();
        second.sort = 0;
        second.chatgpt_account_id = Some("ignored-chatgpt".to_string());

        for account in [&first, &second] {
            storage.insert_account(account).expect("insert account");
        }

        let rows = storage
            .list_account_summary_rows()
            .expect("list account summary rows");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "acc-second-summary-row");
        assert_eq!(rows[0].label, "Second Summary");
        assert_eq!(rows[0].group_name, None);
        assert_eq!(rows[0].sort, 0);
        assert_eq!(rows[0].status, "disabled");
        assert_eq!(rows[1].id, "acc-first-summary-row");
        assert_eq!(rows[1].group_name.as_deref(), Some("team-a"));
        assert_eq!(rows[1].status, "active");
    }

    #[test]
    fn account_summary_storage_snapshot_loads_related_rows_for_requested_accounts() {
        let mut storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let requested = sample_account("acc-summary-snapshot", "active", now);
        storage
            .insert_account(&requested)
            .expect("insert requested account");
        storage
            .set_preferred_account(Some("acc-summary-snapshot"))
            .expect("set preferred account");
        storage
            .insert_account(&sample_account("acc-summary-ignored", "active", now))
            .expect("insert ignored account");
        storage
            .insert_token(&sample_token("acc-summary-snapshot", now))
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-summary-snapshot".to_string(),
                used_percent: Some(12.5),
                window_minutes: Some(60),
                resets_at: Some(now + 60),
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage snapshot");
        storage
            .upsert_account_metadata("acc-summary-snapshot", Some("note"), Some("tag"))
            .expect("insert metadata");
        storage
            .upsert_account_subscription(
                "acc-summary-snapshot",
                true,
                Some("team"),
                Some("plus"),
                Some(now + 100),
                Some(now + 50),
            )
            .expect("insert subscription");
        storage
            .set_quota_source_model_assignments(
                "openai_account",
                "acc-summary-snapshot",
                &["gpt-visible".to_string()],
            )
            .expect("insert model assignment");
        storage
            .set_quota_source_model_assignments(
                "aggregate_api",
                "acc-summary-snapshot",
                &["gpt-hidden".to_string()],
            )
            .expect("insert unrelated model assignment");
        storage
            .upsert_account_quota_capacity_override("acc-summary-snapshot", Some(100), Some(200))
            .expect("insert quota override");

        let snapshot = storage
            .load_account_summary_storage_snapshot(&["acc-summary-snapshot".to_string()])
            .expect("load account summary snapshot");

        assert_eq!(
            snapshot.preferred_account_id.as_deref(),
            Some("acc-summary-snapshot")
        );
        assert_eq!(snapshot.tokens.len(), 1);
        assert_eq!(snapshot.usage_snapshots.len(), 1);
        assert_eq!(snapshot.metadata.len(), 1);
        assert_eq!(snapshot.metadata[0].note.as_deref(), Some("note"));
        assert_eq!(snapshot.subscriptions.len(), 1);
        assert_eq!(snapshot.subscriptions[0].plan_type.as_deref(), Some("plus"));
        assert_eq!(snapshot.model_assignments.len(), 1);
        assert_eq!(snapshot.model_assignments[0].source_kind, "openai_account");
        assert_eq!(snapshot.model_assignments[0].model_slug, "gpt-visible");
        assert_eq!(snapshot.quota_overrides.len(), 1);
        assert_eq!(snapshot.quota_overrides[0].primary_window_tokens, Some(100));

        let empty = storage
            .load_account_summary_storage_snapshot(&[])
            .expect("load empty account summary snapshot");
        assert!(empty.tokens.is_empty());
        assert!(empty.usage_snapshots.is_empty());
        assert!(empty.metadata.is_empty());
        assert!(empty.subscriptions.is_empty());
        assert!(empty.model_assignments.is_empty());
        assert!(empty.quota_overrides.is_empty());
    }

    #[test]
    fn light_account_summary_storage_snapshot_skips_display_details() {
        let mut storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();
        let account_id = "acc-summary-light";

        storage
            .insert_account(&sample_account(account_id, "active", now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: account_id.to_string(),
                used_percent: Some(18.0),
                window_minutes: Some(180),
                resets_at: Some(now + 300),
                secondary_used_percent: Some(28.0),
                secondary_window_minutes: Some(10_080),
                secondary_resets_at: Some(now + 600),
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage snapshot");
        storage
            .upsert_account_metadata(account_id, Some("note"), Some("tag"))
            .expect("insert metadata");
        storage
            .upsert_account_subscription(account_id, true, Some("team"), Some("plus"), None, None)
            .expect("insert subscription");
        storage
            .set_quota_source_model_assignments(
                "openai_account",
                account_id,
                &["gpt-visible".to_string()],
            )
            .expect("insert model assignment");
        storage
            .upsert_account_quota_capacity_override(account_id, Some(100), Some(200))
            .expect("insert quota override");

        let snapshot = storage
            .load_account_summary_storage_snapshot_with_options(
                &[account_id.to_string()],
                AccountSummaryStorageSnapshotOptions::light(),
            )
            .expect("load light account summary snapshot");

        assert_eq!(snapshot.tokens.len(), 1);
        assert_eq!(snapshot.usage_snapshots.len(), 1);
        assert!(snapshot.metadata.is_empty());
        assert!(snapshot.subscriptions.is_empty());
        assert!(snapshot.model_assignments.is_empty());
        assert!(snapshot.quota_overrides.is_empty());
    }

    #[test]
    fn dashboard_light_account_summary_snapshot_keeps_usage_but_skips_runtime_rows() {
        let mut storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();
        let account_id = "acc-summary-dashboard-light";

        storage
            .insert_account(&sample_account(account_id, "active", now))
            .expect("insert account");
        storage
            .set_preferred_account(Some(account_id))
            .expect("set preferred account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
        storage
            .insert_event(&Event {
                account_id: Some(account_id.to_string()),
                event_type: "account_status".to_string(),
                message: "status=unavailable reason=usage_http_401".to_string(),
                created_at: now,
            })
            .expect("insert status reason event");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: account_id.to_string(),
                used_percent: Some(18.0),
                window_minutes: Some(180),
                resets_at: Some(now + 300),
                secondary_used_percent: Some(28.0),
                secondary_window_minutes: Some(10_080),
                secondary_resets_at: Some(now + 600),
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage snapshot");

        let snapshot = storage
            .load_account_summary_storage_snapshot_with_options(
                &[account_id.to_string()],
                AccountSummaryStorageSnapshotOptions::dashboard_light(),
            )
            .expect("load dashboard light account summary snapshot");

        assert!(snapshot.preferred_account_id.is_none());
        assert!(snapshot.status_reasons.is_empty());
        assert!(snapshot.tokens.is_empty());
        assert_eq!(snapshot.usage_snapshots.len(), 1);
        assert!(snapshot.metadata.is_empty());
        assert!(snapshot.subscriptions.is_empty());
        assert!(snapshot.model_assignments.is_empty());
        assert!(snapshot.quota_overrides.is_empty());
    }

    #[test]
    fn account_status_counts_aggregates_normalized_statuses() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        for (id, status) in [
            ("acc-active-a", "active"),
            ("acc-active-b", " Active "),
            ("acc-disabled", "disabled"),
        ] {
            storage
                .insert_account(&sample_account(id, status, now))
                .expect("insert account");
        }

        let counts = storage
            .account_status_counts()
            .expect("count account statuses");

        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0].status, "active");
        assert_eq!(counts[0].count, 2);
        assert_eq!(counts[1].status, "disabled");
        assert_eq!(counts[1].count, 1);

        let plan = collect_query_plan(
            &storage,
            &format!("EXPLAIN QUERY PLAN {}", account_status_counts_sql()),
        );
        assert!(
            plan.contains("idx_accounts_cleanup_status_lookup"),
            "expected normalized status counts to use cleanup status expression index, got {plan}"
        );
    }

    #[test]
    fn account_group_name_filter_uses_group_sort_index() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut team_a = sample_account("acc-team-a", "active", now);
        team_a.group_name = Some("TEAM_A".to_string());
        team_a.sort = 2;
        let mut team_b = sample_account("acc-team-b", "active", now);
        team_b.group_name = Some("TEAM_B".to_string());
        team_b.sort = 1;

        storage.insert_account(&team_a).expect("insert team a");
        storage.insert_account(&team_b).expect("insert team b");

        let accounts = storage
            .list_accounts_filtered(None, Some("TEAM_A"))
            .expect("filter team a accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "acc-team-a");
        assert_eq!(accounts[0].group_name.as_deref(), Some("TEAM_A"));
        assert_eq!(
            storage
                .account_count_filtered(None, Some("TEAM_A"))
                .expect("count team a accounts"),
            1
        );

        let sql = account_query_sql(" WHERE a.group_name = ?", false);
        let plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("prepare explain")
            .query_map(["TEAM_A"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        assert!(
            plan.iter()
                .any(|detail| detail.contains("idx_accounts_group_name_sort_updated_at")),
            "expected group filter plan to use idx_accounts_group_name_sort_updated_at, got {plan:?}"
        );
    }

    #[test]
    fn account_base_lists_use_sort_updated_id_order_index() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        for (label, sql) in [
            ("account id list", account_ids_list_sql()),
            (
                "account auth refresh target list",
                account_auth_refresh_targets_list_sql(),
            ),
            (
                "account quota source summary list",
                account_quota_source_summaries_list_sql(),
            ),
            (
                "account import snapshot list",
                account_import_snapshots_list_sql(),
            ),
            ("account summary row list", account_summary_rows_list_sql()),
        ] {
            let plan = storage
                .conn
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .expect("prepare explain")
                .query_map([], |row| row.get::<_, String>(3))
                .expect("query explain")
                .collect::<Result<Vec<_>>>()
                .expect("collect explain");

            assert!(
                plan.iter()
                    .any(|detail| detail.contains("idx_accounts_list_order")),
                "expected {label} to use account list-order index, got {plan:?}"
            );
            assert!(
                !plan
                    .iter()
                    .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
                "expected {label} to avoid a temp sort, got {plan:?}"
            );
        }
    }

    #[test]
    fn list_accounts_by_statuses_filters_and_preserves_account_order() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut active = sample_account("acc-active", "active", now);
        active.sort = 0;
        let mut banned = sample_account("acc-banned", " BANNED ", now);
        banned.sort = 1;
        let mut limited = sample_account("acc-limited", "limited", now);
        limited.sort = 2;
        let mut disabled = sample_account("acc-disabled", "disabled", now);
        disabled.sort = 3;
        for account in [&active, &banned, &limited, &disabled] {
            storage.insert_account(account).expect("insert account");
        }

        let accounts = storage
            .list_accounts_by_statuses(&["limited".to_string(), "banned".to_string()])
            .expect("list accounts by statuses");

        assert_eq!(
            accounts
                .into_iter()
                .map(|account| account.id)
                .collect::<Vec<_>>(),
            vec!["acc-banned".to_string(), "acc-limited".to_string()]
        );
    }

    #[test]
    fn account_status_chunk_queries_defer_final_ordering_to_rust() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        let account_ids_sql =
            account_ids_by_statuses_chunk_sql("LOWER(TRIM(COALESCE(status, ''))) IN (?1, ?2)");
        let account_ids_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {account_ids_sql}"))
            .expect("prepare explain")
            .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        let accounts_sql =
            accounts_by_statuses_chunk_sql("LOWER(TRIM(COALESCE(a.status, ''))) IN (?1, ?2)");
        let accounts_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {accounts_sql}"))
            .expect("prepare explain")
            .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        let refresh_targets_sql = account_usage_refresh_targets_by_statuses_chunk_sql(
            "LOWER(TRIM(COALESCE(status, ''))) IN (?1, ?2)",
        );
        let refresh_targets_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {refresh_targets_sql}"))
            .expect("prepare explain")
            .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        let usable_refresh_targets_sql =
            account_usage_refresh_targets_with_usable_tokens_by_statuses_chunk_sql(
                "LOWER(TRIM(COALESCE(a.status, ''))) IN (?1, ?2)",
            );
        let usable_refresh_targets_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {usable_refresh_targets_sql}"))
            .expect("prepare explain")
            .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");

        assert!(
            !account_ids_plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "status id chunk query should avoid per-chunk ORDER BY temp sorting, got {account_ids_plan:?}"
        );
        assert!(
            !accounts_plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "status account chunk query should avoid per-chunk ORDER BY temp sorting, got {accounts_plan:?}"
        );
        assert!(
            !refresh_targets_plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "usage refresh target chunk query should avoid per-chunk ORDER BY temp sorting, got {refresh_targets_plan:?}"
        );
        assert!(
            !usable_refresh_targets_plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "usable-token usage refresh target chunk query should avoid per-chunk ORDER BY temp sorting, got {usable_refresh_targets_plan:?}"
        );
        for (label, plan) in [
            ("status id", account_ids_plan),
            ("status account", accounts_plan),
            ("usage refresh target", refresh_targets_plan),
            (
                "usable-token usage refresh target",
                usable_refresh_targets_plan,
            ),
        ] {
            assert!(
                plan.iter()
                    .any(|detail| detail.contains("idx_accounts_cleanup_status_lookup")),
                "expected {label} chunk query to use normalized status index, got {plan:?}"
            );
        }
    }

    #[test]
    fn account_cleanup_candidates_use_normalized_status_index() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        let sql = account_cleanup_candidates_by_statuses_chunk_sql(
            "LOWER(TRIM(COALESCE(status, ''))) IN (?1, ?2)",
        );
        let plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("prepare explain")
            .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");

        assert!(
            plan.iter()
                .any(|detail| detail.contains("idx_accounts_cleanup_status_lookup")),
            "expected cleanup candidate query to use normalized status index, got {plan:?}"
        );
    }

    #[test]
    fn account_id_chunk_queries_defer_final_ordering_to_rust() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        let account_ids_sql = account_ids_for_ids_chunk_sql("id IN (?1, ?2)");
        let account_ids_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {account_ids_sql}"))
            .expect("prepare explain")
            .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        let accounts_sql = accounts_for_ids_chunk_sql("a.id IN (?1, ?2)");
        let accounts_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {accounts_sql}"))
            .expect("prepare explain")
            .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        let token_refresh_issuers_sql =
            account_token_refresh_issuers_for_ids_chunk_sql("id IN (?1, ?2)");
        let token_refresh_issuers_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {token_refresh_issuers_sql}"))
            .expect("prepare explain")
            .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        let dashboard_metadata_sql =
            account_dashboard_source_metadata_for_ids_chunk_sql("id IN (?1, ?2)");
        let dashboard_metadata_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {dashboard_metadata_sql}"))
            .expect("prepare explain")
            .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        let codex_profile_sql =
            active_account_codex_profile_candidates_for_ids_chunk_sql("id IN (?1, ?2)");
        let codex_profile_plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {codex_profile_sql}"))
            .expect("prepare explain")
            .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");

        assert!(
            !account_ids_plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "id chunk query should avoid per-chunk ORDER BY temp sorting, got {account_ids_plan:?}"
        );
        assert!(
            account_ids_plan
                .iter()
                .any(|detail| detail.contains("sqlite_autoindex_accounts_1")),
            "expected id chunk query to use account primary-key lookup, got {account_ids_plan:?}"
        );
        assert!(
            accounts_plan
                .iter()
                .any(|detail| detail.contains("sqlite_autoindex_accounts_1")),
            "expected full account id chunk query to use account primary-key lookup, got {accounts_plan:?}"
        );
        assert!(
            !accounts_plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "full account id chunk query should avoid per-chunk ORDER BY temp sorting, got {accounts_plan:?}"
        );
        for (label, plan) in [
            ("token refresh issuer", token_refresh_issuers_plan),
            ("dashboard metadata", dashboard_metadata_plan),
            ("codex profile candidate", codex_profile_plan),
        ] {
            assert!(
                plan.iter()
                    .any(|detail| detail.contains("sqlite_autoindex_accounts_1")),
                "expected {label} chunk query to use account primary-key lookup, got {plan:?}"
            );
            assert!(
                !plan
                    .iter()
                    .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
                "{label} chunk query should avoid per-chunk ORDER BY temp sorting, got {plan:?}"
            );
        }
    }

    #[test]
    fn list_account_ids_by_statuses_filters_and_reads_only_ids_in_account_order() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut active = sample_account("acc-active", "active", now);
        active.sort = 0;
        let mut banned = sample_account("acc-banned", " BANNED ", now);
        banned.sort = 1;
        let mut limited = sample_account("acc-limited", "limited", now);
        limited.sort = 2;
        let mut disabled = sample_account("acc-disabled", "disabled", now);
        disabled.sort = 3;
        for account in [&active, &banned, &limited, &disabled] {
            storage.insert_account(account).expect("insert account");
        }

        let ids = storage
            .list_account_ids_by_statuses(&["limited".to_string(), "banned".to_string()])
            .expect("list account ids by statuses");

        assert_eq!(
            ids,
            vec!["acc-banned".to_string(), "acc-limited".to_string()]
        );
    }

    #[test]
    fn list_account_usage_refresh_targets_by_statuses_reads_only_refresh_fields() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut active = sample_account("acc-active-refresh", "active", now);
        active.sort = 1;
        active.workspace_id = Some("ws-active".to_string());
        active.label = "ignored label".to_string();
        let mut inactive = sample_account("acc-inactive-refresh", " INACTIVE ", now);
        inactive.sort = 0;
        inactive.workspace_id = Some("ws-inactive".to_string());
        inactive.issuer = "ignored issuer".to_string();
        let mut disabled = sample_account("acc-disabled-refresh", "disabled", now);
        disabled.sort = -1;
        disabled.workspace_id = Some("ws-disabled".to_string());
        for account in [&active, &inactive, &disabled] {
            storage.insert_account(account).expect("insert account");
        }

        let targets = storage
            .list_account_usage_refresh_targets_by_statuses(&[
                "active".to_string(),
                "inactive".to_string(),
            ])
            .expect("list account usage refresh targets");

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].id, "acc-inactive-refresh");
        assert_eq!(targets[0].status, " INACTIVE ");
        assert_eq!(targets[0].workspace_id.as_deref(), Some("ws-inactive"));
        assert_eq!(targets[1].id, "acc-active-refresh");
        assert_eq!(targets[1].status, "active");
        assert_eq!(targets[1].workspace_id.as_deref(), Some("ws-active"));
    }

    #[test]
    fn list_account_usage_refresh_targets_with_usable_tokens_by_statuses_filters_in_sql() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut active = sample_account("acc-active-token-refresh", "active", now);
        active.sort = 1;
        active.workspace_id = Some("ws-active-token".to_string());
        let mut inactive = sample_account("acc-inactive-token-refresh", " INACTIVE ", now);
        inactive.sort = 0;
        inactive.workspace_id = Some("ws-inactive-token".to_string());
        let mut disabled = sample_account("acc-disabled-token-refresh", "disabled", now);
        disabled.sort = -1;
        let no_access = sample_account("acc-no-access-token-refresh", "active", now);
        let no_refresh = sample_account("acc-no-refresh-token-refresh", "active", now);
        let missing_token = sample_account("acc-missing-token-refresh", "active", now);
        for account in [
            &active,
            &inactive,
            &disabled,
            &no_access,
            &no_refresh,
            &missing_token,
        ] {
            storage.insert_account(account).expect("insert account");
        }
        storage
            .insert_token(&sample_token(active.id.as_str(), now))
            .expect("insert active token");
        storage
            .insert_token(&sample_token(inactive.id.as_str(), now))
            .expect("insert inactive token");
        storage
            .insert_token(&sample_token(disabled.id.as_str(), now))
            .expect("insert disabled token");
        storage
            .insert_token(&Token {
                access_token: " ".to_string(),
                ..sample_token(no_access.id.as_str(), now)
            })
            .expect("insert no access token");
        storage
            .insert_token(&Token {
                refresh_token: String::new(),
                ..sample_token(no_refresh.id.as_str(), now)
            })
            .expect("insert no refresh token");

        let targets = storage
            .list_account_usage_refresh_targets_with_usable_tokens_by_statuses(&[
                "active".to_string(),
                "inactive".to_string(),
            ])
            .expect("list account usage refresh targets with usable tokens");

        assert_eq!(
            targets
                .iter()
                .map(|target| target.id.as_str())
                .collect::<Vec<_>>(),
            vec!["acc-inactive-token-refresh", "acc-active-token-refresh"]
        );
        assert_eq!(targets[0].status, " INACTIVE ");
        assert_eq!(
            targets[0].workspace_id.as_deref(),
            Some("ws-inactive-token")
        );
        assert_eq!(targets[1].workspace_id.as_deref(), Some("ws-active-token"));
    }

    #[test]
    fn list_account_usage_refresh_token_targets_filters_blocked_latest_status_in_sql() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut ready = sample_account("acc-ready-token-target", "active", now);
        ready.sort = 1;
        ready.workspace_id = Some("ws-ready".to_string());
        let mut recovered = sample_account("acc-recovered-token-target", "active", now);
        recovered.sort = 0;
        recovered.workspace_id = Some("ws-recovered".to_string());
        let mut blocked = sample_account("acc-blocked-token-target", "active", now);
        blocked.sort = 2;
        let mut no_access = sample_account("acc-no-access-token-target", "active", now);
        no_access.sort = 3;
        let disabled = sample_account("acc-disabled-token-target", "disabled", now);

        for account in [&ready, &recovered, &blocked, &no_access, &disabled] {
            storage.insert_account(account).expect("insert account");
        }
        for account in [&ready, &recovered, &blocked, &disabled] {
            storage
                .insert_token(&sample_token(account.id.as_str(), now))
                .expect("insert token");
        }
        storage
            .insert_token(&Token {
                access_token: " ".to_string(),
                ..sample_token(no_access.id.as_str(), now)
            })
            .expect("insert no access token");

        storage
            .insert_event(&Event {
                account_id: Some(recovered.id.clone()),
                event_type: "account_status_update".to_string(),
                message: "status=banned reason=account_deactivated".to_string(),
                created_at: now,
            })
            .expect("insert old recovered blocked event");
        storage
            .insert_event(&Event {
                account_id: Some(recovered.id.clone()),
                event_type: "account_status_update".to_string(),
                message: "status=active reason=manual_reactivated".to_string(),
                created_at: now + 1,
            })
            .expect("insert latest recovered allowed event");
        storage
            .insert_event(&Event {
                account_id: Some(blocked.id.clone()),
                event_type: "account_status_update".to_string(),
                message: "status=banned reason=refresh_token_region_blocked".to_string(),
                created_at: now + 2,
            })
            .expect("insert blocked event");

        let targets = storage
            .list_account_usage_refresh_token_targets_by_statuses(&[
                "active".to_string(),
                "inactive".to_string(),
            ])
            .expect("list usage refresh token targets");

        assert_eq!(
            targets
                .iter()
                .map(|target| target.account_id.as_str())
                .collect::<Vec<_>>(),
            vec!["acc-recovered-token-target", "acc-ready-token-target"]
        );
        assert_eq!(targets[0].workspace_id.as_deref(), Some("ws-recovered"));
        assert_eq!(targets[0].token.account_id, "acc-recovered-token-target");
        assert_eq!(targets[0].token.access_token, "access");
        assert_eq!(targets[1].workspace_id.as_deref(), Some("ws-ready"));
    }

    #[test]
    fn usage_refresh_token_targets_scope_latest_status_to_target_accounts() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        let sql = usage_refresh_token_targets_by_status_sql(
            "LOWER(TRIM(COALESCE(a.status, ''))) IN ('active', 'inactive')",
        );
        let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));

        assert!(
            sql.contains("INNER JOIN target_accounts ta"),
            "expected latest status CTE to join target accounts before ranking, got {sql}"
        );
        assert!(
            plan.contains("idx_events_account_status_lookup"),
            "expected scoped latest status lookup to use event account status index, got {plan}"
        );
    }

    #[test]
    fn find_account_direct_auth_profile_by_id_reads_direct_auth_fields_only() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-direct-auth-profile", " active ", now);
        account.label = "ignored label".to_string();
        account.issuer = "https://auth.example.test".to_string();
        account.chatgpt_account_id = Some("chatgpt-direct-auth".to_string());
        account.workspace_id = Some("ignored-workspace".to_string());
        account.group_name = Some("ignored group".to_string());
        account.sort = 99;
        storage.insert_account(&account).expect("insert account");

        let profile = storage
            .find_account_direct_auth_profile_by_id("acc-direct-auth-profile")
            .expect("find direct auth profile")
            .expect("profile exists");

        assert_eq!(profile.id, "acc-direct-auth-profile");
        assert_eq!(profile.issuer, "https://auth.example.test");
        assert_eq!(
            profile.chatgpt_account_id.as_deref(),
            Some("chatgpt-direct-auth")
        );
        assert_eq!(profile.status, " active ");
        assert!(storage
            .find_account_direct_auth_profile_by_id("acc-missing-direct-auth")
            .expect("find missing direct auth profile")
            .is_none());

        let plan = collect_query_plan_with_params(
            &storage,
            &format!(
                "EXPLAIN QUERY PLAN {}",
                account_direct_auth_profile_by_id_sql()
            ),
            rusqlite::params!["acc-direct-auth-profile"],
        );
        assert!(
            plan.contains("sqlite_autoindex_accounts_1"),
            "expected direct auth profile lookup to use account primary-key index, got {plan}"
        );
    }

    #[test]
    fn list_active_account_codex_profile_candidates_for_ids_filters_active_and_reads_candidate_fields(
    ) {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first-codex-profile", "active", now);
        first.label = "First Codex".to_string();
        first.issuer = "issuer-first".to_string();
        first.chatgpt_account_id = Some("cgpt-first".to_string());
        first.workspace_id = Some("ws-first".to_string());
        first.group_name = Some("group-first".to_string());
        first.sort = 1;
        let mut second = sample_account("acc-second-codex-profile", " ACTIVE ", now);
        second.label = "Second Codex".to_string();
        second.issuer = "issuer-second".to_string();
        second.chatgpt_account_id = Some("cgpt-second".to_string());
        second.workspace_id = Some("ws-second".to_string());
        second.group_name = Some("group-second".to_string());
        second.sort = 0;
        let mut disabled = sample_account("acc-disabled-codex-profile", "disabled", now);
        disabled.label = "Disabled Codex".to_string();
        disabled.sort = -1;
        for account in [&first, &second, &disabled] {
            storage.insert_account(account).expect("insert account");
        }

        let targets = storage
            .list_active_account_codex_profile_candidates_for_ids(&[
                "acc-disabled-codex-profile".to_string(),
                "acc-first-codex-profile".to_string(),
                "acc-second-codex-profile".to_string(),
            ])
            .expect("list codex profile account candidates");

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].id, "acc-second-codex-profile");
        assert_eq!(targets[0].label, "Second Codex");
        assert_eq!(targets[0].issuer, "issuer-second");
        assert_eq!(
            targets[0].chatgpt_account_id.as_deref(),
            Some("cgpt-second")
        );
        assert_eq!(targets[0].workspace_id.as_deref(), Some("ws-second"));
        assert_eq!(targets[0].group_name.as_deref(), Some("group-second"));
        assert_eq!(targets[0].status, " ACTIVE ");
        assert_eq!(targets[1].id, "acc-first-codex-profile");
        assert_eq!(targets[1].label, "First Codex");
        assert_eq!(targets[1].issuer, "issuer-first");
    }

    #[test]
    fn find_account_by_identity_prefers_id_then_chatgpt_then_workspace() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut by_id = sample_account("acc-by-id", "active", now);
        by_id.chatgpt_account_id = Some("cgpt-shared".to_string());
        by_id.workspace_id = Some("ws-shared".to_string());
        by_id.updated_at = now;
        let mut by_chatgpt = sample_account("acc-by-chatgpt", "active", now);
        by_chatgpt.chatgpt_account_id = Some("cgpt-shared".to_string());
        by_chatgpt.workspace_id = Some("ws-chatgpt".to_string());
        by_chatgpt.updated_at = now.saturating_add(10);
        let mut by_workspace = sample_account("acc-by-workspace", "active", now);
        by_workspace.chatgpt_account_id = Some("cgpt-workspace".to_string());
        by_workspace.workspace_id = Some("ws-shared".to_string());
        by_workspace.updated_at = now.saturating_add(20);

        storage.insert_account(&by_id).expect("insert by id");
        storage
            .insert_account(&by_chatgpt)
            .expect("insert by chatgpt");
        storage
            .insert_account(&by_workspace)
            .expect("insert by workspace");

        let found_by_id = storage
            .find_account_by_identity(Some("acc-by-id"), Some("cgpt-shared"), Some("ws-shared"))
            .expect("find by identity")
            .expect("account exists");
        assert_eq!(found_by_id.id, "acc-by-id");

        let found_by_chatgpt = storage
            .find_account_by_identity(None, Some("cgpt-shared"), Some("ws-shared"))
            .expect("find by chatgpt")
            .expect("account exists");
        assert_eq!(found_by_chatgpt.id, "acc-by-chatgpt");

        let found_by_workspace = storage
            .find_account_by_identity(None, None, Some("ws-shared"))
            .expect("find by workspace")
            .expect("account exists");
        assert_eq!(found_by_workspace.id, "acc-by-workspace");

        assert!(storage
            .find_account_by_identity(None, Some("missing"), Some("also-missing"))
            .expect("find missing identity")
            .is_none());
    }

    #[test]
    fn find_account_id_by_identity_reads_id_only_with_same_priority() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut by_id = sample_account("acc-id-only-by-id", "active", now);
        by_id.label = "ignored label".to_string();
        by_id.chatgpt_account_id = Some("cgpt-id-only-shared".to_string());
        by_id.workspace_id = Some("ws-id-only-shared".to_string());
        by_id.updated_at = now;
        let mut by_chatgpt = sample_account("acc-id-only-by-chatgpt", "active", now);
        by_chatgpt.chatgpt_account_id = Some("cgpt-id-only-shared".to_string());
        by_chatgpt.workspace_id = Some("ws-id-only-chatgpt".to_string());
        by_chatgpt.updated_at = now.saturating_add(10);
        let mut by_workspace = sample_account("acc-id-only-by-workspace", "active", now);
        by_workspace.chatgpt_account_id = Some("cgpt-id-only-workspace".to_string());
        by_workspace.workspace_id = Some("ws-id-only-shared".to_string());
        by_workspace.updated_at = now.saturating_add(20);

        for account in [&by_id, &by_chatgpt, &by_workspace] {
            storage.insert_account(account).expect("insert account");
        }

        assert_eq!(
            storage
                .find_account_id_by_identity(
                    Some("acc-id-only-by-id"),
                    Some("cgpt-id-only-shared"),
                    Some("ws-id-only-shared")
                )
                .expect("find by id")
                .as_deref(),
            Some("acc-id-only-by-id")
        );
        assert_eq!(
            storage
                .find_account_id_by_identity(
                    None,
                    Some("cgpt-id-only-shared"),
                    Some("ws-id-only-shared")
                )
                .expect("find by chatgpt")
                .as_deref(),
            Some("acc-id-only-by-chatgpt")
        );
        assert_eq!(
            storage
                .find_account_id_by_identity(None, None, Some("ws-id-only-shared"))
                .expect("find by workspace")
                .as_deref(),
            Some("acc-id-only-by-workspace")
        );
        assert!(storage
            .find_account_id_by_identity(None, Some("missing"), Some("also-missing"))
            .expect("find missing id")
            .is_none());
    }

    #[test]
    fn account_identity_lookup_uses_identity_indexes() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-indexed-identity", "active", now);
        account.chatgpt_account_id = Some("cgpt-indexed".to_string());
        account.workspace_id = Some("ws-indexed".to_string());
        storage.insert_account(&account).expect("insert account");

        for (column, index_name, value) in [
            (
                "chatgpt_account_id",
                "idx_accounts_chatgpt_account_id_updated_at",
                "cgpt-indexed",
            ),
            (
                "workspace_id",
                "idx_accounts_workspace_id_updated_at",
                "ws-indexed",
            ),
        ] {
            let sql = format!("EXPLAIN QUERY PLAN {}", account_identity_lookup_sql(column));
            let plan = storage
                .conn
                .prepare(&sql)
                .expect("prepare explain")
                .query_map([value], |row| row.get::<_, String>(3))
                .expect("query explain")
                .collect::<Result<Vec<_>>>()
                .expect("collect explain");
            assert!(
                plan.iter().any(|detail| detail.contains(index_name)),
                "expected {column} lookup to use {index_name}, got {plan:?}"
            );
        }
    }

    #[test]
    fn matching_identity_accounts_only_returns_identity_candidates() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut by_chatgpt = sample_account("acc-by-chatgpt", "active", now);
        by_chatgpt.chatgpt_account_id = Some("cgpt-target".to_string());
        by_chatgpt.workspace_id = Some("ws-other".to_string());
        by_chatgpt.updated_at = now.saturating_add(10);
        let mut by_workspace = sample_account("acc-by-workspace", "active", now);
        by_workspace.chatgpt_account_id = Some("cgpt-other".to_string());
        by_workspace.workspace_id = Some("ws-target".to_string());
        by_workspace.updated_at = now.saturating_add(20);
        let by_id = sample_account("acc-by-id", "active", now);
        let unrelated = sample_account("acc-unrelated", "active", now.saturating_add(30));

        for account in [&by_chatgpt, &by_workspace, &by_id, &unrelated] {
            storage.insert_account(account).expect("insert account");
        }

        let accounts = storage
            .list_accounts_matching_identity(
                &["acc-by-id".to_string()],
                Some("cgpt-target"),
                Some("ws-target"),
            )
            .expect("list identity candidates");

        assert_eq!(
            accounts
                .into_iter()
                .map(|account| account.id)
                .collect::<Vec<_>>(),
            vec![
                "acc-by-workspace".to_string(),
                "acc-by-chatgpt".to_string(),
                "acc-by-id".to_string()
            ]
        );
    }

    #[test]
    fn matching_identity_workspace_identities_only_reads_identity_candidates() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut by_chatgpt = sample_account("acc-identity-by-chatgpt", "active", now);
        by_chatgpt.label = "ignored label".to_string();
        by_chatgpt.issuer = "ignored issuer".to_string();
        by_chatgpt.chatgpt_account_id = Some("cgpt-target".to_string());
        by_chatgpt.workspace_id = Some("ws-other".to_string());
        by_chatgpt.updated_at = now.saturating_add(10);
        let mut by_workspace = sample_account("acc-identity-by-workspace", "active", now);
        by_workspace.group_name = Some("ignored group".to_string());
        by_workspace.chatgpt_account_id = Some("cgpt-other".to_string());
        by_workspace.workspace_id = Some("ws-target".to_string());
        by_workspace.updated_at = now.saturating_add(20);
        let by_id = sample_account("acc-identity-by-id", "active", now);
        let unrelated = sample_account("acc-identity-unrelated", "active", now.saturating_add(30));

        for account in [&by_chatgpt, &by_workspace, &by_id, &unrelated] {
            storage.insert_account(account).expect("insert account");
        }

        let identities = storage
            .list_account_workspace_identities_matching_identity(
                &["acc-identity-by-id".to_string()],
                Some("cgpt-target"),
                Some("ws-target"),
            )
            .expect("list identity candidates");

        assert_eq!(
            identities
                .iter()
                .map(|identity| identity.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "acc-identity-by-workspace",
                "acc-identity-by-chatgpt",
                "acc-identity-by-id"
            ]
        );
        assert_eq!(identities[0].workspace_id.as_deref(), Some("ws-target"));
        assert_eq!(
            identities[1].chatgpt_account_id.as_deref(),
            Some("cgpt-target")
        );
    }

    #[test]
    fn list_gateway_candidates_only_returns_active_available_accounts() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let active_available = sample_account("acc-active-ok", "active", now);
        let active_missing_usage = sample_account("acc-active-missing", "active", now);
        let unavailable = sample_account("acc-unavailable", "unavailable", now);

        for account in [&active_available, &active_missing_usage, &unavailable] {
            storage.insert_account(account).expect("insert account");
            storage
                .insert_token(&sample_token(account.id.as_str(), now))
                .expect("insert token");
        }

        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: active_available.id.clone(),
                used_percent: Some(12.0),
                window_minutes: Some(180),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: unavailable.id.clone(),
                used_percent: Some(10.0),
                window_minutes: Some(180),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage");

        let candidates = storage
            .list_gateway_candidates()
            .expect("list gateway candidates");
        let mut ids = candidates
            .into_iter()
            .map(|(account, _)| account.id)
            .collect::<Vec<_>>();
        ids.sort();

        assert_eq!(
            ids,
            vec![
                "acc-active-missing".to_string(),
                "acc-active-ok".to_string()
            ]
        );
    }

    #[test]
    fn list_gateway_candidates_for_accounts_filters_requested_available_accounts() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut first = sample_account("acc-first", "active", now);
        first.sort = 1;
        let mut second = sample_account("acc-second", "active", now);
        second.sort = 0;
        let mut saturated = sample_account("acc-saturated", "active", now);
        saturated.sort = 2;
        let disabled = sample_account("acc-disabled", "disabled", now);
        let unrequested = sample_account("acc-unrequested", "active", now);

        for account in [&first, &second, &saturated, &disabled, &unrequested] {
            storage.insert_account(account).expect("insert account");
            storage
                .insert_token(&sample_token(account.id.as_str(), now))
                .expect("insert token");
        }

        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: saturated.id.clone(),
                used_percent: Some(100.0),
                window_minutes: Some(180),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert saturated usage");

        let requested = vec![
            "acc-first".to_string(),
            "acc-disabled".to_string(),
            "acc-missing".to_string(),
            "acc-second".to_string(),
            "acc-saturated".to_string(),
        ];
        let candidates = storage
            .list_gateway_candidates_for_accounts(&requested)
            .expect("list selected gateway candidates");

        assert_eq!(
            candidates
                .into_iter()
                .map(|(account, _token)| account.id)
                .collect::<Vec<_>>(),
            vec!["acc-second".to_string(), "acc-first".to_string()]
        );
    }

    #[test]
    fn gateway_candidates_for_accounts_scope_latest_usage_cte_to_requested_ids() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let latest_usage_cte =
            latest_usage_cte_sql_for_condition("account_id IN ('acc-first', 'acc-second')");
        let availability_clause = gateway_account_usage_filter_clause("a", "lu");
        let gateway_sql = gateway_candidates_filtered_sql(
            &latest_usage_cte,
            &format!("a.id IN ('acc-first', 'acc-second') AND {availability_clause}"),
        );
        let sql = format!("EXPLAIN QUERY PLAN {gateway_sql}");
        let plan = collect_query_plan(&storage, &sql);

        assert!(
            plan.contains("idx_usage_snapshots_account_captured_id"),
            "expected account-scoped latest usage CTE to use account lookup index, got {plan}"
        );
    }

    #[test]
    fn find_account_with_token_by_id_returns_joined_account_and_token() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-current-with-token", "active", now);
        account.label = "Current Account".to_string();
        storage.insert_account(&account).expect("insert account");
        storage
            .insert_token(&sample_token("acc-current-with-token", now))
            .expect("insert token");
        storage
            .insert_account(&sample_account("acc-without-token", "active", now))
            .expect("insert account without token");

        let (found_account, found_token) = storage
            .find_account_with_token_by_id("acc-current-with-token")
            .expect("find account with token")
            .expect("account with token exists");

        assert_eq!(found_account.id, "acc-current-with-token");
        assert_eq!(found_account.label, "Current Account");
        assert_eq!(found_token.account_id, "acc-current-with-token");
        assert!(storage
            .find_account_with_token_by_id("acc-without-token")
            .expect("find account without token")
            .is_none());
        assert!(storage
            .find_account_with_token_by_id("acc-missing-token-join")
            .expect("find missing account with token")
            .is_none());
    }

    #[test]
    fn find_account_with_token_by_identity_preserves_priority_and_requires_token() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut by_id = sample_account("acc-token-by-id", "active", now);
        by_id.chatgpt_account_id = Some("cgpt-token-shared".to_string());
        by_id.workspace_id = Some("ws-token-shared".to_string());
        by_id.updated_at = now;
        let mut by_chatgpt = sample_account("acc-token-by-chatgpt", "active", now);
        by_chatgpt.chatgpt_account_id = Some("cgpt-token-shared".to_string());
        by_chatgpt.workspace_id = Some("ws-token-chatgpt".to_string());
        by_chatgpt.updated_at = now.saturating_add(10);
        let mut by_workspace = sample_account("acc-token-by-workspace", "active", now);
        by_workspace.chatgpt_account_id = Some("cgpt-token-workspace".to_string());
        by_workspace.workspace_id = Some("ws-token-shared".to_string());
        by_workspace.updated_at = now.saturating_add(20);
        let mut without_token = sample_account("acc-token-missing", "active", now);
        without_token.workspace_id = Some("ws-token-missing".to_string());

        for account in [&by_id, &by_chatgpt, &by_workspace, &without_token] {
            storage.insert_account(account).expect("insert account");
        }
        for account_id in [
            "acc-token-by-id",
            "acc-token-by-chatgpt",
            "acc-token-by-workspace",
        ] {
            storage
                .insert_token(&sample_token(account_id, now))
                .expect("insert token");
        }

        let (account, token) = storage
            .find_account_with_token_by_identity(
                Some("acc-token-by-id"),
                Some("cgpt-token-shared"),
                Some("ws-token-shared"),
            )
            .expect("find by id")
            .expect("id match exists");
        assert_eq!(account.id, "acc-token-by-id");
        assert_eq!(token.account_id, "acc-token-by-id");

        let (account, _) = storage
            .find_account_with_token_by_identity(
                None,
                Some("cgpt-token-shared"),
                Some("ws-token-shared"),
            )
            .expect("find by chatgpt")
            .expect("chatgpt match exists");
        assert_eq!(account.id, "acc-token-by-chatgpt");

        let (account, _) = storage
            .find_account_with_token_by_identity(None, None, Some("ws-token-shared"))
            .expect("find by workspace")
            .expect("workspace match exists");
        assert_eq!(account.id, "acc-token-by-workspace");

        assert!(storage
            .find_account_with_token_by_identity(None, None, Some("ws-token-missing"))
            .expect("find account without token")
            .is_none());
    }

    #[test]
    fn delete_accounts_removes_accounts_and_dependent_rows_in_one_call() {
        let mut storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        for account_id in ["acc-delete-a", "acc-delete-b", "acc-keep"] {
            storage
                .insert_account(&sample_account(account_id, "active", now))
                .expect("insert account");
            storage
                .insert_token(&sample_token(account_id, now))
                .expect("insert token");
            storage
                .insert_usage_snapshot(&UsageSnapshotRecord {
                    account_id: account_id.to_string(),
                    used_percent: Some(42.0),
                    window_minutes: Some(300),
                    resets_at: None,
                    secondary_used_percent: None,
                    secondary_window_minutes: None,
                    secondary_resets_at: None,
                    credits_json: Some("{}".to_string()),
                    captured_at: now,
                })
                .expect("insert usage snapshot");
            storage
                .insert_event(&Event {
                    account_id: Some(account_id.to_string()),
                    event_type: "test".to_string(),
                    message: "event".to_string(),
                    created_at: now,
                })
                .expect("insert event");
            storage
                .upsert_conversation_binding(&ConversationBinding {
                    platform_key_hash: format!("hash-{account_id}"),
                    conversation_id: format!("conversation-{account_id}"),
                    account_id: account_id.to_string(),
                    thread_epoch: 1,
                    thread_anchor: String::new(),
                    status: "active".to_string(),
                    last_model: None,
                    last_switch_reason: None,
                    created_at: now,
                    updated_at: now,
                    last_used_at: now,
                })
                .expect("insert conversation binding");
            storage
                .upsert_model_source_model(&ModelSourceModel {
                    source_kind: "openai_account".to_string(),
                    source_id: account_id.to_string(),
                    upstream_model: "gpt-test".to_string(),
                    display_name: Some("GPT Test".to_string()),
                    status: "available".to_string(),
                    discovery_kind: "test".to_string(),
                    last_synced_at: Some(now),
                    extra_json: "{}".to_string(),
                    created_at: now,
                    updated_at: now,
                })
                .expect("insert model source model");
            storage
                .upsert_model_source_mapping(&ModelSourceMapping {
                    id: format!("mapping-{account_id}"),
                    platform_model_slug: "gpt-test".to_string(),
                    source_kind: "openai_account".to_string(),
                    source_id: account_id.to_string(),
                    upstream_model: "gpt-test".to_string(),
                    enabled: true,
                    priority: 1,
                    weight: 1,
                    billing_model_slug: None,
                    created_at: now,
                    updated_at: now,
                })
                .expect("insert model source mapping");
            storage
                .upsert_model_source_mapping_preference(
                    "openai_account",
                    account_id,
                    "gpt-test",
                    "unlinked",
                )
                .expect("insert model source preference");
        }

        storage
            .upsert_model_source_model(&ModelSourceModel {
                source_kind: "aggregate_api".to_string(),
                source_id: "acc-delete-a".to_string(),
                upstream_model: "gpt-test".to_string(),
                display_name: Some("Aggregate API GPT Test".to_string()),
                status: "available".to_string(),
                discovery_kind: "test".to_string(),
                last_synced_at: Some(now),
                extra_json: "{}".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert non-account model source model");

        let deleted = storage
            .delete_accounts(&[
                " acc-delete-a ".to_string(),
                "acc-delete-a".to_string(),
                "".to_string(),
                "acc-delete-b".to_string(),
                "acc-missing".to_string(),
            ])
            .expect("delete accounts");

        assert_eq!(deleted, 2);
        for deleted_account_id in ["acc-delete-a", "acc-delete-b"] {
            assert!(!storage
                .account_exists(deleted_account_id)
                .expect("deleted account exists check"));
            for (table, column) in [
                ("tokens", "account_id"),
                ("usage_snapshots", "account_id"),
                ("events", "account_id"),
                ("conversation_bindings", "account_id"),
            ] {
                assert_eq!(
                    dependent_row_count(&storage, table, column, deleted_account_id),
                    0,
                    "{table} should not keep rows for deleted account"
                );
            }
            for table in [
                "model_source_mappings",
                "model_source_models",
                "model_source_mapping_preferences",
            ] {
                assert_eq!(
                    model_source_account_row_count(&storage, table, deleted_account_id),
                    0,
                    "{table} should not keep account source rows for deleted account"
                );
            }
        }

        assert!(storage
            .account_exists("acc-keep")
            .expect("kept account exists check"));
        for (table, column) in [
            ("tokens", "account_id"),
            ("usage_snapshots", "account_id"),
            ("events", "account_id"),
            ("conversation_bindings", "account_id"),
        ] {
            assert_eq!(
                dependent_row_count(&storage, table, column, "acc-keep"),
                1,
                "{table} should keep rows for retained account"
            );
        }
        for table in [
            "model_source_mappings",
            "model_source_models",
            "model_source_mapping_preferences",
        ] {
            assert_eq!(
                model_source_account_row_count(&storage, table, "acc-keep"),
                1,
                "{table} should keep account source rows for retained account"
            );
        }
        assert_eq!(
            storage
                .conn
                .query_row(
                    "SELECT COUNT(1)
                     FROM model_source_models
                     WHERE source_kind = 'aggregate_api'
                       AND source_id = 'acc-delete-a'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("count non-account model source rows"),
            1
        );
    }

    #[test]
    fn set_preferred_account_keeps_only_one_account_selected() {
        let mut storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        storage
            .insert_account(&sample_account("acc-a", "active", now))
            .expect("insert account a");
        storage
            .insert_account(&sample_account("acc-b", "active", now))
            .expect("insert account b");

        storage
            .set_preferred_account(Some("acc-a"))
            .expect("set preferred a");
        assert_eq!(
            storage.preferred_account_id().expect("preferred a"),
            Some("acc-a".to_string())
        );

        storage
            .set_preferred_account(Some("acc-b"))
            .expect("set preferred b");
        assert_eq!(
            storage.preferred_account_id().expect("preferred b"),
            Some("acc-b".to_string())
        );

        assert!(
            storage
                .clear_preferred_account_if("acc-a")
                .expect("clear non-preferred")
                == false
        );
        assert!(storage
            .clear_preferred_account_if("acc-b")
            .expect("clear preferred"));
        assert_eq!(storage.preferred_account_id().expect("no preferred"), None);

        let plan = collect_query_plan(
            &storage,
            &format!("EXPLAIN QUERY PLAN {}", preferred_account_id_sql()),
        );
        assert!(
            plan.contains("idx_accounts_preferred_updated_at"),
            "expected preferred account lookup to use preferred lookup index, got {plan}"
        );
    }
    #[test]
    fn account_write_helpers_use_primary_key_indexes() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");

        fn assert_account_pk<P: rusqlite::Params>(
            storage: &Storage,
            label: &str,
            sql: &str,
            params: P,
        ) {
            let plan = collect_query_plan_with_params(
                storage,
                &format!("EXPLAIN QUERY PLAN {sql}"),
                params,
            );
            assert!(
                plan.contains("sqlite_autoindex_accounts_1") || plan.contains("USING INDEX"),
                "expected {label} to use account primary-key index, got {plan}"
            );
        }

        assert_account_pk(
            &storage,
            "account exists",
            account_exists_sql(),
            rusqlite::params!["acc-a"],
        );
        assert_account_pk(
            &storage,
            "sort update",
            update_account_sort_sql(),
            rusqlite::params![1_i64, 2_i64, "acc-a"],
        );
        assert_account_pk(
            &storage,
            "label update",
            update_account_label_sql(),
            rusqlite::params!["label", 2_i64, "acc-a"],
        );
        assert_account_pk(
            &storage,
            "workspace identity update",
            update_account_workspace_identity_sql(),
            rusqlite::params!["cgpt", "workspace", 2_i64, "acc-a"],
        );
        assert_account_pk(
            &storage,
            "touch updated_at",
            touch_account_updated_at_sql(),
            rusqlite::params![2_i64, "acc-a"],
        );
        assert_account_pk(
            &storage,
            "status update",
            update_account_status_sql(),
            rusqlite::params!["active", 2_i64, "acc-a"],
        );
        assert_account_pk(
            &storage,
            "status changed update",
            update_account_status_if_changed_sql(),
            rusqlite::params!["active", 2_i64, "acc-a"],
        );
        assert_account_pk(
            &storage,
            "account delete",
            delete_account_by_id_sql(),
            rusqlite::params!["acc-a"],
        );
        assert_account_pk(
            &storage,
            "set preferred account",
            set_preferred_account_sql(),
            rusqlite::params![2_i64, "acc-a"],
        );
        assert_account_pk(
            &storage,
            "clear preferred account by id",
            clear_preferred_account_by_id_sql(),
            rusqlite::params![2_i64, "acc-a"],
        );
    }
}
