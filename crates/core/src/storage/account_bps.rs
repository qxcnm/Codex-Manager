use super::{
    key_id_filters::{normalize_text_ids, text_id_in_clause, SQLITE_IN_CLAUSE_BATCH_SIZE},
    now_ts, Storage,
};
use rusqlite::{params, params_from_iter, OptionalExtension, Result};

/// Private account-scoped BPS credentials. Never serialize this record to RPC.
#[derive(Clone)]
pub struct AccountBpsSettings {
    pub account_id: String,
    pub enabled: bool,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub upstream_account_id: Option<String>,
    pub expires_at: Option<i64>,
    pub updated_at: i64,
}

impl AccountBpsSettings {
    pub fn authorized_at(&self, now: i64) -> bool {
        self.access_token.as_ref().is_some_and(|v| !v.is_empty())
            && self
                .upstream_account_id
                .as_ref()
                .is_some_and(|v| !v.is_empty())
            && self.expires_at.is_some_and(|expiry| expiry > now)
    }
}

fn row_settings(row: &rusqlite::Row<'_>) -> Result<AccountBpsSettings> {
    Ok(AccountBpsSettings {
        account_id: row.get(0)?,
        enabled: row.get(1)?,
        access_token: row.get(2)?,
        refresh_token: row.get(3)?,
        upstream_account_id: row.get(4)?,
        expires_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

impl Storage {
    pub fn any_account_bps_enabled(&self) -> Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM account_bps_settings WHERE enabled = 1)",
            [],
            |r| r.get(0),
        )
    }
    pub fn find_account_bps_settings(&self, id: &str) -> Result<Option<AccountBpsSettings>> {
        self.conn.query_row("SELECT account_id, enabled, access_token, refresh_token, upstream_account_id, expires_at, updated_at FROM account_bps_settings WHERE account_id = ?1", [id], row_settings).optional()
    }

    pub fn list_account_bps_settings(&self, ids: &[String]) -> Result<Vec<AccountBpsSettings>> {
        let ids = normalize_text_ids(ids);
        let mut rows = Vec::new();
        for chunk in ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            let Some((clause, values)) = text_id_in_clause("account_id", chunk) else {
                continue;
            };
            let mut stmt = self.conn.prepare(&format!("SELECT account_id, enabled, access_token, refresh_token, upstream_account_id, expires_at, updated_at FROM account_bps_settings WHERE {clause}"))?;
            rows.extend(
                stmt.query_map(params_from_iter(values), row_settings)?
                    .collect::<Result<Vec<_>>>()?,
            );
        }
        Ok(rows)
    }

    /// Preserve the routing choice on reauthorization and leave Codex tokens untouched.
    pub fn save_account_bps_credentials(&self, settings: &AccountBpsSettings) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let expected: Option<String> = tx.query_row("SELECT COALESCE(NULLIF(chatgpt_account_id, ''), NULLIF(workspace_id, '')) FROM accounts WHERE id = ?1", [&settings.account_id], |r| r.get(0))?;
        if expected
            .as_deref()
            .is_none_or(|v| Some(v) != settings.upstream_account_id.as_deref())
            || !settings.authorized_at(now_ts())
        {
            return Err(rusqlite::Error::InvalidParameterName(
                "BPS credentials do not match this account or are expired".into(),
            ));
        }
        tx.execute("INSERT INTO account_bps_settings (account_id, enabled, access_token, refresh_token, upstream_account_id, expires_at, updated_at) VALUES (?1, 0, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(account_id) DO UPDATE SET access_token = excluded.access_token, refresh_token = excluded.refresh_token, upstream_account_id = excluded.upstream_account_id, expires_at = excluded.expires_at, updated_at = excluded.updated_at", params![settings.account_id, settings.access_token, settings.refresh_token, settings.upstream_account_id, settings.expires_at, now_ts()])?;
        tx.commit()
    }

    /// The complete selection is validated before any routing preference changes.
    pub fn set_account_bps_enabled(&self, ids: &[String], enabled: bool) -> Result<usize> {
        if ids.is_empty() || ids.iter().any(|id| id.is_empty() || id.trim() != id) {
            return Err(rusqlite::Error::InvalidParameterName(
                "accountIds must contain non-empty account IDs".into(),
            ));
        }
        let ids = normalize_text_ids(ids);
        let tx = self.conn.unchecked_transaction()?;
        for id in &ids {
            let expected: Option<String> = tx.query_row("SELECT COALESCE(NULLIF(chatgpt_account_id, ''), NULLIF(workspace_id, '')) FROM accounts WHERE id = ?1", [id], |r| r.get(0))?;
            if enabled {
                let settings = tx.query_row("SELECT account_id, enabled, access_token, refresh_token, upstream_account_id, expires_at, updated_at FROM account_bps_settings WHERE account_id = ?1", [id], row_settings).optional()?;
                if !settings.is_some_and(|s| {
                    s.authorized_at(now_ts())
                        && expected.is_some()
                        && s.upstream_account_id == expected
                }) {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "BPS authorization is required for every selected account".into(),
                    ));
                }
            }
        }
        for id in &ids {
            tx.execute("INSERT INTO account_bps_settings (account_id, enabled, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(account_id) DO UPDATE SET enabled = excluded.enabled, updated_at = excluded.updated_at", params![id, enabled, now_ts()])?;
        }
        tx.commit()?;
        Ok(ids.len())
    }
}

#[cfg(test)]
#[path = "account_bps_tests.rs"]
mod tests;
