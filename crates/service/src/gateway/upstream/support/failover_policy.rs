#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum FollowUpAction {
    Failover,
    RespondUpstream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in super::super) enum CustomUpstreamStatusKind {
    NotFound,
    RateLimited,
    ServerError,
    Other,
}

/// 函数 `follow_up_action`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - should_failover: 参数 should_failover
/// - has_more_candidates: 参数 has_more_candidates
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn follow_up_action(
    should_failover: bool,
    has_more_candidates: bool,
) -> FollowUpAction {
    if should_failover && has_more_candidates {
        FollowUpAction::Failover
    } else {
        FollowUpAction::RespondUpstream
    }
}

/// 函数 `classify_custom_upstream_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - status_code: 参数 status_code
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn classify_custom_upstream_status(
    status_code: u16,
) -> CustomUpstreamStatusKind {
    match status_code {
        404 => CustomUpstreamStatusKind::NotFound,
        429 => CustomUpstreamStatusKind::RateLimited,
        500..=599 => CustomUpstreamStatusKind::ServerError,
        _ => CustomUpstreamStatusKind::Other,
    }
}

/// 函数 `should_failover_after_fallback_non_success`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - status_code: 参数 status_code
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn should_failover_after_fallback_non_success(status_code: u16) -> bool {
    matches!(status_code, 401 | 403 | 404 | 408 | 409 | 429 | 500..=599)
}

#[cfg(test)]
#[path = "../tests/support/failover_policy_tests.rs"]
mod tests;
