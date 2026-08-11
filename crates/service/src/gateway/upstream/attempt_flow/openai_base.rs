use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};

use super::super::support::outcome::{
    decide_upstream_outcome_with_headers, UpstreamOutcomeDecision,
};
use super::super::GatewayUpstreamResponse;

pub(in crate::gateway::upstream) enum OpenAiAttemptResult {
    Upstream(GatewayUpstreamResponse),
    Failover,
    Terminal { status_code: u16, message: String },
}

/// 函数 `handle_openai_base_attempt`
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
pub(in crate::gateway::upstream) fn handle_openai_base_attempt<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    path: &str,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    base: &str,
    account: &Account,
    token: &mut Token,
    strip_session_affinity: bool,
    debug: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> OpenAiAttemptResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    match super::super::super::try_openai_fallback(
        client,
        storage,
        method,
        path,
        incoming_headers,
        body,
        is_stream,
        base,
        account,
        token,
        strip_session_affinity,
        debug,
    ) {
        Ok(Some(resp)) => match decide_upstream_outcome_with_headers(
            storage,
            &account.id,
            resp.status(),
            resp.headers(),
            base,
            has_more_candidates,
            &mut log_gateway_result,
        ) {
            UpstreamOutcomeDecision::Failover => OpenAiAttemptResult::Failover,
            UpstreamOutcomeDecision::RespondUpstream => OpenAiAttemptResult::Upstream(resp.into()),
        },
        Ok(None) => {
            super::super::super::mark_account_cooldown(
                &account.id,
                super::super::super::CooldownReason::Network,
            );
            log_gateway_result(Some(base), 502, Some("openai upstream unavailable"));
            OpenAiAttemptResult::Terminal {
                status_code: 502,
                message: "openai upstream unavailable".to_string(),
            }
        }
        Err(err) => {
            super::super::super::mark_account_cooldown(
                &account.id,
                super::super::super::CooldownReason::Network,
            );
            log_gateway_result(Some(base), 502, Some(err.as_str()));
            OpenAiAttemptResult::Terminal {
                status_code: 502,
                message: format!("openai upstream error: {err}"),
            }
        }
    }
}
