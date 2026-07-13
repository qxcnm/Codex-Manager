use codexmanager_core::{
    auth::parse_id_token_claims,
    storage::{AccountSubscription, Storage, Token, UsageSnapshotRecord},
};
use serde_json::Value;

const MINUTES_PER_HOUR: i64 = 60;
const MINUTES_PER_DAY: i64 = 24 * MINUTES_PER_HOUR;
const ROUNDING_BIAS: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAccountPlan {
    pub(crate) normalized: String,
    pub(crate) raw: Option<String>,
}

/// 函数 `extract_plan_type_from_id_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn extract_plan_type_from_id_token(id_token: &str) -> Option<String> {
    parse_id_token_claims(id_token)
        .ok()
        .and_then(|claims| claims.auth)
        .and_then(|auth| auth.chatgpt_plan_type)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

/// 函数 `is_free_plan_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn is_free_plan_type(plan_type: Option<&str>) -> bool {
    let Some(plan_type) = plan_type else {
        return false;
    };
    let normalized = plan_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("free")
}

pub(crate) fn is_workspace_plan_type(plan_type: Option<&str>) -> bool {
    let Some(plan_type) = plan_type else {
        return false;
    };
    let normalized = plan_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    normalized.contains("business")
        || normalized.contains("team")
        || normalized.contains("enterprise")
        || normalized == "edu"
        || normalized.contains("education")
}

/// 函数 `is_free_plan_from_credits_json`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn is_free_plan_from_credits_json(raw_credits_json: Option<&str>) -> bool {
    is_free_plan_type(extract_plan_type_from_credits_json(raw_credits_json).as_deref())
}

pub(crate) fn normalize_account_plan_filter(
    value: Option<String>,
) -> Result<Option<String>, String> {
    let trimmed = value.as_deref().map(str::trim).unwrap_or_default();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") || trimmed == "全部" {
        return Ok(None);
    }

    let normalized = trimmed.to_ascii_lowercase();
    let canonical = match normalized.as_str() {
        "free" => "free",
        "go" => "go",
        "plus" => "plus",
        "pro" => "pro",
        "team" => "team",
        "business" => "business",
        "enterprise" => "enterprise",
        "edu" | "education" => "edu",
        "unknown" => "unknown",
        _ => return Err(format!("unsupported account plan filter: {trimmed}")),
    };

    Ok(Some(canonical.to_string()))
}

/// 函数 `resolve_account_plan`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_account_plan(
    token: Option<&Token>,
    snapshot: Option<&UsageSnapshotRecord>,
) -> Option<ResolvedAccountPlan> {
    let token_plan = token
        .and_then(|value| extract_plan_type_from_id_token(&value.access_token))
        .or_else(|| token.and_then(|value| extract_plan_type_from_id_token(&value.id_token)));
    if let Some(plan) = token_plan.as_deref().and_then(normalize_plan_type) {
        return Some(plan);
    }

    let usage_plan = snapshot
        .and_then(|value| extract_plan_type_from_credits_json(value.credits_json.as_deref()));
    if let Some(plan) = usage_plan.as_deref().and_then(normalize_plan_type) {
        return Some(plan);
    }

    if snapshot.is_some_and(is_single_window_long_usage_snapshot) {
        return Some(ResolvedAccountPlan {
            normalized: "free".to_string(),
            raw: None,
        });
    }

    None
}

pub(crate) fn resolve_effective_account_plan(
    token: Option<&Token>,
    snapshot: Option<&UsageSnapshotRecord>,
    subscription: Option<&AccountSubscription>,
) -> Option<ResolvedAccountPlan> {
    if let Some(plan) = subscription
        .and_then(|value| value.account_plan_type.as_deref())
        .and_then(normalize_plan_type)
    {
        return Some(plan);
    }
    if let Some(plan) = subscription
        .and_then(|value| value.plan_type.as_deref())
        .and_then(normalize_plan_type)
    {
        return Some(plan);
    }

    resolve_account_plan(token, snapshot)
}

pub(crate) fn normalize_account_plan_value(value: &str) -> Option<String> {
    normalize_plan_type(value).map(|plan| plan.normalized)
}

/// 函数 `extract_plan_type_from_credits_json`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn extract_plan_type_from_credits_json(
    raw_credits_json: Option<&str>,
) -> Option<String> {
    let Some(raw_credits_json) = raw_credits_json else {
        return None;
    };
    let Ok(value) = serde_json::from_str::<Value>(raw_credits_json) else {
        return None;
    };
    let keys = [
        "plan_type",
        "planType",
        "subscription_tier",
        "subscriptionTier",
        "tier",
        "account_type",
        "accountType",
        "type",
    ];
    extract_string_by_keys_recursive(&value, &keys)
}

/// 函数 `is_single_window_long_usage_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn is_single_window_long_usage_snapshot(snapshot: &UsageSnapshotRecord) -> bool {
    let has_primary_signal = snapshot.used_percent.is_some() || snapshot.window_minutes.is_some();
    let has_secondary_signal =
        snapshot.secondary_used_percent.is_some() || snapshot.secondary_window_minutes.is_some();
    has_primary_signal && !has_secondary_signal && is_long_window(snapshot.window_minutes)
}

/// 函数 `is_free_or_single_window_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn is_free_or_single_window_account(
    storage: &Storage,
    account_id: &str,
    token: &Token,
) -> bool {
    if is_free_plan_type(extract_plan_type_from_id_token(&token.id_token).as_deref())
        || is_free_plan_type(extract_plan_type_from_id_token(&token.access_token).as_deref())
    {
        return true;
    }

    storage
        .latest_usage_snapshot_for_account(account_id)
        .ok()
        .flatten()
        .map(|snapshot| {
            is_free_plan_from_credits_json(snapshot.credits_json.as_deref())
                || is_single_window_long_usage_snapshot(&snapshot)
        })
        .unwrap_or(false)
}

