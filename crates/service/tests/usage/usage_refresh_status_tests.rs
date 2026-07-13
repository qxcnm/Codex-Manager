use super::{
    mark_usage_unreachable_if_needed, record_usage_refresh_failure, should_retry_with_refresh,
};
use crate::account_availability::Availability;
use crate::account_status::{
    deactivation_reason_from_message, mark_account_unavailable_for_auth_error,
    mark_account_unavailable_for_deactivation_error,
    mark_account_unavailable_for_refresh_token_error, set_account_status,
    REFRESH_TOKEN_REGION_BLOCKED_REASON,
};
use crate::usage_snapshot_store::apply_status_from_snapshot;
use codexmanager_core::storage::{now_ts, Account, Storage, UsageSnapshotRecord};
use std::time::{SystemTime, UNIX_EPOCH};

/// 函数 `unique_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - prefix: 参数 prefix
///
/// # 返回
/// 返回函数执行结果
fn unique_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

/// 函数 `apply_status_missing_snapshot_keeps_account_status`
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
fn apply_status_missing_snapshot_keeps_account_status() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-1".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    let record = UsageSnapshotRecord {
        account_id: "acc-1".to_string(),
        used_percent: None,
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(10.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &record);
    assert!(matches!(availability, Availability::Unavailable(_)));
    let loaded = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-1")
        .expect("exists");
    assert_eq!(loaded.status, "active");
}

#[test]
fn apply_status_keeps_business_account_available_without_usage_quota() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-business".to_string(),
            label: "business".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "limited".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert");
    storage
        .upsert_account_subscription(
            "acc-business",
            true,
            Some("business"),
            Some("business"),
            None,
            None,
        )
        .expect("insert subscription");

    let record = UsageSnapshotRecord {
        account_id: "acc-business".to_string(),
        used_percent: None,
        window_minutes: None,
        resets_at: None,
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &record);
    assert!(matches!(availability, Availability::Available));
    let loaded = storage
        .find_account_by_id("acc-business")
        .expect("find")
        .expect("exists");
    assert_eq!(loaded.status, "active");
}

