use codexmanager_core::usage::{
    accounts_check_endpoint, has_usable_luna_reserve, has_usable_luna_reserve_at,
    is_luna_catalog_model, is_luna_reserve_model, merge_missing_extra_rate_limits,
    parse_reset_credits_snapshot, parse_usage_snapshot, reset_credits_consume_endpoint,
    reset_credits_endpoint, usage_endpoint, usage_payload_declares_extra_rate_limits,
};
use serde_json::{json, Value};

/// 函数 `usage_snapshot_parsed`
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
fn usage_snapshot_parsed() {
    let payload = json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 25.0,
                "limit_window_seconds": 900,
                "reset_at": 1730947200
            },
            "secondary_window": {
                "used_percent": 80.0,
                "limit_window_seconds": 120,
                "reset_at": 1730947260
            }
        },
        "code_review_rate_limit": {
            "allowed": true,
            "limit_reached": false,
            "primary_window": {
                "used_percent": 10.0,
                "limit_window_seconds": 604800,
                "reset_at": 1731552000
            }
        },
        "additional_rate_limits": [
            {
                "limit_name": "Spark",
                "metered_feature": "codex_other",
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": 40.0,
                        "limit_window_seconds": 86400,
                        "reset_at": 1731033600
                    }
                }
            }
        ],
        "credits": { "balance": 12.5 },
        "rate_limit_reset_credits": { "available_count": 2 }
    });

    let snap = parse_usage_snapshot(&payload);
    assert_eq!(snap.used_percent, Some(25.0));
    assert_eq!(snap.window_minutes, Some(15));
    assert_eq!(snap.resets_at, Some(1730947200));
    assert_eq!(snap.secondary_used_percent, Some(80.0));
    assert_eq!(snap.secondary_window_minutes, Some(2));
    assert_eq!(snap.secondary_resets_at, Some(1730947260));
    let credits: serde_json::Value =
        serde_json::from_str(snap.credits_json.as_deref().expect("credits json"))
            .expect("parse credits json");
    assert_eq!(credits["balance"], 12.5);
    assert_eq!(credits["rate_limit_reset_credits"]["available_count"], 2);
    let extras = credits["_codexmanager_extra_rate_limits"]
        .as_array()
        .expect("extra rate limits array");
    assert_eq!(extras.len(), 2);
    assert_eq!(extras[0]["source_key"], "code_review_rate_limit");
    assert_eq!(extras[1]["source_key"], "codex_other");
    assert_eq!(extras[1]["limit_id"], "codex_other");
    assert_eq!(extras[1]["limit_name"], "Spark");
    assert_eq!(extras[1]["allowed"], true);
    assert_eq!(extras[1]["limit_reached"], false);
    assert_eq!(extras[1]["primary_window"]["used_percent"], 40.0);

    let url = usage_endpoint("https://chatgpt.com");
    assert_eq!(url, "https://chatgpt.com/backend-api/wham/usage");

    let accounts_check_url = accounts_check_endpoint("https://chatgpt.com");
    assert_eq!(
        accounts_check_url,
        "https://chatgpt.com/backend-api/accounts/check/v4-2023-04-27"
    );

    assert_eq!(
        reset_credits_endpoint("https://chatgpt.com"),
        "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits"
    );
    assert_eq!(
        reset_credits_consume_endpoint("https://chatgpt.com"),
        "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume"
    );
}

#[test]
fn reset_credit_snapshot_parses_compatible_fields() {
    let future = chrono::Utc::now().timestamp() + 3600;
    let past = chrono::Utc::now().timestamp() - 3600;
    let snapshot = parse_reset_credits_snapshot(&json!({
        "credits": [
            { "creditId": "available", "state": "available", "expiresAt": future * 1000 },
            { "id": "expired", "expires_at": past },
            { "id": "used", "status": "redeemed", "redeemed_at": past },
            { "id": "unknown", "status": "pending", "expires_at": future }
        ]
    }));

    assert_eq!(snapshot.available_count, Some(1));
    assert_eq!(snapshot.credits.len(), 4);
    assert_eq!(snapshot.credits[0].id.as_deref(), Some("available"));
    assert_eq!(snapshot.credits[0].expires_at, Some(future));
    assert_eq!(snapshot.next_expires_at, Some(future));
    assert_eq!(snapshot.credits[1].status.as_deref(), Some("expired"));
}

