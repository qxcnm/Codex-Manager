use super::*;
use crate::storage::{
    ConversationBinding, Event, ModelSourceMapping, ModelSourceModel, UsageSnapshotRecord,
};

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

fn dependent_row_count(storage: &Storage, table: &str, column: &str, account_id: &str) -> i64 {
    let sql = format!("SELECT COUNT(1) FROM {table} WHERE {column} = ?1");
    storage
        .conn
        .query_row(&sql, [account_id], |row| row.get(0))
        .expect("count dependent rows")
}

fn model_source_account_row_count(storage: &Storage, table: &str, account_id: &str) -> i64 {
    let sql = format!(
        "SELECT COUNT(1) FROM {table} WHERE source_kind = 'openai_account' AND source_id = ?1"
    );
    storage
        .conn
        .query_row(&sql, [account_id], |row| row.get(0))
        .expect("count model source account rows")
}

fn collect_query_plan(storage: &Storage, sql: &str) -> String {
    collect_query_plan_with_params(storage, sql, [])
}

fn collect_query_plan_with_params<P>(storage: &Storage, sql: &str, params: P) -> String
where
    P: rusqlite::Params,
{
    let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
    let rows = stmt
        .query_map(params, |row| row.get::<_, String>(3))
        .expect("query explain");
    rows.collect::<Result<Vec<_>>>()
        .expect("collect explain")
        .join("\n")
}

#[test]
fn insert_account_update_preserves_existing_token() {
    let mut storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-upsert", "active", now);
    account.chatgpt_account_id = Some("cgpt-old".to_string());
    account.group_name = Some("team-a".to_string());
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
    assert_eq!(found.group_name.as_deref(), Some("team-a"));
    assert_eq!(found.created_at, now);
    assert_eq!(found.updated_at, now.saturating_add(1));
    assert_eq!(
        storage.preferred_account_id().expect("preferred account"),
        Some(account.id.clone())
    );

    let plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", account_by_id_sql()),
        rusqlite::params![account.id.as_str()],
    );
    assert!(
        plan.contains("sqlite_autoindex_accounts_1"),
        "expected account lookup to use account primary-key index, got {plan}"
    );

    let token = storage
        .find_token_by_account_id(account.id.as_str())
        .expect("find token")
        .expect("token still exists");
    assert_eq!(token.access_token, "access");
    assert_eq!(token.refresh_token, "refresh");
}

#[test]
fn update_account_group_name_sets_and_clears_group() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = sample_account("acc-group-update", "active", now_ts());
    storage.insert_account(&account).expect("insert account");

    storage
        .update_account_group_name(&account.id, Some("TEAM_A"))
        .expect("set account group");
    let grouped = storage
        .find_account_by_id(&account.id)
        .expect("find grouped account")
        .expect("grouped account exists");
    assert_eq!(grouped.group_name.as_deref(), Some("TEAM_A"));

    storage
        .update_account_group_name(&account.id, None)
        .expect("clear account group");
    let cleared = storage
        .find_account_by_id(&account.id)
        .expect("find cleared account")
        .expect("cleared account exists");
    assert_eq!(cleared.group_name, None);
}

#[test]
fn upsert_imported_account_bundle_merges_metadata_and_token_in_one_call() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-import-bundle", "active", now);
    account.label = "Original".to_string();
    storage.insert_account(&account).expect("insert account");
    storage
        .upsert_account_metadata(&account.id, Some("keep note"), Some("old tag"))
        .expect("insert metadata");

    let mut updated = account.clone();
    updated.label = "Imported".to_string();
    updated.updated_at = now.saturating_add(10);
    let mut token = sample_token(&updated.id, now.saturating_add(10));
    token.access_token = "imported-access".to_string();
    token.refresh_token = "imported-refresh".to_string();

    storage
        .upsert_imported_account_bundle(&updated, None, Some("new tag"), &token, None)
        .expect("upsert imported bundle");

    let found = storage
        .find_account_by_id(&updated.id)
        .expect("find account")
        .expect("account exists");
    assert_eq!(found.label, "Imported");
    let metadata = storage
        .find_account_metadata(&updated.id)
        .expect("find metadata")
        .expect("metadata exists");
    assert_eq!(metadata.note.as_deref(), Some("keep note"));
    assert_eq!(metadata.tags.as_deref(), Some("new tag"));
    let found_token = storage
        .find_token_by_account_id(&updated.id)
        .expect("find token")
        .expect("token exists");
    assert_eq!(found_token.access_token, "imported-access");
    assert_eq!(found_token.refresh_token, "imported-refresh");

    let raw_access: String = storage
        .conn
        .query_row(
            "SELECT access_token FROM tokens WHERE account_id = ?1",
            [&updated.id],
            |row| row.get(0),
        )
        .expect("read raw imported token");
    assert!(raw_access.starts_with("cmenc:v1:"));
    assert!(!raw_access.contains("imported-access"));
}

#[test]
fn upsert_imported_account_bundle_rejects_mismatched_token_without_writing_account() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let account = sample_account("acc-import-mismatch", "active", now);
    let token = sample_token("acc-import-other", now);

    assert!(storage
        .upsert_imported_account_bundle(&account, Some("note"), Some("tag"), &token, None)
        .is_err());
    assert!(storage
        .find_account_by_id(&account.id)
        .expect("find account")
        .is_none());
    assert_eq!(storage.token_count().expect("token count"), 0);
}

#[test]
fn account_count_reads_account_cardinality_sql() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    assert_eq!(storage.account_count().expect("empty count"), 0);
    storage
        .insert_account(&sample_account("acc-count-first", "active", now))
        .expect("insert first account");
    storage
        .insert_account(&sample_account("acc-count-second", "disabled", now))
        .expect("insert second account");

    assert_eq!(storage.account_count().expect("account count"), 2);

    let plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", account_count_sql()),
    );
    assert!(
        plan.contains("SCAN accounts") || plan.contains("USING COVERING INDEX"),
        "expected account count to stay a direct accounts cardinality scan, got {plan}"
    );
}

#[test]
fn max_account_sort_reads_largest_sort_without_loading_accounts() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    assert_eq!(storage.max_account_sort().expect("max empty sort"), None);

    let mut low = sample_account("acc-low-sort", "active", now);
    low.sort = 2;
    let mut high = sample_account("acc-high-sort", "active", now);
    high.sort = 11;
    storage.insert_account(&low).expect("insert low sort");
    storage.insert_account(&high).expect("insert high sort");

    assert_eq!(storage.max_account_sort().expect("max sort"), Some(11));

    let plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", max_account_sort_sql()),
    );
    assert!(
        plan.contains("USING COVERING INDEX")
            && (plan.contains("idx_accounts_sort_updated_at")
                || plan.contains("idx_accounts_list_order")),
        "expected max sort lookup to use an account sort covering index, got {plan}"
    );
}

#[test]
fn account_quota_overview_stats_aggregates_latest_usage_in_sql() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    assert_eq!(
        storage
            .account_quota_overview_stats()
            .expect("empty stats")
            .account_count,
        0
    );

    for (account_id, status) in [
        ("acc-active-low", "active"),
        ("acc-available-ok", "available"),
        ("acc-disabled-low", "disabled"),
    ] {
        storage
            .insert_account(&sample_account(account_id, status, now))
            .expect("insert account");
    }

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-active-low".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(10.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now - 60,
        })
        .expect("insert old active usage");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-active-low".to_string(),
            used_percent: Some(90.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(10.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert latest active usage");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-available-ok".to_string(),
            used_percent: Some(20.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(40.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now + 10,
        })
        .expect("insert available usage");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-disabled-low".to_string(),
            used_percent: Some(99.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now + 20,
        })
        .expect("insert disabled usage");

    let stats = storage
        .account_quota_overview_stats()
        .expect("quota overview stats");

    assert_eq!(stats.account_count, 3);
    assert_eq!(stats.available_count, 2);
    assert_eq!(stats.low_quota_count, 1);
    assert_eq!(stats.primary_remain_percent_avg, Some(45.0));
    assert_eq!(stats.secondary_remain_percent_avg, Some(75.0));
    assert_eq!(stats.last_refreshed_at, Some(now + 10));

    let plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", account_quota_overview_stats_sql()),
    );
    assert!(
        plan.contains("SCAN a") || plan.contains("SCAN accounts"),
        "expected quota overview to read account cardinality directly, got {plan}"
    );
    assert!(
        plan.contains("idx_usage_snapshots_account_captured_id"),
        "expected quota overview latest usage CTE to use usage snapshot account/captured index, got {plan}"
    );
}

#[test]
fn list_accounts_for_ids_filters_and_preserves_account_order() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first", "active", now);
    first.sort = 1;
    first.updated_at = now;
    let mut second = sample_account("acc-second", "active", now);
    second.sort = 0;
    second.updated_at = now.saturating_sub(10);
    let mut ignored = sample_account("acc-ignored", "active", now);
    ignored.sort = -1;

    for account in [&first, &second, &ignored] {
        storage.insert_account(account).expect("insert account");
    }

    let requested = vec![
        "acc-first".to_string(),
        "acc-missing".to_string(),
        "acc-second".to_string(),
        "acc-first".to_string(),
    ];
    let accounts = storage
        .list_accounts_for_ids(&requested)
        .expect("list accounts for ids");

    assert_eq!(
        accounts
            .into_iter()
            .map(|account| account.id)
            .collect::<Vec<_>>(),
        vec!["acc-second".to_string(), "acc-first".to_string()]
    );
}

