use super::*;

#[test]
fn native_codex_images_generation_with_extension_headers_matches_direct_login_upstream_shape() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-native-images-generation-direct");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let images_json = serde_json::json!({
        "created": 1778832973u64,
        "background": "opaque",
        "data": [{ "b64_json": "REDACTED_IMAGE" }],
        "quality": "medium",
        "size": "1024x1024"
    })
    .to_string();
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(200, images_json, "application/json".to_string())],
            Duration::from_secs(3),
        );
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let _rules_guard = GatewayModelForwardRulesResetGuard::reset();
    seed_model_catalog_models(&storage, &["gpt-image-1.5"]);
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc_native_images_direct".to_string(),
            label: "native-images-direct".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_native_images_direct".to_string()),
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
            account_id: "acc_native_images_direct".to_string(),
            id_token: String::new(),
            access_token: "access_native_images_direct".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_native_images_direct".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_native_images_direct";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_native_images_direct".to_string(),
            name: Some("native-images-direct".to_string()),
            model_slug: Some("gpt-5.5".to_string()),
            reasoning_effort: Some("medium".to_string()),
            service_tier: Some("priority".to_string()),
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
        "prompt": "draw a small orange cat",
        "background": "opaque",
        "model": "gpt-image-1.5",
        "quality": "medium",
        "size": "1024x1024"
    })
    .to_string();
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/images/generations",
        &request_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("originator", "codex_app"),
            ("x-codex-image-turn-id", "turn-native-images-direct"),
            ("x-openai-actor-authorization", "local-image-extension"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(captured.path, "/backend-api/codex/images/generations");
    assert_eq!(
        captured.headers.get("accept").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        captured
            .headers
            .get("x-codex-image-turn-id")
            .map(String::as_str),
        Some("turn-native-images-direct")
    );
    assert!(!captured
        .headers
        .contains_key("x-openai-actor-authorization"));
    let upstream_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&captured)).expect("upstream json");
    assert_eq!(upstream_body["model"], "gpt-image-1.5");
    assert_eq!(upstream_body["prompt"], "draw a small orange cat");
    assert!(upstream_body.get("tools").is_none());
    assert!(upstream_body.get("reasoning").is_none());
    assert!(upstream_body.get("service_tier").is_none());

    let response: serde_json::Value =
        serde_json::from_str(&response_body).expect("images response json");
    assert_eq!(response["data"][0]["b64_json"], "REDACTED_IMAGE");
    assert_eq!(response["quality"], "medium");
    assert_eq!(response["size"], "1024x1024");
}

#[test]
fn gateway_images_generation_wraps_codex_sse_as_openai_images_json() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-images-generation");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let codex_sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ig_test\",\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"aGVsbG8=\",\"revised_prompt\":\"a small cat\",\"output_format\":\"png\",\"size\":\"1024x1024\",\"quality\":\"high\",\"background\":\"transparent\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_image_test\",\"model\":\"gpt-5.4-mini\",\"created_at\":1772000000,\"usage\":{\"input_tokens\":3,\"output_tokens\":1,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(200, codex_sse.to_string(), "text/event-stream".to_string())],
            Duration::from_secs(3),
        );
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let _rules_guard = GatewayModelForwardRulesResetGuard::reset();
    seed_model_catalog_models(&storage, &["gpt-5.4-mini", "gpt-5.4", "gpt-image-2"]);
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc_images_generation".to_string(),
            label: "images-generation".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_images_generation".to_string()),
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
            account_id: "acc_images_generation".to_string(),
            id_token: String::new(),
            access_token: "access_images_generation".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_images_generation".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_images_generation";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_images_generation".to_string(),
            name: Some("images-generation".to_string()),
            model_slug: None,
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
    let req_body = serde_json::json!({
        "prompt": "draw a small cat",
        "model": "gpt-image-2",
        "size": "1024x1024",
        "quality": "high",
        "background": "transparent",
        "output_format": "png",
        "response_format": "b64_json",
        "stream": false
    })
    .to_string();
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/images/generations",
        &req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(captured.path, "/backend-api/codex/responses");
    assert_eq!(
        captured.headers.get("accept").map(String::as_str),
        Some("text/event-stream")
    );
    let upstream_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&captured)).expect("upstream json");
    assert_eq!(upstream_body["model"], "gpt-6-luna");
    assert_eq!(upstream_body["tools"][0]["type"], "image_generation");
    assert_eq!(upstream_body["tools"][0]["model"], "gpt-image-2");
    assert_eq!(upstream_body["tool_choice"]["type"], "image_generation");

    let value: serde_json::Value =
        serde_json::from_str(&response_body).expect("images response json");
    assert_eq!(value["created"], 1772000000);
    assert_eq!(value["data"][0]["b64_json"], "aGVsbG8=");
    assert_eq!(value["data"][0]["revised_prompt"], "a small cat");
    assert_eq!(value["output_format"], "png");
    assert_eq!(value["size"], "1024x1024");
    assert_eq!(value["quality"], "high");
    assert_eq!(value["background"], "transparent");
    assert_eq!(value["usage"]["total_tokens"], 4);
}

