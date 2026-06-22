use rusqlite::{params, params_from_iter, types::Value};

use super::{RequestTokenStat, Storage};
use crate::storage::{ApiKey, ApiKeyOwner, AppUser, RequestLog};

fn collect_query_plan_details_with_params(
    storage: &Storage,
    sql: &str,
    params: Vec<Value>,
) -> Vec<String> {
    let mut stmt = storage.conn.prepare(sql).expect("prepare explain");
    let mut rows = stmt.query(params_from_iter(params)).expect("query explain");
    let mut details = Vec::new();
    while let Some(row) = rows.next().expect("next explain row") {
        let detail: String = row.get(3).expect("detail");
        details.push(detail.to_ascii_lowercase());
    }
    details
}

fn assert_uses_index(details: &[String], index_name: &str, label: &str) {
    assert!(
        details.iter().any(|detail| detail.contains(index_name)),
        "{label} should use {index_name}, got {details:?}"
    );
}

/// 函数 `insert_rollup_row`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// - storage: 参数 storage
/// - key_id: 参数 key_id
/// - account_id: 参数 account_id
/// - model: 参数 model
/// - total_tokens: 参数 total_tokens
/// - estimated_cost_usd: 参数 estimated_cost_usd
/// - updated_at: 参数 updated_at
///
/// # 返回
/// 无
fn insert_rollup_row(
    storage: &Storage,
    key_id: &str,
    account_id: &str,
    model: &str,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    reasoning_output_tokens: i64,
    estimated_cost_usd: f64,
    source_rows: i64,
    updated_at: i64,
) {
    storage
        .conn
        .execute(
            "INSERT INTO request_token_stat_rollups (
                key_id,
                account_id,
                model,
                input_tokens,
                cached_input_tokens,
                output_tokens,
                total_tokens,
                reasoning_output_tokens,
                estimated_cost_usd,
                source_rows,
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                key_id,
                account_id,
                model,
                input_tokens,
                cached_input_tokens,
                output_tokens,
                total_tokens,
                reasoning_output_tokens,
                estimated_cost_usd,
                source_rows,
                updated_at,
            ],
        )
        .expect("insert rollup row");
}

/// 函数 `assert_float_close`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-28
///
/// # 参数
/// - left: 参数 left
/// - right: 参数 right
///
/// # 返回
/// 无
fn assert_float_close(left: f64, right: f64) {
    assert!(
        (left - right).abs() < 1e-9,
        "expected {left} to be close to {right}"
    );
}

fn seed_usage_log(
    storage: &Storage,
    user_id: &str,
    key_id: &str,
    source_kind: &str,
    source_id: &str,
    total_tokens: i64,
    created_at: i64,
) {
    storage
        .insert_app_user(&AppUser {
            id: user_id.to_string(),
            username: format!("{user_id}@example.com"),
            display_name: Some(user_id.to_string()),
            password_hash: "hash".to_string(),
            role: "member".to_string(),
            status: "active".to_string(),
            created_at,
            updated_at: created_at,
            last_login_at: None,
        })
        .expect("insert app user");
    storage
        .insert_api_key(&ApiKey {
            id: key_id.to_string(),
            name: Some(key_id.to_string()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            key_hash: format!("hash-{key_id}"),
            status: "enabled".to_string(),
            rotation_strategy: "account_rotation".to_string(),
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            created_at,
            last_used_at: None,
        })
        .expect("insert api key");
    storage
        .upsert_api_key_owner(&ApiKeyOwner {
            key_id: key_id.to_string(),
            owner_kind: "user".to_string(),
            owner_user_id: Some(user_id.to_string()),
            project_id: None,
            updated_at: created_at,
        })
        .expect("upsert owner");
    storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                key_id: Some(key_id.to_string()),
                account_id: (source_kind == "openai_account").then(|| source_id.to_string()),
                request_path: "/v1/responses".to_string(),
                method: "POST".to_string(),
                model: Some("gpt-5".to_string()),
                actual_source_kind: Some(source_kind.to_string()),
                actual_source_id: Some(source_id.to_string()),
                status_code: Some(200),
                created_at,
                ..Default::default()
            },
            &RequestTokenStat {
                key_id: Some(key_id.to_string()),
                account_id: (source_kind == "openai_account").then(|| source_id.to_string()),
                model: Some("gpt-5".to_string()),
                actual_source_kind: Some(source_kind.to_string()),
                actual_source_id: Some(source_id.to_string()),
                input_tokens: Some(total_tokens),
                total_tokens: Some(total_tokens),
                estimated_cost_usd: Some(total_tokens as f64 / 1000.0),
                created_at,
                ..RequestTokenStat::default()
            },
        )
        .expect("insert request log with token stat");
}

