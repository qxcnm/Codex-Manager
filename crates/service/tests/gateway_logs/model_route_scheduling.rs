use super::*;

fn route(source: &str, upstream: &str, priority: i64, weight: i64) -> ModelRouteV2 {
    ModelRouteV2 {
        source_kind: if source == "default" {
            "account_pool"
        } else {
            "aggregate_api"
        }
        .into(),
        source_id: source.into(),
        upstream_model: upstream.into(),
        enabled: true,
        priority,
        weight,
        ..Default::default()
    }
}

fn configure(storage: &Storage, policy: &str, routes: Vec<ModelRouteV2>) {
    seed_model_catalog_models(storage, &[MODEL]);
    let mut model = storage.get_managed_model_v2(MODEL).unwrap().unwrap();
    model.capabilities["route_policy"] = serde_json::json!(policy);
    model.routes = routes;
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .unwrap();
    assert_eq!(
        storage
            .get_managed_model_v2(MODEL)
            .unwrap()
            .unwrap()
            .capabilities["route_policy"],
        policy
    );
}

fn send(addr: &str, path: &str, stream: bool) -> (u16, String) {
    let mut body = serde_json::json!({"model":MODEL,"stream":stream,
        "tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}]});
    if path == "/v1/responses" {
        body["input"] = serde_json::json!("call lookup");
    } else {
        body["messages"] = serde_json::json!([{"role":"user","content":"call lookup"}]);
        body["tools"] = serde_json::json!([{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}}}]);
    }
    post_http_raw_with_read_timeout(
        addr,
        path,
        &body.to_string(),
        &[
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer pk_model_schedule"),
        ],
        Duration::from_secs(10),
    )
}

fn success(path: &str, stream: bool) -> (String, String) {
    let response = if path == "/v1/responses" {
        serde_json::json!({"id":"resp_scheduled","object":"response","status":"completed","model":MODEL,
            "output":[{"id":"fc_scheduled","type":"function_call","status":"completed","call_id":"call_lookup","name":"lookup","arguments":"{}"}],
            "usage":{"input_tokens":2,"output_tokens":1,"total_tokens":3}})
    } else {
        serde_json::json!({"id":"chat_scheduled","object":"chat.completion","model":MODEL,
            "choices":[{"index":0,"message":{"role":"assistant","tool_calls":[{"id":"call_lookup","type":"function","function":{"name":"lookup","arguments":"{}"}}]},"finish_reason":"tool_calls"}],
            "usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}})
    };
    if !stream {
        return (response.to_string(), "application/json".into());
    }
    if path == "/v1/responses" {
        (
            format!("data: {{\"type\":\"response.completed\",\"response\":{response}}}\n\n"),
            "text/event-stream".into(),
        )
    } else {
        let chunk = serde_json::json!({"id":"chat_scheduled","object":"chat.completion.chunk","model":MODEL,
            "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_lookup","type":"function","function":{"name":"lookup","arguments":"{}"}}]},"finish_reason":"tool_calls"}]});
        (
            format!("data: {chunk}\n\ndata: [DONE]\n\n"),
            "text/event-stream".into(),
        )
    }
}

