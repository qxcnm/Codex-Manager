use rusqlite::{params_from_iter, OptionalExtension, Result, Row};

use super::key_id_filters::{normalize_text_ids, text_id_in_clause, SQLITE_IN_CLAUSE_BATCH_SIZE};
use super::{
    Storage, UsageSnapshotCleanupRow, UsageSnapshotQuotaSourceRow, UsageSnapshotRecord,
    UsageSnapshotSummaryRow,
};

const DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT: usize = 1;
const LONG_USAGE_WINDOW_MINUTES: i64 = 24 * 60 + 3;
const USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV: &str =
    "CODEXMANAGER_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT";

pub(super) fn usage_snapshots_retain_per_account() -> usize {
    std::env::var(USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_USAGE_SNAPSHOTS_RETAIN_PER_ACCOUNT)
}

fn usage_snapshot_count_for_account_sql() -> &'static str {
    "SELECT COUNT(1) FROM usage_snapshots WHERE account_id = ?1"
}

fn usage_snapshot_count_sql() -> &'static str {
    "SELECT COUNT(1) FROM usage_snapshots"
}

fn latest_usage_snapshot_sql() -> &'static str {
    "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at FROM usage_snapshots ORDER BY captured_at DESC, id DESC LIMIT 1"
}

fn latest_usage_snapshot_for_account_sql() -> &'static str {
    "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at
     FROM usage_snapshots
     WHERE account_id = ?1
     ORDER BY captured_at DESC, id DESC
     LIMIT 1"
}

fn latest_usage_snapshot_with_extra_rate_limits_for_account_sql() -> &'static str {
    "SELECT account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at
     FROM usage_snapshots
     WHERE account_id = ?1
       AND CASE
             WHEN json_valid(credits_json)
             THEN COALESCE(json_array_length(credits_json, '$._codexmanager_extra_rate_limits'), 0)
             ELSE 0
           END > 0
     ORDER BY captured_at DESC, id DESC
     LIMIT 1"
}

fn latest_usage_snapshot_summary_rows_sql() -> String {
    format!(
        "{cte}
        SELECT
            account_id,
            used_percent,
            window_minutes,
            secondary_used_percent,
            secondary_window_minutes,
            credits_json
        FROM ranked
        WHERE rn = 1",
        cte = latest_usage_ranked_cte_sql(
            "account_id,
                used_percent,
                window_minutes,
                secondary_used_percent,
                secondary_window_minutes,
                credits_json",
            None,
        ),
    )
}

pub(super) fn delete_usage_snapshots_for_account_sql() -> &'static str {
    "DELETE FROM usage_snapshots WHERE account_id = ?1"
}

fn prune_usage_snapshots_for_account_sql() -> &'static str {
    "DELETE FROM usage_snapshots
     WHERE account_id = ?1
       AND id NOT IN (
         SELECT id
         FROM usage_snapshots
         WHERE account_id = ?1
         ORDER BY captured_at DESC, id DESC
         LIMIT ?2
       )"
}

fn prune_usage_snapshots_all_accounts_sql() -> &'static str {
    "DELETE FROM usage_snapshots
     WHERE id IN (
         SELECT id
         FROM (
             SELECT
                 id,
                 ROW_NUMBER() OVER (
                     PARTITION BY account_id
                     ORDER BY captured_at DESC, id DESC
                 ) AS rn
             FROM usage_snapshots
         )
         WHERE rn > ?1
     )"
}

