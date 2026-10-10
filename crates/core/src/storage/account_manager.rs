use std::collections::HashMap;

use rusqlite::{params, params_from_iter, OptionalExtension, Result, Row};

use super::{
    now_ts, ApiKeyOwner, AppProject, AppSessionUserWithWallet, AppUser, AppUserAccessSummary,
    AppUserSession, AppWallet, AppWalletLedgerEntry, BillingRule, DashboardAppUserSummary,
    PublicAppUserWithWallet, Storage,
};
use crate::storage::key_id_filters::{
    normalize_text_ids, text_id_in_clause, SQLITE_IN_CLAUSE_BATCH_SIZE,
};

fn app_user_select_columns() -> &'static str {
    "id, username, display_name, password_hash, role, status,
     created_at, updated_at, last_login_at"
}

fn app_user_lookup_sql(where_condition: &str) -> String {
    format!(
        "SELECT {columns}
         FROM app_users
         WHERE {where_condition}
         LIMIT 1",
        columns = app_user_select_columns(),
    )
}

fn app_user_by_username_sql() -> String {
    app_user_lookup_sql("lower(username) = lower(?1)")
}

fn app_user_by_id_sql() -> String {
    app_user_lookup_sql("id = ?1")
}

fn app_user_list_sql() -> String {
    format!(
        "SELECT {columns}
         FROM app_users
         ORDER BY created_at ASC, username ASC",
        columns = app_user_select_columns(),
    )
}
fn app_user_count_sql() -> &'static str {
    "SELECT COUNT(*) FROM app_users"
}

fn member_app_user_count_sql() -> &'static str {
    "SELECT COUNT(*) FROM app_users WHERE role = 'member'"
}

fn active_admin_count_sql() -> &'static str {
    "SELECT COUNT(*) FROM app_users WHERE role = 'admin' AND status = 'active'"
}

fn dashboard_app_user_summary_sql(where_condition: Option<&str>) -> String {
    let mut sql = "SELECT
            u.id,
            u.username,
            u.display_name,
            u.role,
            u.status,
            w.balance_credit_micros - w.frozen_credit_micros
         FROM app_users u
         LEFT JOIN app_wallets w
           ON w.owner_kind = 'user'
          AND w.owner_id = u.id"
        .to_string();
    if let Some(condition) = where_condition {
        sql.push_str(&format!("\n         WHERE {condition}"));
    } else {
        sql.push_str("\n         ORDER BY u.created_at ASC, u.username ASC");
    }
    sql
}

fn public_app_user_with_wallet_sql(where_condition: Option<&str>, include_order: bool) -> String {
    let mut sql = "SELECT
            u.id,
            u.username,
            u.display_name,
            u.role,
            u.status,
            u.created_at,
            u.updated_at,
            u.last_login_at,
            w.id,
            w.owner_kind,
            w.owner_id,
            w.balance_credit_micros,
            w.frozen_credit_micros,
            w.status,
            w.created_at,
            w.updated_at
         FROM app_users u
         LEFT JOIN app_wallets w
           ON u.role <> 'admin'
          AND w.owner_kind = 'user'
          AND w.owner_id = u.id"
        .to_string();
    if let Some(condition) = where_condition {
        sql.push_str(&format!("\n         WHERE {condition}"));
    }
    if include_order {
        sql.push_str("\n         ORDER BY u.created_at ASC, u.username ASC");
    }
    sql
}

fn app_user_access_summary_sql(where_condition: &str) -> String {
    format!(
        "SELECT id, username, role, status
         FROM app_users
         WHERE {where_condition}"
    )
}

fn app_user_access_summary_by_id_sql() -> String {
    format!(
        "{}\n         LIMIT 1",
        app_user_access_summary_sql("id = ?1")
    )
}

fn app_session_select_columns() -> &'static str {
    "id, user_id, token_hash, expires_at, created_at, last_seen_at, revoked_at"
}