#[test]
fn native_codex_responses_does_not_inject_image_generation_tool() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-native-codex-no-image-generation-injection");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let result_b64 = "QUJDREVGR0g=";
    let codex_sse = format!(
        concat!(
            "event: response.output_item.done\n",
            "data: {{\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{{\"id\":\"ig_auto\",\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"{result_b64}\",\"output_format\":\"png\"}}}}\n\n",
            "event: response.completed\n",
            "data: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp_auto_image\",\"model\":\"gpt-5.4\",\"output\":[{{\"id\":\"ig_auto\",\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"{result_b64}\",\"output_format\":\"png\"}}],\"usage\":{{\"input_tokens\":5,\"output_tokens\":2,\"total_tokens\":7}}}}}}\n\n",
            "data: [DONE]\n\n"
        ),
        result_b64 = result_b64
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(200, codex_sse, "text/event-stream".to_string())],
            Duration::from_secs(3),
        );
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let _rules_guard = GatewayModelForwardRulesResetGuard::reset();
    seed_model_catalog_models(&storage, &["gpt-5.4-mini", "gpt-5.4"]);
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc_native_image_auto".to_string(),
            label: "native-image-auto".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_native_image_auto".to_string()),
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
            account_id: "acc_native_image_auto".to_string(),
            id_token: String::new(),
            access_token: "access_native_image_auto".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_native_image_auto".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_native_image_auto";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_native_image_auto".to_string(),
            name: Some("native-image-auto".to_string()),
            model_slug: None,
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
    let req_body = serde_json::json!({
        "model": "gpt-5.4",
        "instructions": "Generate the requested image when useful.",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": "帮我生成一个现场作业中台 logo，透明背景，不要文字"
            }]
        }],
        "tool_choice": "auto",
        "stream": true,
        "prompt_cache_key": "thread-native-image-auto",
        "client_metadata": {
            "x-codex-installation-id": "install-native-image-auto"
        }
    })
    .to_string();
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("User-Agent", "codex_cli_rs/0.999.0 (Windows 11; x86_64)"),
            ("originator", "codex_cli_rs"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(captured.path, "/backend-api/codex/responses");

    let upstream_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&captured)).expect("upstream json");
    assert!(upstream_body.get("tools").is_none());
    assert_eq!(upstream_body["tool_choice"], "auto");
    assert_eq!(upstream_body["model"], "gpt-5.4");
    assert!(response_body.contains("event: response.output_item.done"));
    assert!(response_body.contains(result_b64));
    assert!(response_body.contains("data: [DONE]"));
}

