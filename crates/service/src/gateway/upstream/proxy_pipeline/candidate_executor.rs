use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token, UsageSnapshotRecord};
use std::collections::HashMap;
use std::time::Instant;
use tiny_http::Request;

use super::super::attempt_flow::transport::UpstreamRequestContext;
use super::super::executor::CandidateUpstreamDecision;
use super::super::support::candidates::allow_openai_fallback_for_account_with_snapshot;
use super::super::support::deadline;
use super::candidate_attempt::{
    run_candidate_attempt, CandidateAttemptParams, CandidateAttemptTrace,
};
use super::candidate_state::CandidateExecutionState;
use super::execution_context::GatewayUpstreamExecutionContext;
use super::request_setup::UpstreamRequestSetup;
use super::response_finalize::{
    finalize_terminal_candidate, finalize_upstream_response, respond_total_timeout,
    FinalizeUpstreamResponseOutcome,
};

/// 函数 `extract_prompt_cache_key_for_trace`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn extract_prompt_cache_key_for_trace(body: &[u8]) -> Option<String> {
    if body.is_empty() || body.len() > 64 * 1024 {
        return None;
    }
    let value = serde_json::from_slice::<serde_json::Value>(body).ok()?;
    value
        .get("prompt_cache_key")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn should_forward_thread_anchor_as_prompt_cache_key(protocol_type: &str) -> bool {
    protocol_type != crate::apikey_profile::PROTOCOL_GEMINI_NATIVE
}

fn usage_snapshots_for_candidate_plans(
    storage: &Storage,
    candidates: &[(Account, Token)],
) -> HashMap<String, UsageSnapshotRecord> {
    let account_ids = candidates
        .iter()
        .filter(|(_, token)| crate::account_plan::resolve_token_account_plan(token).is_none())
        .map(|(account, _)| account.id.clone())
        .collect::<Vec<_>>();
    if account_ids.is_empty() {
        return HashMap::new();
    }

    match storage.latest_usage_snapshots_for_accounts(&account_ids) {
        Ok(snapshots) => snapshots
            .into_iter()
            .map(|snapshot| (snapshot.account_id.clone(), snapshot))
            .collect(),
        Err(err) => {
            log::warn!("gateway candidate usage snapshot prefetch failed: {err}");
            HashMap::new()
        }
    }
}

pub(in super::super) enum CandidateExecutionResult {
    Handled,
    Exhausted {
        request: Box<Request>,
        attempted_account_ids: Vec<String>,
        skipped_cooldown: usize,
        skipped_inflight: usize,
        last_attempt_url: Option<String>,
        last_attempt_error: Option<String>,
    },
}

pub(in super::super) struct CandidateExecutorParams<'a> {
    pub(in super::super) storage: &'a Storage,
    pub(in super::super) method: &'a reqwest::Method,
    pub(in super::super) incoming_headers: &'a super::super::super::IncomingHeaderSnapshot,
    pub(in super::super) body: &'a Bytes,
    pub(in super::super) path: &'a str,
    pub(in super::super) request_shape: Option<&'a str>,
    pub(in super::super) trace_id: &'a str,
    pub(in super::super) model_for_log: Option<&'a str>,
    pub(in super::super) response_adapter: super::super::super::ResponseAdapter,
    pub(in super::super) gemini_stream_output_mode:
        Option<super::super::super::GeminiStreamOutputMode>,
    pub(in super::super) tool_name_restore_map: &'a super::super::super::ToolNameRestoreMap,
    pub(in super::super) context: &'a GatewayUpstreamExecutionContext<'a>,
    pub(in super::super) setup: &'a UpstreamRequestSetup,
    pub(in super::super) request_deadline: Option<Instant>,
    pub(in super::super) started_at: Instant,
    pub(in super::super) client_is_stream: bool,
    pub(in super::super) upstream_is_stream: bool,
    pub(in super::super) debug: bool,
    pub(in super::super) allow_openai_fallback: bool,
    pub(in super::super) disable_challenge_stateless_retry: bool,
}

