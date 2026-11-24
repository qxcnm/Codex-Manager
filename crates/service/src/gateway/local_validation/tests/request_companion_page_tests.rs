use super::*;
use serde_json::json;

#[test]
fn companion_page_http_context_reaches_account_aggregate_and_bps_as_user_text() {
    let _guard = crate::test_env_guard();
    let database_path = std::env::temp_dir().join(format!(
        "cm-companion-page-{:032x}.db",
        rand::random::<u128>()
    ));
    let _database = RuntimeEnvGuard::set("CODEXMANAGER_DB_PATH", database_path.to_str().unwrap());
    let mut headers = HeaderMap::new();
    headers.insert("originator", HeaderValue::from_static("codex_exec"));
    let incoming_headers = crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers);
    let real_call =
        json!({"type":"function_call","call_id":"call_real","name":"lookup","arguments":"{}"});
    let real_output =
        json!({"type":"function_call_output","call_id":"call_real","output":"real tool result"});

    for strategy in [
        crate::apikey_profile::ROTATION_ACCOUNT,
        crate::apikey_profile::ROTATION_AGGREGATE_API,
    ] {
        for stream in [false, true] {
            let storage = crate::storage_helpers::open_storage().expect("open isolated storage");
            storage.init().expect("initialize storage");
            let body = Bytes::from(serde_json::to_vec(&json!({
                "model":"gpt-5.6-sol", "stream":stream,
                "tools":[{"type":"function","name":"lookup","parameters":{"type":"object","properties":{}}}],
                "input":[real_call, real_output,
                    {"type":"function_call_output","id":"fco_page","name":"codex_apps_open_page","namespace":"agent","output":"{\"page_id\":null}"},
                    {"type":"function_call_output","id":"fco_snapshot","name":"codex_apps_selected_page_part_1_of_1","namespace":"agent","output":"Companion Page snapshot part1\n页面正文\n"},
                    {"type":"function_call_output","name":"goal_objective","namespace":"agent","output":"目标上下文\n"},
                    {"type":"custom_tool_call_output","call_id":" \t","name":"automation_update","output":{"schedule":"heartbeat","enabled":true}},
                    {"type":"tool_search_output","call_id":null,"output":["attached tools",{"future":true}]}
                ]
            })).unwrap());
            let request = axum::http::Request::builder()
                .method("POST")
                .uri("/v1/responses")
                .body(())
                .unwrap();
            let (parts, ()) = request.into_parts();
            let (request, _response) = Request::new(parts, body.clone());
            let mut key = sample_api_key(
                crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                None,
                None,
                None,
            );
            key.rotation_strategy = strategy.to_string();
            key.upstream_base_url = Some("https://chatgpt.com/backend-api/codex".into());
            let prepared = build_local_validation_result(
                &request,
                "trc_companion_page_test".into(),
                incoming_headers.clone(),
                storage,
                body,
                key,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "prepare page context: {} {}",
                    error.status_code, error.message
                )
            });
            assert_eq!(prepared.is_stream, stream);
            for body in [&prepared.body, &prepared.passthrough_body] {
                let upstream: Value = serde_json::from_slice(body).unwrap();
                assert_eq!(upstream["input"][0], real_call);
                assert_eq!(upstream["input"][1], real_output);
                assert_eq!(upstream["input"][2]["role"], "user");
                assert_eq!(upstream["input"][3]["role"], "user");
                assert_eq!(upstream["input"][3]["content"][0]["text"], "Companion Page context (agent.codex_apps_selected_page_part_1_of_1):\nCompanion Page snapshot part1\n页面正文\n");
                for (index, expected_text) in [
                    (4, "Companion Page context (agent.goal_objective):\n目标上下文\n"),
                    (5, "Companion Page context (automation_update):\n{\"enabled\":true,\"schedule\":\"heartbeat\"}"),
                    (6, "Companion Page context (tool_search_output):\n[\"attached tools\",{\"future\":true}]"),
                ] {
                    assert_eq!(upstream["input"][index]["role"], "user");
                    assert_eq!(upstream["input"][index]["content"][0]["type"], "input_text");
                    assert_eq!(upstream["input"][index]["content"][0]["text"], expected_text);
                }
                let (bps_body, _) =
                    crate::gateway::bps_protocol::prepare(body, "page-context-test")
                        .expect("page attachments must also be consumable by BPS");
                let bps: Value = serde_json::from_slice(&bps_body).unwrap();
                assert!(bps["input"].as_array().unwrap().iter().any(|item| {
                    item["role"] == "user"
                        && item["content"][0]["text"] == upstream["input"][3]["content"][0]["text"]
                }));
                for index in 4..=6 {
                    assert!(bps["input"].as_array().unwrap().iter().any(|item| {
                        item["role"] == "user"
                            && item["content"][0]["text"]
                                == upstream["input"][index]["content"][0]["text"]
                    }));
                }
                let chat_body =
                    crate::gateway::protocol_adapter::adapt_openai_responses_to_chat_completions(
                        body,
                        Some("deepseek-chat"),
                    )
                    .expect("desktop attachments must reach Chat upstreams as user context");
                let chat: Value = serde_json::from_slice(&chat_body).unwrap();
                assert_eq!(chat["stream"], stream);
                let messages = chat["messages"].as_array().unwrap();
                assert!(messages.iter().any(|item| item["role"] == "tool"
                    && item["tool_call_id"] == "call_real"
                    && item["content"] == "real tool result"));
                for index in 4..=6 {
                    assert!(messages.iter().any(|item| item["role"] == "user"
                        && item["content"][0]["type"] == "text"
                        && item["content"][0]["text"]
                            == upstream["input"][index]["content"][0]["text"]));
                }
            }
        }
    }
}
