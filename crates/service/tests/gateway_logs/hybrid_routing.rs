use super::*;
use codexmanager_core::storage::AggregateApi;

const MODEL: &str = "gpt-hybrid-route-test";
const UPSTREAM_MODEL: &str = "gpt-hybrid-route-upstream";

fn response_json(id: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "id": id,
        "model": MODEL,
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 2, "output_tokens": 1, "total_tokens": 3 }
    }))
    .expect("serialize upstream response")
}

fn insert_active_account(storage: &Storage, account_id: &str, now: i64) {
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: account_id.to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some(format!("chatgpt_{account_id}")),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: String::new(),
            access_token: format!("access_{account_id}"),
            refresh_token: String::new(),
            api_key_access_token: Some(format!("api_access_{account_id}")),
            last_refresh: now,
        })
        .expect("insert token");
}

fn insert_aggregate_api(storage: &Storage, aggregate_id: &str, addr: &str, action: &str, now: i64) {
    storage
        .insert_aggregate_api(&AggregateApi {
            id: aggregate_id.to_string(),
            provider_type: "codex".to_string(),
            supplier_name: Some("hybrid route test".to_string()),
            sort: 0,
            url: format!("http://{addr}/backend-api/codex"),
            auth_type: "apikey".to_string(),
            auth_params_json: None,
            action: Some(action.to_string()),
            model_override: None,
            user_agent: None,
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
        .expect("insert aggregate API");
    storage
        .upsert_aggregate_api_secret(aggregate_id, "aggregate-secret")
        .expect("insert aggregate API secret");
}

fn replace_with_aggregate_only_route(storage: &Storage, aggregate_id: &str) {
    seed_model_catalog_models(storage, &[MODEL]);
    let mut model = storage
        .get_managed_model_v2(MODEL)
        .expect("get V2 model")
        .expect("V2 model exists");
    model.routes = vec![ModelRouteV2 {
        id: String::new(),
        source_kind: "aggregate_api".to_string(),
        source_id: aggregate_id.to_string(),
        upstream_model: UPSTREAM_MODEL.to_string(),
        enabled: true,
        priority: 0,
        weight: 1,
    }];
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .expect("replace V2 model routes");
}

fn seed_dual_routes(storage: &Storage, aggregate_id: &str) {
    seed_model_catalog_models(storage, &[MODEL]);
    seed_model_catalog_route(
        storage,
        MODEL,
        "aggregate_api",
        aggregate_id,
        UPSTREAM_MODEL,
        0,
    );
}

fn insert_hybrid_key(storage: &Storage, key_id: &str, platform_key: &str, now: i64) {
    storage
        .insert_api_key(&ApiKey {
            id: key_id.to_string(),
            name: Some(key_id.to_string()),
            model_slug: Some(MODEL.to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "hybrid_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert hybrid API key");
}

fn insert_hybrid_aggregate_first_key(
    storage: &Storage,
    key_id: &str,
    platform_key: &str,
    now: i64,
) {
    storage
        .insert_api_key(&ApiKey {
            id: key_id.to_string(),
            name: Some(key_id.to_string()),
            model_slug: Some(MODEL.to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "hybrid_aggregate_first_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert aggregate-first hybrid API key");
}

#[test]
fn rejected_root_array_request_stores_only_redacted_client_payload() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-rejected-root-array-redaction");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let key_id = "gk_rejected_array";
    let platform_key = "pk_rejected_array";
    insert_hybrid_key(&storage, key_id, platform_key, now_ts());
    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let (status, _) = post_http_raw(
        &server.addr,
        "/v1/responses",
        r#"[{"api_key":"sk-dummy-secret","nested":{"password":"dummy-password"}}]"#,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert!(status >= 400, "root array must fail gateway validation");
    let trace_id = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list rejected request logs")
        .into_iter()
        .find_map(|log| log.trace_id)
        .expect("rejected trace id");
    let deadline = Instant::now() + Duration::from_secs(5);
    let captured = loop {
        if let Some(captured) = storage
            .find_request_log_payload_by_trace_id(
                &trace_id,
                codexmanager_core::storage::PAYLOAD_STAGE_CLIENT,
            )
            .expect("read captured client body")
        {
            break captured;
        }
        assert!(
            Instant::now() < deadline,
            "rejected request capture timed out"
        );
        thread::sleep(Duration::from_millis(20));
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&captured.payload).expect("captured root array");
    assert_eq!(parsed[0]["api_key"], "[REDACTED]");
    assert_eq!(parsed[0]["nested"]["password"], "[REDACTED]");
    assert!(!captured.payload.contains("sk-dummy-secret"));
    assert!(!captured.payload.contains("dummy-password"));
}

#[test]
fn hybrid_aggregate_first_aggregate_only_skips_active_account_and_uses_aggregate_api() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-only");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_aggregate_only"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_only";
    let key_id = "gk_hybrid_aggregate_only";
    let platform_key = "pk_hybrid_aggregate_only";
    insert_active_account(&storage, "acc_hybrid_aggregate_only", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    replace_with_aggregate_only_route(&storage, aggregate_id);
    insert_hybrid_aggregate_first_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_aggregate_only"));
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "local account must be skipped"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 1, "aggregate API request count");
    assert_eq!(aggregate_requests[0].path, "/backend-api/codex/responses");
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);

    let log = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .find(|item| item.request_path == "/v1/responses")
        .expect("request log");
    assert_eq!(log.status_code, Some(200));
    assert_eq!(log.actual_source_kind.as_deref(), Some("aggregate_api"));
    assert_eq!(log.actual_source_id.as_deref(), Some(aggregate_id));
}

#[test]
fn hybrid_aggregate_only_streaming_responses_forces_identity_and_logs_success() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-stream-identity");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"aggregate ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_aggregate_identity\",\"model\":\"gpt-hybrid-route-upstream\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":7,\"input_tokens_details\":{\"cached_tokens\":2},\"output_tokens\":5,\"output_tokens_details\":{\"reasoning_tokens\":1},\"total_tokens\":12}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                aggregate_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_stream_identity";
    let key_id = "gk_hybrid_stream_identity";
    let platform_key = "pk_hybrid_stream_identity";
    insert_active_account(&storage, "acc_hybrid_stream_identity", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    replace_with_aggregate_only_route(&storage, aggregate_id);
    insert_hybrid_aggregate_first_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": true
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("Accept-Encoding", "gzip, br, zstd"),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("aggregate ok"));
    assert!(response_body.contains("resp_aggregate_identity"));
    assert!(response_body.contains("response.completed"));
    assert!(response_body.contains("data: [DONE]"));
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "local account must be skipped"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 1, "aggregate API request count");
    assert_eq!(
        aggregate_requests[0]
            .headers
            .get("accept-encoding")
            .map(String::as_str),
        Some("identity")
    );
    assert_eq!(
        aggregate_requests[0]
            .headers
            .get("accept")
            .map(String::as_str),
        Some("text/event-stream")
    );
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);
    assert_eq!(aggregate_body["stream"], true);

    let logs = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .filter(|item| item.request_path == "/v1/responses")
        .collect::<Vec<_>>();
    assert_eq!(logs.len(), 1, "stream must persist one final request log");
    let log = &logs[0];
    assert_eq!(log.status_code, Some(200));
    assert_eq!(log.error, None);
    assert_eq!(log.input_tokens, Some(7));
    assert_eq!(log.cached_input_tokens, Some(2));
    assert_eq!(log.output_tokens, Some(5));
    assert_eq!(log.total_tokens, Some(12));
    assert_eq!(log.reasoning_output_tokens, Some(1));
    assert_eq!(log.actual_source_kind.as_deref(), Some("aggregate_api"));
    assert_eq!(log.actual_source_id.as_deref(), Some(aggregate_id));
}

#[test]
fn hybrid_aggregate_first_aggregate_only_failure_never_uses_account_pool() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-aggregate-only-failure");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_must_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_failure = r#"{"error":{"message":"aggregate unavailable"}}"#.to_string();
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![
            (503, aggregate_failure.clone()),
            (503, aggregate_failure.clone()),
            (503, aggregate_failure.clone()),
            (503, aggregate_failure),
        ],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_only_failure";
    let key_id = "gk_hybrid_aggregate_first_only_failure";
    let platform_key = "pk_hybrid_aggregate_first_only_failure";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_only_failure", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    replace_with_aggregate_only_route(&storage, aggregate_id);
    insert_hybrid_aggregate_first_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 502, "gateway response: {response_body}");
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "aggregate-only model must not bypass its route and use an account"
    );
    assert_eq!(aggregate_rx.try_iter().count(), 4, "aggregate retry count");
}

