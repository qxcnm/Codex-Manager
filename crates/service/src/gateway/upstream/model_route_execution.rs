use super::*;
use crate::gateway::local_validation::LocalValidationResult;
use crate::gateway::upstream::{
    executor::GatewayUpstreamExecutionPlan,
    protocol::aggregate_api::{
        self, AggregateAttemptOutcome, AggregateFailurePolicy, AggregateProxyRequest,
    },
    proxy,
    proxy_pipeline::{
        candidate_executor::{
            execute_candidate_sequence, CandidateExecutionResult, CandidateExecutorParams,
        },
        execution_context::GatewayUpstreamExecutionContext,
        request_gate::acquire_request_gate_async,
        request_setup::prepare_request_setup,
    },
    support::{candidates::prepare_gateway_candidates, deadline},
};
use crate::http::gateway_request::GatewayRequest;
use codexmanager_core::storage::{Account, AggregateApi, ManagedModelV2, Token};

type AccountCandidates = Vec<(Account, Token)>;

pub(in crate::gateway::upstream) async fn proxy_scheduled_request(
    request: GatewayRequest,
    v: LocalValidationResult,
    model: ManagedModelV2,
    plan: GatewayUpstreamExecutionPlan,
    debug: bool,
    started_at: Instant,
    request_deadline: Option<Instant>,
) -> Result<(), String> {
    let guard = request.upstream_replay_guard().clone();
    crate::http::gateway_replay_guard::scope_upstream_replay_guard(
        guard,
        scope_request_deadline(
            request_deadline,
            scope_attempt_budget(Box::pin(execute(
                request,
                v,
                model,
                plan,
                debug,
                started_at,
                request_deadline,
            ))),
        ),
    )
    .await
}

fn respond_error(
    request: GatewayRequest,
    v: &LocalValidationResult,
    started_at: Instant,
    status: u16,
    error: String,
) -> Result<(), String> {
    proxy::respond_model_route_error(
        request,
        &v.storage,
        &v.trace_id,
        &v.key_id,
        &v.original_path,
        &v.path,
        &v.request_method,
        v.response_adapter,
        v.service_tier_for_log.as_deref(),
        v.effective_service_tier_for_log.as_deref(),
        v.service_tier_source_for_log.as_deref(),
        v.gateway_mode_for_log.as_deref(),
        v.client_model_for_log.as_deref(),
        v.model_for_log.as_deref(),
        v.model_source_for_log.as_deref(),
        v.client_reasoning_for_log.as_deref(),
        v.reasoning_for_log.as_deref(),
        v.reasoning_source_for_log.as_deref(),
        started_at,
        status,
        error,
    )
}

async fn account_candidates(
    v: &LocalValidationResult,
    route: &ModelRouteV2,
) -> Result<AccountCandidates, String> {
    crate::account::model_support::scope_route_model_override(route.upstream_model.clone(), async {
        prepare_gateway_candidates(
            &v.storage,
            v.model_for_log.as_deref(),
            v.account_group_filter.as_deref(),
            v.account_plan_filter.as_deref(),
            proxy::low_quota_candidate_mode_for_protocol(&v.protocol_type),
        )
    })
    .await
}

