use codexmanager_core::storage::{Storage, UsageSnapshotRecord};
use reqwest::header::HeaderValue;

use super::failover_policy::{
    classify_custom_upstream_status, follow_up_action, CustomUpstreamStatusKind, FollowUpAction,
};

pub(in super::super) enum UpstreamOutcomeDecision {
    Failover,
    RespondUpstream,
}

fn is_compact_target(url: &str) -> bool {
    let normalized = url.trim().to_ascii_lowercase();
    normalized.contains("/responses/compact")
}

fn latest_cached_usage_snapshot<'a>(
    storage: &Storage,
    account_id: &str,
    cache: &'a mut Option<Option<UsageSnapshotRecord>>,
) -> Option<&'a UsageSnapshotRecord> {
    if cache.is_none() {
        *cache = Some(
            storage
                .latest_usage_snapshot_for_account(account_id)
                .ok()
                .flatten(),
        );
    }
    cache.as_ref().and_then(Option::as_ref)
}

/// 函数 `decide_upstream_outcome`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn decide_upstream_outcome<F>(
    storage: &Storage,
    account_id: &str,
    status: reqwest::StatusCode,
    upstream_content_type: Option<&HeaderValue>,
    url: &str,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> UpstreamOutcomeDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    fn from_follow_up_action(action: FollowUpAction) -> UpstreamOutcomeDecision {
        match action {
            FollowUpAction::Failover => UpstreamOutcomeDecision::Failover,
            FollowUpAction::RespondUpstream => UpstreamOutcomeDecision::RespondUpstream,
        }
    }

    let is_official_target = super::super::config::is_official_openai_target(url);
    let mut usage_snapshot_cache: Option<Option<UsageSnapshotRecord>> = None;
    if status.is_success() {
        super::super::super::clear_account_cooldown(account_id);
        log_gateway_result(Some(url), status.as_u16(), None);
        return UpstreamOutcomeDecision::RespondUpstream;
    }

    let is_challenge =
        super::super::super::is_upstream_challenge_response(status.as_u16(), upstream_content_type);
    if is_challenge {
        super::super::super::mark_account_cooldown(
            account_id,
            super::super::super::CooldownReason::Challenge,
        );
        log_gateway_result(
            Some(url),
            status.as_u16(),
            Some("upstream challenge blocked"),
        );
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    if is_official_target && status.as_u16() == 429 {
        super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
        log_gateway_result(Some(url), status.as_u16(), Some("upstream rate-limited"));
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    // OpenAI / chatgpt.com 间歇性返回 404（非 endpoint 缺失，而是流量被集中降级）。
    // 把该账号置为短暂 cooldown，让剩余候选账号优先尝试；最后一个候选时仍回传上游响应。
    if is_official_target && status.as_u16() == 404 {
        super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        log::warn!(
            "event=upstream_not_found account_id={} upstream_url={} status=404 detail=\"likely OpenAI intermittent block\"",
            account_id,
            url
        );
        log_gateway_result(
            Some(url),
            status.as_u16(),
            Some("upstream not-found (likely OpenAI block)"),
        );
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    if is_official_target
        && is_compact_target(url)
        && matches!(status.as_u16(), 500..=599)
        && latest_cached_usage_snapshot(storage, account_id, &mut usage_snapshot_cache)
            .is_some_and(super::super::super::should_failover_from_low_quota_snapshot_value)
    {
        super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
        log_gateway_result(
            Some(url),
            status.as_u16(),
            Some("upstream compact low-quota server error"),
        );
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    if is_official_target && status.as_u16() == 401 {
        log_gateway_result(Some(url), status.as_u16(), Some("upstream unauthorized"));
        return UpstreamOutcomeDecision::RespondUpstream;
    }

    if !is_official_target {
        match classify_custom_upstream_status(status.as_u16()) {
            CustomUpstreamStatusKind::NotFound if has_more_candidates => {
                super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
                log_gateway_result(
                    Some(url),
                    status.as_u16(),
                    Some("upstream not-found failover"),
                );
                return from_follow_up_action(follow_up_action(true, has_more_candidates));
            }
            CustomUpstreamStatusKind::NotFound => {}
            CustomUpstreamStatusKind::RateLimited => {
                // 中文注释：自定义上游继续保留原有容错策略，避免破坏兼容行为。
                super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
                log_gateway_result(Some(url), status.as_u16(), Some("upstream rate-limited"));
                return from_follow_up_action(follow_up_action(true, has_more_candidates));
            }
            CustomUpstreamStatusKind::ServerError => {
                super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
                log_gateway_result(Some(url), status.as_u16(), Some("upstream server error"));
                return from_follow_up_action(follow_up_action(true, has_more_candidates));
            }
            CustomUpstreamStatusKind::Other => {}
        }
    }

    let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
    let should_failover = (!is_official_target || status.as_u16() != 401)
        && super::super::super::should_failover_from_cached_snapshot_value(
            latest_cached_usage_snapshot(storage, account_id, &mut usage_snapshot_cache),
            false,
        );
    if should_failover {
        if is_official_target {
            super::super::super::mark_account_cooldown(
                account_id,
                super::super::super::CooldownReason::Default,
            );
            log_gateway_result(
                Some(url),
                status.as_u16(),
                Some("upstream account exhausted"),
            );
        } else {
            super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
            log_gateway_result(Some(url), status.as_u16(), Some("upstream non-success"));
        }
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    log_gateway_result(Some(url), status.as_u16(), Some("upstream non-success"));
    UpstreamOutcomeDecision::RespondUpstream
}

#[cfg(test)]
#[path = "../tests/support/outcome_tests.rs"]
mod tests;
