use crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE;
use std::time::{Duration, Instant};
use tiny_http::{Request, Response};

use super::super::local_validation::LocalValidationResult;
use super::candidate_flow::{process_candidate_upstream_flow, CandidateUpstreamDecision};
use super::execution_context::GatewayUpstreamExecutionContext;
use super::precheck::{prepare_candidates_for_proxy, CandidatePrecheckResult};

fn respond_terminal(request: Request, status_code: u16, message: String) -> Result<(), String> {
    let response = Response::from_string(message).with_status_code(status_code);
    let _ = request.respond(response);
    Ok(())
}

fn has_prompt_cache_key(body: &[u8]) -> bool {
    if body.is_empty() {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    value
        .get("prompt_cache_key")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .is_some_and(|v| !v.is_empty())
}

pub(in super::super) fn proxy_validated_request(
    request: Request,
    validated: LocalValidationResult,
    debug: bool,
) -> Result<(), String> {
    let LocalValidationResult {
        trace_id,
        storage,
        path,
        body,
        is_stream,
        protocol_type,
        response_adapter,
        request_method,
        key_id,
        model_for_log,
        reasoning_for_log,
        method,
    } = validated;
    let started_at = Instant::now();

    super::super::trace_log::log_request_start(
        trace_id.as_str(),
        key_id.as_str(),
        request_method.as_str(),
        path.as_str(),
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        is_stream,
        protocol_type.as_str(),
    );
    super::super::trace_log::log_request_body_preview(trace_id.as_str(), &body);

    let (request, mut candidates) = match prepare_candidates_for_proxy(
        request,
        &storage,
        trace_id.as_str(),
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
    let anthropic_has_prompt_cache_key =
        protocol_type == PROTOCOL_ANTHROPIC_NATIVE && has_prompt_cache_key(&body);
    if let Some(preferred_account_id) =
        super::super::preferred_route_account(&key_id, &path, model_for_log.as_deref())
    {
        if let Some(pos) = candidates
            .iter()
            .position(|(account, _)| account.id == preferred_account_id)
        {
            if pos > 0 {
                candidates.rotate_left(pos);
            }
        }
    }

    let context = GatewayUpstreamExecutionContext::new(
        &trace_id,
        &storage,
        &key_id,
        &path,
        &request_method,
        protocol_type.as_str(),
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        candidate_count,
        account_max_inflight,
    );
    let allow_openai_fallback = true;
    let disable_challenge_stateless_retry =
        !(protocol_type == PROTOCOL_ANTHROPIC_NATIVE && body.len() <= 2 * 1024);
    let request_gate_lock =
        super::super::request_gate_lock(&key_id, &path, model_for_log.as_deref());
    let request_gate_wait_timeout = super::super::request_gate_wait_timeout();
    super::super::trace_log::log_request_gate_wait(
        trace_id.as_str(),
        key_id.as_str(),
        path.as_str(),
        model_for_log.as_deref(),
    );
    let gate_wait_started_at = Instant::now();
    let _request_gate_guard = match request_gate_lock.try_lock() {
        Ok(guard) => {
            super::super::trace_log::log_request_gate_acquired(
                trace_id.as_str(),
                key_id.as_str(),
                path.as_str(),
                model_for_log.as_deref(),
                0,
            );
            Some(guard)
        }
        Err(std::sync::TryLockError::WouldBlock) => {
            let mut acquired_guard = None;
            let mut lock_poisoned = false;
            while gate_wait_started_at.elapsed() < request_gate_wait_timeout {
                std::thread::sleep(Duration::from_millis(10));
                match request_gate_lock.try_lock() {
                    Ok(guard) => {
                        acquired_guard = Some(guard);
                        break;
                    }
                    Err(std::sync::TryLockError::WouldBlock) => continue,
                    Err(std::sync::TryLockError::Poisoned(_)) => {
                        lock_poisoned = true;
                        super::super::trace_log::log_request_gate_skip(
                            trace_id.as_str(),
                            "lock_poisoned",
                        );
                        break;
                    }
                }
            }
            if let Some(guard) = acquired_guard {
                super::super::trace_log::log_request_gate_acquired(
                    trace_id.as_str(),
                    key_id.as_str(),
                    path.as_str(),
                    model_for_log.as_deref(),
                    gate_wait_started_at.elapsed().as_millis(),
                );
                Some(guard)
            } else {
                if !lock_poisoned {
                    super::super::trace_log::log_request_gate_skip(
                        trace_id.as_str(),
                        "gate_wait_timeout",
                    );
                }
                None
            }
        }
        Err(std::sync::TryLockError::Poisoned(_)) => {
            super::super::trace_log::log_request_gate_skip(trace_id.as_str(), "lock_poisoned");
            None
        }
    };
    let request_shape = super::super::summarize_request_shape(&body);
    let has_sticky_fallback_session = request
        .as_ref()
        .and_then(|value| super::header_profile::derive_sticky_session_id(value))
        .is_some();
    let has_sticky_fallback_conversation = request
        .as_ref()
        .and_then(|value| super::header_profile::derive_sticky_conversation_id(value))
        .is_some();

    for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        // 中文注释：Claude 兼容入口命中 prompt_cache_key 时，优先保持会话粘性；
        // failover 时若强制重置 Session/Conversation，更容易触发 upstream challenge。
        let strip_session_affinity = if anthropic_has_prompt_cache_key {
            false
        } else {
            idx > 0
        };
        context.log_candidate_start(&account.id, idx, strip_session_affinity);
        if let Some(skip_reason) = context.should_skip_candidate(&account.id, idx) {
            context.log_candidate_skip(&account.id, idx, skip_reason);
            continue;
        }

        let request_ref = request
            .as_ref()
            .ok_or_else(|| "request already consumed".to_string())?;
        let incoming_session_id = super::header_profile::find_incoming_header(request_ref, "session_id");
        let incoming_turn_state =
            super::header_profile::find_incoming_header(request_ref, "x-codex-turn-state");
        let incoming_conversation_id =
            super::header_profile::find_incoming_header(request_ref, "conversation_id");
        super::super::trace_log::log_attempt_profile(
            trace_id.as_str(),
            &account.id,
            idx,
            candidate_count,
            strip_session_affinity,
            incoming_session_id.is_some() || has_sticky_fallback_session,
            incoming_turn_state.is_some(),
            incoming_conversation_id.is_some() || has_sticky_fallback_conversation,
            None,
            request_shape.as_deref(),
            body.len(),
            model_for_log.as_deref(),
        );
        // 中文注释：把 inflight 计数覆盖到整个响应生命周期，确保下一批请求能看到真实负载。
        let mut inflight_guard = Some(super::super::acquire_account_inflight(&account.id));
        let mut last_attempt_url: Option<String> = None;
        let mut last_attempt_error: Option<String> = None;

        let decision = process_candidate_upstream_flow(
            &client,
            &storage,
            &method,
            request_ref,
            &body,
            is_stream,
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
            allow_openai_fallback,
            disable_challenge_stateless_retry,
            context.has_more_candidates(idx),
            |upstream_url, status_code, error| {
                last_attempt_url = upstream_url.map(str::to_string);
                last_attempt_error = error.map(str::to_string);
                super::super::record_route_quality(&account.id, status_code);
                context.log_attempt_result(&account.id, upstream_url, status_code, error);
            },
        );

        match decision {
            CandidateUpstreamDecision::Failover => {
                super::super::record_gateway_failover_attempt();
                continue;
            }
            CandidateUpstreamDecision::Terminal {
                status_code,
                message,
            } => {
                let elapsed_ms = started_at.elapsed().as_millis();
                context.log_final_result(
                    Some(&account.id),
                    last_attempt_url.as_deref(),
                    status_code,
                    Some(message.as_str()),
                    elapsed_ms,
                );
                let request = request
                    .take()
                    .expect("request should be available before terminal response");
                return respond_terminal(request, status_code, message);
            }
            CandidateUpstreamDecision::RespondUpstream(resp) => {
                let status_code = resp.status().as_u16();
                let final_error = if status_code >= 400 {
                    last_attempt_error.as_deref()
                } else {
                    None
                };
                let elapsed_ms = started_at.elapsed().as_millis();
                context.log_final_result(
                    Some(&account.id),
                    last_attempt_url.as_deref(),
                    status_code,
                    final_error,
                    elapsed_ms,
                );
                if status_code >= 200 && status_code < 300 {
                    context.remember_success_account(&account.id);
                }
                let request = request
                    .take()
                    .expect("request should be available before terminal response");
                let guard = inflight_guard
                    .take()
                    .expect("inflight guard should be available before terminal response");
                return super::super::respond_with_upstream(request, resp, guard, response_adapter);
            }
        }
    }

    context.log_final_result(
        None,
        Some(base),
        503,
        Some("no available account"),
        started_at.elapsed().as_millis(),
    );
    let request = request
        .take()
        .ok_or_else(|| "request already consumed".to_string())?;
    respond_terminal(request, 503, "no available account".to_string())
}