#[test]
fn hybrid_aggregate_first_dual_route_falls_back_once_and_preserves_tools() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-dual-fallback");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_account_fallback"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_failure = r#"{"error":{"message":"aggregate unavailable"}}"#.to_string();
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![
            (503, aggregate_failure.clone()),
            (503, aggregate_failure.clone()),
            (503, aggregate_failure.clone()),
            (503, aggregate_failure),
        ],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_dual_fallback";
    let key_id = "gk_hybrid_aggregate_first_dual_fallback";
    let platform_key = "pk_hybrid_aggregate_first_dual_fallback";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_dual_fallback", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_aggregate_first_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "use the tool",
        "stream": false,
        "tools": [{
            "type": "function",
            "name": "get_answer",
            "description": "Return an answer",
            "parameters": {
                "type": "object",
                "properties": { "question": { "type": "string" } }
            }
        }]
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_account_fallback"));
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 4, "aggregate retry count");
    let local_requests = local_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(local_requests.len(), 1, "account fallback request count");
    let local_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&local_requests[0]))
            .expect("parse account fallback body");
    assert_eq!(local_body["tools"][0]["name"], "get_answer");

    let logs = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .filter(|item| item.request_path == "/v1/responses")
        .collect::<Vec<_>>();
    assert_eq!(logs.len(), 1, "fallback must persist one final request log");
    assert_eq!(logs[0].status_code, Some(200));
    assert_eq!(
        logs[0].actual_source_kind.as_deref(),
        Some("openai_account")
    );
    let trace_id = logs[0].trace_id.as_deref().expect("non-stream trace id");
    let attempts = wait_for_upstream_attempts(&storage, trace_id, 5);
    for (attempt, captured) in attempts
        .iter()
        .zip(aggregate_requests.iter().chain(local_requests.iter()))
    {
        assert_eq!(
            attempt.wire_sha256,
            format!("{:x}", Sha256::digest(&captured.body))
        );
    }
}