fn record_failover_attempt(
    attempt_trace: &mut CandidateAttemptTrace,
    last_attempt_url: &mut Option<String>,
    last_attempt_error: &mut Option<String>,
) {
    super::super::super::record_gateway_failover_attempt();
    *last_attempt_url = attempt_trace.last_attempt_url.take();
    *last_attempt_error = attempt_trace.last_attempt_error.take();
}

fn prepare_next_account_candidate_client(
    ordered_account_ids: &[String],
    candidate_idx: usize,
    trace_id: &str,
) {
    let Some(next_account_id) = ordered_account_ids.get(candidate_idx + 1) else {
        return;
    };
    if let Err(err) =
        super::super::super::prepare_upstream_client_for_account(next_account_id.as_str())
    {
        log::warn!(
            "event=gateway_account_candidate_client_prepare_failed trace_id={} account_id={} err={}",
            trace_id,
            next_account_id,
            err
        );
    }
}

fn is_challenge_failover_error(error: Option<&str>) -> bool {
    error
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| {
            let normalized = value.to_ascii_lowercase();
            normalized.contains("challenge")
                || normalized.contains("cloudflare")
                || normalized.contains("cf_ray")
        })
}

fn should_force_strip_after_anthropic_challenge(
    context: &GatewayUpstreamExecutionContext<'_>,
    attempt_trace: &CandidateAttemptTrace,
) -> bool {
    context.protocol_type() == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE
        && is_challenge_failover_error(attempt_trace.last_attempt_error.as_deref())
}

fn should_failover_terminal_gateway_error(
    context: &GatewayUpstreamExecutionContext<'_>,
    account_id: &str,
    has_more_candidates: bool,
    message: &str,
    attempt_trace: &mut CandidateAttemptTrace,
    last_attempt_url: &mut Option<String>,
    last_attempt_error: &mut Option<String>,
) -> bool {
    let gateway_error_follow_up =
        context.apply_gateway_error_follow_up(account_id, message, has_more_candidates);
    if !gateway_error_follow_up.should_failover {
        return false;
    }
    super::super::super::record_gateway_failover_attempt();
    *last_attempt_url = attempt_trace.last_attempt_url.take();
    *last_attempt_error = Some(message.to_string());
    true
}

#[allow(clippy::too_many_arguments)]
fn respond_terminal_attempt(
    request: Request,
    context: &GatewayUpstreamExecutionContext<'_>,
    account_id: &str,
    last_attempt_url: Option<&str>,
    status_code: u16,
    message: String,
    trace_id: &str,
    started_at: Instant,
    model_for_log: Option<&str>,
    attempted_account_ids: Option<&[String]>,
) -> Result<CandidateExecutionResult, String> {
    finalize_terminal_candidate(
        request,
        context,
        account_id,
        last_attempt_url,
        status_code,
        message,
        trace_id,
        started_at,
        model_for_log,
        attempted_account_ids,
    )?;
    Ok(CandidateExecutionResult::Handled)
}