/// 函数 `summaries_for_selected_keys_include_rollups_and_respect_time_ranges`
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
fn summaries_for_selected_keys_include_rollups_and_respect_time_ranges() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    // 明细行和 rollup 行分属不同 key，便于验证过滤是否真的落在数据库层。
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-a".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            input_tokens: Some(10),
            cached_input_tokens: Some(1),
            output_tokens: Some(2),
            total_tokens: Some(12),
            reasoning_output_tokens: Some(3),
            estimated_cost_usd: Some(0.10),
            created_at: 100,
            ..RequestTokenStat::default()
        })
        .expect("insert raw key a");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 2,
            key_id: Some("key-b".to_string()),
            account_id: Some("acc-b".to_string()),
            model: Some("gpt-5-mini".to_string()),
            input_tokens: Some(20),
            cached_input_tokens: Some(0),
            output_tokens: Some(0),
            total_tokens: Some(20),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.20),
            created_at: 110,
            ..RequestTokenStat::default()
        })
        .expect("insert raw key b");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 3,
            key_id: Some("key-c".to_string()),
            account_id: Some("acc-c".to_string()),
            model: Some("gpt-5".to_string()),
            input_tokens: Some(100),
            cached_input_tokens: Some(0),
            output_tokens: Some(0),
            total_tokens: Some(100),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(1.00),
            created_at: 105,
            ..RequestTokenStat::default()
        })
        .expect("insert raw unselected key with same model");

    // Rollup 只写入 key-a，用来验证无时间范围时会把 rollup 一并纳入。
    insert_rollup_row(
        &storage, "key-a", "acc-a", "gpt-5", 5, 0, 0, 5, 0, 0.05, 1, 999,
    );
    insert_rollup_row(
        &storage, "key-c", "acc-c", "gpt-5", 100, 0, 0, 100, 0, 1.00, 1, 999,
    );

    let selected = vec!["key-a".to_string()];
    let by_key = storage
        .summarize_request_token_stats_by_key_for_keys(&selected)
        .expect("summarize by key");
    assert_eq!(by_key.len(), 1);
    assert_eq!(by_key[0].key_id, "key-a");
    assert_eq!(by_key[0].total_tokens, 17);
    assert_float_close(by_key[0].estimated_cost_usd, 0.15);

    let by_model = storage
        .summarize_request_token_stats_by_model_for_keys(None, None, &selected)
        .expect("summarize by model");
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "gpt-5");
    assert_eq!(by_model[0].total_tokens, 17);

    let by_key_and_model = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(Some(90), Some(110), &selected)
        .expect("summarize by key and model");
    assert_eq!(by_key_and_model.len(), 1);
    assert_eq!(by_key_and_model[0].key_id, "key-a");
    assert_eq!(by_key_and_model[0].model, "gpt-5");
    assert_eq!(by_key_and_model[0].total_tokens, 12);
    assert_float_close(by_key_and_model[0].estimated_cost_usd, 0.10);
}

#[test]
fn member_dashboard_usage_breakdown_snapshot_combines_key_and_model_rollups() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 10,
            key_id: Some("key-a".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            input_tokens: Some(10),
            cached_input_tokens: Some(0),
            output_tokens: Some(2),
            total_tokens: Some(12),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.10),
            created_at: 100,
            ..RequestTokenStat::default()
        })
        .expect("insert raw today");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 11,
            key_id: Some("key-a".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5-mini".to_string()),
            input_tokens: Some(20),
            cached_input_tokens: Some(0),
            output_tokens: Some(3),
            total_tokens: Some(23),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.20),
            created_at: 10,
            ..RequestTokenStat::default()
        })
        .expect("insert raw trend");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 12,
            key_id: Some("key-b".to_string()),
            account_id: Some("acc-b".to_string()),
            model: Some("gpt-other".to_string()),
            input_tokens: Some(50),
            cached_input_tokens: Some(0),
            output_tokens: Some(0),
            total_tokens: Some(50),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.50),
            created_at: 100,
            ..RequestTokenStat::default()
        })
        .expect("insert unselected key");

    let snapshot = storage
        .load_member_dashboard_usage_breakdown_snapshot(&["key-a".to_string()], 90, 120, 7, 5)
        .expect("load member usage breakdown snapshot");

    assert_eq!(snapshot.today_key_model_usage.len(), 1);
    assert_eq!(snapshot.today_key_model_usage[0].key_id, "key-a");
    assert_eq!(snapshot.today_key_model_usage[0].model, "gpt-5");
    assert_eq!(snapshot.today_key_model_usage[0].total_tokens, 12);
    assert_eq!(snapshot.total_key_usage.len(), 1);
    assert_eq!(snapshot.total_key_usage[0].key_id, "key-a");
    assert_eq!(snapshot.total_key_usage[0].total_tokens, 35);
    assert_eq!(
        snapshot
            .top_model_usage
            .iter()
            .map(|item| (item.model.as_str(), item.total_tokens))
            .collect::<Vec<_>>(),
        vec![("gpt-5-mini", 23), ("gpt-5", 12)]
    );

    let empty = storage
        .load_member_dashboard_usage_breakdown_snapshot(&[], 90, 120, 7, 5)
        .expect("load empty member usage breakdown snapshot");
    assert!(empty.today_key_model_usage.is_empty());
    assert!(empty.total_key_usage.is_empty());
    assert!(empty.top_model_usage.is_empty());
}