fn active_app_session_by_token_hash_sql() -> String {
    format!(
        "SELECT {columns}
         FROM app_user_sessions
         WHERE token_hash = ?1 AND revoked_at IS NULL AND expires_at > ?2
         LIMIT 1",
        columns = app_session_select_columns(),
    )
}

fn active_app_session_user_with_wallet_sql() -> &'static str {
    "SELECT
        s.id,
        s.expires_at,
        u.id,
        u.username,
        u.display_name,
        u.role,
        u.status,
        u.created_at,
        u.updated_at,
        u.last_login_at,
        w.id,
        w.owner_kind,
        w.owner_id,
        w.balance_credit_micros,
        w.frozen_credit_micros,
        w.status,
        w.created_at,
        w.updated_at
     FROM app_user_sessions s
     INNER JOIN app_users u ON u.id = s.user_id
     LEFT JOIN app_wallets w
       ON u.role <> 'admin'
      AND w.owner_kind = 'user'
      AND w.owner_id = u.id
     WHERE s.token_hash = ?1
       AND s.revoked_at IS NULL
       AND s.expires_at > ?2
       AND u.status = 'active'
     LIMIT 1"
}

fn touch_app_user_session_sql() -> &'static str {
    "UPDATE app_user_sessions SET last_seen_at = ?1 WHERE id = ?2"
}

fn revoke_app_user_session_by_token_hash_sql() -> &'static str {
    "UPDATE app_user_sessions
     SET revoked_at = ?1
     WHERE token_hash = ?2 AND revoked_at IS NULL"
}

fn app_wallet_select_columns() -> &'static str {
    "id, owner_kind, owner_id, balance_credit_micros, frozen_credit_micros,
     status, created_at, updated_at"
}

fn app_wallet_by_owner_sql() -> String {
    format!(
        "SELECT {columns}
         FROM app_wallets
         WHERE owner_kind = ?1 AND owner_id = ?2
         LIMIT 1",
        columns = app_wallet_select_columns(),
    )
}

fn app_wallet_list_sql() -> String {
    format!(
        "SELECT {columns}
         FROM app_wallets
         ORDER BY created_at ASC",
        columns = app_wallet_select_columns(),
    )
}

fn api_key_owner_select_columns() -> &'static str {
    "key_id, owner_kind, owner_user_id, project_id, updated_at"
}

fn api_key_owner_lookup_sql() -> String {
    format!(
        "SELECT {columns}
         FROM api_key_owners
         WHERE key_id = ?1
         LIMIT 1",
        columns = api_key_owner_select_columns(),
    )
}

fn api_key_owner_chunk_sql(where_condition: &str) -> String {
    format!(
        "SELECT {columns}
         FROM api_key_owners
         WHERE {where_condition}",
        columns = api_key_owner_select_columns(),
    )
}

fn api_key_owner_list_sql() -> String {
    format!(
        "SELECT {columns}
         FROM api_key_owners",
        columns = api_key_owner_select_columns(),
    )
}

fn api_key_owner_rows_sql() -> String {
    format!(
        "SELECT {columns}
         FROM api_key_owners
         ORDER BY key_id ASC",
        columns = api_key_owner_select_columns(),
    )
}

fn user_wallets_for_users_chunk_sql(owner_condition: &str) -> String {
    format!(
        "SELECT {columns}
         FROM app_wallets
         WHERE owner_kind = 'user'
           AND {owner_condition}",
        columns = app_wallet_select_columns(),
    )
}

fn user_wallet_available_credit_sql() -> &'static str {
    "SELECT owner_id, balance_credit_micros - frozen_credit_micros
     FROM app_wallets
     WHERE owner_kind = 'user'"
}

fn nonzero_wallet_count_sql() -> &'static str {
    "SELECT COUNT(*)
     FROM app_wallets
     WHERE balance_credit_micros <> 0 OR frozen_credit_micros <> 0"
}

