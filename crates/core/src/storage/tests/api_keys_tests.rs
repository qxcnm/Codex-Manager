use super::{api_key_summaries_for_ids_chunk_sql, api_keys_for_ids_chunk_sql, ApiKey, Storage};
use crate::storage::ApiKeyOwner;

/// 函数 `make_test_api_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// - index: 参数 index
///
/// # 返回
/// 返回函数执行结果
fn make_test_api_key(index: usize) -> ApiKey {
    ApiKey {
        id: format!("key-{index:04}"),
        name: Some(format!("Key {index}")),
        model_slug: Some("gpt-5".to_string()),
        reasoning_effort: Some("medium".to_string()),
        service_tier: Some("priority".to_string()),
        rotation_strategy: "account_rotation".to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: "codex".to_string(),
        protocol_type: "openai_compat".to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("hash-{index:04}"),
        status: "active".to_string(),
        created_at: index as i64,
        last_used_at: Some(index as i64),
    }
}

fn seed_app_user(storage: &Storage, user_id: &str) {
    storage
        .conn
        .execute(
            "INSERT INTO app_users (
                id, username, display_name, password_hash, role, status,
                created_at, updated_at, last_login_at
             ) VALUES (?1, ?2, NULL, 'hash', 'member', 'active', 1, 1, NULL)",
            (user_id, format!("{user_id}@example.com")),
        )
        .expect("seed app user");
}

/// 函数 `large_key_sets_are_chunked_for_api_key_and_quota_queries`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn large_key_sets_are_chunked_for_api_key_and_quota_queries() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut selected = Vec::new();
    for index in 0..901 {
        let key = make_test_api_key(index);
        selected.push(key.id.clone());
        storage.insert_api_key(&key).expect("insert api key");
        storage
            .upsert_api_key_quota_limit(&key.id, Some(1000 + index as i64))
            .expect("insert quota limit");
    }

    let requested = selected.iter().rev().cloned().collect::<Vec<_>>();
    let keys = storage
        .list_api_keys_for_ids(&requested)
        .expect("list api keys");
    assert_eq!(keys.len(), selected.len());
    assert_eq!(keys.first().map(|item| item.id.as_str()), Some("key-0900"));
    assert_eq!(keys.last().map(|item| item.id.as_str()), Some("key-0000"));

    let quota_limits = storage
        .list_api_key_quota_limits_for_ids(&requested)
        .expect("list quota limits");
    assert_eq!(quota_limits.len(), selected.len());
    assert_eq!(quota_limits.get("key-0000"), Some(&1000));
    assert_eq!(quota_limits.get("key-0900"), Some(&1900));
}

#[test]
fn update_api_key_last_used_at_by_id_updates_recent_call_time() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let mut key = make_test_api_key(1);
    key.last_used_at = None;
    storage.insert_api_key(&key).expect("insert api key");

    storage
        .update_api_key_last_used_at_by_id(&key.id, 12345)
        .expect("touch last used");

    let loaded = storage
        .find_api_key_by_id(&key.id)
        .expect("load api key")
        .expect("api key exists");
    assert_eq!(loaded.last_used_at, Some(12345));
}

#[test]
fn api_key_account_group_filter_migrates_defaults_updates_and_clears() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let migration_count: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE version = ?1",
            ["123_api_keys_account_group_filter"],
            |row| row.get(0),
        )
        .expect("query migration marker");
    assert_eq!(migration_count, 1);

    let key = make_test_api_key(1);
    storage
        .insert_api_key(&key)
        .expect("insert legacy-shaped key");
    assert_eq!(
        storage
            .find_api_key_account_group_filter(&key.id)
            .expect("read default group filter"),
        None
    );

    storage
        .update_api_key_account_group_filter(&key.id, Some("TEAM_A"))
        .expect("set group filter");
    assert_eq!(
        storage
            .find_api_key_account_group_filter(&key.id)
            .expect("read group filter")
            .as_deref(),
        Some("TEAM_A")
    );

    let summaries = storage
        .list_api_key_summaries()
        .expect("list key summaries");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].account_group_filter.as_deref(), Some("TEAM_A"));

    storage
        .insert_api_key(&key)
        .expect("update legacy-shaped key without clearing group filter");
    assert_eq!(
        storage
            .find_api_key_account_group_filter(&key.id)
            .expect("read preserved group filter")
            .as_deref(),
        Some("TEAM_A")
    );

    storage
        .update_api_key_account_group_filter(&key.id, None)
        .expect("clear group filter");
    assert_eq!(
        storage
            .find_api_key_account_group_filter(&key.id)
            .expect("read cleared group filter"),
        None
    );
}

