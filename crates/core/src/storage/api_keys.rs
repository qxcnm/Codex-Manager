use rusqlite::{params_from_iter, Result, Row};

use super::key_id_filters::{key_id_in_clause, normalize_key_ids, SQLITE_IN_CLAUSE_BATCH_SIZE};
use super::{now_ts, ApiKey, Storage};

const API_KEY_SELECT_SQL: &str = "SELECT
    k.id,
    k.name,
    COALESCE(p.default_model, k.model_slug) AS model_slug,
    COALESCE(p.reasoning_effort, k.reasoning_effort) AS reasoning_effort,
    p.service_tier,
    COALESCE(k.rotation_strategy, 'account_rotation') AS rotation_strategy,
    k.aggregate_api_id,
    k.account_plan_filter,
    a.url AS aggregate_api_url,
    COALESCE(p.client_type, 'codex') AS client_type,
    COALESCE(p.protocol_type, 'openai_compat') AS protocol_type,
    COALESCE(p.auth_scheme, 'authorization_bearer') AS auth_scheme,
    p.upstream_base_url,
    p.static_headers_json,
    k.key_hash,
    k.status,
    k.created_at,
    k.last_used_at
 FROM api_keys k
 LEFT JOIN api_key_profiles p ON p.key_id = k.id
 LEFT JOIN aggregate_apis a ON a.id = k.aggregate_api_id";