#[test]
fn summaries_for_user_join_owned_keys_across_live_hourly_and_legacy_usage() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    seed_usage_log(
        &storage,
        "user-owned",
        "key-live",
        "openai_account",
        "acc-live",
        15,
        100,
    );
    seed_usage_log(
        &storage,
        "user-other",
        "key-other",
        "openai_account",
        "acc-other",
        900,
        110,
    );
    storage
        .insert_api_key(&ApiKey {
            id: "key-rollup".to_string(),
            name: Some("key-rollup".to_string()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            key_hash: "hash-key-rollup".to_string(),
            status: "enabled".to_string(),
            rotation_strategy: "account_rotation".to_string(),
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            created_at: 120,
            last_used_at: None,
        })
        .expect("insert rollup api key");
    storage
        .upsert_api_key_owner(&ApiKeyOwner {
            key_id: "key-rollup".to_string(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-owned".to_string()),
            project_id: None,
            updated_at: 120,
        })
        .expect("upsert rollup owner");
    storage
        .conn
        .execute(
            "INSERT INTO request_token_stat_hourly_rollups (
                bucket_start, bucket_end, key_id, account_id, model, actual_source_kind,
                actual_source_id, owner_user_id, input_tokens, cached_input_tokens,
                output_tokens, total_tokens, reasoning_output_tokens, estimated_cost_usd,
                request_count, success_count, error_count, updated_at
             ) VALUES
                (3600, 7200, 'key-rollup', '', 'gpt-5', '', '', 'user-owned',
                 20, 0, 5, 25, 0, 0.25, 1, 1, 0, 200)",
            [],
        )
        .expect("insert hourly rollup");
    insert_rollup_row(
        &storage,
        "key-rollup",
        "",
        "gpt-5",
        10,
        0,
        0,
        10,
        0,
        0.10,
        1,
        300,
    );

    let items = storage
        .summarize_request_token_stats_by_key_for_user(" user-owned ")
        .expect("summarize by user");

    assert_eq!(
        items
            .iter()
            .map(|item| (item.key_id.as_str(), item.total_tokens))
            .collect::<Vec<_>>(),
        vec![("key-rollup", 35), ("key-live", 15)]
    );
    assert_float_close(items[0].estimated_cost_usd, 0.35);
    assert!(items.iter().all(|item| item.key_id != "key-other"));
    assert!(storage
        .summarize_request_token_stats_by_key_for_user("   ")
        .expect("blank user")
        .is_empty());
}

#[test]
fn api_key_usage_rollups_merge_legacy_and_hourly_without_double_counting() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    insert_rollup_row(
        &storage,
        "legacy-only",
        "acc-legacy",
        "gpt-legacy",
        10,
        0,
        5,
        15,
        1,
        0.15,
        1,
        100,
    );
    storage
        .conn
        .execute(
            "INSERT INTO request_token_stat_hourly_rollups (
                bucket_start, bucket_end, key_id, account_id, model, actual_source_kind,
                actual_source_id, owner_user_id, input_tokens, cached_input_tokens,
                output_tokens, total_tokens, reasoning_output_tokens, estimated_cost_usd,
                request_count, success_count, error_count, updated_at
             ) VALUES
                (3600, 7200, 'hourly-only', 'acc-hourly', 'gpt-hourly', '', '', '',
                 20, 0, 10, 30, 2, 0.30, 1, 1, 0, 200)",
            [],
        )
        .expect("insert hourly rollups");

    let by_key = storage
        .summarize_request_token_stats_by_key()
        .expect("summarize by key");
    assert_eq!(
        by_key
            .iter()
            .find(|item| item.key_id == "legacy-only")
            .map(|item| item.total_tokens),
        Some(15)
    );
    assert_eq!(
        by_key
            .iter()
            .find(|item| item.key_id == "hourly-only")
            .map(|item| item.total_tokens),
        Some(30)
    );

    let by_model = storage
        .summarize_request_token_stats_by_model(None, None)
        .expect("summarize by model");
    assert_eq!(
        by_model
            .iter()
            .find(|item| item.model == "gpt-legacy")
            .map(|item| item.total_tokens),
        Some(15)
    );
    assert_eq!(
        by_model
            .iter()
            .find(|item| item.model == "gpt-hourly")
            .map(|item| item.total_tokens),
        Some(30)
    );
    let selected_keys = vec!["legacy-only".to_string(), "hourly-only".to_string()];
    let limited_by_model = storage
        .summarize_request_token_stats_by_model_for_keys_limited(
            None,
            None,
            &selected_keys,
            Some(1),
        )
        .expect("summarize limited by model");
    assert_eq!(limited_by_model.len(), 1);
    assert_eq!(limited_by_model[0].model, "gpt-hourly");
    assert_eq!(limited_by_model[0].total_tokens, 30);
    assert!(storage
        .summarize_request_token_stats_by_model_for_keys_limited(
            None,
            None,
            &selected_keys,
            Some(0),
        )
        .expect("zero model limit")
        .is_empty());

    let by_key_model = storage
        .summarize_request_token_stats_by_key_and_model(None, None)
        .expect("summarize by key model");
    assert_eq!(
        by_key_model
            .iter()
            .find(|item| item.key_id == "legacy-only" && item.model == "gpt-legacy")
            .map(|item| item.total_tokens),
        Some(15)
    );
    assert_eq!(
        by_key_model
            .iter()
            .find(|item| item.key_id == "hourly-only" && item.model == "gpt-hourly")
            .map(|item| item.total_tokens),
        Some(30)
    );
}