#[test]
fn list_account_dashboard_source_metadata_for_ids_reads_dashboard_fields_only() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-dashboard-first", "active", now);
    first.label = "First Dashboard".to_string();
    first.issuer = "ignored-issuer".to_string();
    first.chatgpt_account_id = Some("ignored-chatgpt".to_string());
    first.workspace_id = Some("ignored-workspace".to_string());
    first.sort = 1;
    first.updated_at = now;
    let mut second = sample_account("acc-dashboard-second", "disabled", now);
    second.label = "Second Dashboard".to_string();
    second.sort = 0;
    second.updated_at = now.saturating_sub(10);
    let mut ignored = sample_account("acc-dashboard-ignored", "active", now);
    ignored.label = "Ignored Dashboard".to_string();
    ignored.sort = -1;

    for account in [&first, &second, &ignored] {
        storage.insert_account(account).expect("insert account");
    }

    let metadata = storage
        .list_account_dashboard_source_metadata_for_ids(&[
            "acc-dashboard-first".to_string(),
            "acc-dashboard-missing".to_string(),
            "acc-dashboard-second".to_string(),
            "acc-dashboard-first".to_string(),
        ])
        .expect("list dashboard account metadata");

    assert_eq!(metadata.len(), 2);
    assert_eq!(metadata[0].id, "acc-dashboard-second");
    assert_eq!(metadata[0].label, "Second Dashboard");
    assert_eq!(metadata[0].status, "disabled");
    assert_eq!(metadata[1].id, "acc-dashboard-first");
    assert_eq!(metadata[1].label, "First Dashboard");
    assert_eq!(metadata[1].status, "active");
}

#[test]
fn account_status_and_exists_helpers_read_minimal_account_state() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-status-helper", " Limited ", now);
    account.label = "ignored label".to_string();
    account.issuer = "ignored issuer".to_string();
    account.chatgpt_account_id = Some("ignored-chatgpt".to_string());
    account.workspace_id = Some("ignored-workspace".to_string());
    storage.insert_account(&account).expect("insert account");

    assert_eq!(
        storage
            .find_account_status_by_id("acc-status-helper")
            .expect("find account status")
            .as_deref(),
        Some(" Limited ")
    );
    assert_eq!(
        storage
            .find_account_status_by_id("acc-status-missing")
            .expect("find missing account status"),
        None
    );
    assert!(storage
        .account_exists("acc-status-helper")
        .expect("account exists"));
    assert!(!storage
        .account_exists("acc-status-missing")
        .expect("missing account exists"));

    let plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", account_status_by_id_sql()),
        rusqlite::params!["acc-status-helper"],
    );
    assert!(
        plan.contains("sqlite_autoindex_accounts_1"),
        "expected account status lookup to use account primary-key index, got {plan}"
    );
}

#[test]
fn find_account_workspace_identity_by_id_reads_scope_fields_only() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-workspace-helper", "active", now);
    account.label = "ignored label".to_string();
    account.issuer = "ignored issuer".to_string();
    account.chatgpt_account_id = Some("chatgpt-workspace-helper".to_string());
    account.workspace_id = Some("workspace-helper".to_string());
    account.group_name = Some("ignored group".to_string());
    storage.insert_account(&account).expect("insert account");

    let identity = storage
        .find_account_workspace_identity_by_id("acc-workspace-helper")
        .expect("find workspace identity")
        .expect("identity exists");

    assert_eq!(identity.id, "acc-workspace-helper");
    assert_eq!(
        identity.chatgpt_account_id.as_deref(),
        Some("chatgpt-workspace-helper")
    );
    assert_eq!(identity.workspace_id.as_deref(), Some("workspace-helper"));
    assert!(storage
        .find_account_workspace_identity_by_id("acc-workspace-missing")
        .expect("find missing workspace identity")
        .is_none());

    let plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            account_workspace_identity_by_id_sql()
        ),
        rusqlite::params!["acc-workspace-helper"],
    );
    assert!(
        plan.contains("sqlite_autoindex_accounts_1"),
        "expected workspace identity lookup to use account primary-key index, got {plan}"
    );
}

#[test]
fn find_account_upsert_state_by_id_reads_upsert_fields_only() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-upsert-state", "limited", now);
    account.label = "ignored label".to_string();
    account.issuer = "ignored issuer".to_string();
    account.chatgpt_account_id = Some("ignored-chatgpt".to_string());
    account.workspace_id = Some("ignored-workspace".to_string());
    account.group_name = Some("keep group".to_string());
    account.sort = 37;
    account.created_at = now.saturating_sub(20);
    storage.insert_account(&account).expect("insert account");

    let state = storage
        .find_account_upsert_state_by_id("acc-upsert-state")
        .expect("find upsert state")
        .expect("state exists");

    assert_eq!(state.group_name.as_deref(), Some("keep group"));
    assert_eq!(state.sort, 37);
    assert_eq!(state.created_at, now.saturating_sub(20));
    assert!(storage
        .find_account_upsert_state_by_id("acc-upsert-missing")
        .expect("find missing upsert state")
        .is_none());

    let plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", account_upsert_state_by_id_sql()),
        rusqlite::params!["acc-upsert-state"],
    );
    assert!(
        plan.contains("sqlite_autoindex_accounts_1"),
        "expected upsert state lookup to use account primary-key index, got {plan}"
    );
}

#[test]
fn update_account_workspace_identity_only_updates_identity_columns() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-identity-update", "limited", now);
    account.label = "original label".to_string();
    account.issuer = "original issuer".to_string();
    account.chatgpt_account_id = Some("old-chatgpt".to_string());
    account.workspace_id = Some("old-workspace".to_string());
    account.group_name = Some("original group".to_string());
    account.sort = 42;
    storage.insert_account(&account).expect("insert account");

    let changed = storage
        .update_account_workspace_identity(
            "acc-identity-update",
            Some("new-chatgpt"),
            Some("new-workspace"),
            now.saturating_add(5),
        )
        .expect("update identity");

    assert!(changed);
    let updated = storage
        .find_account_by_id("acc-identity-update")
        .expect("find account")
        .expect("account exists");
    assert_eq!(updated.label, "original label");
    assert_eq!(updated.issuer, "original issuer");
    assert_eq!(updated.chatgpt_account_id.as_deref(), Some("new-chatgpt"));
    assert_eq!(updated.workspace_id.as_deref(), Some("new-workspace"));
    assert_eq!(updated.group_name.as_deref(), Some("original group"));
    assert_eq!(updated.sort, 42);
    assert_eq!(updated.status, "limited");
    assert_eq!(updated.created_at, now);
    assert_eq!(updated.updated_at, now.saturating_add(5));
    assert!(!storage
        .update_account_workspace_identity(
            "acc-identity-missing",
            Some("ignored-chatgpt"),
            Some("ignored-workspace"),
            now.saturating_add(6),
        )
        .expect("update missing identity"));
}

#[test]
fn list_account_ids_for_ids_filters_and_reads_only_ids_in_account_order() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-id", "active", now);
    first.sort = 1;
    first.updated_at = now;
    let mut second = sample_account("acc-second-id", "active", now);
    second.sort = 0;
    second.updated_at = now.saturating_sub(10);
    let mut ignored = sample_account("acc-ignored-id", "active", now);
    ignored.sort = -1;

    for account in [&first, &second, &ignored] {
        storage.insert_account(account).expect("insert account");
    }

    let requested = vec![
        "acc-first-id".to_string(),
        "acc-missing-id".to_string(),
        "acc-second-id".to_string(),
        "acc-first-id".to_string(),
    ];
    assert_eq!(
        storage
            .list_account_ids_for_ids(&requested)
            .expect("list account ids for ids"),
        vec!["acc-second-id".to_string(), "acc-first-id".to_string()]
    );
}

#[test]
fn list_account_token_refresh_issuers_for_ids_reads_only_issuer_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-refresh-first", "active", now);
    first.issuer = "https://issuer.first".to_string();
    first.label = "ignored label".to_string();
    first.chatgpt_account_id = Some("ignored-chatgpt".to_string());
    first.workspace_id = Some("ignored-workspace".to_string());
    let mut second = sample_account("acc-refresh-second", "limited", now);
    second.issuer = "https://issuer.second".to_string();
    let mut ignored = sample_account("acc-refresh-ignored", "active", now);
    ignored.issuer = "https://issuer.ignored".to_string();

    for account in [&first, &second, &ignored] {
        storage.insert_account(account).expect("insert account");
    }

    let issuers = storage
        .list_account_token_refresh_issuers_for_ids(&[
            second.id.clone(),
            "acc-refresh-missing".to_string(),
            first.id.clone(),
            second.id.clone(),
        ])
        .expect("list token refresh issuers");

    assert_eq!(
        issuers
            .into_iter()
            .map(|issuer| (issuer.id, issuer.issuer))
            .collect::<Vec<_>>(),
        vec![
            (
                "acc-refresh-first".to_string(),
                "https://issuer.first".to_string()
            ),
            (
                "acc-refresh-second".to_string(),
                "https://issuer.second".to_string()
            ),
        ]
    );
}

#[test]
fn list_account_ids_reads_only_ids_in_account_order() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-list-id", "active", now);
    first.sort = 1;
    first.updated_at = now;
    let mut second = sample_account("acc-second-list-id", "active", now);
    second.sort = 0;
    second.updated_at = now.saturating_sub(10);

    for account in [&first, &second] {
        storage.insert_account(account).expect("insert account");
    }

    assert_eq!(
        storage.list_account_ids().expect("list account ids"),
        vec![
            "acc-second-list-id".to_string(),
            "acc-first-list-id".to_string()
        ]
    );
}

