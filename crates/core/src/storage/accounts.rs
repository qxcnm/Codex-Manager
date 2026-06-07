use rusqlite::{params_from_iter, types::Value, Result, Row};

use super::{now_ts, Account, Storage, Token};

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
                0
            )
             ON CONFLICT(id) DO UPDATE SET
                label = excluded.label,
                issuer = excluded.issuer,
                chatgpt_account_id = excluded.chatgpt_account_id,
                workspace_id = excluded.workspace_id,
                sort = excluded.sort,
                status = excluded.status,
                updated_at = excluded.updated_at",
            (
                &account.id,
                &account.label,
                &account.issuer,
                &account.chatgpt_account_id,
                &account.workspace_id,
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
            .query_row("SELECT COUNT(1) FROM accounts", [], |row| row.get(0))
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
        let availability_clause = gateway_account_usage_filter_clause("a", "lu");
        let sql = format!(
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
             WHERE {availability_clause}
             ORDER BY a.sort ASC, a.updated_at DESC",
            latest_usage_cte = latest_usage_cte_sql(),
            account_select = account_select_columns("a"),
            token_select = token_select_columns("t"),
            availability_clause = availability_clause,
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_gateway_candidate_row(row)?);
        }
        Ok(out)
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
        let mut stmt = self.conn.prepare(
            "SELECT id, label, issuer, chatgpt_account_id, workspace_id, sort, status, created_at, updated_at
             FROM accounts
             WHERE id = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_account_row(row)?))
        } else {
            Ok(None)
        }
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
        self.conn.execute(
            "UPDATE accounts SET sort = ?1, updated_at = ?2 WHERE id = ?3",
            (sort, now_ts(), account_id),
        )?;
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
        self.conn.execute(
            "UPDATE accounts SET label = ?1, updated_at = ?2 WHERE id = ?3",
            (label, now_ts(), account_id),
        )?;
        Ok(())
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
        self.conn.execute(
            "UPDATE accounts SET updated_at = ?1 WHERE id = ?2",
            (now_ts(), account_id),
        )?;
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
        self.conn.execute(
            "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3",
            (status, now_ts(), account_id),
        )?;
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
            "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3 AND status != ?1",
            (status, now_ts(), account_id),
        )?;
        Ok(updated > 0)
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
        tx.execute(
            "DELETE FROM account_metadata WHERE account_id = ?1",
            [account_id],
        )?;
        tx.execute(
            "DELETE FROM account_subscriptions WHERE account_id = ?1",
            [account_id],
        )?;
        tx.execute("DELETE FROM tokens WHERE account_id = ?1", [account_id])?;
        tx.execute(
            "DELETE FROM usage_snapshots WHERE account_id = ?1",
            [account_id],
        )?;
        tx.execute("DELETE FROM events WHERE account_id = ?1", [account_id])?;
        tx.execute(
            "DELETE FROM conversation_bindings WHERE account_id = ?1",
            [account_id],
        )?;
        tx.execute(
            "DELETE FROM model_source_mappings
             WHERE source_kind = 'openai_account' AND source_id = ?1",
            [account_id],
        )?;
        tx.execute(
            "DELETE FROM model_source_models
             WHERE source_kind = 'openai_account' AND source_id = ?1",
            [account_id],
        )?;
        tx.execute(
            "DELETE FROM model_source_mapping_preferences
             WHERE source_kind = 'openai_account' AND source_id = ?1",
            [account_id],
        )?;
        tx.execute("DELETE FROM accounts WHERE id = ?1", [account_id])?;
        tx.commit()?;
        Ok(())
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
        let mut stmt = self.conn.prepare(
            "SELECT id
             FROM accounts
             WHERE preferred = 1
             ORDER BY updated_at DESC, id ASC
             LIMIT 1",
        )?;
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
        tx.execute("UPDATE accounts SET preferred = 0 WHERE preferred != 0", [])?;
        if let Some(account_id) = account_id {
            let normalized_account_id = account_id.trim();
            if !normalized_account_id.is_empty() {
                tx.execute(
                    "UPDATE accounts
                     SET preferred = 1, updated_at = ?1
                     WHERE id = ?2",
                    (now, normalized_account_id),
                )?;
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
            "UPDATE accounts SET preferred = 0, updated_at = ?1 WHERE id = ?2 AND preferred = 1",
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
        let mut sql = format!(
            "SELECT {} FROM accounts a{where_clause} ORDER BY a.sort ASC, a.updated_at DESC",
            account_select_columns("a"),
        );

        if let Some((offset, limit)) = pagination {
            sql.push_str(" LIMIT ? OFFSET ?");
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
    let _ = group_name;

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
            secondary_used_percent,
            secondary_window_minutes,
            ROW_NUMBER() OVER (
                PARTITION BY account_id
                ORDER BY captured_at DESC, id DESC
            ) AS rn
        FROM usage_snapshots
    )"
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
        group_name: None,
        sort: row.get(offset + 5)?,
        status: row.get(offset + 6)?,
        created_at: row.get(offset + 7)?,
        updated_at: row.get(offset + 8)?,
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
    let token = map_token_row_from_offset(row, 9)?;
    Ok((account, token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::UsageSnapshotRecord;

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

    #[test]
    fn insert_account_update_preserves_existing_token() {
        let mut storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();

        let mut account = sample_account("acc-upsert", "active", now);
        account.chatgpt_account_id = Some("cgpt-old".to_string());
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
        assert_eq!(found.created_at, now);
        assert_eq!(found.updated_at, now.saturating_add(1));
        assert_eq!(
            storage.preferred_account_id().expect("preferred account"),
            Some(account.id.clone())
        );

        let token = storage
            .find_token_by_account_id(account.id.as_str())
            .expect("find token")
            .expect("token still exists");
        assert_eq!(token.access_token, "access");
        assert_eq!(token.refresh_token, "refresh");
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
    }
}