#[test]
fn dashboard_top_usage_queries_limit_in_sql_and_include_hourly_rollups() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    seed_usage_log(
        &storage,
        "user-low",
        "key-low",
        "openai_account",
        "acc-low",
        10,
        3_700,
    );
    seed_usage_log(
        &storage,
        "user-high",
        "key-high",
        "openai_account",
        "acc-high",
        30,
        3_800,
    );
    seed_usage_log(
        &storage,
        "user-mid",
        "key-mid",
        "aggregate_api",
        "agg-mid",
        20,
        3_900,
    );
    seed_usage_log(
        &storage,
        "user-agg-high",
        "key-agg-high",
        "aggregate_api",
        "agg-high",
        40,
        4_000,
    );

    storage
        .clear_request_logs()
        .expect("roll up and clear logs");

    let top_users = storage
        .summarize_request_token_stats_by_user_between_limited(0, 7_200, Some(2))
        .expect("limited user summary");
    assert_eq!(top_users.len(), 2);
    assert_eq!(top_users[0].user_id, "user-agg-high");
    assert_eq!(top_users[0].usage.total_tokens, 40);
    assert_eq!(top_users[1].user_id, "user-high");
    assert_eq!(top_users[1].usage.total_tokens, 30);

    let top_sources = storage
        .summarize_request_token_stats_by_sources_between_limited(
            &["openai_account", "aggregate_api"],
            0,
            7_200,
            Some(1),
        )
        .expect("limited source summary");
    assert_eq!(top_sources.len(), 2);
    assert!(top_sources.iter().any(|item| {
        item.source_kind == "openai_account"
            && item.source_id == "acc-high"
            && item.usage.total_tokens == 30
    }));
    assert!(top_sources.iter().any(|item| {
        item.source_kind == "aggregate_api"
            && item.source_id == "agg-high"
            && item.usage.total_tokens == 40
    }));

    assert!(storage
        .summarize_request_token_stats_by_user_between_limited(0, 7_200, Some(0))
        .expect("zero user limit")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_sources_between_limited(
            &["openai_account"],
            0,
            7_200,
            Some(0),
        )
        .expect("zero source limit")
        .is_empty());
}

#[test]
fn daily_range_query_matches_created_at_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let sql = super::raw_token_rollup_select(
        "?1 + CAST((t.created_at - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,",
        "t.created_at >= ?1 AND t.created_at < ?2",
        "GROUP BY bucket_start",
        false,
    );
    let details = collect_query_plan_details_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {sql}"),
        vec![
            Value::Integer(0),
            Value::Integer(604800),
            Value::Integer(86400),
        ],
    );
    assert!(details
        .iter()
        .any(|detail| detail.contains("idx_request_token_stats_created_at")));
}

#[test]
fn raw_stat_rollup_maintenance_queries_match_created_at_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let maintenance_queries = [
        (
            "pending rollup exists",
            super::request_token_stats_pending_rollup_exists_sql(),
        ),
        (
            "delete rolled-up raw stats",
            super::delete_request_token_stats_before_sql(),
        ),
    ];

    for (label, sql) in maintenance_queries {
        let details = collect_query_plan_details_with_params(
            &storage,
            &format!("EXPLAIN QUERY PLAN {sql}"),
            vec![Value::Integer(86_400)],
        );
        assert_uses_index(&details, "idx_request_token_stats_created_at", label);
    }
}
#[test]
fn hourly_rollup_report_queries_match_existing_lookup_indexes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let bucket_sql = super::hourly_token_rollup_select("", super::hourly_rollup_range_clause(), "");
    let bucket_details = collect_query_plan_details_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {bucket_sql}"),
        vec![Value::Integer(0), Value::Integer(604800)],
    );
    assert_uses_index(
        &bucket_details,
        "idx_request_token_stat_hourly_rollups_bucket_start",
        "hourly rollup bucket range query",
    );

    let key_sql = super::hourly_key_usage_select(
        "",
        &format!("h.key_id = ?3 AND {}", super::hourly_rollup_range_clause()),
        "GROUP BY key_id",
    );
    let key_details = collect_query_plan_details_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {key_sql}"),
        vec![
            Value::Integer(0),
            Value::Integer(604800),
            Value::Text("key-a".to_string()),
        ],
    );
    assert_uses_index(
        &key_details,
        "idx_request_token_stat_hourly_rollups_key_bucket",
        "hourly rollup key range query",
    );

    let owner_sql = super::hourly_token_rollup_select(
        "h.owner_user_id,",
        &format!(
            "h.owner_user_id = ?3 AND {}",
            super::hourly_rollup_range_clause()
        ),
        "GROUP BY h.owner_user_id",
    );
    let owner_details = collect_query_plan_details_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {owner_sql}"),
        vec![
            Value::Integer(0),
            Value::Integer(604800),
            Value::Text("user-a".to_string()),
        ],
    );
    assert_uses_index(
        &owner_details,
        "idx_request_token_stat_hourly_rollups_owner_bucket",
        "hourly rollup owner range query",
    );

    let source_sql = super::hourly_token_rollup_select(
        "h.actual_source_kind, h.actual_source_id,",
        &format!(
            "h.actual_source_kind = ?3
            AND h.actual_source_id = ?4
            AND {}",
            super::hourly_rollup_range_clause()
        ),
        "GROUP BY h.actual_source_kind, h.actual_source_id",
    );
    let source_details = collect_query_plan_details_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {source_sql}"),
        vec![
            Value::Integer(0),
            Value::Integer(604800),
            Value::Text("openai_account".to_string()),
            Value::Text("acc-a".to_string()),
        ],
    );
    assert_uses_index(
        &source_details,
        "idx_request_token_stat_hourly_rollups_source_bucket",
        "hourly rollup source range query",
    );
}