#[test]
fn large_key_sets_are_chunked_for_api_key_summary_queries() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut selected = Vec::new();
    for index in 0..901 {
        let key = make_test_api_key(index);
        selected.push(key.id.clone());
        storage.insert_api_key(&key).expect("insert api key");
    }

    let requested = selected.iter().rev().cloned().collect::<Vec<_>>();
    let summaries = storage
        .list_api_key_summaries_for_ids(&requested)
        .expect("list api key summaries");

    assert_eq!(summaries.len(), selected.len());
    assert_eq!(
        summaries.first().map(|item| item.id.as_str()),
        Some("key-0900")
    );
    assert_eq!(
        summaries.last().map(|item| item.id.as_str()),
        Some("key-0000")
    );
    assert_eq!(summaries[0].name.as_deref(), Some("Key 900"));
    assert_eq!(summaries[0].model_slug.as_deref(), Some("gpt-5"));
}

#[test]
fn api_key_id_chunk_queries_defer_final_ordering_to_rust() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let full_key_plan = collect_query_plan(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            api_keys_for_ids_chunk_sql("k.id IN ('key-a', 'key-b')")
        ),
    );
    assert!(
        full_key_plan.contains("sqlite_autoindex_api_keys_1")
            || full_key_plan.contains("USING INDEX"),
        "API key chunk query should use key id lookup index, got {full_key_plan}"
    );
    assert!(
        !full_key_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "API key chunk query should avoid per-chunk ORDER BY temp sorting, got {full_key_plan}"
    );

    let summary_plan = collect_query_plan(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            api_key_summaries_for_ids_chunk_sql("k.id IN ('key-a', 'key-b')")
        ),
    );
    assert!(
        summary_plan.contains("sqlite_autoindex_api_keys_1")
            || summary_plan.contains("USING INDEX"),
        "API key summary chunk query should use key id lookup index, got {summary_plan}"
    );
    assert!(
        !summary_plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "API key summary chunk query should avoid per-chunk ORDER BY temp sorting, got {summary_plan}"
    );
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
fn api_key_summaries_read_quota_list_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut key = make_test_api_key(1);
    key.model_slug = Some("gpt-profile-model".to_string());
    key.rotation_strategy = "hybrid_rotation".to_string();
    key.account_plan_filter = Some("pro".to_string());
    key.client_type = "claude_code".to_string();
    key.protocol_type = "anthropic_native".to_string();
    key.auth_scheme = "x_api_key".to_string();
    key.upstream_base_url = Some("https://api.example.test".to_string());
    key.static_headers_json = Some(r#"{"anthropic-version":"2023-06-01"}"#.to_string());
    key.status = "disabled".to_string();
    storage.insert_api_key(&key).expect("insert api key");
    storage
        .upsert_api_key_quota_limit(&key.id, Some(1234))
        .expect("insert quota limit");

    let summaries = storage
        .list_api_key_summaries()
        .expect("list api key summaries");

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, key.id);
    assert_eq!(summaries[0].name.as_deref(), Some("Key 1"));
    assert_eq!(
        summaries[0].model_slug.as_deref(),
        Some("gpt-profile-model")
    );
    assert_eq!(summaries[0].reasoning_effort.as_deref(), Some("medium"));
    assert_eq!(summaries[0].service_tier.as_deref(), Some("priority"));
    assert_eq!(summaries[0].rotation_strategy, "hybrid_rotation");
    assert_eq!(summaries[0].account_plan_filter.as_deref(), Some("pro"));
    assert_eq!(summaries[0].client_type, "claude_code");
    assert_eq!(summaries[0].protocol_type, "anthropic_native");
    assert_eq!(summaries[0].auth_scheme, "x_api_key");
    assert_eq!(
        summaries[0].upstream_base_url.as_deref(),
        Some("https://api.example.test")
    );
    assert_eq!(
        summaries[0].static_headers_json.as_deref(),
        Some(r#"{"anthropic-version":"2023-06-01"}"#)
    );
    assert_eq!(summaries[0].status, "disabled");
    assert_eq!(summaries[0].quota_limit_tokens, Some(1234));
    assert_eq!(summaries[0].last_used_at, Some(1));
}

