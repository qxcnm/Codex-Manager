use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use std::time::Instant;

use super::super::attempt_flow::transport::UpstreamRequestContext;
use super::super::attempt_flow::{
    openai_base::{handle_openai_base_attempt, OpenAiAttemptResult},
    postprocess::{process_upstream_post_retry_flow, PostRetryFlowDecision},
    primary_flow::{run_primary_upstream_flow, PrimaryFlowDecision},
};
use super::super::support::deadline;
use super::CandidateUpstreamDecision;

#[allow(clippy::too_many_arguments)]
pub(super) fn execute<F>(
    storage: &Storage,
    method: &reqwest::Method,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    base: &str,
    path: &str,
    primary_url: &str,
    alt_url: Option<&str>,
    request_deadline: Option<Instant>,
    upstream_fallback_base: Option<&str>,
    account: &Account,
    token: &mut Token,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    disable_challenge_stateless_retry: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> CandidateUpstreamDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let client = match super::super::super::upstream_client_for_account(account.id.as_str()) {
        Ok(client) => client,
        Err(err) => {
            return CandidateUpstreamDecision::Terminal {
                status_code: 502,
                message: err,
            };
        }
    };

    if deadline::is_expired(request_deadline) {
        return CandidateUpstreamDecision::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    if super::super::super::is_openai_api_base(base) {
        match handle_openai_base_attempt(
            &client,
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
            has_more_candidates,
            &mut log_gateway_result,
        ) {
            OpenAiAttemptResult::Upstream(resp) => {
                return CandidateUpstreamDecision::RespondUpstream(resp);
            }
            OpenAiAttemptResult::Failover => {
                return CandidateUpstreamDecision::Failover;
            }
            OpenAiAttemptResult::Terminal {
                status_code,
                message,
            } => {
                return CandidateUpstreamDecision::Terminal {
                    status_code,
                    message,
                };
            }
        }
    }

    let (upstream, auth_token) = match run_primary_upstream_flow(
        &client,
        storage,
        method,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        base,
        path,
        primary_url,
        request_deadline,
        upstream_fallback_base,
        account,
        token,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        PrimaryFlowDecision::Continue {
            upstream,
            auth_token,
        } => (upstream, auth_token),
        PrimaryFlowDecision::RespondUpstream(resp) => {
            return CandidateUpstreamDecision::RespondUpstream(resp);
        }
        PrimaryFlowDecision::Failover => {
            return CandidateUpstreamDecision::Failover;
        }
        PrimaryFlowDecision::Terminal {
            status_code,
            message,
        } => {
            return CandidateUpstreamDecision::Terminal {
                status_code,
                message,
            };
        }
    };

    match process_upstream_post_retry_flow(
        &client,
        storage,
        method,
        base,
        path,
        primary_url,
        alt_url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        auth_token.as_str(),
        account,
        token,
        upstream_fallback_base,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        disable_challenge_stateless_retry,
        has_more_candidates,
        upstream,
        &mut log_gateway_result,
    ) {
        PostRetryFlowDecision::Failover => CandidateUpstreamDecision::Failover,
        PostRetryFlowDecision::Terminal {
            status_code,
            message,
        } => CandidateUpstreamDecision::Terminal {
            status_code,
            message,
        },
        PostRetryFlowDecision::RespondUpstream(resp) => {
            CandidateUpstreamDecision::RespondUpstream(resp)
        }
    }
}