#[test]
fn hybrid_aggregate_first_dual_route_streams_from_account_after_aggregate_failure() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-first-stream-fallback");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let account_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"account fallback ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_hybrid_aggregate_first_stream\",\"model\":\"gpt-hybrid-route-test\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (local_addr, local_rx, local_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                account_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let aggregate_failure = r#"{"error":{"message":"aggregate unavailable"}}"#.to_string();
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![
            (503, aggregate_failure.clone()),
            (503, aggregate_failure.clone()),
            (503, aggregate_failure.clone()),
            (503, aggregate_failure),
        ],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_first_stream_fallback";
    let key_id = "gk_hybrid_aggregate_first_stream_fallback";
    let platform_key = "pk_hybrid_aggregate_first_stream_fallback";
    insert_active_account(&storage, "acc_hybrid_aggregate_first_stream_fallback", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_aggregate_first_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": true
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("account fallback ok"));
    assert!(response_body.contains("resp_hybrid_aggregate_first_stream"));
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 4, "aggregate retry count");
    let local_requests = local_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(local_requests.len(), 1, "account fallback request count");
    let local_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&local_requests[0]))
            .expect("parse account fallback body");
    assert_eq!(local_body["stream"], true);

    let logs = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .filter(|item| item.request_path == "/v1/responses")
        .collect::<Vec<_>>();
    assert_eq!(
        logs.len(),
        1,
        "stream fallback must persist one final request log"
    );
    assert_eq!(logs[0].status_code, Some(200));
    assert_eq!(
        logs[0].actual_source_kind.as_deref(),
        Some("openai_account")
    );
    let trace_id = logs[0].trace_id.as_deref().expect("stream trace id");
    let attempts = wait_for_upstream_attempts(&storage, trace_id, 5);
    assert_eq!(attempts.len(), 5);
    for (attempt, captured) in attempts
        .iter()
        .zip(aggregate_requests.iter().chain(local_requests.iter()))
    {
        assert_eq!(
            attempt.wire_sha256,
            format!("{:x}", Sha256::digest(&captured.body)),
            "each actual send (aggregate retries and account fallback) must match its mock upstream"
        );
    }
    assert_eq!(
        storage
            .find_request_log_trace_for_response_id(key_id, "resp_hybrid_aggregate_first_stream",)
            .expect("response association"),
        Some(trace_id.to_string()),
    );
}

