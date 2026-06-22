use serde_json::Value;

const EXTRA_RATE_LIMITS_JSON_KEY: &str = "_codexmanager_extra_rate_limits";

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

fn normalize_rate_limit_entry(source_key: Option<&str>, value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let rate_limit = obj
        .get("rate_limit")
        .and_then(Value::as_object)
        .unwrap_or(obj);
    let has_primary = rate_limit.get("primary_window").is_some();
    let has_secondary = rate_limit.get("secondary_window").is_some();
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
    for key in ["limit_name", "metered_feature"] {
        if let Some(field) = obj.get(key) {
            normalized.insert(key.to_string(), field.clone());
        }
    }
    if let Some(field) = obj.get("limit_id").or_else(|| obj.get("metered_feature")) {
        normalized.insert("limit_id".to_string(), field.clone());
    }
    for key in ["allowed", "limit_reached"] {
        if let Some(field) = obj.get(key).or_else(|| rate_limit.get(key)) {
            normalized.insert(key.to_string(), field.clone());
        }
    }
    normalized.insert(
        "primary_window".to_string(),
        rate_limit
            .get("primary_window")
            .cloned()
            .unwrap_or(Value::Null),
    );
    normalized.insert(
        "secondary_window".to_string(),
        rate_limit
            .get("secondary_window")
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
        if key == "rate_limit" || !key.ends_with("_rate_limit") {
            continue;
        }
        if let Some(item) = normalize_rate_limit_entry(Some(key.as_str()), nested) {
            out.push(item);
        }
    }

    match root.get("additional_rate_limits") {
        Some(Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                let source_key = item
                    .get("limit_id")
                    .and_then(Value::as_str)
                    .or_else(|| item.get("metered_feature").and_then(Value::as_str))
                    .or_else(|| item.get("limit_name").and_then(Value::as_str))
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

    out
}

fn serialize_credits_payload(
    credits: Option<&Value>,
    extra_rate_limits: &[Value],
) -> Option<String> {
    if extra_rate_limits.is_empty() {
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

pub fn rate_limit_reset_credits_endpoint(base_url: &str) -> String {
    let base = normalize_base_url(base_url);
    format!("{base}/wham/rate-limit-reset-credits")
}

pub fn rate_limit_reset_credits_consume_endpoint(base_url: &str) -> String {
    let base = normalize_base_url(base_url);
    format!("{base}/wham/rate-limit-reset-credits/consume")
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
    let credits_json = serialize_credits_payload(value.get("credits"), &extra_rate_limits);

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