/// 函数 `apply_status_skips_db_and_event_when_status_unchanged`
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
fn apply_status_skips_db_and_event_when_status_unchanged() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let base_updated_at = now_ts() - 10;
    let account = Account {
        id: "acc-unchanged".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "inactive".to_string(),
        created_at: base_updated_at,
        updated_at: base_updated_at,
    };
    storage.insert_account(&account).expect("insert");

    let missing_primary = UsageSnapshotRecord {
        account_id: "acc-unchanged".to_string(),
        used_percent: None,
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(10.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &missing_primary);
    assert!(matches!(availability, Availability::Unavailable(_)));

    let unchanged_account = storage
        .find_account_by_id("acc-unchanged")
        .expect("find")
        .expect("exists");
    assert_eq!(unchanged_account.status, "inactive");
    assert_eq!(unchanged_account.updated_at, base_updated_at);
    assert_eq!(storage.event_count().expect("count events"), 0);

    let available = UsageSnapshotRecord {
        account_id: "acc-unchanged".to_string(),
        used_percent: Some(10.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(20.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &available);
    assert!(matches!(availability, Availability::Available));
    let reactivated_account = storage
        .find_account_by_id("acc-unchanged")
        .expect("find")
        .expect("exists");
    assert_eq!(reactivated_account.status, "active");
    assert_eq!(storage.event_count().expect("count events"), 1);
}

/// 函数 `apply_status_exhausted_snapshot_marks_account_limited`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-19
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn apply_status_exhausted_snapshot_marks_account_limited() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-limited".to_string(),
        label: "limited".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    let record = UsageSnapshotRecord {
        account_id: "acc-limited".to_string(),
        used_percent: Some(100.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(42.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &record);
    assert!(matches!(
        availability,
        Availability::Unavailable("usage_exhausted_primary")
    ));

    let limited = storage
        .find_account_by_id("acc-limited")
        .expect("find")
        .expect("exists");
    assert_eq!(limited.status, "limited");

    let reasons = storage
        .latest_account_status_reasons(&["acc-limited".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-limited").map(String::as_str),
        Some("usage_limit_exhausted")
    );
}

/// 函数 `apply_status_available_snapshot_recovers_limited_account_to_active`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-19
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn apply_status_available_snapshot_recovers_limited_account_to_active() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-limited-recover".to_string(),
        label: "limited-recover".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "limited".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    let record = UsageSnapshotRecord {
        account_id: "acc-limited-recover".to_string(),
        used_percent: Some(12.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(18.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &record);
    assert!(matches!(availability, Availability::Available));

    let active = storage
        .find_account_by_id("acc-limited-recover")
        .expect("find")
        .expect("exists");
    assert_eq!(active.status, "active");

    let reasons = storage
        .latest_account_status_reasons(&["acc-limited-recover".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-limited-recover").map(String::as_str),
        Some("usage_ok")
    );
}

/// 函数 `mark_usage_unreachable_marks_401_403_as_unavailable_but_ignores_429`
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
fn mark_usage_unreachable_marks_401_403_as_unavailable_but_ignores_429() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-2".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    mark_usage_unreachable_if_needed(&storage, "acc-2", "network timeout");
    let still_active = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(still_active.status, "active");

    mark_usage_unreachable_if_needed(&storage, "acc-2", "usage endpoint status 401 Unauthorized");
    let unavailable = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(unavailable.status, "unavailable");

    let reasons = storage
        .latest_account_status_reasons(&["acc-2".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-2").map(String::as_str),
        Some("usage_http_401")
    );

    storage
        .update_account_status_if_changed("acc-2", "active")
        .expect("reset account status");
    mark_usage_unreachable_if_needed(
        &storage,
        "acc-2",
        "usage endpoint failed: status=401 Unauthorized body=code=token_expired request id=req_usage_123",
    );
    let unavailable_after_failed_format_401 = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(unavailable_after_failed_format_401.status, "unavailable");

    let reasons = storage
        .latest_account_status_reasons(&["acc-2".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-2").map(String::as_str),
        Some("usage_http_401")
    );

    mark_usage_unreachable_if_needed(&storage, "acc-2", "usage endpoint status 403 Forbidden");
    let unavailable_after_403 = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(unavailable_after_403.status, "unavailable");

    let reasons = storage
        .latest_account_status_reasons(&["acc-2".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-2").map(String::as_str),
        Some("usage_http_403")
    );

    mark_usage_unreachable_if_needed(
        &storage,
        "acc-2",
        "usage endpoint status 429 Too Many Requests",
    );
    let still_unavailable_after_429 = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(still_unavailable_after_429.status, "unavailable");

    let reasons = storage
        .latest_account_status_reasons(&["acc-2".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-2").map(String::as_str),
        Some("usage_http_403")
    );

    storage
        .update_account_status_if_changed("acc-2", "active")
        .expect("reset account status after 429");
    mark_usage_unreachable_if_needed(
        &storage,
        "acc-2",
        "usage endpoint status 429 Too Many Requests",
    );
    let active_after_429 = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(active_after_429.status, "active");

    let reasons = storage
        .latest_account_status_reasons(&["acc-2".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-2").map(String::as_str),
        Some("usage_http_403")
    );

    mark_usage_unreachable_if_needed(
        &storage,
        "acc-2",
        "usage endpoint status 500 Internal Server Error",
    );
    let active_after_500 = storage
        .list_accounts()
        .expect("list")
        .into_iter()
        .find(|acc| acc.id == "acc-2")
        .expect("exists");
    assert_eq!(active_after_500.status, "active");

    let reasons = storage
        .latest_account_status_reasons(&["acc-2".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-2").map(String::as_str),
        Some("usage_http_403")
    );
}

/// 函数 `mark_usage_unreachable_does_not_override_manual_disabled_status`
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
fn mark_usage_unreachable_does_not_override_manual_disabled_status() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_account(&Account {
            id: "acc-disabled".to_string(),
            label: "main".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "disabled".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect("insert");

    mark_usage_unreachable_if_needed(
        &storage,
        "acc-disabled",
        "usage endpoint status 401 Unauthorized",
    );
    let disabled = storage
        .find_account_by_id("acc-disabled")
        .expect("find")
        .expect("exists");
    assert_eq!(disabled.status, "disabled");
}

/// 函数 `apply_status_available_preserves_manual_disabled_status`
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
fn apply_status_available_preserves_manual_disabled_status() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let base_updated_at = now_ts() - 10;
    storage
        .insert_account(&Account {
            id: "acc-disabled".to_string(),
            label: "main".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "disabled".to_string(),
            created_at: base_updated_at,
            updated_at: base_updated_at,
        })
        .expect("insert");

    let available = UsageSnapshotRecord {
        account_id: "acc-disabled".to_string(),
        used_percent: Some(10.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(20.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &available);
    assert!(matches!(availability, Availability::Available));

    let account = storage
        .find_account_by_id("acc-disabled")
        .expect("find")
        .expect("exists");
    assert_eq!(account.status, "disabled");
    assert_eq!(account.updated_at, base_updated_at);
    assert_eq!(storage.event_count().expect("count events"), 0);
}

#[test]
fn apply_status_available_preserves_region_blocked_status() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let base_updated_at = now_ts() - 10;
    storage
        .insert_account(&Account {
            id: "acc-region-blocked-status".to_string(),
            label: "main".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: base_updated_at,
            updated_at: base_updated_at,
        })
        .expect("insert");
    set_account_status(
        &storage,
        "acc-region-blocked-status",
        "unavailable",
        REFRESH_TOKEN_REGION_BLOCKED_REASON,
    );

    let available = UsageSnapshotRecord {
        account_id: "acc-region-blocked-status".to_string(),
        used_percent: Some(10.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(20.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    let availability = apply_status_from_snapshot(&storage, &available);
    assert!(matches!(availability, Availability::Available));

    let account = storage
        .find_account_by_id("acc-region-blocked-status")
        .expect("find")
        .expect("exists");
    assert_eq!(account.status, "unavailable");
    let reasons = storage
        .latest_account_status_reasons(&["acc-region-blocked-status".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-region-blocked-status").map(String::as_str),
        Some(REFRESH_TOKEN_REGION_BLOCKED_REASON)
    );
}

/// 函数 `refresh_token_auth_error_marks_account_unavailable`
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
fn refresh_token_auth_error_marks_account_unavailable() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-refresh-auth".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(mark_account_unavailable_for_refresh_token_error(
        &storage,
        "acc-refresh-auth",
        "refresh token failed with status 401 Unauthorized"
    ));
    let unavailable = storage
        .find_account_by_id("acc-refresh-auth")
        .expect("find")
        .expect("exists");
    assert_eq!(unavailable.status, "unavailable");
}

/// 函数 `refresh_token_forbidden_without_invalid_grant_keeps_account_active`
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
fn refresh_token_forbidden_without_invalid_grant_keeps_account_active() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-refresh-forbidden".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(!mark_account_unavailable_for_refresh_token_error(
        &storage,
        "acc-refresh-forbidden",
        "refresh token failed with status 403 Forbidden"
    ));
    let active = storage
        .find_account_by_id("acc-refresh-forbidden")
        .expect("find")
        .expect("exists");
    assert_eq!(active.status, "active");
}

#[test]
fn refresh_token_region_blocked_forbidden_marks_account_unavailable() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-refresh-region-blocked".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(mark_account_unavailable_for_refresh_token_error(
        &storage,
        "acc-refresh-region-blocked",
        "refresh token failed with status 403 Forbidden: code=unsupported_country_region_territory type=request_forbidden Country, region, or territory not supported [cf_ray=ray-HKG, kind=cloudflare_blocked]"
    ));
    let unavailable = storage
        .find_account_by_id("acc-refresh-region-blocked")
        .expect("find")
        .expect("exists");
    assert_eq!(unavailable.status, "unavailable");
    let reasons = storage
        .latest_account_status_reasons(&["acc-refresh-region-blocked".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons
            .get("acc-refresh-region-blocked")
            .map(String::as_str),
        Some(REFRESH_TOKEN_REGION_BLOCKED_REASON)
    );
}

/// 函数 `refresh_token_invalid_grant_on_forbidden_keeps_account_active`
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
fn refresh_token_invalid_grant_on_forbidden_keeps_account_active() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-refresh-invalid-grant-403".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(!mark_account_unavailable_for_refresh_token_error(
        &storage,
        "acc-refresh-invalid-grant-403",
        "refresh token failed with status 403 Forbidden: {\"error\":\"invalid_grant\"}"
    ));
    let active = storage
        .find_account_by_id("acc-refresh-invalid-grant-403")
        .expect("find")
        .expect("exists");
    assert_eq!(active.status, "active");
}

/// 函数 `refresh_token_invalid_grant_on_bad_request_marks_account_unavailable`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-03
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn refresh_token_invalid_grant_on_bad_request_marks_account_unavailable() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-refresh-invalid-grant-400".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(mark_account_unavailable_for_refresh_token_error(
        &storage,
        "acc-refresh-invalid-grant-400",
        "refresh token failed with status 400 Bad Request: invalid_grant"
    ));
    let unavailable = storage
        .find_account_by_id("acc-refresh-invalid-grant-400")
        .expect("find")
        .expect("exists");
    assert_eq!(unavailable.status, "unavailable");
}

/// 函数 `refresh_token_unknown_401_marks_account_unavailable`
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
fn refresh_token_unknown_401_marks_account_unavailable() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-refresh-unknown-401".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(mark_account_unavailable_for_refresh_token_error(
        &storage,
        "acc-refresh-unknown-401",
        "refresh token failed with status 401 Unauthorized: some_unknown_backend_code"
    ));
    let unavailable = storage
        .find_account_by_id("acc-refresh-unknown-401")
        .expect("find")
        .expect("exists");
    assert_eq!(unavailable.status, "unavailable");
}