pub(crate) fn account_matches_plan_filter(
    storage: &Storage,
    account_id: &str,
    token: &Token,
    plan_filter: Option<&str>,
) -> bool {
    let Some(filter) = plan_filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    if filter.eq_ignore_ascii_case("all") {
        return true;
    }

    let normalized_filter = filter.to_ascii_lowercase();
    let snapshot = storage
        .latest_usage_snapshot_for_account(account_id)
        .ok()
        .flatten();
    let subscription = storage.find_account_subscription(account_id).ok().flatten();
    match resolve_effective_account_plan(Some(token), snapshot.as_ref(), subscription.as_ref()) {
        Some(plan) => plan.normalized == normalized_filter,
        None => normalized_filter == "unknown",
    }
}

/// 函数 `is_long_window`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - window_minutes: 参数 window_minutes
///
/// # 返回
/// 返回函数执行结果
fn is_long_window(window_minutes: Option<i64>) -> bool {
    window_minutes.is_some_and(|value| value > MINUTES_PER_DAY + ROUNDING_BIAS)
}

/// 函数 `extract_string_by_keys_recursive`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
/// - keys: 参数 keys
///
/// # 返回
/// 返回函数执行结果
fn extract_string_by_keys_recursive(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(object) = value.as_object() {
        for key in keys {
            let candidate = object
                .get(*key)
                .and_then(Value::as_str)
                .map(|text| text.trim().to_ascii_lowercase())
                .filter(|text| !text.is_empty());
            if candidate.is_some() {
                return candidate;
            }
        }
        for child in object.values() {
            let nested = extract_string_by_keys_recursive(child, keys);
            if nested.is_some() {
                return nested;
            }
        }
        return None;
    }
    if let Some(array) = value.as_array() {
        for child in array {
            let nested = extract_string_by_keys_recursive(child, keys);
            if nested.is_some() {
                return nested;
            }
        }
    }
    None
}

/// 函数 `normalize_plan_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_plan_type(value: &str) -> Option<ResolvedAccountPlan> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.to_ascii_lowercase();
    let known = if normalized.contains("free") {
        Some("free")
    } else if normalized == "go" || normalized.ends_with("_go") || normalized.contains("chatgpt_go")
    {
        Some("go")
    } else if normalized.contains("plus") {
        Some("plus")
    } else if normalized.contains("business") {
        Some("business")
    } else if normalized.contains("team") {
        Some("team")
    } else if normalized.contains("enterprise") {
        Some("enterprise")
    } else if normalized == "edu" || normalized.contains("education") {
        Some("edu")
    } else if normalized.contains("pro") {
        Some("pro")
    } else {
        None
    };

    Some(match known {
        Some(plan) => ResolvedAccountPlan {
            normalized: plan.to_string(),
            raw: if plan == normalized {
                None
            } else {
                Some(trimmed.to_string())
            },
        },
        None => ResolvedAccountPlan {
            normalized: "unknown".to_string(),
            raw: Some(trimmed.to_string()),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{
        account_matches_plan_filter, extract_plan_type_from_credits_json,
        extract_plan_type_from_id_token, is_free_or_single_window_account,
        is_free_plan_from_credits_json, is_free_plan_type, is_single_window_long_usage_snapshot,
        is_workspace_plan_type, normalize_plan_type, resolve_account_plan,
    };
    use codexmanager_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};

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
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
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

    #[test]
    fn workspace_plan_detection_accepts_business_variants() {
        assert!(is_workspace_plan_type(Some("business")));
        assert!(is_workspace_plan_type(Some("chatgpt_business")));
        assert!(is_workspace_plan_type(Some("enterprise")));
        assert!(is_workspace_plan_type(Some("edu")));
        assert!(!is_workspace_plan_type(Some("plus")));
        assert!(!is_workspace_plan_type(Some("pro")));
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
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: "acc-weekly".to_string(),
                used_percent: Some(25.0),
                window_minutes: Some(10_080),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage");

        assert!(is_free_or_single_window_account(
            &storage,
            "acc-weekly",
            &token
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

        let resolved = resolve_account_plan(Some(&token), Some(&usage)).expect("resolve plan");
        assert_eq!(resolved.normalized, "plus");
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
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();
        storage
            .insert_account(&Account {
                id: "acc-unknown".to_string(),
                label: "acc-unknown".to_string(),
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
            account_id: "acc-unknown".to_string(),
            id_token: "header.payload.sig".to_string(),
            access_token: "header.payload.sig".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        };
        storage.insert_token(&token).expect("insert token");

        assert!(account_matches_plan_filter(
            &storage,
            "acc-unknown",
            &token,
            Some("unknown"),
        ));
        assert!(!account_matches_plan_filter(
            &storage,
            "acc-unknown",
            &token,
            Some("plus"),
        ));
    }

    #[test]
    fn account_plan_filter_uses_subscription_plan_type() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();
        storage
            .insert_account(&Account {
                id: "acc-business".to_string(),
                label: "acc-business".to_string(),
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
            account_id: "acc-business".to_string(),
            id_token: "header.payload.sig".to_string(),
            access_token: "header.payload.sig".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        };
        storage.insert_token(&token).expect("insert token");
        storage
            .upsert_account_subscription(
                "acc-business",
                true,
                None,
                Some("business"),
                None,
                None,
            )
            .expect("insert subscription");

        assert!(account_matches_plan_filter(
            &storage,
            "acc-business",
            &token,
            Some("business"),
        ));
        assert!(!account_matches_plan_filter(
            &storage,
            "acc-business",
            &token,
            Some("plus"),
        ));
    }
}
