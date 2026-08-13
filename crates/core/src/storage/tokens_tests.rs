use super::*;
use crate::storage::{now_ts, Account};

fn sample_account(id: &str, now: i64) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn sample_token(account_id: &str, now: i64) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: format!("{account_id}.id"),
        access_token: format!("{account_id}.access"),
        refresh_token: format!("{account_id}.refresh"),
        api_key_access_token: Some(format!("{account_id}.api")),
        last_refresh: now,
    }
}

#[test]
fn insert_token_encrypts_database_fields_and_reads_plaintext() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let token = sample_token("acc-encrypted", now);

    storage
        .insert_account(&sample_account("acc-encrypted", now))
        .expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let stored = storage
        .conn
        .query_row(
            "SELECT id_token, access_token, refresh_token, api_key_access_token
             FROM tokens WHERE account_id = ?1",
            ["acc-encrypted"],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .expect("read raw token row");

    for value in [&stored.0, &stored.1, &stored.2, &stored.3] {
        assert!(value.starts_with("cmenc:v1:"));
        assert!(!value.contains("acc-encrypted"));
    }

    let loaded = storage
        .find_token_by_account_id("acc-encrypted")
        .expect("find token")
        .expect("token exists");
    assert_eq!(loaded.id_token, token.id_token);
    assert_eq!(loaded.access_token, token.access_token);
    assert_eq!(loaded.refresh_token, token.refresh_token);
    assert_eq!(loaded.api_key_access_token, token.api_key_access_token);
}

#[test]
fn init_migrates_legacy_plaintext_token_rows() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    storage
        .insert_account(&sample_account("acc-legacy", now))
        .expect("insert account");
    storage
        .conn
        .execute(
            "INSERT INTO tokens (
                account_id, id_token, access_token, refresh_token, api_key_access_token, last_refresh
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                "acc-legacy",
                "legacy-id",
                "legacy-access",
                "legacy-refresh",
                "legacy-api",
                now,
            ),
        )
        .expect("insert legacy token");
    storage
        .conn
        .execute(
            "DELETE FROM schema_migrations WHERE version = '131_encrypt_account_tokens_at_rest'",
            [],
        )
        .expect("reset data migration");
    *storage.applied_migrations.borrow_mut() = None;

    storage.init().expect("migrate tokens");

    let raw_access: String = storage
        .conn
        .query_row(
            "SELECT access_token FROM tokens WHERE account_id = 'acc-legacy'",
            [],
            |row| row.get(0),
        )
        .expect("read migrated row");
    assert!(raw_access.starts_with("cmenc:v1:"));
    assert!(!raw_access.contains("legacy-access"));

    let loaded = storage
        .find_token_by_account_id("acc-legacy")
        .expect("find token")
        .expect("token exists");
    assert_eq!(loaded.id_token, "legacy-id");
    assert_eq!(loaded.access_token, "legacy-access");
    assert_eq!(loaded.refresh_token, "legacy-refresh");
    assert_eq!(loaded.api_key_access_token.as_deref(), Some("legacy-api"));
}

fn collect_query_plan(storage: &Storage, sql: &str) -> String {
    let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
    let mut rows = stmt.query([]).expect("query explain");
    let mut plan = String::new();
    while let Some(row) = rows.next().expect("read explain row") {
        let detail: String = row.get(3).expect("plan detail");
        plan.push_str(&detail);
        plan.push('\n');
    }
    plan
}

fn collect_query_plan_with_params<P>(storage: &Storage, sql: &str, params: P) -> String
where
    P: rusqlite::Params,
{
    let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
    let mut rows = stmt.query(params).expect("query explain");
    let mut plan = String::new();
    while let Some(row) = rows.next().expect("read explain row") {
        let detail: String = row.get(3).expect("plan detail");
        plan.push_str(&detail);
        plan.push('\n');
    }
    plan
}
#[test]
fn list_account_token_plans_for_accounts_reads_only_requested_plan_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b", "acc-c"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
    }

    let requested = vec!["acc-b".to_string(), "missing".to_string()];
    let plans = storage
        .list_account_token_plans_for_accounts(&requested)
        .expect("list token plans");

    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].account_id, "acc-b");
    assert_eq!(plans[0].id_token, "acc-b.id");
    assert_eq!(plans[0].access_token, "acc-b.access");
}

#[test]
fn list_account_token_candidates_reads_only_candidate_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    storage
        .insert_account(&sample_account("acc-a", now))
        .expect("insert account");
    storage
        .insert_token(&sample_token("acc-a", now))
        .expect("insert token");

    storage
        .insert_account(&sample_account("acc-empty", now))
        .expect("insert empty account");
    storage
        .insert_token(&Token {
            account_id: "acc-empty".to_string(),
            access_token: " ".to_string(),
            refresh_token: String::new(),
            ..sample_token("acc-empty", now + 1)
        })
        .expect("insert empty token");

    let candidates = storage
        .list_account_token_candidates()
        .expect("list token candidates");

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].account_id, "acc-a");
    assert!(candidates[0].has_access_token);
    assert!(candidates[0].has_refresh_token);
    assert_eq!(candidates[0].last_refresh, now);
    assert_eq!(candidates[1].account_id, "acc-empty");
    assert!(!candidates[1].has_access_token);
    assert!(!candidates[1].has_refresh_token);
    assert_eq!(candidates[1].last_refresh, now + 1);
}

