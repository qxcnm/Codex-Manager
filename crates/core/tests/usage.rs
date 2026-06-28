use codexmanager_core::usage::{
    accounts_check_endpoint, rate_limit_reset_credits_consume_endpoint,
    rate_limit_reset_credits_endpoint, parse_usage_snapshot, usage_endpoint,
};
use serde_json::json;

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
        "credits": { "balance": 12.5 }
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

    let reset_credits_url = rate_limit_reset_credits_endpoint("https://chatgpt.com");
    assert_eq!(
        reset_credits_url,
        "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits"
    );

    let reset_consume_url = rate_limit_reset_credits_consume_endpoint("https://chatgpt.com");
    assert_eq!(
        reset_consume_url,
        "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume"
    );
}
