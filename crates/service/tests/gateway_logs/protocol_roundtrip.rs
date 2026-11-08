use super::chat_non_stream_tools::{seed_account_route, MODEL, UPSTREAM_MODEL};
use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const TOOL_RESULT: &str = "{\"city\":\"上海\",\"temperature\":23}";
const ANSWER: &str = "上海23°C，已完成。";

fn completed(output: Value) -> String {
    let event = json!({"type": "response.completed", "response": {
        "id": "resp_roundtrip", "object": "response", "model": UPSTREAM_MODEL, "created_at": 1720000000,
        "status": "completed", "output": output,
        "usage": {"input_tokens": 11, "output_tokens": 7, "total_tokens": 18}
    }});
    let created = json!({"type": "response.created", "response": {
        "id": "resp_roundtrip", "object": "response", "model": UPSTREAM_MODEL,
        "created_at": 1720000000, "status": "in_progress", "output": [], "usage": null
    }});
    format!(
        "event: response.created\ndata: {created}\n\nevent: response.completed\ndata: {event}\n\n"
    )
}

fn output_message(text: &str) -> Value {
    json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]})
}

fn tools(path: &str) -> Value {
    let schema =
        json!({"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]});
    match path {
        "/v1/messages" => json!([{"name": "lookup", "input_schema": schema}]),
        "/v1/chat/completions" => {
            json!([{"type": "function", "function": {"name": "lookup", "parameters": schema}}])
        }
        _ => json!([{"type": "function", "name": "lookup", "parameters": schema}]),
    }
}

fn first_request(path: &str, stream: bool) -> Value {
    let mut body = json!({"model": MODEL, "stream": stream, "tools": tools(path)});
    if path == "/v1/responses" {
        body["input"] = json!([
            {"role": "user", "content": "城市上海"},
            output_message("已记住上海。"),
            {"role": "user", "content": "查询温度"}
        ]);
        body["tool_choice"] = json!({"type": "function", "name": "lookup"});
    } else {
        // The string assistant history reproduces the pre-fix Messages request
        // shape: it must become output_text, not an assistant input_text block.
        body["messages"] = json!([
            {"role": "user", "content": "城市上海"},
            {"role": "assistant", "content": "已记住上海。"},
            {"role": "user", "content": "查询温度"}
        ]);
        body["tool_choice"] = if path == "/v1/messages" {
            body["max_tokens"] = json!(256);
            json!({"type": "tool", "name": "lookup"})
        } else {
            json!({"type": "function", "function": {"name": "lookup"}})
        };
    }
    body
}

// Normalize only the public wire contracts, then feed the received tool ID,
// name and arguments into the next client turn. A 200 alone is never success.
fn read_turn(path: &str, stream: bool, body: &str) -> (String, Vec<Value>) {
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut usage = Value::Null;
    if !stream {
        let value: Value = serde_json::from_str(body).expect("complete JSON reply");
        assert!(value.get("error").is_none(), "{body}");
        usage = value["usage"].clone();
        match path {
            "/v1/messages" => {
                for item in value["content"].as_array().unwrap() {
                    if item["type"] == "text" {
                        text.push_str(item["text"].as_str().unwrap());
                    }
                    if item["type"] == "tool_use" {
                        calls.push(json!({"id": item["id"], "name": item["name"], "arguments": item["input"].to_string()}));
                    }
                }
                assert_eq!(
                    value["stop_reason"],
                    if calls.is_empty() {
                        "end_turn"
                    } else {
                        "tool_use"
                    }
                );
            }
            "/v1/chat/completions" => {
                let message = &value["choices"][0]["message"];
                text = message["content"].as_str().unwrap_or_default().into();
                for call in message["tool_calls"].as_array().into_iter().flatten() {
                    calls.push(json!({"id": call["id"], "name": call["function"]["name"], "arguments": call["function"]["arguments"]}));
                }
                assert_eq!(
                    value["choices"][0]["finish_reason"],
                    if calls.is_empty() {
                        "stop"
                    } else {
                        "tool_calls"
                    }
                );
            }
            _ => {
                assert_eq!(value["status"], "completed");
                for item in value["output"].as_array().unwrap() {
                    if item["type"] == "message" {
                        for part in item["content"].as_array().unwrap() {
                            text.push_str(part["text"].as_str().unwrap_or_default());
                        }
                    }
                    if item["type"] == "function_call" {
                        calls.push(json!({"id": item["call_id"], "name": item["name"], "arguments": item["arguments"]}));
                    }
                }
            }
        }
    } else {
        assert!(
            body.ends_with("\n\n"),
            "final SSE frame is complete: {body}"
        );
        let events = body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .filter(|line| *line != "[DONE]")
            .map(|line| serde_json::from_str::<Value>(line).expect("complete SSE JSON"))
            .collect::<Vec<_>>();
        for event in &events {
            assert!(event.get("error").is_none(), "{body}");
        }
        if path == "/v1/responses" {
            let terminal = events
                .iter()
                .filter(|event| event["type"] == "response.completed")
                .collect::<Vec<_>>();
            assert_eq!(terminal.len(), 1, "{body}");
            return read_turn(path, false, &terminal[0]["response"].to_string());
        }
        let mut indexed = BTreeMap::<usize, Value>::new();
        if path == "/v1/messages" {
            assert_eq!(
                events
                    .iter()
                    .filter(|v| v["type"] == "message_stop")
                    .count(),
                1
            );
            for event in &events {
                if event["type"] == "content_block_start"
                    && event["content_block"]["type"] == "tool_use"
                {
                    let block = &event["content_block"];
                    indexed.insert(
                        event["index"].as_u64().unwrap() as usize,
                        json!({"id": block["id"], "name": block["name"], "arguments": ""}),
                    );
                }
                if event["delta"]["type"] == "text_delta" {
                    text.push_str(event["delta"]["text"].as_str().unwrap());
                }
                if event["delta"]["type"] == "input_json_delta" {
                    let call = indexed
                        .get_mut(&(event["index"].as_u64().unwrap() as usize))
                        .unwrap();
                    call["arguments"] = json!(format!(
                        "{}{}",
                        call["arguments"].as_str().unwrap(),
                        event["delta"]["partial_json"].as_str().unwrap()
                    ));
                }
                if event["type"] == "message_delta" {
                    usage = event["usage"].clone();
                    assert_eq!(
                        event["delta"]["stop_reason"],
                        if indexed.is_empty() {
                            "end_turn"
                        } else {
                            "tool_use"
                        }
                    );
                }
            }
        } else {
            assert_eq!(body.matches("data: [DONE]").count(), 1);
            for event in &events {
                let delta = &event["choices"][0]["delta"];
                text.push_str(delta["content"].as_str().unwrap_or_default());
                for call in delta["tool_calls"].as_array().into_iter().flatten() {
                    let target = indexed
                        .entry(call["index"].as_u64().unwrap() as usize)
                        .or_insert_with(|| json!({"id": "", "name": "", "arguments": ""}));
                    for (field, value) in [
                        ("id", &call["id"]),
                        ("name", &call["function"]["name"]),
                        ("arguments", &call["function"]["arguments"]),
                    ] {
                        if let Some(part) = value.as_str() {
                            target[field] =
                                json!(format!("{}{}", target[field].as_str().unwrap(), part));
                        }
                    }
                }
                if !event["usage"].is_null() {
                    usage = event["usage"].clone();
                }
            }
            let finish = events
                .iter()
                .filter_map(|event| event["choices"][0]["finish_reason"].as_str())
                .collect::<Vec<_>>();
            assert_eq!(
                finish,
                vec![if indexed.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                }]
            );
        }
        calls = indexed.into_values().collect();
    }
    assert_eq!(
        usage
            .get("input_tokens")
            .or_else(|| usage.get("prompt_tokens")),
        Some(&json!(11)),
        "{body}"
    );
    assert_eq!(
        usage
            .get("output_tokens")
            .or_else(|| usage.get("completion_tokens")),
        Some(&json!(7)),
        "{body}"
    );
    (text, calls)
}

fn followup(path: &str, first: &Value, text: &str, call: &Value) -> Value {
    let mut body = first.clone();
    if path == "/v1/responses" {
        let input = body["input"].as_array_mut().unwrap();
        input.push(output_message(text));
        input.push(json!({"type": "function_call", "call_id": call["id"], "name": call["name"], "arguments": call["arguments"]}));
        input.push(
            json!({"type": "function_call_output", "call_id": call["id"], "output": TOOL_RESULT}),
        );
        body["tool_choice"] = json!("auto");
    } else if path == "/v1/messages" {
        let messages = body["messages"].as_array_mut().unwrap();
        messages.push(json!({"role": "assistant", "content": [
            {"type": "text", "text": text},
            {"type": "tool_use", "id": call["id"], "name": call["name"], "input": serde_json::from_str::<Value>(call["arguments"].as_str().unwrap()).unwrap()}
        ]}));
        messages.push(json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": call["id"], "content": TOOL_RESULT}]}));
        body["tool_choice"] = json!({"type": "auto"});
    } else {
        let messages = body["messages"].as_array_mut().unwrap();
        messages.push(json!({"role": "assistant", "content": text, "tool_calls": [{"id": call["id"], "type": "function", "function": {"name": call["name"], "arguments": call["arguments"]}}]}));
        messages.push(json!({"role": "tool", "tool_call_id": call["id"], "content": TOOL_RESULT}));
        body["tool_choice"] = json!("auto");
    }
    body
}

fn run_roundtrip(path: &str, stream: bool) {
    let _lock = test_env_guard();
    let dir = new_test_dir(&format!(
        "protocol-roundtrip-{}-{stream}",
        path.replace('/', "-")
    ));
    let db_path = dir.join("codexmanager.db");
    let _db = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let storage = Storage::open(&db_path).expect("open isolated db");
    storage.init().expect("initialize db");
    seed_account_route(&storage);
    let first_wire = completed(
        json!([output_message("正在查询。"), {"type": "function_call", "id": "fc_lookup", "call_id": "call_lookup", "name": "lookup", "arguments": "{\"city\":\"上海\"}"}]),
    );
    let second_wire = completed(json!([output_message(ANSWER)]));
    let (addr, requests, join) = start_mock_upstream_sequence_lenient_with_content_types(
        vec![
            (200, first_wire, "text/event-stream".into()),
            (200, second_wire, "text/event-stream".into()),
        ],
        Duration::from_secs(5),
    );
    let _upstream = EnvGuard::set(
        "CODEXMANAGER_UPSTREAM_BASE_URL",
        &format!("http://{addr}/backend-api/codex"),
    );
    let mut request = first_request(path, stream);
    let mut captured = Vec::new();
    for turn in 0..2 {
        let server = codexmanager_service::start_one_shot_server().expect("start gateway");
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
        assert_eq!(status, 200, "{path} stream={stream} turn={turn}: {body}");
        let sent = requests
            .recv_timeout(Duration::from_secs(2))
            .expect("captured upstream attempt");
        assert_eq!(sent.path, "/backend-api/codex/responses");
        let payload: Value = serde_json::from_slice(&decode_upstream_request_body(&sent)).unwrap();
        assert_eq!(payload["model"], UPSTREAM_MODEL);
        assert_eq!(payload["stream"], true);
        for item in payload["input"].as_array().unwrap() {
            if item["role"] == "assistant" {
                for part in item["content"].as_array().unwrap() {
                    assert_eq!(
                        part["type"], "output_text",
                        "assistant history must use output_text: {payload}"
                    );
                }
            }
        }
        let (text, calls) = read_turn(path, stream, &body);
        if turn == 0 {
            assert_eq!(
                payload["tool_choice"],
                json!({"type": "function", "name": "lookup"})
            );
            assert_eq!(text, "正在查询。");
            assert_eq!(
                calls.len(),
                1,
                "tool call must reach the client once: {body}"
            );
            assert_eq!(calls[0]["id"], "call_lookup");
            assert_eq!(calls[0]["name"], "lookup");
            assert_eq!(
                serde_json::from_str::<Value>(calls[0]["arguments"].as_str().unwrap()).unwrap(),
                json!({"city": "上海"})
            );
            request = followup(path, &request, &text, &calls[0]);
        } else {
            assert_eq!(text, ANSWER);
            assert!(calls.is_empty());
            let result = payload["input"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["type"] == "function_call_output")
                .expect("tool result reaches upstream");
            assert_eq!(result["call_id"], "call_lookup");
            assert_eq!(result["output"], TOOL_RESULT);
        }
        captured.push(payload);
    }
    join.join().expect("join controlled upstream");
    assert_eq!(captured.len(), 2);
    assert!(
        requests.try_recv().is_err(),
        "no hidden duplicate upstream requests"
    );
}

#[test]
fn protocol_roundtrip_responses_json() {
    run_roundtrip("/v1/responses", false);
}
#[test]
fn protocol_roundtrip_responses_stream() {
    run_roundtrip("/v1/responses", true);
}
#[test]
fn protocol_roundtrip_chat_json() {
    run_roundtrip("/v1/chat/completions", false);
}
#[test]
fn protocol_roundtrip_chat_stream() {
    run_roundtrip("/v1/chat/completions", true);
}
#[test]
fn protocol_roundtrip_messages_json() {
    run_roundtrip("/v1/messages", false);
}
#[test]
fn protocol_roundtrip_messages_stream() {
    run_roundtrip("/v1/messages", true);
}
