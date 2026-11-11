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
fn companion_page_normalization_leaves_other_invalid_tool_outputs_visible() {
    let base = json!({"type":"function_call_output","name":"codex_apps_open_page","namespace":"agent","output":"page text"});
    for (field, value) in [
        ("name", json!("other_tool")),
        ("name", json!("codex_apps_open_page_extra")),
        ("name", json!("codex_apps_selected_page_part_0_of_1")),
        ("name", json!("codex_apps_selected_page_part_1_of_0")),
        ("name", json!("codex_apps_selected_page_part_1_of_1_extra")),
        ("name", json!("codex_apps_selected_page_part_x_of_1")),
        ("namespace", json!("unrelated")),
        ("type", json!("custom_tool_call_output")),
        ("call_id", json!(123)),
        ("output", json!({"text":"page text"})),
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
