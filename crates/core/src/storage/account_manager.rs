use std::collections::HashMap;

use rusqlite::{params, OptionalExtension, Result, Row};

use super::{
    now_ts, ApiKeyOwner, AppProject, AppUser, AppUserSession, AppWallet, AppWalletLedgerEntry,
    BillingRule, Storage,
};

fn map_app_user(row: &Row<'_>) -> Result<AppUser> {
    Ok(AppUser {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        password_hash: row.get(3)?,
        role: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        last_login_at: row.get(8)?,
    })
}

fn map_app_session(row: &Row<'_>) -> Result<AppUserSession> {
    Ok(AppUserSession {
        id: row.get(0)?,
        user_id: row.get(1)?,
        token_hash: row.get(2)?,
        expires_at: row.get(3)?,
        created_at: row.get(4)?,
        last_seen_at: row.get(5)?,
        revoked_at: row.get(6)?,
    })
}

fn map_app_wallet(row: &Row<'_>) -> Result<AppWallet> {
    Ok(AppWallet {
        id: row.get(0)?,
        owner_kind: row.get(1)?,
        owner_id: row.get(2)?,
        balance_credit_micros: row.get(3)?,
        frozen_credit_micros: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn map_api_key_owner(row: &Row<'_>) -> Result<ApiKeyOwner> {
    Ok(ApiKeyOwner {
        key_id: row.get(0)?,
        owner_kind: row.get(1)?,
        owner_user_id: row.get(2)?,
        project_id: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn map_billing_rule(row: &Row<'_>) -> Result<BillingRule> {
    Ok(BillingRule {
        id: row.get(0)?,
        name: row.get(1)?,
        status: row.get(2)?,
        priority: row.get(3)?,
        multiplier_millis: row.get(4)?,
        model_pattern: row.get(5)?,
        service_tier: row.get(6)?,
        user_id: row.get(7)?,
        project_id: row.get(8)?,
        api_key_id: row.get(9)?,
        starts_at: row.get(10)?,
        ends_at: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

impl Storage {
    pub fn app_user_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM app_users", [], |row| row.get(0))
    }

    pub fn member_app_user_count(&self) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM app_users WHERE role = 'member'",
            [],
            |row| row.get(0),
        )
    }

    pub fn active_admin_count(&self) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM app_users WHERE role = 'admin' AND status = 'active'",
            [],
            |row| row.get(0),
        )
    }

    pub fn insert_app_user(&self, user: &AppUser) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_users (
                id, username, display_name, password_hash, role, status,
                created_at, updated_at, last_login_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                &user.id,
                &user.username,
                &user.display_name,
                &user.password_hash,
                &user.role,
                &user.status,
                user.created_at,
                user.updated_at,
                user.last_login_at,
            ),
        )?;
        Ok(())
    }

    pub fn delete_app_user(&self, user_id: &str) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM api_key_owners
             WHERE owner_kind = 'user' AND owner_user_id = ?1",
            [user_id],
        )?;
        tx.execute(
            "DELETE FROM app_user_sessions WHERE user_id = ?1",
            [user_id],
        )?;
        tx.execute(
            "DELETE FROM user_model_groups WHERE user_id = ?1",
            [user_id],
        )?;
        tx.execute(
            "DELETE FROM app_wallet_ledger_entries
             WHERE wallet_id IN (
                SELECT id FROM app_wallets WHERE owner_kind = 'user' AND owner_id = ?1
             )",
            [user_id],
        )?;
        tx.execute(
            "DELETE FROM app_wallets WHERE owner_kind = 'user' AND owner_id = ?1",
            [user_id],
        )?;
        let deleted = tx.execute("DELETE FROM app_users WHERE id = ?1", [user_id])?;
        tx.commit()?;
        Ok(deleted)
    }

    pub fn list_app_users(&self) -> Result<Vec<AppUser>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, display_name, password_hash, role, status,
                    created_at, updated_at, last_login_at
             FROM app_users
             ORDER BY created_at ASC, username ASC",
        )?;
        let rows = stmt.query_map([], map_app_user)?;
        rows.collect()
    }

    pub fn find_app_user_by_username(&self, username: &str) -> Result<Option<AppUser>> {
        self.conn
            .query_row(
                "SELECT id, username, display_name, password_hash, role, status,
                        created_at, updated_at, last_login_at
                 FROM app_users
                 WHERE lower(username) = lower(?1)
                 LIMIT 1",
                [username],
                map_app_user,
            )
            .optional()
    }

    pub fn find_app_user_by_id(&self, id: &str) -> Result<Option<AppUser>> {
        self.conn
            .query_row(
                "SELECT id, username, display_name, password_hash, role, status,
                        created_at, updated_at, last_login_at
                 FROM app_users
                 WHERE id = ?1
                 LIMIT 1",
                [id],
                map_app_user,
            )
            .optional()
    }

    pub fn update_app_user_last_login(&self, id: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE app_users SET last_login_at = ?1, updated_at = ?1 WHERE id = ?2",
            (ts, id),
        )?;
        Ok(())
    }

    pub fn update_app_user_status(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE app_users SET status = ?1, updated_at = ?2 WHERE id = ?3",
            (status, now_ts(), id),
        )?;
        Ok(())
    }

    pub fn update_app_user_role(&self, id: &str, role: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE app_users SET role = ?1, updated_at = ?2 WHERE id = ?3",
            (role, now_ts(), id),
        )?;
        Ok(())
    }

    pub fn update_app_user_display_name(
        &self,
        id: &str,
        display_name: Option<String>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE app_users SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
            (display_name, now_ts(), id),
        )?;
        Ok(())
    }

    pub fn update_app_user_password_hash(&self, id: &str, password_hash: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE app_users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            (password_hash, now_ts(), id),
        )?;
        Ok(())
    }

    pub fn insert_app_user_session(&self, session: &AppUserSession) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_user_sessions (
                id, user_id, token_hash, expires_at, created_at, last_seen_at, revoked_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                &session.id,
                &session.user_id,
                &session.token_hash,
                session.expires_at,
                session.created_at,
                session.last_seen_at,
                session.revoked_at,
            ),
        )?;
        Ok(())
    }

    pub fn find_active_app_session_by_token_hash(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<AppUserSession>> {
        self.conn
            .query_row(
                "SELECT id, user_id, token_hash, expires_at, created_at, last_seen_at, revoked_at
                 FROM app_user_sessions
                 WHERE token_hash = ?1 AND revoked_at IS NULL AND expires_at > ?2
                 LIMIT 1",
                (token_hash, now),
                map_app_session,
            )
            .optional()
    }

    pub fn touch_app_user_session(&self, session_id: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE app_user_sessions SET last_seen_at = ?1 WHERE id = ?2",
            (ts, session_id),
        )?;
        Ok(())
    }

    pub fn revoke_app_user_session_by_token_hash(&self, token_hash: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE app_user_sessions
             SET revoked_at = ?1
             WHERE token_hash = ?2 AND revoked_at IS NULL",
            (ts, token_hash),
        )?;
        Ok(())
    }

    pub fn insert_app_project(&self, project: &AppProject) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_projects (
                id, name, owner_user_id, status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                &project.id,
                &project.name,
                &project.owner_user_id,
                &project.status,
                project.created_at,
                project.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn ensure_wallet_for_owner(
        &self,
        id: &str,
        owner_kind: &str,
        owner_id: &str,
    ) -> Result<AppWallet> {
        if let Some(wallet) = self.find_wallet_by_owner(owner_kind, owner_id)? {
            return Ok(wallet);
        }
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO app_wallets (
                id, owner_kind, owner_id, balance_credit_micros, frozen_credit_micros,
                status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 0, 0, 'active', ?4, ?4)",
            (id, owner_kind, owner_id, now),
        )?;
        self.find_wallet_by_owner(owner_kind, owner_id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn find_wallet_by_owner(
        &self,
        owner_kind: &str,
        owner_id: &str,
    ) -> Result<Option<AppWallet>> {
        self.conn
            .query_row(
                "SELECT id, owner_kind, owner_id, balance_credit_micros, frozen_credit_micros,
                        status, created_at, updated_at
                 FROM app_wallets
                 WHERE owner_kind = ?1 AND owner_id = ?2
                 LIMIT 1",
                (owner_kind, owner_id),
                map_app_wallet,
            )
            .optional()
    }

    pub fn list_wallets(&self) -> Result<Vec<AppWallet>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, owner_kind, owner_id, balance_credit_micros, frozen_credit_micros,
                    status, created_at, updated_at
             FROM app_wallets
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], map_app_wallet)?;
        rows.collect()
    }

    pub fn nonzero_wallet_count(&self) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*)
             FROM app_wallets
             WHERE balance_credit_micros <> 0 OR frozen_credit_micros <> 0",
            [],
            |row| row.get(0),
        )
    }

    pub fn wallet_ledger_entry_count(&self) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM app_wallet_ledger_entries",
            [],
            |row| row.get(0),
        )
    }

    pub fn request_charge_ledger_entry_count(&self) -> Result<i64> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM app_wallet_ledger_entries WHERE entry_kind = 'request_charge'",
            [],
            |row| row.get(0),
        )
    }

    pub fn adjust_wallet_balance(
        &self,
        ledger: &AppWalletLedgerEntry,
    ) -> Result<AppWalletLedgerEntry> {
        let tx = self.conn.unchecked_transaction()?;
        let balance_after = tx.query_row(
            "SELECT balance_credit_micros + ?2
             FROM app_wallets
             WHERE id = ?1 AND status = 'active'",
            (&ledger.wallet_id, ledger.amount_credit_micros),
            |row| row.get::<_, i64>(0),
        )?;
        tx.execute(
            "UPDATE app_wallets
             SET balance_credit_micros = balance_credit_micros + ?2, updated_at = ?3
             WHERE id = ?1",
            (
                &ledger.wallet_id,
                ledger.amount_credit_micros,
                ledger.created_at,
            ),
        )?;
        tx.execute(
            "INSERT INTO app_wallet_ledger_entries (
                id, wallet_id, entry_kind, amount_credit_micros, balance_after_credit_micros,
                request_log_id, api_key_id, pricing_rule_id, raw_usage_json, note,
                created_by_user_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            (
                &ledger.id,
                &ledger.wallet_id,
                &ledger.entry_kind,
                ledger.amount_credit_micros,
                balance_after,
                ledger.request_log_id,
                &ledger.api_key_id,
                &ledger.pricing_rule_id,
                &ledger.raw_usage_json,
                &ledger.note,
                &ledger.created_by_user_id,
                ledger.created_at,
            ),
        )?;
        tx.commit()?;
        let mut next = ledger.clone();
        next.balance_after_credit_micros = balance_after;
        Ok(next)
    }

    pub fn upsert_api_key_owner(&self, owner: &ApiKeyOwner) -> Result<()> {
        self.conn.execute(
            "INSERT INTO api_key_owners (
                key_id, owner_kind, owner_user_id, project_id, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key_id) DO UPDATE SET
                owner_kind = excluded.owner_kind,
                owner_user_id = excluded.owner_user_id,
                project_id = excluded.project_id,
                updated_at = excluded.updated_at",
            (
                &owner.key_id,
                &owner.owner_kind,
                &owner.owner_user_id,
                &owner.project_id,
                owner.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn find_api_key_owner(&self, key_id: &str) -> Result<Option<ApiKeyOwner>> {
        self.conn
            .query_row(
                "SELECT key_id, owner_kind, owner_user_id, project_id, updated_at
                 FROM api_key_owners
                 WHERE key_id = ?1
                 LIMIT 1",
                [key_id],
                map_api_key_owner,
            )
            .optional()
    }

    pub fn list_api_key_owners(&self) -> Result<HashMap<String, ApiKeyOwner>> {
        let mut stmt = self.conn.prepare(
            "SELECT key_id, owner_kind, owner_user_id, project_id, updated_at
             FROM api_key_owners",
        )?;
        let rows = stmt.query_map([], map_api_key_owner)?;
        let mut out = HashMap::new();
        for row in rows {
            let owner = row?;
            out.insert(owner.key_id.clone(), owner);
        }
        Ok(out)
    }

    /// 函数 `list_api_key_ids_for_user`
    ///
    ///
    /// 时间: 2026-05-28
    ///
    /// # 参数
    /// - self: 参数 self
    /// - user_id: 参数 user_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_api_key_ids_for_user(&self, user_id: &str) -> Result<Vec<String>> {
        let normalized_user_id = user_id.trim();
        if normalized_user_id.is_empty() {
            return Ok(Vec::new());
        }

        // 中文注释：这里直接按 owner_user_id 走索引取 key_id，避免把整张 api_key_owners 表拉回 Rust 再过滤。
        let mut stmt = self.conn.prepare(
            "SELECT key_id
             FROM api_key_owners
             WHERE owner_kind = 'user' AND owner_user_id = ?1
             ORDER BY key_id ASC",
        )?;
        let rows = stmt.query_map([normalized_user_id], |row| row.get(0))?;
        rows.collect()
    }

    pub fn api_key_owner_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM api_key_owners", [], |row| row.get(0))
    }

    pub fn list_billing_rules(&self) -> Result<Vec<BillingRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id, name, status, priority, multiplier_millis, model_pattern, service_tier,
                user_id, project_id, api_key_id, starts_at, ends_at, created_at, updated_at
             FROM billing_rules
             ORDER BY status ASC, priority DESC, updated_at DESC, name ASC",
        )?;
        let rows = stmt.query_map([], map_billing_rule)?;
        rows.collect()
    }

    pub fn list_active_billing_rules(&self, now: i64) -> Result<Vec<BillingRule>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                id, name, status, priority, multiplier_millis, model_pattern, service_tier,
                user_id, project_id, api_key_id, starts_at, ends_at, created_at, updated_at
             FROM billing_rules
             WHERE status = 'active'
               AND (starts_at IS NULL OR starts_at <= ?1)
               AND (ends_at IS NULL OR ends_at > ?1)
             ORDER BY priority DESC, updated_at DESC, name ASC",
        )?;
        let rows = stmt.query_map([now], map_billing_rule)?;
        rows.collect()
    }

    pub fn upsert_billing_rule(&self, rule: &BillingRule) -> Result<()> {
        self.conn.execute(
            "INSERT INTO billing_rules (
                id, name, status, priority, multiplier_millis, model_pattern, service_tier,
                user_id, project_id, api_key_id, starts_at, ends_at, created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
             )
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                status = excluded.status,
                priority = excluded.priority,
                multiplier_millis = excluded.multiplier_millis,
                model_pattern = excluded.model_pattern,
                service_tier = excluded.service_tier,
                user_id = excluded.user_id,
                project_id = excluded.project_id,
                api_key_id = excluded.api_key_id,
                starts_at = excluded.starts_at,
                ends_at = excluded.ends_at,
                updated_at = excluded.updated_at",
            params![
                &rule.id,
                &rule.name,
                &rule.status,
                rule.priority,
                rule.multiplier_millis,
                &rule.model_pattern,
                &rule.service_tier,
                &rule.user_id,
                &rule.project_id,
                &rule.api_key_id,
                rule.starts_at,
                rule.ends_at,
                rule.created_at,
                rule.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_billing_rule(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM billing_rules WHERE id = ?1", [id])?;
        Ok(())
    }

    pub(super) fn ensure_account_manager_tables(&self) -> Result<()> {
        self.conn
            .execute_batch(include_str!("../../migrations/057_account_manager.sql"))?;
        Ok(())
    }
}
