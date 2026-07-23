use super::{
    exhausted_gateway_error_for_log, has_enabled_aggregate_api_route,
    has_enabled_default_account_pool_route, hybrid_route_error_message, provider_upstream_hint,
    request_deadline_for_path, resolve_aggregate_candidates_for_route, resolve_upstream_is_stream,
    respond_when_account_candidates_empty, should_fallback_to_aggregate_after_account_exhaustion,
    should_try_provider_executor_aggregate_route, validate_model_route,
};
use crate::gateway::upstream::executor::{
    GatewayUpstreamExecutionPlan, GatewayUpstreamExecutorKind, GatewayUpstreamRouteKind,
};
use codexmanager_core::storage::{
    now_ts, Account, AggregateApi, ManagedModelV2, ManagedModelV2Upsert, ModelRouteV2, Storage,
};
use std::time::{Duration, Instant};

fn execution_plan(route_kind: GatewayUpstreamRouteKind) -> GatewayUpstreamExecutionPlan {
    GatewayUpstreamExecutionPlan {
        executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
        route_kind,
    }
}

fn insert_test_aggregate_api(storage: &Storage, id: &str) {
    insert_test_aggregate_api_with_provider(storage, id, "codex");
}

fn insert_test_aggregate_api_with_provider(storage: &Storage, id: &str, provider_type: &str) {
    let now = now_ts();
    storage
        .insert_aggregate_api(&AggregateApi {
            id: id.to_string(),
            provider_type: provider_type.to_string(),
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

fn insert_test_aggregate_api_with_model_override(storage: &Storage, id: &str, model: &str) {
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
            model_override: Some(model.to_string()),
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
    if storage
        .get_managed_model_v2(slug)
        .expect("read V2 model")
        .is_some()
    {
        return;
    }
    let mut model = storage
        .get_managed_model_v2("gpt-5.4-mini")
        .expect("read template model")
        .expect("template model");
    model.id.clear();
    model.slug = slug.to_string();
    model.display_name = slug.to_string();
    model.origin = "custom".to_string();
    model.builtin_revision = None;
    model.user_edited = false;
    model.routes.clear();
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .expect("seed V2 platform catalog");
}

fn add_model_route_v2(
    storage: &Storage,
    slug: &str,
    source_kind: &str,
    source_id: &str,
    upstream_model: &str,
) {
    seed_platform_catalog(storage, slug);
    let mut model = storage
        .get_managed_model_v2(slug)
        .expect("read V2 model")
        .expect("V2 model");
    model.routes.push(ModelRouteV2 {
        source_kind: source_kind.to_string(),
        source_id: source_id.to_string(),
        upstream_model: upstream_model.to_string(),
        enabled: true,
        weight: 1,
        ..Default::default()
    });
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some(slug.to_string()),
            model,
        })
        .expect("save V2 model route");
}

fn model_with_routes(routes: &[(&str, &str)]) -> ManagedModelV2 {
    ManagedModelV2 {
        routes: routes
            .iter()
            .map(|(source_kind, source_id)| ModelRouteV2 {
                source_kind: (*source_kind).to_string(),
                source_id: (*source_id).to_string(),
                upstream_model: "test-upstream".to_string(),
                enabled: true,
                weight: 1,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn aggregate_route_model_validation_accepts_model_override_candidate() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    insert_test_aggregate_api_with_model_override(&storage, "agg-minimax", "MiniMax-M3");
    add_model_route_v2(
        &storage,
        "gpt-5.4",
        "aggregate_api",
        "agg-minimax",
        "MiniMax-M3",
    );

    validate_model_route(
        &storage,
        "key-route",
        Some("gpt-5.4"),
        execution_plan(GatewayUpstreamRouteKind::AggregateApi),
    )
    .expect("aggregate override should make the route usable for client models");
}

#[test]
fn aggregate_candidate_filter_keeps_model_override_candidate_for_client_model() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    insert_test_aggregate_api_with_model_override(&storage, "agg-minimax", "MiniMax-M3");
    add_model_route_v2(
        &storage,
        "gpt-5.4",
        "aggregate_api",
        "agg-minimax",
        "MiniMax-M3",
    );

    let candidates =
        resolve_aggregate_candidates_for_route(&storage, "openai_responses", None, Some("gpt-5.4"))
            .expect("resolve aggregate candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "agg-minimax");
    assert_eq!(candidates[0].model_override.as_deref(), Some("MiniMax-M3"));
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
    let deadline =
        request_deadline_for_path(started_at, false, "/v1/responses").expect("responses deadline");
    let timeout = deadline
        .checked_duration_since(started_at)
        .expect("deadline should be after start");

    crate::gateway::set_upstream_total_timeout_ms(previous_total);
    crate::gateway::set_upstream_stream_timeout_ms(previous_stream);

    assert!(timeout > Duration::from_secs(250));
    assert!(timeout <= Duration::from_secs(300));
}

#[test]
fn aggregate_strategy_and_hybrid_aggregate_only_use_aggregate_candidates_directly() {
    assert!(should_try_provider_executor_aggregate_route(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::Claude,
            route_kind: GatewayUpstreamRouteKind::AggregateApi,
        },
        None,
    ));
    assert!(should_try_provider_executor_aggregate_route(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::Gemini,
            route_kind: GatewayUpstreamRouteKind::AggregateApi,
        },
        None,
    ));
    assert!(!should_try_provider_executor_aggregate_route(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::Claude,
            route_kind: GatewayUpstreamRouteKind::AccountRotation,
        },
        None,
    ));
    assert!(should_try_provider_executor_aggregate_route(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::AggregateApi,
        },
        None,
    ));
    assert!(!should_try_provider_executor_aggregate_route(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::Gemini,
            route_kind: GatewayUpstreamRouteKind::AccountRotation,
        },
        None,
    ));
    assert!(!should_try_provider_executor_aggregate_route(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::AccountRotation,
        },
        None,
    ));

    let hybrid = execution_plan(GatewayUpstreamRouteKind::HybridAccountFirst);
    let aggregate_only = model_with_routes(&[("aggregate_api", "agg-test")]);
    let dual_route =
        model_with_routes(&[("account_pool", "default"), ("aggregate_api", "agg-test")]);
    assert!(should_try_provider_executor_aggregate_route(
        hybrid,
        Some(&aggregate_only),
    ));
    assert!(!should_try_provider_executor_aggregate_route(
        hybrid,
        Some(&dual_route),
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

    assert!(!respond_when_account_candidates_empty(hybrid, None));
    assert!(respond_when_account_candidates_empty(account_only, None));
    assert!(respond_when_account_candidates_empty(aggregate_only, None));
}

#[test]
fn only_hybrid_falls_back_to_aggregate_after_account_exhaustion() {
    assert!(should_fallback_to_aggregate_after_account_exhaustion(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::HybridAccountFirst,
        },
        None,
    ));
    assert!(!should_fallback_to_aggregate_after_account_exhaustion(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::AccountRotation,
        },
        None,
    ));
    assert!(!should_fallback_to_aggregate_after_account_exhaustion(
        GatewayUpstreamExecutionPlan {
            executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
            route_kind: GatewayUpstreamRouteKind::AggregateApi,
        },
        None,
    ));
}

