use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use std::time::Instant;

use super::super::attempt_flow::transport::UpstreamRequestContext;
use super::super::executor::{
    execute_candidate_upstream_flow, resolve_gateway_upstream_executor_kind,
    CandidateUpstreamDecision,
};
use super::execution_context::GatewayUpstreamExecutionContext;
use super::request_setup::UpstreamRequestSetup;

#[derive(Default)]
pub(in super::super) struct CandidateAttemptTrace {
    pub(in super::super) last_attempt_url: Option<String>,
    pub(in super::super) last_attempt_error: Option<String>,
}

pub(in super::super) struct CandidateAttemptParams<'a> {
    pub(in super::super) storage: &'a Storage,
    pub(in super::super) method: &'a reqwest::Method,
    pub(in super::super) request_ctx: UpstreamRequestContext<'a>,
    pub(in super::super) incoming_headers: &'a super::super::super::IncomingHeaderSnapshot,
    pub(in super::super) body: &'a Bytes,
    pub(in super::super) upstream_is_stream: bool,
    pub(in super::super) path: &'a str,
    pub(in super::super) request_deadline: Option<Instant>,
    pub(in super::super) account: &'a Account,
    pub(in super::super) token: &'a mut Token,
    pub(in super::super) strip_session_affinity: bool,
    pub(in super::super) debug: bool,
    pub(in super::super) allow_openai_fallback: bool,
    pub(in super::super) disable_challenge_stateless_retry: bool,
    pub(in super::super) has_more_candidates: bool,
    pub(in super::super) context: &'a GatewayUpstreamExecutionContext<'a>,
    pub(in super::super) setup: &'a UpstreamRequestSetup,
    pub(in super::super) trace: &'a mut CandidateAttemptTrace,
}

/// 函数 `run_candidate_attempt`
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
pub(in super::super) async fn run_candidate_attempt(
    params: CandidateAttemptParams<'_>,
) -> CandidateUpstreamDecision {
    let CandidateAttemptParams {
        storage,
        method,
        request_ctx,
        incoming_headers,
        body,
        upstream_is_stream,
        path,
        request_deadline,
        account,
        token,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        disable_challenge_stateless_retry,
        has_more_candidates,
        context,
        setup,
        trace,
    } = params;

    let executor_kind = resolve_gateway_upstream_executor_kind(context.protocol_type());

    let attempt = execute_candidate_upstream_flow(
        executor_kind,
        storage,
        method,
        request_ctx,
        incoming_headers,
        body,
        upstream_is_stream,
        setup.upstream_base.as_str(),
        path,
        setup.url.as_str(),
        setup.url_alt.as_deref(),
        request_deadline,
        setup.upstream_fallback_base.as_deref(),
        account,
        token,
        strip_session_affinity,
        debug,
        allow_openai_fallback,
        disable_challenge_stateless_retry,
        has_more_candidates,
        |upstream_url: Option<&str>, status_code, error: Option<&str>| {
            trace.last_attempt_url = upstream_url.map(str::to_string);
            trace.last_attempt_error = error.map(str::to_string);
            super::super::super::record_route_quality(&account.id, status_code);
            context.log_attempt_result(&account.id, upstream_url, status_code, error);
        },
    );
    crate::http::gateway_request::with_response_cancellation(attempt)
        .await
        .unwrap_or_else(|()| CandidateUpstreamDecision::Terminal {
            status_code: 499,
            message: "downstream request cancelled".to_owned(),
        })
}