impl Storage {
    /// 函数 `insert_api_key`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key: 参数 key
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_api_key(&self, key: &ApiKey) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO api_keys (id, name, model_slug, reasoning_effort, key_hash, status, created_at, last_used_at, rotation_strategy, aggregate_api_id, account_plan_filter) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            (
                &key.id,
                &key.name,
                &key.model_slug,
                &key.reasoning_effort,
                &key.key_hash,
                &key.status,
                key.created_at,
                &key.last_used_at,
                &key.rotation_strategy,
                &key.aggregate_api_id,
                &key.account_plan_filter,
            ),
        )?;
        self.conn.execute(
            "INSERT INTO api_key_profiles (key_id, client_type, protocol_type, auth_scheme, upstream_base_url, static_headers_json, default_model, reasoning_effort, service_tier, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(key_id) DO UPDATE SET
               client_type = excluded.client_type,
               protocol_type = excluded.protocol_type,
               auth_scheme = excluded.auth_scheme,
               upstream_base_url = excluded.upstream_base_url,
               static_headers_json = excluded.static_headers_json,
               default_model = excluded.default_model,
               reasoning_effort = excluded.reasoning_effort,
               service_tier = excluded.service_tier,
               updated_at = excluded.updated_at",
            (
                &key.id,
                &key.client_type,
                &key.protocol_type,
                &key.auth_scheme,
                &key.upstream_base_url,
                &key.static_headers_json,
                &key.model_slug,
                &key.reasoning_effort,
                &key.service_tier,
                key.created_at,
                now_ts(),
            ),
        )?;
        Ok(())
    }

    /// 函数 `list_api_keys`
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
    pub fn list_api_keys(&self) -> Result<Vec<ApiKey>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{API_KEY_SELECT_SQL} ORDER BY k.created_at DESC"))?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_api_key_row(row)?);
        }
        Ok(out)
    }

    /// 函数 `list_api_keys_for_ids`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-05-28
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_ids: 参数 key_ids
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_api_keys_for_ids(&self, key_ids: &[String]) -> Result<Vec<ApiKey>> {
        let key_ids = normalize_key_ids(key_ids);
        if key_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in key_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_api_keys_for_ids_chunk(self, chunk)?);
        }
        out.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(out)
    }

    /// 函数 `find_api_key_by_hash`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_hash: 参数 key_hash
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let mut stmt = self.conn.prepare(&format!(
            "{API_KEY_SELECT_SQL}
             WHERE k.key_hash = ?1
             LIMIT 1"
        ))?;
        let mut rows = stmt.query([key_hash])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_api_key_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `find_api_key_by_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn find_api_key_by_id(&self, key_id: &str) -> Result<Option<ApiKey>> {
        let mut stmt = self.conn.prepare(&format!(
            "{API_KEY_SELECT_SQL}
             WHERE k.id = ?1
             LIMIT 1"
        ))?;
        let mut rows = stmt.query([key_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_api_key_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `update_api_key_last_used`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_hash: 参数 key_hash
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_api_key_last_used(&self, key_hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET last_used_at = ?1 WHERE key_hash = ?2",
            (now_ts(), key_hash),
        )?;
        Ok(())
    }

    /// 函数 `update_api_key_status`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    /// - status: 参数 status
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_api_key_status(&self, key_id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET status = ?1 WHERE id = ?2",
            (status, key_id),
        )?;
        Ok(())
    }

    /// 函数 `update_api_key_rotation_config`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    /// - rotation_strategy: 参数 rotation_strategy
    /// - aggregate_api_id: 参数 aggregate_api_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_api_key_rotation_config(
        &self,
        key_id: &str,
        rotation_strategy: &str,
        aggregate_api_id: Option<&str>,
        account_plan_filter: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET rotation_strategy = ?1, aggregate_api_id = ?2, account_plan_filter = ?3 WHERE id = ?4",
            (rotation_strategy, aggregate_api_id, account_plan_filter, key_id),
        )?;
        Ok(())
    }

    /// 函数 `update_api_key_name`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    /// - name: 参数 name
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_api_key_name(&self, key_id: &str, name: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET name = ?1 WHERE id = ?2",
            (name, key_id),
        )?;
        Ok(())
    }

    /// 函数 `update_api_key_model_slug`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    /// - model_slug: 参数 model_slug
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_api_key_model_slug(&self, key_id: &str, model_slug: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET model_slug = ?1 WHERE id = ?2",
            (model_slug, key_id),
        )?;
        Ok(())
    }

    /// 函数 `update_api_key_model_config`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    /// - model_slug: 参数 model_slug
    /// - reasoning_effort: 参数 reasoning_effort
    /// - service_tier: 参数 service_tier
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_api_key_model_config(
        &self,
        key_id: &str,
        model_slug: Option<&str>,
        reasoning_effort: Option<&str>,
        service_tier: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET model_slug = ?1, reasoning_effort = ?2 WHERE id = ?3",
            (model_slug, reasoning_effort, key_id),
        )?;
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO api_key_profiles (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                default_model,
                reasoning_effort,
                service_tier,
                created_at,
                updated_at
            )
            SELECT
                id,
                'codex',
                'openai_compat',
                'authorization_bearer',
                NULL,
                NULL,
                ?2,
                ?3,
                ?4,
                ?5,
                ?5
            FROM api_keys
            WHERE id = ?1
            ON CONFLICT(key_id) DO UPDATE SET
                default_model = excluded.default_model,
                reasoning_effort = excluded.reasoning_effort,
                service_tier = excluded.service_tier,
                updated_at = excluded.updated_at",
            (key_id, model_slug, reasoning_effort, service_tier, now),
        )?;
        Ok(())
    }

    /// 函数 `update_api_key_profile_config`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    /// - client_type: 参数 client_type
    /// - protocol_type: 参数 protocol_type
    /// - auth_scheme: 参数 auth_scheme
    /// - upstream_base_url: 参数 upstream_base_url
    /// - static_headers_json: 参数 static_headers_json
    /// - service_tier: 参数 service_tier
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_api_key_profile_config(
        &self,
        key_id: &str,
        client_type: &str,
        protocol_type: &str,
        auth_scheme: &str,
        upstream_base_url: Option<&str>,
        static_headers_json: Option<&str>,
        service_tier: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO api_key_profiles (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                default_model,
                reasoning_effort,
                service_tier,
                created_at,
                updated_at
            )
            SELECT
                id,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                model_slug,
                reasoning_effort,
                ?7,
                created_at,
                ?8
            FROM api_keys
            WHERE id = ?1
            ON CONFLICT(key_id) DO UPDATE SET
                client_type = excluded.client_type,
                protocol_type = excluded.protocol_type,
                auth_scheme = excluded.auth_scheme,
                upstream_base_url = excluded.upstream_base_url,
                static_headers_json = excluded.static_headers_json,
                service_tier = excluded.service_tier,
                updated_at = excluded.updated_at",
            (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                service_tier,
                now_ts(),
            ),
        )?;
        Ok(())
    }

    /// 函数 `delete_api_key`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn delete_api_key(&self, key_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM api_key_quota_limits WHERE key_id = ?1",
            [key_id],
        )?;
        self.conn
            .execute("DELETE FROM api_key_secrets WHERE key_id = ?1", [key_id])?;
        self.conn
            .execute("DELETE FROM api_keys WHERE id = ?1", [key_id])?;
        Ok(())
    }

    /// 函数 `upsert_api_key_secret`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    /// - key_value: 参数 key_value
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn upsert_api_key_secret(&self, key_id: &str, key_value: &str) -> Result<()> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO api_key_secrets (key_id, key_value, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(key_id) DO UPDATE SET
               key_value = excluded.key_value,
               updated_at = excluded.updated_at",
            (key_id, key_value, now),
        )?;
        Ok(())
    }

    /// 函数 `find_api_key_secret_by_id`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - key_id: 参数 key_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn find_api_key_secret_by_id(&self, key_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key_value FROM api_key_secrets WHERE key_id = ?1 LIMIT 1")?;
        let mut rows = stmt.query([key_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `ensure_api_key_model_column`
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
    pub(super) fn ensure_api_key_model_column(&self) -> Result<()> {
        self.ensure_column("api_keys", "model_slug", "TEXT")?;
        Ok(())
    }

    /// 函数 `ensure_api_key_reasoning_column`
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
    pub(super) fn ensure_api_key_reasoning_column(&self) -> Result<()> {
        self.ensure_column("api_keys", "reasoning_effort", "TEXT")?;
        Ok(())
    }

    /// 函数 `ensure_api_key_rotation_columns`
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
    pub(super) fn ensure_api_key_rotation_columns(&self) -> Result<()> {
        self.ensure_column("api_keys", "rotation_strategy", "TEXT")?;
        self.ensure_column("api_keys", "aggregate_api_id", "TEXT")?;
        self.ensure_column("api_keys", "account_plan_filter", "TEXT")?;
        self.conn.execute(
            "UPDATE api_keys
             SET rotation_strategy = COALESCE(NULLIF(TRIM(rotation_strategy), ''), 'account_rotation')
             WHERE rotation_strategy IS NULL OR TRIM(rotation_strategy) = ''",
            [],
        )?;
        self.conn.execute(
            "UPDATE api_keys
             SET account_plan_filter = NULL
             WHERE account_plan_filter IS NOT NULL AND TRIM(account_plan_filter) = ''",
            [],
        )?;
        Ok(())
    }

    /// 函数 `ensure_api_key_profiles_table`
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
    pub(super) fn ensure_api_key_profiles_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS api_key_profiles (
                key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
                client_type TEXT NOT NULL CHECK (client_type IN ('codex', 'claude_code')),
                protocol_type TEXT NOT NULL CHECK (protocol_type IN ('openai_compat', 'anthropic_native')),
                auth_scheme TEXT NOT NULL CHECK (auth_scheme IN ('authorization_bearer', 'x_api_key')),
                upstream_base_url TEXT,
                static_headers_json TEXT,
                default_model TEXT,
                reasoning_effort TEXT,
                service_tier TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_key_profiles_client_protocol ON api_key_profiles(client_type, protocol_type)",
            [],
        )?;
        self.backfill_api_key_profiles()
    }

    /// 函数 `ensure_api_key_service_tier_column`
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
    pub(super) fn ensure_api_key_service_tier_column(&self) -> Result<()> {
        self.ensure_column("api_key_profiles", "service_tier", "TEXT")?;
        Ok(())
    }

    /// 函数 `ensure_api_key_secrets_table`
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
    pub(super) fn ensure_api_key_secrets_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS api_key_secrets (
                key_id TEXT PRIMARY KEY,
                key_value TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_key_secrets_updated_at ON api_key_secrets(updated_at)",
            [],
        )?;
        Ok(())
    }

    /// 函数 `backfill_api_key_profiles`
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
    fn backfill_api_key_profiles(&self) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO api_key_profiles (
                key_id,
                client_type,
                protocol_type,
                auth_scheme,
                upstream_base_url,
                static_headers_json,
                default_model,
                reasoning_effort,
                service_tier,
                created_at,
                updated_at
            )
            SELECT
                id,
                'codex',
                'openai_compat',
                'authorization_bearer',
                NULL,
                NULL,
                model_slug,
                reasoning_effort,
                NULL,
                created_at,
                created_at
            FROM api_keys",
            [],
        )?;
        Ok(())
    }
}

fn list_api_keys_for_ids_chunk(storage: &Storage, key_ids: &[String]) -> Result<Vec<ApiKey>> {
    let Some((clause, params)) = key_id_in_clause("k.id", key_ids) else {
        return Ok(Vec::new());
    };
    let sql = format!(
        "{API_KEY_SELECT_SQL}
         WHERE {clause}
         ORDER BY k.created_at DESC, k.id ASC"
    );
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params.iter()))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_api_key_row(row)?);
    }
    Ok(out)
}

/// 函数 `map_api_key_row`
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
fn map_api_key_row(row: &Row<'_>) -> Result<ApiKey> {
    Ok(ApiKey {
        id: row.get(0)?,
        name: row.get(1)?,
        model_slug: row.get(2)?,
        reasoning_effort: row.get(3)?,
        service_tier: row.get(4)?,
        rotation_strategy: row.get(5)?,
        aggregate_api_id: row.get(6)?,
        account_plan_filter: row.get(7)?,
        aggregate_api_url: row.get(8)?,
        client_type: row.get(9)?,
        protocol_type: row.get(10)?,
        auth_scheme: row.get(11)?,
        upstream_base_url: row.get(12)?,
        static_headers_json: row.get(13)?,
        key_hash: row.get(14)?,
        status: row.get(15)?,
        created_at: row.get(16)?,
        last_used_at: row.get(17)?,
    })
}

#[cfg(test)]
#[path = "tests/api_keys_tests.rs"]
mod tests;