#[test]
fn list_account_auth_refresh_targets_reads_only_refresh_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-refresh-target", "active", now);
    first.label = "First".to_string();
    first.issuer = "issuer-first".to_string();
    first.sort = 1;
    first.group_name = Some("ignored-group".to_string());
    let mut second = sample_account("acc-second-refresh-target", "disabled", now);
    second.label = "Second".to_string();
    second.issuer = "issuer-second".to_string();
    second.sort = 0;
    second.workspace_id = Some("ignored-workspace".to_string());

    for account in [&first, &second] {
        storage.insert_account(account).expect("insert account");
    }

    let targets = storage
        .list_account_auth_refresh_targets()
        .expect("list account auth refresh targets");

    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].id, "acc-second-refresh-target");
    assert_eq!(targets[0].label, "Second");
    assert_eq!(targets[0].issuer, "issuer-second");
    assert_eq!(targets[1].id, "acc-first-refresh-target");
    assert_eq!(targets[1].label, "First");
    assert_eq!(targets[1].issuer, "issuer-first");
}

#[test]
fn list_account_cleanup_candidates_by_statuses_reads_only_id_and_status() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-cleanup", "banned", now);
    first.sort = 2;
    first.label = "ignored label".to_string();
    let mut second = sample_account("acc-second-cleanup", "limited", now);
    second.sort = 1;
    second.chatgpt_account_id = Some("ignored-chatgpt-id".to_string());
    let mut ignored = sample_account("acc-ignored-cleanup", "active", now);
    ignored.sort = 0;

    for account in [&first, &second, &ignored] {
        storage.insert_account(account).expect("insert account");
    }

    let candidates = storage
        .list_account_cleanup_candidates_by_statuses(&["banned".to_string(), "limited".to_string()])
        .expect("list cleanup candidates");

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].id, "acc-second-cleanup");
    assert_eq!(candidates[0].status, "limited");
    assert_eq!(candidates[1].id, "acc-first-cleanup");
    assert_eq!(candidates[1].status, "banned");
}

#[test]
fn list_account_quota_source_summaries_reads_only_source_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-quota-source", "active", now);
    first.label = "First Quota".to_string();
    first.sort = 1;
    first.issuer = "ignored-issuer".to_string();
    let mut second = sample_account("acc-second-quota-source", "limited", now);
    second.label = "Second Quota".to_string();
    second.sort = 0;
    second.workspace_id = Some("ignored-workspace".to_string());

    for account in [&first, &second] {
        storage.insert_account(account).expect("insert account");
    }

    let summaries = storage
        .list_account_quota_source_summaries()
        .expect("list account quota source summaries");

    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0].id, "acc-second-quota-source");
    assert_eq!(summaries[0].label, "Second Quota");
    assert_eq!(summaries[0].status, "limited");
    assert_eq!(summaries[1].id, "acc-first-quota-source");
    assert_eq!(summaries[1].label, "First Quota");
    assert_eq!(summaries[1].status, "active");
}

#[test]
fn list_available_account_quota_pool_sources_filters_and_reads_only_id_label() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-pool-source", "active", now);
    first.label = "First Pool".to_string();
    first.sort = 1;
    first.issuer = "ignored-issuer".to_string();
    let mut second = sample_account("acc-second-pool-source", " AVAILABLE ", now);
    second.label = "Second Pool".to_string();
    second.sort = 0;
    second.workspace_id = Some("ignored-workspace".to_string());
    let mut disabled = sample_account("acc-disabled-pool-source", "disabled", now);
    disabled.label = "Disabled Pool".to_string();
    disabled.sort = -1;

    for account in [&first, &second, &disabled] {
        storage.insert_account(account).expect("insert account");
    }

    let sources = storage
        .list_available_account_quota_pool_sources()
        .expect("list account quota pool sources");

    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].id, "acc-second-pool-source");
    assert_eq!(sources[0].label, "Second Pool");
    assert_eq!(sources[1].id, "acc-first-pool-source");
    assert_eq!(sources[1].label, "First Pool");
}

#[test]
fn list_account_import_snapshots_reads_only_import_index_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-import-snapshot", "disabled", now);
    first.label = "First Import".to_string();
    first.issuer = "issuer-first".to_string();
    first.chatgpt_account_id = Some("cgpt-first".to_string());
    first.workspace_id = Some("ws-first".to_string());
    first.group_name = Some("ignored-group".to_string());
    first.sort = 1;
    first.created_at = now.saturating_sub(10);
    let mut second = sample_account("acc-second-import-snapshot", "active", now);
    second.label = "Second Import".to_string();
    second.issuer = "issuer-second".to_string();
    second.sort = 0;

    for account in [&first, &second] {
        storage.insert_account(account).expect("insert account");
    }

    let snapshots = storage
        .list_account_import_snapshots()
        .expect("list account import snapshots");

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].id, "acc-second-import-snapshot");
    assert_eq!(snapshots[0].label, "Second Import");
    assert_eq!(snapshots[0].issuer, "issuer-second");
    assert_eq!(snapshots[0].sort, 0);
    assert_eq!(snapshots[1].id, "acc-first-import-snapshot");
    assert_eq!(
        snapshots[1].chatgpt_account_id.as_deref(),
        Some("cgpt-first")
    );
    assert_eq!(snapshots[1].workspace_id.as_deref(), Some("ws-first"));
    assert_eq!(snapshots[1].created_at, now.saturating_sub(10));
}

#[test]
fn list_account_summary_rows_reads_only_list_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-summary-row", "active", now);
    first.label = "First Summary".to_string();
    first.group_name = Some("team-a".to_string());
    first.sort = 1;
    first.issuer = "ignored-issuer".to_string();
    let mut second = sample_account("acc-second-summary-row", "disabled", now);
    second.label = "Second Summary".to_string();
    second.sort = 0;
    second.chatgpt_account_id = Some("ignored-chatgpt".to_string());

    for account in [&first, &second] {
        storage.insert_account(account).expect("insert account");
    }

    let rows = storage
        .list_account_summary_rows()
        .expect("list account summary rows");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, "acc-second-summary-row");
    assert_eq!(rows[0].label, "Second Summary");
    assert_eq!(rows[0].group_name, None);
    assert_eq!(rows[0].sort, 0);
    assert_eq!(rows[0].status, "disabled");
    assert_eq!(rows[1].id, "acc-first-summary-row");
    assert_eq!(rows[1].group_name.as_deref(), Some("team-a"));
    assert_eq!(rows[1].status, "active");
}

#[test]
fn account_summary_storage_snapshot_loads_related_rows_for_requested_accounts() {
    let mut storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let requested = sample_account("acc-summary-snapshot", "active", now);
    storage
        .insert_account(&requested)
        .expect("insert requested account");
    storage
        .set_preferred_account(Some("acc-summary-snapshot"))
        .expect("set preferred account");
    storage
        .insert_account(&sample_account("acc-summary-ignored", "active", now))
        .expect("insert ignored account");
    storage
        .insert_token(&sample_token("acc-summary-snapshot", now))
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-summary-snapshot".to_string(),
            used_percent: Some(12.5),
            window_minutes: Some(60),
            resets_at: Some(now + 60),
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");
    storage
        .upsert_account_metadata("acc-summary-snapshot", Some("note"), Some("tag"))
        .expect("insert metadata");
    storage
        .upsert_account_subscription(
            "acc-summary-snapshot",
            true,
            Some("team"),
            Some("plus"),
            Some(now + 100),
            Some(now + 50),
        )
        .expect("insert subscription");
    storage
        .set_quota_source_model_assignments(
            "openai_account",
            "acc-summary-snapshot",
            &["gpt-visible".to_string()],
        )
        .expect("insert model assignment");
    storage
        .set_quota_source_model_assignments(
            "aggregate_api",
            "acc-summary-snapshot",
            &["gpt-hidden".to_string()],
        )
        .expect("insert unrelated model assignment");
    storage
        .upsert_account_quota_capacity_override("acc-summary-snapshot", Some(100), Some(200))
        .expect("insert quota override");

    let snapshot = storage
        .load_account_summary_storage_snapshot(&["acc-summary-snapshot".to_string()])
        .expect("load account summary snapshot");

    assert_eq!(
        snapshot.preferred_account_id.as_deref(),
        Some("acc-summary-snapshot")
    );
    assert_eq!(snapshot.tokens.len(), 1);
    assert_eq!(snapshot.usage_snapshots.len(), 1);
    assert_eq!(snapshot.metadata.len(), 1);
    assert_eq!(snapshot.metadata[0].note.as_deref(), Some("note"));
    assert_eq!(snapshot.subscriptions.len(), 1);
    assert_eq!(snapshot.subscriptions[0].plan_type.as_deref(), Some("plus"));
    assert!(
        snapshot.model_assignments.is_empty(),
        "account runtime must not consume legacy model assignments"
    );
    assert_eq!(snapshot.quota_overrides.len(), 1);
    assert_eq!(snapshot.quota_overrides[0].primary_window_tokens, Some(100));

    let empty = storage
        .load_account_summary_storage_snapshot(&[])
        .expect("load empty account summary snapshot");
    assert!(empty.tokens.is_empty());
    assert!(empty.usage_snapshots.is_empty());
    assert!(empty.metadata.is_empty());
    assert!(empty.subscriptions.is_empty());
    assert!(empty.model_assignments.is_empty());
    assert!(empty.quota_overrides.is_empty());
}