impl Storage {
    /// 函数 `insert_usage_snapshot`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - snap: 参数 snap
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_usage_snapshot(&self, snap: &UsageSnapshotRecord) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO usage_snapshots (account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                &snap.account_id,
                snap.used_percent,
                snap.window_minutes,
                snap.resets_at,
                snap.secondary_used_percent,
                snap.secondary_window_minutes,
                snap.secondary_resets_at,
                &snap.credits_json,
                snap.captured_at,
            ),
        )?;
        super::account_reset_warmups::observe_usage_snapshot(&tx, snap)?;
        tx.commit()?;
        Ok(())
    }

    /// Inserts one usage snapshot and prunes older rows for the same account in
    /// a single transaction. A retain value of zero preserves the existing
    /// unlimited-retention behavior.
    pub fn insert_usage_snapshot_and_prune(
        &self,
        snap: &UsageSnapshotRecord,
        retain: usize,
    ) -> Result<usize> {
        self.insert_usage_snapshot_and_prune_with_previous(snap, retain, |_, _| Ok(()))
            .map(|(_, pruned)| pruned)
    }

    /// Resolves a snapshot against the immediately previous row while holding
    /// the same immediate transaction used for insertion and pruning. This
    /// keeps read-modify-write decisions account-serializable across storage
    /// connections.
    pub fn insert_usage_snapshot_and_prune_with_previous<F>(
        &self,
        snap: &UsageSnapshotRecord,
        retain: usize,
        resolve: F,
    ) -> Result<(UsageSnapshotRecord, usize)>
    where
        F: FnOnce(Option<&UsageSnapshotRecord>, &mut Option<String>) -> Result<()>,
    {
        let tx = self.conn.unchecked_transaction()?;
        let previous = tx
            .query_row(
                latest_usage_snapshot_for_account_sql(),
                [&snap.account_id],
                |row| map_usage_snapshot_row(row),
            )
            .optional()?;
        let mut resolved = snap.clone();
        if let Some(previous) = previous.as_ref() {
            // Locally captured snapshots are ordered by arrival. Keep the
            // persisted timestamp monotonic so a corrected system clock cannot
            // make an older, future-dated row survive pruning indefinitely.
            resolved.captured_at = resolved.captured_at.max(previous.captured_at);
        }
        resolve(previous.as_ref(), &mut resolved.credits_json)?;
        tx.execute(
            "INSERT INTO usage_snapshots (account_id, used_percent, window_minutes, resets_at, secondary_used_percent, secondary_window_minutes, secondary_resets_at, credits_json, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                &resolved.account_id,
                resolved.used_percent,
                resolved.window_minutes,
                resolved.resets_at,
                resolved.secondary_used_percent,
                resolved.secondary_window_minutes,
                resolved.secondary_resets_at,
                &resolved.credits_json,
                resolved.captured_at,
            ),
        )?;
        super::account_reset_warmups::observe_usage_snapshot(&tx, &resolved)?;
        let pruned = if retain > 0 {
            tx.execute(
                prune_usage_snapshots_for_account_sql(),
                (&resolved.account_id, retain as i64),
            )?
        } else {
            0
        };
        tx.commit()?;
        Ok((resolved, pruned))
    }

    /// 函数 `prune_usage_snapshots_for_account`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - account_id: 参数 account_id
    /// - retain: 参数 retain
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn prune_usage_snapshots_for_account(
        &self,
        account_id: &str,
        retain: usize,
    ) -> Result<usize> {
        if retain == 0 {
            return Ok(0);
        }
        self.conn.execute(
            prune_usage_snapshots_for_account_sql(),
            (account_id, retain as i64),
        )
    }

    pub fn prune_usage_snapshots_all_accounts(&self, retain: usize) -> Result<usize> {
        if retain == 0 {
            return Ok(0);
        }
        self.conn
            .execute(prune_usage_snapshots_all_accounts_sql(), [retain as i64])
    }

    /// 函数 `usage_snapshot_count_for_account`
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
    pub fn usage_snapshot_count_for_account(&self, account_id: &str) -> Result<i64> {
        self.conn.query_row(
            usage_snapshot_count_for_account_sql(),
            [account_id],
            |row| row.get(0),
        )
    }

    pub fn usage_snapshot_count(&self) -> Result<i64> {
        self.conn
            .query_row(usage_snapshot_count_sql(), [], |row| row.get(0))
    }

    /// 函数 `latest_usage_snapshot`
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
    pub fn latest_usage_snapshot(&self) -> Result<Option<UsageSnapshotRecord>> {
        let mut stmt = self.conn.prepare(latest_usage_snapshot_sql())?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_usage_snapshot_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `latest_usage_snapshot_for_account`
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
    pub fn latest_usage_snapshot_for_account(
        &self,
        account_id: &str,
    ) -> Result<Option<UsageSnapshotRecord>> {
        let mut stmt = self.conn.prepare(latest_usage_snapshot_for_account_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_usage_snapshot_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Returns the newest snapshot for an account that still contains at least
    /// one normalized optional rate-limit bucket.
    ///
    /// A previous application version could persist an empty optional bucket
    /// list when the upstream response temporarily returned `null`.  Keeping
    /// this lookup account-scoped lets the service recover a still-valid
    /// reserve bucket without mixing data between accounts.
    pub fn latest_usage_snapshot_with_extra_rate_limits_for_account(
        &self,
        account_id: &str,
    ) -> Result<Option<UsageSnapshotRecord>> {
        let mut stmt = self
            .conn
            .prepare(latest_usage_snapshot_with_extra_rate_limits_for_account_sql())?;
        let mut rows = stmt.query([account_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_usage_snapshot_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// 函数 `latest_usage_snapshots_by_account`
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
    pub fn latest_usage_snapshots_by_account(&self) -> Result<Vec<UsageSnapshotRecord>> {
        self.latest_usage_snapshots_by_account_limited(None)
    }

    pub fn latest_usage_snapshots_by_account_limited(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<UsageSnapshotRecord>> {
        if limit == Some(0) {
            return Ok(Vec::new());
        }
        let sql = latest_usage_snapshots_by_account_sql(limit);
        let params = limit.map(|limit| vec![limit as i64]).unwrap_or_default();
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_usage_snapshot_row(row)?);
        }
        Ok(out)
    }

    pub fn latest_usage_snapshot_summary_rows(&self) -> Result<Vec<UsageSnapshotSummaryRow>> {
        let sql = latest_usage_snapshot_summary_rows_sql();
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(map_usage_snapshot_summary_row(row)?);
        }
        Ok(out)
    }

    pub fn latest_usage_snapshots_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<UsageSnapshotRecord>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(latest_usage_snapshots_for_accounts_chunk(self, chunk)?);
        }
        out.sort_by(|a, b| {
            b.captured_at
                .cmp(&a.captured_at)
                .then_with(|| a.account_id.cmp(&b.account_id))
        });
        Ok(out)
    }

    pub fn latest_usage_quota_source_rows_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<UsageSnapshotQuotaSourceRow>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(latest_usage_quota_source_rows_for_accounts_chunk(
                self, chunk,
            )?);
        }
        out.sort_by(|a, b| {
            b.captured_at
                .cmp(&a.captured_at)
                .then_with(|| a.account_id.cmp(&b.account_id))
        });
        Ok(out)
    }

    pub fn latest_usage_cleanup_rows_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<UsageSnapshotCleanupRow>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(latest_usage_cleanup_rows_for_accounts_chunk(self, chunk)?);
        }
        out.sort_by(|a, b| a.account_id.cmp(&b.account_id));
        Ok(out)
    }

    pub fn low_quota_account_ids_for_accounts(
        &self,
        account_ids: &[String],
        primary_min_remaining_percent: f64,
        secondary_min_remaining_percent: f64,
    ) -> Result<Vec<String>> {
        let account_ids = normalize_text_ids(account_ids);
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }
        let primary_threshold = normalize_remaining_threshold(primary_min_remaining_percent);
        let secondary_threshold = normalize_remaining_threshold(secondary_min_remaining_percent);
        if primary_threshold <= 0.0 && secondary_threshold <= 0.0 {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            out.extend(low_quota_account_ids_for_accounts_chunk(
                self,
                chunk,
                primary_threshold,
                secondary_threshold,
            )?);
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// 函数 `ensure_usage_secondary_columns`
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
    pub(super) fn ensure_usage_secondary_columns(&self) -> Result<()> {
        self.ensure_column("usage_snapshots", "secondary_used_percent", "REAL")?;
        self.ensure_column("usage_snapshots", "secondary_window_minutes", "INTEGER")?;
        self.ensure_column("usage_snapshots", "secondary_resets_at", "INTEGER")?;
        Ok(())
    }
}

