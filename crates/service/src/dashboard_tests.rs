use super::{
    build_dashboard_source_summaries, build_dashboard_user_summaries, daily_usage_bucket,
    dashboard_source_ids, filter_source_usage, read_member_usage_breakdown, read_usage_trend_7d,
    SourceMetadata,
};
use codexmanager_core::storage::{
    ApiKey, ApiKeyOwner, AppUser, DailyTokenUsageRollup, RequestTokenStat, SourceTokenUsageRollup,
    Storage, TokenUsageRollup, UserTokenUsageRollup,
};
use std::collections::HashMap;

fn source_usage(source_id: &str, total_tokens: i64) -> SourceTokenUsageRollup {
    SourceTokenUsageRollup {
        source_kind: "openai_account".to_string(),
        source_id: source_id.to_string(),
        usage: TokenUsageRollup {
            total_tokens,
            request_count: 1,
            success_count: 1,
            ..TokenUsageRollup::default()
        },
    }
}

fn user_usage(user_id: &str, total_tokens: i64) -> UserTokenUsageRollup {
    UserTokenUsageRollup {
        user_id: user_id.to_string(),
        usage: TokenUsageRollup {
            total_tokens,
            request_count: 1,
            success_count: 1,
            ..TokenUsageRollup::default()
        },
    }
}

fn daily_usage(start: i64, end: i64, total_tokens: i64) -> DailyTokenUsageRollup {
    DailyTokenUsageRollup {
        day_start_ts: start,
        day_end_ts: end,
        usage: TokenUsageRollup {
            total_tokens,
            request_count: 1,
            success_count: 1,
            ..TokenUsageRollup::default()
        },
    }
}

fn api_key(id: &str) -> ApiKey {
    ApiKey {
        id: id.to_string(),
        name: Some(id.to_string()),
        model_slug: Some("gpt-test".to_string()),
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: "account_rotation".to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: "codex".to_string(),
        protocol_type: "openai_compat".to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("hash-{id}"),
        status: "active".to_string(),
        created_at: 1,
        last_used_at: None,
    }
}

#[test]
fn dashboard_source_ids_deduplicates_today_and_range_sources() {
    let today = vec![source_usage(" acc-a ", 10), source_usage("acc-b", 5)];
    let range = vec![source_usage("acc-b", 20), source_usage("", 1)];

    assert_eq!(
        dashboard_source_ids(&today, &range),
        vec!["acc-a".to_string(), "acc-b".to_string()]
    );
}

#[test]
fn daily_usage_bucket_reuses_exact_today_bucket() {
    let items = vec![
        daily_usage(1_700_000_000, 1_700_086_400, 10),
        daily_usage(1_700_086_400, 1_700_172_800, 25),
    ];

    assert_eq!(
        daily_usage_bucket(&items, 1_700_086_400, 1_700_172_800).map(|usage| usage.total_tokens),
        Some(25)
    );
    assert_eq!(
        daily_usage_bucket(&items, 1_700_086_400, 1_700_160_000).map(|usage| usage.total_tokens),
        None
    );
}

#[test]
fn filter_source_usage_keeps_only_requested_source_kind() {
    let items = vec![
        source_usage("acc-a", 10),
        SourceTokenUsageRollup {
            source_kind: "aggregate_api".to_string(),
            source_id: "agg-a".to_string(),
            usage: TokenUsageRollup {
                total_tokens: 20,
                request_count: 1,
                success_count: 1,
                ..TokenUsageRollup::default()
            },
        },
    ];

    let openai = filter_source_usage(&items, "openai_account");
    let aggregate = filter_source_usage(&items, "aggregate_api");

    assert_eq!(openai.len(), 1);
    assert_eq!(openai[0].source_id, "acc-a");
    assert_eq!(aggregate.len(), 1);
    assert_eq!(aggregate[0].source_id, "agg-a");
}

#[test]
fn dashboard_source_summaries_keep_usage_for_deleted_sources() {
    let metadata = HashMap::from([(
        "acc-known".to_string(),
        SourceMetadata {
            name: Some("Known".to_string()),
            status: Some("active".to_string()),
            provider: Some("openai".to_string()),
        },
    )]);
    let summaries = build_dashboard_source_summaries(
        "openai_account",
        metadata,
        vec![source_usage("acc-known", 25)],
        vec![source_usage("acc-deleted", 12)],
    );

    assert!(summaries.iter().any(|item| {
        item.source_id == "acc-known"
            && item.name.as_deref() == Some("Known")
            && item.today_usage.total_tokens == 25
    }));
    assert!(summaries.iter().any(|item| {
        item.source_id == "acc-deleted"
            && item.name.is_none()
            && item.range_usage.total_tokens == 12
    }));
}

#[test]
fn dashboard_source_summaries_are_limited_to_top_sources() {
    let today = (0..20)
        .map(|index| source_usage(&format!("acc-{index:02}"), index))
        .collect::<Vec<_>>();

    let summaries =
        build_dashboard_source_summaries("openai_account", HashMap::new(), today, Vec::new());

    // The limit is an upper bound. With fewer source rows, every row remains.
    assert_eq!(summaries.len(), 20);
    assert_eq!(summaries[0].source_id, "acc-19");
    assert_eq!(summaries[0].today_usage.total_tokens, 19);
    assert_eq!(
        summaries.last().map(|item| item.source_id.as_str()),
        Some("acc-00")
    );
}