#[test]
fn scheduled_priority_same_source_branches_and_tools_cover_both_endpoints_json_and_sse() {
    let _lock = test_env_guard();
    for path in ["/v1/responses", "/v1/chat/completions"] {
        for stream in [false, true] {
            let dir = new_test_dir("model-route-protocols");
            let db = dir.join("codexmanager.db");
            let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
            let (ok, content_type) = success(path, stream);
            let (addr, rx, join) = start_mock_upstream_sequence_lenient_with_content_types(
                vec![
                    (
                        429,
                        r#"{"error":{"message":"too many requests"}}"#.into(),
                        "application/json".into(),
                    ),
                    (
                        503,
                        r#"{"error":{"message":"unavailable"}}"#.into(),
                        "application/json".into(),
                    ),
                    (200, ok, content_type),
                ],
                Duration::from_secs(2),
            );
            let storage = Storage::open(&db).unwrap();
            insert_aggregate_api(&storage, "agg_a", &addr, path, now_ts());
            insert_aggregate_api(&storage, "agg_b", &addr, path, now_ts());
            configure(
                &storage,
                "weighted",
                vec![
                    route("agg_b", "last", -10, 1),
                    route("agg_a", "secondary", 0, 1),
                    route("agg_a", "primary", 10, 1),
                ],
            );
            insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
            let server = codexmanager_service::start_one_shot_server().unwrap();
            let (status, body) = send(&server.addr, path, stream);
            server.join();
            join.join().unwrap();
            assert_eq!(status, 200, "{path}, stream={stream}: {body}");
            assert!(body.contains("call_lookup"), "tool call survives: {body}");
            let requests = rx.try_iter().collect::<Vec<_>>();
            assert_eq!(
                requests.len(),
                3,
                "no per-channel retry hides branch fallback"
            );
            let models = requests
                .iter()
                .map(|request| {
                    let value: serde_json::Value =
                        serde_json::from_slice(&decode_upstream_request_body(request)).unwrap();
                    assert!(value["tools"]
                        .as_array()
                        .is_some_and(|tools| tools.len() == 1));
                    value["model"].as_str().unwrap().to_owned()
                })
                .collect::<Vec<_>>();
            assert_eq!(models, ["primary", "secondary", "last"]);
            let logs = storage
                .list_request_logs(Some("key:=gk_model_schedule"), 10)
                .unwrap();
            assert_eq!(logs.len(), 1, "one terminal accounting record");
            assert_eq!(logs[0].actual_source_id.as_deref(), Some("agg_b"));
            assert_eq!(logs[0].upstream_model.as_deref(), Some("last"));
        }
    }
}

#[test]
fn scheduled_fatal_400_is_terminal_and_shared_transport_budget_is_not_reset() {
    let _lock = test_env_guard();
    for fatal in [true, false] {
        let dir = new_test_dir("model-route-budget");
        let db = dir.join("codexmanager.db");
        let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
        let (addr, rx, join) = start_mock_upstream_sequence_lenient(
            vec![
                (
                    if fatal { 400 } else { 503 },
                    r#"{"error":{"message":"fixture failure"}}"#.into()
                );
                20
            ],
            Duration::from_secs(2),
        );
        let storage = Storage::open(&db).unwrap();
        insert_aggregate_api(&storage, "agg_budget", &addr, "/responses", now_ts());
        configure(
            &storage,
            "weighted",
            (0..20)
                .map(|i| route("agg_budget", &format!("branch-{i}"), 20 - i, 1))
                .collect(),
        );
        insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
        let server = codexmanager_service::start_one_shot_server().unwrap();
        let (status, body) = send(&server.addr, "/v1/responses", false);
        server.join();
        join.join().unwrap();
        assert_eq!(status, if fatal { 400 } else { 503 }, "{body}");
        assert_eq!(rx.try_iter().count(), if fatal { 1 } else { 16 });
        if !fatal {
            assert!(body.contains("attempt budget exhausted"), "{body}");
        }
    }
}