/// 函数 `deactivation_reason_detects_workspace_and_account_scope`
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
fn deactivation_reason_detects_workspace_and_account_scope() {
    assert_eq!(
        deactivation_reason_from_message(
            "unexpected status 402 Payment Required: detail: code deactivated workspace"
        ),
        Some("workspace_deactivated")
    );
    assert_eq!(
        deactivation_reason_from_message(
            "unexpected status 402 Payment Required: detail: code deactivated_workspace"
        ),
        Some("workspace_deactivated")
    );
    assert_eq!(
        deactivation_reason_from_message("auth error: account_deactivated"),
        Some("account_deactivated")
    );
    assert_eq!(
        deactivation_reason_from_message("unexpected upstream code: team_deactivated"),
        Some("account_deactivated")
    );
    assert_eq!(
        deactivation_reason_from_message("usage endpoint status 429"),
        None
    );
}

/// 函数 `deactivation_error_marks_account_banned`
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
fn deactivation_error_marks_account_banned() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-workspace-deactivated".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(mark_account_unavailable_for_deactivation_error(
        &storage,
        "acc-workspace-deactivated",
        "unexpected status 402 Payment Required: detail: code deactivated workspace"
    ));
    let banned = storage
        .find_account_by_id("acc-workspace-deactivated")
        .expect("find")
        .expect("exists");
    assert_eq!(banned.status, "banned");
}