/// 函数 `execute_candidate_sequence`
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
pub(in super::super) fn execute_candidate_sequence(
    request: Request,
    candidates: Vec<(Account, Token)>,
    params: CandidateExecutorParams<'_>,
) -> Result<CandidateExecutionResult, String> {
    let CandidateExecutorParams {
        storage,
        method,
        incoming_headers,
        body,
        path,
        request_shape,
        trace_id,
        model_for_log,
        response_adapter,
        gemini_stream_output_mode,
        tool_name_restore_map,
        context,
        setup,
        request_deadline,
        started_at,
        client_is_stream,
        upstream_is_stream,
        debug,
        allow_openai_fallback,
        disable_challenge_stateless_retry,
    } = params;
    let mut request = Some(request);
    let mut state = CandidateExecutionState::default();
    let mut attempted_account_ids = Vec::new();
    let mut skipped_cooldown = 0usize;
    let mut skipped_inflight = 0usize;
    let mut last_attempt_url = None;
    let mut last_attempt_error = None;
    let mut force_strip_session_affinity_after_challenge = false;
    let max_attempts = super::super::super::account_batch_rotation_config()
        .max_attempts_per_request
        .max(1);
    let mut attempts_used = 0usize;
    let usage_snapshots = usage_snapshots_for_candidate_plans(storage, &candidates);
    let ordered_account_ids = candidates
        .iter()
        .map(|(account, _)| account.id.clone())
        .collect::<Vec<_>>();
    let queue_candidate_ids = candidates
        .iter()
        .enumerate()
        .filter(|(idx, (account, _))| {
            !super::super::super::is_account_in_cooldown(&account.id)
                || *idx + 1 == setup.candidate_count
        })
        .map(|(_, (account, _))| account.id.clone())
        .collect::<Vec<_>>();
    // 有并发上限时先原子预留一个账号槽位。全部账号繁忙则留在网关内等待，
    // 而不是立即向 Codex 客户端返回 no available account/503。
    let mut queued_inflight = if setup.account_max_inflight > 0 {
        super::super::super::wait_acquire_candidate_account_inflight(
            queue_candidate_ids.as_slice(),
            setup.account_max_inflight,
            deadline::remaining(request_deadline),
        )
    } else {
        None
    };
    'candidates: for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        if attempts_used >= max_attempts {
            last_attempt_error = Some(format!(
                "gateway request attempt budget exhausted ({attempts_used}/{max_attempts})"
            ));
            log::warn!(
                "event=gateway_attempt_budget_exhausted trace_id={} attempts_used={} max_attempts={}",
                trace_id,
                attempts_used,
                max_attempts
            );
            break 'candidates;
        }
        if deadline::is_expired(request_deadline) {
            let request = request
                .take()
                .ok_or_else(|| "request already consumed before timeout response".to_string())?;
            respond_total_timeout(
                request,
                context,
                trace_id,
                started_at,
                model_for_log,
                Some(attempted_account_ids.as_slice()),
            )?;
            return Ok(CandidateExecutionResult::Handled);
        }

        if super::super::super::proxy_stream_circuit::is_account_proxy_stream_circuit_open(
            &account.id,
        ) {
            if queued_inflight
                .as_ref()
                .is_some_and(|(account_id, _)| account_id == &account.id)
            {
                queued_inflight.take();
            }
            skipped_cooldown += 1;
            log::warn!(
                "event=gateway_candidate_skipped_proxy_stream_circuit trace_id={} account_id={}",
                trace_id,
                account.id
            );
            continue;
        }

        let strip_session_affinity = force_strip_session_affinity_after_challenge
            || state.strip_session_affinity(&account, idx, setup.anthropic_has_thread_anchor);
        let attempt_thread = super::super::super::conversation_binding::resolve_attempt_thread(
            setup.conversation_routing.as_ref(),
            &account,
        );
        let attempt_headers = attempt_thread
            .as_ref()
            .map(|thread| {
                incoming_headers.with_thread_affinity_override(
                    Some(thread.thread_anchor.as_str()),
                    thread.reset_session_affinity,
                )
            })
            .unwrap_or_else(|| incoming_headers.clone());
        let attempt_model_override: Option<&str> = None;
        let attempt_allow_openai_fallback = allow_openai_fallback
            && allow_openai_fallback_for_account_with_snapshot(
                &token,
                usage_snapshots.get(account.id.as_str()),
            );
        let attempt_model_for_log = model_for_log;
        let attempt_prompt_cache_key =
            if should_forward_thread_anchor_as_prompt_cache_key(context.protocol_type()) {
                attempt_thread
                    .as_ref()
                    .map(|thread| thread.thread_anchor.as_str())
            } else {
                None
            };
        let body_for_attempt = state.body_for_attempt(
            path,
            body,
            strip_session_affinity,
            setup,
            attempt_model_override,
            attempt_prompt_cache_key,
        );
        context.log_candidate_start(&account.id, idx, strip_session_affinity);
        let has_queued_slot = queued_inflight
            .as_ref()
            .is_some_and(|(account_id, _)| account_id == &account.id);
        if let Some(skip_reason) = context.should_skip_candidate(&account.id, idx) {
            if has_queued_slot
                && matches!(
                    skip_reason,
                    super::super::support::candidates::CandidateSkipReason::Inflight
                )
            {
                // 预留动作本身会让计数达到上限，这不是需要跳过的外部占用。
            } else {
                if has_queued_slot {
                    queued_inflight.take();
                }
                context.log_candidate_skip(&account.id, idx, skip_reason);
                match skip_reason {
                    super::super::support::candidates::CandidateSkipReason::Cooldown => {
                        skipped_cooldown += 1;
                    }
                    super::super::support::candidates::CandidateSkipReason::Inflight => {
                        skipped_inflight += 1;
                    }
                }
                continue;
            }
        }
        let inflight_guard = if has_queued_slot {
            queued_inflight.take().map(|(_, guard)| guard)
        } else {
            super::super::super::try_acquire_account_inflight(
                &account.id,
                setup.account_max_inflight,
            )
        };
        let Some(inflight_guard) = inflight_guard else {
            // 中文注释：预检只用于快速分流；这里的原子占位才是严格上限，
            // 防止多个平台 Key 同时命中同一 Codex 账号造成异常突发。
            context.log_candidate_skip(
                &account.id,
                idx,
                super::super::support::candidates::CandidateSkipReason::Inflight,
            );
            super::super::super::record_gateway_candidate_skip(
                super::super::super::GatewayCandidateSkipReason::Inflight,
            );
            skipped_inflight += 1;
            continue;
        };
        prepare_next_account_candidate_client(ordered_account_ids.as_slice(), idx, trace_id);
        attempted_account_ids.push(account.id.clone());

        let request_ref = request
            .as_ref()
            .ok_or_else(|| "request already consumed".to_string())?;
        let request_ctx =
            UpstreamRequestContext::from_request(request_ref, context.protocol_type());
        let incoming_session_id = attempt_headers.session_id();
        let incoming_turn_state = attempt_headers.turn_state();
        let incoming_conversation_id = attempt_headers.conversation_id();
        let prompt_cache_key_for_trace =
            extract_prompt_cache_key_for_trace(body_for_attempt.as_ref());
        super::super::super::trace_log::log_attempt_profile(
            super::super::super::trace_log::AttemptProfileLog {
                trace_id,
                account_id: &account.id,
                candidate_index: idx,
                total: setup.candidate_count,
                strip_session_affinity,
                has_incoming_session: incoming_session_id.is_some()
                    || setup.has_sticky_fallback_session,
                has_incoming_turn_state: incoming_turn_state.is_some(),
                has_incoming_conversation: incoming_conversation_id.is_some()
                    || setup.has_sticky_fallback_conversation,
                prompt_cache_key: prompt_cache_key_for_trace.as_deref(),
                request_shape,
                body_len: body_for_attempt.len(),
                body_model: attempt_model_for_log,
            },
        );

        let mut attempt_trace = CandidateAttemptTrace::default();
        attempts_used = attempts_used.saturating_add(1);
        let decision = run_candidate_attempt(CandidateAttemptParams {
            storage,
            method,
            request_ctx,
            incoming_headers: &attempt_headers,
            body: &body_for_attempt,
            upstream_is_stream,
            path,
            request_deadline,
            account: &account,
            token: &mut token,
            strip_session_affinity,
            debug,
            allow_openai_fallback: attempt_allow_openai_fallback,
            disable_challenge_stateless_retry,
            has_more_candidates: context.has_more_candidates(idx),
            context,
            setup,
            trace: &mut attempt_trace,
        });

        match decision {
            CandidateUpstreamDecision::Failover => {
                if should_force_strip_after_anthropic_challenge(context, &attempt_trace) {
                    force_strip_session_affinity_after_challenge = true;
                    log::warn!(
                        "event=gateway_anthropic_challenge_strip_affinity trace_id={} account_id={} next_attempt_strip_session_affinity=true",
                        trace_id,
                        account.id
                    );
                }
                record_failover_attempt(
                    &mut attempt_trace,
                    &mut last_attempt_url,
                    &mut last_attempt_error,
                );
                continue;
            }
            CandidateUpstreamDecision::Terminal {
                status_code,
                message,
            } => {
                if should_failover_terminal_gateway_error(
                    context,
                    &account.id,
                    context.has_more_candidates(idx),
                    &message,
                    &mut attempt_trace,
                    &mut last_attempt_url,
                    &mut last_attempt_error,
                ) {
                    if context.protocol_type() == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE
                        && is_challenge_failover_error(Some(message.as_str()))
                    {
                        force_strip_session_affinity_after_challenge = true;
                        log::warn!(
                            "event=gateway_anthropic_challenge_strip_affinity trace_id={} account_id={} next_attempt_strip_session_affinity=true",
                            trace_id,
                            account.id
                        );
                    }
                    continue;
                }
                let request = request.take().ok_or_else(|| {
                    "request already consumed before terminal response".to_string()
                })?;
                return respond_terminal_attempt(
                    request,
                    context,
                    &account.id,
                    attempt_trace.last_attempt_url.as_deref(),
                    status_code,
                    message,
                    trace_id,
                    started_at,
                    attempt_model_for_log,
                    Some(attempted_account_ids.as_slice()),
                );
            }
            CandidateUpstreamDecision::RespondUpstream(mut resp) => {
                if resp.status().as_u16() == 400
                    && !strip_session_affinity
                    && (incoming_turn_state.is_some() || setup.has_body_encrypted_content)
                    && attempts_used < max_attempts
                {
                    let retry_body = state.retry_body(
                        path,
                        body,
                        setup,
                        attempt_model_override,
                        attempt_prompt_cache_key,
                    );
                    attempts_used = attempts_used.saturating_add(1);
                    let retry_decision = run_candidate_attempt(CandidateAttemptParams {
                        storage,
                        method,
                        request_ctx,
                        incoming_headers: &attempt_headers,
                        body: &retry_body,
                        upstream_is_stream,
                        path,
                        request_deadline,
                        account: &account,
                        token: &mut token,
                        strip_session_affinity: true,
                        debug,
                        allow_openai_fallback: attempt_allow_openai_fallback,
                        disable_challenge_stateless_retry,
                        has_more_candidates: context.has_more_candidates(idx),
                        context,
                        setup,
                        trace: &mut attempt_trace,
                    });

                    match retry_decision {
                        CandidateUpstreamDecision::RespondUpstream(retry_resp) => {
                            resp = retry_resp;
                        }
                        CandidateUpstreamDecision::Failover => {
                            record_failover_attempt(
                                &mut attempt_trace,
                                &mut last_attempt_url,
                                &mut last_attempt_error,
                            );
                            continue;
                        }
                        CandidateUpstreamDecision::Terminal {
                            status_code,
                            message,
                        } => {
                            let request = request.take().ok_or_else(|| {
                                "request already consumed before retry terminal response"
                                    .to_string()
                            })?;
                            return respond_terminal_attempt(
                                request,
                                context,
                                &account.id,
                                attempt_trace.last_attempt_url.as_deref(),
                                status_code,
                                message,
                                trace_id,
                                started_at,
                                attempt_model_for_log,
                                Some(attempted_account_ids.as_slice()),
                            );
                        }
                    }
                }

                if client_is_stream
                    && upstream_is_stream
                    && path.starts_with("/v1/responses")
                    && matches!(
                        response_adapter,
                        super::super::super::ResponseAdapter::Passthrough
                    )
                {
                    loop {
                        match resp.preflight_openai_responses_stream() {
                            super::super::OpenAiResponsesPreflight::Ready(preflighted) => {
                                super::super::super::proxy_stream_circuit::record_account_proxy_stream_success(
                                    &account.id,
                                );
                                resp = preflighted;
                                break;
                            }
                            super::super::OpenAiResponsesPreflight::RetryableTerminal {
                                response,
                                failure,
                            } => {
                                let (cooldown_reason, quality_status) = match failure.class {
                                    super::super::OpenAiResponsesTerminalFailureClass::UpstreamTransient => (
                                        super::super::super::CooldownReason::Upstream5xx,
                                        502,
                                    ),
                                    super::super::OpenAiResponsesTerminalFailureClass::RateLimited => (
                                        super::super::super::CooldownReason::RateLimited,
                                        429,
                                    ),
                                    super::super::OpenAiResponsesTerminalFailureClass::AccountUnavailable => (
                                        super::super::super::CooldownReason::Default,
                                        401,
                                    ),
                                };
                                log::warn!(
                                    "event=gateway_upstream_stream_preflight_terminal trace_id={} account_id={} class={:?} err={}",
                                    trace_id,
                                    account.id,
                                    failure.class,
                                    failure.message
                                );
                                super::super::super::mark_account_cooldown(
                                    &account.id,
                                    cooldown_reason,
                                );
                                super::super::super::record_route_quality(
                                    &account.id,
                                    quality_status,
                                );
                                attempt_trace.last_attempt_error = Some(failure.message.clone());
                                if context.has_more_candidates(idx) {
                                    drop(inflight_guard);
                                    record_failover_attempt(
                                        &mut attempt_trace,
                                        &mut last_attempt_url,
                                        &mut last_attempt_error,
                                    );
                                    continue 'candidates;
                                }

                                // Preserve the provider's original terminal frame for the
                                // last candidate instead of masking it with a synthetic 502.
                                resp = response;
                                break;
                            }
                            super::super::OpenAiResponsesPreflight::TransportFailure(error) => {
                                log::warn!(
                                "event=gateway_upstream_stream_preflight_failed trace_id={} account_id={} err={}",
                                trace_id,
                                account.id,
                                error
                            );
                                super::super::super::mark_account_cooldown(
                                    &account.id,
                                    super::super::super::CooldownReason::Network,
                                );
                                super::super::super::proxy_stream_circuit::record_account_proxy_stream_failure(
                                    &account.id,
                                );
                                super::super::super::record_route_quality(&account.id, 502);
                                attempt_trace.last_attempt_error = Some(error.clone());
                                if context.has_more_candidates(idx) {
                                    drop(inflight_guard);
                                    record_failover_attempt(
                                        &mut attempt_trace,
                                        &mut last_attempt_url,
                                        &mut last_attempt_error,
                                    );
                                    continue 'candidates;
                                }
                                let request = request.take().ok_or_else(|| {
                                "request already consumed before stream preflight failure response"
                                    .to_string()
                            })?;
                                return respond_terminal_attempt(
                                    request,
                                    context,
                                    &account.id,
                                    attempt_trace.last_attempt_url.as_deref(),
                                    502,
                                    error,
                                    trace_id,
                                    started_at,
                                    attempt_model_for_log,
                                    Some(attempted_account_ids.as_slice()),
                                );
                            }
                        }
                    }
                }
                let request = request.take().ok_or_else(|| {
                    "request already consumed before upstream response".to_string()
                })?;
                let guard = inflight_guard;
                let response_status = resp.status().as_u16();
                match finalize_upstream_response(
                    request,
                    resp,
                    guard,
                    context,
                    &account.id,
                    attempt_trace.last_attempt_url.as_deref(),
                    attempt_trace.last_attempt_error.as_deref(),
                    response_adapter,
                    gemini_stream_output_mode,
                    tool_name_restore_map,
                    client_is_stream,
                    path,
                    trace_id,
                    started_at,
                    attempt_model_for_log,
                    Some(attempted_account_ids.as_slice()),
                    context.has_more_candidates(idx),
                )? {
                    FinalizeUpstreamResponseOutcome::Handled => {
                        if let Err(err) = super::super::super::conversation_binding::record_conversation_binding_terminal_response(
                            storage,
                            setup.conversation_routing.as_ref(),
                            &account,
                            attempt_model_for_log,
                            response_status,
                        ) {
                            log::warn!(
                                "event=gateway_conversation_binding_update_failed trace_id={} account_id={} err={}",
                                trace_id,
                                account.id,
                                err
                            );
                        }
                        return Ok(CandidateExecutionResult::Handled);
                    }
                    FinalizeUpstreamResponseOutcome::Failover => {
                        record_failover_attempt(
                            &mut attempt_trace,
                            &mut last_attempt_url,
                            &mut last_attempt_error,
                        );
                        continue;
                    }
                }
            }
        }
    }

    Ok(CandidateExecutionResult::Exhausted {
        request: Box::new(
            request
                .ok_or_else(|| "request already consumed after candidate exhaustion".to_string())?,
        ),
        attempted_account_ids,
        skipped_cooldown,
        skipped_inflight,
        last_attempt_url,
        last_attempt_error,
    })
}

#[cfg(test)]
#[path = "candidate_executor_tests.rs"]
mod tests;