async fn execute(
    mut request: GatewayRequest,
    v: LocalValidationResult,
    model: ManagedModelV2,
    plan: GatewayUpstreamExecutionPlan,
    debug: bool,
    started_at: Instant,
    request_deadline: Option<Instant>,
) -> Result<(), String> {
    let policy = ModelRoutePolicyV2::from_capabilities(&model.capabilities)?;
    if model.routes.len() > 256 {
        return respond_error(
            request,
            &v,
            started_at,
            500,
            "scheduled model supports at most 256 routes".into(),
        );
    }
    let allowed = model
        .routes
        .into_iter()
        .filter(|route| route_allowed(route, plan.route_kind, v.aggregate_api_id.as_deref()))
        .collect::<Vec<_>>();
    let mut aggregates = HashMap::<String, AggregateApi>::new();
    if allowed
        .iter()
        .any(|route| route.source_kind == "aggregate_api")
    {
        match proxy::resolve_aggregate_source_candidates(
            &v.storage,
            &v.protocol_type,
            &v.original_path,
            v.aggregate_api_id.as_deref(),
            v.model_for_log.as_deref(),
        ) {
            Ok(candidates) => {
                aggregates.extend(candidates.into_iter().map(|api| (api.id.clone(), api)))
            }
            Err(error) if error.starts_with("aggregate api not found for provider ") => {}
            Err(error) => return respond_error(request, &v, started_at, 500, error),
        }
    }
    let mut accounts = HashMap::new();
    let mut eligible = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for route in allowed {
        if !seen.insert(branch_key(&route)) {
            continue;
        }
        if route.source_kind == "account_pool" {
            match account_candidates(&v, &route).await {
                Ok(candidates) if !candidates.is_empty() => {
                    accounts.insert(branch_key(&route), candidates);
                }
                Ok(_) => continue,
                Err(error) => return respond_error(request, &v, started_at, 500, error),
            }
        } else if !aggregates.contains_key(&route.source_id) {
            continue;
        }
        eligible.push(route);
    }
    let groups = tiers(policy, eligible);
    let mut routes_left = groups.iter().map(Vec::len).sum::<usize>();
    let mut last_error = "model_unavailable: no eligible model routes".to_owned();
    // Exactly one gate for the inbound request, retained through cross-source
    // fallback and deferred delivery, not a new gate on each branch.
    request.hold_until_complete(
        acquire_request_gate_async(
            &v.trace_id,
            &v.key_id,
            &v.path,
            v.model_for_log.as_deref(),
            request_deadline,
        )
        .await,
    );
    for group in groups {
        if request.is_cancelled() {
            return Ok(());
        }
        if deadline::is_expired(request_deadline) {
            return respond_error(
                request,
                &v,
                started_at,
                504,
                "model route request timeout".into(),
            );
        }
        if budget_exhausted() {
            break;
        }
        for route in order_tier(&model.slug, policy, group) {
            routes_left -= 1;
            if request.is_cancelled() {
                return Ok(());
            }
            if deadline::is_expired(request_deadline) {
                return respond_error(
                    request,
                    &v,
                    started_at,
                    504,
                    "model route request timeout".into(),
                );
            }
            if budget_exhausted() {
                break;
            }
            if request.upstream_replay_guard().is_blocked() {
                return respond_error(
                    request,
                    &v,
                    started_at,
                    502,
                    crate::http::gateway_replay_guard::AMBIGUOUS_UPSTREAM_DELIVERY_MESSAGE.into(),
                );
            }
            log::debug!("event=model_route_attempt trace_id={} source_kind={} source_id={} upstream_model={} priority={} weight={}",
                v.trace_id, route.source_kind, route.source_id, route.upstream_model, route.priority, route.weight);
            if route.source_kind == "aggregate_api" {
                let mut candidate = aggregates[&route.source_id].clone();
                candidate.model_override = (!crate::models_v2::should_preserve_luna_reserve_alias(
                    v.model_for_log.as_deref(),
                    Some(&route.upstream_model),
                ))
                .then(|| route.upstream_model.clone());
                let hybrid = matches!(
                    plan.route_kind,
                    GatewayUpstreamRouteKind::HybridAccountFirst
                        | GatewayUpstreamRouteKind::HybridAggregateFirst
                );
                let (path, body) = if hybrid {
                    (&v.passthrough_path, &v.passthrough_body)
                } else {
                    (&v.path, &v.body)
                };
                match Box::pin(aggregate_api::proxy_aggregate_request(
                    AggregateProxyRequest {
                        request,
                        storage: &v.storage,
                        trace_id: &v.trace_id,
                        key_id: &v.key_id,
                        original_path: &v.original_path,
                        path,
                        request_method: &v.request_method,
                        method: &v.method,
                        body,
                        is_stream: v.is_stream,
                        response_adapter: crate::gateway::ResponseAdapter::Passthrough,
                        gateway_mode_for_log: v.gateway_mode_for_log.as_deref(),
                        route_strategy_for_log: Some("model_priority_weight"),
                        route_source_for_log: Some("model_route"),
                        client_model_for_log: v.client_model_for_log.as_deref(),
                        model_for_log: v.model_for_log.as_deref(),
                        model_source_for_log: v.model_source_for_log.as_deref(),
                        client_reasoning_for_log: v.client_reasoning_for_log.as_deref(),
                        reasoning_for_log: v.reasoning_for_log.as_deref(),
                        reasoning_source_for_log: v.reasoning_source_for_log.as_deref(),
                        service_tier_for_log: v.service_tier_for_log.as_deref(),
                        effective_service_tier_for_log: v.effective_service_tier_for_log.as_deref(),
                        service_tier_source_for_log: v.service_tier_source_for_log.as_deref(),
                        aggregate_api_candidates: vec![candidate],
                        request_deadline,
                        started_at,
                        failure_policy: AggregateFailurePolicy::ReleaseRequest,
                    },
                ))
                .await?
                {
                    AggregateAttemptOutcome::Responded => return Ok(()),
                    AggregateAttemptOutcome::RequestReleased {
                        request: released,
                        error,
                    } => {
                        request = released;
                        last_error = error;
                    }
                }
            } else {
                let candidates = accounts.remove(&branch_key(&route)).unwrap_or_default();
                match crate::account::model_support::scope_route_model_override(
                    route.upstream_model,
                    Box::pin(execute_accounts(
                        request,
                        &v,
                        candidates,
                        debug,
                        started_at,
                        request_deadline,
                        routes_left > 0,
                    )),
                )
                .await?
                {
                    CandidateExecutionResult::Handled => return Ok(()),
                    CandidateExecutionResult::Exhausted {
                        request: released,
                        last_attempt_error,
                        ..
                    } => {
                        request = *released;
                        last_error =
                            last_attempt_error.unwrap_or_else(|| "account pool exhausted".into());
                    }
                }
            }
        }
    }
    if deadline::is_expired(request_deadline) {
        return respond_error(
            request,
            &v,
            started_at,
            504,
            "model route request timeout".into(),
        );
    }
    if budget_exhausted() {
        last_error = "model route attempt budget exhausted".into();
    }
    respond_error(request, &v, started_at, 503, last_error)
}