#[test]
fn hybrid_account_only_does_not_fallback_to_aggregate() {
    let hybrid = GatewayUpstreamExecutionPlan {
        executor_kind: GatewayUpstreamExecutorKind::CodexResponses,
        route_kind: GatewayUpstreamRouteKind::HybridAccountFirst,
    };
    let account_only = model_with_routes(&[("account_pool", "default")]);

    assert!(respond_when_account_candidates_empty(
        hybrid,
        Some(&account_only),
    ));
    assert!(!should_fallback_to_aggregate_after_account_exhaustion(
        hybrid,
        Some(&account_only),
    ));
}

#[test]
fn hybrid_dual_route_keeps_account_first_and_aggregate_fallback() {
    let hybrid = execution_plan(GatewayUpstreamRouteKind::HybridAccountFirst);
    let dual_route =
        model_with_routes(&[("account_pool", "default"), ("aggregate_api", "agg-test")]);

    assert!(!should_try_provider_executor_aggregate_route(
        hybrid,
        Some(&dual_route),
    ));
    assert!(!respond_when_account_candidates_empty(
        hybrid,
        Some(&dual_route),
    ));
    assert!(should_fallback_to_aggregate_after_account_exhaustion(
        hybrid,
        Some(&dual_route),
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
fn aggregate_route_model_validation_uses_explicit_v2_route() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    insert_test_aggregate_api(&storage, "agg-route");
    add_model_route_v2(
        &storage,
        "vendor-route",
        "aggregate_api",
        "agg-route",
        "vendor-upstream",
    );

    validate_model_route(
        &storage,
        "key-route",
        Some("vendor-route"),
        execution_plan(GatewayUpstreamRouteKind::AggregateApi),
    )
    .expect("explicit aggregate V2 route should be usable");
}

#[test]
fn aggregate_route_model_filter_uses_v2_routes() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    insert_test_aggregate_api(&storage, "agg-with-model");
    insert_test_aggregate_api(&storage, "agg-without-model");
    add_model_route_v2(
        &storage,
        "vendor-batched",
        "aggregate_api",
        "agg-with-model",
        "vendor-top",
    );

    let candidates = resolve_aggregate_candidates_for_route(
        &storage,
        "openai_responses",
        None,
        Some("vendor-batched"),
    )
    .expect("resolve aggregate candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, "agg-with-model");
    assert_eq!(candidates[0].model_override.as_deref(), Some("vendor-top"));
}

#[test]
fn explicit_aggregate_route_candidate_precedes_provider_candidates() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    insert_test_aggregate_api_with_provider(&storage, "agg-codex-explicit", "codex");
    insert_test_aggregate_api_with_provider(&storage, "agg-claude-explicit", "claude");
    add_model_route_v2(
        &storage,
        "vendor-cross-provider",
        "aggregate_api",
        "agg-codex-explicit",
        "vendor-codex",
    );
    add_model_route_v2(
        &storage,
        "vendor-cross-provider",
        "aggregate_api",
        "agg-claude-explicit",
        "vendor-claude",
    );

    let openai_candidates = resolve_aggregate_candidates_for_route(
        &storage,
        "openai_responses",
        Some("agg-claude-explicit"),
        Some("vendor-cross-provider"),
    )
    .expect("resolve openai candidates with explicit claude aggregate");
    let openai_candidate_ids = openai_candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        openai_candidate_ids,
        vec!["agg-claude-explicit", "agg-codex-explicit"]
    );
    assert_eq!(
        openai_candidates[0].model_override.as_deref(),
        Some("vendor-claude")
    );

    let anthropic_candidates = resolve_aggregate_candidates_for_route(
        &storage,
        "anthropic_native",
        Some("agg-codex-explicit"),
        Some("vendor-cross-provider"),
    )
    .expect("resolve anthropic candidates with explicit codex aggregate");
    let anthropic_candidate_ids = anthropic_candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        anthropic_candidate_ids,
        vec!["agg-codex-explicit", "agg-claude-explicit"]
    );
    assert_eq!(
        anthropic_candidates[0].model_override.as_deref(),
        Some("vendor-codex")
    );
}

