// 中文注释：accounts.rs 保留业务流程；这里集中维护基础 accounts 表 SQL 片段。
pub(super) fn account_count_sql() -> &'static str {
    "SELECT COUNT(1) FROM accounts"
}

pub(super) fn account_count_filtered_sql(where_clause: &str) -> String {
    format!("SELECT COUNT(1) FROM accounts{where_clause}")
}

pub(super) fn max_account_sort_sql() -> &'static str {
    "SELECT MAX(sort) FROM accounts"
}

pub(super) fn account_status_counts_sql() -> &'static str {
    "SELECT LOWER(TRIM(COALESCE(status, ''))), COUNT(1)
     FROM accounts
     GROUP BY LOWER(TRIM(COALESCE(status, '')))
     ORDER BY COUNT(1) DESC, LOWER(TRIM(COALESCE(status, ''))) ASC"
}

pub(super) fn account_direct_auth_profile_by_id_sql() -> &'static str {
    "SELECT id, issuer, chatgpt_account_id, status
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn account_by_id_sql() -> &'static str {
    "SELECT id, label, issuer, chatgpt_account_id, workspace_id, group_name, sort, status, created_at, updated_at
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn account_status_by_id_sql() -> &'static str {
    "SELECT status
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn account_workspace_identity_by_id_sql() -> &'static str {
    "SELECT id, chatgpt_account_id, workspace_id
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn account_upsert_state_by_id_sql() -> &'static str {
    "SELECT group_name, sort, created_at
     FROM accounts
     WHERE id = ?1
     LIMIT 1"
}

pub(super) fn account_exists_sql() -> &'static str {
    "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = ?1)"
}

pub(super) fn preferred_account_id_sql() -> &'static str {
    "SELECT id
     FROM accounts
     WHERE preferred = 1
     ORDER BY updated_at DESC, id ASC
     LIMIT 1"
}

pub(super) fn update_account_sort_sql() -> &'static str {
    "UPDATE accounts SET sort = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_account_label_sql() -> &'static str {
    "UPDATE accounts SET label = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_account_group_name_sql() -> &'static str {
    "UPDATE accounts SET group_name = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_account_workspace_identity_sql() -> &'static str {
    "UPDATE accounts
     SET chatgpt_account_id = ?1,
         workspace_id = ?2,
         updated_at = ?3
     WHERE id = ?4"
}

pub(super) fn touch_account_updated_at_sql() -> &'static str {
    "UPDATE accounts SET updated_at = ?1 WHERE id = ?2"
}

pub(super) fn update_account_status_sql() -> &'static str {
    "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3"
}

pub(super) fn update_account_status_if_changed_sql() -> &'static str {
    "UPDATE accounts SET status = ?1, updated_at = ?2 WHERE id = ?3 AND status != ?1"
}

pub(super) fn update_account_status_if_context_matches_sql() -> &'static str {
    "UPDATE accounts
     SET status = ?1, updated_at = ?2
     WHERE id = ?3 AND status = ?4 AND updated_at = ?5"
}

pub(super) fn delete_account_by_id_sql() -> &'static str {
    "DELETE FROM accounts WHERE id = ?1"
}

pub(super) fn clear_preferred_accounts_sql() -> &'static str {
    "UPDATE accounts SET preferred = 0 WHERE preferred != 0"
}

pub(super) fn set_preferred_account_sql() -> &'static str {
    "UPDATE accounts
     SET preferred = 1, updated_at = ?1
     WHERE id = ?2"
}

pub(super) fn clear_preferred_account_by_id_sql() -> &'static str {
    "UPDATE accounts SET preferred = 0, updated_at = ?1 WHERE id = ?2 AND preferred = 1"
}

pub(super) fn account_ids_list_sql() -> &'static str {
    "SELECT id FROM accounts ORDER BY sort ASC, updated_at DESC, id ASC"
}

pub(super) fn account_auth_refresh_targets_list_sql() -> &'static str {
    "SELECT id, label, issuer
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}

pub(super) fn account_quota_source_summaries_list_sql() -> &'static str {
    "SELECT id, label, status
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}

pub(super) fn account_import_snapshots_list_sql() -> &'static str {
    "SELECT id, label, issuer, chatgpt_account_id, workspace_id, sort, created_at
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}

pub(super) fn account_summary_rows_list_sql() -> &'static str {
    "SELECT id, label, group_name, sort, status
     FROM accounts
     ORDER BY sort ASC, updated_at DESC, id ASC"
}