#[test]
fn api_key_quota_summaries_read_minimal_quota_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut first = make_test_api_key(1);
    first.id = "key-old".to_string();
    first.name = Some("Old key".to_string());
    first.model_slug = Some("gpt-profile-old".to_string());
    first.created_at = 100;
    first.last_used_at = Some(110);
    let mut second = make_test_api_key(2);
    second.id = "key-new".to_string();
    second.name = Some("New key".to_string());
    second.model_slug = Some("gpt-profile-new".to_string());
    second.status = "disabled".to_string();
    second.created_at = 200;
    second.last_used_at = Some(210);
    storage.insert_api_key(&first).expect("insert first key");
    storage.insert_api_key(&second).expect("insert second key");
    storage
        .upsert_api_key_quota_limit(&second.id, Some(4096))
        .expect("seed quota limit");

    let summaries = storage
        .list_api_key_quota_summaries()
        .expect("list api key quota summaries");

    assert_eq!(
        summaries
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["key-new", "key-old"]
    );
    assert_eq!(summaries[0].name.as_deref(), Some("New key"));
    assert_eq!(summaries[0].model_slug.as_deref(), Some("gpt-profile-new"));
    assert_eq!(summaries[0].status, "disabled");
    assert_eq!(summaries[0].quota_limit_tokens, Some(4096));
    assert_eq!(summaries[0].last_used_at, Some(210));
    assert_eq!(summaries[1].quota_limit_tokens, None);
}

#[test]
fn api_key_codex_profile_candidates_read_minimal_active_profile_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut active = make_test_api_key(1);
    active.id = "key-active-profile".to_string();
    active.name = Some("Active profile".to_string());
    active.model_slug = Some("fallback-model".to_string());
    active.reasoning_effort = Some("low".to_string());
    active.rotation_strategy = "hybrid_rotation".to_string();
    active.created_at = 100;
    let mut disabled = make_test_api_key(2);
    disabled.id = "key-disabled-profile".to_string();
    disabled.status = "disabled".to_string();
    disabled.created_at = 200;
    storage.insert_api_key(&active).expect("insert active key");
    storage
        .insert_api_key(&disabled)
        .expect("insert disabled key");
    storage
        .conn
        .execute(
            "UPDATE api_key_profiles
             SET default_model = 'profile-model',
                 reasoning_effort = 'high'
             WHERE key_id = 'key-active-profile'",
            [],
        )
        .expect("update profile");

    let candidates = storage
        .list_api_key_codex_profile_candidates()
        .expect("list codex profile candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "key-active-profile");
    assert_eq!(candidates[0].name.as_deref(), Some("Active profile"));
    assert_eq!(candidates[0].model_slug.as_deref(), Some("profile-model"));
    assert_eq!(candidates[0].reasoning_effort.as_deref(), Some("high"));
    assert_eq!(candidates[0].rotation_strategy, "hybrid_rotation");
    assert_eq!(candidates[0].status, "active");
}