#[test]
fn account_route_model_validation_ignores_aggregate_only_mapping() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    seed_platform_catalog(&storage, "vendor-account-route");
    add_model_route_v2(
        &storage,
        "vendor-account-route",
        "aggregate_api",
        "agg-only",
        "vendor-account-route",
    );

    let err = validate_model_route(
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
    add_model_route_v2(
        &storage,
        "vendor-hybrid",
        "aggregate_api",
        "agg-hybrid",
        "vendor-hybrid-upstream",
    );

    let model = validate_model_route(
        &storage,
        "key-route",
        Some("vendor-hybrid"),
        execution_plan(GatewayUpstreamRouteKind::HybridAccountFirst),
    )
    .expect("hybrid route should accept aggregate mapping")
    .expect("configured model");

    assert!(!has_enabled_default_account_pool_route(&model));
    assert!(has_enabled_aggregate_api_route(&model));
}

#[test]
fn hybrid_model_validation_returns_dual_route_configuration() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    add_model_route_v2(
        &storage,
        "vendor-hybrid-dual",
        "account_pool",
        "default",
        "vendor-hybrid-dual",
    );
    add_model_route_v2(
        &storage,
        "vendor-hybrid-dual",
        "aggregate_api",
        "agg-hybrid-dual",
        "vendor-hybrid-upstream",
    );

    let model = validate_model_route(
        &storage,
        "key-route",
        Some("vendor-hybrid-dual"),
        execution_plan(GatewayUpstreamRouteKind::HybridAccountFirst),
    )
    .expect("hybrid route should accept both route kinds")
    .expect("configured model");

    assert!(has_enabled_default_account_pool_route(&model));
    assert!(has_enabled_aggregate_api_route(&model));
}