fn normalize_remaining_threshold(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

/// 函数 `map_usage_snapshot_row`
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
fn map_usage_snapshot_row(row: &Row<'_>) -> Result<UsageSnapshotRecord> {
    Ok(UsageSnapshotRecord {
        account_id: row.get(0)?,
        used_percent: row.get(1)?,
        window_minutes: row.get(2)?,
        resets_at: row.get(3)?,
        secondary_used_percent: row.get(4)?,
        secondary_window_minutes: row.get(5)?,
        secondary_resets_at: row.get(6)?,
        credits_json: row.get(7)?,
        captured_at: row.get(8)?,
    })
}

fn map_usage_snapshot_summary_row(row: &Row<'_>) -> Result<UsageSnapshotSummaryRow> {
    Ok(UsageSnapshotSummaryRow {
        account_id: row.get(0)?,
        used_percent: row.get(1)?,
        window_minutes: row.get(2)?,
        secondary_used_percent: row.get(3)?,
        secondary_window_minutes: row.get(4)?,
        credits_json: row.get(5)?,
    })
}

fn map_usage_quota_source_row(row: &Row<'_>) -> Result<UsageSnapshotQuotaSourceRow> {
    Ok(UsageSnapshotQuotaSourceRow {
        account_id: row.get(0)?,
        used_percent: row.get(1)?,
        secondary_used_percent: row.get(2)?,
        captured_at: row.get(3)?,
    })
}

fn map_usage_cleanup_row(row: &Row<'_>) -> Result<UsageSnapshotCleanupRow> {
    Ok(UsageSnapshotCleanupRow {
        account_id: row.get(0)?,
        used_percent: row.get(1)?,
        window_minutes: row.get(2)?,
        secondary_used_percent: row.get(3)?,
        secondary_window_minutes: row.get(4)?,
        credits_json: row.get(5)?,
    })
}

fn latest_usage_snapshots_by_account_sql(limit: Option<usize>) -> String {
    let mut sql = format!(
        "{cte}
        SELECT
            account_id,
            used_percent,
            window_minutes,
            resets_at,
            secondary_used_percent,
            secondary_window_minutes,
            secondary_resets_at,
            credits_json,
            captured_at
        FROM ranked
        WHERE rn = 1
        ORDER BY captured_at DESC, id DESC",
        cte = latest_usage_ranked_cte_sql(
            "account_id,
                used_percent,
                window_minutes,
                resets_at,
                secondary_used_percent,
                secondary_window_minutes,
                secondary_resets_at,
                credits_json,
                captured_at",
            None,
        ),
    );
    if limit.is_some() {
        sql.push_str(" LIMIT ?");
    }
    sql
}
fn latest_usage_ranked_cte_sql(select_columns: &str, where_condition: Option<&str>) -> String {
    let where_clause = where_condition
        .map(|condition| format!("WHERE {condition}"))
        .unwrap_or_default();
    format!(
        "WITH ranked AS (
            SELECT
                id,
                {select_columns},
                ROW_NUMBER() OVER (
                    PARTITION BY account_id
                    ORDER BY captured_at DESC, id DESC
                ) AS rn
            FROM usage_snapshots
            {where_clause}
        )"
    )
}

