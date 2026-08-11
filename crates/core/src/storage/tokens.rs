use rusqlite::{params_from_iter, Result, Row};

use super::key_id_filters::{normalize_text_ids, text_id_in_clause, SQLITE_IN_CLAUSE_BATCH_SIZE};
use super::{AccountImportTokenSubject, AccountTokenCandidate, AccountTokenPlan, Storage, Token};

pub(super) fn delete_token_for_account_sql() -> &'static str {
    "DELETE FROM tokens WHERE account_id = ?1"
}

impl Storage {
    /// 函数 `insert_token`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - token: 参数 token
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_token(&self, token: &Token) -> Result<()> {
        let encrypted = self.encrypt_token(token)?;
        self.conn.execute(
            "INSERT INTO tokens (account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(account_id) DO UPDATE SET
                id_token = excluded.id_token,
                access_token = excluded.access_token,
                refresh_token = excluded.refresh_token,
                api_key_access_token = excluded.api_key_access_token,
                last_refresh = excluded.last_refresh",
            (
                &encrypted.account_id,
                &encrypted.id_token,
                &encrypted.access_token,
                &encrypted.refresh_token,
                &encrypted.api_key_access_token,
                encrypted.last_refresh,
            ),
        )?;
        Ok(())
    }

    /// 函数 `list_tokens_due_for_refresh`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - refresh_due_cutoff_ts: 参数 refresh_due_cutoff_ts
    /// - access_exp_cutoff_ts: 参数 access_exp_cutoff_ts
    /// - limit: 参数 limit
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_tokens_due_for_refresh(
        &self,
        refresh_due_cutoff_ts: i64,
        access_exp_cutoff_ts: i64,
        limit: usize,
    ) -> Result<Vec<Token>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let sql = tokens_due_for_refresh_sql();
        let mut stmt = self.conn.prepare(sql)?;
        let mut rows = stmt.query((refresh_due_cutoff_ts, access_exp_cutoff_ts, limit as i64))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(self.map_token_row(row)?);
        }
        Ok(out)
    }

    /// 函数 `update_token_refresh_schedule`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    /// - access_token_exp: 参数 access_token_exp
    /// - next_refresh_at: 参数 next_refresh_at
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_token_refresh_schedule(
        &self,
        account_id: &str,
        access_token_exp: Option<i64>,
        next_refresh_at: Option<i64>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE tokens
             SET access_token_exp = ?1,
                 next_refresh_at = ?2
             WHERE account_id = ?3",
            (access_token_exp, next_refresh_at, account_id),
        )?;
        Ok(())
    }

    /// 函数 `touch_token_refresh_attempt`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    /// - attempt_ts: 参数 attempt_ts
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn touch_token_refresh_attempt(&self, account_id: &str, attempt_ts: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE tokens
             SET last_refresh_attempt_at = ?1
             WHERE account_id = ?2",
            (attempt_ts, account_id),
        )?;
        Ok(())
    }

    /// 函数 `token_count`
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
    pub fn token_count(&self) -> Result<i64> {
        self.conn.query_row(token_count_sql(), [], |row| row.get(0))
    }

    pub fn token_account_count(&self) -> Result<i64> {
        self.conn
            .query_row(token_account_count_sql(), [], |row| row.get(0))
    }

    /// 函数 `list_tokens`
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
    pub fn list_tokens(&self) -> Result<Vec<Token>> {
        let mut stmt = self.conn.prepare(token_list_sql())?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(self.map_token_row(row)?);
        }
        Ok(out)
    }

    pub fn list_account_token_candidates(&self) -> Result<Vec<AccountTokenCandidate>> {
        let mut stmt = self.conn.prepare(account_token_candidates_sql())?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_account_token_candidate_row(row)?);
        }
        Ok(out)
    }

    pub fn list_usable_account_token_candidates(&self) -> Result<Vec<AccountTokenCandidate>> {
        let mut stmt = self.conn.prepare(usable_account_token_candidates_sql())?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_account_token_candidate_row(row)?);
        }
        Ok(out)
    }

    pub fn list_account_token_candidates_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<AccountTokenCandidate>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_token_candidates_for_accounts_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        Ok(out)
    }

    pub fn list_usable_account_token_candidates_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<AccountTokenCandidate>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_usable_account_token_candidates_for_accounts_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        Ok(out)
    }

    pub fn list_account_import_token_subjects(&self) -> Result<Vec<AccountImportTokenSubject>> {
        let mut stmt = self.conn.prepare(account_import_token_subjects_sql())?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let account_id: String = row.get(0)?;
            out.push(AccountImportTokenSubject {
                id_token: self.decrypt_token_field(&account_id, "id", &row.get::<_, String>(1)?)?,
                access_token: self.decrypt_token_field(
                    &account_id,
                    "access",
                    &row.get::<_, String>(2)?,
                )?,
                refresh_token: self.decrypt_token_field(
                    &account_id,
                    "refresh",
                    &row.get::<_, String>(3)?,
                )?,
                account_id,
            });
        }
        Ok(out)
    }

    pub fn list_tokens_for_accounts(&self, account_ids: &[String]) -> Result<Vec<Token>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_tokens_for_accounts_chunk(self, chunk)?);
        }
        out.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        Ok(out)
    }

    pub fn list_account_token_plans_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<AccountTokenPlan>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_account_token_plans_for_accounts_chunk(self, chunk)?);
        }
        out.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        Ok(out)
    }

    /// 函数 `find_token_by_account_id`
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
    pub fn find_token_by_account_id(&self, account_id: &str) -> Result<Option<Token>> {
        let mut stmt = self.conn.prepare(token_by_account_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(self.map_token_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `ensure_token_api_key_column`
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
    pub(super) fn ensure_token_api_key_column(&self) -> Result<()> {
        if self.has_column("tokens", "api_key_access_token")? {
            return Ok(());
        }
        self.conn.execute(
            "ALTER TABLE tokens ADD COLUMN api_key_access_token TEXT",
            [],
        )?;
        Ok(())
    }

    /// 函数 `ensure_token_refresh_schedule_columns`
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
    pub(super) fn ensure_token_refresh_schedule_columns(&self) -> Result<()> {
        self.ensure_column("tokens", "access_token_exp", "INTEGER")?;
        self.ensure_column("tokens", "next_refresh_at", "INTEGER")?;
        self.ensure_column("tokens", "last_refresh_attempt_at", "INTEGER")?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tokens_next_refresh_at ON tokens(next_refresh_at)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_tokens_refresh_due_order
             ON tokens(COALESCE(next_refresh_at, 0) ASC, account_id ASC)",
            [],
        )?;
        Ok(())
    }

    pub(super) fn encrypt_token(&self, token: &Token) -> Result<Token> {
        Ok(Token {
            account_id: token.account_id.clone(),
            id_token: self.encrypt_token_field(&token.account_id, "id", &token.id_token)?,
            access_token: self.encrypt_token_field(
                &token.account_id,
                "access",
                &token.access_token,
            )?,
            refresh_token: self.encrypt_token_field(
                &token.account_id,
                "refresh",
                &token.refresh_token,
            )?,
            api_key_access_token: token
                .api_key_access_token
                .as_deref()
                .map(|value| self.encrypt_token_field(&token.account_id, "api-key-access", value))
                .transpose()?,
            last_refresh: token.last_refresh,
        })
    }

    fn encrypt_token_field(&self, account_id: &str, kind: &str, value: &str) -> Result<String> {
        if value.trim().is_empty() {
            // Preserve blank/whitespace-only sentinels so existing SQL health
            // predicates can continue to identify unusable credentials.
            return Ok(value.to_string());
        }
        self.encrypt_vault_text(&format!("codex-token:{account_id}:{kind}"), value)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
    }

    pub(super) fn decrypt_token_field(
        &self,
        account_id: &str,
        kind: &str,
        value: &str,
    ) -> Result<String> {
        self.decrypt_vault_text(&format!("codex-token:{account_id}:{kind}"), value)
            .map_err(|error| rusqlite::Error::FromSql(error.to_string()))
    }

    fn map_token_row(&self, row: &Row<'_>) -> Result<Token> {
        let account_id: String = row.get(0)?;
        Ok(Token {
            id_token: self.decrypt_token_field(&account_id, "id", &row.get::<_, String>(1)?)?,
            access_token: self.decrypt_token_field(
                &account_id,
                "access",
                &row.get::<_, String>(2)?,
            )?,
            refresh_token: self.decrypt_token_field(
                &account_id,
                "refresh",
                &row.get::<_, String>(3)?,
            )?,
            api_key_access_token: row
                .get::<_, Option<String>>(4)?
                .map(|value| self.decrypt_token_field(&account_id, "api-key-access", &value))
                .transpose()?,
            account_id,
            last_refresh: row.get(5)?,
        })
    }

    #[cfg(windows)]
    pub(super) fn encrypt_plaintext_codex_tokens(&self) -> Result<()> {
        let tokens = {
            let mut stmt = self.conn.prepare(token_list_sql())?;
            let mut rows = stmt.query([])?;
            let mut tokens = Vec::new();
            while let Some(row) = rows.next()? {
                let raw_fields = [
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ];
                let raw_api_key = row.get::<_, Option<String>>(4)?;
                let needs_upgrade = raw_fields
                    .iter()
                    .chain(raw_api_key.iter())
                    .any(|value| !value.is_empty() && !value.starts_with("vault:v1:"));
                if needs_upgrade {
                    tokens.push(self.map_token_row(row)?);
                }
            }
            tokens
        };
        for token in tokens {
            self.insert_token(&token)?;
        }
        Ok(())
    }

    #[cfg(not(windows))]
    pub(super) fn encrypt_plaintext_codex_tokens(&self) -> Result<()> {
        Ok(())
    }
}

fn token_count_sql() -> &'static str {
    "SELECT COUNT(1) FROM tokens"
}

