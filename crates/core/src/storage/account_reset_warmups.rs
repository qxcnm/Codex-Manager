use rusqlite::{params_from_iter, OptionalExtension, Result, Transaction};

use super::key_id_filters::{normalize_text_ids, text_id_in_clause, SQLITE_IN_CLAUSE_BATCH_SIZE};
use super::{Storage, UsageSnapshotRecord};

const RESET_GRACE_SECONDS: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountResetWarmupTarget {
    pub account_id: String,
    pub reset_at: i64,
    pub due_at: i64,
}

// Use the latest main quota windows, including a long window that can occupy
// either position. An exhausted long window with no reset time cannot be sent.
fn eligible_targets_sql() -> String {
    format!(
        "SELECT w.account_id, w.pending_reset_at AS reset_at,
                MAX(w.pending_reset_at,
                    CASE WHEN s.window_minutes > 300 AND s.used_percent >= 100
                         THEN CASE WHEN s.resets_at > 0 THEN s.resets_at END ELSE 0 END,
                    CASE WHEN s.secondary_window_minutes > 300 AND s.secondary_used_percent >= 100
                         THEN CASE WHEN s.secondary_resets_at > 0 THEN s.secondary_resets_at END ELSE 0 END) + {RESET_GRACE_SECONDS} AS due_at
         FROM account_reset_warmups w
         JOIN accounts a ON a.id = w.account_id
         JOIN tokens t ON t.account_id = a.id
         JOIN usage_snapshots s ON s.id = (
             SELECT id FROM usage_snapshots WHERE account_id = a.id
             ORDER BY captured_at DESC, id DESC LIMIT 1
         )
         WHERE w.enabled = 1 AND w.pending_reset_at IS NOT NULL
           AND w.pending_reset_at > COALESCE(w.consumed_reset_at, 0)
           AND LOWER(TRIM(a.status)) NOT IN ('inactive', 'disabled', 'unavailable', 'banned')
           AND LENGTH(TRIM(t.access_token)) > 0"
    )
}