fn wallet_ledger_entry_count_sql() -> &'static str {
    "SELECT COUNT(*) FROM app_wallet_ledger_entries"
}

fn request_charge_ledger_entry_count_sql() -> &'static str {
    "SELECT COUNT(*) FROM app_wallet_ledger_entries WHERE entry_kind = 'request_charge'"
}

fn user_api_key_ids_for_user_sql() -> &'static str {
    "SELECT key_id
     FROM api_key_owners
     WHERE owner_kind = 'user' AND owner_user_id = ?1
     ORDER BY key_id ASC"
}

fn delete_api_key_owners_for_user_sql() -> &'static str {
    "DELETE FROM api_key_owners
     WHERE owner_kind = 'user' AND owner_user_id = ?1"
}

fn delete_app_user_sessions_for_user_sql() -> &'static str {
    "DELETE FROM app_user_sessions WHERE user_id = ?1"
}

fn delete_user_model_groups_for_user_sql() -> &'static str {
    "DELETE FROM user_model_groups WHERE user_id = ?1"
}

fn delete_app_wallet_ledger_entries_for_user_wallets_sql() -> &'static str {
    "DELETE FROM app_wallet_ledger_entries
     WHERE wallet_id IN (
        SELECT id FROM app_wallets WHERE owner_kind = 'user' AND owner_id = ?1
     )"
}

fn delete_app_wallets_for_user_sql() -> &'static str {
    "DELETE FROM app_wallets WHERE owner_kind = 'user' AND owner_id = ?1"
}

fn delete_app_user_by_id_sql() -> &'static str {
    "DELETE FROM app_users WHERE id = ?1"
}

fn app_user_exists_sql() -> &'static str {
    "SELECT EXISTS(SELECT 1 FROM app_users WHERE id = ?1)"
}

fn app_username_exists_sql() -> &'static str {
    "SELECT EXISTS(SELECT 1 FROM app_users WHERE lower(username) = lower(?1))"
}

fn update_app_user_last_login_sql() -> &'static str {
    "UPDATE app_users SET last_login_at = ?1, updated_at = ?1 WHERE id = ?2"
}

fn update_app_user_status_sql() -> &'static str {
    "UPDATE app_users SET status = ?1, updated_at = ?2 WHERE id = ?3"
}

fn update_app_user_role_sql() -> &'static str {
    "UPDATE app_users SET role = ?1, updated_at = ?2 WHERE id = ?3"
}

fn update_app_user_display_name_sql() -> &'static str {
    "UPDATE app_users SET display_name = ?1, updated_at = ?2 WHERE id = ?3"
}

fn update_app_user_password_hash_sql() -> &'static str {
    "UPDATE app_users SET password_hash = ?1, updated_at = ?2 WHERE id = ?3"
}

fn billing_rule_select_columns() -> &'static str {
    "id, name, status, priority, multiplier_millis, model_pattern, service_tier,
     user_id, project_id, api_key_id, starts_at, ends_at, created_at, updated_at"
}

fn billing_rule_list_sql() -> String {
    format!(
        "SELECT
            {columns}
         FROM billing_rules
         ORDER BY status ASC, priority DESC, updated_at DESC, name ASC",
        columns = billing_rule_select_columns(),
    )
}

fn active_billing_rule_where_sql() -> &'static str {
    "status = 'active'
     AND (starts_at IS NULL OR starts_at <= ?1)
     AND (ends_at IS NULL OR ends_at > ?1)"
}

fn active_billing_rules_sql() -> String {
    format!(
        "SELECT
            {columns}
         FROM billing_rules
         WHERE {where_clause}
         ORDER BY priority DESC, updated_at DESC, name ASC",
        columns = billing_rule_select_columns(),
        where_clause = active_billing_rule_where_sql(),
    )
}

fn delete_billing_rule_by_id_sql() -> &'static str {
    "DELETE FROM billing_rules WHERE id = ?1"
}

