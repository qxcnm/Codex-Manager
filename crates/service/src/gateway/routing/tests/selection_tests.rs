use super::{
    clear_candidate_cache_for_tests, collect_gateway_candidates, CANDIDATE_CACHE_TTL_ENV,
    LOW_QUOTA_THRESHOLD_ENV, QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK_ENV,
};
use crate::account_status::mark_account_unavailable_for_gateway_error;
use codexmanager_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};

/// 函数 `candidate_snapshot_cache_reuses_recent_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn candidate_snapshot_cache_reuses_recent_snapshot() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "2000");
    std::env::set_var("CODEXMANAGER_DB_PATH", "selection-cache-test-1");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_account(&Account {
            id: "acc-cache-1".to_string(),
            label: "cached".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-cache-1".to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-cache-1".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now_ts(),
        })
        .expect("insert snapshot");

    let first = collect_gateway_candidates(&storage).expect("first candidates");
    assert_eq!(first.len(), 1);

    storage
        .update_account_status("acc-cache-1", "inactive")
        .expect("mark inactive");
    let second = collect_gateway_candidates(&storage).expect("second candidates");
    assert_eq!(second.len(), 1);

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    super::reload_from_env();
}

/// 函数 `candidates_follow_account_sort_order`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn candidates_follow_account_sort_order() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "0");
    std::env::set_var("CODEXMANAGER_DB_PATH", "selection-cache-test-2");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let now = now_ts();
    let accounts = vec![
        ("acc-sort-10", 10_i64),
        ("acc-sort-0", 0_i64),
        ("acc-sort-1", 1_i64),
    ];
    for (id, sort) in &accounts {
        storage
            .insert_account(&Account {
                id: (*id).to_string(),
                label: (*id).to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: *sort,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: (*id).to_string(),
                id_token: "id".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: (*id).to_string(),
                used_percent: Some(10.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage");
    }

    let candidates = collect_gateway_candidates(&storage).expect("collect candidates");
    let ordered_ids = candidates
        .iter()
        .map(|(account, _)| account.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ordered_ids, vec!["acc-sort-0", "acc-sort-1", "acc-sort-10"]);

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    super::reload_from_env();
}

/// 函数 `gateway_error_status_change_invalidates_candidate_snapshot_cache`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-03
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn gateway_error_status_change_invalidates_candidate_snapshot_cache() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "2000");
    std::env::set_var("CODEXMANAGER_DB_PATH", "selection-cache-test-3");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-cache-usage-limit".to_string(),
            label: "cache-usage-limit".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-cache-usage-limit".to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-cache-usage-limit".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert snapshot");

    let first = collect_gateway_candidates(&storage).expect("first candidates");
    assert_eq!(first.len(), 1);

    assert!(mark_account_unavailable_for_gateway_error(
        &storage,
        "acc-cache-usage-limit",
        "You've hit your usage limit. To get more access now, try again at 8:02 PM."
    ));

    let second = collect_gateway_candidates(&storage).expect("second candidates");
    assert!(
        second.is_empty(),
        "usage-limit should mark the account limited and evict cached candidate"
    );

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    super::reload_from_env();
}

/// 函数 `gateway_deactivation_status_change_invalidates_candidate_snapshot_cache`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-03
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn gateway_deactivation_status_change_invalidates_candidate_snapshot_cache() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "2000");
    std::env::set_var("CODEXMANAGER_DB_PATH", "selection-cache-test-4");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-cache-deactivated".to_string(),
            label: "cache-deactivated".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-cache-deactivated".to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-cache-deactivated".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert snapshot");

    let first = collect_gateway_candidates(&storage).expect("first candidates");
    assert_eq!(first.len(), 1);

    assert!(mark_account_unavailable_for_gateway_error(
        &storage,
        "acc-cache-deactivated",
        "Your OpenAI account has been deactivated"
    ));

    let second = collect_gateway_candidates(&storage).expect("second candidates");
    assert!(
        second.is_empty(),
        "deactivation should invalidate cached candidate"
    );

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    super::reload_from_env();
}