async fn execute_accounts(
    request: GatewayRequest,
    v: &LocalValidationResult,
    mut candidates: AccountCandidates,
    debug: bool,
    started_at: Instant,
    request_deadline: Option<Instant>,
    following_routes: bool,
) -> Result<CandidateExecutionResult, String> {
    let setup = prepare_request_setup(
        &v.storage,
        &v.path,
        &v.protocol_type,
        v.has_prompt_cache_key,
        &v.incoming_headers,
        &v.body,
        &mut candidates,
        &v.key_id,
        &v.platform_key_hash,
        v.route_conversation_id.as_deref(),
        v.route_conversation_source.unwrap_or(
            crate::gateway::conversation_binding::RouteConversationSource::StickyFallback,
        ),
        v.conversation_binding.as_ref(),
        v.model_for_log.as_deref(),
        &v.trace_id,
    );
    let context = GatewayUpstreamExecutionContext::new(
        &v.trace_id,
        &v.storage,
        &v.key_id,
        &v.original_path,
        &v.path,
        &v.request_method,
        v.response_adapter,
        &v.protocol_type,
        v.client_model_for_log.as_deref(),
        v.model_for_log.as_deref(),
        v.model_source_for_log.as_deref(),
        v.client_reasoning_for_log.as_deref(),
        v.reasoning_for_log.as_deref(),
        v.reasoning_source_for_log.as_deref(),
        v.service_tier_for_log.as_deref(),
        v.effective_service_tier_for_log.as_deref(),
        v.service_tier_source_for_log.as_deref(),
        v.gateway_mode_for_log.as_deref(),
        Some("model_priority_weight"),
        Some("model_route"),
        crate::gateway::request_log::estimate_input_tokens_from_body(&v.body),
        setup.candidate_count,
        setup.account_max_inflight,
    )
    .with_request_type(request.request_type())
    .with_following_model_routes(following_routes);
    Box::pin(execute_candidate_sequence(
        request,
        candidates,
        CandidateExecutorParams {
            storage: &v.storage,
            method: &v.method,
            incoming_headers: &v.incoming_headers,
            body: &v.body,
            path: &v.path,
            request_shape: v.request_shape.as_deref(),
            trace_id: &v.trace_id,
            model_for_log: v.model_for_log.as_deref(),
            response_adapter: v.response_adapter,
            gemini_stream_output_mode: v.gemini_stream_output_mode,
            tool_name_restore_map: &v.tool_name_restore_map,
            context: &context,
            setup: &setup,
            request_deadline,
            started_at,
            client_is_stream: v.is_stream,
            upstream_is_stream: proxy::resolve_upstream_is_stream(v.is_stream, &v.path),
            debug,
            allow_openai_fallback: setup.upstream_fallback_base.is_some(),
            disable_challenge_stateless_retry: !(v.protocol_type
                == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE
                && v.body.len() <= 2 * 1024)
                && !v.path.starts_with("/v1/responses"),
        },
    ))
    .await
}
