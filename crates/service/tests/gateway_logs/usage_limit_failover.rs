use super::*;
use codexmanager_core::storage::UsageSnapshotRecord;

/// 当上游用 200 + SSE `data:` 正文夹带 "You've hit your usage limit" 回应时，
/// 网关在内容尚未提交给客户端前识别该失败，并自动切换到下一个健康账号：
/// - 客户端侧 HTTP status 保持 200，且只看到第二个账号的完整回答
/// - request_log 只保留最终成功结果，避免把内部切换暴露成客户端断线
///
/// 这条链路覆盖：PassthroughSseUsageReader 扫描 data 正文（Fix A）→
/// bridge.stream_terminal_error → response_finalize 的 failover 分支。
#[test]
fn gateway_usage_limit_in_sse_fails_over_before_downstream_commit() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-usage-limit-sse-failover");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let usage_limit_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"You've hit your usage limit. To get more access now, send a request to your admin or try again at 7:44 PM.\"}\n\n",
        "data: [DONE]\n\n"
    );
    let ok_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_failover_ok\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    );

    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![
                (
                    200,
                    usage_limit_sse.to_string(),
                    "text/event-stream".to_string(),
                ),
                (200, ok_sse.to_string(), "text/event-stream".to_string()),
            ],
            Duration::from_secs(3),
        );
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    seed_model_catalog_models(&storage, &["gpt-5.3-codex"]);
    let now = now_ts();

    // 两个候选账号都健康（10%），以保证 has_more_candidates=true 让
    // should_failover_for_gateway_error 返回 true，走 failover 标记分支。
    for (id, sort) in [("acc_primary", 0_i64), ("acc_secondary", 1_i64)] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some(format!("chatgpt_{id}")),
                workspace_id: None,
                group_name: None,
                sort,
                status: "active".to_string(),
                created_at: now + sort,
                updated_at: now + sort,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: id.to_string(),
                id_token: String::new(),
                access_token: format!("access_{id}"),
                refresh_token: String::new(),
                api_key_access_token: Some(format!("api_access_{id}")),
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: id.to_string(),
                used_percent: Some(10.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert snapshot");
    }

    let platform_key = "pk_usage_limit_failover_marker";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_usage_limit_failover_marker".to_string(),
            name: Some("usage-limit-failover-marker".to_string()),
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
    let req_body_json = serde_json::json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": true
    });
    let req_body = serde_json::to_string(&req_body_json).expect("serialize request");
    let (status, body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert_eq!(status, 200, "客户端应看到第二个账号的成功响应");
    assert!(body.contains("\"delta\":\"ok\""));
    assert!(!body.contains("You've hit your usage limit"));

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive first upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive failover upstream request");
    upstream_join.join().expect("join mock upstream");
    let first_auth = first
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        first_auth.contains("access_acc_primary"),
        "首次应命中 primary 账号，实际 auth 头：{first_auth}"
    );
    let second_auth = second
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        second_auth.contains("access_acc_secondary"),
        "额度失败后应切换 secondary 账号，实际 auth 头：{second_auth}"
    );

    // 等 request log 异步落盘。
    let mut log = None;
    for _ in 0..40 {
        let logs = storage
            .list_request_logs(Some("key:=gk_usage_limit_failover_marker"), 20)
            .expect("list request logs");
        log = logs
            .into_iter()
            .find(|item| item.request_path == "/v1/responses");
        if log.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let log = log.expect("request log should be recorded");
    assert_eq!(
        log.status_code,
        Some(200),
        "内部切换成功后客户端请求应记录最终成功，实际 {:?}",
        log.status_code
    );
    assert_eq!(
        log.account_id.as_deref(),
        Some("acc_secondary"),
        "最终请求日志应记录成功返回的 secondary 账号"
    );
}

/// Fix B 端到端：快要耗尽的账号（99% used）即使 sort 排前，也应被降权到候选尾部，
/// 首个请求直接命中健康账号，不必经历失败-重试流程。
#[test]
fn gateway_low_quota_account_is_skipped_on_first_request() {
    let _lock = test_env_guard();
    // Low-quota protection is intentionally opt-in in production. This test
    // exercises the enabled behavior, so make that precondition explicit.
    let _quota_guard = EnvGuard::set("CODEXMANAGER_QUOTA_GUARD_ENABLED", "1");
    let dir = new_test_dir("codexmanager-gateway-low-quota-skip");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let ok_sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_lowq_ok\",\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );

    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(200, ok_sse.to_string(), "text/event-stream".to_string())],
            Duration::from_secs(3),
        );
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    seed_model_catalog_models(&storage, &["gpt-5.3-codex"]);
    let now = now_ts();

    // sort=0 的账号快照 99%（快耗尽），sort=1 的健康（10%）。
    // Fix B 应把 exhausted 排到尾部，实际请求只打 healthy。
    let rows: Vec<(&str, i64, f64)> = vec![("acc_exhausted", 0, 99.0), ("acc_healthy", 1, 10.0)];
    for (id, sort, used_pct) in &rows {
        storage
            .insert_account(&Account {
                id: (*id).to_string(),
                label: (*id).to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some(format!("chatgpt_{id}")),
                workspace_id: None,
                group_name: None,
                sort: *sort,
                status: "active".to_string(),
                created_at: now + *sort,
                updated_at: now + *sort,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: (*id).to_string(),
                id_token: String::new(),
                access_token: format!("access_{id}"),
                refresh_token: String::new(),
                api_key_access_token: Some(format!("api_access_{id}")),
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: (*id).to_string(),
                used_percent: Some(*used_pct),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert snapshot");
    }

    let platform_key = "pk_low_quota_skip";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_low_quota_skip".to_string(),
            name: Some("low-quota-skip".to_string()),
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
    let request_body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "input": "hello",
        "stream": true
    });
    let request_body = serde_json::to_string(&request_body).expect("serialize request");
    let (status, gateway_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &request_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {gateway_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");

    let auth = captured
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        auth.contains("access_acc_healthy"),
        "即便 sort=0 的账号排在前，99% used 的账号也应该被降到尾部；实际 auth 头：{auth}"
    );
    assert!(
        upstream_rx
            .recv_timeout(Duration::from_millis(300))
            .is_err(),
        "低配额账号应被直接跳过，不应再有第二次上游请求"
    );
}
