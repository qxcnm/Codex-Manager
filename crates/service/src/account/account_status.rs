use codexmanager_core::storage::{now_ts, Event, Storage};

pub(crate) const REFRESH_TOKEN_REGION_BLOCKED_REASON: &str = "refresh_token_region_blocked";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountAvailabilitySignal {
    RefreshToken(crate::usage_http::RefreshTokenAuthErrorReason),
    RefreshTokenRegionBlocked,
    Deactivation(&'static str),
    UsageHttp(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayErrorKind {
    Deactivation,
    UsageLimit,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GatewayErrorFollowUp {
    pub kind: GatewayErrorKind,
    pub should_failover: bool,
    pub should_mark_account_unavailable: bool,
    pub should_mark_default_cooldown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccountStatusContext {
    pub status: String,
    pub reason: Option<String>,
    pub updated_at: Option<i64>,
}

/// 函数 `latest_status_reason`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn latest_status_reason(storage: &Storage, account_id: &str) -> Option<String> {
    storage
        .latest_account_status_reasons(&[account_id.to_string()])
        .ok()
        .and_then(|mut reasons| reasons.remove(account_id))
}

pub(crate) fn load_account_status_context(
    storage: &Storage,
    account_id: &str,
) -> AccountStatusContext {
    let account = storage.find_account_by_id(account_id).ok().flatten();
    AccountStatusContext {
        status: account
            .as_ref()
            .map(|account| account.status.clone())
            .unwrap_or_default(),
        reason: latest_status_reason(storage, account_id),
        updated_at: account.map(|account| account.updated_at),
    }
}

/// 函数 `set_account_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn set_account_status(storage: &Storage, account_id: &str, status: &str, reason: &str) {
    set_account_status_with_context(storage, account_id, status, reason, None);
}

pub(crate) fn set_account_status_with_context(
    storage: &Storage,
    account_id: &str,
    status: &str,
    reason: &str,
    context: Option<&AccountStatusContext>,
) {
    let (account_exists, changed) = storage
        .update_account_status_if_changed_with_existence(account_id, status)
        .unwrap_or((false, false));
    if changed {
        crate::gateway::invalidate_candidate_cache();
    }
    let should_insert_event = if !account_exists || changed {
        account_exists
    } else {
        let latest_reason = context
            .filter(|context| context.status.trim().eq_ignore_ascii_case(status))
            .and_then(|context| context.reason.as_deref())
            .map(str::to_string)
            .or_else(|| latest_status_reason(storage, account_id));
        latest_reason.as_deref() != Some(reason)
    };
    if should_insert_event {
        let _ = storage.insert_event(&Event {
            account_id: Some(account_id.to_string()),
            event_type: "account_status_update".to_string(),
            message: format!("status={status} reason={reason}"),
            created_at: now_ts(),
        });
    }
}

/// 函数 `should_preserve_manual_account_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn should_preserve_manual_account_status(storage: &Storage, account_id: &str) -> bool {
    storage
        .find_account_status_by_id(account_id)
        .ok()
        .flatten()
        .map(|status| {
            status.trim().eq_ignore_ascii_case("disabled")
                || status.trim().eq_ignore_ascii_case("inactive")
        })
        .unwrap_or(false)
}

/// 函数 `classify_account_availability_signal`
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
pub(crate) fn classify_account_availability_signal(err: &str) -> Option<AccountAvailabilitySignal> {
    if crate::usage_http::is_refresh_token_region_blocked_error_message(err) {
        return Some(AccountAvailabilitySignal::RefreshTokenRegionBlocked);
    }
    if let Some(reason) = crate::usage_http::refresh_token_auth_error_reason_from_message(err) {
        return Some(AccountAvailabilitySignal::RefreshToken(reason));
    }
    if let Some(reason) = deactivation_reason_from_message(err) {
        return Some(AccountAvailabilitySignal::Deactivation(reason));
    }
    if let Some(status_code) = extract_usage_http_status_code(err) {
        return Some(AccountAvailabilitySignal::UsageHttp(status_code));
    }
    None
}

/// 函数 `extract_usage_http_status_code`
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
fn extract_usage_http_status_code(message: &str) -> Option<u16> {
    let trimmed = message.trim();
    let rest = if let Some(rest) = trimmed.strip_prefix("usage endpoint status ") {
        Some(rest)
    } else if let Some(rest) = trimmed.strip_prefix("usage endpoint failed: status=") {
        Some(rest)
    } else if let Some(rest) = trimmed.strip_prefix("subscription endpoint status ") {
        Some(rest)
    } else if let Some(rest) = trimmed.strip_prefix("subscription endpoint failed: status=") {
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

/// 函数 `deactivation_reason_from_message`
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
pub(crate) fn deactivation_reason_from_message(message: &str) -> Option<&'static str> {
    let normalized = message.trim().to_ascii_lowercase();
    if normalized.contains("workspace_deactivated")
        || normalized.contains("deactivated_workspace")
        || normalized.contains("workspace deactivated")
        || normalized.contains("workspace-deactivated")
        || normalized.contains("deactivated workspace")
    {
        return Some("workspace_deactivated");
    }
    if normalized.contains("account_deactivated")
        || normalized.contains("account deactivated")
        || normalized.contains("deactivated")
    {
        return Some("account_deactivated");
    }
    None
}

pub(crate) fn usage_limit_reason_from_message(message: &str) -> Option<&'static str> {
    let normalized = message.trim().to_ascii_lowercase();
    let has_usage_limit_code = normalized
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .any(|token| {
            matches!(
                token,
                "usage_limit_reached" | "usage_limit_exceeded" | "usage_limit_exhausted"
            )
        });
    if normalized.contains("you've hit your usage limit")
        || normalized.contains("you have hit your usage limit")
        || normalized.contains("usage limit has been reached")
        || has_usage_limit_code
        || normalized.contains("insufficient_quota")
        || normalized.contains("quota exceeded")
        || normalized.contains("usage exhausted")
        || (normalized.contains("usage limit") && normalized.contains("try again"))
    {
        return Some("usage_limit_exhausted");
    }
    None
}

pub(crate) fn analyze_gateway_error(err: &str, has_more_candidates: bool) -> GatewayErrorFollowUp {
    let kind = if deactivation_reason_from_message(err).is_some() {
        GatewayErrorKind::Deactivation
    } else if usage_limit_reason_from_message(err).is_some() {
        GatewayErrorKind::UsageLimit
    } else {
        GatewayErrorKind::Other
    };
    let is_actionable = !matches!(kind, GatewayErrorKind::Other);
    let should_failover = has_more_candidates && is_actionable;
    GatewayErrorFollowUp {
        kind,
        should_failover,
        should_mark_account_unavailable: is_actionable,
        should_mark_default_cooldown: matches!(kind, GatewayErrorKind::UsageLimit)
            && should_failover,
    }
}

/// 函数 `should_failover_for_deactivation_error`
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
pub(crate) fn mark_account_unavailable_for_gateway_error(
    storage: &Storage,
    account_id: &str,
    err: &str,
) -> bool {
    if let Some(reason) = deactivation_reason_from_message(err) {
        return set_account_banned_with_reason(storage, account_id, reason);
    }
    if usage_limit_reason_from_message(err).is_some() {
        return mark_account_unavailable_for_confirmed_usage_exhausted(storage, account_id);
    }
    false
}

fn mark_account_unavailable_for_confirmed_usage_exhausted(
    storage: &Storage,
    account_id: &str,
) -> bool {
    set_account_limited_with_reason(storage, account_id, "usage_limit_exhausted")
}

/// 函数 `set_account_unavailable_with_reason`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
/// - reason: 参数 reason
///
/// # 返回
/// 返回函数执行结果
fn set_account_unavailable_with_reason(storage: &Storage, account_id: &str, reason: &str) -> bool {
    if should_preserve_manual_account_status(storage, account_id) {
        return false;
    }
    set_account_status(storage, account_id, "unavailable", reason);
    true
}

fn set_account_limited_with_reason(storage: &Storage, account_id: &str, reason: &str) -> bool {
    if should_preserve_manual_account_status(storage, account_id) {
        return false;
    }
    set_account_status(storage, account_id, "limited", reason);
    true
}

/// 函数 `set_account_banned_with_reason`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
/// - reason: 参数 reason
///
/// # 返回
/// 返回函数执行结果
fn set_account_banned_with_reason(storage: &Storage, account_id: &str, reason: &str) -> bool {
    if should_preserve_manual_account_status(storage, account_id) {
        return false;
    }
    set_account_status(storage, account_id, "banned", reason);
    true
}

/// 函数 `mark_account_unavailable_for_usage_http_error`
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
pub(crate) fn mark_account_unavailable_for_usage_http_error(
    storage: &Storage,
    account_id: &str,
    err: &str,
) -> bool {
    let Some(AccountAvailabilitySignal::UsageHttp(status_code)) =
        classify_account_availability_signal(err)
    else {
        return false;
    };
    match status_code {
        401 | 403 => {
            let status_reason = format!("usage_http_{status_code}");
            set_account_unavailable_with_reason(storage, account_id, &status_reason)
        }
        _ => false,
    }
}

/// 函数 `mark_account_unavailable_for_deactivation_error`
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
pub(crate) fn mark_account_unavailable_for_deactivation_error(
    storage: &Storage,
    account_id: &str,
    err: &str,
) -> bool {
    let Some(AccountAvailabilitySignal::Deactivation(reason)) =
        classify_account_availability_signal(err)
    else {
        return false;
    };
    set_account_banned_with_reason(storage, account_id, reason)
}

/// 函数 `mark_account_unavailable_for_auth_error`
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
pub(crate) fn mark_account_unavailable_for_auth_error(
    storage: &Storage,
    account_id: &str,
    err: &str,
) -> bool {
    let Some(signal) = classify_account_availability_signal(err) else {
        return false;
    };
    match signal {
        AccountAvailabilitySignal::RefreshTokenRegionBlocked => {
            set_account_unavailable_with_reason(
                storage,
                account_id,
                REFRESH_TOKEN_REGION_BLOCKED_REASON,
            )
        }
        AccountAvailabilitySignal::RefreshToken(reason) => {
            let status_reason = format!("refresh_token_invalid:{}", reason.as_code());
            set_account_unavailable_with_reason(storage, account_id, &status_reason)
        }
        AccountAvailabilitySignal::Deactivation(reason) => {
            set_account_banned_with_reason(storage, account_id, reason)
        }
        AccountAvailabilitySignal::UsageHttp(_) => false,
    }
}

/// 函数 `mark_account_unavailable_for_refresh_token_error`
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
pub(crate) fn mark_account_unavailable_for_refresh_token_error(
    storage: &Storage,
    account_id: &str,
    err: &str,
) -> bool {
    match classify_account_availability_signal(err) {
        Some(AccountAvailabilitySignal::RefreshTokenRegionBlocked) => {
            set_account_unavailable_with_reason(
                storage,
                account_id,
                REFRESH_TOKEN_REGION_BLOCKED_REASON,
            )
        }
        Some(AccountAvailabilitySignal::RefreshToken(reason)) => {
            let status_reason = format!("refresh_token_invalid:{}", reason.as_code());
            set_account_unavailable_with_reason(storage, account_id, &status_reason)
        }
        _ => false,
    }
}

/// 函数 `mark_account_unavailable_for_test_auth_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
/// - status_code: 参数 status_code
///
/// # 返回
/// 返回是否已变更账号状态
///
/// 测试账号在真实上游请求中遇到 401/403 时，将账号标记为不可用。
pub(crate) fn mark_account_unavailable_for_test_auth_status(
    storage: &Storage,
    account_id: &str,
    status_code: u16,
    context: &AccountStatusContext,
) -> bool {
    set_account_status_after_test_if_context_matches(
        storage,
        account_id,
        "unavailable",
        &format!("test_http_{status_code}"),
        context,
    )
}

/// 函数 `mark_account_limited_for_test_rate_limit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回是否已变更账号状态
///
/// 测试账号在真实上游请求中遇到 429 时，将账号标记为限流。
pub(crate) fn mark_account_limited_for_test_rate_limit(
    storage: &Storage,
    account_id: &str,
    context: &AccountStatusContext,
) -> bool {
    set_account_status_after_test_if_context_matches(
        storage,
        account_id,
        "limited",
        "test_rate_limited",
        context,
    )
}

fn set_account_status_after_test_if_context_matches(
    storage: &Storage,
    account_id: &str,
    status: &str,
    reason: &str,
    context: &AccountStatusContext,
) -> bool {
    let normalized = context.status.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "disabled" | "inactive" | "banned") {
        return false;
    }
    if load_account_status_context(storage, account_id) != *context {
        return false;
    }
    let Some(expected_updated_at) = context.updated_at else {
        return false;
    };
    let changed = storage
        .update_account_status_if_context_matches(
            account_id,
            &context.status,
            expected_updated_at,
            status,
        )
        .unwrap_or(false);
    if !changed {
        return false;
    }
    crate::gateway::invalidate_candidate_cache();
    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "account_status_update".to_string(),
        message: format!("status={status} reason={reason}"),
        created_at: now_ts(),
    });
    true
}

/// 函数 `restore_account_active_after_test`
///
/// 作者: gaohongshun
///
/// 时间: 2026-08-26
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回是否已变更账号状态
///
/// 测试成功后，仅当账号从测试开始至今仍处于同一个自动失败状态
///（unavailable/limited）时恢复为 active；banned 与手动停用状态始终保留。
pub(crate) fn restore_account_active_after_test(
    storage: &Storage,
    account_id: &str,
    context: &AccountStatusContext,
) -> bool {
    let normalized = context.status.trim().to_ascii_lowercase();
    if !matches!(normalized.as_str(), "unavailable" | "limited") {
        return false;
    }
    set_account_status_after_test_if_context_matches(
        storage, account_id, "active", "test_ok", context,
    )
}

#[cfg(test)]
#[path = "account_status_tests.rs"]
mod tests;