#[test]
fn hybrid_aggregate_only_streams_chat_completions_tool_calls_from_aggregate_api() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-aggregate-only-chat-tools");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let tool_call_sse = concat!(
        "data: {\"id\":\"chatcmpl_hybrid_tool\",\"object\":\"chat.completion.chunk\",\"created\":1775900000,\"model\":\"gpt-hybrid-route-test\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_hybrid_1\",\"type\":\"function\",\"function\":{\"name\":\"get_answer\",\"arguments\":\"{\\\"question\\\":\\\"2+2\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl_hybrid_tool\",\"object\":\"chat.completion.chunk\",\"created\":1775900000,\"model\":\"gpt-hybrid-route-test\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let (aggregate_addr, aggregate_rx, aggregate_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                tool_call_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_aggregate_only_chat_tools";
    let key_id = "gk_hybrid_aggregate_only_chat_tools";
    let platform_key = "pk_hybrid_aggregate_only_chat_tools";
    insert_active_account(&storage, "acc_hybrid_aggregate_only_chat_tools", now);
    insert_aggregate_api(
        &storage,
        aggregate_id,
        &aggregate_addr,
        "/chat/completions",
        now,
    );
    replace_with_aggregate_only_route(&storage, aggregate_id);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "messages": [{ "role": "user", "content": "answer with a tool" }],
        "stream": true,
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_answer",
                "description": "Return an answer",
                "parameters": {
                    "type": "object",
                    "properties": { "question": { "type": "string" } },
                    "required": ["question"]
                }
            }
        }]
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/chat/completions",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "local account must be skipped"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 1, "aggregate API request count");
    assert_eq!(
        aggregate_requests[0].path,
        "/backend-api/codex/chat/completions"
    );
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);
    assert!(aggregate_body["messages"].is_array());
    assert!(aggregate_body["tools"].is_array());
    assert!(
        response_body.contains("\"tool_calls\""),
        "chat completions response: {response_body}"
    );
    assert!(response_body.contains("\"id\":\"call_hybrid_1\""));
    assert!(response_body.contains("\"name\":\"get_answer\""));
    assert!(response_body.contains("{\\\"question\\\":\\\"2+2\\\"}"));
    assert!(response_body.contains("\"finish_reason\":\"tool_calls\""));
    assert!(response_body.contains("data: [DONE]"));
    let trace_id = storage
        .list_request_logs(Some(&format!("key:={key_id}")), 10)
        .expect("list request logs")
        .into_iter()
        .find_map(|log| log.trace_id)
        .expect("aggregate request trace id");
    let attempts = wait_for_upstream_attempts(&storage, &trace_id, 1);
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].method, "POST");
    assert_eq!(attempts[0].transport, "http");
    assert!(attempts[0].url.ends_with("/chat/completions"));
    assert_eq!(
        attempts[0].wire_sha256,
        format!("{:x}", Sha256::digest(&aggregate_requests[0].body)),
        "captured wire digest must match bytes observed by mock aggregate upstream"
    );
    let captured = storage
        .find_request_log_payload_by_trace_id(&trace_id, &attempts[0].stage)
        .expect("read captured attempt")
        .expect("distinct upstream payload");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&captured.payload).unwrap(),
        aggregate_body,
        "decoded upstream attempt must match the mock's received body"
    );
}

