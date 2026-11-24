use super::*;
use serde_json::{json, Value};

const MODEL: &str = "gpt-compaction-history-test";
const PLATFORM_KEY: &str = "pk_compaction_history_mock";
const LEADING_INSTRUCTIONS: &str = "  Summarise the complete conversation.\n";
const EXISTING_INSTRUCTIONS: &str = "  Keep these existing instructions exactly.\n";

fn seed_compaction_account(storage: &Storage) {
    let now = now_ts();
    seed_model_catalog_models(storage, &[MODEL]);
    storage
        .insert_account(&Account {
            id: "acc_compaction_mock".to_string(),
            label: "compaction history local mock".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_compaction_mock".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert local mock account");
    storage
        .insert_token(&Token {
            account_id: "acc_compaction_mock".to_string(),
            id_token: String::new(),
            access_token: "mock-compaction-access".to_string(),
            refresh_token: String::new(),
            api_key_access_token: Some("mock-compaction-api-access".to_string()),
            last_refresh: now,
        })
        .expect("insert local mock account token");
    storage
        .insert_api_key(&ApiKey {
            id: "gk_compaction_mock".to_string(),
            name: Some("compaction history local mock".to_string()),
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
            key_hash: hash_platform_key_for_test(PLATFORM_KEY),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert local mock platform key");
}

fn compaction_input() -> Value {
    json!([
        {"role": "system", "content": LEADING_INSTRUCTIONS},
        {"role": "user", "content": [{"type": "input_text", "text": "请检查原始会话。\n"}]},
        {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Checking the tool history."}]},
        {"type": "function_call", "call_id": "call_compaction_519", "name": "lookup_context", "arguments": "{\"section\":\"history\"}"},
        {"type": "function_call_output", "call_id": "call_compaction_519", "output": "  Tool context: 用户内容\n"},
        {"role": "system", "content": [{"type": "input_text", "text": "  Compaction checkpoint: preserve the tool result.\n"}]},
        {"role": "user", "content": "Summarise this conversation without dropping the call result."}
    ])
}

fn assert_compaction_history_forwarded(stream: bool, existing_instructions: bool) {
    let dir = new_test_dir("codexmanager-compaction-history");
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open isolated compaction db");
    storage.init().expect("init isolated compaction db");
    let _rules_guard = GatewayModelForwardRulesResetGuard::reset();
    seed_compaction_account(&storage);

    let completed = json!({
        "id": "resp_compaction_519", "object": "response", "status": "completed", "model": MODEL,
        "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Compacted conversation with its tool context."}]}],
        "usage": {"input_tokens": 11, "output_tokens": 3, "total_tokens": 14}
    });
    let upstream_sse = format!(
        "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"type": "response.completed", "response": completed})
    );
    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_once_with_content_type(&upstream_sse, "text/event-stream");
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let original_input = compaction_input();
    let mut request = json!({"model": MODEL, "input": original_input, "stream": stream});
    if existing_instructions {
        request["instructions"] = json!(EXISTING_INSTRUCTIONS);
    }
    let server = codexmanager_service::start_one_shot_server().expect("start real gateway");
    let response = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build local HTTP client")
        .post(format!("http://{}/v1/responses", server.addr))
        .bearer_auth(PLATFORM_KEY)
        .json(&request)
        .send()
        .expect("send compaction through gateway");
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let response_body = response.text().expect("read real gateway response");
    server.join();
    upstream_join.join().expect("join local mock upstream");
    assert_eq!(status, 200, "gateway response: {response_body}");

    let captured = upstream_rx.try_iter().collect::<Vec<_>>();
    assert_eq!(
        captured.len(),
        1,
        "compaction must use one local upstream request"
    );
    assert_eq!(captured[0].path, "/backend-api/codex/responses");
    let forwarded: Value = serde_json::from_slice(&decode_upstream_request_body(&captured[0]))
        .expect("parse captured upstream compaction request");
    assert_eq!(forwarded["model"], MODEL);
    assert_eq!(
        forwarded["stream"], true,
        "Codex upstream uses SSE for both client modes"
    );
    let mut expected_input = original_input
        .as_array()
        .expect("original input array")
        .clone();
    expected_input[5]["role"] = json!("developer");
    if existing_instructions {
        expected_input[0]["role"] = json!("developer");
        assert_eq!(forwarded["instructions"], EXISTING_INSTRUCTIONS);
    } else {
        expected_input.remove(0);
        assert_eq!(forwarded["instructions"], LEADING_INSTRUCTIONS);
    }
    assert_eq!(
        forwarded["input"],
        json!(expected_input),
        "normalizing instruction roles must preserve message/tool ordering, content and call_id"
    );

    if stream {
        assert!(
            content_type.starts_with("text/event-stream"),
            "{content_type}"
        );
        let response_event = response_body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
            .find(|event| event["type"] == "response.completed")
            .expect("streaming client receives the completed response event");
        assert_eq!(response_event["response"], completed);
    } else {
        assert!(
            content_type.starts_with("application/json"),
            "{content_type}"
        );
        let response_json: Value =
            serde_json::from_str(&response_body).expect("non-streaming client receives JSON");
        assert_eq!(response_json, completed);
    }
}

