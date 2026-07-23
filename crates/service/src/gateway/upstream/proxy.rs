use crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE;
use crate::gateway::request_log::RequestLogUsage;
use codexmanager_core::storage::ManagedModelV2;
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
    configured_model: Option<&ManagedModelV2>,
) -> bool {
    match execution_plan.route_kind {
        GatewayUpstreamRouteKind::AggregateApi => true,
        // 若模型没有配置本地池就直接请求聚合api
        GatewayUpstreamRouteKind::HybridAccountFirst => {
            configured_model.is_some_and(|model: &ManagedModelV2| {
                has_enabled_aggregate_api_route(model)
                    && !has_enabled_default_account_pool_route(model)
            })
        }
        GatewayUpstreamRouteKind::AccountRotation => false,
    }
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
    configured_model: Option<&ManagedModelV2>,
) -> bool {
    !should_fallback_to_aggregate_after_account_exhaustion(execution_plan, configured_model)
}

fn should_fallback_to_aggregate_after_account_exhaustion(
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
    configured_model: Option<&ManagedModelV2>,
) -> bool {
    is_hybrid_account_first_route(execution_plan)
        && configured_model.is_none_or(has_enabled_aggregate_api_route)
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

fn has_enabled_default_account_pool_route(model: &ManagedModelV2) -> bool {
    model.routes.iter().any(|route| {
        route.enabled && route.source_kind == "account_pool" && route.source_id == "default"
    })
}

fn has_enabled_aggregate_api_route(model: &ManagedModelV2) -> bool {
    model
        .routes
        .iter()
        .any(|route| route.enabled && route.source_kind == "aggregate_api")
}

fn validate_model_route(
    storage: &codexmanager_core::storage::Storage,
    key_id: &str,
    model: Option<&str>,
    execution_plan: super::executor::GatewayUpstreamExecutionPlan,
) -> Result<Option<ManagedModelV2>, (u16, String)> {
    let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let managed_model: Option<ManagedModelV2> = storage
        .get_enabled_model_v2(model)
        .map_err(|err| (500, format!("model_catalog_v2_read_failed: {err}")))?;
    let Some(managed_model) = managed_model else {
        return Err((404, format!("model_not_found: {model}")));
    };
    let model_group_access = crate::resolve_api_key_model_group_access(storage, key_id, model)
        .map_err(|err| {
            if err.contains("model_not_allowed") {
                (403, err)
            } else {
                (500, err)
            }
        })?;
    if crate::distribution_enabled_for_storage(storage)
        && managed_model.price.price_status == "missing"
    {
        return Err((402, format!("model_price_missing: {model}")));
    }
    if let Err(err) = crate::auth::app_manager::wallet_precheck_for_api_key_rate(
        storage,
        key_id,
        model_group_access.map(|access| access.rate_multiplier_millis),
    ) {
        if err.contains("余额不足") {
            return Err((402, "额度不足，请联系管理员".to_string()));
        }
        return Err((403, err));
    }
    let route_enabled = match execution_plan.route_kind {
        GatewayUpstreamRouteKind::AccountRotation => {
            has_enabled_default_account_pool_route(&managed_model)
        }
        GatewayUpstreamRouteKind::AggregateApi => has_enabled_aggregate_api_route(&managed_model),
        GatewayUpstreamRouteKind::HybridAccountFirst => {
            has_enabled_default_account_pool_route(&managed_model)
                || has_enabled_aggregate_api_route(&managed_model)
        }
    };
    if !route_enabled {
        return Err((503, format!("model_unavailable: {model}")));
    }
    Ok(Some(managed_model))
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
    Ok(candidate.filter(|api| api.status.trim().eq_ignore_ascii_case("active")))
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
    let managed_model = storage
        .get_enabled_model_v2(model)
        .map_err(|err| format!("read aggregate model routes V2 failed: {err}"))?
        .ok_or_else(|| format!("model_not_found: {model}"))?;
    let mut routes = std::collections::HashMap::new();
    for route in managed_model
        .routes
        .into_iter()
        .filter(|route| route.enabled && route.source_kind == "aggregate_api")
    {
        let replace = routes.get(&route.source_id).is_none_or(
            |current: &codexmanager_core::storage::ModelRouteV2| route.priority > current.priority,
        );
        if replace {
            routes.insert(route.source_id.clone(), route);
        }
    }
    candidates.retain_mut(|api| {
        let Some(route) = routes.get(&api.id) else {
            return false;
        };
        api.model_override = Some(route.upstream_model.clone());
        true
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
        body,
        is_stream,
        has_prompt_cache_key,
        request_shape,
        protocol_type,
        rotation_strategy,
        aggregate_api_id,
        account_group_filter,
        account_plan_filter,
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
        model_for_log,
        model_source_for_log,
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

    let configured_model = match validate_model_route(
        &storage,
        key_id.as_str(),
        model_for_log.as_deref(),
        execution_plan,
    ) {
        Ok(configured_model) => configured_model,
        Err((status_code, message)) => {
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
    };

    if should_try_provider_executor_aggregate_route(execution_plan, configured_model.as_ref()) {
        let (aggregate_path, aggregate_body) = if is_hybrid_account_first_route(execution_plan) {
            (passthrough_path.as_str(), &passthrough_body)
        } else {
            (path.as_str(), &body)
        };
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
                    aggregate_path,
                    request_method.as_str(),
                    &method,
                    aggregate_body,
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
                    aggregate_path,
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
        account_group_filter.as_deref(),
        account_plan_filter.as_deref(),
        low_quota_candidate_mode_for_protocol(protocol_type.as_str()),
        respond_when_account_candidates_empty(execution_plan, configured_model.as_ref()),
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
    if should_fallback_to_aggregate_after_account_exhaustion(
        execution_plan,
        configured_model.as_ref(),
    ) {
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
        super::super::request_log::estimate_input_tokens_from_body(body.as_ref()),
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
    if should_fallback_to_aggregate_after_account_exhaustion(
        execution_plan,
        configured_model.as_ref(),
    ) {
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