#[test]
fn light_account_summary_storage_snapshot_skips_display_details() {
    let mut storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let account_id = "acc-summary-light";

    storage
        .insert_account(&sample_account(account_id, "active", now))
        .expect("insert account");
    storage
        .insert_token(&sample_token(account_id, now))
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(18.0),
            window_minutes: Some(180),
            resets_at: Some(now + 300),
            secondary_used_percent: Some(28.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: Some(now + 600),
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");
    storage
        .upsert_account_metadata(account_id, Some("note"), Some("tag"))
        .expect("insert metadata");
    storage
        .upsert_account_subscription(account_id, true, Some("team"), Some("plus"), None, None)
        .expect("insert subscription");
    storage
        .set_quota_source_model_assignments(
            "openai_account",
            account_id,
            &["gpt-visible".to_string()],
        )
        .expect("insert model assignment");
    storage
        .upsert_account_quota_capacity_override(account_id, Some(100), Some(200))
        .expect("insert quota override");

    let snapshot = storage
        .load_account_summary_storage_snapshot_with_options(
            &[account_id.to_string()],
            AccountSummaryStorageSnapshotOptions::light(),
        )
        .expect("load light account summary snapshot");

    assert_eq!(snapshot.tokens.len(), 1);
    assert_eq!(snapshot.usage_snapshots.len(), 1);
    assert!(snapshot.metadata.is_empty());
    assert!(snapshot.subscriptions.is_empty());
    assert!(snapshot.model_assignments.is_empty());
    assert!(snapshot.quota_overrides.is_empty());
}

#[test]
fn dashboard_light_account_summary_snapshot_keeps_usage_but_skips_runtime_rows() {
    let mut storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let account_id = "acc-summary-dashboard-light";

    storage
        .insert_account(&sample_account(account_id, "active", now))
        .expect("insert account");
    storage
        .set_preferred_account(Some(account_id))
        .expect("set preferred account");
    storage
        .insert_token(&sample_token(account_id, now))
        .expect("insert token");
    storage
        .insert_event(&Event {
            account_id: Some(account_id.to_string()),
            event_type: "account_status".to_string(),
            message: "status=unavailable reason=usage_http_401".to_string(),
            created_at: now,
        })
        .expect("insert status reason event");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(18.0),
            window_minutes: Some(180),
            resets_at: Some(now + 300),
            secondary_used_percent: Some(28.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: Some(now + 600),
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");

    let snapshot = storage
        .load_account_summary_storage_snapshot_with_options(
            &[account_id.to_string()],
            AccountSummaryStorageSnapshotOptions::dashboard_light(),
        )
        .expect("load dashboard light account summary snapshot");

    assert!(snapshot.preferred_account_id.is_none());
    assert!(snapshot.status_reasons.is_empty());
    assert!(snapshot.tokens.is_empty());
    assert_eq!(snapshot.usage_snapshots.len(), 1);
    assert!(snapshot.metadata.is_empty());
    assert!(snapshot.subscriptions.is_empty());
    assert!(snapshot.model_assignments.is_empty());
    assert!(snapshot.quota_overrides.is_empty());
}

#[test]
fn account_status_counts_aggregates_normalized_statuses() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for (id, status) in [
        ("acc-active-a", "active"),
        ("acc-active-b", " Active "),
        ("acc-disabled", "disabled"),
    ] {
        storage
            .insert_account(&sample_account(id, status, now))
            .expect("insert account");
    }

    let counts = storage
        .account_status_counts()
        .expect("count account statuses");

    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].status, "active");
    assert_eq!(counts[0].count, 2);
    assert_eq!(counts[1].status, "disabled");
    assert_eq!(counts[1].count, 1);

    let plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", account_status_counts_sql()),
    );
    assert!(
        plan.contains("idx_accounts_cleanup_status_lookup"),
        "expected normalized status counts to use cleanup status expression index, got {plan}"
    );
}

#[test]
fn account_group_name_filter_uses_group_sort_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut team_a = sample_account("acc-team-a", "active", now);
    team_a.group_name = Some("TEAM_A".to_string());
    team_a.sort = 2;
    let mut team_b = sample_account("acc-team-b", "active", now);
    team_b.group_name = Some("TEAM_B".to_string());
    team_b.sort = 1;

    storage.insert_account(&team_a).expect("insert team a");
    storage.insert_account(&team_b).expect("insert team b");

    let accounts = storage
        .list_accounts_filtered(None, Some("TEAM_A"))
        .expect("filter team a accounts");
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, "acc-team-a");
    assert_eq!(accounts[0].group_name.as_deref(), Some("TEAM_A"));
    assert_eq!(
        storage
            .account_count_filtered(None, Some("TEAM_A"))
            .expect("count team a accounts"),
        1
    );

    let sql = account_query_sql(" WHERE a.group_name = ?", false);
    let plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("prepare explain")
        .query_map(["TEAM_A"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    assert!(
        plan.iter()
            .any(|detail| detail.contains("idx_accounts_group_name_sort_updated_at")),
        "expected group filter plan to use idx_accounts_group_name_sort_updated_at, got {plan:?}"
    );

    let count_sql = account_count_filtered_sql(" WHERE accounts.group_name = ?");
    let count_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {count_sql}"))
        .expect("prepare count explain")
        .query_map(["TEAM_A"], |row| row.get::<_, String>(3))
        .expect("query count explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect count explain");
    assert!(
        count_plan
            .iter()
            .any(|detail| detail.contains("idx_accounts_group_name_sort_updated_at")),
        "expected group count plan to use idx_accounts_group_name_sort_updated_at, got {count_plan:?}"
    );
}

#[test]
fn account_base_lists_use_sort_updated_id_order_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    for (label, sql) in [
        ("account id list", account_ids_list_sql()),
        (
            "account auth refresh target list",
            account_auth_refresh_targets_list_sql(),
        ),
        (
            "account quota source summary list",
            account_quota_source_summaries_list_sql(),
        ),
        (
            "account import snapshot list",
            account_import_snapshots_list_sql(),
        ),
        ("account summary row list", account_summary_rows_list_sql()),
    ] {
        let plan = storage
            .conn
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("prepare explain")
            .query_map([], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");

        assert!(
            plan.iter()
                .any(|detail| detail.contains("idx_accounts_list_order")),
            "expected {label} to use account list-order index, got {plan:?}"
        );
        assert!(
            !plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "expected {label} to avoid a temp sort, got {plan:?}"
        );
    }
}

#[test]
fn list_accounts_by_statuses_filters_and_preserves_account_order() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut active = sample_account("acc-active", "active", now);
    active.sort = 0;
    let mut banned = sample_account("acc-banned", " BANNED ", now);
    banned.sort = 1;
    let mut limited = sample_account("acc-limited", "limited", now);
    limited.sort = 2;
    let mut disabled = sample_account("acc-disabled", "disabled", now);
    disabled.sort = 3;
    for account in [&active, &banned, &limited, &disabled] {
        storage.insert_account(account).expect("insert account");
    }

    let accounts = storage
        .list_accounts_by_statuses(&["limited".to_string(), "banned".to_string()])
        .expect("list accounts by statuses");

    assert_eq!(
        accounts
            .into_iter()
            .map(|account| account.id)
            .collect::<Vec<_>>(),
        vec!["acc-banned".to_string(), "acc-limited".to_string()]
    );
}

#[test]
fn account_status_chunk_queries_defer_final_ordering_to_rust() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let account_ids_sql =
        account_ids_by_statuses_chunk_sql("LOWER(TRIM(COALESCE(status, ''))) IN (?1, ?2)");
    let account_ids_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {account_ids_sql}"))
        .expect("prepare explain")
        .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    let accounts_sql =
        accounts_by_statuses_chunk_sql("LOWER(TRIM(COALESCE(a.status, ''))) IN (?1, ?2)");
    let accounts_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {accounts_sql}"))
        .expect("prepare explain")
        .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    let refresh_targets_sql = account_usage_refresh_targets_by_statuses_chunk_sql(
        "LOWER(TRIM(COALESCE(status, ''))) IN (?1, ?2)",
    );
    let refresh_targets_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {refresh_targets_sql}"))
        .expect("prepare explain")
        .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    let usable_refresh_targets_sql =
        account_usage_refresh_targets_with_usable_tokens_by_statuses_chunk_sql(
            "LOWER(TRIM(COALESCE(a.status, ''))) IN (?1, ?2)",
        );
    let usable_refresh_targets_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {usable_refresh_targets_sql}"))
        .expect("prepare explain")
        .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");

    assert!(
        !account_ids_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
        "status id chunk query should avoid per-chunk ORDER BY temp sorting, got {account_ids_plan:?}"
    );
    assert!(
        !accounts_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
        "status account chunk query should avoid per-chunk ORDER BY temp sorting, got {accounts_plan:?}"
    );
    assert!(
        !refresh_targets_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
        "usage refresh target chunk query should avoid per-chunk ORDER BY temp sorting, got {refresh_targets_plan:?}"
    );
    assert!(
        !usable_refresh_targets_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
        "usable-token usage refresh target chunk query should avoid per-chunk ORDER BY temp sorting, got {usable_refresh_targets_plan:?}"
    );
    for (label, plan) in [
        ("status id", account_ids_plan),
        ("status account", accounts_plan),
        ("usage refresh target", refresh_targets_plan),
        (
            "usable-token usage refresh target",
            usable_refresh_targets_plan,
        ),
    ] {
        assert!(
            plan.iter()
                .any(|detail| detail.contains("idx_accounts_cleanup_status_lookup")),
            "expected {label} chunk query to use normalized status index, got {plan:?}"
        );
    }
}