#[test]
fn api_key_summaries_for_user_join_owners_and_keep_summary_fields() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    seed_app_user(&storage, "user-1");
    seed_app_user(&storage, "user-2");

    let mut first = make_test_api_key(1);
    first.id = "key-owned-old".to_string();
    first.name = Some("Owned Old".to_string());
    first.created_at = 100;
    let mut second = make_test_api_key(2);
    second.id = "key-owned-new".to_string();
    second.name = Some("Owned New".to_string());
    second.created_at = 200;
    let mut other = make_test_api_key(3);
    other.id = "key-other".to_string();
    other.created_at = 300;
    for key in [&first, &second, &other] {
        storage.insert_api_key(key).expect("insert api key");
    }
    for owner in [
        ApiKeyOwner {
            key_id: first.id.clone(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-1".to_string()),
            project_id: None,
            updated_at: 1,
        },
        ApiKeyOwner {
            key_id: second.id.clone(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-1".to_string()),
            project_id: None,
            updated_at: 2,
        },
        ApiKeyOwner {
            key_id: other.id.clone(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-2".to_string()),
            project_id: None,
            updated_at: 3,
        },
    ] {
        storage.upsert_api_key_owner(&owner).expect("seed owner");
    }
    storage
        .upsert_api_key_quota_limit(&second.id, Some(2048))
        .expect("seed quota");

    let summaries = storage
        .list_api_key_summaries_for_user(" user-1 ")
        .expect("list user api key summaries");

    assert_eq!(
        summaries
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["key-owned-new", "key-owned-old"]
    );
    assert_eq!(summaries[0].name.as_deref(), Some("Owned New"));
    assert_eq!(summaries[0].quota_limit_tokens, Some(2048));
    assert!(storage
        .list_api_key_summaries_for_user("   ")
        .expect("blank user")
        .is_empty());
}

#[test]
fn api_key_summaries_for_user_use_owner_lookup_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut stmt = storage
        .conn
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            super::api_key_summary_for_user_sql()
        ))
        .expect("prepare explain");
    let mut rows = stmt.query(["user-1"]).expect("query explain");
    let mut plan = String::new();
    while let Some(row) = rows.next().expect("read explain row") {
        let detail: String = row.get(3).expect("plan detail");
        plan.push_str(&detail);
        plan.push('\n');
    }

    assert!(
        plan.contains("idx_api_key_owners_user_key_lookup"),
        "expected user key owner lookup index in plan, got {plan}"
    );
}

#[test]
fn api_key_base_lists_use_created_id_order_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut stmt = storage
        .conn
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            super::api_key_summary_list_sql()
        ))
        .expect("prepare explain");
    let mut rows = stmt.query([]).expect("query explain");
    let mut plan = String::new();
    while let Some(row) = rows.next().expect("read explain row") {
        let detail: String = row.get(3).expect("plan detail");
        plan.push_str(&detail);
        plan.push('\n');
    }

    assert!(
        plan.contains("idx_api_keys_list_order"),
        "expected API key list order index in plan, got {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
        "expected API key list query to avoid a temp sort, got {plan}"
    );
}

#[test]
fn api_key_specialized_lists_use_created_id_order_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    for sql in [
        super::api_key_quota_summary_list_sql(),
        super::api_key_codex_profile_candidate_list_sql(),
    ] {
        let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));
        assert!(
            plan.contains("idx_api_keys_list_order"),
            "expected API key list order index in plan, got {plan}"
        );
        assert!(
            !plan.contains("USE TEMP B-TREE FOR ORDER BY"),
            "expected specialized API key list query to avoid a temp sort, got {plan}"
        );
    }
}

