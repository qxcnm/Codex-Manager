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
