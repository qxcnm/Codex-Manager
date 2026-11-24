use super::normalize_companion_page_context;
use serde_json::{json, Value};

#[test]
fn companion_page_attachments_keep_text_and_real_tool_results() {
    let call =
        json!({"type":"function_call","call_id":"call_real","name":"lookup","arguments":"{}"});
    let result = json!({"type":"function_call_output","call_id":"call_real","name":"codex_apps_open_page","namespace":"agent","output":"real result"});
    let custom =
        json!({"type":"custom_tool_call_output","call_id":"call_custom","output":"custom result"});
    let snapshot = "Companion Page snapshot part1\n<html>中文 & exact text</html>\n";
    let mut payload = json!({"model":"gpt-5.4","input":[
        call, result, custom,
        {"id":"fco_page","type":"function_call_output","name":"codex_apps_open_page","namespace":"agent","output":"{\"page_id\":null}"},
        {"id":"fco_snapshot","type":"function_call_output","name":"codex_apps_selected_page_part_1_of_1","namespace":"agent","output":snapshot}
    ]});

    assert_eq!(
        normalize_companion_page_context("/v1/responses", &mut payload),
        2
    );
    assert_eq!(payload["input"][0], call);
    assert_eq!(payload["input"][1], result);
    assert_eq!(payload["input"][2], custom);
    for (index, name, output) in [
        (3, "codex_apps_open_page", "{\"page_id\":null}"),
        (4, "codex_apps_selected_page_part_1_of_1", snapshot),
    ] {
        let message = &payload["input"][index];
        assert_eq!(message["type"], "message");
        assert_eq!(message["role"], "user");
        assert_eq!(message["content"][0]["type"], "input_text");
        assert_eq!(
            message["content"][0]["text"],
            format!("Companion Page context (agent.{name}):\n{output}")
        );
        assert!(message.get("call_id").is_none());
    }
    assert_eq!(
        normalize_companion_page_context("/v1/responses", &mut payload),
        0
    );
}

#[test]
fn companion_page_attachments_allow_absent_namespace_and_empty_ids() {
    for id in [Value::Null, json!(""), json!(" \t")] {
        let mut payload = json!({"input":[{"type":"function_call_output","name":"codex_apps_selected_page_part_2_of_12","call_id":id,"output":"page text"}]});
        assert_eq!(
            normalize_companion_page_context("/v1/responses/compact?source=desktop", &mut payload),
            1
        );
        assert_eq!(payload["input"][0]["role"], "user");
        assert_eq!(
            payload["input"][0]["content"][0]["text"],
            "Companion Page context (codex_apps_selected_page_part_2_of_12):\npage text"
        );
    }
}

#[test]
fn desktop_context_outputs_are_detected_structurally_without_tool_name_allowlists() {
    for output_type in [
        "function_call_output",
        "custom_tool_call_output",
        "tool_search_output",
        "future_context_output",
    ] {
        for call_id in [
            None,
            Some(Value::Null),
            Some(json!("")),
            Some(json!(" \t\n")),
        ] {
            let mut item = json!({
                "type": output_type,
                "namespace": "agent",
                "name": "goal_objective",
                "output": "Continue the user's goal.\n目标原文\n"
            });
            if let Some(call_id) = call_id {
                item["call_id"] = call_id;
            }
            let mut payload = json!({"input": [item]});
            assert_eq!(
                normalize_companion_page_context("/v1/responses", &mut payload),
                1,
                "missed desktop context: {payload}"
            );
            assert_eq!(payload["input"][0]["role"], "user");
            assert_eq!(
                payload["input"][0]["content"][0]["text"],
                "Companion Page context (agent.goal_objective):\nContinue the user's goal.\n目标原文\n"
            );
        }
    }
}

