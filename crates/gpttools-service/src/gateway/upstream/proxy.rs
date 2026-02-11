use tiny_http::{Request, Response};

use super::super::local_validation::LocalValidationResult;
use super::candidate_flow::{process_candidate_upstream_flow, CandidateUpstreamDecision};
use super::execution_context::GatewayUpstreamExecutionContext;
use super::precheck::{prepare_candidates_for_proxy, CandidatePrecheckResult};

enum CandidateDecisionDispatch {
    Continue,
    Return(Result<(), String>),
}

fn respond_terminal(request: Request, status_code: u16, message: String) -> Result<(), String> {
    let response = Response::from_string(message).with_status_code(status_code);
    let _ = request.respond(response);
    Ok(())
}

fn dispatch_candidate_decision(
    decision: CandidateUpstreamDecision,
    request: &mut Option<Request>,
    inflight_guard: &mut Option<super::super::AccountInFlightGuard>,
) -> CandidateDecisionDispatch {
    match decision {
        CandidateUpstreamDecision::RespondUpstream(resp) => {
            let request = request
                .take()
                .expect("request should be available before terminal response");
            let guard = inflight_guard
                .take()
                .expect("inflight guard should be available before terminal response");
            CandidateDecisionDispatch::Return(super::super::respond_with_upstream(request, resp, guard))
        }
        CandidateUpstreamDecision::Failover => {
            super::super::record_gateway_failover_attempt();
            CandidateDecisionDispatch::Continue
        }
        CandidateUpstreamDecision::Terminal {
            status_code,
            message,
        } => {
            let request = request
                .take()
                .expect("request should be available before terminal response");
            CandidateDecisionDispatch::Return(respond_terminal(request, status_code, message))
        }
    }
}

pub(in super::super) fn proxy_validated_request(
    request: Request,
    validated: LocalValidationResult,
    debug: bool,
) -> Result<(), String> {
    let LocalValidationResult {
        storage,
        path,
        body,
        request_method,
        key_id,
        model_for_log,
        reasoning_for_log,
        method,
    } = validated;

    let (request, candidates) = match prepare_candidates_for_proxy(
        request,
        &storage,
        &key_id,
        &path,
        &request_method,
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
    ) {
        CandidatePrecheckResult::Ready { request, candidates } => (request, candidates),
        CandidatePrecheckResult::Responded => return Ok(()),
    };
    let mut request = Some(request);

    let upstream_base = super::super::resolve_upstream_base_url();
    let base = upstream_base.as_str();
    let upstream_fallback_base = super::super::resolve_upstream_fallback_base_url(base);
    let (url, url_alt) = super::super::request_rewrite::compute_upstream_url(base, &path);

    let client = super::super::upstream_client();
    let upstream_cookie = std::env::var("GPTTOOLS_UPSTREAM_COOKIE").ok();

    let candidate_count = candidates.len();
    let account_max_inflight = super::super::account_max_inflight_limit();
    let context = GatewayUpstreamExecutionContext::new(
        &storage,
        &key_id,
        &path,
        &request_method,
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        candidate_count,
        account_max_inflight,
    );

    for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        let strip_session_affinity = idx > 0;
        if context.should_skip_candidate(&account.id, idx) {
            continue;
        }

        let request_ref = request
            .as_ref()
            .ok_or_else(|| "request already consumed".to_string())?;
        // 中文注释：把 inflight 计数覆盖到整个响应生命周期，确保下一批请求能看到真实负载。
        let mut inflight_guard = Some(super::super::acquire_account_inflight(&account.id));

        let decision = process_candidate_upstream_flow(
            &client,
            &storage,
            &method,
            request_ref,
            &body,
            base,
            &path,
            url.as_str(),
            url_alt.as_deref(),
            upstream_fallback_base.as_deref(),
            &account,
            &mut token,
            upstream_cookie.as_deref(),
            strip_session_affinity,
            debug,
            context.has_more_candidates(idx),
            |upstream_url, status_code, error| context.log_result(upstream_url, status_code, error),
        );

        match dispatch_candidate_decision(decision, &mut request, &mut inflight_guard) {
            CandidateDecisionDispatch::Continue => continue,
            CandidateDecisionDispatch::Return(result) => return result,
        }
    }

    context.log_result(Some(base), 503, Some("no available account"));
    let request = request
        .take()
        .ok_or_else(|| "request already consumed".to_string())?;
    respond_terminal(request, 503, "no available account".to_string())
}