#[test]
fn gateway_compaction_hoists_leading_system_and_normalizes_mid_history_system() {
    let _lock = test_env_guard();
    for stream in [false, true] {
        assert_compaction_history_forwarded(stream, false);
    }
}

#[test]
fn gateway_compaction_preserves_existing_instructions_and_leading_message_position() {
    let _lock = test_env_guard();
    for stream in [false, true] {
        assert_compaction_history_forwarded(stream, true);
    }
}

fn assert_compact_continuation_recovers_without_compression(client_stream: bool, native: bool) {
    let dir = new_test_dir("codexmanager-compact-continuation-531");
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open isolated continuation db");
    storage.init().expect("init isolated continuation db");
    let _rules_guard = GatewayModelForwardRulesResetGuard::reset();
    let _compression_guard = EnvGuard::set("CODEXMANAGER_ENABLE_REQUEST_COMPRESSION", "1");
    let _websocket_guard = EnvGuard::set("CODEXMANAGER_USE_WEBSOCKET_UPSTREAM", "0");
    seed_compaction_account(&storage);

    // Opaque local fixture, not a real account's encrypted conversation.
    let compact_output = json!([
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Preserve this conversation."}]},
        {"type": "compaction", "encrypted_content": "mock-opaque-compaction-531/".repeat(6000)}
    ]);
    let compact_response = json!({"object": "response.compaction", "output": compact_output});
    let compact_v2_response = json!({
        "id": "resp_compact_531", "object": "response", "status": "completed", "model": MODEL,
        "output": compact_output
    });
    let compact_v2_sse = format!(
        "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"type": "response.completed", "response": compact_v2_response})
    );
    let completed = json!({
        "id": "resp_continuation_531", "object": "response", "status": "completed", "model": MODEL,
        "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Conversation continued."}]}],
        "usage": {"input_tokens": 11, "output_tokens": 3, "total_tokens": 14}
    });
    let sse = format!(
        "event: response.completed\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"type": "response.completed", "response": completed})
    );
    let compact_wire = compact_response.to_string();
    let upstream =
        tiny_http::Server::http("127.0.0.1:0").expect("start local continuation upstream");
    // Select the real ChatGPT transport policy while keeping the destination
    // on loopback. No request leaves the local test server.
    let upstream_base = format!(
        "http://{}/chatgpt.com/backend-api/codex",
        upstream.server_addr()
    );
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);
    let (tx, rx) = mpsc::channel();
    let compact_attempts = if native { 2 } else { 1 };
    let expected_attempts = compact_attempts + 4;
    let upstream_join = thread::spawn(move || {
        let mut count = 0;
        loop {
            let timeout = if count >= expected_attempts {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(5)
            };
            let Some(mut request) = upstream
                .recv_timeout(timeout)
                .expect("receive local request")
            else {
                break;
            };
            let path = request.url().to_string();
            let headers = request
                .headers()
                .iter()
                .map(|header| {
                    (
                        header.field.to_string().to_ascii_lowercase(),
                        header.value.to_string(),
                    )
                })
                .collect::<HashMap<_, _>>();
            let compressed = headers.contains_key("content-encoding");
            let compact = path == "/chatgpt.com/backend-api/codex/responses/compact";
            let mut body = Vec::new();
            request
                .as_reader()
                .read_to_end(&mut body)
                .expect("read local upstream body");
            let plain = if compressed {
                zstd::stream::decode_all(std::io::Cursor::new(&body))
                    .expect("decode local zstd body")
            } else {
                body.clone()
            };
            let compact_v2 = serde_json::from_slice::<Value>(&plain)
                .expect("upstream JSON")
                .get("input")
                .and_then(Value::as_array)
                .is_some_and(|input| {
                    input
                        .iter()
                        .any(|item| item["type"] == "compaction_trigger")
                });
            tx.send(CapturedUpstreamRequest {
                path,
                headers,
                body,
            })
            .expect("capture local request");
            // Reproduce an edge that accepts this exact payload as JSON, but
            // rejects its compressed transport even without session headers.
            let (status, body, content_type) = if compact {
                (200, compact_wire.as_str(), "application/json")
            } else if compressed {
                (400, r#"{"detail":"Bad Request"}"#, "application/json")
            } else if compact_v2 {
                (200, compact_v2_sse.as_str(), "text/event-stream")
            } else {
                (200, sse.as_str(), "text/event-stream")
            };
            request
                .respond(
                    tiny_http::Response::from_string(body)
                        .with_status_code(tiny_http::StatusCode(status))
                        .with_header(
                            tiny_http::Header::from_bytes("Content-Type", content_type)
                                .expect("content type"),
                        ),
                )
                .expect("respond local upstream");
            count += 1;
        }
    });
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build local continuation client");
    let send = |path: &str, payload: &Value| {
        let server = codexmanager_service::start_one_shot_server().expect("start real gateway");
        let mut request = client
            .post(format!("http://{}{path}", server.addr))
            .bearer_auth(PLATFORM_KEY)
            .header("session-id", "compact-continuation-session-531")
            .json(payload);
        if native {
            request = request.header("user-agent", "codex_cli_rs/local-regression");
        }
        let response = request.send().expect("send through real gateway");
        let status = response.status().as_u16();
        let body = response.text().expect("read gateway response");
        server.join();
        (status, body)
    };
    let (compact_path, compact_request) = if native {
        (
            "/v1/responses",
            json!({
                "model": MODEL, "instructions": EXISTING_INSTRUCTIONS,
                "input": [{"role": "user", "content": "Compact this conversation."}, {"type": "compaction_trigger"}],
                "stream": true, "store": false,
                "prompt_cache_key": "compact-continuation-session-531"
            }),
        )
    } else {
        (
            "/v1/responses/compact",
            json!({
                "model": MODEL, "instructions": EXISTING_INSTRUCTIONS,
                "input": [{"role": "user", "content": "Compact this conversation."}],
                "prompt_cache_key": "compact-continuation-session-531"
            }),
        )
    };
    let (compact_status, compact_body) = send(compact_path, &compact_request);
    assert_eq!(compact_status, 200, "compact response: {compact_body}");
    if native {
        let event = compact_body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|data| serde_json::from_str::<Value>(data.trim()).ok())
            .find(|event| event["type"] == "response.completed")
            .expect("v2 compact completion");
        assert_eq!(event["response"], compact_v2_response);
    } else {
        assert_eq!(
            serde_json::from_str::<Value>(&compact_body).expect("compact JSON"),
            compact_response
        );
    }

    let mut input = compact_output
        .as_array()
        .expect("compact output array")
        .clone();
    input.extend([
        json!({"type": "function_call", "call_id": "call_531", "name": "lookup", "arguments": "{}"}),
        json!({"type": "function_call_output", "call_id": "call_531", "output": "Tool history must remain intact."}),
        json!({"role": "user", "content": "Continue after compaction."}),
    ]);
    let mut payloads = Vec::new();
    let mut results = Vec::new();
    for text in ["Continue after compaction.", "Hi"] {
        input.last_mut().expect("last message")["content"] = json!(text);
        let payload = json!({
            "model": MODEL, "instructions": EXISTING_INSTRUCTIONS, "input": input,
            "tools": [{"type": "function", "name": "lookup", "parameters": {"type": "object", "properties": {}}}],
            "stream": client_stream, "store": false,
            "prompt_cache_key": "compact-continuation-session-531"
        });
        results.push(send("/v1/responses", &payload));
        payloads.push(payload);
    }
    upstream_join.join().expect("join continuation upstream");
    let captured = rx.try_iter().collect::<Vec<_>>();
    assert_eq!(
        captured.len(),
        expected_attempts,
        "compact plus exactly two attempts per compressed request"
    );
    if native {
        assert_eq!(
            captured[0]
                .headers
                .get("content-encoding")
                .map(String::as_str),
            Some("zstd")
        );
        assert!(!captured[1].headers.contains_key("content-encoding"));
        assert_eq!(decode_upstream_request_body(&captured[0]), captured[1].body);
        let forwarded: Value = serde_json::from_slice(&captured[1].body).expect("v2 compact JSON");
        assert_eq!(forwarded["input"], compact_request["input"]);
    } else {
        assert!(!captured[0].headers.contains_key("content-encoding"));
    }
    let original_account = &captured[0].headers["chatgpt-account-id"];
    let original_auth = &captured[0].headers["authorization"];
    for (index, attempts) in captured[compact_attempts..].chunks_exact(2).enumerate() {
        let (status, response_body) = &results[index];
        assert_eq!(*status, 200, "continuation {index}: {response_body}");
        if client_stream {
            assert!(
                response_body.contains("Conversation continued."),
                "{response_body}"
            );
            assert!(
                response_body.contains("response.completed"),
                "{response_body}"
            );
        } else {
            assert_eq!(
                serde_json::from_str::<Value>(response_body).expect("continuation JSON"),
                completed
            );
        }
        assert_eq!(
            attempts[0]
                .headers
                .get("content-encoding")
                .map(String::as_str),
            Some("zstd")
        );
        assert!(!attempts[1].headers.contains_key("content-encoding"));
        assert_eq!(
            decode_upstream_request_body(&attempts[0]),
            attempts[1].body,
            "recovery must preserve exact model, tools, opaque compaction and history"
        );
        let forwarded: Value = serde_json::from_slice(&attempts[1].body).expect("plain JSON");
        assert_eq!(forwarded["input"], payloads[index]["input"]);
        assert_eq!(forwarded["tools"], payloads[index]["tools"]);
        for attempt in attempts {
            assert_eq!(attempt.path, "/chatgpt.com/backend-api/codex/responses");
            assert_eq!(&attempt.headers["chatgpt-account-id"], original_account);
            assert_eq!(&attempt.headers["authorization"], original_auth);
        }
    }
}

#[test]
fn gateway_compact_then_native_stream_continuation_recovers_from_compressed_400() {
    let _lock = test_env_guard();
    assert_compact_continuation_recovers_without_compression(true, true);
}

#[test]
fn gateway_compact_then_json_continuation_recovers_from_compressed_400() {
    let _lock = test_env_guard();
    assert_compact_continuation_recovers_without_compression(false, false);
}
