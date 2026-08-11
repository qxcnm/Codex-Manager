use super::{
    analyze_gateway_error, classify_account_availability_signal, derive_credential_state,
    mark_account_unavailable_for_gateway_error, AccountAvailabilitySignal, CredentialStateSnapshot,
    GatewayErrorKind,
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
    assert_eq!(
        classify_account_availability_signal(
            "subscription endpoint failed: status=403 Forbidden body=Cloudflare 安全验证页（ray=test, zone=chatgpt.com） [kind=cloudflare_challenge]"
        ),
        None
    );
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
}

#[test]
fn credential_state_keeps_repairable_tokens_separate_from_deactivation() {
    let snapshot = |status, reason, has_token, expires_at| {
        derive_credential_state(status, reason, has_token, expires_at, 1_000)
    };

    assert_eq!(
        snapshot(
            "unavailable",
            Some("refresh_token_invalid:refresh_token_expired"),
            true,
            Some(900),
        ),
        CredentialStateSnapshot {
            state: "refresh_token_expired",
            action: "reauthenticate",
        }
    );
    assert_eq!(
        snapshot(
            "unavailable",
            Some("refresh_token_invalid:refresh_token_invalidated"),
            true,
            Some(2_000),
        ),
        CredentialStateSnapshot {
            state: "refresh_token_revoked",
            action: "reauthenticate",
        }
    );
    assert_eq!(
        snapshot("active", None, true, Some(900)),
        CredentialStateSnapshot {
            state: "access_token_expired",
            action: "refresh",
        }
    );
    assert_eq!(
        snapshot("banned", Some("account_deactivated"), true, Some(2_000)),
        CredentialStateSnapshot {
            state: "account_deactivated",
            action: "stop",
        }
    );
    assert_eq!(
        snapshot(
            "unavailable",
            Some("usage_cloudflare_challenge"),
            true,
            Some(2_000),
        ),
        CredentialStateSnapshot {
            state: "network_unknown",
            action: "retry_network",
        }
    );
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
