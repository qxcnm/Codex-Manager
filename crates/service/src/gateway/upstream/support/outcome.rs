use codexmanager_core::storage::{
    now_ts, AdapterCredentialProbeState, Storage, UsageSnapshotRecord,
};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};

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

fn mark_chatgpt_codex_unauthorized_retryable(storage: &Storage, account_id: &str) {
    let now = now_ts();
    let _ = storage.upsert_adapter_credential_probe_state(&AdapterCredentialProbeState {
        pool_id: "codex".to_string(),
        credential_id: account_id.to_string(),
        status: "failed".to_string(),
        error_code: Some("codex_unauthorized".to_string()),
        checked_at: now,
        retry_after: Some(now.saturating_add(30)),
    });
}

fn mark_chatgpt_codex_forbidden_retryable(storage: &Storage, account_id: &str) {
    let now = now_ts();
    let _ = storage.upsert_adapter_credential_probe_state(&AdapterCredentialProbeState {
        pool_id: "codex".to_string(),
        credential_id: account_id.to_string(),
        status: "failed".to_string(),
        error_code: Some("codex_forbidden_recheck".to_string()),
        checked_at: now,
        retry_after: Some(now.saturating_add(60)),
    });
}

fn mark_chatgpt_codex_available(storage: &Storage, account_id: &str, url: &str) {
    if !super::super::config::is_chatgpt_backend_base(url)
        || !url.to_ascii_lowercase().contains("/codex/")
    {
        return;
    }
    crate::usage_refresh::record_codex_responses_verified(storage, account_id);
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
#[cfg_attr(not(test), allow(dead_code))]
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
    decide_upstream_outcome_inner(
        storage,
        account_id,
        status,
        upstream_content_type,
        None,
        url,
        has_more_candidates,
        &mut log_gateway_result,
    )
}

pub(in super::super) fn decide_upstream_outcome_with_headers<F>(
    storage: &Storage,
    account_id: &str,
    status: reqwest::StatusCode,
    upstream_headers: &HeaderMap,
    url: &str,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> UpstreamOutcomeDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let now = now_ts();
    let rate_limit_reset_at =
        super::super::super::rate_limit_reset_at_from_headers(upstream_headers, now);
    decide_upstream_outcome_inner(
        storage,
        account_id,
        status,
        upstream_headers.get(CONTENT_TYPE),
        rate_limit_reset_at,
        url,
        has_more_candidates,
        &mut log_gateway_result,
    )
}

#[allow(clippy::too_many_arguments)]
fn decide_upstream_outcome_inner<F>(
    storage: &Storage,
    account_id: &str,
    status: reqwest::StatusCode,
    upstream_content_type: Option<&HeaderValue>,
    rate_limit_reset_at: Option<i64>,
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
        // A real user request is stronger evidence than any background probe.
        // Publishing the healthy state also atomically cancels an older 404
        // recheck so a stale result cannot overwrite this success afterwards.
        mark_chatgpt_codex_available(storage, account_id, url);
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
        if let Some(until) = rate_limit_reset_at {
            super::super::super::mark_account_rate_limited_until(account_id, until);
        } else {
            super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        }
        let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
        log_gateway_result(Some(url), status.as_u16(), Some("upstream rate-limited"));
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

    // 中文注释：即使保留官方上游的原始响应语义，也要先对账号做短暂冷却，
    // 避免多个平台 Key 在上游故障期间持续把 5xx 突发压到同一账号。
    if is_official_target && status.is_server_error() {
        super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        log_gateway_result(
            Some(url),
            status.as_u16(),
            Some("upstream server error failover"),
        );
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    if is_official_target && status.as_u16() == 401 {
        // Token 刷新/重试在 transport 层已经完成；仍返回 401 时短暂隔离账号，
        // 有其他候选则立即换号，最后一个候选才保留上游 401。
        super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        mark_chatgpt_codex_unauthorized_retryable(storage, account_id);
        let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
        log_gateway_result(Some(url), status.as_u16(), Some("upstream unauthorized"));
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    if is_official_target && status.as_u16() == 403 {
        // A JSON 403 can be account permission, workspace policy, or a temporary
        // security decision. Treat one request as retryable evidence, not as a
        // permanent ban. HTML challenges were already classified above.
        super::super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
        mark_chatgpt_codex_forbidden_retryable(storage, account_id);
        let _ = crate::usage_refresh::enqueue_usage_refresh_for_account(account_id);
        crate::usage_refresh::schedule_codex_fast_reprobe(account_id);
        log_gateway_result(
            Some(url),
            status.as_u16(),
            Some("upstream forbidden; short cooldown and account recheck queued"),
        );
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
    }

    // A single official Codex 404 is request-level evidence only. It can be
    // caused by a transient edge failure or by conversation state that belongs
    // to another account. Keep the credential in the live pool, fail over this
    // request when possible, and verify the account independently in the
    // background. Only repeated clean admission probes may quarantine it.
    if super::super::config::is_chatgpt_backend_base(url)
        && url.to_ascii_lowercase().contains("/codex/")
        && status.as_u16() == 404
    {
        crate::usage_refresh::schedule_codex_fast_reprobe(account_id);
        log_gateway_result(
            Some(url),
            status.as_u16(),
            Some("chatgpt codex request not-found; fast account recheck queued"),
        );
        return from_follow_up_action(follow_up_action(true, has_more_candidates));
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
