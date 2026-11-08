use super::chat_non_stream_tools::{seed_account_route, MODEL, UPSTREAM_MODEL};
use super::*;
use serde_json::{json, Value};

fn run_edge(path: &str, stream: bool, upstream_sse: bool, case: &str) {
    let dir = new_test_dir(&format!(
        "protocol-edge-{}-{stream}-{upstream_sse}-{case}",
        path.replace('/', "-")
    ));
    let db_path = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).unwrap();
    storage.init().unwrap();
    seed_account_route(&storage);
    let partial = case == "limit-text";
    let tool_only = case == "limit-tool-only";
    let mut output = if tool_only {
        vec![]
    } else {
        vec![
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"上海23°C"}]}),
        ]
    };
    output.push(json!({"type":"function_call","id":"fc_truncated","call_id":"call_truncated","name":"lookup","arguments":"{\"city\":"}));
    let mut response = json!({
        "id":"resp_edge","object":"response","model":UPSTREAM_MODEL,
        "created_at":1720000000,"output":output,
        "status":if case == "failed" {"failed"} else if case == "error-on-completed" {"completed"} else {"incomplete"},
        "usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18}
    });
    if case == "failed" || case == "error-on-completed" {
        response["error"] = json!({"type":"server_error","code":"synthetic_failure","message":"Do not execute this failed response","param":"input"});
    } else {
        response["incomplete_details"] =
            json!({"reason":if partial || tool_only {"max_output_tokens"} else {"unknown_reason"}});
    }
    let body = if upstream_sse {
        let created = json!({"type":"response.created","response":{"id":"resp_edge","model":UPSTREAM_MODEL,"status":"in_progress","output":[]}});
        let event_type = if case == "error-on-completed" {
            "response.completed"
        } else if case == "failed" {
            "response.failed"
        } else {
            "response.incomplete"
        };
        let terminal = json!({"type":event_type,"response":response});
        // Prefix-only repair must append the missing suffix, never replay 上海.
        let prefix = if partial {
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"上海\"}\n\n"
        } else {
            ""
        };
        format!("data: {created}\n\n{prefix}data: {terminal}\n\n")
    } else {
        response.to_string()
    };
    let (addr, requests, join) = start_mock_upstream_once_with_content_type(
        &body,
        if upstream_sse {
            "text/event-stream"
        } else {
            "application/json"
        },
    );
    let _upstream = EnvGuard::set(
        "CODEXMANAGER_UPSTREAM_BASE_URL",
        &format!("http://{addr}/backend-api/codex"),
    );
    let server = codexmanager_service::start_one_shot_server().unwrap();
    let request = json!({"model":MODEL,"stream":stream,"max_tokens":256,"messages":[{"role":"user","content":"查询天气"}]});
    let (status, body) = post_http_raw(
        &server.addr,
        path,
        &request.to_string(),
        &[
            ("Content-Type", "application/json"),
            ("Authorization", "Bearer pk_non_stream_tools"),
            ("anthropic-version", "2023-06-01"),
        ],
    );
    server.join();
    join.join().unwrap();
    assert_eq!(requests.try_iter().count(), 1, "no model replay for {case}");
    assert!(
        !body.contains("\"type\":\"tool_use\""),
        "partial/failed tools must not execute: {body}"
    );
    assert!(
        !body.contains("\"tool_calls\""),
        "partial/failed tools must not execute: {body}"
    );
    if !stream {
        let value: Value = serde_json::from_str(&body).expect("complete JSON");
        assert_eq!(status, if partial { 200 } else { 502 }, "{case}: {body}");
        if partial {
            let (text, stop) = if path == "/v1/messages" {
                (
                    value["content"][0]["text"].as_str().unwrap(),
                    value["stop_reason"].as_str().unwrap(),
                )
            } else {
                (
                    value["choices"][0]["message"]["content"].as_str().unwrap(),
                    value["choices"][0]["finish_reason"].as_str().unwrap(),
                )
            };
            assert_eq!(text, "上海23°C");
            assert_eq!(
                stop,
                if path == "/v1/messages" {
                    "max_tokens"
                } else {
                    "length"
                }
            );
            assert_eq!(
                value["usage"]
                    .get("output_tokens")
                    .or_else(|| value["usage"].get("completion_tokens")),
                Some(&json!(7))
            );
        } else {
            let code = value["error"]["code"]
                .as_str()
                .expect("structured error code");
            assert_eq!(
                code,
                if case == "failed" || case == "error-on-completed" {
                    "synthetic_failure"
                } else {
                    "upstream_response_incomplete"
                }
            );
            assert!(value.get("content").is_none());
            assert!(value.get("choices").is_none());
        }
    } else {
        assert_eq!(status, 200, "stream headers were already committed");
        let events = body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .filter(|line| *line != "[DONE]")
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        if partial {
            assert!(!events.iter().any(|event| event.get("error").is_some()));
            let text = events
                .iter()
                .filter_map(|event| {
                    event
                        .pointer(if path == "/v1/messages" {
                            "/delta/text"
                        } else {
                            "/choices/0/delta/content"
                        })
                        .and_then(Value::as_str)
                })
                .collect::<String>();
            assert_eq!(text, "上海23°C");
            if path == "/v1/messages" {
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| event["type"] == "message_stop")
                        .count(),
                    1
                );
                assert!(events
                    .iter()
                    .any(|event| event["delta"]["stop_reason"] == "max_tokens"));
            } else {
                assert_eq!(body.matches("data: [DONE]").count(), 1);
                assert!(events
                    .iter()
                    .any(|event| event["choices"][0]["finish_reason"] == "length"));
            }
        } else {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.get("error").is_some())
                    .count(),
                1,
                "{body}"
            );
            assert!(!body.contains("message_stop"));
            assert!(!body.contains("data: [DONE]"));
        }
    }
}

fn matrix(path: &str, stream: bool) {
    let _lock = test_env_guard();
    for case in [
        "failed",
        "error-on-completed",
        "limit-text",
        "limit-tool-only",
        "unknown-incomplete",
    ] {
        run_edge(path, stream, true, case);
        if !stream {
            run_edge(path, stream, false, case);
        }
    }
}

#[test]
fn protocol_response_edges_chat_json() {
    matrix("/v1/chat/completions", false);
}
#[test]
fn protocol_response_edges_chat_stream() {
    matrix("/v1/chat/completions", true);
}
#[test]
fn protocol_response_edges_messages_json() {
    matrix("/v1/messages", false);
}
#[test]
fn protocol_response_edges_messages_stream() {
    matrix("/v1/messages", true);
}