#[test]
fn by_key_usage_summary_query_includes_raw_hourly_and_legacy_sources() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let key_filter_clauses = super::key_filter_sql_clauses(None);
    let raw = super::raw_key_usage_select(
        "",
        "t.key_id IS NOT NULL AND TRIM(t.key_id) <> ''",
        "GROUP BY t.key_id",
    );
    let hourly = super::hourly_key_usage_select(
        "",
        "NULLIF(TRIM(h.key_id), '') IS NOT NULL",
        "GROUP BY key_id",
    );
    let legacy = super::legacy_key_usage_select(
        "",
        "NULLIF(TRIM(r.key_id), '') IS NOT NULL",
        "GROUP BY key_id",
    );
    let combined_selects = super::union_all_selects([raw, hourly, legacy]);
    let sql = super::request_token_stats_by_key_sql(&combined_selects, &key_filter_clauses);

    let details = collect_query_plan_details_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {sql}"),
        Vec::new(),
    );

    assert_uses_index(
        &details,
        "idx_request_token_stats_key_model_created_at",
        "by-key raw usage summary",
    );
    assert_uses_index(
        &details,
        "idx_request_token_stat_hourly_rollups_key_bucket",
        "by-key hourly usage summary",
    );
    assert_uses_index(
        &details,
        "idx_request_token_stat_rollups_key_id",
        "by-key legacy usage summary",
    );
}

#[test]
fn summarize_request_token_stats_between_short_circuits_empty_range() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let summary = storage
        .summarize_request_token_stats_between(10_000, 10_000)
        .expect("summarize empty token range");

    assert_eq!(summary.input_tokens, 0);
    assert_eq!(summary.cached_input_tokens, 0);
    assert_eq!(summary.output_tokens, 0);
    assert_eq!(summary.reasoning_output_tokens, 0);
    assert_eq!(summary.estimated_cost_usd, 0.0);
}

#[test]
fn daily_usage_query_skips_owner_joins() {
    let sql = super::raw_token_rollup_select(
        "?1 + CAST((t.created_at - ?1) / ?3 AS INTEGER) * ?3 AS bucket_start,",
        "t.created_at >= ?1 AND t.created_at < ?2",
        "GROUP BY bucket_start",
        false,
    );
    assert!(!sql.contains("api_key_owners"));
}

#[test]
fn user_usage_query_keeps_owner_join() {
    let sql = super::raw_token_rollup_select(
        &format!("{} AS user_id,", super::USER_OWNER_EXPR),
        &format!(
            "t.created_at >= ?1 AND t.created_at < ?2 AND {} IS NOT NULL",
            super::USER_OWNER_EXPR
        ),
        "GROUP BY user_id",
        true,
    );
    assert!(sql.contains("api_key_owners"));
}

#[test]
fn rollup_request_token_stats_short_circuits_empty_raw_stats() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let deleted = storage
        .rollup_request_token_stats_before(i64::MAX)
        .expect("roll up empty raw stats");
    assert_eq!(deleted, 0);

    let hourly_rows: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM request_token_stat_hourly_rollups",
            [],
            |row| row.get(0),
        )
        .expect("count hourly rollups");
    assert_eq!(hourly_rows, 0);
}

#[test]
fn clear_request_logs_skips_rollup_work_when_empty() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    storage.clear_request_logs().expect("clear empty logs");

    let hourly_rows: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM request_token_stat_hourly_rollups",
            [],
            |row| row.get(0),
        )
        .expect("count hourly rollups");
    assert_eq!(hourly_rows, 0);
}

