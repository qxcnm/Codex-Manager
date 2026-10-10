use crate::http::gateway_request::GatewayRequest as Request;
use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token, UsageSnapshotRecord};
use std::collections::HashMap;
use std::time::Instant;

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
use super::stream_preflight::{preflight_stream_response, StreamPreflightOutcome};

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

    match crate::account::remote_storage::AccountStorage::new(&storage)
        .latest_usage_snapshots_for_accounts(&account_ids)
    {
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

fn should_retry_same_account_after_failover(retry_count: u8) -> bool {
    retry_count == 0
}

fn account_model_override_for_request(
    storage: &Storage,
    model_for_log: Option<&str>,
) -> Option<String> {
    model_for_log
        .and_then(|model| {
            crate::models_v2::enabled_model(storage, crate::models_v2::policy_catalog_slug(model))
                .ok()
                .flatten()
        })
        .and_then(|model| {
            model
                .routes
                .into_iter()
                .filter(|route| {
                    route.enabled
                        && route.source_kind == "account_pool"
                        && route.source_id == "default"
                })
                .max_by_key(|route| route.priority)
                .map(|route| route.upstream_model)
        })
        .filter(|configured_model| {
            !crate::models_v2::should_preserve_luna_reserve_alias(
                model_for_log,
                Some(configured_model.as_str()),
            )
        })
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
fn finalize_cancelled_candidate(
    request: &Request,
    context: &GatewayUpstreamExecutionContext<'_>,
    account_id: Option<&str>,
    upstream_url: Option<&str>,
    started_at: Instant,
    attempted_account_ids: &[String],
) -> bool {
    if !request.is_cancelled() {
        return false;
    }
    context.log_final_result(
        account_id,
        upstream_url,
        499,
        super::super::super::request_log::RequestLogUsage::default(),
        Some("downstream request cancelled"),
        started_at.elapsed().as_millis(),
        Some(attempted_account_ids),
    );
    true
}

pub(in super::super) async fn execute_candidate_sequence(
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
    let account_model_override = account_model_override_for_request(storage, model_for_log);
    let usage_snapshots = usage_snapshots_for_candidate_plans(storage, &candidates);
    let ordered_account_ids = candidates
        .iter()
        .map(|(account, _)| account.id.clone())
        .collect::<Vec<_>>();
    for (idx, (account, mut token)) in candidates.into_iter().enumerate() {
        if finalize_cancelled_candidate(
            request
                .as_ref()
                .ok_or_else(|| "request already consumed".to_owned())?,
            context,
            attempted_account_ids.last().map(String::as_str),
            last_attempt_url.as_deref(),
            started_at,
            &attempted_account_ids,
        ) {
            return Ok(CandidateExecutionResult::Handled);
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
        let attempt_model_override = account_model_override.as_deref();
        let attempt_allow_openai_fallback = allow_openai_fallback
            && allow_openai_fallback_for_account_with_snapshot(
                &token,
                usage_snapshots.get(account.id.as_str()),
            );
        let attempt_model_for_log = attempt_model_override.or(model_for_log);
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
        if let Some(skip_reason) = context.should_skip_candidate(&account.id, idx) {
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
        prepare_next_account_candidate_client(ordered_account_ids.as_slice(), idx, trace_id);
        attempted_account_ids.push(account.id.clone());

        let request_ref = request
            .as_ref()
            .ok_or_else(|| "request already consumed".to_string())?;
        let request_ctx =
            UpstreamRequestContext::from_request(request_ref, context.protocol_type())
                .with_capture(context.payload_capture());
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

        let mut inflight_guard = Some(super::super::super::acquire_account_inflight(&account.id));
        let mut attempt_trace = CandidateAttemptTrace::default();
        let mut same_account_retry_count = 0u8;
        let mut decision = run_candidate_attempt(CandidateAttemptParams {
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
        })
        .await;
        if finalize_cancelled_candidate(
            request
                .as_ref()
                .ok_or_else(|| "request already consumed".to_owned())?,
            context,
            Some(&account.id),
            attempt_trace.last_attempt_url.as_deref(),
            started_at,
            &attempted_account_ids,
        ) {
            return Ok(CandidateExecutionResult::Handled);
        }

        // A transient upstream error gets one retry on the same account. If that
        // retry also fails, the normal candidate failover path selects the next
        // account instead of repeatedly hammering the current one.
        if matches!(decision, CandidateUpstreamDecision::Failover)
            && should_retry_same_account_after_failover(same_account_retry_count)
        {
            same_account_retry_count += 1;
            log::warn!(
                "event=gateway_same_account_retry trace_id={} account_id={} retry={} ",
                trace_id,
                account.id,
                same_account_retry_count
            );
            attempt_trace = CandidateAttemptTrace::default();
            let request_ref = request
                .as_ref()
                .ok_or_else(|| "request already consumed before same-account retry".to_string())?;
            let retry_request_ctx =
                UpstreamRequestContext::from_request(request_ref, context.protocol_type())
                    .with_capture(context.payload_capture());
            decision = run_candidate_attempt(CandidateAttemptParams {
                storage,
                method,
                request_ctx: retry_request_ctx,
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
            })
            .await;
            if finalize_cancelled_candidate(
                request
                    .as_ref()
                    .ok_or_else(|| "request already consumed".to_owned())?,
                context,
                Some(&account.id),
                attempt_trace.last_attempt_url.as_deref(),
                started_at,
                &attempted_account_ids,
            ) {
                return Ok(CandidateExecutionResult::Handled);
            }
        }

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
                {
                    let retry_body = state.retry_body(
                        path,
                        body,
                        setup,
                        attempt_model_override,
                        attempt_prompt_cache_key,
                    );
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
                    })
                    .await;
                    if finalize_cancelled_candidate(
                        request
                            .as_ref()
                            .ok_or_else(|| "request already consumed".to_owned())?,
                        context,
                        Some(&account.id),
                        attempt_trace.last_attempt_url.as_deref(),
                        started_at,
                        &attempted_account_ids,
                    ) {
                        return Ok(CandidateExecutionResult::Handled);
                    }

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
                let preflight = crate::http::gateway_request::with_response_cancellation(
                    preflight_stream_response(
                        resp,
                        path,
                        upstream_is_stream,
                        context.has_more_candidates(idx),
                    ),
                )
                .await;

                if finalize_cancelled_candidate(
                    request
                        .as_ref()
                        .ok_or_else(|| "request already consumed".to_owned())?,
                    context,
                    Some(&account.id),
                    attempt_trace.last_attempt_url.as_deref(),
                    started_at,
                    &attempted_account_ids,
                ) {
                    return Ok(CandidateExecutionResult::Handled);
                }
                match preflight.map_err(|()| "downstream request cancelled".to_owned())? {
                    StreamPreflightOutcome::Ready(response) => {
                        resp = response;
                    }
                    StreamPreflightOutcome::Failover(message) => {
                        if should_failover_terminal_gateway_error(
                            context,
                            &account.id,
                            context.has_more_candidates(idx),
                            message.as_str(),
                            &mut attempt_trace,
                            &mut last_attempt_url,
                            &mut last_attempt_error,
                        ) {
                            continue;
                        }
                        let request = request.take().ok_or_else(|| {
                            "request already consumed before stream preflight error response"
                                .to_string()
                        })?;
                        return respond_terminal_attempt(
                            request,
                            context,
                            &account.id,
                            attempt_trace.last_attempt_url.as_deref(),
                            429,
                            message,
                            trace_id,
                            started_at,
                            attempt_model_for_log,
                            Some(attempted_account_ids.as_slice()),
                        );
                    }
                    StreamPreflightOutcome::StatusFailover {
                        status_code,
                        message,
                    } => {
                        super::super::super::mark_account_cooldown_for_status(
                            &account.id,
                            status_code,
                        );
                        super::super::super::record_route_quality(&account.id, status_code);
                        attempt_trace.last_attempt_error = Some(message);
                        record_failover_attempt(
                            &mut attempt_trace,
                            &mut last_attempt_url,
                            &mut last_attempt_error,
                        );
                        continue;
                    }
                    StreamPreflightOutcome::RetryUsageNotice(message) => {
                        // Some Codex-compatible upstreams encode quota exhaustion as assistant
                        // output followed by an incomplete terminal sequence. Retry it before
                        // delivery, but do not persistently disable the account based on model
                        // output alone.
                        super::super::super::mark_account_cooldown(
                            &account.id,
                            super::super::super::CooldownReason::Default,
                        );
                        let _ =
                            crate::usage_refresh::enqueue_usage_refresh_for_account(&account.id);
                        attempt_trace.last_attempt_error = Some(message);
                        record_failover_attempt(
                            &mut attempt_trace,
                            &mut last_attempt_url,
                            &mut last_attempt_error,
                        );
                        continue;
                    }
                    StreamPreflightOutcome::TransportFailover(message) => {
                        super::super::super::mark_account_cooldown(
                            &account.id,
                            super::super::super::CooldownReason::Network,
                        );
                        super::super::super::record_route_quality(&account.id, 502);
                        attempt_trace.last_attempt_error = Some(message);
                        record_failover_attempt(
                            &mut attempt_trace,
                            &mut last_attempt_url,
                            &mut last_attempt_error,
                        );
                        continue;
                    }
                }
                let request = request.take().ok_or_else(|| {
                    "request already consumed before upstream response".to_string()
                })?;
                let guard = inflight_guard.take().ok_or_else(|| {
                    "inflight guard already consumed before upstream response".to_string()
                })?;
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
                )
                .await?
                {
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
