use crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE;
use crate::gateway::request_log::RequestLogUsage;
use std::time::Instant;
use tiny_http::Request;

use super::super::local_validation::LocalValidationResult;
use super::executor::{
    resolve_gateway_upstream_execution_plan, GatewayUpstreamExecutorKind, GatewayUpstreamRouteKind,
};
use super::proxy_pipeline::candidate_executor::{
    execute_candidate_sequence, CandidateExecutionResult, CandidateExecutorParams,
};
use super::proxy_pipeline::execution_context::GatewayUpstreamExecutionContext;
use super::proxy_pipeline::request_gate::acquire_request_gate;
use super::proxy_pipeline::request_setup::prepare_request_setup;
use super::proxy_pipeline::response_finalize::respond_terminal;
use super::support::precheck::{prepare_candidates_for_proxy, CandidatePrecheckResult};

/// 函数 `exhausted_gateway_error_for_log`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - attempted_account_ids: 参数 attempted_account_ids
/// - skipped_cooldown: 参数 skipped_cooldown
/// - skipped_inflight: 参数 skipped_inflight
/// - last_attempt_error: 参数 last_attempt_error
///
/// # 返回
/// 返回函数执行结果
fn exhausted_gateway_error_for_log(
    attempted_account_ids: &[String],
    skipped_cooldown: usize,
    skipped_inflight: usize,
    last_attempt_error: Option<&str>,
) -> String {
    let kind = if !attempted_account_ids.is_empty() {
        "no_available_account_exhausted"
    } else if skipped_cooldown > 0 && skipped_inflight > 0 {
        "no_available_account_skipped"
    } else if skipped_cooldown > 0 {
        "no_available_account_cooldown"
    } else if skipped_inflight > 0 {
        "no_available_account_inflight"
    } else {
        "no_available_account"
    };
    let mut parts = vec![
        crate::gateway::bilingual_error("无可用账号", "no available account"),
        format!("kind={kind}"),
    ];
    if !attempted_account_ids.is_empty() {
        parts.push(format!("attempted={}", attempted_account_ids.join(",")));
    }
    if skipped_cooldown > 0 || skipped_inflight > 0 {
        parts.push(format!(
            "skipped(cooldown={}, inflight={})",
            skipped_cooldown, skipped_inflight
        ));
    }
    if let Some(last_attempt_error) = last_attempt_error
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("last_attempt={last_attempt_error}"));
    }
    parts.join("; ")
}

fn resolve_upstream_is_stream(client_is_stream: bool, path: &str) -> bool {
    let is_compact_path =
        path == "/v1/responses/compact" || path.starts_with("/v1/responses/compact?");
    client_is_stream || (path.starts_with("/v1/responses") && !is_compact_path)
}

fn request_deadline_for_path(
    started_at: Instant,
    client_is_stream: bool,
    path: &str,
) -> Option<Instant> {
    let upstream_is_stream = resolve_upstream_is_stream(client_is_stream, path);
    // 中文注释：deadline 要按真实上游传输形态计算，避免 /v1/responses 在下游非流式时
    // 仍被较短 total timeout 抢先截断 SSE 上游读取。
    super::support::deadline::request_deadline(started_at, upstream_is_stream)
}