fn latest_usage_snapshots_for_accounts_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<UsageSnapshotRecord>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = latest_usage_snapshots_for_accounts_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(params))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(map_usage_snapshot_row(row)?);
    }
    Ok(out)
}

fn latest_usage_snapshots_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "{cte}
        SELECT
            account_id,
            used_percent,
            window_minutes,
            resets_at,
            secondary_used_percent,
            secondary_window_minutes,
            secondary_resets_at,
            credits_json,
            captured_at
        FROM ranked
        WHERE rn = 1",
        cte = latest_usage_ranked_cte_sql(
            "account_id,
                used_percent,
                window_minutes,
                resets_at,
                secondary_used_percent,
                secondary_window_minutes,
                secondary_resets_at,
                credits_json,
                captured_at",
            Some(account_condition),
        ),
    )
}

fn latest_usage_quota_source_rows_for_accounts_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<UsageSnapshotQuotaSourceRow>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = latest_usage_quota_source_rows_for_accounts_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), map_usage_quota_source_row)?;
    rows.collect()
}

fn latest_usage_quota_source_rows_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "{cte}
        SELECT account_id, used_percent, secondary_used_percent, captured_at
        FROM ranked
        WHERE rn = 1",
        cte = latest_usage_ranked_cte_sql(
            "account_id,
                used_percent,
                secondary_used_percent,
                captured_at",
            Some(account_condition),
        ),
    )
}

