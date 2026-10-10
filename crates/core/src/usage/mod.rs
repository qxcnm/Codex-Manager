use serde::{Deserialize, Serialize};
use serde_json::Value;

const EXTRA_RATE_LIMITS_JSON_KEY: &str = "_codexmanager_extra_rate_limits";
pub const RESET_CREDITS_JSON_KEY: &str = "rate_limit_reset_credits";
const ADDITIONAL_RATE_LIMITS_KEYS: [&str; 2] = ["additional_rate_limits", "additionalRateLimits"];

#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub credits_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCredit {
    pub id: Option<String>,
    pub status: Option<String>,
    pub reset_type: Option<String>,
    pub granted_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub redeemed_at: Option<i64>,
    pub raw_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCreditsSnapshot {
    pub available_count: Option<i64>,
    pub credits: Vec<ResetCredit>,
    pub next_expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResetCreditConsumeResult {
    pub consumed: bool,
    pub usage_refreshed: bool,
    pub snapshot: Option<ResetCreditsSnapshot>,
    pub warning: Option<String>,
}

fn object_value<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| obj.get(*key))
}

fn object_string<'a>(obj: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    object_value(obj, keys).and_then(Value::as_str)
}

fn normalized_identifier(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn is_luna_reserve_identifier(value: &str) -> bool {
    let normalized = normalized_identifier(value);
    matches!(normalized.as_str(), "gptreserve" | "lunareserve")
}

fn is_extra_rate_limit_key(key: &str) -> bool {
    let normalized_key = key.to_ascii_lowercase().replace('-', "_");
    normalized_key.ends_with("_rate_limit")
        || normalized_key.ends_with("ratelimit")
        || (normalized_key.contains("luna") && normalized_key.contains("reserve"))
        || (normalized_key.contains("gpt") && normalized_key.contains("reserve"))
}

fn is_stable_non_reserve_rate_limit_key(key: &str) -> bool {
    normalized_identifier(key) == "codereviewratelimit"
}

fn normalize_rate_limit_entry(source_key: Option<&str>, value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let rate_limit = obj
        .get("rate_limit")
        .or_else(|| obj.get("rateLimit"))
        .and_then(Value::as_object)
        .unwrap_or(obj);
    let has_primary = object_value(rate_limit, &["primary_window", "primaryWindow"]).is_some();
    let has_secondary =
        object_value(rate_limit, &["secondary_window", "secondaryWindow"]).is_some();
    if !has_primary && !has_secondary {
        return None;
    }

    let mut normalized = serde_json::Map::new();
    if let Some(source_key) = source_key.map(str::trim).filter(|value| !value.is_empty()) {
        normalized.insert(
            "source_key".to_string(),
            Value::String(source_key.to_string()),
        );
    }
    for (key, aliases) in [
        ("limit_name", &["limit_name", "limitName"][..]),
        (
            "metered_feature",
            &["metered_feature", "meteredFeature"][..],
        ),
    ] {
        if let Some(field) =
            object_value(obj, aliases).or_else(|| object_value(rate_limit, aliases))
        {
            normalized.insert(key.to_string(), field.clone());
        }
    }
    if let Some(field) = object_value(obj, &["limit_id", "limitId"])
        .or_else(|| object_value(obj, &["metered_feature", "meteredFeature"]))
        .or_else(|| object_value(rate_limit, &["limit_id", "limitId"]))
        .or_else(|| object_value(rate_limit, &["metered_feature", "meteredFeature"]))
    {
        normalized.insert("limit_id".to_string(), field.clone());
    }
    for (key, aliases) in [
        ("allowed", &["allowed"][..]),
        ("limit_reached", &["limit_reached", "limitReached"][..]),
    ] {
        if let Some(field) =
            object_value(obj, aliases).or_else(|| object_value(rate_limit, aliases))
        {
            normalized.insert(key.to_string(), field.clone());
        }
    }
    normalized.insert(
        "primary_window".to_string(),
        object_value(rate_limit, &["primary_window", "primaryWindow"])
            .cloned()
            .unwrap_or(Value::Null),
    );
    normalized.insert(
        "secondary_window".to_string(),
        object_value(rate_limit, &["secondary_window", "secondaryWindow"])
            .cloned()
            .unwrap_or(Value::Null),
    );
    Some(Value::Object(normalized))
}

fn collect_extra_rate_limits(value: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let Some(root) = value.as_object() else {
        return out;
    };

    for (key, nested) in root {
        if key == "rate_limit"
            || key == "rateLimit"
            || key == EXTRA_RATE_LIMITS_JSON_KEY
            || ADDITIONAL_RATE_LIMITS_KEYS.contains(&key.as_str())
            || !is_extra_rate_limit_key(key)
        {
            continue;
        }
        if let Some(item) = normalize_rate_limit_entry(Some(key.as_str()), nested) {
            out.push(item);
        }
    }

    let additional = ADDITIONAL_RATE_LIMITS_KEYS
        .iter()
        .find_map(|key| root.get(*key));
    match additional {
        Some(Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                let source_key = item
                    .as_object()
                    .and_then(|item| {
                        object_string(item, &["limit_id", "limitId"])
                            .or_else(|| object_string(item, &["metered_feature", "meteredFeature"]))
                            .or_else(|| object_string(item, &["limit_name", "limitName"]))
                    })
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("additional_rate_limits[{index}]"));
                if let Some(normalized) =
                    normalize_rate_limit_entry(Some(source_key.as_str()), item)
                {
                    out.push(normalized);
                }
            }
        }
        Some(Value::Object(items)) => {
            for (key, item) in items {
                if let Some(normalized) = normalize_rate_limit_entry(Some(key.as_str()), item) {
                    out.push(normalized);
                }
            }
        }
        _ => {}
    }

    if let Some(Value::Array(items)) = root.get(EXTRA_RATE_LIMITS_JSON_KEY) {
        for (index, item) in items.iter().enumerate() {
            let source_key = item
                .as_object()
                .and_then(|item| object_string(item, &["source_key", "sourceKey"]))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("{EXTRA_RATE_LIMITS_JSON_KEY}[{index}]"));
            if let Some(normalized) = normalize_rate_limit_entry(Some(source_key.as_str()), item) {
                out.push(normalized);
            }
        }
    }

    if let Some(credits) = root.get("credits") {
        out.extend(collect_extra_rate_limits(credits));
    }

    out
}