#[test]
fn list_usable_account_token_candidates_filters_empty_tokens_in_sql() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-ready", "acc-no-access", "acc-no-refresh"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_token(&sample_token("acc-ready", now))
        .expect("insert ready token");
    storage
        .insert_token(&Token {
            account_id: "acc-no-access".to_string(),
            access_token: " ".to_string(),
            ..sample_token("acc-no-access", now + 1)
        })
        .expect("insert no access token");
    storage
        .insert_token(&Token {
            account_id: "acc-no-refresh".to_string(),
            refresh_token: String::new(),
            ..sample_token("acc-no-refresh", now + 2)
        })
        .expect("insert no refresh token");

    let candidates = storage
        .list_usable_account_token_candidates()
        .expect("list usable token candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].account_id, "acc-ready");
    assert!(candidates[0].has_access_token);
    assert!(candidates[0].has_refresh_token);
    assert_eq!(candidates[0].last_refresh, now);
}

#[test]
fn token_account_count_counts_distinct_token_accounts() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
    }

    assert_eq!(
        storage.token_account_count().expect("count token accounts"),
        2
    );
}

#[test]
fn list_account_token_candidates_for_accounts_filters_requested_ids() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b", "acc-c"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
    }

    let requested = vec![
        "acc-c".to_string(),
        "missing".to_string(),
        "acc-c".to_string(),
        " ".to_string(),
    ];
    let candidates = storage
        .list_account_token_candidates_for_accounts(&requested)
        .expect("list token candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].account_id, "acc-c");
    assert!(candidates[0].has_access_token);
    assert!(candidates[0].has_refresh_token);
}

#[test]
fn list_usable_account_token_candidates_for_accounts_filters_requested_and_empty_tokens() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-ready", "acc-no-access", "acc-no-refresh", "acc-other"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
    }
    storage
        .insert_token(&sample_token("acc-ready", now))
        .expect("insert ready token");
    storage
        .insert_token(&Token {
            account_id: "acc-no-access".to_string(),
            access_token: " ".to_string(),
            ..sample_token("acc-no-access", now + 1)
        })
        .expect("insert no access token");
    storage
        .insert_token(&Token {
            account_id: "acc-no-refresh".to_string(),
            refresh_token: String::new(),
            ..sample_token("acc-no-refresh", now + 2)
        })
        .expect("insert no refresh token");
    storage
        .insert_token(&sample_token("acc-other", now + 3))
        .expect("insert unrequested token");

    let requested = vec![
        "acc-no-access".to_string(),
        "acc-ready".to_string(),
        "missing".to_string(),
        "acc-no-refresh".to_string(),
        "acc-ready".to_string(),
        " ".to_string(),
    ];
    let candidates = storage
        .list_usable_account_token_candidates_for_accounts(&requested)
        .expect("list usable token candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].account_id, "acc-ready");
    assert!(candidates[0].has_access_token);
    assert!(candidates[0].has_refresh_token);
}

#[test]
fn list_account_token_candidates_for_accounts_chunks_large_account_sets() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let target = "acc-0949";
    storage
        .insert_account(&sample_account(target, now))
        .expect("insert target account");
    storage
        .insert_token(&sample_token(target, now))
        .expect("insert target token");

    let requested = (0..950)
        .map(|index| format!("acc-{index:04}"))
        .collect::<Vec<_>>();
    let candidates = storage
        .list_account_token_candidates_for_accounts(&requested)
        .expect("list token candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].account_id, target);
}

#[test]
fn account_token_full_list_helpers_use_primary_key_ordering() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    for sql in [
        account_token_candidates_sql(),
        usable_account_token_candidates_sql(),
        account_import_token_subjects_sql(),
    ] {
        let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));
        assert!(
            plan.contains("sqlite_autoindex_tokens_1") || plan.contains("USING INDEX"),
            "expected full token list query to scan token primary-key order, got {plan}"
        );
        assert!(
            !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
            "full token list query should avoid ORDER BY temp sorting, got {plan}"
        );
    }
}

#[test]
fn token_lookup_helpers_use_primary_key_indexes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let token_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", token_by_account_sql()),
        ["acc-a"],
    );
    assert!(
        token_plan.contains("sqlite_autoindex_tokens_1") || token_plan.contains("USING INDEX"),
        "expected token lookup by account primary key, got {token_plan}"
    );

    let account_count_plan = collect_query_plan(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", token_account_count_sql()),
    );
    assert!(
        account_count_plan.contains("sqlite_autoindex_tokens_1")
            || account_count_plan.contains("USING COVERING INDEX"),
        "expected token account count to use account primary key, got {account_count_plan}"
    );
}
#[test]
fn list_account_token_candidates_for_accounts_uses_account_lookup_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let sql = account_token_candidates_for_accounts_chunk_sql("account_id IN ('acc-a')");
    let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));

    assert!(
        plan.contains("sqlite_autoindex_tokens_1") || plan.contains("USING INDEX"),
        "expected token account lookup index in plan, got {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "token candidate chunk query should avoid per-chunk ORDER BY temp sorting, got {plan}"
    );
}

