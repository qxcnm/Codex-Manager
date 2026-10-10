use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use std::time::Instant;

use super::super::attempt_flow::transport::UpstreamRequestContext;
use super::CandidateUpstreamDecision;

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute<F>(
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
    log_gateway_result: F,
) -> CandidateUpstreamDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    // 中文注释：当前阶段只完成物理拆分，不改变运行语义；
    // 后续接 Claude 原生 executor 时，只替换这一支。
    super::codex::execute(
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
    )
    .await
}