impl Storage {
    pub fn list_account_reset_warmup_targets(
        &self,
        now: i64,
        limit: usize,
    ) -> Result<Vec<AccountResetWarmupTarget>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT account_id, reset_at, due_at FROM ({})
             WHERE reset_at <= ?1 AND due_at <= ?1 ORDER BY due_at, account_id LIMIT ?2",
            eligible_targets_sql()
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map((now, limit.min(i64::MAX as usize) as i64), |row| {
            Ok(AccountResetWarmupTarget {
                account_id: row.get(0)?,
                reset_at: row.get(1)?,
                due_at: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// Reserve one attempt before sending. All eligibility checks run in the
    /// same UPDATE so another worker, process, or settings change cannot claim
    /// the same cycle twice. A failed/ambiguous send is deliberately not retried.
    pub fn claim_account_reset_warmup(
        &self,
        account_id: &str,
        reset_at: i64,
        now: i64,
    ) -> Result<bool> {
        let sql = format!(
            "UPDATE account_reset_warmups
             SET consumed_reset_at = pending_reset_at, pending_reset_at = NULL, last_claimed_at = ?3
             WHERE account_id = ?1 AND pending_reset_at = ?2
               AND EXISTS (
                   SELECT 1 FROM ({}) eligible
                   WHERE eligible.account_id = ?1 AND eligible.reset_at = ?2 AND eligible.due_at <= ?3
               )",
            eligible_targets_sql()
        );
        Ok(self.conn.execute(&sql, (account_id, reset_at, now))? == 1)
    }

    /// Apply an entire selection atomically, rejecting malformed or missing
    /// accounts before changing any setting. Import upserts never touch this table.
    pub fn set_account_reset_warmup_enabled(
        &self,
        account_ids: &[String],
        enabled: bool,
    ) -> Result<usize> {
        if account_ids.is_empty()
            || account_ids
                .iter()
                .any(|id| id.trim().is_empty() || id.trim() != id)
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "accountIds must contain non-empty account IDs".to_string(),
            ));
        }
        let account_ids = normalize_text_ids(account_ids);
        let tx = self.conn.unchecked_transaction()?;
        for account_id in &account_ids {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = ?1)",
                [account_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
        }
        for account_id in &account_ids {
            tx.execute(
                "INSERT INTO account_reset_warmups (account_id, enabled) VALUES (?1, ?2)
                 ON CONFLICT(account_id) DO UPDATE SET enabled = excluded.enabled",
                (account_id, enabled),
            )?;
        }
        tx.commit()?;
        Ok(account_ids.len())
    }

    pub fn list_account_reset_warmup_settings_for_accounts(
        &self,
        account_ids: &[String],
    ) -> Result<Vec<(String, bool)>> {
        let account_ids = normalize_text_ids(account_ids);
        let mut settings = Vec::new();
        for chunk in account_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            let Some((condition, params)) = text_id_in_clause("a.id", chunk) else {
                continue;
            };
            let sql = format!(
                "SELECT a.id, COALESCE(w.enabled, 1) FROM accounts a
                 LEFT JOIN account_reset_warmups w ON w.account_id = a.id
                 WHERE {condition} ORDER BY a.id"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            settings.extend(
                stmt.query_map(params_from_iter(params), |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<Result<Vec<_>>>()?,
            );
        }
        Ok(settings)
    }
}

/// Called inside the snapshot transaction, before pruning the exhausted row.
/// Keeping the pending cycle separately prevents a post-reset quota refresh
/// from erasing the reminder before the worker can run.
pub(super) fn observe_usage_snapshot(
    conn: &Transaction<'_>,
    snap: &UsageSnapshotRecord,
) -> Result<()> {
    let windows = [
        (snap.window_minutes, snap.used_percent, snap.resets_at),
        (
            snap.secondary_window_minutes,
            snap.secondary_used_percent,
            snap.secondary_resets_at,
        ),
    ];
    let five_hour_windows = windows
        .into_iter()
        .filter_map(|(minutes, used, reset)| {
            (minutes == Some(300))
                .then_some((used, reset))
                .and_then(|(used, reset)| {
                    reset.filter(|reset| *reset > 0).map(|reset| (used, reset))
                })
        })
        .collect::<Vec<_>>();
    if five_hour_windows.is_empty() {
        return Ok(());
    }
    let latest_captured_at = conn
        .query_row(
            "SELECT captured_at FROM usage_snapshots WHERE account_id = ?1
         ORDER BY captured_at DESC, id DESC LIMIT 1",
            [&snap.account_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if latest_captured_at.is_some_and(|captured_at| captured_at > snap.captured_at) {
        return Ok(());
    }
    // A later reset in a recovered window means a real request already started
    // the next cycle. Mark the old cycle consumed so stale snapshots cannot rearm it.
    if let Some(reset_at) = five_hour_windows
        .iter()
        .filter(|(used, _)| used.is_some_and(|used| used.is_finite() && used > 0.0 && used < 100.0))
        .map(|(_, reset)| *reset)
        .max()
    {
        conn.execute(
            "UPDATE account_reset_warmups
             SET consumed_reset_at = MAX(COALESCE(consumed_reset_at, 0), pending_reset_at), pending_reset_at = NULL
             WHERE account_id = ?1 AND pending_reset_at < ?2 AND pending_reset_at <= ?3",
            (&snap.account_id, reset_at, snap.captured_at),
        )?;
    }
    if let Some(reset_at) = five_hour_windows
        .iter()
        .filter(|(used, _)| used.is_some_and(|used| used.is_finite() && used >= 100.0))
        .map(|(_, reset)| *reset)
        .max()
    {
        conn.execute(
            "INSERT INTO account_reset_warmups (account_id, pending_reset_at)
             SELECT id, ?2 FROM accounts WHERE id = ?1
             ON CONFLICT(account_id) DO UPDATE SET
                 pending_reset_at = MAX(COALESCE(account_reset_warmups.pending_reset_at, 0), excluded.pending_reset_at)
             WHERE excluded.pending_reset_at > COALESCE(account_reset_warmups.consumed_reset_at, 0)",
            (&snap.account_id, reset_at),
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "account_reset_warmups_tests.rs"]
mod tests;