#[test]
fn api_key_lookup_helpers_use_expected_indexes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let by_hash_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", super::api_key_by_hash_sql()),
        ["hash-0001"],
    );
    assert!(
        by_hash_plan.contains("idx_api_keys_key_hash"),
        "expected API key hash lookup index in plan, got {by_hash_plan}"
    );

    let by_id_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", super::api_key_by_id_sql()),
        ["key-0001"],
    );
    assert!(
        by_id_plan.contains("sqlite_autoindex_api_keys_1"),
        "expected API key primary-key lookup index in plan, got {by_id_plan}"
    );

    let status_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", super::api_key_status_by_id_sql()),
        ["key-0001"],
    );
    assert!(
        status_plan.contains("sqlite_autoindex_api_keys_1"),
        "expected status lookup to use API key primary key, got {status_plan}"
    );

    let exists_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", super::api_key_exists_sql()),
        ["key-0001"],
    );
    assert!(
        exists_plan.contains("sqlite_autoindex_api_keys_1"),
        "expected exists lookup to use API key primary key, got {exists_plan}"
    );

    let hash_exists_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", super::api_key_hash_exists_sql()),
        ["hash-0001"],
    );
    assert!(
        hash_exists_plan.contains("idx_api_keys_key_hash"),
        "expected hash exists lookup to use hash index, got {hash_exists_plan}"
    );

    let touch_last_used_plan = collect_query_plan(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            super::api_key_touch_last_used_by_hash_sql()
                .replace("?1", "1")
                .replace("?2", "'hash-0001'")
        ),
    );
    assert!(
        touch_last_used_plan.contains("idx_api_keys_key_hash"),
        "expected last-used hash update to use hash index, got {touch_last_used_plan}"
    );

    let id_update_plans = [
        (
            "status update",
            super::api_key_update_status_by_id_sql()
                .replace("?1", "'active'")
                .replace("?2", "'key-0001'"),
        ),
        (
            "rotation config update",
            super::api_key_update_rotation_config_by_id_sql()
                .replace("?1", "'account_rotation'")
                .replace("?2", "NULL")
                .replace("?3", "NULL")
                .replace("?4", "'key-0001'"),
        ),
        (
            "name update",
            super::api_key_update_name_by_id_sql()
                .replace("?1", "'Key 1'")
                .replace("?2", "'key-0001'"),
        ),
        (
            "model slug update",
            super::api_key_update_model_slug_by_id_sql()
                .replace("?1", "'gpt-5'")
                .replace("?2", "'key-0001'"),
        ),
        (
            "model config update",
            super::api_key_update_model_config_by_id_sql()
                .replace("?1", "'gpt-5'")
                .replace("?2", "'medium'")
                .replace("?3", "'key-0001'"),
        ),
    ];
    for (label, sql) in id_update_plans {
        let plan = collect_query_plan(&storage, &format!("EXPLAIN QUERY PLAN {sql}"));
        assert!(
            plan.contains("sqlite_autoindex_api_keys_1"),
            "expected {label} to use API key primary key, got {plan}"
        );
    }

    let secret_plan = collect_query_plan_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {}", super::api_key_secret_by_id_sql()),
        ["key-0001"],
    );
    assert!(
        secret_plan.contains("sqlite_autoindex_api_key_secrets_1"),
        "expected secret lookup to use secret primary key, got {secret_plan}"
    );

    let quota_delete_plan = collect_query_plan(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            super::delete_api_key_quota_limit_by_key_sql().replace("?1", "'key-0001'")
        ),
    );
    assert!(
        quota_delete_plan.contains("sqlite_autoindex_api_key_quota_limits_1"),
        "expected quota delete to use quota primary key, got {quota_delete_plan}"
    );

    let secret_delete_plan = collect_query_plan(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            super::delete_api_key_secret_by_id_sql().replace("?1", "'key-0001'")
        ),
    );
    assert!(
        secret_delete_plan.contains("sqlite_autoindex_api_key_secrets_1"),
        "expected secret delete to use secret primary key, got {secret_delete_plan}"
    );

    let api_key_delete_plan = collect_query_plan(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            super::delete_api_key_by_id_sql().replace("?1", "'key-0001'")
        ),
    );
    assert!(
        api_key_delete_plan.contains("sqlite_autoindex_api_keys_1"),
        "expected API key delete to use API key primary key, got {api_key_delete_plan}"
    );

    let gateway_plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            super::api_key_gateway_auth_by_id_sql()
        ),
        ["key-0001"],
    );
    assert!(
        gateway_plan.contains("sqlite_autoindex_api_keys_1"),
        "expected gateway auth lookup to use API key primary key, got {gateway_plan}"
    );
    assert!(
        gateway_plan.contains("sqlite_autoindex_api_key_secrets_1"),
        "expected gateway auth lookup to use secret primary key, got {gateway_plan}"
    );

    let profile_plan = collect_query_plan_with_params(
        &storage,
        &format!(
            "EXPLAIN QUERY PLAN {}",
            super::api_key_profile_config_by_id_sql()
        ),
        ["key-0001"],
    );
    assert!(
        profile_plan.contains("sqlite_autoindex_api_keys_1"),
        "expected profile config lookup to use API key primary key, got {profile_plan}"
    );
    assert!(
        profile_plan.contains("sqlite_autoindex_api_key_profiles_1"),
        "expected profile config lookup to use profile primary key, got {profile_plan}"
    );
}
#[test]
fn api_key_exists_helpers_read_minimal_key_state() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut key = make_test_api_key(42);
    key.name = Some("existence helper".to_string());
    key.client_type = "claude_code".to_string();
    key.protocol_type = "anthropic_native".to_string();
    key.upstream_base_url = Some("https://api.example.test".to_string());
    key.status = " disabled ".to_string();
    storage.insert_api_key(&key).expect("insert api key");

    let status = storage
        .find_api_key_status_by_id("key-0042")
        .expect("find api key status")
        .expect("api key status exists");
    assert_eq!(status.id, "key-0042");
    assert_eq!(status.status, " disabled ");
    assert!(storage
        .find_api_key_status_by_id("key-missing")
        .expect("find missing api key status")
        .is_none());
    assert!(storage.api_key_exists("key-0042").expect("api key exists"));
    assert!(!storage
        .api_key_exists("key-missing")
        .expect("missing api key exists"));
    assert!(storage
        .api_key_hash_exists("hash-0042")
        .expect("api key hash exists"));
    assert!(!storage
        .api_key_hash_exists("hash-missing")
        .expect("missing api key hash exists"));
}