fn should_try_provider_executor_aggregate_route(
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> bool {
    matches!(
        execution_plan.route_kind,
        GatewayUpstreamRouteKind::AggregateApi
    )
}

fn is_hybrid_account_first_route(
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> bool {
    matches!(
        execution_plan.route_kind,
        GatewayUpstreamRouteKind::HybridAccountFirst
    )
}

fn respond_when_account_candidates_empty(
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> bool {
    !is_hybrid_account_first_route(execution_plan)
}

fn should_fallback_to_aggregate_after_account_exhaustion(
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> bool {
    is_hybrid_account_first_route(execution_plan)
}

fn executor_kind_label(value: GatewayUpstreamExecutorKind) -> &'static str {
    match value {
        GatewayUpstreamExecutorKind::CodexResponses => "codex_responses",
        GatewayUpstreamExecutorKind::Claude => "claude",
        GatewayUpstreamExecutorKind::Gemini => "gemini",
    }
}

fn route_kind_label(value: GatewayUpstreamRouteKind) -> &'static str {
    match value {
        GatewayUpstreamRouteKind::AccountRotation => "account_rotation",
        GatewayUpstreamRouteKind::AggregateApi => "aggregate_api",
        GatewayUpstreamRouteKind::HybridAccountFirst => "hybrid_account_first",
    }
}

fn model_route_error(
    storage: &codexmanager_core::storage::Storage,
    key_id: &str,
    model: Option<&str>,
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> Result<(), (u16, String)> {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    bootstrap_model_routes_for_plan(storage, execution_plan);
    let model_exists = storage
        .list_model_catalog_models("default")
        .map_err(|err| (500, format!("model_catalog_read_failed: {err}")))?
        .into_iter()
        .any(|item| item.slug == model);
    if !model_exists {
        return Err((404, format!("model_not_found: {model}")));
    }
    if let Err(err) = crate::resolve_api_key_model_group_access(storage, key_id, model) {
        if err.contains("model_not_allowed") {
            return Err((403, err));
        }
        return Err((500, err));
    }
    let mappings = storage
        .list_enabled_model_source_mappings_for_platform(model)
        .map_err(|err| (500, format!("model_mapping_read_failed: {err}")))?;
    if !mappings
        .into_iter()
        .any(|mapping| source_mapping_matches_route(mapping.source_kind.as_str(), execution_plan))
    {
        return Err((503, format!("model_unavailable: {model}")));
    }
    Ok(())
}

fn bootstrap_model_routes_for_plan(
    storage: &codexmanager_core::storage::Storage,
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) {
    match execution_plan.route_kind {
        GatewayUpstreamRouteKind::AccountRotation => {
            let _ = crate::apikey_models::bootstrap_account_pool_model_routes(storage, false);
        }
        GatewayUpstreamRouteKind::AggregateApi => {
            let _ = crate::apikey_models::bootstrap_aggregate_api_model_routes(storage);
        }
        GatewayUpstreamRouteKind::HybridAccountFirst => {
            let _ = crate::apikey_models::bootstrap_account_pool_model_routes(storage, false);
            let _ = crate::apikey_models::bootstrap_aggregate_api_model_routes(storage);
        }
    }
}

fn source_mapping_matches_route(
    source_kind: &str,
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> bool {
    match execution_plan.route_kind {
        GatewayUpstreamRouteKind::AccountRotation => source_kind == "openai_account",
        GatewayUpstreamRouteKind::AggregateApi => source_kind == "aggregate_api",
        GatewayUpstreamRouteKind::HybridAccountFirst => {
            source_kind == "openai_account" || source_kind == "aggregate_api"
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn respond_model_route_error(
    request: Request,
    storage: &crate::storage_helpers::StorageHandle,
    trace_id: &str,
    key_id: &str,
    original_path: &str,
    path: &str,
    request_method: &str,
    response_adapter: super::super::ResponseAdapter,
    effective_service_tier_for_log: Option<&str>,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    started_at: Instant,
    status_code: u16,
    message: String,
) -> Result<(), String> {
    super::super::record_gateway_request_outcome(path, status_code, Some("model_route"));
    super::super::trace_log::log_request_final(
        trace_id,
        status_code,
        Some(key_id),
        None,
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
    );
    super::super::write_request_log(
        storage,
        super::super::request_log::RequestLogTraceContext {
            trace_id: Some(trace_id),
            original_path: Some(original_path),
            adapted_path: Some(path),
            response_adapter: Some(response_adapter),
            effective_service_tier: effective_service_tier_for_log,
            ..Default::default()
        },
        Some(key_id),
        None,
        path,
        request_method,
        model_for_log,
        reasoning_for_log,
        None,
        Some(status_code),
        super::super::request_log::RequestLogUsage::default(),
        Some(message.as_str()),
        Some(started_at.elapsed().as_millis()),
    );
    let response = super::super::error_response::terminal_text_response(
        status_code,
        super::super::error_message_for_client(
            super::super::prefers_raw_errors_for_tiny_http_request(&request),
            message,
        ),
        Some(trace_id),
    );
    let _ = request.respond(response);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn resolve_aggregate_candidates_for_route(
    storage: &crate::storage_helpers::StorageHandle,
    protocol_type: &str,
    aggregate_api_id: Option<&str>,
    model_for_log: Option<&str>,
) -> Result<Vec<codexmanager_core::storage::AggregateApi>, String> {
    let mut candidates = super::protocol::aggregate_api::resolve_aggregate_api_rotation_candidates(
        storage,
        protocol_type,
        aggregate_api_id,
    )?;
    let Some(model) = model_for_log
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(candidates);
    };
    candidates = candidates
        .into_iter()
        .filter_map(|mut api| {
            let mapping = storage
                .find_enabled_model_source_mapping(model, "aggregate_api", api.id.as_str())
                .ok()
                .flatten()?;
            api.model_override = Some(mapping.upstream_model);
            Some(api)
        })
        .collect();
    if candidates.is_empty() {
        Err(format!("model_unavailable: {model}"))
    } else {
        Ok(candidates)
    }
}

fn hybrid_route_error_message(account_error: Option<&str>, aggregate_error: &str) -> String {
    match account_error.map(str::trim).filter(|value| !value.is_empty()) {
        Some(account_error) => crate::gateway::bilingual_error(
            format!("账号池与聚合 API 均不可用：{account_error}；聚合 API：{aggregate_error}"),
            format!("account pool and aggregate api are unavailable: {account_error}; aggregate api: {aggregate_error}"),
        ),
        None => crate::gateway::bilingual_error(
            format!("账号池与聚合 API 均不可用；聚合 API：{aggregate_error}"),
            format!("account pool and aggregate api are unavailable; aggregate api: {aggregate_error}"),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn respond_hybrid_route_error(
    request: Request,
    storage: &crate::storage_helpers::StorageHandle,
    trace_id: &str,
    key_id: &str,
    original_path: &str,
    path: &str,
    request_method: &str,
    response_adapter: super::super::ResponseAdapter,
    effective_service_tier_for_log: Option<&str>,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    started_at: Instant,
    account_error: Option<&str>,
    aggregate_error: String,
) -> Result<(), String> {
    let message = hybrid_route_error_message(account_error, aggregate_error.as_str());
    respond_aggregate_route_error(
        request,
        storage,
        trace_id,
        key_id,
        original_path,
        path,
        request_method,
        response_adapter,
        effective_service_tier_for_log,
        model_for_log,
        reasoning_for_log,
        started_at,
        message,
    )
}

#[cfg(test)]
fn provider_upstream_hint(
    value: GatewayUpstreamExecutorKind,
) -> Option<(&'static str, &'static str)> {
    match value {
        GatewayUpstreamExecutorKind::Claude => Some(("Claude", "claude")),
        GatewayUpstreamExecutorKind::Gemini => Some(("Gemini", "gemini")),
        GatewayUpstreamExecutorKind::CodexResponses => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn respond_aggregate_route_error(
    request: Request,
    storage: &crate::storage_helpers::StorageHandle,
    trace_id: &str,
    key_id: &str,
    original_path: &str,
    path: &str,
    request_method: &str,
    response_adapter: super::super::ResponseAdapter,
    effective_service_tier_for_log: Option<&str>,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    started_at: Instant,
    message: String,
) -> Result<(), String> {
    super::super::record_gateway_request_outcome(path, 404, Some("aggregate_api"));
    super::super::trace_log::log_request_final(
        trace_id,
        404,
        Some(key_id),
        None,
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
    );
    super::super::write_request_log(
        storage,
        super::super::request_log::RequestLogTraceContext {
            trace_id: Some(trace_id),
            original_path: Some(original_path),
            adapted_path: Some(path),
            response_adapter: Some(response_adapter),
            effective_service_tier: effective_service_tier_for_log,
            ..Default::default()
        },
        Some(key_id),
        None,
        path,
        request_method,
        model_for_log,
        reasoning_for_log,
        None,
        Some(404),
        super::super::request_log::RequestLogUsage::default(),
        Some(message.as_str()),
        Some(started_at.elapsed().as_millis()),
    );
    let response = super::super::error_response::terminal_text_response(
        404,
        super::super::error_message_for_client(
            super::super::prefers_raw_errors_for_tiny_http_request(&request),
            message,
        ),
        Some(trace_id),
    );
    let _ = request.respond(response);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn proxy_with_aggregate_candidates(
    request: Request,
    storage: &crate::storage_helpers::StorageHandle,
    trace_id: &str,
    key_id: &str,
    original_path: &str,
    path: &str,
    request_method: &str,
    method: &reqwest::Method,
    body: &bytes::Bytes,
    client_is_stream: bool,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    effective_service_tier_for_log: Option<&str>,
    aggregate_api_id: Option<&str>,
    request_deadline: Option<Instant>,
    started_at: Instant,
    aggregate_api_candidates: Vec<codexmanager_core::storage::AggregateApi>,
) -> Result<(), String> {
    let mut aggregate_api_candidates = aggregate_api_candidates;
    super::protocol::aggregate_api::apply_gateway_route_strategy_to_aggregate_candidates(
        &mut aggregate_api_candidates,
        key_id,
        model_for_log,
        aggregate_api_id,
    );

    super::protocol::aggregate_api::proxy_aggregate_request(
        super::protocol::aggregate_api::AggregateProxyRequest {
            request,
            storage,
            trace_id,
            key_id,
            original_path,
            path,
            request_method,
            method,
            body,
            is_stream: client_is_stream,
            response_adapter: super::super::ResponseAdapter::Passthrough,
            model_for_log,
            reasoning_for_log,
            effective_service_tier_for_log,
            aggregate_api_candidates,
            request_deadline,
            started_at,
        },
    )
}

/// 函数 `proxy_validated_request`
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
pub(in super::super) fn proxy_validated_request(
    request: Request,
    validated: LocalValidationResult,
    debug: bool,
) -> Result<(), String> {
    let LocalValidationResult {
        trace_id,
        incoming_headers,
        storage,
        original_path,
        passthrough_path,
        path,
        passthrough_body,
        body,
        is_stream,
        has_prompt_cache_key,
        request_shape,
        protocol_type,
        rotation_strategy,
        aggregate_api_id,
        account_plan_filter,
        response_adapter,
        gemini_stream_output_mode,
        tool_name_restore_map,
        request_method,
        key_id,
        platform_key_hash,
        local_conversation_id,
        conversation_binding,
        model_for_log,
        reasoning_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        method,
    } = validated;
    let started_at = Instant::now();
    let client_is_stream = is_stream;
    // 中文注释：对齐 Codex 上游协议：/v1/responses 固定走 SSE。
    // 下游是否流式仍由客户端 `stream` 参数决定（在 response bridge 层聚合/透传）。
    let upstream_is_stream = resolve_upstream_is_stream(client_is_stream, path.as_str());
    let request_deadline = request_deadline_for_path(started_at, client_is_stream, path.as_str());

    super::super::trace_log::log_request_start(
        trace_id.as_str(),
        key_id.as_str(),
        request_method.as_str(),
        path.as_str(),
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        service_tier_for_log.as_deref(),
        client_is_stream,
        "http",
        protocol_type.as_str(),
    );
    super::super::trace_log::log_request_body_preview(trace_id.as_str(), body.as_ref());
    if protocol_type == crate::apikey_profile::PROTOCOL_GEMINI_NATIVE {
        super::super::trace_log::log_gemini_request_diagnostics(
            trace_id.as_str(),
            original_path.as_str(),
            path.as_str(),
            format!("{response_adapter:?}").as_str(),
            gemini_stream_output_mode.map(|mode| match mode {
                super::super::GeminiStreamOutputMode::Sse => "sse",
                super::super::GeminiStreamOutputMode::Raw => "raw",
            }),
            body.as_ref(),
        );
    }

    let execution_plan =
        resolve_gateway_upstream_execution_plan(protocol_type.as_str(), rotation_strategy.as_str());
    super::super::log_request_execution_plan(
        trace_id.as_str(),
        path.as_str(),
        protocol_type.as_str(),
        executor_kind_label(execution_plan.executor_kind),
        route_kind_label(execution_plan.route_kind),
    );

    if let Err((status_code, message)) = model_route_error(
        &storage,
        key_id.as_str(),
        model_for_log.as_deref(),
        execution_plan,
    ) {
        return respond_model_route_error(
            request,
            &storage,
            trace_id.as_str(),
            key_id.as_str(),
            original_path.as_str(),
            path.as_str(),
            request_method.as_str(),
            response_adapter,
            effective_service_tier_for_log.as_deref(),
            model_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            started_at,
            status_code,
            message,
        );
    }

    if should_try_provider_executor_aggregate_route(execution_plan) {
        match resolve_aggregate_candidates_for_route(
            &storage,
            protocol_type.as_str(),
            aggregate_api_id.as_deref(),
            model_for_log.as_deref(),
        ) {
            Ok(aggregate_api_candidates) => {
                return proxy_with_aggregate_candidates(
                    request,
                    &storage,
                    trace_id.as_str(),
                    key_id.as_str(),
                    original_path.as_str(),
                    path.as_str(),
                    request_method.as_str(),
                    &method,
                    &body,
                    client_is_stream,
                    model_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    effective_service_tier_for_log.as_deref(),
                    aggregate_api_id.as_deref(),
                    request_deadline,
                    started_at,
                    aggregate_api_candidates,
                );
            }
            Err(err) => {
                return respond_aggregate_route_error(
                    request,
                    &storage,
                    trace_id.as_str(),
                    key_id.as_str(),
                    original_path.as_str(),
                    path.as_str(),
                    request_method.as_str(),
                    super::super::ResponseAdapter::Passthrough,
                    effective_service_tier_for_log.as_deref(),
                    model_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    started_at,
                    err,
                );
            }
        }
    }

    let (request, mut candidates) = match prepare_candidates_for_proxy(
        request,
        &storage,
        trace_id.as_str(),
        &key_id,
        &original_path,
        &path,
        response_adapter,
        &request_method,
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        account_plan_filter.as_deref(),
        respond_when_account_candidates_empty(execution_plan),
    ) {
        CandidatePrecheckResult::Ready {
            request,
            candidates,
        } => (request, candidates),
        CandidatePrecheckResult::Empty { request } => {
            match resolve_aggregate_candidates_for_route(
                &storage,
                protocol_type.as_str(),
                aggregate_api_id.as_deref(),
                model_for_log.as_deref(),
            ) {
                Ok(aggregate_api_candidates) => {
                    return proxy_with_aggregate_candidates(
                        request,
                        &storage,
                        trace_id.as_str(),
                        key_id.as_str(),
                        original_path.as_str(),
                        passthrough_path.as_str(),
                        request_method.as_str(),
                        &method,
                        &passthrough_body,
                        client_is_stream,
                        model_for_log.as_deref(),
                        reasoning_for_log.as_deref(),
                        effective_service_tier_for_log.as_deref(),
                        aggregate_api_id.as_deref(),
                        request_deadline,
                        started_at,
                        aggregate_api_candidates,
                    );
                }
                Err(err) => {
                    return respond_hybrid_route_error(
                        request,
                        &storage,
                        trace_id.as_str(),
                        key_id.as_str(),
                        original_path.as_str(),
                        passthrough_path.as_str(),
                        request_method.as_str(),
                        super::super::ResponseAdapter::Passthrough,
                        effective_service_tier_for_log.as_deref(),
                        model_for_log.as_deref(),
                        reasoning_for_log.as_deref(),
                        started_at,
                        Some("无可用账号(no available account)"),
                        err,
                    );
                }
            }
        }
        CandidatePrecheckResult::Responded => return Ok(()),
    };
    let setup = prepare_request_setup(
        path.as_str(),
        protocol_type.as_str(),
        has_prompt_cache_key,
        &incoming_headers,
        &body,
        &mut candidates,
        key_id.as_str(),
        platform_key_hash.as_str(),
        local_conversation_id.as_deref(),
        conversation_binding.as_ref(),
        model_for_log.as_deref(),
        trace_id.as_str(),
    );
    let base = setup.upstream_base.as_str();

    let context = GatewayUpstreamExecutionContext::new(
        &trace_id,
        &storage,
        &key_id,
        &original_path,
        &path,
        &request_method,
        response_adapter,
        protocol_type.as_str(),
        model_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        service_tier_for_log.as_deref(),
        effective_service_tier_for_log.as_deref(),
        setup.candidate_count,
        setup.account_max_inflight,
    );
    let allow_openai_fallback = setup.upstream_fallback_base.is_some();
    let disable_challenge_stateless_retry = !(protocol_type == PROTOCOL_ANTHROPIC_NATIVE
        && body.len() <= 2 * 1024)
        && !path.starts_with("/v1/responses");
    let _request_gate_guard = acquire_request_gate(
        trace_id.as_str(),
        key_id.as_str(),
        path.as_str(),
        model_for_log.as_deref(),
        request_deadline,
    );
    let exhausted = match execute_candidate_sequence(
        request,
        candidates,
        CandidateExecutorParams {
            storage: &storage,
            method: &method,
            incoming_headers: &incoming_headers,
            body: &body,
            path: path.as_str(),
            request_shape: request_shape.as_deref(),
            trace_id: trace_id.as_str(),
            model_for_log: model_for_log.as_deref(),
            response_adapter,
            gemini_stream_output_mode,
            tool_name_restore_map: &tool_name_restore_map,
            context: &context,
            setup: &setup,
            request_deadline,
            started_at,
            client_is_stream,
            upstream_is_stream,
            debug,
            allow_openai_fallback,
            disable_challenge_stateless_retry,
        },
    )? {
        CandidateExecutionResult::Handled => return Ok(()),
        CandidateExecutionResult::Exhausted {
            request,
            attempted_account_ids,
            skipped_cooldown,
            skipped_inflight,
            last_attempt_url,
            last_attempt_error,
        } => (
            request,
            attempted_account_ids,
            skipped_cooldown,
            skipped_inflight,
            last_attempt_url,
            last_attempt_error,
        ),
    };
    let (
        request,
        attempted_account_ids,
        skipped_cooldown,
        skipped_inflight,
        last_attempt_url,
        last_attempt_error,
    ) = exhausted;
    let final_error = exhausted_gateway_error_for_log(
        attempted_account_ids.as_slice(),
        skipped_cooldown,
        skipped_inflight,
        last_attempt_error.as_deref(),
    );
    if should_fallback_to_aggregate_after_account_exhaustion(execution_plan) {
        match resolve_aggregate_candidates_for_route(
            &storage,
            protocol_type.as_str(),
            aggregate_api_id.as_deref(),
            model_for_log.as_deref(),
        ) {
            Ok(aggregate_api_candidates) => {
                return proxy_with_aggregate_candidates(
                    request,
                    &storage,
                    trace_id.as_str(),
                    key_id.as_str(),
                    original_path.as_str(),
                    passthrough_path.as_str(),
                    request_method.as_str(),
                    &method,
                    &passthrough_body,
                    client_is_stream,
                    model_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    effective_service_tier_for_log.as_deref(),
                    aggregate_api_id.as_deref(),
                    request_deadline,
                    started_at,
                    aggregate_api_candidates,
                );
            }
            Err(err) => {
                return respond_hybrid_route_error(
                    request,
                    &storage,
                    trace_id.as_str(),
                    key_id.as_str(),
                    original_path.as_str(),
                    passthrough_path.as_str(),
                    request_method.as_str(),
                    super::super::ResponseAdapter::Passthrough,
                    effective_service_tier_for_log.as_deref(),
                    model_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    started_at,
                    Some(final_error.as_str()),
                    err,
                );
            }
        }
    }

    context.log_final_result(
        None,
        last_attempt_url.as_deref().or(Some(base)),
        503,
        RequestLogUsage::default(),
        Some(final_error.as_str()),
        started_at.elapsed().as_millis(),
        (!attempted_account_ids.is_empty()).then_some(attempted_account_ids.as_slice()),
    );
    respond_terminal(
        request,
        503,
        crate::gateway::bilingual_error("无可用账号", "no available account"),
        Some(trace_id.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        exhausted_gateway_error_for_log, hybrid_route_error_message, model_route_error,
        provider_upstream_hint, request_deadline_for_path, resolve_upstream_is_stream,
        respond_when_account_candidates_empty,
        should_fallback_to_aggregate_after_account_exhaustion,
        should_try_provider_executor_aggregate_route,
    };
    use crate::gateway::upstream::executor::{
        GatewayUpstreamExecutionPlan, GatewayUpstreamExecutorKind, GatewayUpstreamRouteKind,
    };
    use codexmanager_core::rpc::types::{
        ManagedModelCatalogEntry, ManagedModelCatalogResult, ModelInfo,
    };
    use codexmanager_core::storage::{now_ts, AggregateApi, ModelSourceMapping, Storage};
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    fn execution_plan(route_kind: GatewayUpstreamRouteKind) -> GatewayUpstreamExecutionPlan {
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind,
        }
    }

    fn insert_test_aggregate_api(storage: &Storage, id: &str) {
        let now = now_ts();
        storage
            .insert_aggregate_api(&AggregateApi {
                id: id.to_string(),
                provider_type: "codex".to_string(),
                supplier_name: Some(id.to_string()),
                sort: 0,
                url: format!("https://{id}.example/v1"),
                auth_type: "apikey".to_string(),
                auth_params_json: None,
                action: None,
                model_override: None,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
                last_test_at: None,
                last_test_status: None,
                last_test_error: None,
                balance_query_enabled: false,
                balance_query_template: None,
                balance_query_base_url: None,
                balance_query_user_id: None,
                balance_query_config_json: None,
                last_balance_at: None,
                last_balance_status: None,
                last_balance_error: None,
                last_balance_json: None,
            })
            .expect("insert aggregate api");
    }

    fn seed_platform_catalog(storage: &Storage, slug: &str) {
        crate::apikey_models::save_managed_model_catalog_with_storage(
            storage,
            &ManagedModelCatalogResult {
                items: vec![ManagedModelCatalogEntry {
                    model: ModelInfo {
                        slug: slug.to_string(),
                        display_name: slug.to_string(),
                        supported_in_api: true,
                        visibility: Some("list".to_string()),
                        ..Default::default()
                    },
                    source_kind: "remote".to_string(),
                    user_edited: false,
                    sort_index: 0,
                    updated_at: now_ts(),
                }],
                extra: BTreeMap::new(),
            },
        )
        .expect("seed platform catalog");
    }

    /// 函数 `exhausted_gateway_error_includes_attempts_skips_and_last_error`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn exhausted_gateway_error_includes_attempts_skips_and_last_error() {
        let message = exhausted_gateway_error_for_log(
            &["acc-a".to_string(), "acc-b".to_string()],
            2,
            1,
            Some("upstream challenge blocked"),
        );

        assert!(message.contains("no available account"));
        assert!(message.contains("kind=no_available_account_exhausted"));
        assert!(message.contains("attempted=acc-a,acc-b"));
        assert!(message.contains("skipped(cooldown=2, inflight=1)"));
        assert!(message.contains("last_attempt=upstream challenge blocked"));
    }

    /// 函数 `exhausted_gateway_error_marks_cooldown_only_skip_kind`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn exhausted_gateway_error_marks_cooldown_only_skip_kind() {
        let message = exhausted_gateway_error_for_log(&[], 2, 0, None);

        assert!(message.contains("kind=no_available_account_cooldown"));
    }

    #[test]
    fn resolve_upstream_is_stream_keeps_non_compact_responses_on_sse_upstream() {
        assert!(resolve_upstream_is_stream(false, "/v1/responses"));
        assert!(resolve_upstream_is_stream(
            false,
            "/v1/responses?stream=false"
        ));
        assert!(!resolve_upstream_is_stream(false, "/v1/responses/compact"));
        assert!(!resolve_upstream_is_stream(false, "/v1/chat/completions"));
        assert!(resolve_upstream_is_stream(true, "/v1/chat/completions"));
    }

    #[test]
    fn request_deadline_for_responses_uses_upstream_stream_semantics() {
        let _guard = crate::test_env_guard();
        let previous_total = crate::gateway::current_upstream_total_timeout_ms();
        let previous_stream = crate::gateway::current_upstream_stream_timeout_ms();

        crate::gateway::set_upstream_total_timeout_ms(120_000);
        crate::gateway::set_upstream_stream_timeout_ms(300_000);

        let started_at = Instant::now();
        let deadline = request_deadline_for_path(started_at, false, "/v1/responses")
            .expect("responses deadline");
        let timeout = deadline
            .checked_duration_since(started_at)
            .expect("deadline should be after start");

        crate::gateway::set_upstream_total_timeout_ms(previous_total);
        crate::gateway::set_upstream_stream_timeout_ms(previous_stream);

        assert!(timeout > Duration::from_secs(250));
        assert!(timeout <= Duration::from_secs(300));
    }

    #[test]
    fn only_explicit_aggregate_route_uses_aggregate_candidates() {
        assert!(should_try_provider_executor_aggregate_route(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::Claude,
                route_kind: GatewayUpstreamRouteKind::AggregateApi,
            }
        ));
        assert!(should_try_provider_executor_aggregate_route(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::Gemini,
                route_kind: GatewayUpstreamRouteKind::AggregateApi,
            }
        ));
        assert!(!should_try_provider_executor_aggregate_route(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::Claude,
                route_kind: GatewayUpstreamRouteKind::AccountRotation,
            }
        ));
        assert!(should_try_provider_executor_aggregate_route(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
                route_kind: GatewayUpstreamRouteKind::AggregateApi,
            }
        ));
        assert!(!should_try_provider_executor_aggregate_route(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::Gemini,
                route_kind: GatewayUpstreamRouteKind::AccountRotation,
            }
        ));
        assert!(!should_try_provider_executor_aggregate_route(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
                route_kind: GatewayUpstreamRouteKind::AccountRotation,
            }
        ));
    }

    #[test]
    fn hybrid_account_first_keeps_account_empty_for_aggregate_fallback() {
        let hybrid = GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::HybridAccountFirst,
        };
        let account_only = GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::AccountRotation,
        };
        let aggregate_only = GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::AggregateApi,
        };

        assert!(!respond_when_account_candidates_empty(hybrid));
        assert!(respond_when_account_candidates_empty(account_only));
        assert!(respond_when_account_candidates_empty(aggregate_only));
    }

    #[test]
    fn only_hybrid_falls_back_to_aggregate_after_account_exhaustion() {
        assert!(should_fallback_to_aggregate_after_account_exhaustion(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
                route_kind: GatewayUpstreamRouteKind::HybridAccountFirst,
            }
        ));
        assert!(!should_fallback_to_aggregate_after_account_exhaustion(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
                route_kind: GatewayUpstreamRouteKind::AccountRotation,
            }
        ));
        assert!(!should_fallback_to_aggregate_after_account_exhaustion(
            GatewayUpstreamExecutionPlan {
                executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
                route_kind: GatewayUpstreamRouteKind::AggregateApi,
            }
        ));
    }

    #[test]
    fn hybrid_route_error_mentions_both_pools() {
        let message = hybrid_route_error_message(
            Some("无可用账号(no available account)"),
            "aggregate api not found for provider codex",
        );

        assert!(message.contains("账号池与聚合 API 均不可用"));
        assert!(message.contains("no available account"));
        assert!(message.contains("aggregate api not found for provider codex"));
    }

    #[test]
    fn provider_upstream_hint_reports_expected_aggregate_provider_type() {
        assert_eq!(
            provider_upstream_hint(GatewayUpstreamExecutorKind::Claude),
            Some(("Claude", "claude"))
        );
        assert_eq!(
            provider_upstream_hint(GatewayUpstreamExecutorKind::Gemini),
            Some(("Gemini", "gemini"))
        );
        assert_eq!(
            provider_upstream_hint(GatewayUpstreamExecutorKind::CodexResponses),
            None
        );
    }

    #[test]
    fn aggregate_route_model_validation_bootstraps_aggregate_source() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-route");
        storage
            .upsert_discovered_model_source_models(
                "aggregate_api",
                "agg-route",
                &["vendor-route".to_string()],
                "synced",
            )
            .expect("seed aggregate source model");

        model_route_error(
            &storage,
            "key-route",
            Some("vendor-route"),
            execution_plan(GatewayUpstreamRouteKind::AggregateApi),
        )
        .expect("aggregate route should bootstrap source mapping");

        let mappings = storage
            .list_enabled_model_source_mappings_for_platform("vendor-route")
            .expect("list mappings");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].source_kind, "aggregate_api");
        assert_eq!(mappings[0].source_id, "agg-route");
    }

    #[test]
    fn account_route_model_validation_ignores_aggregate_only_mapping() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        seed_platform_catalog(&storage, "vendor-account-route");
        let now = now_ts();
        storage
            .upsert_model_source_mapping(&ModelSourceMapping {
                id: "mapping-aggregate-only".to_string(),
                platform_model_slug: "vendor-account-route".to_string(),
                source_kind: "aggregate_api".to_string(),
                source_id: "agg-only".to_string(),
                upstream_model: "vendor-account-route".to_string(),
                enabled: true,
                priority: 0,
                weight: 1,
                billing_model_slug: None,
                created_at: now,
                updated_at: now,
            })
            .expect("seed aggregate mapping");

        let err = model_route_error(
            &storage,
            "key-route",
            Some("vendor-account-route"),
            execution_plan(GatewayUpstreamRouteKind::AccountRotation),
        )
        .expect_err("account route should require an account mapping");

        assert_eq!(err.0, 503);
        assert!(err.1.contains("model_unavailable"));
    }

    #[test]
    fn hybrid_model_validation_accepts_aggregate_mapping() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        insert_test_aggregate_api(&storage, "agg-hybrid");
        storage
            .upsert_discovered_model_source_models(
                "aggregate_api",
                "agg-hybrid",
                &["vendor-hybrid".to_string()],
                "synced",
            )
            .expect("seed aggregate source model");

        model_route_error(
            &storage,
            "key-route",
            Some("vendor-hybrid"),
            execution_plan(GatewayUpstreamRouteKind::HybridAccountFirst),
        )
        .expect("hybrid route should accept aggregate mapping");
    }
}
