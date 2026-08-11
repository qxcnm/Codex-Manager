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

fn low_quota_candidate_mode_for_protocol(
    protocol_type: &str,
) -> super::super::LowQuotaCandidateMode {
    if protocol_type == PROTOCOL_ANTHROPIC_NATIVE {
        // 中文注释：Anthropic messages 兼容请求容易触发账号级 Cloudflare challenge；
        // 只在该协议下把低额度账号放到末尾兜底，普通 Codex 路径仍保留原有配额保护。
        return super::super::LowQuotaCandidateMode::AppendFallback;
    }
    super::super::LowQuotaCandidateMode::NormalOnly
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
        .model_catalog_model_exists("default", model)
        .map_err(|err| (500, format!("model_catalog_read_failed: {err}")))?;
    if !model_exists {
        return Err((404, format!("model_not_found: {model}")));
    }
    if let Err(err) = crate::resolve_api_key_model_group_access(storage, key_id, model) {
        if err.contains("model_not_allowed") {
            return Err((403, err));
        }
        return Err((500, err));
    }
    let route_source_kinds = source_kinds_for_route(execution_plan);
    let has_upstream_source_match =
        direct_upstream_model_matches_route(storage, model, &route_source_kinds)
            .map_err(|err| (500, format!("source_model_read_failed: {err}")))?;
    if !has_upstream_source_match {
        return Err((503, format!("model_unavailable: {model}")));
    }
    Ok(())
}