fn latest_usage_cleanup_rows_for_accounts_chunk(
    storage: &Storage,
    account_ids: &[String],
) -> Result<Vec<UsageSnapshotCleanupRow>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = latest_usage_cleanup_rows_for_accounts_chunk_sql(&condition);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), map_usage_cleanup_row)?;
    rows.collect()
}

fn latest_usage_cleanup_rows_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "{cte}
        SELECT
            account_id,
            used_percent,
            window_minutes,
            secondary_used_percent,
            secondary_window_minutes,
            credits_json
        FROM ranked
        WHERE rn = 1",
        cte = latest_usage_ranked_cte_sql(
            "account_id,
                used_percent,
                window_minutes,
                secondary_used_percent,
                secondary_window_minutes,
                credits_json",
            Some(account_condition),
        ),
    )
}

fn low_quota_account_ids_for_accounts_chunk(
    storage: &Storage,
    account_ids: &[String],
    primary_min_remaining_percent: f64,
    secondary_min_remaining_percent: f64,
) -> Result<Vec<String>> {
    let Some((condition, params)) = text_id_in_clause("account_id", account_ids) else {
        return Ok(Vec::new());
    };
    let sql = low_quota_account_ids_for_accounts_chunk_sql(&condition);
    let mut values = params;
    values.extend([
        rusqlite::types::Value::Real(secondary_min_remaining_percent),
        rusqlite::types::Value::Real(primary_min_remaining_percent),
        rusqlite::types::Value::Real(secondary_min_remaining_percent),
        rusqlite::types::Value::Real(primary_min_remaining_percent),
        rusqlite::types::Value::Real(primary_min_remaining_percent),
        rusqlite::types::Value::Real(secondary_min_remaining_percent),
        rusqlite::types::Value::Real(primary_min_remaining_percent),
        rusqlite::types::Value::Real(secondary_min_remaining_percent),
    ]);
    let mut stmt = storage.conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values), |row| row.get(0))?;
    rows.collect()
}

fn low_quota_account_ids_for_accounts_chunk_sql(account_condition: &str) -> String {
    format!(
        "{cte}
        SELECT account_id
        FROM ranked
        WHERE rn = 1
          AND (
                (
                    used_percent IS NOT NULL
                    AND CASE
                        WHEN window_minutes > {long_window_minutes} THEN ?
                        ELSE ?
                    END > 0.0
                    AND (100.0 - used_percent) <= CASE
                        WHEN window_minutes > {long_window_minutes} THEN ?
                        ELSE ?
                    END
                )
                OR (
                    secondary_used_percent IS NOT NULL
                    AND CASE
                        WHEN secondary_window_minutes IS NOT NULL
                             AND secondary_window_minutes <= {long_window_minutes} THEN ?
                        ELSE ?
                    END > 0.0
                    AND (100.0 - secondary_used_percent) <= CASE
                        WHEN secondary_window_minutes IS NOT NULL
                             AND secondary_window_minutes <= {long_window_minutes} THEN ?
                        ELSE ?
                    END
                )
          )",
        cte = latest_usage_ranked_cte_sql(
            "account_id,
                used_percent,
                window_minutes,
                secondary_used_percent,
                secondary_window_minutes",
            Some(account_condition),
        ),
        long_window_minutes = LONG_USAGE_WINDOW_MINUTES,
    )
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