#[test]
fn dashboard_user_summaries_only_load_users_with_usage() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    storage
        .insert_app_user(&AppUser {
            id: "user-active".to_string(),
            username: "active@example.com".to_string(),
            display_name: Some("Active".to_string()),
            password_hash: "hash-active".to_string(),
            role: "member".to_string(),
            status: "active".to_string(),
            created_at: 1,
            updated_at: 1,
            last_login_at: None,
        })
        .expect("insert active user");
    storage
        .insert_app_user(&AppUser {
            id: "user-unused".to_string(),
            username: "unused@example.com".to_string(),
            display_name: Some("Unused".to_string()),
            password_hash: "hash-unused".to_string(),
            role: "member".to_string(),
            status: "active".to_string(),
            created_at: 2,
            updated_at: 2,
            last_login_at: None,
        })
        .expect("insert unused user");
    storage
        .insert_api_key(&api_key("key-active"))
        .expect("insert active api key");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-active".to_string()),
            account_id: None,
            model: Some("gpt-test".to_string()),
            total_tokens: Some(20),
            estimated_cost_usd: Some(0.02),
            created_at: 100,
            ..RequestTokenStat::default()
        })
        .expect("insert active user stat");
    storage
        .upsert_api_key_owner(&ApiKeyOwner {
            key_id: "key-active".to_string(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-active".to_string()),
            project_id: None,
            updated_at: 1,
        })
        .expect("seed active key owner");

    let today = storage
        .summarize_request_token_stats_by_user_between(90, 110)
        .expect("summarize today users");
    let range = storage
        .summarize_request_token_stats_by_user_between(0, 200)
        .expect("summarize range users");
    let summaries =
        build_dashboard_user_summaries(&storage, today, range).expect("build summaries");

    assert_eq!(summaries.len(), 1);
    assert!(summaries.iter().any(|item| {
        item.user_id == "user-active"
            && item.username.as_deref() == Some("active@example.com")
            && item.today_usage.total_tokens == 20
    }));
    assert!(!summaries.iter().any(|item| item.user_id == "user-unused"));
}

#[test]
fn dashboard_user_summaries_are_limited_to_top_users() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let today = (0..20)
        .map(|index| user_usage(&format!("user-{index:02}"), index))
        .collect::<Vec<_>>();

    let summaries =
        build_dashboard_user_summaries(&storage, today, Vec::new()).expect("build summaries");

    assert_eq!(summaries.len(), super::ADMIN_TOP_USER_LIMIT);
    assert_eq!(summaries[0].user_id, "user-19");
    assert_eq!(summaries[0].today_usage.total_tokens, 19);
    assert_eq!(
        summaries.last().map(|item| item.user_id.as_str()),
        Some("user-08")
    );
}

#[test]
fn dashboard_user_summaries_short_circuit_empty_usage() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let summaries = build_dashboard_user_summaries(&storage, Vec::new(), Vec::new())
        .expect("build empty summaries");

    assert!(summaries.is_empty());
}

#[test]
fn member_usage_breakdown_short_circuits_empty_key_ids() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let (top_keys, top_models) =
        read_member_usage_breakdown(&storage, &[], &[], 1_700_000_000, 1_700_086_400)
            .expect("read empty member usage breakdown");

    assert!(top_keys.is_empty());
    assert!(top_models.is_empty());
}

#[test]
fn member_alerts_skip_available_model_warning_when_count_is_not_loaded() {
    let api_key_summary = super::MemberDashboardApiKeySummary {
        total_count: 1,
        enabled_count: 1,
        disabled_count: 0,
        last_used_at: None,
    };
    let usage_today = super::MemberDashboardUsageToday::default();

    let light_alerts = super::build_alerts(false, None, &api_key_summary, &usage_today, None);
    assert!(!light_alerts
        .iter()
        .any(|item| item.kind == "no_available_model"));

    let detail_alerts = super::build_alerts(false, None, &api_key_summary, &usage_today, Some(0));
    assert!(detail_alerts
        .iter()
        .any(|item| item.kind == "no_available_model"));
}

#[test]
fn member_usage_trend_reuses_today_rollup() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    storage
        .insert_app_user(&AppUser {
            id: "user-member-trend".to_string(),
            username: "member-trend@example.com".to_string(),
            display_name: None,
            password_hash: "hash".to_string(),
            role: "member".to_string(),
            status: "active".to_string(),
            created_at: 1,
            updated_at: 1,
            last_login_at: None,
        })
        .expect("insert user");
    storage
        .insert_api_key(&api_key("key-member-trend"))
        .expect("insert api key");
    storage
        .upsert_api_key_owner(&ApiKeyOwner {
            key_id: "key-member-trend".to_string(),
            owner_kind: "user".to_string(),
            owner_user_id: Some("user-member-trend".to_string()),
            project_id: None,
            updated_at: 1,
        })
        .expect("seed owner");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 7_001,
            key_id: Some("key-member-trend".to_string()),
            model: Some("gpt-test".to_string()),
            total_tokens: Some(42),
            estimated_cost_usd: Some(0.42),
            created_at: 1_700_000_120,
            ..RequestTokenStat::default()
        })
        .expect("insert token stat");

    let trend = read_usage_trend_7d(&storage, "user-member-trend", 1_700_000_000, 1_700_086_400)
        .expect("read trend");

    assert_eq!(trend.today_usage.total_tokens, 42);
    assert_eq!(trend.points.len(), super::TREND_DAYS as usize);
    assert_eq!(
        trend.points.last().map(|point| point.total_tokens),
        Some(42)
    );
}
