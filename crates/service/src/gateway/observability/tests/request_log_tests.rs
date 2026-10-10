use codexmanager_core::storage::{ApiKey, Storage};

fn test_api_key(id: &str) -> ApiKey {
    ApiKey {
        id: id.to_string(),
        name: Some("last used test key".to_string()),
        model_slug: Some("gpt-6-sol".to_string()),
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: "account_rotation".to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: "codex".to_string(),
        protocol_type: "openai_compat".to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("hash-{id}"),
        status: "active".to_string(),
        created_at: 1,
        last_used_at: None,
    }
}

#[test]
fn successful_request_log_touches_key_and_records_v2_snapshot() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let key = test_api_key("key-last-used-success");
    storage.insert_api_key(&key).expect("insert api key");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-last-used-success"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            ..Default::default()
        },
        Some(&key.id),
        None,
        "/v1/responses",
        "POST",
        Some("gpt-6-sol"),
        None,
        Some("https://example.test/v1/responses"),
        Some(200),
        super::RequestLogUsage {
            input_tokens: Some(1),
            output_tokens: Some(1),
            total_tokens: Some(2),
            ..Default::default()
        },
        None,
        Some(10),
    );

    let loaded = storage
        .find_api_key_by_id(&key.id)
        .expect("load api key")
        .expect("api key exists");
    assert!(loaded.last_used_at.unwrap_or(0) > 0);
    let snapshot = storage
        .get_charge_snapshot_v2(1)
        .expect("read snapshot")
        .expect("snapshot");
    assert_eq!(snapshot.usage_source, "actual");
    assert_eq!(snapshot.input_tokens, 1);
    assert_eq!(snapshot.output_tokens, 1);
    assert_eq!(snapshot.base_cost_microusd, 12);
    let logs = storage
        .list_request_logs(None, 10)
        .expect("read request logs");
    assert_eq!(logs[0].estimated_cost_usd, Some(0.000_012));
}

#[test]
fn request_log_charges_gpt56_cache_writes_at_the_configured_rate() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-gpt56-cache-write"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            ..Default::default()
        },
        None,
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.6-sol"),
        None,
        Some("https://example.test/v1/responses"),
        Some(200),
        super::RequestLogUsage {
            input_tokens: Some(100),
            cached_input_tokens: Some(40),
            cache_write_tokens: Some(20),
            output_tokens: Some(10),
            total_tokens: Some(110),
            ..Default::default()
        },
        None,
        Some(10),
    );

    let snapshot = storage
        .get_charge_snapshot_v2(1)
        .expect("read snapshot")
        .expect("snapshot");
    assert_eq!(snapshot.cached_input_tokens, 40);
    assert_eq!(snapshot.cache_write_tokens, 20);
    assert_eq!(snapshot.cache_write_microusd_per_1m, 6_250_000);
    assert_eq!(snapshot.base_cost_microusd, 645);
}

#[test]
fn missing_usage_uses_deterministic_nonzero_input_estimate() {
    let estimate = super::estimate_input_tokens_from_body(br#"{"input":"hello world"}"#);
    assert!(estimate > 0);
    assert_eq!(
        estimate,
        super::estimate_input_tokens_from_body(br#"{"input":"hello world"}"#)
    );
    let usage = super::resolve_charge_usage(super::RequestLogUsage {
        estimated_input_tokens: Some(estimate),
        ..Default::default()
    });
    assert_eq!(usage.usage_source, "estimated");
    assert_eq!(usage.input_tokens, estimate);
    assert_eq!(usage.output_tokens, 0);
}

#[test]
fn actual_usage_clamps_cache_read_then_cache_write_to_total_input() {
    let usage = super::resolve_charge_usage(super::RequestLogUsage {
        input_tokens: Some(10),
        cached_input_tokens: Some(8),
        cache_write_tokens: Some(20),
        output_tokens: Some(3),
        ..Default::default()
    });
    assert_eq!(usage.usage_source, "actual");
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.cached_input_tokens, 8);
    assert_eq!(usage.cache_write_tokens, 2);
    assert_eq!(usage.output_tokens, 3);
}

#[test]
fn request_log_persists_client_ultra_and_effective_max_separately() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-ultra-max"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            request_type: Some("http"),
            client_model: Some("gpt-5.6-sol"),
            client_reasoning_effort: Some("ultra"),
            ..Default::default()
        },
        None,
        None,
        "/v1/responses",
        "POST",
        Some("gpt-5.6-sol"),
        Some("max"),
        None,
        Some(200),
        super::RequestLogUsage::default(),
        None,
        Some(1),
    );

    let logs = storage
        .list_request_logs(None, 10)
        .expect("read request logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].client_reasoning_effort.as_deref(), Some("ultra"));
    assert_eq!(logs[0].reasoning_effort.as_deref(), Some("max"));
    assert_eq!(
        logs[0].reasoning_source.as_deref(),
        Some("client_request_normalized")
    );
}