#[test]
fn list_tokens_for_accounts_uses_account_lookup_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let sql = tokens_for_accounts_chunk_sql("account_id IN ('acc-a')");
    let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));

    assert!(
        plan.contains("sqlite_autoindex_tokens_1") || plan.contains("USING INDEX"),
        "expected token row lookup by account index in plan, got {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "token row chunk query should avoid per-chunk ORDER BY temp sorting, got {plan}"
    );
}

#[test]
fn list_account_token_plans_for_accounts_uses_account_lookup_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let sql = account_token_plans_for_accounts_chunk_sql("account_id IN ('acc-a')");
    let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));

    assert!(
        plan.contains("sqlite_autoindex_tokens_1") || plan.contains("USING INDEX"),
        "expected token plan lookup by account index in plan, got {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "token plan chunk query should avoid per-chunk ORDER BY temp sorting, got {plan}"
    );
}

#[test]
fn list_usable_account_token_candidates_for_accounts_uses_account_lookup_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let sql = usable_account_token_candidates_for_accounts_chunk_sql("account_id IN ('acc-a')");
    let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));

    assert!(
        plan.contains("sqlite_autoindex_tokens_1") || plan.contains("USING INDEX"),
        "expected token account lookup index in plan, got {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "usable token candidate chunk query should avoid per-chunk ORDER BY temp sorting, got {plan}"
    );
}

#[test]
fn list_tokens_due_for_refresh_uses_due_order_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let sql = tokens_due_for_refresh_sql();
    let mut stmt = storage
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("prepare explain");
    let mut rows = stmt
        .query((100_i64, 100_i64, 10_i64))
        .expect("query explain");
    let mut plan = String::new();
    while let Some(row) = rows.next().expect("read explain row") {
        let detail: String = row.get(3).expect("plan detail");
        plan.push_str(&detail);
        plan.push('\n');
    }

    assert!(
        sql.contains("FROM tokens target_tokens"),
        "expected latest status CTE to scope events through due tokens, got {sql}"
    );
    assert!(
        plan.contains("idx_tokens_refresh_due_order"),
        "expected token refresh due order index in plan, got {plan}"
    );
    assert!(
        plan.contains("idx_events_account_status_lookup"),
        "expected latest status CTE to use event status lookup index, got {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "expected refresh due query to avoid a temp sort, got {plan}"
    );
}

#[test]
fn list_tokens_due_for_refresh_short_circuits_zero_limit() {
    let storage = Storage::open_in_memory().expect("open storage");

    let tokens = storage
        .list_tokens_due_for_refresh(100, 200, 0)
        .expect("zero limit should not query storage");

    assert!(tokens.is_empty());
}

#[test]
fn list_account_import_token_subjects_reads_only_subject_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-b", "acc-a"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
    }

    let subjects = storage
        .list_account_import_token_subjects()
        .expect("list import token subjects");

    assert_eq!(subjects.len(), 2);
    assert_eq!(subjects[0].account_id, "acc-a");
    assert_eq!(subjects[0].id_token, "acc-a.id");
    assert_eq!(subjects[0].access_token, "acc-a.access");
    assert_eq!(subjects[0].refresh_token, "acc-a.refresh");
    assert_eq!(subjects[1].account_id, "acc-b");
}

#[test]
fn list_tokens_for_accounts_filters_requested_ids() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    for account_id in ["acc-a", "acc-b", "acc-c"] {
        storage
            .insert_account(&sample_account(account_id, now))
            .expect("insert account");
        storage
            .insert_token(&sample_token(account_id, now))
            .expect("insert token");
    }

    let requested = vec!["acc-c".to_string(), "missing".to_string()];
    let tokens = storage
        .list_tokens_for_accounts(&requested)
        .expect("list tokens");

    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].account_id, "acc-c");
    assert_eq!(tokens[0].refresh_token, "acc-c.refresh");
}

#[test]
fn list_account_token_plans_for_accounts_chunks_large_account_sets() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();

    let target = "acc-0949";
    storage
        .insert_account(&sample_account(target, now))
        .expect("insert target account");
    storage
        .insert_token(&sample_token(target, now))
        .expect("insert target token");

    let requested = (0..950)
        .map(|index| format!("acc-{index:04}"))
        .collect::<Vec<_>>();
    let plans = storage
        .list_account_token_plans_for_accounts(&requested)
        .expect("list token plans");

    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].account_id, target);
    assert_eq!(plans[0].id_token, "acc-0949.id");
    assert_eq!(plans[0].access_token, "acc-0949.access");
}
#[test]
fn token_delete_uses_primary_key_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", delete_token_for_account_sql()),
        ["acc-a"],
    );

    assert!(
        plan.contains("sqlite_autoindex_tokens_1") || plan.contains("USING INDEX"),
        "token delete should use account_id primary-key index, got {plan}"
    );
}