#[test]
fn luna_reserve_survives_camel_case_usage_payload_and_exhausted_standard_window() {
    let payload = json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 100.0,
                "limit_window_seconds": 18000
            },
            "secondary_window": {
                "used_percent": 100.0,
                "limit_window_seconds": 604800
            }
        },
        "additionalRateLimits": [
            {
                "limitName": "Luna Reserve",
                "meteredFeature": "base_model_inference",
                "allowed": true,
                "limitReached": false,
                "rateLimit": {
                    "primaryWindow": {
                        "usedPercent": 0.0,
                        "remainingPercent": 100.0,
                        "limitWindowSeconds": 604800
                    }
                }
            }
        ]
    });

    let snapshot = parse_usage_snapshot(&payload);
    assert!(has_usable_luna_reserve(snapshot.credits_json.as_deref()));
    assert!(is_luna_reserve_model(Some("gpt-reserve")));
    assert!(is_luna_reserve_model(Some(" GPT-RESERVE ")));
    assert!(!is_luna_reserve_model(Some("gpt-5.6-luna")));
    assert!(is_luna_catalog_model(Some("gpt-6-luna")));
    assert!(is_luna_catalog_model(Some(" GPT-5.6-LUNA ")));
    assert!(!is_luna_catalog_model(Some("gpt-reserve")));
    assert!(!is_luna_reserve_model(Some("custom-luna-router")));
    assert!(!is_luna_reserve_model(Some("gpt-reserve-preview")));
    assert!(!is_luna_reserve_model(Some("gpt-5.6")));

    let credits: serde_json::Value =
        serde_json::from_str(snapshot.credits_json.as_deref().expect("credits json"))
            .expect("parse credits json");
    let reserve = &credits["_codexmanager_extra_rate_limits"][0];
    assert_eq!(reserve["limit_name"], "Luna Reserve");
    assert_eq!(reserve["metered_feature"], "base_model_inference");
    assert_eq!(reserve["primary_window"]["remainingPercent"], 100.0);
}

#[test]
fn luna_reserve_is_unusable_when_explicitly_reached_or_empty() {
    for reserve in [
        json!({
            "limitName": "Luna Reserve",
            "limitReached": true,
            "rateLimit": { "primaryWindow": { "remainingPercent": 100.0 } }
        }),
        json!({
            "limitName": "Luna Reserve",
            "rateLimit": { "primaryWindow": { "remainingPercent": 0.0 } }
        }),
    ] {
        let payload = json!({
            "rate_limit": {
                "primary_window": {
                    "used_percent": 100.0,
                    "limit_window_seconds": 18000
                }
            },
            "additionalRateLimits": [reserve]
        });
        let snapshot = parse_usage_snapshot(&payload);
        assert!(!has_usable_luna_reserve(snapshot.credits_json.as_deref()));
    }
}

#[test]
fn luna_reserve_requires_explicit_reset_timestamps_to_be_current() {
    let now = 2_000_000_000_i64;
    let credits = |reset_at: Value| {
        json!({
            "_codexmanager_extra_rate_limits": [{
                "limit_name": "Luna Reserve",
                "primary_window": {
                    "remainingPercent": 100.0,
                    "resetAt": reset_at
                }
            }]
        })
        .to_string()
    };

    for expired in [
        json!(now),
        json!(now - 1),
        json!(now * 1000),
        json!(((now - 1) * 1000).to_string()),
        json!("not-a-timestamp"),
    ] {
        assert!(
            !has_usable_luna_reserve_at(Some(&credits(expired)), now),
            "expired or invalid reset timestamp must fail closed"
        );
    }
    for future in [
        json!(now + 1),
        json!((now + 1) * 1000),
        json!(((now + 1) * 1000).to_string()),
    ] {
        assert!(has_usable_luna_reserve_at(Some(&credits(future)), now));
    }
    let legacy_without_reset = json!({
        "_codexmanager_extra_rate_limits": [{
            "limit_name": "Luna Reserve",
            "primary_window": { "remaining_percent": 1.0 }
        }]
    })
    .to_string();
    assert!(has_usable_luna_reserve_at(Some(&legacy_without_reset), now));
}