fn token_account_count_sql() -> &'static str {
    "SELECT COUNT(DISTINCT account_id) FROM tokens"
}

fn token_list_sql() -> &'static str {
    "SELECT account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh
     FROM tokens"
}

fn account_token_candidates_sql() -> &'static str {
    "SELECT
        account_id,
        TRIM(COALESCE(access_token, '')) <> '',
        TRIM(COALESCE(refresh_token, '')) <> '',
        last_refresh
     FROM tokens
     ORDER BY account_id ASC"
}

fn usable_account_token_candidates_sql() -> &'static str {
    "SELECT
        account_id,
        1,
        1,
        last_refresh
     FROM tokens
     WHERE TRIM(COALESCE(access_token, '')) <> ''
       AND TRIM(COALESCE(refresh_token, '')) <> ''
     ORDER BY account_id ASC"
}

fn account_import_token_subjects_sql() -> &'static str {
    "SELECT account_id, id_token, access_token, refresh_token
     FROM tokens
     ORDER BY account_id ASC"
}

fn token_by_account_sql() -> &'static str {
    "SELECT account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh
     FROM tokens
     WHERE account_id = ?1
     LIMIT 1"
}

fn tokens_due_for_refresh_sql() -> &'static str {
    "WITH latest_status AS (
        SELECT
            e.account_id,
            e.message,
            ROW_NUMBER() OVER (
                PARTITION BY e.account_id
                ORDER BY e.created_at DESC, e.id DESC
            ) AS rn
        FROM tokens target_tokens
        INNER JOIN events e
          ON e.account_id = target_tokens.account_id
        WHERE e.type = 'account_status_update'
          AND TRIM(COALESCE(target_tokens.refresh_token, '')) <> ''
          AND (
                target_tokens.next_refresh_at IS NULL
                OR target_tokens.next_refresh_at <= ?1
                OR (
                    target_tokens.access_token_exp IS NOT NULL
                    AND target_tokens.access_token_exp <= ?2
                )
          )
     )
     SELECT tokens.account_id, tokens.id_token, tokens.access_token, tokens.refresh_token, tokens.api_key_access_token, tokens.last_refresh
     FROM tokens
     LEFT JOIN latest_status
       ON latest_status.account_id = tokens.account_id
      AND latest_status.rn = 1
     WHERE TRIM(COALESCE(refresh_token, '')) <> ''
       AND (
            latest_status.message IS NULL
            OR (
                latest_status.message NOT LIKE '% reason=account_deactivated'
                AND latest_status.message NOT LIKE '% reason=workspace_deactivated'
            )
       )
       AND (
            next_refresh_at IS NULL
            OR next_refresh_at <= ?1
            OR (
                access_token_exp IS NOT NULL
                AND access_token_exp <= ?2
            )
       )
     ORDER BY COALESCE(tokens.next_refresh_at, 0) ASC, tokens.account_id ASC
     LIMIT ?3"
}