#[test]
fn hybrid_model_validation_ignores_non_default_account_pool_routes() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    add_model_route_v2(
        &storage,
        "vendor-hybrid-non-default-pool",
        "account_pool",
        "secondary",
        "vendor-hybrid-non-default-pool",
    );
    add_model_route_v2(
        &storage,
        "vendor-hybrid-non-default-pool",
        "aggregate_api",
        "agg-hybrid-non-default-pool",
        "vendor-hybrid-upstream",
    );

    let model = validate_model_route(
        &storage,
        "key-route",
        Some("vendor-hybrid-non-default-pool"),
        execution_plan(GatewayUpstreamRouteKind::HybridAccountFirst),
    )
    .expect("hybrid route should accept its aggregate route")
    .expect("configured model");

    assert!(!has_enabled_default_account_pool_route(&model));
    assert!(has_enabled_aggregate_api_route(&model));
}

#[test]
fn model_route_validation_ignores_disabled_routes() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    add_model_route_v2(
        &storage,
        "vendor-disabled-route",
        "aggregate_api",
        "agg-disabled",
        "vendor-disabled-upstream",
    );
    let mut model = storage
        .get_managed_model_v2("vendor-disabled-route")
        .expect("read V2 model")
        .expect("V2 model");
    model.routes[0].enabled = false;
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some("vendor-disabled-route".to_string()),
            model,
        })
        .expect("disable V2 model route");

    let error = validate_model_route(
        &storage,
        "key-route",
        Some("vendor-disabled-route"),
        execution_plan(GatewayUpstreamRouteKind::HybridAccountFirst),
    )
    .expect_err("disabled routes should be unavailable");

    assert_eq!(error.0, 503);
    assert!(error.1.contains("model_unavailable"));
}

#[test]
fn model_route_validation_preserves_missing_model_default_paths() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    for route_kind in [
        GatewayUpstreamRouteKind::AccountRotation,
        GatewayUpstreamRouteKind::AggregateApi,
        GatewayUpstreamRouteKind::HybridAccountFirst,
    ] {
        let configured_model =
            validate_model_route(&storage, "key-route", None, execution_plan(route_kind))
                .expect("requests without a model should keep the strategy default path");
        assert!(configured_model.is_none());
    }
}

#[test]
fn account_route_model_validation_accepts_direct_upstream_source_model() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-direct-route".to_string(),
            label: "acc-direct-route".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .upsert_discovered_model_source_models(
            "openai_account",
            "acc-direct-route",
            &["gpt-5.4-mini".to_string()],
            "manual",
        )
        .expect("seed direct upstream source model");

    let model = validate_model_route(
        &storage,
        "key-route",
        Some("gpt-5.4-mini"),
        execution_plan(GatewayUpstreamRouteKind::AccountRotation),
    )
    .expect("account route should accept direct upstream source model")
    .expect("configured model");

    assert!(has_enabled_default_account_pool_route(&model));
    assert!(!has_enabled_aggregate_api_route(&model));
}