/// Returns whether a usage response explicitly contains additional rate-limit data.
pub fn usage_payload_declares_extra_rate_limits(value: &Value) -> bool {
    let Some(root) = value.as_object() else {
        return false;
    };
    // `null` is returned by the upstream usage endpoint when the optional
    // reserve section is not present in that response. Keep the previous
    // cached bucket in that case; an array/object (including an empty one)
    // remains an authoritative refresh.
    if root
        .get(EXTRA_RATE_LIMITS_JSON_KEY)
        .is_some_and(|value| !value.is_null())
        || ADDITIONAL_RATE_LIMITS_KEYS
            .iter()
            .any(|key| root.get(*key).is_some_and(|value| !value.is_null()))
    {
        return true;
    }
    if root.iter().any(|(key, value)| {
        key != "rate_limit"
            && key != "rateLimit"
            && !value.is_null()
            && is_extra_rate_limit_key(key)
            && !is_stable_non_reserve_rate_limit_key(key)
    }) {
        return true;
    }
    root.get("credits")
        .is_some_and(usage_payload_declares_extra_rate_limits)
}

fn rate_limit_entry_identifier(entry: &Value) -> impl Iterator<Item = &str> {
    ["source_key", "limit_id", "limit_name", "metered_feature"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
}

fn rate_limit_reset_at_seconds(value: &Value) -> Option<i64> {
    let mut timestamp = value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))?;
    if timestamp > 1_000_000_000_000 {
        timestamp /= 1000;
    }
    Some(timestamp)
}

fn rate_limit_window_is_usable_at(window: Option<&Value>, now: i64) -> bool {
    let Some(window) = window.and_then(Value::as_object) else {
        return false;
    };
    if let Some(reset_at) = object_value(window, &["reset_at", "resetAt"]) {
        if !rate_limit_reset_at_seconds(reset_at).is_some_and(|reset_at| reset_at > now) {
            return false;
        }
    }
    if let Some(remaining) =
        object_value(window, &["remaining_percent", "remainingPercent"]).and_then(Value::as_f64)
    {
        return remaining > 0.0;
    }
    object_value(window, &["used_percent", "usedPercent"])
        .and_then(Value::as_f64)
        .is_some_and(|used| used < 100.0)
}