#[test]
fn hybrid_dual_route_prefers_active_account_for_streaming_responses() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-dual-account-first");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let account_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"account ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_hybrid_account\",\"model\":\"gpt-hybrid-route-test\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (local_addr, local_rx, local_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                account_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(2),
        );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_aggregate_should_not_run"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_dual_account_first";
    let key_id = "gk_hybrid_dual_account_first";
    let platform_key = "pk_hybrid_dual_account_first";
    insert_active_account(&storage, "acc_hybrid_dual_account_first", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": true
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_hybrid_account"));
    let local_requests = local_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(local_requests.len(), 1, "local account request count");
    assert_eq!(local_requests[0].path, "/backend-api/codex/responses");
    let local_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&local_requests[0]))
            .expect("parse local account request body");
    assert_eq!(local_body["model"], MODEL);
    assert_eq!(
        aggregate_rx.try_iter().count(),
        0,
        "aggregate API must remain idle after account success"
    );
}

#[test]
fn hybrid_dual_route_falls_back_to_aggregate_when_account_pool_is_empty() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-dual-empty-account-pool");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_local_should_not_run"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_hybrid_empty_account_fallback"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_dual_empty_account_pool";
    let key_id = "gk_hybrid_dual_empty_account_pool";
    let platform_key = "pk_hybrid_dual_empty_account_pool";
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_dual_routes(&storage, aggregate_id);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "input": "hello",
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("resp_hybrid_empty_account_fallback"));
    assert_eq!(
        local_rx.try_iter().count(),
        0,
        "local account request count"
    );
    let aggregate_requests = aggregate_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(aggregate_requests.len(), 1, "aggregate API request count");
    assert_eq!(aggregate_requests[0].path, "/backend-api/codex/responses");
    let aggregate_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&aggregate_requests[0]))
            .expect("parse aggregate request body");
    assert_eq!(aggregate_body["model"], UPSTREAM_MODEL);
}

#[test]
fn hybrid_account_only_uses_account_and_ignores_unbound_aggregate_api() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-hybrid-account-only");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_hybrid_account_only"))],
        Duration::from_secs(2),
    );
    let local_base = format!("http://{local_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &local_base);
    let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_aggregate_should_not_run"))],
        Duration::from_secs(2),
    );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let aggregate_id = "agg_hybrid_account_only_unbound";
    let key_id = "gk_hybrid_account_only";
    let platform_key = "pk_hybrid_account_only";
    insert_active_account(&storage, "acc_hybrid_account_only", now);
    insert_aggregate_api(&storage, aggregate_id, &aggregate_addr, "/responses", now);
    seed_model_catalog_models(&storage, &[MODEL]);
    insert_hybrid_key(&storage, key_id, platform_key, now);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let request = serde_json::json!({
        "model": MODEL,
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": false
    });
    let request = serde_json::to_string(&request).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/chat/completions",
        &request,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    local_join.join().expect("join local upstream");
    aggregate_join.join().expect("join aggregate upstream");

    assert_eq!(status, 200, "gateway response: {response_body}");
    let response: serde_json::Value =
        serde_json::from_str(&response_body).expect("parse chat completions response");
    assert_eq!(response["id"], "resp_hybrid_account_only");
    assert_eq!(response["choices"][0]["message"]["content"], "ok");
    let local_requests = local_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(local_requests.len(), 1, "local account request count");
    assert_eq!(local_requests[0].path, "/backend-api/codex/responses");
    assert_eq!(
        aggregate_rx.try_iter().count(),
        0,
        "unbound aggregate API must remain idle"
    );
}