#[test]
fn api_key_gateway_auth_reads_status_and_secret_in_one_query() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut active_key = make_test_api_key(44);
    active_key.status = "active".to_string();
    storage
        .insert_api_key(&active_key)
        .expect("insert active api key");
    storage
        .upsert_api_key_secret("key-0044", "sk-live-test")
        .expect("insert active api key secret");

    let mut disabled_key = make_test_api_key(45);
    disabled_key.status = "disabled".to_string();
    storage
        .insert_api_key(&disabled_key)
        .expect("insert disabled api key");

    let active_auth = storage
        .find_api_key_gateway_auth_by_id("key-0044")
        .expect("find active gateway auth")
        .expect("active gateway auth exists");
    assert_eq!(active_auth.id, "key-0044");
    assert_eq!(active_auth.status, "active");
    assert_eq!(active_auth.secret.as_deref(), Some("sk-live-test"));

    let disabled_auth = storage
        .find_api_key_gateway_auth_by_id("key-0045")
        .expect("find disabled gateway auth")
        .expect("disabled gateway auth exists");
    assert_eq!(disabled_auth.id, "key-0045");
    assert_eq!(disabled_auth.status, "disabled");
    assert!(disabled_auth.secret.is_none());

    assert!(storage
        .find_api_key_gateway_auth_by_id("key-missing")
        .expect("find missing gateway auth")
        .is_none());
}