fn active_billing_rules_for_context_sql() -> String {
    format!(
        "SELECT
            {columns}
         FROM billing_rules
         WHERE {where_clause}
           AND (api_key_id IS NULL OR TRIM(api_key_id) = '' OR api_key_id = ?2)
           AND (user_id IS NULL OR TRIM(user_id) = '' OR user_id = ?3)
           AND (project_id IS NULL OR TRIM(project_id) = '' OR project_id = ?4)
           AND (
                service_tier IS NULL
                OR TRIM(service_tier) = ''
                OR LOWER(TRIM(service_tier)) = LOWER(TRIM(?5))
           )
         ORDER BY priority DESC, updated_at DESC, name ASC",
        columns = billing_rule_select_columns(),
        where_clause = active_billing_rule_where_sql(),
    )
}

fn active_billing_rules_for_request_candidate_sql() -> String {
    format!(
        "SELECT
            {columns}
         FROM billing_rules
         WHERE {where_clause}
           AND (api_key_id IS NULL OR TRIM(api_key_id) = '' OR api_key_id = ?2)
           AND (user_id IS NULL OR TRIM(user_id) = '' OR user_id = ?3)
           AND (project_id IS NULL OR TRIM(project_id) = '' OR project_id = ?4)
           AND (
                service_tier IS NULL
                OR TRIM(service_tier) = ''
                OR LOWER(TRIM(service_tier)) = LOWER(TRIM(?5))
           )
           AND (
                model_pattern IS NULL
                OR TRIM(model_pattern) = ''
                OR TRIM(model_pattern) = '*'
                OR (
                    ?6 <> ''
                    AND (
                        INSTR(model_pattern, '*') > 0
                        OR LOWER(TRIM(model_pattern)) = LOWER(TRIM(?6))
                        OR LOWER(TRIM(?6)) LIKE LOWER(TRIM(model_pattern)) || '%'
                    )
                )
           )
         ORDER BY
            priority DESC,
            (
                CASE WHEN api_key_id IS NOT NULL AND TRIM(api_key_id) <> '' THEN 1 ELSE 0 END
              + CASE WHEN user_id IS NOT NULL AND TRIM(user_id) <> '' THEN 1 ELSE 0 END
              + CASE WHEN project_id IS NOT NULL AND TRIM(project_id) <> '' THEN 1 ELSE 0 END
              + CASE WHEN service_tier IS NOT NULL AND TRIM(service_tier) <> '' THEN 1 ELSE 0 END
              + CASE WHEN model_pattern IS NOT NULL AND TRIM(model_pattern) <> '' THEN 1 ELSE 0 END
            ) DESC,
            LENGTH(IFNULL(model_pattern, '')) DESC,
            updated_at DESC",
        columns = billing_rule_select_columns(),
        where_clause = active_billing_rule_where_sql(),
    )
}

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

fn map_dashboard_app_user_summary(row: &Row<'_>) -> Result<DashboardAppUserSummary> {
    Ok(DashboardAppUserSummary {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        role: row.get(3)?,
        status: row.get(4)?,
        wallet_available_credit_micros: row.get(5)?,
    })
}

fn map_app_user_access_summary(row: &Row<'_>) -> Result<AppUserAccessSummary> {
    Ok(AppUserAccessSummary {
        id: row.get(0)?,
        username: row.get(1)?,
        role: row.get(2)?,
        status: row.get(3)?,
    })
}

fn map_public_app_user_with_wallet(row: &Row<'_>) -> Result<PublicAppUserWithWallet> {
    Ok(PublicAppUserWithWallet {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        role: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        last_login_at: row.get(7)?,
        wallet_id: row.get(8)?,
        wallet_owner_kind: row.get(9)?,
        wallet_owner_id: row.get(10)?,
        wallet_balance_credit_micros: row.get(11)?,
        wallet_frozen_credit_micros: row.get(12)?,
        wallet_status: row.get(13)?,
        wallet_created_at: row.get(14)?,
        wallet_updated_at: row.get(15)?,
    })
}