#[test]
fn account_cleanup_candidates_use_normalized_status_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let sql = account_cleanup_candidates_by_statuses_chunk_sql(
        "LOWER(TRIM(COALESCE(status, ''))) IN (?1, ?2)",
    );
    let plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("prepare explain")
        .query_map(["limited", "banned"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");

    assert!(
        plan.iter()
            .any(|detail| detail.contains("idx_accounts_cleanup_status_lookup")),
        "expected cleanup candidate query to use normalized status index, got {plan:?}"
    );
}

#[test]
fn account_id_chunk_queries_defer_final_ordering_to_rust() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let account_ids_sql = account_ids_for_ids_chunk_sql("id IN (?1, ?2)");
    let account_ids_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {account_ids_sql}"))
        .expect("prepare explain")
        .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    let accounts_sql = accounts_for_ids_chunk_sql("a.id IN (?1, ?2)");
    let accounts_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {accounts_sql}"))
        .expect("prepare explain")
        .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    let token_refresh_issuers_sql =
        account_token_refresh_issuers_for_ids_chunk_sql("id IN (?1, ?2)");
    let token_refresh_issuers_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {token_refresh_issuers_sql}"))
        .expect("prepare explain")
        .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    let dashboard_metadata_sql =
        account_dashboard_source_metadata_for_ids_chunk_sql("id IN (?1, ?2)");
    let dashboard_metadata_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {dashboard_metadata_sql}"))
        .expect("prepare explain")
        .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");
    let codex_profile_sql =
        active_account_codex_profile_candidates_for_ids_chunk_sql("id IN (?1, ?2)");
    let codex_profile_plan = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {codex_profile_sql}"))
        .expect("prepare explain")
        .query_map(["acc-a", "acc-b"], |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>>>()
        .expect("collect explain");

    assert!(
        !account_ids_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
        "id chunk query should avoid per-chunk ORDER BY temp sorting, got {account_ids_plan:?}"
    );
    assert!(
        account_ids_plan
            .iter()
            .any(|detail| detail.contains("sqlite_autoindex_accounts_1")),
        "expected id chunk query to use account primary-key lookup, got {account_ids_plan:?}"
    );
    assert!(
        accounts_plan
            .iter()
            .any(|detail| detail.contains("sqlite_autoindex_accounts_1")),
        "expected full account id chunk query to use account primary-key lookup, got {accounts_plan:?}"
    );
    assert!(
        !accounts_plan
            .iter()
            .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
        "full account id chunk query should avoid per-chunk ORDER BY temp sorting, got {accounts_plan:?}"
    );
    for (label, plan) in [
        ("token refresh issuer", token_refresh_issuers_plan),
        ("dashboard metadata", dashboard_metadata_plan),
        ("codex profile candidate", codex_profile_plan),
    ] {
        assert!(
            plan.iter()
                .any(|detail| detail.contains("sqlite_autoindex_accounts_1")),
            "expected {label} chunk query to use account primary-key lookup, got {plan:?}"
        );
        assert!(
            !plan
                .iter()
                .any(|detail| detail.contains("USE TEMP B-TREE FOR ORDER BY")),
            "{label} chunk query should avoid per-chunk ORDER BY temp sorting, got {plan:?}"
        );
    }
}

#[test]
fn list_account_ids_by_statuses_filters_and_reads_only_ids_in_account_order() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut active = sample_account("acc-active", "active", now);
    active.sort = 0;
    let mut banned = sample_account("acc-banned", " BANNED ", now);
    banned.sort = 1;
    let mut limited = sample_account("acc-limited", "limited", now);
    limited.sort = 2;
    let mut disabled = sample_account("acc-disabled", "disabled", now);
    disabled.sort = 3;
    for account in [&active, &banned, &limited, &disabled] {
        storage.insert_account(account).expect("insert account");
    }

    let ids = storage
        .list_account_ids_by_statuses(&["limited".to_string(), "banned".to_string()])
        .expect("list account ids by statuses");

    assert_eq!(
        ids,
        vec!["acc-banned".to_string(), "acc-limited".to_string()]
    );
}

#[test]
fn list_account_usage_refresh_targets_by_statuses_reads_only_refresh_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut active = sample_account("acc-active-refresh", "active", now);
    active.sort = 1;
    active.workspace_id = Some("ws-active".to_string());
    active.label = "ignored label".to_string();
    let mut inactive = sample_account("acc-inactive-refresh", " INACTIVE ", now);
    inactive.sort = 0;
    inactive.workspace_id = Some("ws-inactive".to_string());
    inactive.issuer = "ignored issuer".to_string();
    let mut disabled = sample_account("acc-disabled-refresh", "disabled", now);
    disabled.sort = -1;
    disabled.workspace_id = Some("ws-disabled".to_string());
    for account in [&active, &inactive, &disabled] {
        storage.insert_account(account).expect("insert account");
    }

    let targets = storage
        .list_account_usage_refresh_targets_by_statuses(&[
            "active".to_string(),
            "inactive".to_string(),
        ])
        .expect("list account usage refresh targets");

    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].id, "acc-inactive-refresh");
    assert_eq!(targets[0].status, " INACTIVE ");
    assert_eq!(targets[0].workspace_id.as_deref(), Some("ws-inactive"));
    assert_eq!(targets[1].id, "acc-active-refresh");
    assert_eq!(targets[1].status, "active");
    assert_eq!(targets[1].workspace_id.as_deref(), Some("ws-active"));
}

#[test]
fn list_account_usage_refresh_targets_with_usable_tokens_by_statuses_filters_in_sql() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut active = sample_account("acc-active-token-refresh", "active", now);
    active.sort = 1;
    active.workspace_id = Some("ws-active-token".to_string());
    let mut inactive = sample_account("acc-inactive-token-refresh", " INACTIVE ", now);
    inactive.sort = 0;
    inactive.workspace_id = Some("ws-inactive-token".to_string());
    let mut disabled = sample_account("acc-disabled-token-refresh", "disabled", now);
    disabled.sort = -1;
    let no_access = sample_account("acc-no-access-token-refresh", "active", now);
    let no_refresh = sample_account("acc-no-refresh-token-refresh", "active", now);
    let missing_token = sample_account("acc-missing-token-refresh", "active", now);
    for account in [
        &active,
        &inactive,
        &disabled,
        &no_access,
        &no_refresh,
        &missing_token,
    ] {
        storage.insert_account(account).expect("insert account");
    }
    storage
        .insert_token(&sample_token(active.id.as_str(), now))
        .expect("insert active token");
    storage
        .insert_token(&sample_token(inactive.id.as_str(), now))
        .expect("insert inactive token");
    storage
        .insert_token(&sample_token(disabled.id.as_str(), now))
        .expect("insert disabled token");
    storage
        .insert_token(&Token {
            access_token: " ".to_string(),
            ..sample_token(no_access.id.as_str(), now)
        })
        .expect("insert no access token");
    storage
        .insert_token(&Token {
            refresh_token: String::new(),
            ..sample_token(no_refresh.id.as_str(), now)
        })
        .expect("insert no refresh token");

    let targets = storage
        .list_account_usage_refresh_targets_with_usable_tokens_by_statuses(&[
            "active".to_string(),
            "inactive".to_string(),
        ])
        .expect("list account usage refresh targets with usable tokens");

    assert_eq!(
        targets
            .iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        vec!["acc-inactive-token-refresh", "acc-active-token-refresh"]
    );
    assert_eq!(targets[0].status, " INACTIVE ");
    assert_eq!(
        targets[0].workspace_id.as_deref(),
        Some("ws-inactive-token")
    );
    assert_eq!(targets[1].workspace_id.as_deref(), Some("ws-active-token"));
}

#[test]
fn list_account_usage_refresh_token_targets_filters_blocked_latest_status_in_sql() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut ready = sample_account("acc-ready-token-target", "active", now);
    ready.sort = 1;
    ready.workspace_id = Some("ws-ready".to_string());
    let mut recovered = sample_account("acc-recovered-token-target", "active", now);
    recovered.sort = 0;
    recovered.workspace_id = Some("ws-recovered".to_string());
    let mut region_blocked = sample_account("acc-region-blocked-token-target", "active", now);
    region_blocked.sort = 2;
    let mut blocked = sample_account("acc-blocked-token-target", "active", now);
    blocked.sort = 3;
    let mut no_access = sample_account("acc-no-access-token-target", "active", now);
    no_access.sort = 4;
    let mut agent_identity = sample_account("acc-agent-identity-token-target", "active", now);
    agent_identity.sort = 5;
    agent_identity.workspace_id = Some("ws-agent-identity".to_string());
    let disabled = sample_account("acc-disabled-token-target", "disabled", now);

    for account in [
        &ready,
        &recovered,
        &region_blocked,
        &blocked,
        &no_access,
        &agent_identity,
        &disabled,
    ] {
        storage.insert_account(account).expect("insert account");
    }
    for account in [&ready, &recovered, &region_blocked, &blocked, &disabled] {
        storage
            .insert_token(&sample_token(account.id.as_str(), now))
            .expect("insert token");
    }
    storage
        .insert_token(&Token {
            access_token: " ".to_string(),
            ..sample_token(no_access.id.as_str(), now)
        })
        .expect("insert no access token");
    storage
        .insert_token(&Token {
            id_token: String::new(),
            access_token: String::new(),
            refresh_token: String::new(),
            ..sample_token(agent_identity.id.as_str(), now)
        })
        .expect("insert empty agent identity token");
    storage
        .upsert_account_agent_identity(&AccountAgentIdentity {
            account_id: agent_identity.id.clone(),
            agent_runtime_id: "agent-runtime-1".to_string(),
            agent_private_key: "private-key-1".to_string(),
            task_id: None,
            chatgpt_user_id: "user-1".to_string(),
            chatgpt_account_is_fedramp: false,
            auth_mode: "agentIdentity".to_string(),
            workspace_id: Some("ws-agent-identity".to_string()),
            created_at: now,
            updated_at: now,
        })
        .expect("insert agent identity");

    storage
        .insert_event(&Event {
            account_id: Some(recovered.id.clone()),
            event_type: "account_status_update".to_string(),
            message: "status=banned reason=account_deactivated".to_string(),
            created_at: now,
        })
        .expect("insert old recovered blocked event");
    storage
        .insert_event(&Event {
            account_id: Some(recovered.id.clone()),
            event_type: "account_status_update".to_string(),
            message: "status=active reason=manual_reactivated".to_string(),
            created_at: now + 1,
        })
        .expect("insert latest recovered allowed event");
    storage
        .insert_event(&Event {
            account_id: Some(region_blocked.id.clone()),
            event_type: "account_status_update".to_string(),
            message: "status=unavailable reason=refresh_token_region_blocked".to_string(),
            created_at: now + 2,
        })
        .expect("insert region blocked event");
    storage
        .insert_event(&Event {
            account_id: Some(blocked.id.clone()),
            event_type: "account_status_update".to_string(),
            message: "status=banned reason=account_deactivated".to_string(),
            created_at: now + 3,
        })
        .expect("insert blocked event");

    let targets = storage
        .list_account_usage_refresh_token_targets_by_statuses(&[
            "active".to_string(),
            "inactive".to_string(),
        ])
        .expect("list usage refresh token targets");

    assert_eq!(
        targets
            .iter()
            .map(|target| target.account_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "acc-recovered-token-target",
            "acc-ready-token-target",
            "acc-region-blocked-token-target",
            "acc-agent-identity-token-target",
        ]
    );
    assert_eq!(targets[0].workspace_id.as_deref(), Some("ws-recovered"));
    assert_eq!(targets[0].token.account_id, "acc-recovered-token-target");
    assert_eq!(targets[0].token.access_token, "access");
    assert_eq!(targets[1].workspace_id.as_deref(), Some("ws-ready"));
    assert_eq!(
        targets[2].token.account_id,
        "acc-region-blocked-token-target"
    );
    assert_eq!(
        targets[3].workspace_id.as_deref(),
        Some("ws-agent-identity")
    );
    assert!(targets[3].token.access_token.is_empty());
    assert!(targets[3].token.refresh_token.is_empty());
}

