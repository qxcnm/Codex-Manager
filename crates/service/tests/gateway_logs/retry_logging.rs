use super::*;

/// 函数 `gateway_stateless_retry_strips_encrypted_content_on_invalid_encrypted_content`
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
fn gateway_stateless_retry_strips_encrypted_content_on_invalid_encrypted_content() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-strip-encrypted-content-on-400");
    let db_path: PathBuf = dir.join("codexmanager.db");

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "The encrypted content gAAA_test could not be verified. Reason: Encrypted content organization_id did not match the target organization.",
            "type": "invalid_request_error",
            "code": "invalid_encrypted_content"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_retry_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 3, "output_tokens": 2, "total_tokens": 5 }
    });
    let err_body = serde_json::to_string(&first_response).expect("serialize first response");
    let ok_body = serde_json::to_string(&second_response).expect("serialize second response");
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence(vec![(400, err_body), (200, ok_body)]);

    let upstream_base = format!("http://{upstream_addr}/v1");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    seed_model_catalog_models(&storage, &["claude-3-5-sonnet-20241022", "gpt-5.3-codex"]);

    storage
        .insert_account(&Account {
            id: "acc_retry_encrypted_content".to_string(),
            label: "retry".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: None,
            workspace_id: Some("ws_retry".to_string()),
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_retry_encrypted_content".to_string(),
            id_token: String::new(),
            access_token: "access_token_retry".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_retry".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_retry_strip_encrypted_content";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_retry_strip_encrypted_content".to_string(),
            name: Some("retry-strip-encrypted-content".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
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
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req_body = r#"{"model":"gpt-5.3-codex","input":"hello","stream":false,"encrypted_content":"gAAA_test_payload"}"#;
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("x-codex-turn-state", "gAAA_dummy_turn_state_blob"),
            ("Conversation_id", "conv_dummy"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive second upstream request");
    upstream_join.join().expect("join upstream");

    assert_eq!(
        first.headers.get("x-codex-turn-state").map(String::as_str),
        Some("gAAA_dummy_turn_state_blob")
    );
    assert!(!first.headers.contains_key("conversation_id"));
    let first_body: serde_json::Value =
        serde_json::from_slice(&first.body).expect("parse first request body");
    assert!(
        first_body.get("encrypted_content").is_some(),
        "default passthrough should preserve encrypted_content before stateless retry"
    );

    assert!(!second.headers.contains_key("x-codex-turn-state"));
    assert!(!second.headers.contains_key("conversation_id"));
    let second_body: serde_json::Value =
        serde_json::from_slice(&second.body).expect("parse second request body");
    assert!(
        second_body.get("encrypted_content").is_none(),
        "stateless retry must drop org-scoped encrypted_content field"
    );
    let trace_id = storage
        .list_request_logs(Some("key:=gk_retry_strip_encrypted_content"), 10)
        .expect("list request logs")
        .into_iter()
        .find_map(|log| log.trace_id)
        .expect("retry trace id");
    let attempts = wait_for_upstream_attempts(&storage, &trace_id, 2);
    assert_eq!(attempts.len(), 2);
    for (index, (attempt, mock_body)) in
        attempts.iter().zip([&first.body, &second.body]).enumerate()
    {
        assert_eq!(
            attempt.wire_sha256,
            format!("{:x}", Sha256::digest(mock_body)),
            "attempt {} wire bytes",
            index + 1
        );
        let payload_stage = if attempt.identical_to_client {
            codexmanager_core::storage::PAYLOAD_STAGE_CLIENT
        } else {
            attempt.stage.as_str()
        };
        let captured = storage
            .find_request_log_payload_by_trace_id(&trace_id, payload_stage)
            .expect("read retry payload")
            .expect("retry payload persisted");
        let actual = serde_json::from_str::<serde_json::Value>(&captured.payload).unwrap();
        let expected = serde_json::from_slice::<serde_json::Value>(mock_body).unwrap();
        assert_eq!(
            actual,
            expected,
            "attempt {} body differs from mock upstream",
            index + 1
        );
    }
}

/// 函数 `gateway_request_log_keeps_only_final_result_for_multi_attempt_flow`
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
fn gateway_request_log_keeps_only_final_result_for_multi_attempt_flow() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-final-log");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let trace_log_path: PathBuf = dir.join("gateway-trace.log");
    let _ = fs::remove_file(&trace_log_path);

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let first_response = serde_json::json!({
        "error": {
            "message": "not found for this account",
            "type": "invalid_request_error"
        }
    });
    let second_response = serde_json::json!({
        "id": "resp_final_ok",
        "model": "gpt-5.3-codex",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": { "input_tokens": 8, "output_tokens": 4, "total_tokens": 12 }
    });
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (
            404,
            serde_json::to_string(&first_response).expect("serialize first response"),
        ),
        (
            200,
            serde_json::to_string(&second_response).expect("serialize second response"),
        ),
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
                id: format!("acc_final_{index}"),
                label: format!("final-{index}"),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some(format!("chatgpt_acc_final_{index}")),
                workspace_id: None,
                group_name: None,
                sort: index,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: format!("acc_final_{index}"),
                id_token: String::new(),
                access_token: format!("access_token_{index}"),
                refresh_token: String::new(),
                api_key_access_token: Some(format!("api_access_token_{index}")),
                last_refresh: now,
            })
            .expect("insert token");
    }

    let platform_key = "pk_final_result_only";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_final_result_only".to_string(),
            name: Some("final-result-only".to_string()),
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
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let _ = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive first upstream request");
    let _ = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive second upstream request");
    upstream_join.join().expect("join upstream");

    let logs = storage
        .list_request_logs(Some("key:gk_final_result_only"), 20)
        .expect("list logs");
    let final_logs = logs
        .iter()
        .filter(|item| {
            item.request_path == "/v1/responses"
                && item.method == "POST"
                && item.key_id.as_deref() == Some("gk_final_result_only")
        })
        .collect::<Vec<_>>();
    assert_eq!(final_logs.len(), 1, "logs: {final_logs:#?}");
    assert_eq!(final_logs[0].status_code, Some(200));
    assert!(!final_logs[0].trace_id.as_deref().unwrap_or("").is_empty());
    assert_eq!(final_logs[0].original_path.as_deref(), Some("/v1/messages"));
    assert_eq!(final_logs[0].adapted_path.as_deref(), Some("/v1/responses"));
    assert_eq!(
        final_logs[0].response_adapter.as_deref(),
        Some("AnthropicMessagesFromResponses")
    );

    assert!(
        !trace_log_path.exists(),
        "successful retried request should not leave gateway trace log"
    );
}

