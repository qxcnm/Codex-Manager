use super::{
    account_matches_plan_filter_with_snapshot, extract_plan_type_from_credits_json,
    extract_plan_type_from_id_token, is_free_or_single_window_account_with_snapshot,
    is_free_plan_from_credits_json, is_free_plan_type, is_single_window_long_usage_snapshot,
    normalize_plan_type, resolve_account_plan, resolve_effective_account_plan,
    resolve_effective_account_plan_at,
};
use codexmanager_core::storage::{
    now_ts, Account, AccountSubscription, AccountTokenPlan, Storage, Token, UsageSnapshotRecord,
};

/// 函数 `encode_base64url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - bytes: 参数 bytes
///
/// # 返回
/// 返回函数执行结果
fn encode_base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let chunk = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(chunk & 0x3f) as usize] as char);
        index += 3;
    }
    match bytes.len().saturating_sub(index) {
        1 => {
            let chunk = (bytes[index] as u32) << 16;
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        }
        2 => {
            let chunk = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        }
        _ => {}
    }
    out
}

/// 函数 `free_plan_detection_accepts_common_variants`
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
fn free_plan_detection_accepts_common_variants() {
    assert!(is_free_plan_type(Some("free")));
    assert!(is_free_plan_type(Some("ChatGPT_Free")));
    assert!(is_free_plan_type(Some("free_tier")));
}

/// 函数 `free_plan_detection_rejects_paid_or_unknown_variants`
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
fn free_plan_detection_rejects_paid_or_unknown_variants() {
    assert!(!is_free_plan_type(None));
    assert!(!is_free_plan_type(Some("")));
    assert!(!is_free_plan_type(Some("plus")));
    assert!(!is_free_plan_type(Some("pro")));
    assert!(!is_free_plan_type(Some("team")));
}

/// 函数 `free_plan_detection_accepts_credits_json_marker`
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
fn free_plan_detection_accepts_credits_json_marker() {
    let credits_json = r#"{"planType":"free"}"#;
    assert!(is_free_plan_from_credits_json(Some(credits_json)));
}

/// 函数 `extract_plan_type_from_credits_json_reads_nested_value`
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
fn extract_plan_type_from_credits_json_reads_nested_value() {
    let credits_json = r#"{"subscription":{"planType":"business"}}"#;
    assert_eq!(
        extract_plan_type_from_credits_json(Some(credits_json)).as_deref(),
        Some("business")
    );
}

/// 函数 `extract_plan_type_from_id_token_reads_chatgpt_claim`
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
fn extract_plan_type_from_id_token_reads_chatgpt_claim() {
    let header = encode_base64url(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = encode_base64url(
        serde_json::json!({
            "sub": "acc-plan-free",
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "free"
            }
        })
        .to_string()
        .as_bytes(),
    );
    let token = format!("{header}.{payload}.sig");
    assert_eq!(
        extract_plan_type_from_id_token(&token).as_deref(),
        Some("free")
    );
}

/// 函数 `single_window_long_usage_snapshot_counts_as_free_like`
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
fn single_window_long_usage_snapshot_counts_as_free_like() {
    let snapshot = UsageSnapshotRecord {
        account_id: "acc-free".to_string(),
        used_percent: Some(20.0),
        window_minutes: Some(10_080),
        resets_at: None,
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    };

    assert!(is_single_window_long_usage_snapshot(&snapshot));
}

/// 函数 `free_or_single_window_account_accepts_weekly_single_window_without_plan_claim`
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
fn free_or_single_window_account_accepts_weekly_single_window_without_plan_claim() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-weekly".to_string(),
            label: "acc-weekly".to_string(),
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
    let token = Token {
        account_id: "acc-weekly".to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: "header.payload.sig".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now,
    };
    storage.insert_token(&token).expect("insert token");
    let snapshot = UsageSnapshotRecord {
        account_id: "acc-weekly".to_string(),
        used_percent: Some(25.0),
        window_minutes: Some(10_080),
        resets_at: None,
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now,
    };

    assert!(is_free_or_single_window_account_with_snapshot(
        &token,
        Some(&snapshot)
    ));
}

/// 函数 `normalize_plan_type_maps_known_variants`
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
fn normalize_plan_type_maps_known_variants() {
    assert_eq!(
        normalize_plan_type("ChatGPT_Free").map(|plan| (plan.normalized, plan.raw)),
        Some(("free".to_string(), Some("ChatGPT_Free".to_string())))
    );
    assert_eq!(
        normalize_plan_type("education").map(|plan| (plan.normalized, plan.raw)),
        Some(("edu".to_string(), Some("education".to_string())))
    );
    assert_eq!(
        normalize_plan_type("pro").map(|plan| (plan.normalized, plan.raw)),
        Some(("pro".to_string(), None))
    );
}