#[test]
fn usage_refresh_token_targets_scope_latest_status_to_target_accounts() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let sql = usage_refresh_token_targets_by_status_sql(
        "LOWER(TRIM(COALESCE(a.status, ''))) IN ('active', 'inactive')",
    );
    let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));

    assert!(
        sql.contains("INNER JOIN target_accounts ta"),
        "expected latest status CTE to join target accounts before ranking, got {sql}"
    );
    assert!(
        plan.contains("idx_events_account_status_lookup"),
        "expected scoped latest status lookup to use event account status index, got {plan}"
    );
}

#[test]
fn usage_refresh_token_targets_ignore_non_agent_identity_auth_mode() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let account = sample_account("acc-non-agent-identity-target", "active", now);
    storage.insert_account(&account).expect("insert account");
    storage
        .insert_token(&Token {
            id_token: String::new(),
            access_token: String::new(),
            refresh_token: String::new(),
            ..sample_token(&account.id, now)
        })
        .expect("insert empty token");
    storage
        .upsert_account_agent_identity(&AccountAgentIdentity {
            account_id: account.id.clone(),
            agent_runtime_id: "agent-runtime-1".to_string(),
            agent_private_key: "private-key-1".to_string(),
            task_id: None,
            chatgpt_user_id: "user-1".to_string(),
            chatgpt_account_is_fedramp: false,
            auth_mode: "oauth".to_string(),
            workspace_id: None,
            created_at: now,
            updated_at: now,
        })
        .expect("insert non-agent identity row");

    let targets = storage
        .list_account_usage_refresh_token_targets_by_statuses(&["active".to_string()])
        .expect("list usage refresh targets");
    assert!(targets.is_empty());
}

#[test]
fn find_account_direct_auth_profile_by_id_reads_direct_auth_fields_only() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-direct-auth-profile", " active ", now);
    account.label = "ignored label".to_string();
    account.issuer = "https://auth.example.test".to_string();
    account.chatgpt_account_id = Some("chatgpt-direct-auth".to_string());
    account.workspace_id = Some("ignored-workspace".to_string());
    account.group_name = Some("ignored group".to_string());
    account.sort = 99;
    storage.insert_account(&account).expect("insert account");

    let profile = storage
        .find_account_direct_auth_profile_by_id("acc-direct-auth-profile")
        .expect("find direct auth profile")
        .expect("profile exists");

    assert_eq!(profile.id, "acc-direct-auth-profile");
    assert_eq!(profile.issuer, "https://auth.example.test");
    assert_eq!(
        profile.chatgpt_account_id.as_deref(),
        Some("chatgpt-direct-auth")
    );
    assert_eq!(profile.status, " active ");
    assert!(storage
        .find_account_direct_auth_profile_by_id("acc-missing-direct-auth")
        .expect("find missing direct auth profile")
        .is_none());

    let plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            account_direct_auth_profile_by_id_sql()
        ),
        rusqlite::params!["acc-direct-auth-profile"],
    );
    assert!(
        plan.contains("sqlite_autoindex_accounts_1"),
        "expected direct auth profile lookup to use account primary-key index, got {plan}"
    );
}

#[test]
fn list_active_account_codex_profile_candidates_for_ids_filters_active_and_reads_candidate_fields()
{
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first-codex-profile", "active", now);
    first.label = "First Codex".to_string();
    first.issuer = "issuer-first".to_string();
    first.chatgpt_account_id = Some("cgpt-first".to_string());
    first.workspace_id = Some("ws-first".to_string());
    first.group_name = Some("group-first".to_string());
    first.sort = 1;
    let mut second = sample_account("acc-second-codex-profile", " ACTIVE ", now);
    second.label = "Second Codex".to_string();
    second.issuer = "issuer-second".to_string();
    second.chatgpt_account_id = Some("cgpt-second".to_string());
    second.workspace_id = Some("ws-second".to_string());
    second.group_name = Some("group-second".to_string());
    second.sort = 0;
    let mut disabled = sample_account("acc-disabled-codex-profile", "disabled", now);
    disabled.label = "Disabled Codex".to_string();
    disabled.sort = -1;
    for account in [&first, &second, &disabled] {
        storage.insert_account(account).expect("insert account");
    }

    let targets = storage
        .list_active_account_codex_profile_candidates_for_ids(&[
            "acc-disabled-codex-profile".to_string(),
            "acc-first-codex-profile".to_string(),
            "acc-second-codex-profile".to_string(),
        ])
        .expect("list codex profile account candidates");

    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].id, "acc-second-codex-profile");
    assert_eq!(targets[0].label, "Second Codex");
    assert_eq!(targets[0].issuer, "issuer-second");
    assert_eq!(
        targets[0].chatgpt_account_id.as_deref(),
        Some("cgpt-second")
    );
    assert_eq!(targets[0].workspace_id.as_deref(), Some("ws-second"));
    assert_eq!(targets[0].group_name.as_deref(), Some("group-second"));
    assert_eq!(targets[0].status, " ACTIVE ");
    assert_eq!(targets[1].id, "acc-first-codex-profile");
    assert_eq!(targets[1].label, "First Codex");
    assert_eq!(targets[1].issuer, "issuer-first");
}

#[test]
fn find_account_by_identity_prefers_id_then_chatgpt_then_workspace() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut by_id = sample_account("acc-by-id", "active", now);
    by_id.chatgpt_account_id = Some("cgpt-shared".to_string());
    by_id.workspace_id = Some("ws-shared".to_string());
    by_id.updated_at = now;
    let mut by_chatgpt = sample_account("acc-by-chatgpt", "active", now);
    by_chatgpt.chatgpt_account_id = Some("cgpt-shared".to_string());
    by_chatgpt.workspace_id = Some("ws-chatgpt".to_string());
    by_chatgpt.updated_at = now.saturating_add(10);
    let mut by_workspace = sample_account("acc-by-workspace", "active", now);
    by_workspace.chatgpt_account_id = Some("cgpt-workspace".to_string());
    by_workspace.workspace_id = Some("ws-shared".to_string());
    by_workspace.updated_at = now.saturating_add(20);

    storage.insert_account(&by_id).expect("insert by id");
    storage
        .insert_account(&by_chatgpt)
        .expect("insert by chatgpt");
    storage
        .insert_account(&by_workspace)
        .expect("insert by workspace");

    let found_by_id = storage
        .find_account_by_identity(Some("acc-by-id"), Some("cgpt-shared"), Some("ws-shared"))
        .expect("find by identity")
        .expect("account exists");
    assert_eq!(found_by_id.id, "acc-by-id");

    let found_by_chatgpt = storage
        .find_account_by_identity(None, Some("cgpt-shared"), Some("ws-shared"))
        .expect("find by chatgpt")
        .expect("account exists");
    assert_eq!(found_by_chatgpt.id, "acc-by-chatgpt");

    let found_by_workspace = storage
        .find_account_by_identity(None, None, Some("ws-shared"))
        .expect("find by workspace")
        .expect("account exists");
    assert_eq!(found_by_workspace.id, "acc-by-workspace");

    assert!(storage
        .find_account_by_identity(None, Some("missing"), Some("also-missing"))
        .expect("find missing identity")
        .is_none());
}

#[test]
fn find_account_id_by_identity_reads_id_only_with_same_priority() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut by_id = sample_account("acc-id-only-by-id", "active", now);
    by_id.label = "ignored label".to_string();
    by_id.chatgpt_account_id = Some("cgpt-id-only-shared".to_string());
    by_id.workspace_id = Some("ws-id-only-shared".to_string());
    by_id.updated_at = now;
    let mut by_chatgpt = sample_account("acc-id-only-by-chatgpt", "active", now);
    by_chatgpt.chatgpt_account_id = Some("cgpt-id-only-shared".to_string());
    by_chatgpt.workspace_id = Some("ws-id-only-chatgpt".to_string());
    by_chatgpt.updated_at = now.saturating_add(10);
    let mut by_workspace = sample_account("acc-id-only-by-workspace", "active", now);
    by_workspace.chatgpt_account_id = Some("cgpt-id-only-workspace".to_string());
    by_workspace.workspace_id = Some("ws-id-only-shared".to_string());
    by_workspace.updated_at = now.saturating_add(20);

    for account in [&by_id, &by_chatgpt, &by_workspace] {
        storage.insert_account(account).expect("insert account");
    }

    assert_eq!(
        storage
            .find_account_id_by_identity(
                Some("acc-id-only-by-id"),
                Some("cgpt-id-only-shared"),
                Some("ws-id-only-shared")
            )
            .expect("find by id")
            .as_deref(),
        Some("acc-id-only-by-id")
    );
    assert_eq!(
        storage
            .find_account_id_by_identity(
                None,
                Some("cgpt-id-only-shared"),
                Some("ws-id-only-shared")
            )
            .expect("find by chatgpt")
            .as_deref(),
        Some("acc-id-only-by-chatgpt")
    );
    assert_eq!(
        storage
            .find_account_id_by_identity(None, None, Some("ws-id-only-shared"))
            .expect("find by workspace")
            .as_deref(),
        Some("acc-id-only-by-workspace")
    );
    assert!(storage
        .find_account_id_by_identity(None, Some("missing"), Some("also-missing"))
        .expect("find missing id")
        .is_none());
}

