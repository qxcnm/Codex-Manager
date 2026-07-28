use codexmanager_core::storage::{now_ts, Event, Storage};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::account_status::{
    mark_account_unavailable_for_deactivation_error,
    mark_account_unavailable_for_refresh_token_error,
    mark_account_unavailable_for_usage_http_error,
};

const DEFAULT_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS: i64 = 60;
const USAGE_REFRESH_FAILURE_EVENT_WINDOW_ENV: &str =
    "CODEXMANAGER_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FailureThrottleKey {
    account_id: String,
    error_class: String,
}

static FAILURE_EVENT_THROTTLE: OnceLock<Mutex<HashMap<FailureThrottleKey, i64>>> = OnceLock::new();

/// 函数 `record_usage_refresh_failure`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn record_usage_refresh_failure(storage: &Storage, account_id: &str, message: &str) {
    let created_at = now_ts();
    let error_class = classify_usage_refresh_error(message);
    let dedupe_window_secs = usage_refresh_failure_event_window_secs();

    if !should_record_failure_event(account_id, &error_class, created_at, dedupe_window_secs) {
        return;
    }

    crate::storage_helpers::insert_event_best_effort(
        storage,
        &Event {
            account_id: Some(account_id.to_string()),
            event_type: "usage_refresh_failed".to_string(),
            message: message.to_string(),
            created_at,
        },
        "usage_refresh.failure",
    );
    if let Some(reason) = status_reason_for_refresh_failure(&error_class) {
        crate::storage_helpers::insert_event_best_effort(
            storage,
            &Event {
                account_id: Some(account_id.to_string()),
                event_type: "account_status_update".to_string(),
                message: format!("status=active reason={reason}"),
                created_at,
            },
            "usage_refresh.status_update",
        );
    }
}

/// 函数 `mark_usage_unreachable_if_needed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn mark_usage_unreachable_if_needed(storage: &Storage, account_id: &str, err: &str) {
    if mark_account_unavailable_for_refresh_token_error(storage, account_id, err) {
        return;
    }
    if mark_account_unavailable_for_deactivation_error(storage, account_id, err) {
        return;
    }
    let _ = mark_account_unavailable_for_usage_http_error(storage, account_id, err);
}

/// 函数 `should_retry_with_refresh`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn should_retry_with_refresh(err: &str) -> bool {
    if crate::usage_http::is_region_blocked_error_message(err) {
        return false;
    }
    err.contains("401") || err.contains("403")
}

/// 函数 `usage_refresh_failure_event_window_secs`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn usage_refresh_failure_event_window_secs() -> i64 {
    std::env::var(USAGE_REFRESH_FAILURE_EVENT_WINDOW_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .map(|secs| secs.max(0))
        .unwrap_or(DEFAULT_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS)
}

/// 函数 `classify_usage_refresh_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
fn classify_usage_refresh_error(message: &str) -> String {
    let normalized = message.trim().to_ascii_lowercase();
    if let Some(status_code) = extract_usage_status_code(&normalized) {
        return format!("usage_status_{status_code}");
    }
    if let Some(reason) = crate::usage_http::refresh_token_auth_error_reason_from_message(message) {
        return format!("token_refresh_{}", reason.as_code());
    }
    if normalized.contains("timeout") {
        return "timeout".to_string();
    }
    if normalized.contains("connection")
        || normalized.contains("connect")
        || normalized.contains("error sending request")
    {
        return "connection".to_string();
    }
    if normalized.contains("dns") {
        return "dns".to_string();
    }
    if normalized.contains("storage unavailable") {
        return "storage_unavailable".to_string();
    }
    if normalized.contains("refresh token") || normalized.contains("token refresh") {
        return "token_refresh".to_string();
    }
    "other".to_string()
}

fn status_reason_for_refresh_failure(error_class: &str) -> Option<&'static str> {
    match error_class {
        "timeout" => Some("usage_refresh_timeout"),
        "connection" => Some("usage_refresh_connection"),
        "dns" => Some("usage_refresh_dns"),
        "other" => Some("usage_refresh_failed"),
        _ => None,
    }
}

/// 函数 `extract_usage_status_code`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
fn extract_usage_status_code(message: &str) -> Option<u16> {
    let rest = if let Some(rest) = message.strip_prefix("usage endpoint status ") {
        Some(rest)
    } else if let Some(rest) = message.strip_prefix("usage endpoint failed: status=") {
        Some(rest)
    } else if let Some(rest) = message.strip_prefix("subscription endpoint status ") {
        Some(rest)
    } else if let Some(rest) = message.strip_prefix("subscription endpoint failed: status=") {
        Some(rest)
    } else {
        None
    }?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u16>().ok()
}

/// 函数 `should_record_failure_event`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
/// - error_class: 参数 error_class
/// - created_at: 参数 created_at
/// - dedupe_window_secs: 参数 dedupe_window_secs
///
/// # 返回
/// 返回函数执行结果
fn should_record_failure_event(
    account_id: &str,
    error_class: &str,
    created_at: i64,
    dedupe_window_secs: i64,
) -> bool {
    let key = FailureThrottleKey {
        account_id: account_id.to_string(),
        error_class: error_class.to_string(),
    };
    let throttle = FAILURE_EVENT_THROTTLE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut state = crate::lock_utils::lock_recover(throttle, "usage_refresh_failure_throttle");
    should_record_failure_event_with_state(&mut state, key, created_at, dedupe_window_secs)
}

/// 函数 `should_record_failure_event_with_state`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - state: 参数 state
/// - key: 参数 key
/// - created_at: 参数 created_at
/// - dedupe_window_secs: 参数 dedupe_window_secs
///
/// # 返回
/// 返回函数执行结果
fn should_record_failure_event_with_state(
    state: &mut HashMap<FailureThrottleKey, i64>,
    key: FailureThrottleKey,
    created_at: i64,
    dedupe_window_secs: i64,
) -> bool {
    if dedupe_window_secs <= 0 {
        state.insert(key, created_at);
        return true;
    }

    if let Some(previous) = state.get(&key).copied() {
        let within_window = if created_at <= previous {
            true
        } else {
            created_at - previous < dedupe_window_secs
        };
        if within_window {
            return false;
        }
    }

    state.insert(key, created_at);
    prune_failure_event_state(state, created_at, dedupe_window_secs);
    true
}

/// 函数 `prune_failure_event_state`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - state: 参数 state
/// - now: 参数 now
/// - dedupe_window_secs: 参数 dedupe_window_secs
///
/// # 返回
/// 无
fn prune_failure_event_state(
    state: &mut HashMap<FailureThrottleKey, i64>,
    now: i64,
    dedupe_window_secs: i64,
) {
    let retain_secs = dedupe_window_secs
        .saturating_mul(10)
        .max(DEFAULT_USAGE_REFRESH_FAILURE_EVENT_WINDOW_SECS);
    state.retain(|_, recorded_at| {
        if *recorded_at > now {
            true
        } else {
            now - *recorded_at <= retain_secs
        }
    });
}

#[cfg(test)]
#[path = "../tests/usage_refresh_errors_tests.rs"]
mod tests;