/// 函数 `gateway_usage_limit_with_exhausted_snapshot_invalidates_candidate_snapshot_cache`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-03
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn gateway_usage_limit_with_exhausted_snapshot_invalidates_candidate_snapshot_cache() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "2000");
    std::env::set_var("CODEXMANAGER_DB_PATH", "selection-cache-test-5");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-cache-usage-exhausted".to_string(),
            label: "cache-usage-exhausted".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-cache-usage-exhausted".to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-cache-usage-exhausted".to_string(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(10.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert snapshot");

    let first = collect_gateway_candidates(&storage).expect("first candidates");
    assert_eq!(first.len(), 1);

    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-cache-usage-exhausted".to_string(),
            used_percent: Some(100.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(100.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now + 1,
        })
        .expect("insert exhausted snapshot");

    assert!(mark_account_unavailable_for_gateway_error(
        &storage,
        "acc-cache-usage-exhausted",
        "You've hit your usage limit. To get more access now, try again at 8:02 PM."
    ));

    let second = collect_gateway_candidates(&storage).expect("second candidates");
    assert!(
        second.is_empty(),
        "confirmed exhausted snapshot should invalidate cached candidate"
    );

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    super::reload_from_env();
}

/// 低配额账号（used_percent 超过阈值）应当进入低额度备用池（候选列表尾部），
/// 配额充足的账号优先被挑选，保留每个池子内的原有顺序。
#[test]
fn low_quota_accounts_are_skipped_when_healthy_available() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    let previous_threshold = std::env::var(LOW_QUOTA_THRESHOLD_ENV).ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "0");
    std::env::set_var("CODEXMANAGER_DB_PATH", "selection-low-quota-test");
    std::env::set_var(LOW_QUOTA_THRESHOLD_ENV, "95");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let now = now_ts();
    let rows: Vec<(&str, i64, f64, Option<f64>)> = vec![
        ("acc-exhausted", 0, 99.0, None),
        ("acc-healthy-high", 1, 10.0, None),
        ("acc-secondary-low", 2, 10.0, Some(99.0)),
        ("acc-healthy-low", 3, 5.0, None),
    ];
    for (id, sort, primary_pct, secondary_pct) in &rows {
        storage
            .insert_account(&Account {
                id: (*id).to_string(),
                label: (*id).to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: *sort,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: (*id).to_string(),
                id_token: "id".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: (*id).to_string(),
                used_percent: Some(*primary_pct),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: *secondary_pct,
                secondary_window_minutes: secondary_pct.map(|_| 10_080),
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert snapshot");
    }

    let candidates = collect_gateway_candidates(&storage).expect("collect candidates");
    let ids: Vec<&str> = candidates
        .iter()
        .map(|(account, _)| account.id.as_str())
        .collect();
    assert_eq!(ids, vec!["acc-healthy-high", "acc-healthy-low"]);

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    if let Some(value) = previous_threshold {
        std::env::set_var(LOW_QUOTA_THRESHOLD_ENV, value);
    } else {
        std::env::remove_var(LOW_QUOTA_THRESHOLD_ENV);
    }
    super::reload_from_env();
}

/// 全部账号都触阈时仍应返回低额度候选，供正常池不可用后的兜底使用。
#[test]
fn all_low_quota_still_returns_candidates() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    let previous_threshold = std::env::var(LOW_QUOTA_THRESHOLD_ENV).ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "0");
    std::env::set_var("CODEXMANAGER_DB_PATH", "selection-all-low-quota-test");
    std::env::set_var(LOW_QUOTA_THRESHOLD_ENV, "95");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let now = now_ts();
    for (id, sort) in &[("acc-a", 0_i64), ("acc-b", 1_i64)] {
        storage
            .insert_account(&Account {
                id: (*id).to_string(),
                label: (*id).to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: *sort,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: (*id).to_string(),
                id_token: "id".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: (*id).to_string(),
                used_percent: Some(98.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert snapshot");
    }

    let candidates = collect_gateway_candidates(&storage).expect("collect candidates");
    let ids: Vec<&str> = candidates
        .iter()
        .map(|(account, _)| account.id.as_str())
        .collect();
    assert_eq!(ids, vec!["acc-a", "acc-b"]);

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    if let Some(value) = previous_threshold {
        std::env::set_var(LOW_QUOTA_THRESHOLD_ENV, value);
    } else {
        std::env::remove_var(LOW_QUOTA_THRESHOLD_ENV);
    }
    super::reload_from_env();
}

#[test]
fn all_low_quota_without_fallback_returns_no_candidates() {
    let _guard = crate::test_env_guard();
    let previous_ttl = std::env::var(CANDIDATE_CACHE_TTL_ENV).ok();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    let previous_threshold = std::env::var(LOW_QUOTA_THRESHOLD_ENV).ok();
    let previous_fallback = std::env::var(QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK_ENV).ok();
    std::env::set_var(CANDIDATE_CACHE_TTL_ENV, "0");
    std::env::set_var(
        "CODEXMANAGER_DB_PATH",
        "selection-all-low-quota-no-fallback-test",
    );
    std::env::set_var(LOW_QUOTA_THRESHOLD_ENV, "95");
    std::env::set_var(QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK_ENV, "0");
    super::reload_from_env();
    clear_candidate_cache_for_tests();

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    let now = now_ts();
    for (id, sort) in &[("acc-a", 0_i64), ("acc-b", 1_i64)] {
        storage
            .insert_account(&Account {
                id: (*id).to_string(),
                label: (*id).to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: *sort,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: (*id).to_string(),
                id_token: "id".to_string(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: (*id).to_string(),
                used_percent: Some(98.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert snapshot");
    }

    let candidates = collect_gateway_candidates(&storage).expect("collect candidates");
    assert!(candidates.is_empty());

    clear_candidate_cache_for_tests();
    if let Some(value) = previous_ttl {
        std::env::set_var(CANDIDATE_CACHE_TTL_ENV, value);
    } else {
        std::env::remove_var(CANDIDATE_CACHE_TTL_ENV);
    }
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    if let Some(value) = previous_threshold {
        std::env::set_var(LOW_QUOTA_THRESHOLD_ENV, value);
    } else {
        std::env::remove_var(LOW_QUOTA_THRESHOLD_ENV);
    }
    if let Some(value) = previous_fallback {
        std::env::set_var(QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK_ENV, value);
    } else {
        std::env::remove_var(QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK_ENV);
    }
    super::reload_from_env();
}