#[test]
fn account_identity_lookup_uses_identity_indexes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-indexed-identity", "active", now);
    account.chatgpt_account_id = Some("cgpt-indexed".to_string());
    account.workspace_id = Some("ws-indexed".to_string());
    storage.insert_account(&account).expect("insert account");

    for (column, index_name, value) in [
        (
            "chatgpt_account_id",
            "idx_accounts_chatgpt_account_id_updated_at",
            "cgpt-indexed",
        ),
        (
            "workspace_id",
            "idx_accounts_workspace_id_updated_at",
            "ws-indexed",
        ),
    ] {
        let sql = format!("EXPLAIN QUERY PLAN {}", account_identity_lookup_sql(column));
        let plan = storage
            .conn
            .prepare(&sql)
            .expect("prepare explain")
            .query_map([value], |row| row.get::<_, String>(3))
            .expect("query explain")
            .collect::<Result<Vec<_>>>()
            .expect("collect explain");
        assert!(
            plan.iter().any(|detail| detail.contains(index_name)),
            "expected {column} lookup to use {index_name}, got {plan:?}"
        );
    }
}

#[test]
fn matching_identity_accounts_only_returns_identity_candidates() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut by_chatgpt = sample_account("acc-by-chatgpt", "active", now);
    by_chatgpt.chatgpt_account_id = Some("cgpt-target".to_string());
    by_chatgpt.workspace_id = Some("ws-other".to_string());
    by_chatgpt.updated_at = now.saturating_add(10);
    let mut by_workspace = sample_account("acc-by-workspace", "active", now);
    by_workspace.chatgpt_account_id = Some("cgpt-other".to_string());
    by_workspace.workspace_id = Some("ws-target".to_string());
    by_workspace.updated_at = now.saturating_add(20);
    let by_id = sample_account("acc-by-id", "active", now);
    let unrelated = sample_account("acc-unrelated", "active", now.saturating_add(30));

    for account in [&by_chatgpt, &by_workspace, &by_id, &unrelated] {
        storage.insert_account(account).expect("insert account");
    }

    let accounts = storage
        .list_accounts_matching_identity(
            &["acc-by-id".to_string()],
            Some("cgpt-target"),
            Some("ws-target"),
        )
        .expect("list identity candidates");

    assert_eq!(
        accounts
            .into_iter()
            .map(|account| account.id)
            .collect::<Vec<_>>(),
        vec![
            "acc-by-workspace".to_string(),
            "acc-by-chatgpt".to_string(),
            "acc-by-id".to_string()
        ]
    );
}

#[test]
fn matching_identity_workspace_identities_only_reads_identity_candidates() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut by_chatgpt = sample_account("acc-identity-by-chatgpt", "active", now);
    by_chatgpt.label = "ignored label".to_string();
    by_chatgpt.issuer = "ignored issuer".to_string();
    by_chatgpt.chatgpt_account_id = Some("cgpt-target".to_string());
    by_chatgpt.workspace_id = Some("ws-other".to_string());
    by_chatgpt.updated_at = now.saturating_add(10);
    let mut by_workspace = sample_account("acc-identity-by-workspace", "active", now);
    by_workspace.group_name = Some("ignored group".to_string());
    by_workspace.chatgpt_account_id = Some("cgpt-other".to_string());
    by_workspace.workspace_id = Some("ws-target".to_string());
    by_workspace.updated_at = now.saturating_add(20);
    let by_id = sample_account("acc-identity-by-id", "active", now);
    let unrelated = sample_account("acc-identity-unrelated", "active", now.saturating_add(30));

    for account in [&by_chatgpt, &by_workspace, &by_id, &unrelated] {
        storage.insert_account(account).expect("insert account");
    }

    let identities = storage
        .list_account_workspace_identities_matching_identity(
            &["acc-identity-by-id".to_string()],
            Some("cgpt-target"),
            Some("ws-target"),
        )
        .expect("list identity candidates");

    assert_eq!(
        identities
            .iter()
            .map(|identity| identity.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "acc-identity-by-workspace",
            "acc-identity-by-chatgpt",
            "acc-identity-by-id"
        ]
    );
    assert_eq!(identities[0].workspace_id.as_deref(), Some("ws-target"));
    assert_eq!(
        identities[1].chatgpt_account_id.as_deref(),
        Some("cgpt-target")
    );
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
fn list_gateway_candidates_uses_account_id_as_stable_final_tiebreaker() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-c", "acc-a", "acc-b"] {
        storage
            .insert_account(&sample_account(account_id, "active", now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
    }

    let candidates = storage
        .list_gateway_candidates()
        .expect("list gateway candidates");
    let account_ids = candidates
        .into_iter()
        .map(|(account, _)| account.id)
        .collect::<Vec<_>>();

    assert_eq!(account_ids, vec!["acc-a", "acc-b", "acc-c"]);
    let sql = gateway_candidates_filtered_sql(latest_usage_cte_sql(), "1 = 1");
    assert!(
        sql.contains("ORDER BY a.sort ASC, a.updated_at DESC, a.id ASC"),
        "gateway candidate SQL must declare the final id tie-breaker: {sql}"
    );
}

#[test]
fn list_gateway_candidates_for_accounts_filters_requested_available_accounts() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut first = sample_account("acc-first", "active", now);
    first.sort = 1;
    let mut second = sample_account("acc-second", "active", now);
    second.sort = 0;
    let mut saturated = sample_account("acc-saturated", "active", now);
    saturated.sort = 2;
    let disabled = sample_account("acc-disabled", "disabled", now);
    let unrequested = sample_account("acc-unrequested", "active", now);

    for account in [&first, &second, &saturated, &disabled, &unrequested] {
        storage.insert_account(account).expect("insert account");
        storage
            .insert_token(&sample_token(account.id.as_str(), now))
            .expect("insert token");
    }

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: saturated.id.clone(),
            used_percent: Some(100.0),
            window_minutes: Some(180),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert saturated usage");

    let requested = vec![
        "acc-first".to_string(),
        "acc-disabled".to_string(),
        "acc-missing".to_string(),
        "acc-second".to_string(),
        "acc-saturated".to_string(),
    ];
    let candidates = storage
        .list_gateway_candidates_for_accounts(&requested)
        .expect("list selected gateway candidates");

    assert_eq!(
        candidates
            .into_iter()
            .map(|(account, _token)| account.id)
            .collect::<Vec<_>>(),
        vec!["acc-second".to_string(), "acc-first".to_string()]
    );
}

#[test]
fn gateway_candidates_for_accounts_scope_latest_usage_cte_to_requested_ids() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let latest_usage_cte =
        latest_usage_cte_sql_for_condition("account_id IN ('acc-first', 'acc-second')");
    let availability_clause = gateway_account_usage_filter_clause("a", "lu");
    let gateway_sql = gateway_candidates_filtered_sql(
        &latest_usage_cte,
        &format!("a.id IN ('acc-first', 'acc-second') AND {availability_clause}"),
    );
    let sql = format!("EXPLAIN QUERY PLAN {gateway_sql}");
    let plan = collect_query_plan(&storage, &sql);

    assert!(
        plan.contains("idx_usage_snapshots_account_captured_id"),
        "expected account-scoped latest usage CTE to use account lookup index, got {plan}"
    );
}

#[test]
fn find_account_with_token_by_id_returns_joined_account_and_token() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut account = sample_account("acc-current-with-token", "active", now);
    account.label = "Current Account".to_string();
    storage.insert_account(&account).expect("insert account");
    storage
        .insert_token(&sample_token("acc-current-with-token", now))
        .expect("insert token");
    storage
        .insert_account(&sample_account("acc-without-token", "active", now))
        .expect("insert account without token");

    let (found_account, found_token) = storage
        .find_account_with_token_by_id("acc-current-with-token")
        .expect("find account with token")
        .expect("account with token exists");

    assert_eq!(found_account.id, "acc-current-with-token");
    assert_eq!(found_account.label, "Current Account");
    assert_eq!(found_token.account_id, "acc-current-with-token");
    assert!(storage
        .find_account_with_token_by_id("acc-without-token")
        .expect("find account without token")
        .is_none());
    assert!(storage
        .find_account_with_token_by_id("acc-missing-token-join")
        .expect("find missing account with token")
        .is_none());
}