#[test]
fn scheduled_model_source_preference_overrides_only_hybrid_key_order() {
    let _lock = test_env_guard();
    for policy in ["account_first", "aggregate_first", "weighted"] {
        let dir = new_test_dir("model-route-source-preference");
        let db = dir.join("codexmanager.db");
        let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
        let (local_addr, local_rx, local_join) = start_mock_upstream_sequence_lenient(
            vec![(200, response_json("resp_account_selected"))],
            Duration::from_secs(2),
        );
        let _upstream = EnvGuard::set(
            "CODEXMANAGER_UPSTREAM_BASE_URL",
            &format!("http://{local_addr}/backend-api/codex"),
        );
        let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
            vec![(200, response_json("resp_aggregate_selected"))],
            Duration::from_secs(2),
        );
        let storage = Storage::open(&db).unwrap();
        insert_active_account(&storage, &format!("acc_scheduled_{policy}"), now_ts());
        insert_aggregate_api(
            &storage,
            "agg_preference",
            &aggregate_addr,
            "/responses",
            now_ts(),
        );
        configure(
            &storage,
            policy,
            vec![
                route("default", "pool_selected", 0, 1),
                route("agg_preference", "aggregate_selected", 10, 1),
            ],
        );
        if policy == "account_first" {
            insert_hybrid_aggregate_first_key(
                &storage,
                "gk_model_schedule",
                "pk_model_schedule",
                now_ts(),
            );
        } else {
            insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
        }
        let server = codexmanager_service::start_one_shot_server().unwrap();
        let (status, body) = send(&server.addr, "/v1/responses", false);
        server.join();
        local_join.join().unwrap();
        aggregate_join.join().unwrap();
        assert_eq!(status, 200, "{policy}: {body}");
        assert_eq!(
            local_rx.try_iter().count(),
            usize::from(policy == "account_first")
        );
        assert_eq!(
            aggregate_rx.try_iter().count(),
            usize::from(policy != "account_first")
        );
        assert!(
            body.contains(if policy == "account_first" {
                "resp_account_selected"
            } else {
                "resp_aggregate_selected"
            }),
            "{body}"
        );
    }
}

#[test]
fn scheduled_explicit_aggregate_binding_remains_a_hard_limit_on_failure() {
    let _lock = test_env_guard();
    let dir = new_test_dir("model-route-hard-binding");
    let db = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let (addr, rx, join) = start_mock_upstream_sequence_lenient(
        vec![
            (
                503,
                r#"{"error":{"message":"bound source unavailable"}}"#.into(),
            ),
            (200, response_json("must_not_run")),
        ],
        Duration::from_secs(2),
    );
    let storage = Storage::open(&db).unwrap();
    insert_active_account(&storage, "acc_hard_binding", now_ts());
    insert_aggregate_api(&storage, "agg_bound", &addr, "/responses", now_ts());
    insert_aggregate_api(&storage, "agg_other", &addr, "/responses", now_ts());
    configure(
        &storage,
        "account_first",
        vec![
            route("default", "pool", 100, 100),
            route("agg_other", "other", 100, 100),
            route("agg_bound", "bound", 0, 1),
        ],
    );
    insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
    storage
        .update_api_key_rotation_config(
            "gk_model_schedule",
            "hybrid_rotation",
            Some("agg_bound"),
            None,
        )
        .unwrap();
    let server = codexmanager_service::start_one_shot_server().unwrap();
    let (status, body) = send(&server.addr, "/v1/responses", false);
    server.join();
    join.join().unwrap();
    assert_eq!(status, 503, "{body}");
    let requests = rx.try_iter().collect::<Vec<_>>();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&decode_upstream_request_body(&requests[0]))
            .unwrap()["model"],
        "bound"
    );
}

#[test]
fn scheduled_submitted_post_without_response_headers_is_not_replayed() {
    let _lock = test_env_guard();
    let dir = new_test_dir("model-route-no-ambiguous-replay");
    let db = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let listener = bind_test_listener("scheduled ambiguous delivery");
    let addr = listener.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    let upstream = thread::spawn(move || {
        if let Some((stream, captured)) = accept_http_request(&listener, Duration::from_secs(5)) {
            tx.send(captured).unwrap();
            stream.shutdown(std::net::Shutdown::Both).unwrap();
        }
        if let Some((stream, captured)) = accept_http_request(&listener, Duration::from_millis(500))
        {
            tx.send(captured).unwrap();
            stream.shutdown(std::net::Shutdown::Both).unwrap();
        }
    });
    let storage = Storage::open(&db).unwrap();
    insert_aggregate_api(&storage, "agg_no_replay", &addr, "/responses", now_ts());
    configure(
        &storage,
        "weighted",
        vec![
            route("agg_no_replay", "first", 10, 1),
            route("agg_no_replay", "second", 0, 1),
        ],
    );
    insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
    let server = codexmanager_service::start_one_shot_server().unwrap();
    let (status, body) = send(&server.addr, "/v1/responses", false);
    server.join();
    upstream.join().unwrap();
    assert_eq!(status, 502, "{body}");
    assert!(body.contains("automatic replay stopped"), "{body}");
    assert_eq!(rx.try_iter().count(), 1);
}