#[test]
fn desktop_context_outputs_preserve_structured_content_and_optional_source() {
    for output in [
        json!({"objective": "用户目标", "done": false}),
        json!([{"type": "text", "text": "heartbeat context"}, {"status": 2}]),
        Value::Null,
        json!(123),
        json!(false),
    ] {
        for (source_fields, expected_source) in [
            (json!({}), "function_call_output"),
            (json!({"name":"automation_update"}), "automation_update"),
            (
                json!({"namespace":"new_namespace","name":"heartbeat"}),
                "new_namespace.heartbeat",
            ),
        ] {
            let mut item = source_fields;
            item["type"] = json!("function_call_output");
            item["output"] = output.clone();
            let mut payload = json!({"input": item});
            assert_eq!(
                normalize_companion_page_context("/v1/responses", &mut payload),
                1
            );
            assert_eq!(payload["input"]["role"], "user");
            assert_eq!(
                payload["input"]["content"][0]["text"],
                format!("Companion Page context ({expected_source}):\n{output}")
            );
            assert_eq!(
                normalize_companion_page_context("/v1/responses", &mut payload),
                0
            );
        }
    }
}

#[test]
fn desktop_context_outputs_without_output_preserve_the_original_attachment() {
    let item = json!({
        "type":"function_call_output",
        "namespace":"agent",
        "name":"goal_objective",
        "tools":[{"type":"function","name":"retained_tool"}],
        "goal":{"objective":"不丢失目标", "status":"active"}
    });
    let mut payload = json!({"input":[item]});
    assert_eq!(
        normalize_companion_page_context("/v1/responses", &mut payload),
        1
    );
    assert_eq!(payload["input"][0]["role"], "user");
    let text = payload["input"][0]["content"][0]["text"].as_str().unwrap();
    let (source, preserved) = text.split_once('\n').unwrap();
    assert_eq!(source, "Companion Page context (agent.goal_objective):");
    assert_eq!(serde_json::from_str::<Value>(preserved).unwrap(), item);

    let mut minimal = json!({"input":[{"type":"function_call_output","namespace":"agent","name":"goal_objective"}]});
    assert_eq!(
        normalize_companion_page_context("/v1/responses", &mut minimal),
        1
    );
    assert_eq!(minimal["input"][0]["role"], "user");
}

#[test]
fn desktop_context_normalization_never_rewrites_real_tool_results() {
    for output_type in [
        "function_call_output",
        "custom_tool_call_output",
        "tool_search_output",
        "future_tool_output",
    ] {
        for call_id in ["call_valid", " call_has_nonempty_text "] {
            let mut payload = json!({"input":[{
                "type":output_type,
                "namespace":"unknown_namespace",
                "name":"goal_objective",
                "call_id":call_id,
                "output":{"real":"tool result"}
            }]});
            let original = payload.clone();
            assert_eq!(
                normalize_companion_page_context("/v1/responses", &mut payload),
                0
            );
            assert_eq!(payload, original);
        }
    }
}

#[test]
fn desktop_context_normalization_leaves_unrelated_invalid_items_visible() {
    let base = json!({"type":"function_call_output","name":"codex_apps_open_page","namespace":"agent","output":"page text"});
    for (field, value) in [
        ("type", json!("function_call")),
        ("type", json!("message")),
        ("type", json!("output")),
        ("type", Value::Null),
        ("call_id", json!(123)),
        ("call_id", json!({"invalid":"id"})),
        ("call_id", json!(false)),
    ] {
        let mut item = base.clone();
        item[field] = value;
        let mut payload = json!({"input":[item]});
        let original = payload.clone();
        assert_eq!(
            normalize_companion_page_context("/v1/responses", &mut payload),
            0,
            "unexpected conversion: {original}"
        );
        assert_eq!(payload, original);
    }
    let mut payload = json!({"input":[base]});
    let original = payload.clone();
    assert_eq!(
        normalize_companion_page_context("/v1/chat/completions", &mut payload),
        0
    );
    assert_eq!(payload, original);
}