#[test]
fn find_account_with_token_by_identity_preserves_priority_and_requires_token() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let mut by_id = sample_account("acc-token-by-id", "active", now);
    by_id.chatgpt_account_id = Some("cgpt-token-shared".to_string());
    by_id.workspace_id = Some("ws-token-shared".to_string());
    by_id.updated_at = now;
    let mut by_chatgpt = sample_account("acc-token-by-chatgpt", "active", now);
    by_chatgpt.chatgpt_account_id = Some("cgpt-token-shared".to_string());
    by_chatgpt.workspace_id = Some("ws-token-chatgpt".to_string());
    by_chatgpt.updated_at = now.saturating_add(10);
    let mut by_workspace = sample_account("acc-token-by-workspace", "active", now);
    by_workspace.chatgpt_account_id = Some("cgpt-token-workspace".to_string());
    by_workspace.workspace_id = Some("ws-token-shared".to_string());
    by_workspace.updated_at = now.saturating_add(20);
    let mut without_token = sample_account("acc-token-missing", "active", now);
    without_token.workspace_id = Some("ws-token-missing".to_string());

    for account in [&by_id, &by_chatgpt, &by_workspace, &without_token] {
        storage.insert_account(account).expect("insert account");
    }
    for account_id in [
        "acc-token-by-id",
        "acc-token-by-chatgpt",
        "acc-token-by-workspace",
    ] {
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
    }

    let (account, token) = storage
        .find_account_with_token_by_identity(
            Some("acc-token-by-id"),
            Some("cgpt-token-shared"),
            Some("ws-token-shared"),
        )
        .expect("find by id")
        .expect("id match exists");
    assert_eq!(account.id, "acc-token-by-id");
    assert_eq!(token.account_id, "acc-token-by-id");

    let (account, _) = storage
        .find_account_with_token_by_identity(
            None,
            Some("cgpt-token-shared"),
            Some("ws-token-shared"),
        )
        .expect("find by chatgpt")
        .expect("chatgpt match exists");
    assert_eq!(account.id, "acc-token-by-chatgpt");

    let (account, _) = storage
        .find_account_with_token_by_identity(None, None, Some("ws-token-shared"))
        .expect("find by workspace")
        .expect("workspace match exists");
    assert_eq!(account.id, "acc-token-by-workspace");

    assert!(storage
        .find_account_with_token_by_identity(None, None, Some("ws-token-missing"))
        .expect("find account without token")
        .is_none());
}

#[test]
fn delete_accounts_removes_accounts_and_dependent_rows_in_one_call() {
    let mut storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-delete-a", "acc-delete-b", "acc-keep"] {
        storage
            .insert_account(&sample_account(account_id, "active", now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: account_id.to_string(),
                used_percent: Some(42.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: Some("{}".to_string()),
                captured_at: now,
            })
            .expect("insert usage snapshot");
        storage
            .insert_event(&Event {
                account_id: Some(account_id.to_string()),
                event_type: "test".to_string(),
                message: "event".to_string(),
                created_at: now,
            })
            .expect("insert event");
        storage
            .upsert_conversation_binding(&ConversationBinding {
                platform_key_hash: format!("hash-{account_id}"),
                conversation_id: format!("conversation-{account_id}"),
                account_id: account_id.to_string(),
                thread_epoch: 1,
                thread_anchor: String::new(),
                status: "active".to_string(),
                last_model: None,
                last_switch_reason: None,
                created_at: now,
                updated_at: now,
                last_used_at: now,
            })
            .expect("insert conversation binding");
        storage
            .upsert_model_source_model(&ModelSourceModel {
                source_kind: "openai_account".to_string(),
                source_id: account_id.to_string(),
                upstream_model: "gpt-test".to_string(),
                display_name: Some("GPT Test".to_string()),
                status: "available".to_string(),
                discovery_kind: "test".to_string(),
                last_synced_at: Some(now),
                extra_json: "{}".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert model source model");
        storage
            .upsert_model_source_mapping(&ModelSourceMapping {
                id: format!("mapping-{account_id}"),
                platform_model_slug: "gpt-test".to_string(),
                source_kind: "openai_account".to_string(),
                source_id: account_id.to_string(),
                upstream_model: "gpt-test".to_string(),
                enabled: true,
                priority: 1,
                weight: 1,
                billing_model_slug: None,
                created_at: now,
                updated_at: now,
            })
            .expect("insert model source mapping");
        storage
            .upsert_model_source_mapping_preference(
                "openai_account",
                account_id,
                "gpt-test",
                "unlinked",
            )
            .expect("insert model source preference");
    }

    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "aggregate_api".to_string(),
            source_id: "acc-delete-a".to_string(),
            upstream_model: "gpt-test".to_string(),
            display_name: Some("Aggregate API GPT Test".to_string()),
            status: "available".to_string(),
            discovery_kind: "test".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert non-account model source model");

    let deleted = storage
        .delete_accounts(&[
            " acc-delete-a ".to_string(),
            "acc-delete-a".to_string(),
            "".to_string(),
            "acc-delete-b".to_string(),
            "acc-missing".to_string(),
        ])
        .expect("delete accounts");

    assert_eq!(deleted, 2);
    for deleted_account_id in ["acc-delete-a", "acc-delete-b"] {
        assert!(!storage
            .account_exists(deleted_account_id)
            .expect("deleted account exists check"));
        for (table, column) in [
            ("tokens", "account_id"),
            ("usage_snapshots", "account_id"),
            ("events", "account_id"),
            ("conversation_bindings", "account_id"),
        ] {
            assert_eq!(
                dependent_row_count(&storage, table, column, deleted_account_id),
                0,
                "{table} should not keep rows for deleted account"
            );
        }
        for table in [
            "model_source_mappings",
            "model_source_models",
            "model_source_mapping_preferences",
        ] {
            assert_eq!(
                model_source_account_row_count(&storage, table, deleted_account_id),
                1,
                "{table} is legacy read-only data and must remain untouched"
            );
        }
    }

    assert!(storage
        .account_exists("acc-keep")
        .expect("kept account exists check"));
    for (table, column) in [
        ("tokens", "account_id"),
        ("usage_snapshots", "account_id"),
        ("events", "account_id"),
        ("conversation_bindings", "account_id"),
    ] {
        assert_eq!(
            dependent_row_count(&storage, table, column, "acc-keep"),
            1,
            "{table} should keep rows for retained account"
        );
    }
    for table in [
        "model_source_mappings",
        "model_source_models",
        "model_source_mapping_preferences",
    ] {
        assert_eq!(
            model_source_account_row_count(&storage, table, "acc-keep"),
            1,
            "{table} should keep account source rows for retained account"
        );
    }
    assert_eq!(
        storage
            .conn
            .query_row(
                "SELECT COUNT(1)
                 FROM model_source_models
                 WHERE source_kind = 'aggregate_api'
                   AND source_id = 'acc-delete-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("count non-account model source rows"),
        1
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

    assert!(!storage
        .clear_preferred_account_if("acc-a")
        .expect("clear non-preferred"));
    assert!(storage
        .clear_preferred_account_if("acc-b")
        .expect("clear preferred"));
    assert_eq!(storage.preferred_account_id().expect("no preferred"), None);

    let plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", preferred_account_id_sql()),
    );
    assert!(
        plan.contains("idx_accounts_preferred_updated_at"),
        "expected preferred account lookup to use preferred lookup index, got {plan}"
    );
}
#[test]
fn account_write_helpers_use_primary_key_indexes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    fn assert_account_pk<P: rusqlite::Params>(
        storage: &Storage,
        label: &str,
        sql: &str,
        params: P,
    ) {
        let plan =
            collect_query_plan_with_params(storage, &format!("EXPLAIN QUERY PLAN {sql}"), params);
        assert!(
            plan.contains("sqlite_autoindex_accounts_1") || plan.contains("USING INDEX"),
            "expected {label} to use account primary-key index, got {plan}"
        );
    }

    assert_account_pk(
        &storage,
        "account exists",
        account_exists_sql(),
        rusqlite::params!["acc-a"],
    );
    assert_account_pk(
        &storage,
        "sort update",
        update_account_sort_sql(),
        rusqlite::params![1_i64, 2_i64, "acc-a"],
    );
    assert_account_pk(
        &storage,
        "label update",
        update_account_label_sql(),
        rusqlite::params!["label", 2_i64, "acc-a"],
    );
    assert_account_pk(
        &storage,
        "workspace identity update",
        update_account_workspace_identity_sql(),
        rusqlite::params!["cgpt", "workspace", 2_i64, "acc-a"],
    );
    assert_account_pk(
        &storage,
        "touch updated_at",
        touch_account_updated_at_sql(),
        rusqlite::params![2_i64, "acc-a"],
    );
    assert_account_pk(
        &storage,
        "status update",
        update_account_status_sql(),
        rusqlite::params!["active", 2_i64, "acc-a"],
    );
    assert_account_pk(
        &storage,
        "status changed update",
        update_account_status_if_changed_sql(),
        rusqlite::params!["active", 2_i64, "acc-a"],
    );
    assert_account_pk(
        &storage,
        "account delete",
        delete_account_by_id_sql(),
        rusqlite::params!["acc-a"],
    );
    assert_account_pk(
        &storage,
        "set preferred account",
        set_preferred_account_sql(),
        rusqlite::params![2_i64, "acc-a"],
    );
    assert_account_pk(
        &storage,
        "clear preferred account by id",
        clear_preferred_account_by_id_sql(),
        rusqlite::params![2_i64, "acc-a"],
    );
}

#[test]
fn subject_identity_lookup_separates_accounts_with_shared_workspace() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let mut first = sample_account("user-a::cgpt=team|ws=team", "active", now);
    first.chatgpt_account_id = Some("team".to_string());
    first.workspace_id = Some("team".to_string());
    let mut second = sample_account("user-b::cgpt=team|ws=team", "active", now);
    second.chatgpt_account_id = Some("team".to_string());
    second.workspace_id = Some("team".to_string());
    storage.insert_account(&first).expect("insert first");
    storage.insert_account(&second).expect("insert second");
    storage
        .update_account_subject_identity(&first.id, "user-a")
        .expect("set first subject");
    storage
        .update_account_subject_identity(&second.id, "user-b")
        .expect("set second subject");

    let first_matches = storage
        .list_account_workspace_identities_for_subject("user-a")
        .expect("find first subject");
    let second_matches = storage
        .list_account_workspace_identities_for_subject("user-b")
        .expect("find second subject");

    assert_eq!(first_matches.len(), 1);
    assert_eq!(first_matches[0].id, first.id);
    assert_eq!(second_matches.len(), 1);
    assert_eq!(second_matches[0].id, second.id);
}