/// 函数 `generic_deactivated_error_marks_account_banned`
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
fn generic_deactivated_error_marks_account_banned() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-generic-deactivated".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(mark_account_unavailable_for_deactivation_error(
        &storage,
        "acc-generic-deactivated",
        "unexpected upstream code: team_deactivated"
    ));
    let banned = storage
        .find_account_by_id("acc-generic-deactivated")
        .expect("find")
        .expect("exists");
    assert_eq!(banned.status, "banned");

    let reasons = storage
        .latest_account_status_reasons(&["acc-generic-deactivated".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-generic-deactivated").map(String::as_str),
        Some("account_deactivated")
    );
}

/// 函数 `auth_error_deactivated_marks_account_banned`
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
fn auth_error_deactivated_marks_account_banned() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-auth-generic-deactivated".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    assert!(mark_account_unavailable_for_auth_error(
        &storage,
        "acc-auth-generic-deactivated",
        "refresh token failed with status 403 Forbidden: team_deactivated"
    ));
    let banned = storage
        .find_account_by_id("acc-auth-generic-deactivated")
        .expect("find")
        .expect("exists");
    assert_eq!(banned.status, "banned");

    let reasons = storage
        .latest_account_status_reasons(&["acc-auth-generic-deactivated".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons
            .get("acc-auth-generic-deactivated")
            .map(String::as_str),
        Some("account_deactivated")
    );
}