fn rate_limit_entry_is_usable_at(entry: &Value, now: i64) -> bool {
    let Some(obj) = entry.as_object() else {
        return false;
    };
    if object_value(obj, &["allowed"])
        .and_then(Value::as_bool)
        .is_some_and(|allowed| !allowed)
    {
        return false;
    }
    if object_value(obj, &["limit_reached", "limitReached"])
        .and_then(Value::as_bool)
        .is_some_and(|reached| reached)
    {
        return false;
    }
    rate_limit_window_is_usable_at(obj.get("primary_window"), now)
        || rate_limit_window_is_usable_at(obj.get("secondary_window"), now)
}

/// Returns whether a stored usage payload contains a Luna Reserve window that
/// is usable at the supplied Unix timestamp.
pub fn has_usable_luna_reserve_at(credits_json: Option<&str>, now: i64) -> bool {
    let Some(raw) = credits_json.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    collect_extra_rate_limits(&value).iter().any(|entry| {
        rate_limit_entry_identifier(entry).any(is_luna_reserve_identifier)
            && rate_limit_entry_is_usable_at(entry, now)
    })
}

/// Returns whether a stored usage payload contains a currently usable Luna
/// Reserve window. Missing reset timestamps retain the legacy capacity-only
/// behavior, while an explicit invalid or expired timestamp fails closed.
pub fn has_usable_luna_reserve(credits_json: Option<&str>) -> bool {
    has_usable_luna_reserve_at(credits_json, crate::storage::now_ts())
}

fn extra_rate_limit_identifiers(entry: &Value) -> Vec<String> {
    entry
        .as_object()
        .into_iter()
        .flat_map(|object| {
            ["source_key", "limit_id", "limit_name", "metered_feature"]
                .into_iter()
                .filter_map(|key| object.get(key).and_then(Value::as_str))
                .map(normalized_identifier)
                .filter(|value| !value.is_empty())
        })
        .collect()
}

fn extra_rate_limit_entries_overlap(left: &Value, right: &Value) -> bool {
    let right_identifiers = extra_rate_limit_identifiers(right);
    !right_identifiers.is_empty()
        && extra_rate_limit_identifiers(left)
            .into_iter()
            .any(|identifier| right_identifiers.contains(&identifier))
}

/// Merges the previous extra rate-limit buckets when a newer usage response omits them.
/// An explicitly supplied current bucket list remains authoritative.
pub fn merge_missing_extra_rate_limits(
    current_credits_json: Option<&str>,
    previous_credits_json: Option<&str>,
) -> Option<String> {
    let current_raw = current_credits_json
        .map(str::trim)
        .filter(|raw| !raw.is_empty());
    let previous = previous_credits_json
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    let previous_extra = previous.as_ref().and_then(|value| {
        let mut entries = Vec::new();
        for entry in collect_extra_rate_limits(value) {
            if !entries
                .iter()
                .any(|existing| extra_rate_limit_entries_overlap(existing, &entry))
            {
                entries.push(entry);
            }
        }
        (!entries.is_empty()).then_some(Value::Array(entries))
    });

    let Some(previous_extra) = previous_extra else {
        return current_raw.map(ToString::to_string);
    };

    let mut current = match current_raw {
        Some(raw) => match serde_json::from_str::<Value>(raw).ok() {
            Some(value) => match value {
                Value::Object(obj) => obj,
                value => {
                    let mut wrapped = serde_json::Map::new();
                    wrapped.insert("credits".to_string(), value);
                    wrapped
                }
            },
            None => return Some(raw.to_string()),
        },
        None => serde_json::Map::new(),
    };
    match current.get_mut(EXTRA_RATE_LIMITS_JSON_KEY) {
        Some(Value::Array(current_entries)) => {
            if let Value::Array(previous_entries) = previous_extra {
                for previous_entry in previous_entries {
                    if !current_entries.iter().any(|current_entry| {
                        extra_rate_limit_entries_overlap(current_entry, &previous_entry)
                    }) {
                        current_entries.push(previous_entry);
                    }
                }
            }
        }
        Some(_) => return Some(Value::Object(current).to_string()),
        None => {
            current.insert(EXTRA_RATE_LIMITS_JSON_KEY.to_string(), previous_extra);
        }
    }
    Some(Value::Object(current).to_string())
}