#[test]
fn dashboard_rollups_survive_cleared_request_logs() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_app_user(&AppUser {
            id: "user-1".to_string(),
            username: "user-1".to_string(),
            display_name: Some("User 1".to_string()),
            password_hash: "hash".to_string(),
            role: "member".to_string(),
            status: "active".to_string(),
            created_at: 3_700,
            updated_at: 3_700,
            last_login_at: None,
        })
        .expect("insert app user");
    storage
        .insert_api_key(&ApiKey {
            id: "key-owned".to_string(),
            name: Some("Owned key".to_string()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            key_hash: "hash-owned".to_string(),
            status: "enabled".to_string(),
            rotation_strategy: "account_rotation".to_string(),
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            created_at: 3_700,
            last_used_at: None,
        })
        .expect("insert api key");
    storage
        .upsert_api_key_owner(&ApiKeyOwner {
            key_id: "key-owned".to_string(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-1".to_string()),
            project_id: None,
            updated_at: 3_700,
        })
        .expect("upsert owner");
    let request_log_id = storage
        .insert_request_log(&RequestLog {
            key_id: Some("key-owned".to_string()),
            account_id: Some("acc-from-log".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            model: Some("gpt-5".to_string()),
            actual_source_kind: Some("openai_account".to_string()),
            actual_source_id: Some("acc-from-log".to_string()),
            status_code: Some(200),
            created_at: 3_700,
            ..Default::default()
        })
        .expect("insert log");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id,
            key_id: Some("key-owned".to_string()),
            account_id: Some("acc-from-stat".to_string()),
            model: Some("gpt-5".to_string()),
            actual_source_kind: Some("openai_account".to_string()),
            actual_source_id: Some("acc-from-stat".to_string()),
            input_tokens: Some(10),
            cached_input_tokens: Some(1),
            output_tokens: Some(5),
            total_tokens: Some(15),
            reasoning_output_tokens: Some(2),
            estimated_cost_usd: Some(0.25),
            created_at: 3_700,
        })
        .expect("insert openai stat");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: request_log_id + 1,
            key_id: Some("key-owned".to_string()),
            account_id: None,
            model: Some("gpt-5".to_string()),
            actual_source_kind: Some("aggregate_api".to_string()),
            actual_source_id: Some("agg-from-stat".to_string()),
            input_tokens: Some(12),
            cached_input_tokens: Some(2),
            output_tokens: Some(8),
            total_tokens: Some(20),
            reasoning_output_tokens: Some(3),
            estimated_cost_usd: Some(0.35),
            created_at: 3_700,
            ..RequestTokenStat::default()
        })
        .expect("insert aggregate stat");

    storage.clear_request_logs().expect("clear logs");

    let daily = storage
        .summarize_request_token_stats_daily(0, 7_200, 86_400)
        .expect("daily summary");
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].usage.total_tokens, 35);
    assert_eq!(daily[0].usage.request_count, 2);

    let by_user = storage
        .summarize_request_token_stats_by_user_between(0, 7_200)
        .expect("user summary");
    assert_eq!(by_user.len(), 1);
    assert_eq!(by_user[0].user_id, "user-1");
    assert_eq!(by_user[0].usage.total_tokens, 35);
    assert_eq!(by_user[0].usage.request_count, 2);

    let for_user = storage
        .summarize_request_token_stats_for_user_between("user-1", 0, 7_200)
        .expect("single user summary");
    assert_eq!(for_user.total_tokens, 35);
    assert_eq!(for_user.request_count, 2);

    let daily_for_user = storage
        .summarize_request_token_stats_daily_for_user("user-1", 0, 7_200, 86_400)
        .expect("daily user summary");
    assert_eq!(daily_for_user.len(), 1);
    assert_eq!(daily_for_user[0].usage.total_tokens, 35);

    let by_source = storage
        .summarize_request_token_stats_by_source_between("openai_account", 0, 7_200)
        .expect("source summary");
    assert_eq!(by_source.len(), 1);
    assert_eq!(by_source[0].source_id, "acc-from-stat");
    assert_eq!(by_source[0].usage.total_tokens, 15);
    assert_eq!(by_source[0].usage.request_count, 1);

    let aggregate_source = storage
        .summarize_request_token_stats_by_source_between("aggregate_api", 0, 7_200)
        .expect("aggregate source summary");
    assert_eq!(aggregate_source.len(), 1);
    assert_eq!(aggregate_source[0].source_id, "agg-from-stat");
    assert_eq!(aggregate_source[0].usage.total_tokens, 20);
    assert_eq!(aggregate_source[0].usage.request_count, 1);

    let all_sources = storage
        .summarize_request_token_stats_by_sources_between(
            &[
                "aggregate_api",
                "openai_account",
                "openai_account",
                "unknown",
            ],
            0,
            7_200,
        )
        .expect("multi-source summary");
    assert_eq!(all_sources.len(), 2);
    assert!(all_sources.iter().any(|item| {
        item.source_kind == "openai_account"
            && item.source_id == "acc-from-stat"
            && item.usage.total_tokens == 15
            && item.usage.request_count == 1
    }));
    assert!(all_sources.iter().any(|item| {
        item.source_kind == "aggregate_api"
            && item.source_id == "agg-from-stat"
            && item.usage.total_tokens == 20
            && item.usage.request_count == 1
    }));

    let deleted = storage
        .rollup_request_token_stats_before(7_200)
        .expect("roll up old token stats");
    assert_eq!(deleted, 0);
    let raw_rows: i64 = storage
        .conn
        .query_row("SELECT COUNT(1) FROM request_token_stats", [], |row| {
            row.get(0)
        })
        .expect("count raw stats");
    assert_eq!(raw_rows, 0);
    let legacy_rollup_rows: i64 = storage
        .conn
        .query_row(
            "SELECT COUNT(1) FROM request_token_stat_rollups",
            [],
            |row| row.get(0),
        )
        .expect("count legacy rollups");
    assert_eq!(legacy_rollup_rows, 0);

    let daily_after_rollup = storage
        .summarize_request_token_stats_daily(0, 7_200, 86_400)
        .expect("daily summary after rollup");
    assert_eq!(daily_after_rollup.len(), 1);
    assert_eq!(daily_after_rollup[0].usage.total_tokens, 35);
    assert_eq!(daily_after_rollup[0].usage.request_count, 2);

    let by_user_after_rollup = storage
        .summarize_request_token_stats_by_user_between(0, 7_200)
        .expect("user summary after rollup");
    assert_eq!(by_user_after_rollup.len(), 1);
    assert_eq!(by_user_after_rollup[0].user_id, "user-1");
    assert_eq!(by_user_after_rollup[0].usage.total_tokens, 35);
    assert_eq!(by_user_after_rollup[0].usage.request_count, 2);

    let for_user_after_rollup = storage
        .summarize_request_token_stats_for_user_between("user-1", 0, 7_200)
        .expect("single user summary after rollup");
    assert_eq!(for_user_after_rollup.total_tokens, 35);
    assert_eq!(for_user_after_rollup.request_count, 2);

    let daily_for_user_after_rollup = storage
        .summarize_request_token_stats_daily_for_user("user-1", 0, 7_200, 86_400)
        .expect("daily user summary after rollup");
    assert_eq!(daily_for_user_after_rollup.len(), 1);
    assert_eq!(daily_for_user_after_rollup[0].usage.total_tokens, 35);

    let by_key_after_rollup = storage
        .summarize_request_token_stats_by_key_for_keys(&["key-owned".to_string()])
        .expect("key summary after rollup");
    assert_eq!(by_key_after_rollup.len(), 1);
    assert_eq!(by_key_after_rollup[0].key_id, "key-owned");
    assert_eq!(by_key_after_rollup[0].total_tokens, 35);
    assert_float_close(by_key_after_rollup[0].estimated_cost_usd, 0.60);

    let by_model_after_rollup = storage
        .summarize_request_token_stats_by_model_for_keys(None, None, &["key-owned".to_string()])
        .expect("model summary after rollup");
    assert_eq!(by_model_after_rollup.len(), 1);
    assert_eq!(by_model_after_rollup[0].model, "gpt-5");
    assert_eq!(by_model_after_rollup[0].total_tokens, 35);
    assert_float_close(by_model_after_rollup[0].estimated_cost_usd, 0.60);

    let by_key_model_after_rollup = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(
            None,
            None,
            &["key-owned".to_string()],
        )
        .expect("key model summary after rollup");
    assert_eq!(by_key_model_after_rollup.len(), 1);
    assert_eq!(by_key_model_after_rollup[0].key_id, "key-owned");
    assert_eq!(by_key_model_after_rollup[0].model, "gpt-5");
    assert_eq!(by_key_model_after_rollup[0].total_tokens, 35);
    assert_float_close(by_key_model_after_rollup[0].estimated_cost_usd, 0.60);

    let by_source_after_rollup = storage
        .summarize_request_token_stats_by_source_between("openai_account", 0, 7_200)
        .expect("source summary after rollup");
    assert_eq!(by_source_after_rollup.len(), 1);
    assert_eq!(by_source_after_rollup[0].source_id, "acc-from-stat");
    assert_eq!(by_source_after_rollup[0].usage.total_tokens, 15);
    assert_eq!(by_source_after_rollup[0].usage.request_count, 1);

    let aggregate_source_after_rollup = storage
        .summarize_request_token_stats_by_source_between("aggregate_api", 0, 7_200)
        .expect("aggregate source summary after rollup");
    assert_eq!(aggregate_source_after_rollup.len(), 1);
    assert_eq!(aggregate_source_after_rollup[0].source_id, "agg-from-stat");
    assert_eq!(aggregate_source_after_rollup[0].usage.total_tokens, 20);
    assert_eq!(aggregate_source_after_rollup[0].usage.request_count, 1);

    let all_sources_after_rollup = storage
        .summarize_request_token_stats_by_sources_between(
            &["openai_account", "aggregate_api"],
            0,
            7_200,
        )
        .expect("multi-source summary after rollup");
    assert_eq!(all_sources_after_rollup.len(), 2);
    assert!(all_sources_after_rollup.iter().any(|item| {
        item.source_kind == "openai_account"
            && item.source_id == "acc-from-stat"
            && item.usage.total_tokens == 15
    }));
    assert!(all_sources_after_rollup.iter().any(|item| {
        item.source_kind == "aggregate_api"
            && item.source_id == "agg-from-stat"
            && item.usage.total_tokens == 20
    }));
}

