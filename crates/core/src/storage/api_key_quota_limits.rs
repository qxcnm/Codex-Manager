use std::collections::HashMap;

use rusqlite::{params_from_iter, OptionalExtension, Result};

use super::key_id_filters::{key_id_in_clause, normalize_key_ids, SQLITE_IN_CLAUSE_BATCH_SIZE};
use super::{now_ts, Storage};

impl Storage {
    pub fn upsert_api_key_quota_limit(
        &self,
        key_id: &str,
        quota_limit_tokens: Option<i64>,
    ) -> Result<()> {
        let normalized = quota_limit_tokens.filter(|value| *value > 0);
        let Some(limit) = normalized else {
            self.conn.execute(
                "DELETE FROM api_key_quota_limits WHERE key_id = ?1",
                [key_id],
            )?;
            return Ok(());
        };

        let now = now_ts();
        self.conn.execute(
            "INSERT INTO api_key_quota_limits (
                key_id, quota_limit_tokens, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(key_id) DO UPDATE SET
                quota_limit_tokens = excluded.quota_limit_tokens,
                updated_at = excluded.updated_at",
            (key_id, limit, now),
        )?;
        Ok(())
    }

    pub fn find_api_key_quota_limit(&self, key_id: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT quota_limit_tokens
                 FROM api_key_quota_limits
                 WHERE key_id = ?1
                 LIMIT 1",
                [key_id],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn list_api_key_quota_limits(&self) -> Result<HashMap<String, i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT key_id, quota_limit_tokens
             FROM api_key_quota_limits
             WHERE quota_limit_tokens > 0",
        )?;
        let mut rows = stmt.query([])?;
        let mut out = HashMap::new();
        while let Some(row) = rows.next()? {
            out.insert(row.get(0)?, row.get(1)?);
        }
        Ok(out)
    }

    pub fn list_api_key_quota_limits_for_ids(
        &self,
        key_ids: &[String],
    ) -> Result<HashMap<String, i64>> {
        let key_ids = normalize_key_ids(key_ids);
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut out = HashMap::new();
        for chunk in key_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(list_api_key_quota_limits_for_ids_chunk(self, chunk)?);
        }
        Ok(out)
    }

    pub fn api_key_total_token_usage(&self, key_id: &str) -> Result<i64> {
        let mut stmt = self.conn.prepare(
            "WITH all_stats AS (
                SELECT
                    key_id,
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                    total_tokens
                FROM request_token_stats
                UNION ALL
                SELECT
                    NULLIF(key_id, '') AS key_id,
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                    total_tokens
                FROM request_token_stat_rollups
             )
             SELECT
                IFNULL(
                    SUM(
                        CASE
                            WHEN total_tokens IS NOT NULL THEN
                                CASE WHEN total_tokens > 0 THEN total_tokens ELSE 0 END
                            ELSE
                                CASE
                                    WHEN IFNULL(input_tokens, 0) - IFNULL(cached_input_tokens, 0) + IFNULL(output_tokens, 0) > 0
                                        THEN IFNULL(input_tokens, 0) - IFNULL(cached_input_tokens, 0) + IFNULL(output_tokens, 0)
                                    ELSE 0
                                END
                        END
                    ),
                    0
                ) AS total_tokens
             FROM all_stats
             WHERE key_id = ?1",
        )?;
        let mut rows = stmt.query([key_id])?;
        if let Some(row) = rows.next()? {
            let total: i64 = row.get(0)?;
            return Ok(total.max(0));
        }
        Ok(0)
    }

    pub(super) fn ensure_api_key_quota_limits_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS api_key_quota_limits (
                key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
                quota_limit_tokens INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_api_key_quota_limits_updated_at
             ON api_key_quota_limits(updated_at DESC)",
            [],
        )?;
        Ok(())
    }
}

fn list_api_key_quota_limits_for_ids_chunk(
    storage: &Storage,
    key_ids: &[String],
) -> Result<HashMap<String, i64>> {
    let Some((clause, params)) = key_id_in_clause("key_id", key_ids) else {
        return Ok(HashMap::new());
    };
    let sql = format!(
        "SELECT key_id, quota_limit_tokens
         FROM api_key_quota_limits
         WHERE quota_limit_tokens > 0
           AND {clause}"
    );
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params.iter()))?;
    let mut out = HashMap::new();
    while let Some(row) = rows.next()? {
        out.insert(row.get(0)?, row.get(1)?);
    }
    Ok(out)
}