#[test]
fn scheduled_truncated_fatal_error_body_does_not_allow_fallback() {
    let _lock = test_env_guard();
    let dir = new_test_dir("model-route-truncated-fatal-body");
    let db = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let listener = bind_test_listener("scheduled fatal body");
    let addr = listener.local_addr().unwrap().to_string();
    let upstream = thread::spawn(move || {
        let (mut socket, _) = accept_http_request(&listener, Duration::from_secs(5)).unwrap();
        socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 100000\r\n\r\n{").unwrap();
        socket.flush().unwrap();
        drop(socket);
        assert!(
            accept_http_request(&listener, Duration::from_millis(500)).is_none(),
            "fatal status must remain terminal even if its diagnostic body is truncated"
        );
    });
    let storage = Storage::open(&db).unwrap();
    insert_aggregate_api(&storage, "agg_fatal_body", &addr, "/responses", now_ts());
    configure(
        &storage,
        "weighted",
        vec![
            route("agg_fatal_body", "first", 10, 1),
            route("agg_fatal_body", "second", 0, 1),
        ],
    );
    insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
    let server = codexmanager_service::start_one_shot_server().unwrap();
    let (status, body) = send(&server.addr, "/v1/responses", false);
    server.join();
    upstream.join().unwrap();
    assert_eq!(status, 400, "{body}");
}

#[test]
fn scheduled_error_body_read_respects_the_shared_request_deadline() {
    let _lock = test_env_guard();
    let dir = new_test_dir("model-route-shared-deadline");
    let db = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let _total = EnvGuard::set("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS", "250");
    let _stream = EnvGuard::set("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS", "250");
    let listener = bind_test_listener("scheduled deadline");
    let addr = listener.local_addr().unwrap().to_string();
    let upstream = thread::spawn(move || {
        let (mut socket, _) = accept_http_request(&listener, Duration::from_secs(5)).unwrap();
        socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: 100000\r\n\r\n{").unwrap();
        socket.flush().unwrap();
        thread::sleep(Duration::from_millis(700));
        drop(socket);
        assert!(
            accept_http_request(&listener, Duration::from_millis(200)).is_none(),
            "deadline must not reset for a fallback branch"
        );
    });
    let storage = Storage::open(&db).unwrap();
    insert_aggregate_api(&storage, "agg_deadline", &addr, "/responses", now_ts());
    configure(
        &storage,
        "weighted",
        vec![
            route("agg_deadline", "first", 10, 1),
            route("agg_deadline", "second", 0, 1),
        ],
    );
    insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
    let server = codexmanager_service::start_one_shot_server().unwrap();
    let started = Instant::now();
    let (status, body) = send(&server.addr, "/v1/responses", true);
    assert!(started.elapsed() < Duration::from_secs(2));
    server.join();
    upstream.join().unwrap();
    assert_eq!(status, 504, "{body}");
}

#[test]
fn scheduled_weighted_first_hops_share_state_across_requests() {
    let _lock = test_env_guard();
    let dir = new_test_dir("model-route-weighted-first-hops");
    let db = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let (addr, rx, join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_weighted")); 8],
        Duration::from_secs(5),
    );
    let storage = Storage::open(&db).unwrap();
    insert_aggregate_api(&storage, "agg_weight", &addr, "/responses", now_ts());
    configure(
        &storage,
        "weighted",
        vec![
            route("agg_weight", "weighted-three", 10, 3),
            route("agg_weight", "weighted-one", 10, 1),
        ],
    );
    insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
    for _ in 0..8 {
        let server = codexmanager_service::start_one_shot_server().unwrap();
        let (status, body) = send(&server.addr, "/v1/responses", false);
        server.join();
        assert_eq!(status, 200, "{body}");
    }
    join.join().unwrap();
    let models = rx
        .try_iter()
        .map(|request| {
            serde_json::from_slice::<serde_json::Value>(&decode_upstream_request_body(&request))
                .unwrap()["model"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(models.len(), 8);
    assert_eq!(
        models
            .iter()
            .filter(|model| model.as_str() == "weighted-three")
            .count(),
        6
    );
    assert_eq!(
        models
            .iter()
            .filter(|model| model.as_str() == "weighted-one")
            .count(),
        2
    );
}

#[test]
fn scheduled_distinct_sources_follow_priority_for_json_sse_and_both_endpoints() {
    let _lock = test_env_guard();
    for path in ["/v1/responses", "/v1/chat/completions"] {
        for stream in [false, true] {
            let dir = new_test_dir("model-route-independent-sources");
            let db = dir.join("codexmanager.db");
            let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
            let (first, first_rx, first_join) = start_mock_upstream_sequence_lenient(
                vec![(
                    503,
                    r#"{"error":{"message":"source A unavailable"}}"#.into(),
                )],
                Duration::from_secs(2),
            );
            let (ok, content_type) = success(path, stream);
            let (second, second_rx, second_join) =
                start_mock_upstream_sequence_lenient_with_content_types(
                    vec![(200, ok, content_type)],
                    Duration::from_secs(2),
                );
            assert_ne!(first, second);
            let storage = Storage::open(&db).unwrap();
            insert_aggregate_api(&storage, "agg_distinct_a", &first, path, now_ts());
            insert_aggregate_api(&storage, "agg_distinct_b", &second, path, now_ts());
            configure(
                &storage,
                "weighted",
                vec![
                    route("agg_distinct_b", "backup", 0, 1),
                    route("agg_distinct_a", "primary", 10, 1),
                ],
            );
            insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
            let server = codexmanager_service::start_one_shot_server().unwrap();
            let (status, body) = send(&server.addr, path, stream);
            server.join();
            first_join.join().unwrap();
            second_join.join().unwrap();
            assert_eq!(status, 200, "{path}, stream={stream}: {body}");
            assert!(
                body.contains("call_lookup"),
                "tool call survives fallback: {body}"
            );
            for (rx, expected) in [(first_rx, "primary"), (second_rx, "backup")] {
                let requests = rx.try_iter().collect::<Vec<_>>();
                assert_eq!(requests.len(), 1, "one attempt per independent source");
                let sent: serde_json::Value =
                    serde_json::from_slice(&decode_upstream_request_body(&requests[0])).unwrap();
                assert_eq!(sent["model"], expected);
                assert_eq!(sent["tools"].as_array().unwrap().len(), 1);
            }
            let logs = storage
                .list_request_logs(Some("key:=gk_model_schedule"), 10)
                .unwrap();
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].actual_source_id.as_deref(), Some("agg_distinct_b"));
        }
    }
}

#[test]
fn scheduled_distinct_sources_have_weighted_first_hops() {
    let _lock = test_env_guard();
    let dir = new_test_dir("model-route-independent-weights");
    let db = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
    let (first, first_rx, first_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_weighted_source_a")); 12],
        Duration::from_secs(2),
    );
    let (second, second_rx, second_join) = start_mock_upstream_sequence_lenient(
        vec![(200, response_json("resp_weighted_source_b")); 12],
        Duration::from_secs(2),
    );
    assert_ne!(first, second);
    let storage = Storage::open(&db).unwrap();
    insert_aggregate_api(&storage, "agg_weight_a", &first, "/responses", now_ts());
    insert_aggregate_api(&storage, "agg_weight_b", &second, "/responses", now_ts());
    configure(
        &storage,
        "weighted",
        vec![
            route("agg_weight_a", "weighted-three", 10, 3),
            route("agg_weight_b", "weighted-one", 10, 1),
        ],
    );
    insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
    let mut counts = [0; 2];
    for _ in 0..12 {
        let server = codexmanager_service::start_one_shot_server().unwrap();
        let (status, body) = send(&server.addr, "/v1/responses", false);
        server.join();
        assert_eq!(status, 200, "{body}");
        if body.contains("resp_weighted_source_a") {
            counts[0] += 1;
        } else {
            assert!(body.contains("resp_weighted_source_b"));
            counts[1] += 1;
        }
    }
    first_join.join().unwrap();
    second_join.join().unwrap();
    assert_eq!(counts, [9, 3]);
    assert_eq!(first_rx.try_iter().count(), 9);
    assert_eq!(second_rx.try_iter().count(), 3);
    assert_eq!(
        storage
            .list_request_logs(Some("key:=gk_model_schedule"), 20)
            .unwrap()
            .len(),
        12
    );
}

#[test]
fn scheduled_account_and_aggregate_sources_fallback_in_both_directions() {
    let _lock = test_env_guard();
    for policy in ["account_first", "aggregate_first"] {
        let dir = new_test_dir("model-route-cross-kind-fallback");
        let db = dir.join("codexmanager.db");
        let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db.to_str().unwrap());
        let failure = (
            503,
            r#"{"error":{"message":"temporary unavailable"}}"#.into(),
        );
        let (account_addr, account_rx, account_join) = start_mock_upstream_sequence_lenient(
            if policy == "account_first" {
                vec![failure.clone(); 16]
            } else {
                vec![(200, response_json("resp_cross_kind_account"))]
            },
            Duration::from_secs(2),
        );
        let base = format!("http://{account_addr}/backend-api/codex");
        let _upstream = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &base);
        let _fallback = EnvGuard::set("CODEXMANAGER_UPSTREAM_FALLBACK_BASE_URL", &base);
        let (aggregate_addr, aggregate_rx, aggregate_join) = start_mock_upstream_sequence_lenient(
            if policy == "aggregate_first" {
                vec![failure]
            } else {
                vec![(200, response_json("resp_cross_kind_aggregate"))]
            },
            Duration::from_secs(2),
        );
        let storage = Storage::open(&db).unwrap();
        insert_active_account(&storage, &format!("acc_cross_kind_{policy}"), now_ts());
        insert_aggregate_api(
            &storage,
            "agg_cross_kind",
            &aggregate_addr,
            "/responses",
            now_ts(),
        );
        configure(
            &storage,
            policy,
            vec![
                route("default", "pool-model", 0, 1),
                route("agg_cross_kind", "aggregate-model", 10, 1),
            ],
        );
        insert_hybrid_key(&storage, "gk_model_schedule", "pk_model_schedule", now_ts());
        let server = codexmanager_service::start_one_shot_server().unwrap();
        let (status, body) = send(&server.addr, "/v1/responses", false);
        server.join();
        account_join.join().unwrap();
        aggregate_join.join().unwrap();
        assert_eq!(status, 200, "{policy}: {body}");
        assert!(
            body.contains(if policy == "account_first" {
                "resp_cross_kind_aggregate"
            } else {
                "resp_cross_kind_account"
            }),
            "{policy}: {body}"
        );
        let account_count = account_rx.try_iter().count();
        let aggregate_count = aggregate_rx.try_iter().count();
        assert!(
            (1..16).contains(&account_count),
            "bounded account attempts: {account_count}"
        );
        assert_eq!(aggregate_count, 1);
        let logs = storage
            .list_request_logs(Some("key:=gk_model_schedule"), 10)
            .unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(
            logs[0].actual_source_kind.as_deref(),
            Some(if policy == "account_first" {
                "aggregate_api"
            } else {
                "openai_account"
            })
        );
    }
}
