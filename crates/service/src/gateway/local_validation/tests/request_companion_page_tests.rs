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
                    {"type":"function_call_output","id":"fco_snapshot","name":"codex_apps_selected_page_part_1_of_1","namespace":"agent","output":"Companion Page snapshot part1\n页面正文\n"}
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
                let (bps_body, _) =
                    crate::gateway::bps_protocol::prepare(body, "page-context-test")
                        .expect("page attachments must also be consumable by BPS");
                let bps: Value = serde_json::from_slice(&bps_body).unwrap();
                assert!(bps["input"].as_array().unwrap().iter().any(|item| {
                    item["role"] == "user"
                        && item["content"][0]["text"] == upstream["input"][3]["content"][0]["text"]
                }));
            }
        }
    }
}
