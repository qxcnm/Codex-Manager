use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use std::time::Instant;

use super::attempt_flow::transport::UpstreamRequestContext;

mod claude;
mod codex;
mod gemini;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GatewayUpstreamExecutorKind {
    CodexResponses,
    Claude,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GatewayUpstreamRouteKind {
    AccountRotation,
    AggregateApi,
    HybridAccountFirst,
    HybridAggregateFirst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GatewayUpstreamExecutionPlan {
    pub(super) executor_kind: GatewayUpstreamExecutorKind,
    pub(super) route_kind: GatewayUpstreamRouteKind,
}

pub(super) fn resolve_gateway_upstream_executor_kind(
    protocol_type: &str,
) -> GatewayUpstreamExecutorKind {
    if protocol_type == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE {
        return GatewayUpstreamExecutorKind::Claude;
    }
    if protocol_type == crate::apikey_profile::PROTOCOL_GEMINI_NATIVE {
        return GatewayUpstreamExecutorKind::Gemini;
    }
    GatewayUpstreamExecutorKind::CodexResponses
}

pub(super) fn resolve_gateway_upstream_execution_plan(
    protocol_type: &str,
    rotation_strategy: &str,
) -> GatewayUpstreamExecutionPlan {
    let route_kind = match rotation_strategy {
        crate::apikey_profile::ROTATION_AGGREGATE_API => GatewayUpstreamRouteKind::AggregateApi,
        crate::apikey_profile::ROTATION_HYBRID => GatewayUpstreamRouteKind::HybridAccountFirst,
        crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST => {
            GatewayUpstreamRouteKind::HybridAggregateFirst
        }
        _ => GatewayUpstreamRouteKind::AccountRotation,
    };
    GatewayUpstreamExecutionPlan {
        executor_kind: resolve_gateway_upstream_executor_kind(protocol_type),
        route_kind,
    }
}

pub(super) enum CandidateUpstreamDecision {
    RespondUpstream(super::GatewayUpstreamResponse),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_candidate_upstream_flow<F>(
    executor_kind: GatewayUpstreamExecutorKind,
    storage: &Storage,
    method: &reqwest::Method,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::IncomingHeaderSnapshot,
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
    log_gateway_result: F,
) -> CandidateUpstreamDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    match executor_kind {
        GatewayUpstreamExecutorKind::CodexResponses => codex::execute(
            storage,
            method,
            request_ctx,
            incoming_headers,
            body,
            is_stream,
            base,
            path,
            primary_url,
            alt_url,
            request_deadline,
            upstream_fallback_base,
            account,
            token,
            strip_session_affinity,
            debug,
            allow_openai_fallback,
            disable_challenge_stateless_retry,
            has_more_candidates,
            log_gateway_result,
        ),
        GatewayUpstreamExecutorKind::Claude => claude::execute(
            storage,
            method,
            request_ctx,
            incoming_headers,
            body,
            is_stream,
            base,
            path,
            primary_url,
            alt_url,
            request_deadline,
            upstream_fallback_base,
            account,
            token,
            strip_session_affinity,
            debug,
            allow_openai_fallback,
            disable_challenge_stateless_retry,
            has_more_candidates,
            log_gateway_result,
        ),
        GatewayUpstreamExecutorKind::Gemini => gemini::execute(
            storage,
            method,
            request_ctx,
            incoming_headers,
            body,
            is_stream,
            base,
            path,
            primary_url,
            alt_url,
            request_deadline,
            upstream_fallback_base,
            account,
            token,
            strip_session_affinity,
            debug,
            allow_openai_fallback,
            disable_challenge_stateless_retry,
            has_more_candidates,
            log_gateway_result,
        ),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
