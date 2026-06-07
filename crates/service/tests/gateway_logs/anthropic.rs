use super::*;
use codexmanager_core::storage::AggregateApi;

/// 函数 `gateway_claude_protocol_rewrites_messages_path_with_sticky_prompt_cache_key`
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
fn gateway_claude_protocol_rewrites_messages_path_with_sticky_prompt_cache_key() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-claude-sticky-thread-anchor");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_response = serde_json::json!({
        "id": "resp_claude_sticky_anchor",
        "model": "gpt-5.4-mini",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": {
            "input_tokens": 20,
            "output_tokens": 4,
            "total_tokens": 24
        }
    });
    let ok_body = serde_json::to_string(&upstream_response).expect("serialize upstream response");
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence(vec![(200, ok_body.clone()), (200, ok_body)]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    seed_model_catalog_models(&storage, &["gpt-5.4-mini"]);

    storage
        .insert_account(&Account {
            id: "acc_claude_sticky_anchor".to_string(),
            label: "claude-sticky-anchor".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_claude_sticky_anchor".to_string()),
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
            account_id: "acc_claude_sticky_anchor".to_string(),
            id_token: String::new(),
            access_token: "access_token_claude_sticky_anchor".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_claude_sticky_anchor".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_claude_sticky_anchor";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_claude_sticky_anchor".to_string(),
            name: Some("claude-sticky-anchor".to_string()),
            model_slug: Some("gpt-5.4-mini".to_string()),
            reasoning_effort: Some("high".to_string()),
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    for user_id in ["ephemeral-user-1", "ephemeral-user-2"] {
        let server = codexmanager_service::start_one_shot_server().expect("start server");
        let body = serde_json::json!({
            "model": "gpt-5.4-mini",
            "messages": [{ "role": "user", "content": "hello" }],
            "metadata": { "user_id": user_id },
            "stream": false
        });
        let body = serde_json::to_string(&body).expect("serialize request");
        let (status, response_body) = post_http_raw(
            &server.addr,
            "/v1/messages?beta=true",
            &body,
            &[
                ("Content-Type", "application/json"),
                ("x-api-key", platform_key),
                ("anthropic-version", "2023-06-01"),
            ],
        );
        assert_eq!(status, 200, "gateway response: {response_body}");
        server.join();
    }

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive second upstream request");
    upstream_join.join().expect("join upstream");

    let first_body = decode_upstream_request_body(&first);
    let second_body = decode_upstream_request_body(&second);
    let first_payload: serde_json::Value =
        serde_json::from_slice(&first_body).expect("parse first upstream payload");
    let second_payload: serde_json::Value =
        serde_json::from_slice(&second_body).expect("parse second upstream payload");

    assert_eq!(first.path, "/backend-api/codex/responses");
    assert_eq!(second.path, "/backend-api/codex/responses");
    let first_prompt_cache_key = first_payload
        .get("prompt_cache_key")
        .and_then(serde_json::Value::as_str)
        .expect("first prompt_cache_key");
    let second_prompt_cache_key = second_payload
        .get("prompt_cache_key")
        .and_then(serde_json::Value::as_str)
        .expect("second prompt_cache_key");
    assert!(!first_prompt_cache_key.trim().is_empty());
    assert_eq!(first_prompt_cache_key, second_prompt_cache_key);
}

#[test]
fn gateway_aggregate_messages_passthrough_accepts_message_stop_for_non_claude_provider() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-aggregate-message-stop");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let anthropic_sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-3-5-sonnet-20241022\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                anthropic_sse.to_string(),
                "text/event-stream".to_string(),
            )],
            Duration::from_secs(3),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    seed_model_catalog_models(&storage, &["claude-3-5-sonnet-20241022"]);
    let now = now_ts();
    let aggregate_id = "agg_non_claude_messages_sse";
    storage
        .insert_aggregate_api(&AggregateApi {
            id: aggregate_id.to_string(),
            provider_type: "codex".to_string(),
            supplier_name: Some("non-claude-anthropic-compatible".to_string()),
            sort: 0,
            url: format!("http://{upstream_addr}"),
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
    storage
        .upsert_aggregate_api_secret(aggregate_id, "upstream-secret")
        .expect("insert aggregate secret");
    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "aggregate_api".to_string(),
            source_id: aggregate_id.to_string(),
            upstream_model: "claude-3-5-sonnet-20241022".to_string(),
            display_name: Some("Claude 3.5 Sonnet".to_string()),
            status: "available".to_string(),
            discovery_kind: "manual".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert aggregate source model");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "mapping_non_claude_messages_sse".to_string(),
            platform_model_slug: "claude-3-5-sonnet-20241022".to_string(),
            source_kind: "aggregate_api".to_string(),
            source_id: aggregate_id.to_string(),
            upstream_model: "claude-3-5-sonnet-20241022".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("insert aggregate source mapping");

    let platform_key = "pk_non_claude_messages_sse";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_non_claude_messages_sse".to_string(),
            name: Some("non-claude-messages-sse".to_string()),
            model_slug: Some("claude-3-5-sonnet-20241022".to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "aggregate_api_rotation".to_string(),
            aggregate_api_id: Some(aggregate_id.to_string()),
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": true
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");
    assert!(response_body.contains("event: message_stop"));

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(captured.path, "/v1/messages");
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer upstream-secret")
    );
    assert_eq!(captured.headers.get("x-api-key").map(String::as_str), None);

    let mut matched = None;
    for _ in 0..40 {
        let logs = storage
            .list_request_logs(Some("key:=gk_non_claude_messages_sse"), 20)
            .expect("list request logs");
        matched = logs
            .into_iter()
            .find(|item| item.request_path == "/v1/messages");
        if matched.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let log = matched.expect("aggregate request log");
    assert_eq!(log.status_code, Some(200));
    assert_eq!(log.error.as_deref(), None);
    assert_eq!(log.response_adapter.as_deref(), Some("Passthrough"));
    assert_eq!(log.actual_source_kind.as_deref(), Some("aggregate_api"));
    assert_eq!(log.actual_source_id.as_deref(), Some(aggregate_id));
}

#[test]
fn gateway_aggregate_responses_bridge_adds_anthropic_headers_and_messages_path() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-responses-anthropic-bridge-headers");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let anthropic_response = serde_json::json!({
        "id": "msg_bridge_headers",
        "type": "message",
        "role": "assistant",
        "model": "claude-3-5-sonnet-20241022",
        "content": [{ "type": "text", "text": "ok" }],
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": { "input_tokens": 4, "output_tokens": 1 }
    });
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(
                200,
                serde_json::to_string(&anthropic_response).expect("serialize upstream response"),
                "application/json".to_string(),
            )],
            Duration::from_secs(3),
        );

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    seed_model_catalog_models(&storage, &["claude-3-5-sonnet-20241022"]);
    let now = now_ts();
    let aggregate_id = "agg_claude_responses_bridge_headers";
    storage
        .insert_aggregate_api(&AggregateApi {
            id: aggregate_id.to_string(),
            provider_type: "claude".to_string(),
            supplier_name: Some("claude-anthropic-compatible".to_string()),
            sort: 0,
            url: format!("http://{upstream_addr}/v1"),
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
    storage
        .upsert_aggregate_api_secret(aggregate_id, "upstream-secret")
        .expect("insert aggregate secret");
    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "aggregate_api".to_string(),
            source_id: aggregate_id.to_string(),
            upstream_model: "claude-3-5-sonnet-20241022".to_string(),
            display_name: Some("Claude 3.5 Sonnet".to_string()),
            status: "available".to_string(),
            discovery_kind: "manual".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert aggregate source model");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "mapping_claude_responses_bridge_headers".to_string(),
            platform_model_slug: "claude-3-5-sonnet-20241022".to_string(),
            source_kind: "aggregate_api".to_string(),
            source_id: aggregate_id.to_string(),
            upstream_model: "claude-3-5-sonnet-20241022".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("insert aggregate source mapping");

    let platform_key = "pk_claude_responses_bridge_headers";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_claude_responses_bridge_headers".to_string(),
            name: Some("claude-responses-bridge-headers".to_string()),
            model_slug: Some("claude-3-5-sonnet-20241022".to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "aggregate_api_rotation".to_string(),
            aggregate_api_id: Some(aggregate_id.to_string()),
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "input": "hello",
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(captured.path, "/v1/messages");
    assert_eq!(
        captured.headers.get("authorization").map(String::as_str),
        Some("Bearer upstream-secret")
    );
    assert_eq!(
        captured.headers.get("x-api-key").map(String::as_str),
        Some("upstream-secret")
    );
    assert_eq!(
        captured
            .headers
            .get("anthropic-version")
            .map(String::as_str),
        Some("2023-06-01")
    );

    let upstream_body = decode_upstream_request_body(&captured);
    let upstream_payload: serde_json::Value =
        serde_json::from_slice(&upstream_body).expect("parse upstream payload");
    assert_eq!(upstream_payload["messages"][0]["role"], "user");
}

/// 函数 `gateway_claude_messages_stay_on_chatgpt_codex_base`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-04
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn gateway_claude_messages_stay_on_chatgpt_codex_base() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-claude-chatgpt-base");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_response = serde_json::json!({
        "id": "resp_claude_chatgpt_base",
        "model": "gpt-5.4-mini",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": {
            "input_tokens": 20,
            "output_tokens": 4,
            "total_tokens": 24
        }
    });
    let ok_body = serde_json::to_string(&upstream_response).expect("serialize upstream response");
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_once(&ok_body);
    let upstream_base = format!("http://{upstream_addr}/chatgpt.com/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    seed_model_catalog_models(&storage, &["gpt-5.4-mini"]);

    storage
        .insert_account(&Account {
            id: "acc_claude_chatgpt_base".to_string(),
            label: "claude-chatgpt-base".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_claude_chatgpt_base".to_string()),
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
            account_id: "acc_claude_chatgpt_base".to_string(),
            id_token: String::new(),
            access_token: "access_token_claude_chatgpt_base".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_claude_chatgpt_base".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_claude_chatgpt_base";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_claude_chatgpt_base".to_string(),
            name: Some("claude-chatgpt-base".to_string()),
            model_slug: Some("gpt-5.4-mini".to_string()),
            reasoning_effort: Some("high".to_string()),
            service_tier: Some("fast".to_string()),
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "gpt-5.4-mini",
        "messages": [{ "role": "user", "content": "hello" }],
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/messages?beta=true",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");
    assert_eq!(captured.path, "/chatgpt.com/backend-api/codex/responses");
    let upstream_body =
        String::from_utf8(decode_upstream_request_body(&captured)).expect("upstream body utf8");
    assert!(
        upstream_body.contains("\"service_tier\":\"priority\""),
        "unexpected upstream body: {upstream_body}"
    );
}

/// 函数 `gateway_claude_protocol_end_to_end_uses_codex_headers`
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
fn gateway_claude_protocol_end_to_end_uses_codex_headers() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-claude-e2e");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_response = serde_json::json!({
        "id": "resp_test_1",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "pong" }]
        }],
        "usage": {
            "input_tokens": 12,
            "cache_read_input_tokens": 9,
            "output_tokens": 6,
            "total_tokens": 18
        }
    });
    let upstream_response =
        serde_json::to_string(&upstream_response).expect("serialize upstream response");
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_once(&upstream_response);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    seed_model_catalog_models(&storage, &["claude-3-5-sonnet-20241022", "gpt-5.3-codex"]);

    storage
        .insert_account(&Account {
            id: "acc_claude_e2e".to_string(),
            label: "claude-e2e".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_acc_test".to_string()),
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
            account_id: "acc_claude_e2e".to_string(),
            id_token: String::new(),
            access_token: "access_token_fallback".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_test".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_claude_e2e";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_claude_e2e".to_string(),
            name: Some("claude-e2e".to_string()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "messages": [
            { "role": "user", "content": "你好" }
        ],
        "max_tokens": 64,
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, gateway_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
            ("x-stainless-lang", "js"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {gateway_body}");

    let value: serde_json::Value =
        serde_json::from_str(&gateway_body).expect("parse anthropic compatibility response");
    assert_eq!(value["id"], "resp_test_1");
    assert_eq!(value["type"], "message");
    assert_eq!(value["content"][0]["type"], "text");
    assert_eq!(value["content"][0]["text"], "pong");
    assert_eq!(value["stop_reason"], "end_turn");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");

    assert_eq!(captured.path, "/backend-api/codex/responses");
    let authorization = captured
        .headers
        .get("authorization")
        .expect("authorization header");
    assert!(authorization.starts_with("Bearer "));
    assert!(!authorization.contains(platform_key));
    assert_eq!(
        captured.headers.get("accept").map(String::as_str),
        Some("text/event-stream")
    );
    assert!(
        captured.headers.get("user-agent").is_some_and(
            |value| value.contains(codexmanager_service::default_gateway_user_agent_version())
        ),
        "user-agent should carry codex client version"
    );
    assert_eq!(
        captured
            .headers
            .get("anthropic-version")
            .map(String::as_str),
        None
    );
    assert_eq!(
        captured.headers.get("x-stainless-lang").map(String::as_str),
        None
    );

    let upstream_payload: serde_json::Value =
        serde_json::from_slice(&captured.body).expect("parse upstream payload");
    assert_eq!(upstream_payload["model"], "claude-3-5-sonnet-20241022");
    assert_eq!(upstream_payload["stream"], true);
    assert_eq!(upstream_payload["input"][0]["type"], "message");
    assert_eq!(upstream_payload["input"][0]["role"], "user");
    assert_eq!(
        upstream_payload["input"][0]["content"][0]["type"],
        "input_text"
    );
    assert_eq!(upstream_payload["input"][0]["content"][0]["text"], "你好");

    let mut matched = None;
    for _ in 0..40 {
        let logs = storage
            .list_request_logs(Some("key:=gk_claude_e2e"), 20)
            .expect("list request logs");
        matched = logs
            .into_iter()
            .find(|item| item.request_path == "/v1/responses" && item.status_code == Some(200));
        if matched.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let log = matched.expect("claude e2e request log");
    assert!(!log.trace_id.as_deref().unwrap_or("").is_empty());
    assert_eq!(log.original_path.as_deref(), Some("/v1/messages"));
    assert_eq!(log.adapted_path.as_deref(), Some("/v1/responses"));
    assert_eq!(
        log.response_adapter.as_deref(),
        Some("AnthropicMessagesFromResponses")
    );
}

/// 函数 `gateway_claude_failover_cross_workspace_strips_session_affinity_headers`
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
fn gateway_claude_failover_cross_workspace_strips_session_affinity_headers() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-claude-strip-cross-workspace");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "not found for this account",
            "type": "invalid_request_error"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_strip_cross_workspace_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 8, "output_tokens": 4, "total_tokens": 12 }
    });
    let err_body = serde_json::to_string(&first_response).expect("serialize first response");
    let ok_body = serde_json::to_string(&second_response).expect("serialize second response");
    // A 404 can trigger alternate-path + stateless retries before failover. Force those retries to
    // also 404 so the gateway actually fails over to wsB.
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body),
        (200, ok_body),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    seed_model_catalog_models(&storage, &["claude-3-5-sonnet-20241022", "gpt-5.3-codex"]);

    storage
        .insert_account(&Account {
            id: "acc_ws_a".to_string(),
            label: "ws-a".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("wsA".to_string()),
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account wsA");
    storage
        .insert_token(&Token {
            account_id: "acc_ws_a".to_string(),
            id_token: String::new(),
            access_token: "access_token_ws_a".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_ws_a".to_string()),
            last_refresh: now,
        })
        .expect("insert token wsA");

    storage
        .insert_account(&Account {
            id: "acc_ws_b".to_string(),
            label: "ws-b".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("wsB".to_string()),
            group_name: None,
            sort: 2,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account wsB");
    storage
        .insert_token(&Token {
            account_id: "acc_ws_b".to_string(),
            id_token: String::new(),
            access_token: "access_token_ws_b".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_ws_b".to_string()),
            last_refresh: now,
        })
        .expect("insert token wsB");

    let platform_key = "pk_strip_cross_workspace";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_strip_cross_workspace".to_string(),
            name: Some("strip-cross-workspace".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [{ "role": "user", "content": "hello" }],
        "metadata": { "user_id": "user_strip_cross_workspace" },
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
            ("x-stainless-lang", "js"),
            ("x-codex-turn-state", "turn_state_cross_ws"),
            ("conversation_id", "conv_cross_ws"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let mut captured = Vec::new();
    for idx in 0..5 {
        captured.push(
            upstream_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap_or_else(|_| panic!("receive upstream request {idx}")),
        );
    }
    upstream_join.join().expect("join upstream");
    let captured_debug = format!("{captured:#?}");

    let ws_a_stateful = captured
        .iter()
        .find(|req| {
            req.headers
                .get("authorization")
                .map(|v| v.contains("access_token_ws_a"))
                .unwrap_or(false)
                && req.headers.contains_key("x-codex-turn-state")
        })
        .unwrap_or_else(|| panic!("expected wsA stateful upstream request: {captured_debug}"));
    let ws_b = captured
        .iter()
        .find(|req| {
            req.headers
                .get("authorization")
                .map(|v| v.contains("access_token_ws_b"))
                .unwrap_or(false)
        })
        .expect("expected wsB upstream request");

    assert_eq!(
        ws_a_stateful
            .headers
            .get("x-codex-turn-state")
            .map(String::as_str),
        Some("turn_state_cross_ws")
    );
    assert_eq!(
        ws_a_stateful
            .headers
            .get("conversation_id")
            .map(String::as_str),
        None
    );
    assert!(
        ws_a_stateful
            .headers
            .get("authorization")
            .map(|v| v.contains("access_token_ws_a"))
            .unwrap_or(false),
        "wsA upstream authorization missing expected bearer token"
    );

    assert!(!ws_b.headers.contains_key("x-codex-turn-state"));
    assert!(!ws_b.headers.contains_key("conversation_id"));
    assert!(
        ws_b.headers
            .get("authorization")
            .map(|v| v.contains("access_token_ws_b"))
            .unwrap_or(false),
        "wsB upstream authorization missing expected bearer token"
    );
}

/// 函数 `gateway_claude_failover_same_workspace_preserves_session_affinity_headers`
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
fn gateway_claude_failover_same_workspace_preserves_session_affinity_headers() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-claude-strip-same-workspace");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "not found for this account",
            "type": "invalid_request_error"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_strip_same_workspace_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 8, "output_tokens": 4, "total_tokens": 12 }
    });
    let err_body = serde_json::to_string(&first_response).expect("serialize first response");
    let ok_body = serde_json::to_string(&second_response).expect("serialize second response");
    // A 404 can trigger alternate-path + stateless retries before failover. Force those retries to
    // also 404 so the gateway actually fails over to the 2nd account (same workspace scope).
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body.clone()),
        (404, err_body),
        (200, ok_body),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    seed_model_catalog_models(&storage, &["claude-3-5-sonnet-20241022", "gpt-5.3-codex"]);

    for index in 1..=2 {
        storage
            .insert_account(&Account {
                id: format!("acc_ws_same_{index}"),
                label: format!("ws-same-{index}"),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: Some("wsSame".to_string()),
                group_name: None,
                sort: index,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account wsSame");
        storage
            .insert_token(&Token {
                account_id: format!("acc_ws_same_{index}"),
                id_token: String::new(),
                access_token: format!("access_token_ws_same_{index}"),
                refresh_token: String::new(),
                api_key_access_token: Some(format!("api_access_token_ws_same_{index}")),
                last_refresh: now,
            })
            .expect("insert token wsSame");
    }

    let platform_key = "pk_strip_same_workspace";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_strip_same_workspace".to_string(),
            name: Some("strip-same-workspace".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: Some("high".to_string()),
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "anthropic_native".to_string(),
            auth_scheme: "x_api_key".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [{ "role": "user", "content": "hello" }],
        "metadata": { "user_id": "user_strip_same_workspace" },
        "stream": false
    });
    let body = serde_json::to_string(&body).expect("serialize request");
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/messages",
        &body,
        &[
            ("Content-Type", "application/json"),
            ("x-api-key", platform_key),
            ("anthropic-version", "2023-06-01"),
            ("x-stainless-lang", "js"),
            ("x-codex-turn-state", "turn_state_same_ws"),
            ("conversation_id", "conv_same_ws"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let mut captured = Vec::new();
    for idx in 0..5 {
        captured.push(
            upstream_rx
                .recv_timeout(Duration::from_secs(2))
                .unwrap_or_else(|_| panic!("receive upstream request {idx}")),
        );
    }
    upstream_join.join().expect("join upstream");
    let captured_debug = format!("{captured:#?}");

    let account_2 = captured
        .iter()
        .find(|req| {
            req.headers
                .get("authorization")
                .map(|v| v.contains("access_token_ws_same_2"))
                .unwrap_or(false)
        })
        .expect("expected upstream request for account 2");

    assert_eq!(
        account_2
            .headers
            .get("x-codex-turn-state")
            .map(String::as_str),
        Some("turn_state_same_ws"),
        "captured upstream requests: {captured_debug}"
    );
    assert_eq!(
        account_2.headers.get("conversation_id").map(String::as_str),
        None
    );
}