#[test]
fn native_codex_image_generation_responses_request_passthroughs_tool_and_sse() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-native-codex-image-generation");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let partial_b64 = "cGFydGlhbF9pbWFnZV9jaHVuaw==";
    let result_b64 = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=";
    let codex_sse = format!(
        concat!(
            "event: response.output_item.added\n",
            "data: {{\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{{\"id\":\"ig_native\",\"type\":\"image_generation_call\",\"status\":\"in_progress\"}}}}\n\n",
            "event: response.image_generation_call.partial_image\n",
            "data: {{\"type\":\"response.image_generation_call.partial_image\",\"item_id\":\"ig_native\",\"partial_image_index\":0,\"partial_image_b64\":\"{partial_b64}\",\"output_format\":\"png\"}}\n\n",
            "event: response.output_item.done\n",
            "data: {{\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{{\"id\":\"ig_native\",\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"{result_b64}\",\"output_format\":\"png\"}}}}\n\n",
            "event: response.completed\n",
            "data: {{\"type\":\"response.completed\",\"response\":{{\"id\":\"resp_native_image\",\"model\":\"gpt-5.4\",\"output\":[{{\"id\":\"ig_native\",\"type\":\"image_generation_call\",\"status\":\"completed\",\"result\":\"{result_b64}\",\"output_format\":\"png\"}}],\"usage\":{{\"input_tokens\":5,\"output_tokens\":2,\"total_tokens\":7}}}}}}\n\n",
            "data: [DONE]\n\n"
        ),
        partial_b64 = partial_b64,
        result_b64 = result_b64
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient_with_content_types(
            vec![(200, codex_sse, "text/event-stream".to_string())],
            Duration::from_secs(3),
        );
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let _rules_guard = GatewayModelForwardRulesResetGuard::reset();
    seed_model_catalog_models(&storage, &["gpt-5.4-mini", "gpt-5.4"]);
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc_native_image_generation".to_string(),
            label: "native-image-generation".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_native_image_generation".to_string()),
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
            account_id: "acc_native_image_generation".to_string(),
            id_token: String::new(),
            access_token: "access_native_image_generation".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("api_access_native_image_generation".to_string()),
            last_refresh: now,
        })
        .expect("insert token");

    let platform_key = "pk_native_image_generation";
    storage
        .insert_api_key(&ApiKey {
            id: "gk_native_image_generation".to_string(),
            name: Some("native-image-generation".to_string()),
            model_slug: None,
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
    let req_body = serde_json::json!({
        "model": "gpt-5.4",
        "instructions": "Generate the requested image.",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "draw a small cat as a clean icon"
                }]
            },
            {
                "id": "ig_previous",
                "type": "image_generation_call",
                "status": "completed",
                "result": "cHJldmlvdXNfaW1hZ2U=",
                "output_format": "png"
            }
        ],
        "tools": [{
            "type": "image_generation",
            "output_format": "png"
        }],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "stream": true,
        "store": false,
        "prompt_cache_key": "thread-native-image",
        "client_metadata": {
            "x-codex-installation-id": "install-native-image",
            "turn_id": "turn-native-image"
        }
    })
    .to_string();
    let (status, response_body) = post_http_raw(
        &server.addr,
        "/v1/responses",
        &req_body,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", &format!("Bearer {platform_key}")),
            ("User-Agent", "codex_cli_rs/0.999.0 (Windows 11; x86_64)"),
            ("originator", "codex_cli_rs"),
            ("x-codex-window-id", "win-native-image"),
        ],
    );
    server.join();
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(captured.path, "/backend-api/codex/responses");
    assert!(captured
        .headers
        .get("user-agent")
        .is_some_and(|value| value.contains("codex_cli_rs")));

    let upstream_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&captured)).expect("upstream json");
    assert_eq!(upstream_body["model"], "gpt-5.4");
    assert_eq!(upstream_body["tools"][0]["type"], "image_generation");
    assert_eq!(upstream_body["tools"][0]["output_format"], "png");
    assert!(upstream_body["tools"][0].get("model").is_none());
    assert_eq!(upstream_body["tool_choice"], "auto");
    assert_eq!(upstream_body["stream"], true);
    assert_eq!(upstream_body["prompt_cache_key"], "thread-native-image");
    assert_eq!(
        upstream_body["client_metadata"]["turn_id"],
        "turn-native-image"
    );
    assert!(upstream_body["input"]
        .as_array()
        .expect("input array")
        .iter()
        .any(|item| item.get("type").and_then(serde_json::Value::as_str)
            == Some("image_generation_call")
            && item.get("result").and_then(serde_json::Value::as_str)
                == Some("cHJldmlvdXNfaW1hZ2U=")));

    assert!(response_body.contains("event: response.output_item.added"));
    assert!(response_body.contains("event: response.image_generation_call.partial_image"));
    assert!(response_body.contains(partial_b64));
    assert!(response_body.contains(result_b64));
    assert!(response_body.contains("\"result\":\"QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=\""));
    assert!(response_body.contains("data: [DONE]"));
}