pub const LUNA_RESERVE_MODEL_SLUG: &str = "gpt-reserve";
pub const LUNA_MODEL_SLUG: &str = "gpt-6-luna";
pub const LEGACY_LUNA_MODEL_SLUG: &str = "gpt-5.6-luna";

/// Returns whether a catalog slug is the canonical Luna model or its retained
/// compatibility slug.
pub fn is_luna_catalog_model(model: Option<&str>) -> bool {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return false;
    };
    model.eq_ignore_ascii_case(LUNA_MODEL_SLUG)
        || model.eq_ignore_ascii_case(LEGACY_LUNA_MODEL_SLUG)
}

/// Returns whether a request model is the explicit Luna Reserve alias.
pub fn is_luna_reserve_model(model: Option<&str>) -> bool {
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return false;
    };
    model.eq_ignore_ascii_case(LUNA_RESERVE_MODEL_SLUG)
}

fn serialize_credits_payload(
    credits: Option<&Value>,
    extra_rate_limits: &[Value],
    reset_credits: Option<&Value>,
) -> Option<String> {
    if extra_rate_limits.is_empty() && reset_credits.is_none_or(Value::is_null) {
        return credits.and_then(|value| (!value.is_null()).then(|| value.to_string()));
    }

    let mut payload = match credits {
        Some(Value::Object(obj)) => obj.clone(),
        Some(value) if !value.is_null() => {
            let mut wrapped = serde_json::Map::new();
            wrapped.insert("credits".to_string(), value.clone());
            wrapped
        }
        _ => serde_json::Map::new(),
    };
    payload.insert(
        EXTRA_RATE_LIMITS_JSON_KEY.to_string(),
        Value::Array(extra_rate_limits.to_vec()),
    );
    if let Some(reset_credits) = reset_credits.filter(|value| !value.is_null()) {
        payload.insert(RESET_CREDITS_JSON_KEY.to_string(), reset_credits.clone());
    }
    Some(Value::Object(payload).to_string())
}

/// 函数 `normalize_base_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - base_url: 参数 base_url
///
/// # 返回
/// 返回函数执行结果
pub fn normalize_base_url(base_url: &str) -> String {
    let mut base = base_url.trim_end_matches('/').to_string();
    let is_chatgpt_host =
        base.starts_with("https://chatgpt.com") || base.starts_with("https://chat.openai.com");
    if is_chatgpt_host && !base.contains("/backend-api") {
        base.push_str("/backend-api");
    }
    base
}

/// 函数 `usage_endpoint`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - base_url: 参数 base_url
///
/// # 返回
/// 返回函数执行结果
pub fn usage_endpoint(base_url: &str) -> String {
    let base = normalize_base_url(base_url);
    if base.contains("/backend-api") {
        format!("{base}/wham/usage")
    } else {
        format!("{base}/api/codex/usage")
    }
}

pub fn reset_credits_endpoint(base_url: &str) -> String {
    let base = normalize_base_url(base_url);
    if base.contains("/backend-api") {
        format!("{base}/wham/rate-limit-reset-credits")
    } else {
        format!("{base}/api/codex/rate-limit-reset-credits")
    }
}

pub fn reset_credits_consume_endpoint(base_url: &str) -> String {
    format!("{}/consume", reset_credits_endpoint(base_url))
}

fn parse_reset_credit_timestamp(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => {
            let mut timestamp = number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|raw| i64::try_from(raw).ok()))?;
            if timestamp > 1_000_000_000_000 {
                timestamp /= 1000;
            }
            Some(timestamp)
        }
        Value::String(text) => text.trim().parse::<i64>().ok().or_else(|| {
            chrono::DateTime::parse_from_rfc3339(text.trim())
                .ok()
                .map(|value| value.timestamp())
        }),
        _ => None,
    }
}

fn reset_credit_timestamp(record: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| parse_reset_credit_timestamp(record.get(*key)))
}