#[test]
fn hourly_dashboard_rollups_respect_partial_range_boundaries() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_app_user(&AppUser {
            id: "user-1".to_string(),
            username: "user-1".to_string(),
            display_name: Some("User 1".to_string()),
            password_hash: "hash".to_string(),
            role: "member".to_string(),
            status: "active".to_string(),
            created_at: 100,
            updated_at: 100,
            last_login_at: None,
        })
        .expect("insert app user");
    storage
        .insert_api_key(&ApiKey {
            id: "key-owned".to_string(),
            name: Some("Owned key".to_string()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            key_hash: "hash-owned".to_string(),
            status: "active".to_string(),
            rotation_strategy: "account_rotation".to_string(),
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            created_at: 100,
            last_used_at: None,
        })
        .expect("insert api key");
    storage
        .upsert_api_key_owner(&ApiKeyOwner {
            key_id: "key-owned".to_string(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-1".to_string()),
            project_id: None,
            updated_at: 100,
        })
        .expect("upsert owner");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-owned".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            actual_source_kind: Some("openai_account".to_string()),
            actual_source_id: Some("acc-a".to_string()),
            total_tokens: Some(10),
            estimated_cost_usd: Some(0.10),
            created_at: 100,
            ..RequestTokenStat::default()
        })
        .expect("insert stat before range");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 2,
            key_id: Some("key-owned".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            actual_source_kind: Some("openai_account".to_string()),
            actual_source_id: Some("acc-a".to_string()),
            total_tokens: Some(20),
            estimated_cost_usd: Some(0.20),
            created_at: 3_700,
            ..RequestTokenStat::default()
        })
        .expect("insert stat inside range");
    storage
        .rollup_request_token_stats_before(7_200)
        .expect("roll up old token stats");

    let summary = storage
        .summarize_request_token_stats_for_user_between("user-1", 1_800, 7_200)
        .expect("user summary");
    assert_eq!(summary.total_tokens, 20);
    assert_eq!(summary.request_count, 1);

    let daily = storage
        .summarize_request_token_stats_daily_for_user("user-1", 1_800, 7_200, 86_400)
        .expect("daily user summary");
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].usage.total_tokens, 20);

    let source = storage
        .summarize_request_token_stats_by_source_between("openai_account", 1_800, 7_200)
        .expect("source summary");
    assert_eq!(source.len(), 1);
    assert_eq!(source[0].usage.total_tokens, 20);
}