/// 函数 `deactivation_error_updates_reason_for_existing_unavailable_account`
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
fn deactivation_error_updates_reason_for_existing_unavailable_account() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account = Account {
        id: "acc-deactivated-reason-update".to_string(),
        label: "main".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).expect("insert");

    mark_usage_unreachable_if_needed(
        &storage,
        "acc-deactivated-reason-update",
        "usage endpoint status 401 Unauthorized",
    );
    assert!(mark_account_unavailable_for_deactivation_error(
        &storage,
        "acc-deactivated-reason-update",
        "account_deactivated"
    ));

    let banned = storage
        .find_account_by_id("acc-deactivated-reason-update")
        .expect("find")
        .expect("exists");
    assert_eq!(banned.status, "banned");

    let reasons = storage
        .latest_account_status_reasons(&["acc-deactivated-reason-update".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons
            .get("acc-deactivated-reason-update")
            .map(String::as_str),
        Some("account_deactivated")
    );
    assert_eq!(storage.event_count().expect("count events"), 2);
}

/// 函数 `deactivation_error_preserves_manual_disabled_status`
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
fn deactivation_error_preserves_manual_disabled_status() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_account(&Account {
            id: "acc-account-deactivated-disabled".to_string(),
            label: "main".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "disabled".to_string(),
            created_at: now_ts(),
            updated_at: now_ts(),
        })
        .expect("insert");

    assert!(!mark_account_unavailable_for_deactivation_error(
        &storage,
        "acc-account-deactivated-disabled",
        "account_deactivated"
    ));
    let disabled = storage
        .find_account_by_id("acc-account-deactivated-disabled")
        .expect("find")
        .expect("exists");
    assert_eq!(disabled.status, "disabled");
}

/// 函数 `refresh_retry_filter_matches_auth_failures`
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
fn refresh_retry_filter_matches_auth_failures() {
    assert!(should_retry_with_refresh("usage endpoint status 401"));
    assert!(should_retry_with_refresh("usage endpoint status 403"));
    assert!(!should_retry_with_refresh("usage endpoint status 429"));
}

/// 函数 `usage_refresh_failure_events_are_throttled_by_error_class`
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
fn usage_refresh_failure_events_are_throttled_by_error_class() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account_id = unique_id("acc-throttle");

    record_usage_refresh_failure(
        &storage,
        &account_id,
        "usage endpoint status 500 Internal Server Error",
    );
    record_usage_refresh_failure(
        &storage,
        &account_id,
        "usage endpoint status 500 upstream overloaded",
    );
    assert_eq!(storage.event_count().expect("count events"), 1);

    record_usage_refresh_failure(
        &storage,
        &account_id,
        "usage endpoint status 503 Service Unavailable",
    );
    assert_eq!(storage.event_count().expect("count events"), 2);

    record_usage_refresh_failure(
        &storage,
        &account_id,
        "usage endpoint failed: status=503 Service Unavailable body=upstream unavailable",
    );
    assert_eq!(storage.event_count().expect("count events"), 2);
}

/// 函数 `usage_refresh_failure_throttle_splits_401_reason_classes`
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
fn usage_refresh_failure_throttle_splits_401_reason_classes() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let account_id = unique_id("acc-throttle-401");

    record_usage_refresh_failure(
        &storage,
        &account_id,
        "refresh token failed with status 401 Unauthorized: Your access token could not be refreshed because your refresh token was revoked. Please log out and sign in again.",
    );
    record_usage_refresh_failure(
        &storage,
        &account_id,
        "refresh token failed with status 401 Unauthorized: Your access token could not be refreshed. Please log out and sign in again.",
    );

    assert_eq!(storage.event_count().expect("count events"), 2);
}