fn reset_credit_string(record: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        record.get(*key).and_then(|value| match value {
            Value::String(text) => {
                let trimmed = text.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
    })
}

fn parse_reset_credit(value: &Value) -> Option<ResetCredit> {
    let record = value.as_object()?;
    let raw_status = reset_credit_string(record, &["status", "state"]);
    let expires_at = reset_credit_timestamp(record, &["expires_at", "expire_at", "expiresAt"]);
    let status = raw_status
        .as_deref()
        .map(str::to_ascii_lowercase)
        .or_else(|| {
            expires_at
                .is_some_and(|timestamp| timestamp <= chrono::Utc::now().timestamp())
                .then(|| "expired".to_string())
        });

    Some(ResetCredit {
        id: reset_credit_string(record, &["id", "credit_id", "creditId"]),
        status,
        reset_type: reset_credit_string(record, &["type", "reset_type", "resetType"]),
        granted_at: reset_credit_timestamp(record, &["granted_at", "created_at", "grantedAt"]),
        expires_at,
        redeemed_at: reset_credit_timestamp(
            record,
            &["redeemed_at", "used_at", "consumed_at", "redeemedAt"],
        ),
        raw_status,
    })
}

fn reset_credit_is_available(credit: &ResetCredit, now: i64) -> bool {
    let status = credit
        .status
        .as_deref()
        .or(credit.raw_status.as_deref())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if status != "available" {
        return false;
    }
    credit
        .expires_at
        .map(|timestamp| timestamp > now)
        .unwrap_or(true)
}

pub fn parse_reset_credits_snapshot(value: &Value) -> ResetCreditsSnapshot {
    let credits = value
        .get("credits")
        .or_else(|| value.get("data").and_then(|data| data.get("credits")))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(parse_reset_credit)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let available_count = value
        .get("available_count")
        .or_else(|| value.get("availableCount"))
        .or_else(|| {
            value.get("data").and_then(|data| {
                data.get("available_count")
                    .or_else(|| data.get("availableCount"))
            })
        })
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_u64().and_then(|raw| i64::try_from(raw).ok()))
        })
        .map(|count| count.max(0))
        .or_else(|| {
            Some(
                credits
                    .iter()
                    .filter(|credit| reset_credit_is_available(credit, now))
                    .count() as i64,
            )
        });
    let next_expires_at = credits
        .iter()
        .filter(|credit| reset_credit_is_available(credit, now))
        .filter_map(|credit| credit.expires_at)
        .min();

    ResetCreditsSnapshot {
        available_count,
        credits,
        next_expires_at,
    }
}

/// 函数 `subscription_endpoint`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-17
///
/// # 参数
/// - base_url: 参数 base_url
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
pub fn subscription_endpoint(base_url: &str, account_id: &str) -> String {
    let base = normalize_base_url(base_url);
    let trimmed_account_id = account_id.trim();
    let base_endpoint = format!("{base}/subscriptions");
    format!(
        "{base_endpoint}?account_id={}",
        urlencoding::encode(trimmed_account_id)
    )
}

pub fn accounts_check_endpoint(base_url: &str) -> String {
    let base = normalize_base_url(base_url);
    format!("{base}/accounts/check/v4-2023-04-27")
}

/// 函数 `parse_usage_snapshot`
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
pub fn parse_usage_snapshot(value: &Value) -> UsageSnapshot {
    let used_percent = value
        .pointer("/rate_limit/primary_window/used_percent")
        .and_then(Value::as_f64);
    let window_minutes = value
        .pointer("/rate_limit/primary_window/limit_window_seconds")
        .and_then(Value::as_i64)
        .map(|s| (s + 59) / 60);
    let resets_at = value
        .pointer("/rate_limit/primary_window/reset_at")
        .and_then(Value::as_i64);
    let secondary_used_percent = value
        .pointer("/rate_limit/secondary_window/used_percent")
        .and_then(Value::as_f64);
    let secondary_window_minutes = value
        .pointer("/rate_limit/secondary_window/limit_window_seconds")
        .and_then(Value::as_i64)
        .map(|s| (s + 59) / 60);
    let secondary_resets_at = value
        .pointer("/rate_limit/secondary_window/reset_at")
        .and_then(Value::as_i64);
    let extra_rate_limits = collect_extra_rate_limits(value);
    let credits_json = serialize_credits_payload(
        value.get("credits"),
        &extra_rate_limits,
        value.get(RESET_CREDITS_JSON_KEY),
    );

    UsageSnapshot {
        used_percent,
        window_minutes,
        resets_at,
        secondary_used_percent,
        secondary_window_minutes,
        secondary_resets_at,
        credits_json,
    }
}
