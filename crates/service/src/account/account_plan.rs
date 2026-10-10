use codexmanager_core::{
    auth::parse_id_token_claims,
    storage::{now_ts, AccountSubscription, AccountTokenPlan, Token, UsageSnapshotRecord},
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
#[cfg_attr(not(test), allow(dead_code))]
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
#[cfg_attr(not(test), allow(dead_code))]
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
    token: Option<&AccountTokenPlan>,
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
    token: Option<&AccountTokenPlan>,
    snapshot: Option<&UsageSnapshotRecord>,
    subscription: Option<&AccountSubscription>,
) -> Option<ResolvedAccountPlan> {
    resolve_effective_account_plan_at(token, snapshot, subscription, now_ts())
}

fn resolve_effective_account_plan_at(
    token: Option<&AccountTokenPlan>,
    snapshot: Option<&UsageSnapshotRecord>,
    subscription: Option<&AccountSubscription>,
    now: i64,
) -> Option<ResolvedAccountPlan> {
    if subscription.is_some_and(|value| {
        !value.has_subscription || value.expires_at.is_some_and(|expires_at| expires_at <= now)
    }) {
        return Some(ResolvedAccountPlan {
            normalized: "free".to_string(),
            raw: None,
        });
    }

    if let Some(plan) = subscription
        .and_then(|value| value.account_plan_type.as_deref())
        .and_then(normalize_plan_type)
    {
        return Some(plan);
    }

    resolve_account_plan(token, snapshot)
}

pub(crate) fn token_plan_from_token(token: &Token) -> AccountTokenPlan {
    AccountTokenPlan {
        account_id: token.account_id.clone(),
        id_token: token.id_token.clone(),
        access_token: token.access_token.clone(),
    }
}

pub(crate) fn resolve_token_account_plan(token: &Token) -> Option<ResolvedAccountPlan> {
    resolve_account_plan(Some(&token_plan_from_token(token)), None)
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
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_free_or_single_window_account_with_snapshot(
    token: &Token,
    snapshot: Option<&UsageSnapshotRecord>,
) -> bool {
    if is_free_plan_type(extract_plan_type_from_id_token(&token.id_token).as_deref())
        || is_free_plan_type(extract_plan_type_from_id_token(&token.access_token).as_deref())
    {
        return true;
    }

    snapshot
        .map(|snapshot| {
            is_free_plan_from_credits_json(snapshot.credits_json.as_deref())
                || is_single_window_long_usage_snapshot(snapshot)
        })
        .unwrap_or(false)
}

pub(crate) fn account_matches_plan_filter_with_snapshot(
    token: &Token,
    snapshot: Option<&UsageSnapshotRecord>,
    plan_filter: Option<&str>,
) -> bool {
    let Some(filter) = plan_filter.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    if filter.eq_ignore_ascii_case("all") {
        return true;
    }

    let normalized_filter = filter.to_ascii_lowercase();
    let token_plan = token_plan_from_token(token);
    match resolve_account_plan(Some(&token_plan), snapshot) {
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
#[path = "account_plan_tests.rs"]
mod tests;
