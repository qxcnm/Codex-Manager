use gpttools_core::storage::{Account, Storage, Token};
use reqwest::header::CONTENT_TYPE;
use tiny_http::Request;

use super::fallback_branch::{handle_openai_fallback_branch, FallbackBranchResult};
use super::primary_attempt::{run_primary_upstream_attempt, PrimaryAttemptResult};

pub(super) enum PrimaryFlowDecision {
    Continue {
        upstream: reqwest::blocking::Response,
        auth_token: String,
    },
    RespondUpstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_primary_upstream_flow<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    request: &Request,
    body: &[u8],
    base: &str,
    path: &str,
    primary_url: &str,
    upstream_fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> PrimaryFlowDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let auth_token = token.access_token.clone();
    if debug {
        eprintln!(
            "gateway upstream: base={}, token_source=access_token",
            base
        );
    }

    let upstream = match run_primary_upstream_attempt(
        client,
        method,
        primary_url,
        request,
        body,
        upstream_cookie,
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
        request,
        body,
        base,
        path,
        upstream_fallback_base,
        account,
        token,
        upstream_cookie,
        strip_session_affinity,
        debug,
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


