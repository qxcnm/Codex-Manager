use super::{
    classify_custom_upstream_status, follow_up_action, should_failover_after_fallback_non_success,
    CustomUpstreamStatusKind, FollowUpAction,
};

/// 函数 `follow_up_action_only_failovers_when_candidates_remain`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn follow_up_action_only_failovers_when_candidates_remain() {
    assert_eq!(follow_up_action(true, true), FollowUpAction::Failover);
    assert_eq!(
        follow_up_action(true, false),
        FollowUpAction::RespondUpstream
    );
    assert_eq!(
        follow_up_action(false, true),
        FollowUpAction::RespondUpstream
    );
}

/// 函数 `classify_custom_upstream_status_groups_known_retryable_statuses`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn classify_custom_upstream_status_groups_known_retryable_statuses() {
    assert_eq!(
        classify_custom_upstream_status(404),
        CustomUpstreamStatusKind::NotFound
    );
    assert_eq!(
        classify_custom_upstream_status(429),
        CustomUpstreamStatusKind::RateLimited
    );
    assert_eq!(
        classify_custom_upstream_status(502),
        CustomUpstreamStatusKind::ServerError
    );
    assert_eq!(
        classify_custom_upstream_status(400),
        CustomUpstreamStatusKind::Other
    );
}

/// 函数 `fallback_non_success_helper_matches_existing_retryable_status_set`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn fallback_non_success_helper_matches_existing_retryable_status_set() {
    assert!(should_failover_after_fallback_non_success(401));
    assert!(should_failover_after_fallback_non_success(403));
    assert!(should_failover_after_fallback_non_success(404));
    assert!(should_failover_after_fallback_non_success(408));
    assert!(should_failover_after_fallback_non_success(409));
    assert!(should_failover_after_fallback_non_success(429));
    assert!(should_failover_after_fallback_non_success(500));
}
