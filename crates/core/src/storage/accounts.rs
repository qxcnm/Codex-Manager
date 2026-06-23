use rusqlite::{params_from_iter, types::Value, Result, Row};

use super::account_metadata::delete_account_metadata_for_account_sql;
use super::account_subscriptions::delete_account_subscription_for_account_sql;
use super::accounts_sql::*;
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
        let sql = account_quota_overview_stats_sql();
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
        let sql = account_count_filtered_sql(&where_clause);
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
        tx.execute(
            "DELETE FROM account_proxy_settings WHERE account_id = ?1",
            [account_id],
        )?;
        tx.execute(
            "DELETE FROM account_proxy_url_tests WHERE account_id = ?1",
            [account_id],
        )?;
        tx.execute(
            "DELETE FROM proxy_speed_tests WHERE account_id = ?1",
            [account_id],
        )?;
        tx.execute(
            "DELETE FROM proxy_diagnostics_history WHERE account_id = ?1",
            [account_id],
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

fn account_quota_overview_stats_sql() -> String {
    format!(
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
    )
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
#[path = "accounts_tests.rs"]
mod tests;
