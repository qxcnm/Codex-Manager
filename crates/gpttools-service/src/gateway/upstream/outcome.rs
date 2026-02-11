use gpttools_core::storage::Storage;
use reqwest::header::HeaderValue;

pub(super) enum UpstreamOutcomeDecision {
    Failover,
    Terminal { status_code: u16, message: String },
    RespondUpstream,
}

pub(super) fn decide_upstream_outcome<F>(
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
    if matches!(status.as_u16(), 429 | 500..=599) {
        // 中文注释：即使当前响应会回给客户端，也要先标记冷却，
        // 否则并发流量会继续命中同一故障账号造成雪崩。
        super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
    }
    if status.is_success() {
        super::super::clear_account_cooldown(account_id);
        log_gateway_result(Some(url), status.as_u16(), None);
        return UpstreamOutcomeDecision::RespondUpstream;
    }

    let is_challenge = super::super::is_upstream_challenge_response(status.as_u16(), upstream_content_type);
    if is_challenge {
        super::super::mark_account_cooldown(account_id, super::super::CooldownReason::Challenge);
        log_gateway_result(Some(url), status.as_u16(), Some("upstream challenge blocked"));
        if has_more_candidates {
            return UpstreamOutcomeDecision::Failover;
        }
        return UpstreamOutcomeDecision::Terminal {
            status_code: 502,
            message: "upstream blocked by Cloudflare/WAF; please refresh account auth or configure GPTTOOLS_UPSTREAM_COOKIE".to_string(),
        };
    }

    let refresh_result = crate::usage_refresh::refresh_usage_for_account(account_id);
    let should_failover = super::super::should_failover_after_refresh(storage, account_id, refresh_result);
    if should_failover {
        super::super::mark_account_cooldown_for_status(account_id, status.as_u16());
    }
    log_gateway_result(Some(url), status.as_u16(), Some("upstream non-success"));
    if should_failover && has_more_candidates {
        return UpstreamOutcomeDecision::Failover;
    }

    UpstreamOutcomeDecision::RespondUpstream
}