#[test]
fn api_key_profile_config_reads_update_profile_fields_only() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut key = make_test_api_key(43);
    key.name = Some("ignored name".to_string());
    key.model_slug = Some("ignored-model".to_string());
    key.reasoning_effort = Some("ignored-reasoning".to_string());
    key.client_type = "claude_code".to_string();
    key.protocol_type = "anthropic_native".to_string();
    key.auth_scheme = "x_api_key".to_string();
    key.upstream_base_url = Some("https://anthropic.example.test".to_string());
    key.static_headers_json = Some(r#"{"anthropic-version":"2023-06-01"}"#.to_string());
    key.service_tier = Some("priority".to_string());
    storage.insert_api_key(&key).expect("insert api key");

    let config = storage
        .find_api_key_profile_config_by_id("key-0043")
        .expect("find profile config")
        .expect("profile config exists");

    assert_eq!(config.protocol_type, "anthropic_native");
    assert_eq!(
        config.upstream_base_url.as_deref(),
        Some("https://anthropic.example.test")
    );
    assert_eq!(
        config.static_headers_json.as_deref(),
        Some(r#"{"anthropic-version":"2023-06-01"}"#)
    );
    assert_eq!(config.service_tier.as_deref(), Some("priority"));
    assert!(storage
        .find_api_key_profile_config_by_id("key-missing")
        .expect("find missing profile config")
        .is_none());
}

#[test]
fn api_key_quota_overview_stats_sums_limits_after_live_hourly_and_legacy_usage() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    for index in 1..=3 {
        let key = make_test_api_key(index);
        storage.insert_api_key(&key).expect("insert api key");
    }
    storage
        .upsert_api_key_quota_limit("key-0001", Some(1000))
        .expect("limit key 1");
    storage
        .upsert_api_key_quota_limit("key-0002", Some(500))
        .expect("limit key 2");
    storage
        .upsert_api_key_quota_limit("key-0003", Some(400))
        .expect("limit key 3");

    storage
        .conn
        .execute(
            "INSERT INTO request_token_stats (
                request_log_id, key_id, account_id, model,
                input_tokens, cached_input_tokens, output_tokens, total_tokens,
                reasoning_output_tokens, estimated_cost_usd, created_at
             ) VALUES
                (1, 'key-0001', NULL, 'gpt-5', 100, 0, 50, 150, 0, 0.0, 1),
                (2, 'key-0002', NULL, 'gpt-5', 0, 0, 0, 900, 0, 0.0, 2)",
            [],
        )
        .expect("seed live token stats");
    storage
        .conn
        .execute(
            "INSERT INTO request_token_stat_hourly_rollups (
                bucket_start, bucket_end, key_id, account_id, model, actual_source_kind,
                actual_source_id, owner_user_id, input_tokens, cached_input_tokens,
                output_tokens, total_tokens, reasoning_output_tokens, estimated_cost_usd,
                request_count, success_count, error_count, updated_at
             ) VALUES
                (3600, 7200, 'key-0003', '', 'gpt-5', '', '', '',
                 100, 0, 50, 150, 0, 0.30, 1, 1, 0, 10)",
            [],
        )
        .expect("seed hourly token stats");
    storage
        .conn
        .execute(
            "INSERT INTO request_token_stat_rollups (
                key_id, account_id, model,
                input_tokens, cached_input_tokens, output_tokens, total_tokens,
                reasoning_output_tokens, estimated_cost_usd, source_rows, updated_at
             ) VALUES
                ('key-0001', '', 'gpt-5', 200, 0, 50, 250, 0, 0.0, 1, 10)",
            [],
        )
        .expect("seed rollup token stats");

    let stats = storage
        .api_key_quota_overview_stats()
        .expect("quota overview stats");
    assert_eq!(stats.key_count, 3);
    assert_eq!(stats.limited_key_count, 3);
    assert_eq!(stats.total_limit_tokens, 1900);
    assert_eq!(stats.total_used_tokens, 1450);
    assert_eq!(stats.total_remaining_tokens, 850);
    assert!((stats.estimated_cost_usd - 0.30).abs() < 1e-9);
    assert_eq!(
        storage
            .api_key_remaining_quota_tokens()
            .expect("remaining quota"),
        850
    );
}
