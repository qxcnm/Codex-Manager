use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::header::CONTENT_TYPE;
use std::time::Instant;

use super::super::GatewayUpstreamResponse;
use super::fallback_branch::{handle_openai_fallback_branch, FallbackBranchResult};
use super::primary_attempt::{run_primary_upstream_attempt, PrimaryAttemptResult};
use super::transport::UpstreamRequestContext;

pub(in crate::gateway::upstream) enum PrimaryFlowDecision {
    Continue {
        upstream: GatewayUpstreamResponse,
        auth_token: String,
    },
    RespondUpstream(GatewayUpstreamResponse),
    Failover,
    Terminal {
        status_code: u16,
        message: String,
    },
}

/// 函数 `resolve_chatgpt_primary_bearer`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
fn resolve_chatgpt_primary_bearer(token: &Token) -> Option<String> {
    let access = token.access_token.trim();
    if access.is_empty() {
        None
    } else {
        Some(access.to_string())
    }
}

fn resolve_chatgpt_primary_auth(
    storage: &Storage,
    account: &Account,
    token: &Token,
) -> Result<Option<(String, &'static str)>, String> {
    if let Some(assertion) =
        crate::account_agent_identity::build_agent_identity_authorization(storage, &account.id)?
    {
        return Ok(Some((assertion, "agent_identity")));
    }
    Ok(resolve_chatgpt_primary_bearer(token).map(|access| (access, "access_token")))
}

/// 函数 `run_primary_upstream_flow`
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
#[allow(clippy::too_many_arguments)]
pub(in crate::gateway::upstream) fn run_primary_upstream_flow<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    base: &str,
    path: &str,
    primary_url: &str,
    request_deadline: Option<Instant>,
    upstream_fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> PrimaryFlowDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let (auth_token, token_source) = match resolve_chatgpt_primary_auth(storage, account, token) {
        Ok(Some(resolved)) => resolved,
        Ok(None) => {
            let err = "missing chatgpt access token";
            log_gateway_result(Some(primary_url), 401, Some(err));
            return PrimaryFlowDecision::Terminal {
                status_code: 401,
                message: err.to_string(),
            };
        }
        Err(error) => {
            log_gateway_result(Some(primary_url), 401, Some(error.as_str()));
            return PrimaryFlowDecision::Terminal {
                status_code: 401,
                message: format!("agent identity authentication failed: {error}"),
            };
        }
    };
    if debug {
        log::debug!(
            "event=gateway_upstream_token_source path={} account_id={} token_source={} upstream_base={}",
            path,
            account.id,
            token_source,
            base,
        );
    }

    let upstream = match run_primary_upstream_attempt(
        client,
        method,
        primary_url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        auth_token.as_str(),
        account,
        strip_session_affinity,
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        PrimaryAttemptResult::Upstream(resp) => resp,
        PrimaryAttemptResult::Failover => return PrimaryFlowDecision::Failover,
        PrimaryAttemptResult::Terminal {
            status_code,
            message,
        } => {
            return PrimaryFlowDecision::Terminal {
                status_code,
                message,
            };
        }
    };

    let status = upstream.status();
    match handle_openai_fallback_branch(
        client,
        storage,
        method,
        incoming_headers,
        body,
        is_stream,
        base,
        path,
        upstream_fallback_base,
        account,
        token,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        status,
        upstream.headers().get(CONTENT_TYPE),
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        FallbackBranchResult::NotTriggered => PrimaryFlowDecision::Continue {
            upstream,
            auth_token,
        },
        FallbackBranchResult::RespondUpstream(resp) => PrimaryFlowDecision::RespondUpstream(resp),
        FallbackBranchResult::Failover => PrimaryFlowDecision::Failover,
        FallbackBranchResult::Terminal {
            status_code,
            message,
        } => PrimaryFlowDecision::Terminal {
            status_code,
            message,
        },
    }
}

#[cfg(test)]
#[path = "../tests/attempt_flow/primary_flow_tests.rs"]
mod tests;