fn map_public_app_user_with_wallet_from_offset(
    row: &Row<'_>,
    offset: usize,
) -> Result<PublicAppUserWithWallet> {
    Ok(PublicAppUserWithWallet {
        id: row.get(offset)?,
        username: row.get(offset + 1)?,
        display_name: row.get(offset + 2)?,
        role: row.get(offset + 3)?,
        status: row.get(offset + 4)?,
        created_at: row.get(offset + 5)?,
        updated_at: row.get(offset + 6)?,
        last_login_at: row.get(offset + 7)?,
        wallet_id: row.get(offset + 8)?,
        wallet_owner_kind: row.get(offset + 9)?,
        wallet_owner_id: row.get(offset + 10)?,
        wallet_balance_credit_micros: row.get(offset + 11)?,
        wallet_frozen_credit_micros: row.get(offset + 12)?,
        wallet_status: row.get(offset + 13)?,
        wallet_created_at: row.get(offset + 14)?,
        wallet_updated_at: row.get(offset + 15)?,
    })
}

fn map_app_session_user_with_wallet(row: &Row<'_>) -> Result<AppSessionUserWithWallet> {
    Ok(AppSessionUserWithWallet {
        session_id: row.get(0)?,
        expires_at: row.get(1)?,
        user: map_public_app_user_with_wallet_from_offset(row, 2)?,
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
            .query_row(app_user_count_sql(), [], |row| row.get(0))
    }

    pub fn member_app_user_count(&self) -> Result<i64> {
        self.conn
            .query_row(member_app_user_count_sql(), [], |row| row.get(0))
    }

    pub fn active_admin_count(&self) -> Result<i64> {
        self.conn
            .query_row(active_admin_count_sql(), [], |row| row.get(0))
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
        tx.execute(delete_api_key_owners_for_user_sql(), [user_id])?;
        tx.execute(delete_app_user_sessions_for_user_sql(), [user_id])?;
        tx.execute(delete_user_model_groups_for_user_sql(), [user_id])?;
        tx.execute(
            delete_app_wallet_ledger_entries_for_user_wallets_sql(),
            [user_id],
        )?;
        tx.execute(delete_app_wallets_for_user_sql(), [user_id])?;
        let deleted = tx.execute(delete_app_user_by_id_sql(), [user_id])?;
        tx.commit()?;
        Ok(deleted)
    }

    pub fn list_app_users(&self) -> Result<Vec<AppUser>> {
        let mut stmt = self.conn.prepare(&app_user_list_sql())?;
        let rows = stmt.query_map([], map_app_user)?;
        rows.collect()
    }

    pub fn list_dashboard_app_user_summaries(&self) -> Result<Vec<DashboardAppUserSummary>> {
        let mut stmt = self.conn.prepare(&dashboard_app_user_summary_sql(None))?;
        let rows = stmt.query_map([], map_dashboard_app_user_summary)?;
        rows.collect()
    }

    pub fn list_dashboard_app_user_summaries_for_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<DashboardAppUserSummary>> {
        let user_ids = normalize_text_ids(user_ids);
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut users = Vec::new();
        for chunk in user_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            let Some((condition, params)) = text_id_in_clause("u.id", chunk) else {
                continue;
            };
            let sql = dashboard_app_user_summary_sql(Some(&condition));
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(params), map_dashboard_app_user_summary)?;
            for row in rows {
                users.push(row?);
            }
        }
        users.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(users)
    }

    pub fn list_public_app_users_with_wallets(&self) -> Result<Vec<PublicAppUserWithWallet>> {
        let mut stmt = self
            .conn
            .prepare(&public_app_user_with_wallet_sql(None, true))?;
        let rows = stmt.query_map([], map_public_app_user_with_wallet)?;
        rows.collect()
    }

    pub fn find_public_app_user_with_wallet_by_id(
        &self,
        id: &str,
    ) -> Result<Option<PublicAppUserWithWallet>> {
        self.conn
            .query_row(
                &format!(
                    "{}\n         LIMIT 1",
                    public_app_user_with_wallet_sql(Some("u.id = ?1"), false)
                ),
                [id],
                map_public_app_user_with_wallet,
            )
            .optional()
    }

    pub fn find_app_user_by_username(&self, username: &str) -> Result<Option<AppUser>> {
        self.conn
            .query_row(&app_user_by_username_sql(), [username], map_app_user)
            .optional()
    }

    pub fn find_app_user_by_id(&self, id: &str) -> Result<Option<AppUser>> {
        self.conn
            .query_row(&app_user_by_id_sql(), [id], map_app_user)
            .optional()
    }

    pub fn list_app_user_access_summaries_for_ids(
        &self,
        user_ids: &[String],
    ) -> Result<Vec<AppUserAccessSummary>> {
        let user_ids = normalize_text_ids(user_ids);
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut users = Vec::new();
        for chunk in user_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            let Some((condition, params)) = text_id_in_clause("id", chunk) else {
                continue;
            };
            let sql = app_user_access_summary_sql(&condition);
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(params), map_app_user_access_summary)?;
            for row in rows {
                users.push(row?);
            }
        }
        users.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(users)
    }

    pub fn find_app_user_access_summary_by_id(
        &self,
        id: &str,
    ) -> Result<Option<AppUserAccessSummary>> {
        self.conn
            .query_row(
                &app_user_access_summary_by_id_sql(),
                [id],
                map_app_user_access_summary,
            )
            .optional()
    }

    pub fn app_user_exists(&self, id: &str) -> Result<bool> {
        self.conn
            .query_row(app_user_exists_sql(), [id], |row| row.get(0))
    }

    pub fn app_username_exists(&self, username: &str) -> Result<bool> {
        self.conn
            .query_row(app_username_exists_sql(), [username], |row| row.get(0))
    }

    pub fn update_app_user_last_login(&self, id: &str, ts: i64) -> Result<()> {
        self.conn
            .execute(update_app_user_last_login_sql(), (ts, id))?;
        Ok(())
    }

    pub fn update_app_user_status(&self, id: &str, status: &str) -> Result<()> {
        self.conn
            .execute(update_app_user_status_sql(), (status, now_ts(), id))?;
        Ok(())
    }

    pub fn update_app_user_role(&self, id: &str, role: &str) -> Result<()> {
        self.conn
            .execute(update_app_user_role_sql(), (role, now_ts(), id))?;
        Ok(())
    }

    pub fn update_app_user_display_name(
        &self,
        id: &str,
        display_name: Option<String>,
    ) -> Result<()> {
        self.conn.execute(
            update_app_user_display_name_sql(),
            (display_name, now_ts(), id),
        )?;
        Ok(())
    }

    pub fn update_app_user_password_hash(&self, id: &str, password_hash: &str) -> Result<()> {
        self.conn.execute(
            update_app_user_password_hash_sql(),
            (password_hash, now_ts(), id),
        )?;
        Ok(())
    }

    pub fn update_app_user_password_hash_if_current(
        &self,
        id: &str,
        expected_password_hash: &str,
        password_hash: &str,
    ) -> Result<bool> {
        let updated = self.conn.execute(
            "UPDATE app_users
             SET password_hash = ?1, updated_at = ?2
             WHERE id = ?3 AND password_hash = ?4",
            (password_hash, now_ts(), id, expected_password_hash),
        )?;
        Ok(updated == 1)
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
                &active_app_session_by_token_hash_sql(),
                (token_hash, now),
                map_app_session,
            )
            .optional()
    }

    pub fn find_active_app_session_user_by_token_hash(
        &self,
        token_hash: &str,
        now: i64,
    ) -> Result<Option<AppSessionUserWithWallet>> {
        self.conn
            .query_row(
                active_app_session_user_with_wallet_sql(),
                (token_hash, now),
                map_app_session_user_with_wallet,
            )
            .optional()
    }

    pub fn touch_app_user_session(&self, session_id: &str, ts: i64) -> Result<()> {
        self.conn
            .execute(touch_app_user_session_sql(), (ts, session_id))?;
        Ok(())
    }

    pub fn revoke_app_user_session_by_token_hash(&self, token_hash: &str, ts: i64) -> Result<()> {
        self.conn.execute(
            revoke_app_user_session_by_token_hash_sql(),
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
                &app_wallet_by_owner_sql(),
                (owner_kind, owner_id),
                map_app_wallet,
            )
            .optional()
    }

    pub fn user_wallets_for_users(&self, user_ids: &[String]) -> Result<Vec<AppWallet>> {
        let user_ids = normalize_text_ids(user_ids);
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut wallets = Vec::new();
        for chunk in user_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            let Some((condition, params)) = text_id_in_clause("owner_id", chunk) else {
                continue;
            };
            let sql = user_wallets_for_users_chunk_sql(&condition);
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(params), map_app_wallet)?;
            for row in rows {
                wallets.push(row?);
            }
        }
        Ok(wallets)
    }

    pub fn list_wallets(&self) -> Result<Vec<AppWallet>> {
        let mut stmt = self.conn.prepare(&app_wallet_list_sql())?;
        let rows = stmt.query_map([], map_app_wallet)?;
        rows.collect()
    }

    pub fn user_wallet_available_credit_micros(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(user_wallet_available_credit_sql())?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }

    pub fn nonzero_wallet_count(&self) -> Result<i64> {
        self.conn
            .query_row(nonzero_wallet_count_sql(), [], |row| row.get(0))
    }

    pub fn wallet_ledger_entry_count(&self) -> Result<i64> {
        self.conn
            .query_row(wallet_ledger_entry_count_sql(), [], |row| row.get(0))
    }

    pub fn request_charge_ledger_entry_count(&self) -> Result<i64> {
        self.conn
            .query_row(request_charge_ledger_entry_count_sql(), [], |row| {
                row.get(0)
            })
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
            .query_row(&api_key_owner_lookup_sql(), [key_id], map_api_key_owner)
            .optional()
    }

    pub fn list_api_key_owners_for_ids(&self, key_ids: &[String]) -> Result<Vec<ApiKeyOwner>> {
        let key_ids = normalize_text_ids(key_ids);
        if key_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut owners = Vec::new();
        for chunk in key_ids.chunks(SQLITE_IN_CLAUSE_BATCH_SIZE) {
            let Some((condition, params)) = text_id_in_clause("key_id", chunk) else {
                continue;
            };
            let sql = api_key_owner_chunk_sql(&condition);
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(params), map_api_key_owner)?;
            for row in rows {
                owners.push(row?);
            }
        }
        owners.sort_by(|left, right| left.key_id.cmp(&right.key_id));
        Ok(owners)
    }

    pub fn list_api_key_owners(&self) -> Result<HashMap<String, ApiKeyOwner>> {
        let mut stmt = self.conn.prepare(&api_key_owner_list_sql())?;
        let rows = stmt.query_map([], map_api_key_owner)?;
        let mut out = HashMap::new();
        for row in rows {
            let owner = row?;
            out.insert(owner.key_id.clone(), owner);
        }
        Ok(out)
    }

    pub fn list_api_key_owner_rows(&self) -> Result<Vec<ApiKeyOwner>> {
        let mut stmt = self.conn.prepare(&api_key_owner_rows_sql())?;
        let rows = stmt.query_map([], map_api_key_owner)?;
        rows.collect()
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
        let mut stmt = self.conn.prepare(user_api_key_ids_for_user_sql())?;
        let rows = stmt.query_map([normalized_user_id], |row| row.get(0))?;
        rows.collect()
    }

    pub fn api_key_owner_count(&self) -> Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM api_key_owners", [], |row| row.get(0))
    }

    pub fn list_billing_rules(&self) -> Result<Vec<BillingRule>> {
        let mut stmt = self.conn.prepare(&billing_rule_list_sql())?;
        let rows = stmt.query_map([], map_billing_rule)?;
        rows.collect()
    }

    pub fn list_active_billing_rules(&self, now: i64) -> Result<Vec<BillingRule>> {
        let mut stmt = self.conn.prepare(&active_billing_rules_sql())?;
        let rows = stmt.query_map([now], map_billing_rule)?;
        rows.collect()
    }

    pub fn list_active_billing_rules_for_context(
        &self,
        now: i64,
        key_id: &str,
        user_id: Option<&str>,
        project_id: Option<&str>,
        service_tier: Option<&str>,
    ) -> Result<Vec<BillingRule>> {
        let normalized_key_id = key_id.trim();
        if normalized_key_id.is_empty() {
            return Ok(Vec::new());
        }
        let normalized_user_id = normalize_optional_context_text(user_id);
        let normalized_project_id = normalize_optional_context_text(project_id);
        let normalized_service_tier = normalize_optional_context_text(service_tier);
        let mut stmt = self.conn.prepare(&active_billing_rules_for_context_sql())?;
        let rows = stmt.query_map(
            (
                now,
                normalized_key_id,
                normalized_user_id.as_deref().unwrap_or(""),
                normalized_project_id.as_deref().unwrap_or(""),
                normalized_service_tier.as_deref().unwrap_or(""),
            ),
            map_billing_rule,
        )?;
        rows.collect()
    }

    pub fn list_active_billing_rules_for_request_candidate(
        &self,
        now: i64,
        key_id: &str,
        user_id: Option<&str>,
        project_id: Option<&str>,
        service_tier: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<BillingRule>> {
        let normalized_key_id = key_id.trim();
        if normalized_key_id.is_empty() {
            return Ok(Vec::new());
        }
        let normalized_user_id = normalize_optional_context_text(user_id);
        let normalized_project_id = normalize_optional_context_text(project_id);
        let normalized_service_tier = normalize_optional_context_text(service_tier);
        let normalized_model = normalize_optional_context_text(model)
            .filter(|value| !value.eq_ignore_ascii_case("unknown"));
        let mut stmt = self
            .conn
            .prepare(&active_billing_rules_for_request_candidate_sql())?;
        let rows = stmt.query_map(
            (
                now,
                normalized_key_id,
                normalized_user_id.as_deref().unwrap_or(""),
                normalized_project_id.as_deref().unwrap_or(""),
                normalized_service_tier.as_deref().unwrap_or(""),
                normalized_model.as_deref().unwrap_or(""),
            ),
            map_billing_rule,
        )?;
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
        self.conn.execute(delete_billing_rule_by_id_sql(), [id])?;
        Ok(())
    }

    pub(super) fn ensure_account_manager_tables(&self) -> Result<()> {
        self.conn
            .execute_batch(include_str!("../../migrations/057_account_manager.sql"))?;
        self.conn.execute_batch(include_str!(
            "../../migrations/102_app_users_list_order_index.sql"
        ))?;
        self.conn.execute_batch(include_str!(
            "../../migrations/103_app_project_user_lookup_indexes.sql"
        ))?;
        self.conn.execute_batch(include_str!(
            "../../migrations/104_billing_rules_owner_lookup_indexes.sql"
        ))?;
        self.conn.execute_batch(include_str!(
            "../../migrations/105_redeem_records_lookup_indexes.sql"
        ))?;
        self.conn.execute_batch(include_str!(
            "../../migrations/106_account_manager_created_by_lookup_indexes.sql"
        ))?;
        Ok(())
    }
}

fn normalize_optional_context_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
#[path = "account_manager_tests.rs"]
mod tests;