/// 函数 `map_token_row`
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
fn map_account_token_candidate_row(row: &Row<'_>) -> Result<AccountTokenCandidate> {
    Ok(AccountTokenCandidate {
        account_id: row.get(0)?,
        has_access_token: row.get(1)?,
        has_refresh_token: row.get(2)?,
        last_refresh: row.get(3)?,
    })
}

fn list_tokens_for_accounts_chunk(storage: &Storage, account_ids: &[String]) -> Result<Vec<Token>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = tokens_for_accounts_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(storage.map_token_row(row)?);
    }
    Ok(out)
}

fn tokens_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "SELECT account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh
         FROM tokens
         WHERE {account_condition}"
    )
}

fn list_account_token_candidates_for_accounts_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<AccountTokenCandidate>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = account_token_candidates_for_accounts_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_account_token_candidate_row(row)?);
    }
    Ok(out)
}

fn account_token_candidates_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "SELECT
            account_id,
            TRIM(COALESCE(access_token, '')) <> '',
            TRIM(COALESCE(refresh_token, '')) <> '',
            last_refresh
         FROM tokens
         WHERE {account_condition}"
    )
}

fn list_usable_account_token_candidates_for_accounts_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<AccountTokenCandidate>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = usable_account_token_candidates_for_accounts_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_account_token_candidate_row(row)?);
    }
    Ok(out)
}

fn usable_account_token_candidates_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "SELECT
            account_id,
            1,
            1,
            last_refresh
         FROM tokens
         WHERE {account_condition}
           AND TRIM(COALESCE(access_token, '')) <> ''
           AND TRIM(COALESCE(refresh_token, '')) <> ''"
    )
}

fn list_account_token_plans_for_accounts_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<AccountTokenPlan>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = account_token_plans_for_accounts_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let account_id: String = row.get(0)?;
        out.push(AccountTokenPlan {
            id_token: storage.decrypt_token_field(&account_id, "id", &row.get::<_, String>(1)?)?,
            access_token: storage.decrypt_token_field(
                &account_id,
                "access",
                &row.get::<_, String>(2)?,
            )?,
            account_id,
        });
    }
    Ok(out)
}

fn account_token_plans_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "SELECT account_id, id_token, access_token
         FROM tokens
         WHERE {account_condition}"
    )
}

#[cfg(test)]
#[path = "tokens_tests.rs"]
mod tests;