#[test]
fn key_model_range_query_matches_composite_index() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let sql = super::raw_key_usage_select(
        "",
        "t.key_id = ?1
            AND t.model = ?2
            AND t.created_at >= ?3
            AND t.created_at < ?4",
        "GROUP BY t.key_id, normalized_model",
    );
    let details = collect_query_plan_details_with_params(
        &storage,
        &format!("EXPLAIN QUERY PLAN {sql}"),
        vec![
            Value::Text("key-a".to_string()),
            Value::Text("gpt-5".to_string()),
            Value::Integer(0),
            Value::Integer(604800),
        ],
    );
    assert!(details
        .iter()
        .any(|detail| detail.contains("idx_request_token_stats_key_model_created_at")));
}

#[test]
fn summaries_for_empty_key_lists_return_empty_results() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-a".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            input_tokens: Some(10),
            cached_input_tokens: Some(0),
            output_tokens: Some(5),
            total_tokens: Some(15),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.10),
            created_at: 100,
            ..RequestTokenStat::default()
        })
        .expect("insert raw key a");

    let empty = Vec::<String>::new();
    assert!(storage
        .summarize_request_token_stats_by_key_for_keys(&empty)
        .expect("summarize by key")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_model_for_keys(None, None, &empty)
        .expect("summarize by model")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_key_and_model_for_keys(None, None, &empty)
        .expect("summarize by key and model")
        .is_empty());
}

#[test]
fn model_summaries_short_circuit_empty_optional_ranges() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-a".to_string()),
            account_id: Some("acc-a".to_string()),
            model: Some("gpt-5".to_string()),
            total_tokens: Some(15),
            estimated_cost_usd: Some(0.10),
            created_at: 100,
            ..RequestTokenStat::default()
        })
        .expect("insert raw stat");

    assert!(storage
        .summarize_request_token_stats_by_model(Some(200), Some(200))
        .expect("summarize model empty range")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_model_for_keys(
            Some(200),
            Some(200),
            &["key-a".to_string()],
        )
        .expect("summarize model keys empty range")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_key_and_model(Some(200), Some(200))
        .expect("summarize key model empty range")
        .is_empty());
    assert!(storage
        .summarize_request_token_stats_by_key_and_model_for_keys(
            Some(200),
            Some(200),
            &["key-a".to_string()],
        )
        .expect("summarize key model keys empty range")
        .is_empty());
}

/// 函数 `summaries_for_large_key_lists_use_temp_filter`
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
fn summaries_for_large_key_lists_use_temp_filter() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let mut selected = Vec::new();
    for index in 0..901 {
        let key_id = format!("key-{index:04}");
        selected.push(key_id.clone());
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id: index as i64 + 1,
                key_id: Some(key_id),
                account_id: Some(format!("acc-{index:04}")),
                model: Some("gpt-5".to_string()),
                input_tokens: Some(1),
                cached_input_tokens: Some(0),
                output_tokens: Some(0),
                total_tokens: Some(1),
                reasoning_output_tokens: Some(0),
                estimated_cost_usd: Some(0.01),
                created_at: 1_000 + index as i64,
                ..RequestTokenStat::default()
            })
            .expect("insert request token stat");
    }

    let by_key = storage
        .summarize_request_token_stats_by_key_for_keys(&selected)
        .expect("summarize by key");
    assert_eq!(by_key.len(), selected.len());
    assert_eq!(
        by_key.first().map(|item| item.key_id.as_str()),
        Some("key-0000")
    );
    assert_eq!(
        by_key.last().map(|item| item.key_id.as_str()),
        Some("key-0900")
    );

    let by_model = storage
        .summarize_request_token_stats_by_model_for_keys(None, None, &selected)
        .expect("summarize by model");
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "gpt-5");
    assert_eq!(by_model[0].total_tokens, selected.len() as i64);
    assert_float_close(by_model[0].estimated_cost_usd, 9.01);

    let by_key_and_model = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(None, None, &selected)
        .expect("summarize by key and model");
    assert_eq!(by_key_and_model.len(), selected.len());
    assert_eq!(
        by_key_and_model.first().map(|item| item.key_id.as_str()),
        Some("key-0000")
    );
    assert_eq!(
        by_key_and_model.last().map(|item| item.key_id.as_str()),
        Some("key-0900")
    );
}