/// 函数 `gateway_error_logging_writes_only_trace_log_file`
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
fn gateway_error_logging_writes_only_trace_log_file() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-single-log-file");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let trace_log_path: PathBuf = dir.join("gateway-trace.log");
    let error_log_path: PathBuf = dir.join("gateway-error.txt");
    let _ = fs::remove_file(&trace_log_path);
    let _ = fs::remove_file(&error_log_path);

    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let upstream_error = serde_json::json!({
        "error": {
            "message": "upstream unavailable",
            "type": "server_error"
        }
    });
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![(
        502,
        serde_json::to_string(&upstream_error).expect("serialize upstream error"),
    )]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    seed_model_catalog_models(
        &storage,
        &["claude-3-5-sonnet-20241022", "gpt-5.3-codex", "gpt-5.4"],
    );

    storage
        .insert_account(&Account {
            id: "acc_single_log_error".to_string(),
            label: "single-log-error".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_acc_single_log_error".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_single_log_error".to_string(),
            id_token: String::new(),
            access_token: "access_token_single_log_error".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_token_single_log_error".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_single_log_error";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_single_log_error".to_string(),
            name: Some("single-log-error".to_string()),
            model_slug: Some("gpt-5.4".to_string()),
            reasoning_effort: Some("high".to_string()),
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
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    let req_body =
        serde_json::json!({ "model": "gpt-5.4", "input": "hello", "stream": false }).to_string();
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert_eq!(status, 502, "gateway response: {response_body}");

    let _ = upstream_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("receive upstream request");
    upstream_join.join().expect("join upstream");

    assert!(
        trace_log_path.exists(),
        "failed request should leave gateway trace log"
    );
    assert!(
        !error_log_path.exists(),
        "failed request should not leave standalone gateway error log"
    );

    let trace_content = fs::read_to_string(&trace_log_path).expect("read gateway trace log");
    assert!(trace_content.contains("event=REQUEST_START"));
    assert!(trace_content.contains("event=CANDIDATE_POOL"));
    assert!(trace_content.contains("event=CANDIDATE_START"));
    assert!(trace_content.contains("event=ATTEMPT_PROFILE"));
    assert!(trace_content.contains("event=FAILED_REQUEST"));
    assert!(!trace_content.contains("event=REQUEST_FINAL"));
    assert!(!trace_content.contains("event=REQUEST_RECORD"));
    assert!(trace_content.contains("event=ATTEMPT_RESULT"));
    assert!(trace_content.contains("event=BRIDGE_RESULT"));
    assert!(trace_content.contains("trace_id="));
    assert!(trace_content.contains("request_path=/v1/responses"));
    assert!(trace_content.contains("upstream_url="));
    assert!(trace_content.contains("status=502"));
}