/// 函数 `resolve_account_plan_prefers_token_claims_and_falls_back_to_usage`
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
fn resolve_account_plan_prefers_token_claims_and_falls_back_to_usage() {
    let token = Token {
        account_id: "acc-plus".to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: {
            let header = encode_base64url(br#"{"alg":"none","typ":"JWT"}"#);
            let payload = encode_base64url(
                serde_json::json!({
                    "sub": "acc-plus",
                    "https://api.openai.com/auth": {
                        "chatgpt_plan_type": "plus"
                    }
                })
                .to_string()
                .as_bytes(),
            );
            format!("{header}.{payload}.sig")
        },
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };
    let usage = UsageSnapshotRecord {
        account_id: "acc-plus".to_string(),
        used_percent: Some(10.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(20.0),
        secondary_window_minutes: Some(10_080),
        secondary_resets_at: None,
        credits_json: Some(r#"{"planType":"free"}"#.to_string()),
        captured_at: now_ts(),
    };

    let token_plan = super::token_plan_from_token(&token);
    let resolved = resolve_account_plan(Some(&token_plan), Some(&usage)).expect("resolve plan");
    assert_eq!(resolved.normalized, "plus");

    let effective_from_token =
        resolve_effective_account_plan(Some(&token_plan), Some(&usage), None)
            .expect("resolve effective plan without subscription record");
    assert_eq!(effective_from_token.normalized, "plus");

    let effective_from_usage = resolve_effective_account_plan(None, Some(&usage), None)
        .expect("resolve effective usage plan without subscription record");
    assert_eq!(effective_from_usage.normalized, "free");
}

#[test]
fn resolve_effective_account_plan_treats_inactive_subscription_as_free() {
    let now = now_ts();
    let token = AccountTokenPlan {
        account_id: "acc-downgraded".to_string(),
        id_token: String::new(),
        access_token: String::new(),
    };
    let snapshot = UsageSnapshotRecord {
        account_id: token.account_id.clone(),
        used_percent: Some(10.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(20.0),
        secondary_window_minutes: Some(10_080),
        secondary_resets_at: None,
        credits_json: Some(r#"{"planType":"plus"}"#.to_string()),
        captured_at: now,
    };
    let subscription = AccountSubscription {
        account_id: token.account_id.clone(),
        has_subscription: false,
        account_plan_type: Some("pro".to_string()),
        plan_type: Some("plus".to_string()),
        expires_at: Some(now + 3_600),
        renews_at: None,
        updated_at: now,
    };

    let resolved =
        resolve_effective_account_plan(Some(&token), Some(&snapshot), Some(&subscription))
            .expect("resolve inactive subscription plan");

    assert_eq!(resolved.normalized, "free");
    assert_eq!(resolved.raw, None);
}

#[test]
fn resolve_effective_account_plan_treats_expired_subscription_as_free() {
    let now = now_ts();
    let subscription = AccountSubscription {
        account_id: "acc-expired".to_string(),
        has_subscription: true,
        account_plan_type: Some("pro".to_string()),
        plan_type: Some("plus".to_string()),
        expires_at: Some(now),
        renews_at: None,
        updated_at: now,
    };

    let resolved = resolve_effective_account_plan_at(None, None, Some(&subscription), now)
        .expect("resolve subscription expiring at the current instant");

    assert_eq!(resolved.normalized, "free");
    assert_eq!(resolved.raw, None);
}

#[test]
fn resolve_effective_account_plan_keeps_active_unexpired_subscription_plan() {
    let now = now_ts();
    let subscription = AccountSubscription {
        account_id: "acc-active".to_string(),
        has_subscription: true,
        account_plan_type: Some("pro".to_string()),
        plan_type: Some("plus".to_string()),
        expires_at: Some(now + 3_600),
        renews_at: None,
        updated_at: now,
    };

    let resolved = resolve_effective_account_plan_at(None, None, Some(&subscription), now)
        .expect("resolve active subscription plan");

    assert_eq!(resolved.normalized, "pro");

    let subscription_without_expiry = AccountSubscription {
        expires_at: None,
        ..subscription
    };
    let resolved =
        resolve_effective_account_plan_at(None, None, Some(&subscription_without_expiry), now)
            .expect("resolve active subscription plan without expiry");

    assert_eq!(resolved.normalized, "pro");
}

#[test]
fn resolve_token_account_plan_reads_token_claim_without_usage_snapshot() {
    let token = Token {
        account_id: "acc-go".to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: {
            let header = encode_base64url(br#"{"alg":"none","typ":"JWT"}"#);
            let payload = encode_base64url(
                serde_json::json!({
                    "sub": "acc-go",
                    "https://api.openai.com/auth": {
                        "chatgpt_plan_type": "go"
                    }
                })
                .to_string()
                .as_bytes(),
            );
            format!("{header}.{payload}.sig")
        },
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    let resolved = super::resolve_token_account_plan(&token).expect("resolve token plan");

    assert_eq!(resolved.normalized, "go");
}

#[test]
fn account_plan_filter_with_preloaded_snapshot_matches_usage_plan() {
    let token = Token {
        account_id: "acc-free".to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: "header.payload.sig".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };
    let usage = UsageSnapshotRecord {
        account_id: "acc-free".to_string(),
        used_percent: Some(10.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(20.0),
        secondary_window_minutes: Some(10_080),
        secondary_resets_at: None,
        credits_json: Some(r#"{"planType":"free"}"#.to_string()),
        captured_at: now_ts(),
    };

    assert!(super::account_matches_plan_filter_with_snapshot(
        &token,
        Some(&usage),
        Some("free")
    ));
    assert!(!super::account_matches_plan_filter_with_snapshot(
        &token,
        Some(&usage),
        Some("plus")
    ));
}

/// 函数 `account_plan_filter_unknown_accepts_unresolved_accounts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-10
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn account_plan_filter_unknown_accepts_unresolved_accounts() {
    let now = now_ts();
    let token = Token {
        account_id: "acc-unknown".to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: "header.payload.sig".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now,
    };

    assert!(account_matches_plan_filter_with_snapshot(
        &token,
        None,
        Some("unknown"),
    ));
    assert!(!account_matches_plan_filter_with_snapshot(
        &token,
        None,
        Some("plus"),
    ));
}