fn direct_upstream_model_matches_route(
    storage: &codexmanager_core::storage::Storage,
    model: &str,
    source_kinds: &[&str],
) -> Result<bool, String> {
    for source_kind in source_kinds {
        let ids = storage
            .list_available_source_model_ids_by_upstream_model(source_kind, model)
            .map_err(|err| err.to_string())?;
        if !ids.is_empty() {
            return Ok(true);
        }
        if *source_kind == "aggregate_api" && has_active_aggregate_model_override(storage)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_active_aggregate_model_override(
    storage: &codexmanager_core::storage::Storage,
) -> Result<bool, String> {
    let apis = storage
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?;
    Ok(apis.iter().any(|api| {
        api.status.trim().eq_ignore_ascii_case("active")
            && api
                .model_override
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
    }))
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

fn source_kinds_for_route(
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> Vec<&'static str> {
    match execution_plan.route_kind {
        GatewayUpstreamRouteKind::AccountRotation => vec!["openai_account"],
        GatewayUpstreamRouteKind::AggregateApi => vec!["aggregate_api"],
        GatewayUpstreamRouteKind::HybridAccountFirst => vec!["openai_account", "aggregate_api"],
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
    service_tier_for_log: Option<&str>,
    effective_service_tier_for_log: Option<&str>,
    service_tier_source_for_log: Option<&str>,
    gateway_mode_for_log: Option<&str>,
    client_model_for_log: Option<&str>,
    model_for_log: Option<&str>,
    model_source_for_log: Option<&str>,
    client_reasoning_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    reasoning_source_for_log: Option<&str>,
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
            gateway_mode: gateway_mode_for_log,
            route_strategy: Some(super::super::current_route_strategy()),
            route_source: Some("route_strategy"),
            client_model: client_model_for_log,
            model_source: model_source_for_log,
            client_reasoning_effort: client_reasoning_for_log,
            reasoning_source: reasoning_source_for_log,
            response_adapter: Some(response_adapter),
            service_tier: service_tier_for_log,
            effective_service_tier: effective_service_tier_for_log,
            service_tier_source: service_tier_source_for_log,
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
    storage: &codexmanager_core::storage::Storage,
    protocol_type: &str,
    aggregate_api_id: Option<&str>,
    model_for_log: Option<&str>,
) -> Result<Vec<codexmanager_core::storage::AggregateApi>, String> {
    let explicit_candidate =
        resolve_active_explicit_aggregate_candidate(storage, aggregate_api_id)?;
    let mut candidates =
        match super::protocol::aggregate_api::resolve_aggregate_api_rotation_candidates(
            storage,
            protocol_type,
            aggregate_api_id,
        ) {
            Ok(candidates) => candidates,
            Err(_) if explicit_candidate.is_some() => Vec::new(),
            Err(err) => return Err(err),
        };
    if let Some(explicit_candidate) = explicit_candidate {
        candidates.retain(|candidate| candidate.id != explicit_candidate.id);
        candidates.insert(0, explicit_candidate);
    }
    apply_aggregate_model_filter(storage, candidates, model_for_log)
}

fn resolve_active_explicit_aggregate_candidate(
    storage: &codexmanager_core::storage::Storage,
    aggregate_api_id: Option<&str>,
) -> Result<Option<codexmanager_core::storage::AggregateApi>, String> {
    let Some(api_id) = aggregate_api_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let candidate = storage
        .find_aggregate_api_by_id(api_id)
        .map_err(|err| format!("find explicit aggregate api failed: {err}"))?;
    Ok(candidate.filter(super::protocol::aggregate_api::aggregate_api_is_routable))
}

fn apply_aggregate_model_filter(
    storage: &codexmanager_core::storage::Storage,
    mut candidates: Vec<codexmanager_core::storage::AggregateApi>,
    model_for_log: Option<&str>,
) -> Result<Vec<codexmanager_core::storage::AggregateApi>, String> {
    let Some(model) = model_for_log
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(candidates);
    };
    let source_ids = storage
        .list_available_source_model_ids_by_upstream_model("aggregate_api", model)
        .map_err(|err| format!("list aggregate source models failed: {err}"))?;
    let allowed = source_ids
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    candidates.retain(|api| {
        allowed.contains(&api.id)
            || api
                .model_override
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
    });
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
    service_tier_for_log: Option<&str>,
    effective_service_tier_for_log: Option<&str>,
    service_tier_source_for_log: Option<&str>,
    gateway_mode_for_log: Option<&str>,
    client_model_for_log: Option<&str>,
    model_for_log: Option<&str>,
    model_source_for_log: Option<&str>,
    client_reasoning_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    reasoning_source_for_log: Option<&str>,
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
        service_tier_for_log,
        effective_service_tier_for_log,
        service_tier_source_for_log,
        gateway_mode_for_log,
        client_model_for_log,
        model_for_log,
        model_source_for_log,
        client_reasoning_for_log,
        reasoning_for_log,
        reasoning_source_for_log,
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
    service_tier_for_log: Option<&str>,
    effective_service_tier_for_log: Option<&str>,
    service_tier_source_for_log: Option<&str>,
    gateway_mode_for_log: Option<&str>,
    client_model_for_log: Option<&str>,
    model_for_log: Option<&str>,
    model_source_for_log: Option<&str>,
    client_reasoning_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    reasoning_source_for_log: Option<&str>,
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
            gateway_mode: gateway_mode_for_log,
            route_strategy: Some(super::super::current_route_strategy()),
            route_source: Some("route_strategy"),
            client_model: client_model_for_log,
            model_source: model_source_for_log,
            client_reasoning_effort: client_reasoning_for_log,
            reasoning_source: reasoning_source_for_log,
            response_adapter: Some(response_adapter),
            service_tier: service_tier_for_log,
            effective_service_tier: effective_service_tier_for_log,
            service_tier_source: service_tier_source_for_log,
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
    gateway_mode_for_log: Option<&str>,
    client_model_for_log: Option<&str>,
    model_for_log: Option<&str>,
    model_source_for_log: Option<&str>,
    client_reasoning_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    reasoning_source_for_log: Option<&str>,
    service_tier_for_log: Option<&str>,
    effective_service_tier_for_log: Option<&str>,
    service_tier_source_for_log: Option<&str>,
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
    super::protocol::aggregate_api::prepare_first_aggregate_candidate_client(
        aggregate_api_candidates.as_slice(),
        trace_id,
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
            gateway_mode_for_log,
            route_strategy_for_log: Some(super::super::current_route_strategy()),
            route_source_for_log: Some(if aggregate_api_id.is_some() {
                "aggregate_api_preferred"
            } else {
                "route_strategy"
            }),
            client_model_for_log,
            model_for_log,
            model_source_for_log,
            client_reasoning_for_log,
            reasoning_for_log,
            reasoning_source_for_log,
            service_tier_for_log,
            effective_service_tier_for_log,
            service_tier_source_for_log,
            aggregate_api_candidates,
            request_deadline,
            started_at,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn proxy_with_direct_provider(
    provider: &'static dyn super::super::provider_runtime::ProviderRuntimeAdapter,
    request: Request,
    storage: &crate::storage_helpers::StorageHandle,
    trace_id: &str,
    key_id: &str,
    original_path: &str,
    path: &str,
    request_method: &str,
    body: &bytes::Bytes,
    client_is_stream: bool,
    response_adapter: super::super::ResponseAdapter,
    gateway_mode_for_log: Option<&str>,
    client_model_for_log: Option<&str>,
    model_for_log: &str,
    model_source_for_log: Option<&str>,
    client_reasoning_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    reasoning_source_for_log: Option<&str>,
    service_tier_for_log: Option<&str>,
    effective_service_tier_for_log: Option<&str>,
    service_tier_source_for_log: Option<&str>,
    started_at: Instant,
    route_source_for_log: &str,
) -> Result<(), String> {
    let provider_id = provider.id();
    let upstream_url = format!("{provider_id}://provider");
    let upstream = match provider.execute(storage, body, model_for_log) {
        Ok(upstream) => upstream,
        Err(message) => {
            super::super::record_gateway_request_outcome(path, 502, Some(provider_id));
            super::super::write_request_log(
                storage,
                super::super::request_log::RequestLogTraceContext {
                    trace_id: Some(trace_id),
                    original_path: Some(original_path),
                    adapted_path: Some(path),
                    gateway_mode: gateway_mode_for_log,
                    route_strategy: Some(super::super::current_route_strategy()),
                    route_source: Some(route_source_for_log),
                    client_model: client_model_for_log,
                    model_source: model_source_for_log,
                    client_reasoning_effort: client_reasoning_for_log,
                    reasoning_source: reasoning_source_for_log,
                    response_adapter: Some(response_adapter),
                    service_tier: service_tier_for_log,
                    effective_service_tier: effective_service_tier_for_log,
                    service_tier_source: service_tier_source_for_log,
                    upstream_model: Some(provider.upstream_model(model_for_log)),
                    actual_source_kind: Some(provider_id),
                    ..Default::default()
                },
                Some(key_id),
                None,
                path,
                request_method,
                Some(model_for_log),
                reasoning_for_log,
                Some(upstream_url.as_str()),
                Some(502),
                RequestLogUsage::default(),
                Some(message.as_str()),
                Some(started_at.elapsed().as_millis()),
            );
            let raw_errors = super::super::prefers_raw_errors_for_tiny_http_request(&request);
            respond_terminal(
                request,
                502,
                super::super::error_message_for_client(raw_errors, message),
                Some(trace_id),
            )?;
            return Ok(());
        }
    };

    let actual_provider_source_id = upstream.actual_source_id().map(str::to_string);
    let inflight_guard = super::super::acquire_account_inflight(key_id);
    let bridge = super::super::respond_with_upstream(
        request,
        upstream,
        inflight_guard,
        response_adapter,
        None,
        None,
        path,
        None,
        client_is_stream,
        false,
        Some(trace_id),
        Some(model_for_log),
        started_at,
    )?;
    let ok = bridge.is_ok(client_is_stream);
    let status_code = bridge
        .delivered_status_code
        .unwrap_or(if ok { 200 } else { 502 });
    let error = (!ok).then(|| {
        bridge
            .error_message(client_is_stream)
            .unwrap_or_else(|| format!("{provider_id} upstream response incomplete"))
    });
    super::super::record_gateway_request_outcome(path, status_code, Some(provider_id));
    super::super::write_request_log(
        storage,
        super::super::request_log::RequestLogTraceContext {
            trace_id: Some(trace_id),
            original_path: Some(original_path),
            adapted_path: Some(path),
            gateway_mode: gateway_mode_for_log,
            route_strategy: Some(super::super::current_route_strategy()),
            route_source: Some(route_source_for_log),
            client_model: client_model_for_log,
            model_source: model_source_for_log,
            client_reasoning_effort: client_reasoning_for_log,
            reasoning_source: reasoning_source_for_log,
            response_adapter: Some(response_adapter),
            service_tier: service_tier_for_log,
            effective_service_tier: effective_service_tier_for_log,
            service_tier_source: service_tier_source_for_log,
            upstream_model: Some(provider.upstream_model(model_for_log)),
            actual_source_kind: Some(provider_id),
            actual_source_id: actual_provider_source_id.as_deref(),
            ..Default::default()
        },
        Some(key_id),
        None,
        path,
        request_method,
        Some(model_for_log),
        reasoning_for_log,
        Some(upstream_url.as_str()),
        Some(status_code),
        RequestLogUsage {
            input_tokens: bridge.usage.input_tokens,
            cached_input_tokens: bridge.usage.cached_input_tokens,
            output_tokens: bridge.usage.output_tokens,
            total_tokens: bridge.usage.total_tokens,
            reasoning_output_tokens: bridge.usage.reasoning_output_tokens,
            first_response_ms: bridge.usage.first_response_ms,
        },
        error.as_deref(),
        Some(started_at.elapsed().as_millis()),
    );
    Ok(())
}

fn resolve_hybrid_aggregate_candidates_for_prepare(
    storage: &codexmanager_core::storage::Storage,
    protocol_type: &str,
    aggregate_api_id: Option<&str>,
    key_id: &str,
    model_for_log: Option<&str>,
    trace_id: &str,
) -> Result<Vec<codexmanager_core::storage::AggregateApi>, String> {
    let candidates = resolve_aggregate_candidates_for_route(
        storage,
        protocol_type,
        aggregate_api_id,
        model_for_log,
    )?;
    let mut preview = candidates.clone();
    super::protocol::aggregate_api::preview_gateway_route_strategy_to_aggregate_candidates(
        &mut preview,
        key_id,
        model_for_log,
        aggregate_api_id,
    );
    super::protocol::aggregate_api::prepare_first_aggregate_candidate_client(
        preview.as_slice(),
        trace_id,
    );
    Ok(candidates)
}

fn take_or_resolve_aggregate_candidates(
    prepared: &mut Option<Result<Vec<codexmanager_core::storage::AggregateApi>, String>>,
    storage: &codexmanager_core::storage::Storage,
    protocol_type: &str,
    aggregate_api_id: Option<&str>,
    model_for_log: Option<&str>,
) -> Result<Vec<codexmanager_core::storage::AggregateApi>, String> {
    if let Some(result) = prepared.take() {
        return result;
    }
    resolve_aggregate_candidates_for_route(storage, protocol_type, aggregate_api_id, model_for_log)
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
        mut body,
        is_stream,
        has_prompt_cache_key,
        request_shape,
        protocol_type,
        rotation_strategy,
        aggregate_api_id,
        account_plan_filter,
        allowed_platforms,
        concurrency_limit: _concurrency_limit,
        response_adapter,
        gemini_stream_output_mode,
        tool_name_restore_map,
        request_method,
        key_id,
        platform_key_hash,
        local_conversation_id: _local_conversation_id,
        route_conversation_id,
        route_conversation_source,
        conversation_binding,
        client_model_for_log,
        mut model_for_log,
        mut model_source_for_log,
        client_reasoning_for_log,
        reasoning_for_log,
        reasoning_source_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        service_tier_source_for_log,
        gateway_mode_for_log,
        method,
    } = validated;
    let started_at = Instant::now();
    let client_is_stream = is_stream;
    // 中文注释：对齐 Codex 上游协议：/v1/responses 固定走 SSE。
    // 下游是否流式仍由客户端 `stream` 参数决定（在 response bridge 层聚合/透传）。
    let upstream_is_stream = resolve_upstream_is_stream(client_is_stream, path.as_str());
    let request_deadline = request_deadline_for_path(started_at, client_is_stream, path.as_str());
    let mut kiro_route_source = "kiro_exact_model";
    if let Some(canonical_codex_model) = model_for_log
        .as_deref()
        .and_then(|model| model.strip_prefix("codex/"))
        .filter(|model| model.starts_with("gpt-"))
        .map(str::to_string)
    {
        body = crate::kiro::routing::rewrite_request_model(&body, &canonical_codex_model)?;
        model_for_log = Some(canonical_codex_model);
        model_source_for_log = Some("codex_exact_alias".into());
    } else if crate::kiro::routing::is_smart_alias(model_for_log.as_deref()) {
        let alias = model_for_log.as_deref().unwrap_or_default();
        let decision = crate::kiro::routing::resolve_smart_route(
            &storage,
            key_id.as_str(),
            alias,
            &body,
            &allowed_platforms,
        )?;
        body = crate::kiro::routing::rewrite_request_model(&body, &decision.selected_model)?;
        log::info!(
            "event=kiro_smart_route trace_id={} {}",
            trace_id,
            decision.explanation
        );
        model_for_log = Some(decision.selected_model);
        model_source_for_log = Some(format!("smart_alias|{}", decision.explanation));
        kiro_route_source = decision.route_source;
    }

    // Every first-party provider receives the same normalized internal request.
    // Chat Completions has already been converted to Responses by local
    // validation; this typed round trip also preserves unknown future fields.
    if path == "/v1/responses" || path.starts_with("/v1/responses?") {
        let canonical = crate::gateway::canonical::CanonicalRequest::from_responses_bytes(&body)?;
        body = bytes::Bytes::from(crate::gateway::canonical::ProviderAdapter::adapt_request(
            &crate::gateway::canonical::CodexProviderAdapter,
            &canonical,
        )?);
    }

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

    if let Some(provider) = model_for_log
        .as_deref()
        .and_then(super::super::provider_runtime::direct_provider_for_model)
    {
        let provider_model = model_for_log.as_deref().unwrap_or_default();
        let route_error = if !provider.is_supported_model(provider_model) {
            Some((404, format!("model_not_found: {provider_model}")))
        } else if let Err(error) =
            crate::resolve_api_key_model_group_access(&storage, key_id.as_str(), provider_model)
        {
            Some((
                if error.contains("model_not_allowed") {
                    403
                } else {
                    500
                },
                error,
            ))
        } else {
            match provider.has_available_credentials(&storage) {
                Ok(true) => None,
                Ok(false) => Some((
                    503,
                    format!("model_unavailable: no active {} credential", provider.id()),
                )),
                Err(error) => Some((500, error)),
            }
        };
        if let Some((status, message)) = route_error {
            return respond_model_route_error(
                request,
                &storage,
                trace_id.as_str(),
                key_id.as_str(),
                original_path.as_str(),
                path.as_str(),
                request_method.as_str(),
                response_adapter,
                service_tier_for_log.as_deref(),
                effective_service_tier_for_log.as_deref(),
                service_tier_source_for_log.as_deref(),
                gateway_mode_for_log.as_deref(),
                client_model_for_log.as_deref(),
                model_for_log.as_deref(),
                model_source_for_log.as_deref(),
                client_reasoning_for_log.as_deref(),
                reasoning_for_log.as_deref(),
                reasoning_source_for_log.as_deref(),
                started_at,
                status,
                message,
            );
        }
        if path != "/v1/responses" && !path.starts_with("/v1/responses?") {
            return respond_model_route_error(
                request,
                &storage,
                trace_id.as_str(),
                key_id.as_str(),
                original_path.as_str(),
                path.as_str(),
                request_method.as_str(),
                response_adapter,
                service_tier_for_log.as_deref(),
                effective_service_tier_for_log.as_deref(),
                service_tier_source_for_log.as_deref(),
                gateway_mode_for_log.as_deref(),
                client_model_for_log.as_deref(),
                model_for_log.as_deref(),
                model_source_for_log.as_deref(),
                client_reasoning_for_log.as_deref(),
                reasoning_for_log.as_deref(),
                reasoning_source_for_log.as_deref(),
                started_at,
                400,
                format!(
                    "{} provider requires the canonical Responses path",
                    provider.id()
                ),
            );
        }
        return proxy_with_direct_provider(
            provider,
            request,
            &storage,
            trace_id.as_str(),
            key_id.as_str(),
            original_path.as_str(),
            path.as_str(),
            request_method.as_str(),
            &body,
            client_is_stream,
            response_adapter,
            gateway_mode_for_log.as_deref(),
            client_model_for_log.as_deref(),
            model_for_log.as_deref().unwrap_or_default(),
            model_source_for_log.as_deref(),
            client_reasoning_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            reasoning_source_for_log.as_deref(),
            service_tier_for_log.as_deref(),
            effective_service_tier_for_log.as_deref(),
            service_tier_source_for_log.as_deref(),
            started_at,
            kiro_route_source,
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
            service_tier_for_log.as_deref(),
            effective_service_tier_for_log.as_deref(),
            service_tier_source_for_log.as_deref(),
            gateway_mode_for_log.as_deref(),
            client_model_for_log.as_deref(),
            model_for_log.as_deref(),
            model_source_for_log.as_deref(),
            client_reasoning_for_log.as_deref(),
            reasoning_for_log.as_deref(),
            reasoning_source_for_log.as_deref(),
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
                    gateway_mode_for_log.as_deref(),
                    client_model_for_log.as_deref(),
                    model_for_log.as_deref(),
                    model_source_for_log.as_deref(),
                    client_reasoning_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    reasoning_source_for_log.as_deref(),
                    service_tier_for_log.as_deref(),
                    effective_service_tier_for_log.as_deref(),
                    service_tier_source_for_log.as_deref(),
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
                    service_tier_for_log.as_deref(),
                    effective_service_tier_for_log.as_deref(),
                    service_tier_source_for_log.as_deref(),
                    gateway_mode_for_log.as_deref(),
                    client_model_for_log.as_deref(),
                    model_for_log.as_deref(),
                    model_source_for_log.as_deref(),
                    client_reasoning_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    reasoning_source_for_log.as_deref(),
                    started_at,
                    err,
                );
            }
        }
    }

    let mut prepared_hybrid_aggregate_candidates = None;
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
        low_quota_candidate_mode_for_protocol(protocol_type.as_str()),
        respond_when_account_candidates_empty(execution_plan),
    ) {
        CandidatePrecheckResult::Ready {
            request,
            candidates,
        } => (request, candidates),
        CandidatePrecheckResult::Empty { request } => {
            match take_or_resolve_aggregate_candidates(
                &mut prepared_hybrid_aggregate_candidates,
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
                        gateway_mode_for_log.as_deref(),
                        client_model_for_log.as_deref(),
                        model_for_log.as_deref(),
                        model_source_for_log.as_deref(),
                        client_reasoning_for_log.as_deref(),
                        reasoning_for_log.as_deref(),
                        reasoning_source_for_log.as_deref(),
                        service_tier_for_log.as_deref(),
                        effective_service_tier_for_log.as_deref(),
                        service_tier_source_for_log.as_deref(),
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
                        service_tier_for_log.as_deref(),
                        effective_service_tier_for_log.as_deref(),
                        service_tier_source_for_log.as_deref(),
                        gateway_mode_for_log.as_deref(),
                        client_model_for_log.as_deref(),
                        model_for_log.as_deref(),
                        model_source_for_log.as_deref(),
                        client_reasoning_for_log.as_deref(),
                        reasoning_for_log.as_deref(),
                        reasoning_source_for_log.as_deref(),
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
        &storage,
        path.as_str(),
        protocol_type.as_str(),
        has_prompt_cache_key,
        &incoming_headers,
        &body,
        &mut candidates,
        key_id.as_str(),
        platform_key_hash.as_str(),
        route_conversation_id.as_deref(),
        route_conversation_source
            .unwrap_or(super::super::conversation_binding::RouteConversationSource::StickyFallback),
        conversation_binding.as_ref(),
        model_for_log.as_deref(),
        trace_id.as_str(),
    );
    let base = setup.upstream_base.as_str();
    if should_fallback_to_aggregate_after_account_exhaustion(execution_plan) {
        prepared_hybrid_aggregate_candidates =
            Some(resolve_hybrid_aggregate_candidates_for_prepare(
                &storage,
                protocol_type.as_str(),
                aggregate_api_id.as_deref(),
                key_id.as_str(),
                model_for_log.as_deref(),
                trace_id.as_str(),
            ));
        if let Some(Err(err)) = prepared_hybrid_aggregate_candidates.as_ref() {
            log::debug!(
                "event=gateway_hybrid_aggregate_candidate_prepare_deferred trace_id={} err={}",
                trace_id,
                err
            );
        }
    }

    let context = GatewayUpstreamExecutionContext::new(
        &trace_id,
        &storage,
        &key_id,
        &original_path,
        &path,
        &request_method,
        response_adapter,
        protocol_type.as_str(),
        client_model_for_log.as_deref(),
        model_for_log.as_deref(),
        model_source_for_log.as_deref(),
        client_reasoning_for_log.as_deref(),
        reasoning_for_log.as_deref(),
        reasoning_source_for_log.as_deref(),
        service_tier_for_log.as_deref(),
        effective_service_tier_for_log.as_deref(),
        service_tier_source_for_log.as_deref(),
        gateway_mode_for_log.as_deref(),
        Some(setup.route_strategy_for_log),
        Some(setup.route_source_for_log),
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
        route_conversation_id
            .as_deref()
            .unwrap_or(trace_id.as_str()),
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
            *request,
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
        match take_or_resolve_aggregate_candidates(
            &mut prepared_hybrid_aggregate_candidates,
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
                    gateway_mode_for_log.as_deref(),
                    client_model_for_log.as_deref(),
                    model_for_log.as_deref(),
                    model_source_for_log.as_deref(),
                    client_reasoning_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    reasoning_source_for_log.as_deref(),
                    service_tier_for_log.as_deref(),
                    effective_service_tier_for_log.as_deref(),
                    service_tier_source_for_log.as_deref(),
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
                    service_tier_for_log.as_deref(),
                    effective_service_tier_for_log.as_deref(),
                    service_tier_source_for_log.as_deref(),
                    gateway_mode_for_log.as_deref(),
                    client_model_for_log.as_deref(),
                    model_for_log.as_deref(),
                    model_source_for_log.as_deref(),
                    client_reasoning_for_log.as_deref(),
                    reasoning_for_log.as_deref(),
                    reasoning_source_for_log.as_deref(),
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
#[path = "proxy_tests.rs"]
mod tests;
