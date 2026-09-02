use super::{
    analyze_gateway_error, classify_account_availability_signal, load_account_status_context,
    mark_account_limited_for_test_rate_limit, mark_account_unavailable_for_gateway_error,
    mark_account_unavailable_for_test_auth_status, restore_account_active_after_test,
    AccountAvailabilitySignal, GatewayErrorKind,
};
use codexmanager_core::storage::{now_ts, Account, Storage, UsageSnapshotRecord};

/// 函数 `classify_account_availability_signal_separates_usage_refresh_and_deactivation`
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
fn classify_account_availability_signal_separates_usage_refresh_and_deactivation() {
    assert!(matches!(
        classify_account_availability_signal("usage endpoint status 401 Unauthorized"),
        Some(AccountAvailabilitySignal::UsageHttp(401))
    ));
    assert!(matches!(
        classify_account_availability_signal("usage endpoint status 403 Forbidden"),
        Some(AccountAvailabilitySignal::UsageHttp(403))
    ));
    assert!(matches!(
        classify_account_availability_signal("usage endpoint status 429 Too Many Requests"),
        Some(AccountAvailabilitySignal::UsageHttp(429))
    ));
    assert!(matches!(
        classify_account_availability_signal("subscription endpoint status 403 Forbidden"),
        Some(AccountAvailabilitySignal::UsageHttp(403))
    ));
    assert!(matches!(
        classify_account_availability_signal(
            "subscription endpoint failed: status=401 Unauthorized body=token expired"
        ),
        Some(AccountAvailabilitySignal::UsageHttp(401))
    ));

    assert!(matches!(
        classify_account_availability_signal(
            "refresh token failed with status 401 Unauthorized: Your access token could not be refreshed because your refresh token was revoked. Please log out and sign in again."
        ),
        Some(AccountAvailabilitySignal::RefreshToken(
            crate::usage_http::RefreshTokenAuthErrorReason::Invalidated
        ))
    ));

    assert!(matches!(
        classify_account_availability_signal("account_deactivated"),
        Some(AccountAvailabilitySignal::Deactivation(
            "account_deactivated"
        ))
    ));

    let deactivation = analyze_gateway_error("Your OpenAI account has been deactivated", true);
    assert_eq!(deactivation.kind, GatewayErrorKind::Deactivation);
    assert!(deactivation.should_failover);
    assert!(deactivation.should_mark_account_unavailable);
    assert!(!deactivation.should_mark_default_cooldown);

    let usage_limit = analyze_gateway_error(
        "You've hit your usage limit. To get more access now, try again at 8:02 PM.",
        true,
    );
    assert_eq!(usage_limit.kind, GatewayErrorKind::UsageLimit);
    assert!(usage_limit.should_failover);
    assert!(usage_limit.should_mark_account_unavailable);
    assert!(usage_limit.should_mark_default_cooldown);

    let usage_limit_last = analyze_gateway_error(
        "You've hit your usage limit. To get more access now, try again at 8:02 PM.",
        false,
    );
    assert_eq!(usage_limit_last.kind, GatewayErrorKind::UsageLimit);
    assert!(!usage_limit_last.should_failover);
    assert!(usage_limit_last.should_mark_account_unavailable);
    assert!(!usage_limit_last.should_mark_default_cooldown);

    // Regression: backend-native WS upstream phrasing.
    let ws_usage_limit = analyze_gateway_error("The usage limit has been reached", true);
    assert_eq!(ws_usage_limit.kind, GatewayErrorKind::UsageLimit);
    assert!(ws_usage_limit.should_failover);
    assert!(ws_usage_limit.should_mark_account_unavailable);
    assert!(ws_usage_limit.should_mark_default_cooldown);

    let ws_usage_limit_code = analyze_gateway_error("usage_limit_reached", true);
    assert_eq!(ws_usage_limit_code.kind, GatewayErrorKind::UsageLimit);
    assert!(ws_usage_limit_code.should_failover);
}

/// 函数 `gateway_usage_limit_error_marks_account_limited_immediately`
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
fn gateway_usage_limit_error_marks_account_limited_immediately() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-usage-limit".to_string(),
            label: "usage-limit".to_string(),
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

    assert!(mark_account_unavailable_for_gateway_error(
        &storage,
        "acc-usage-limit",
        "You've hit your usage limit. To get more access now, try again at 8:02 PM."
    ));

    let account = storage
        .find_account_by_id("acc-usage-limit")
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.status, "limited");
    let reasons = storage
        .latest_account_status_reasons(&["acc-usage-limit".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-usage-limit").map(String::as_str),
        Some("usage_limit_exhausted")
    );
}

/// 函数 `gateway_usage_limit_error_marks_account_limited_when_snapshot_exhausted`
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
fn gateway_usage_limit_error_marks_account_limited_when_snapshot_exhausted() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-usage-exhausted".to_string(),
            label: "usage-exhausted".to_string(),
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
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc-usage-exhausted".to_string(),
            used_percent: Some(100.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(100.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");

    assert!(mark_account_unavailable_for_gateway_error(
        &storage,
        "acc-usage-exhausted",
        "You've hit your usage limit. To get more access now, try again at 8:02 PM."
    ));

    let account = storage
        .find_account_by_id("acc-usage-exhausted")
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.status, "limited");
    let reasons = storage
        .latest_account_status_reasons(&["acc-usage-exhausted".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-usage-exhausted").map(String::as_str),
        Some("usage_limit_exhausted")
    );
}

#[test]
fn stale_account_test_outcomes_do_not_overwrite_newer_banned_status() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account_id = "acc-stale-account-test";
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: "stale-account-test".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "unavailable".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    let start_context = load_account_status_context(&storage, account_id);

    storage
        .update_account_status(account_id, "banned")
        .expect("ban account while test is running");

    assert!(!restore_account_active_after_test(
        &storage,
        account_id,
        &start_context
    ));
    assert!(!mark_account_unavailable_for_test_auth_status(
        &storage,
        account_id,
        401,
        &start_context
    ));
    assert!(!mark_account_limited_for_test_rate_limit(
        &storage,
        account_id,
        &start_context
    ));
    assert_eq!(
        storage
            .find_account_status_by_id(account_id)
            .expect("read account status")
            .as_deref(),
        Some("banned")
    );
}

#[test]
fn account_test_success_never_restores_an_existing_banned_status() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account_id = "acc-banned-before-test";
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: "banned-before-test".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "banned".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    let context = load_account_status_context(&storage, account_id);

    assert!(!restore_account_active_after_test(
        &storage, account_id, &context
    ));
    assert_eq!(
        storage
            .find_account_status_by_id(account_id)
            .expect("read account status")
            .as_deref(),
        Some("banned")
    );
}