#[test]
fn generic_base_model_inference_is_not_luna_reserve() {
    let payload = json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 100.0,
                "limit_window_seconds": 18000
            }
        },
        "additionalRateLimits": [{
            "meteredFeature": "base_model_inference",
            "rateLimit": {
                "primaryWindow": {
                    "remainingPercent": 100.0,
                    "limitWindowSeconds": 604800
                }
            }
        }]
    });

    let snapshot = parse_usage_snapshot(&payload);
    assert!(!has_usable_luna_reserve(snapshot.credits_json.as_deref()));
}

#[test]
fn nested_extra_rate_limits_are_preserved_and_explicit_empty_is_authoritative() {
    let first = json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 100.0,
                "limit_window_seconds": 18000
            }
        },
        "credits": {
            "additionalRateLimits": [{
                "limitName": "Luna Reserve",
                "rateLimit": { "primaryWindow": { "remainingPercent": 70.0 } }
            }]
        }
    });
    let first_snapshot = parse_usage_snapshot(&first);
    assert!(usage_payload_declares_extra_rate_limits(&first));
    assert!(has_usable_luna_reserve(
        first_snapshot.credits_json.as_deref()
    ));

    let second = json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 100.0,
                "limit_window_seconds": 18000
            }
        },
        "credits": { "balance": 1.0 }
    });
    assert!(!usage_payload_declares_extra_rate_limits(&second));
    let merged = merge_missing_extra_rate_limits(
        parse_usage_snapshot(&second).credits_json.as_deref(),
        first_snapshot.credits_json.as_deref(),
    );
    assert!(has_usable_luna_reserve(merged.as_deref()));

    let explicit_empty = json!({
        "rate_limit": { "primary_window": { "used_percent": 100.0 } },
        "additionalRateLimits": []
    });
    assert!(usage_payload_declares_extra_rate_limits(&explicit_empty));
    assert!(!has_usable_luna_reserve(
        parse_usage_snapshot(&explicit_empty)
            .credits_json
            .as_deref()
    ));

    let mixed_followup = json!({
        "rate_limit": { "primary_window": { "used_percent": 100.0 } },
        "code_review_rate_limit": {
            "primary_window": {
                "used_percent": 0.0,
                "limit_window_seconds": 18000
            }
        },
        "additional_rate_limits": null
    });
    assert!(!usage_payload_declares_extra_rate_limits(&mixed_followup));
    let merged_mixed = merge_missing_extra_rate_limits(
        parse_usage_snapshot(&mixed_followup)
            .credits_json
            .as_deref(),
        first_snapshot.credits_json.as_deref(),
    )
    .expect("mixed usage payload should be serialized");
    let merged_mixed_value: Value = serde_json::from_str(&merged_mixed).expect("valid merged JSON");
    let merged_entries = merged_mixed_value
        .get("_codexmanager_extra_rate_limits")
        .and_then(Value::as_array)
        .expect("merged extra rate limits");
    assert_eq!(merged_entries.len(), 2);
    assert!(has_usable_luna_reserve(Some(&merged_mixed)));

    let legacy_previous = json!({
        "_codexmanager_extra_rate_limits": [],
        "additionalRateLimits": [{
            "limitName": "Luna Reserve",
            "meteredFeature": "base_model_inference",
            "rateLimit": { "primaryWindow": { "remainingPercent": 65.0 } }
        }]
    });
    let merged_legacy = merge_missing_extra_rate_limits(
        parse_usage_snapshot(&mixed_followup)
            .credits_json
            .as_deref(),
        Some(&legacy_previous.to_string()),
    )
    .expect("legacy usage payload should be serialized");
    assert!(has_usable_luna_reserve(Some(&merged_legacy)));
}
