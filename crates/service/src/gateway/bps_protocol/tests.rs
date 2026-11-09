use super::*;
use serde_json::json;

fn convert(body: Value) -> (Value, Bridge) {
    let (body, bridge) =
        prepare(&serde_json::to_vec(&body).unwrap(), "account-a/session-a").unwrap();
    (serde_json::from_slice(&body).unwrap(), bridge)
}

fn error(body: Value) -> String {
    prepare(&serde_json::to_vec(&body).unwrap(), "test")
        .err()
        .expect("must reject")
}

fn native_call(code: Value) -> Value {
    json!({"type":"function_call","id":"fc_one","call_id":"call_one","name":"run_officejs","arguments":json!({"code":code.to_string(),"summary":"request","extended_summary":"request","destructive":false,"references":[]}).to_string()})
}

fn completed(output: Value) -> Value {
    json!({"type":"response.completed","response":{"id":"resp_one","status":"completed","model":"upstream-reported-model","reasoning":{"effort":"high"},"usage":{"input_tokens":12,"output_tokens":8},"output":output}})
}

fn with_function() -> Bridge {
    convert(json!({"model":"requested-model","input":"find it","reasoning":{"effort":"xhigh"},"tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}]})).1
}

#[test]
fn request_selects_original_model_and_explicit_effort_without_claiming_actual_quality() {
    let (body, mut bridge) = convert(
        json!({"model":"requested-model","instructions":"Original instructions","input":"Hello","reasoning":{"effort":"ultra"},"stream":false}),
    );
    assert_eq!(body["model"], "requested-model");
    assert_eq!(body["model_selection"], "explicit");
    assert_eq!(body["reasoning_effort"], "xhigh");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(
        body["input"][0]["content"][0]["text"],
        "Original instructions"
    );
    assert_eq!(body["input"][2]["content"][0]["text"], "Hello");
    let returned = bridge.translate_event(completed(json!([]))).unwrap();
    assert_eq!(returned[0]["response"]["model"], "upstream-reported-model");
    assert_eq!(returned[0]["response"]["reasoning"]["effort"], "high");
    assert_eq!(returned[0]["response"]["usage"]["output_tokens"], 8);
}

#[test]
fn historical_effort_updates_are_used_only_when_current_request_is_unspecified() {
    let mut request = json!({"model":"m","input":[{"type":"configuration_update","reasoning":{"effort":"low"}},{"type":"configuration_update","reasoning":{"effort":"high"}},{"role":"user","content":"hello"}]});
    assert_eq!(convert(request.clone()).0["reasoning_effort"], "high");
    request["reasoning"] = json!({"effort":"medium"});
    assert_eq!(convert(request).0["reasoning_effort"], "medium");
    for (given, expected) in [
        ("max", "xhigh"),
        ("none", "low"),
        ("minimal", "low"),
        ("high", "high"),
        ("low", "low"),
    ] {
        assert_eq!(
            convert(json!({"model":"m","input":"hello","reasoning":{"effort":given}})).0
                ["reasoning_effort"],
            expected
        );
    }
}

#[test]
fn unsupported_constraints_are_rejected_before_sending() {
    for (field, value) in [
        ("previous_response_id", json!("r1")),
        ("conversation", json!("c1")),
        ("max_output_tokens", json!(20)),
        ("temperature", json!(0.2)),
        ("background", json!(true)),
        ("service_tier", json!("priority")),
        ("text", json!({"format":{"type":"json_schema","schema":{}}})),
    ] {
        let mut request = json!({"model":"m","input":"hi"});
        request[field] = value;
        assert!(error(request).contains("BPS"));
    }
}

#[test]
fn hosted_tools_are_disclosed_but_never_executed_or_forced() {
    let (request, mut bridge) =
        convert(json!({"model":"m","input":"hello","tools":[{"type":"web_search"}]}));
    assert!(!request["input"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Client tool catalog"));
    assert!(request["input"][0]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("web_search"));
    let events = bridge.translate_event(completed(json!([]))).unwrap();
    assert_eq!(
        events[0]["response"]["metadata"]["codexmanager_bps_omitted_tools"],
        "web_search"
    );
    assert!(error(json!({"model":"m","input":"hi","tools":[{"type":"web_search"}],"tool_choice":{"type":"web_search"}})).contains("tool_choice"));
}

#[test]
fn function_calls_are_buffered_then_emitted_with_original_identity_and_no_native_tool() {
    let mut bridge = with_function();
    assert!(bridge.translate_event(json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_one","name":"run_officejs","arguments":""}})).unwrap().is_empty());
    assert!(bridge.translate_event(json!({"type":"response.function_call_arguments.delta","item_id":"fc_one","delta":"partial"})).unwrap().is_empty());
    let args = json!({"query":"line 1\n中文 \"quoted\" C:\\temp"});
    let result = bridge
        .translate_event(completed(json!([native_call(
            json!({"tool":"lookup","arguments":args})
        )])))
        .unwrap();
    assert_eq!(result.len(), 5);
    assert_eq!(result[0]["item"]["name"], "lookup");
    assert_eq!(result[0]["item"]["arguments"], "");
    assert_eq!(result[3]["item"]["call_id"], "call_one");
    assert_eq!(
        serde_json::from_str::<Value>(result[3]["item"]["arguments"].as_str().unwrap()).unwrap(),
        args
    );
    assert_eq!(result[4]["response"]["output"][0]["name"], "lookup");
    assert_eq!(result[4]["response"]["parallel_tool_calls"], false);
    for (index, event) in result.iter().enumerate() {
        assert_eq!(event["sequence_number"], index as u64);
    }
}

#[test]
fn custom_namespaced_tool_round_trip_preserves_raw_input_and_output() {
    let body = json!({"model":"m","input":"apply it","tools":[{"type":"namespace","name":"functions","tools":[{"type":"custom","name":"apply_patch","description":"edit a file"}]}]});
    let (_, mut bridge) = convert(body.clone());
    let patch = "*** Begin Patch\n*** Add File: 中文.txt\n+test\\value\n*** End Patch";
    let events = bridge
        .translate_event(completed(json!([native_call(
            json!({"tool":"functions.apply_patch","input":patch})
        )])))
        .unwrap();
    let call = events.last().unwrap()["response"]["output"][0].clone();
    assert_eq!(call["type"], "custom_tool_call");
    assert_eq!(call["namespace"], "functions");
    assert_eq!(call["name"], "apply_patch");
    assert_eq!(call["input"], patch);
    let mut next = body;
    next["input"] = json!([{"role":"user","content":"apply it"},call,{"type":"custom_tool_call_output","call_id":"call_one","output":"success"}]);
    let (request, _) = convert(next);
    let input = request["input"].as_array().unwrap();
    let replay = &input[input.len() - 2];
    assert_eq!(replay["name"], "run_officejs");
    let arguments: Value = serde_json::from_str(replay["arguments"].as_str().unwrap()).unwrap();
    let envelope: Value = serde_json::from_str(arguments["code"].as_str().unwrap()).unwrap();
    assert_eq!(envelope["input"], patch);
    assert_eq!(input.last().unwrap()["output"], "success");
    assert_eq!(input.last().unwrap()["call_id"], "call_one");
}

#[test]
fn incomplete_missing_and_invalid_tools_never_escape_to_the_client() {
    let mut bridge = with_function();
    bridge.translate_event(json!({"type":"response.output_item.added","item":{"id":"fc_one","type":"function_call"}})).unwrap();
    assert!(bridge
        .translate_event(completed(json!([])))
        .unwrap_err()
        .contains("omitted"));
    let mut bridge = with_function();
    let failure = json!({"type":"response.failed","response":{"status":"failed","output":[native_call(json!({"tool":"lookup","arguments":{}}))]}});
    let events = bridge.translate_event(failure).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["response"]["output"], json!([]));
    for code in [
        json!({"tool":"undeclared","arguments":{}}),
        json!({"tool":"lookup","arguments":"not-an-object"}),
    ] {
        assert!(with_function()
            .translate_event(completed(json!([native_call(code)])))
            .is_err());
    }
}

#[test]
fn required_and_forbidden_tool_choices_are_enforced_at_completion() {
    let mut request = json!({"model":"m","input":"hello","tools":[{"type":"function","name":"lookup"}],"tool_choice":"required"});
    assert!(convert(request.clone())
        .1
        .translate_event(completed(json!([])))
        .is_err());
    request["tool_choice"] = json!({"type":"function","name":"lookup"});
    assert!(convert(request.clone())
        .1
        .translate_event(completed(json!([native_call(
            json!({"tool":"lookup","arguments":{}})
        )])))
        .is_ok());
    request["tool_choice"] = json!("none");
    assert!(convert(request)
        .1
        .translate_event(completed(json!([native_call(
            json!({"tool":"lookup","arguments":{}})
        )])))
        .is_err());
}

#[test]
fn opaque_history_and_images_are_preserved_without_plaintext_substitution() {
    let agent = json!({"type":"agent_message","id":"a1","author":{"role":"assistant"},"content":[{"type":"input_text","text":"visible"},{"type":"encrypted_content","encrypted_content":"opaque-agent-state"}]});
    let (request, _) = convert(
        json!({"model":"m","input":[{"type":"reasoning","encrypted_content":"opaque-reasoning","summary":[{"text":"private"}]},agent,{"role":"user","content":[{"type":"input_image","image_url":"https://example.org/a.png","detail":"high"}]}]}),
    );
    assert_eq!(request["input"][1]["encrypted_content"], "opaque-reasoning");
    assert_eq!(request["input"][1]["summary"], json!([]));
    assert_eq!(request["input"][2], agent);
    assert_eq!(request["input"][3]["content"][0]["detail"], "high");
    for image in [
        "data:image/png;base64,secret",
        "http://example.org/a.png",
        "https://user:password@example.org/a.png",
    ] {
        let message = error(
            json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":image}]}]}),
        );
        assert!(!message.contains("secret") && !message.contains("password"));
    }
}

#[test]
fn history_rejects_missing_or_duplicate_tool_identity() {
    assert!(error(json!({"model":"m","input":[{"type":"function_call_output","call_id":"missing","output":"result"}]})).contains("original call"));
    let call = json!({"type":"function_call","call_id":"one","name":"lookup","arguments":"{}"});
    assert!(error(json!({"model":"m","input":[call,call]})).contains("duplicate"));
}

#[test]
fn transport_controls_are_filtered_and_terminal_state_is_strict() {
    let mut bridge = with_function();
    for kind in [
        "basispoints.response.metadata",
        "basispoints.response.heartbeat",
        "basispoints.response.resume_token",
        "basispoints.response.upstream_sent",
    ] {
        assert!(bridge
            .translate_event(json!({"type":kind}))
            .unwrap()
            .is_empty());
    }
    let delta = json!({"type":"response.output_text.delta","delta":"hello","basispoints_replay_cursor":"private-cursor"});
    let events = bridge.translate_event(delta).unwrap();
    assert_eq!(events[0]["delta"], "hello");
    assert!(events[0].get("basispoints_replay_cursor").is_none());
    bridge.translate_event(completed(json!([]))).unwrap();
    assert!(bridge
        .translate_event(json!({"type":"response.output_text.delta","delta":"late"}))
        .is_err());
}

#[test]
fn conversation_ids_and_prompt_cache_are_scoped_to_account_and_session() {
    let source = json!({"model":"m","input":"hi","prompt_cache_key":"same"});
    let raw = serde_json::to_vec(&source).unwrap();
    let first: Value =
        serde_json::from_slice(&prepare(&raw, "account-a/session-a").unwrap().0).unwrap();
    let repeat: Value =
        serde_json::from_slice(&prepare(&raw, "account-a/session-a").unwrap().0).unwrap();
    let second: Value =
        serde_json::from_slice(&prepare(&raw, "account-b/session-b").unwrap().0).unwrap();
    assert_eq!(first["metadata"], repeat["metadata"]);
    assert_ne!(first["metadata"]["task_id"], second["metadata"]["task_id"]);
    assert_ne!(first["prompt_cache_key"], second["prompt_cache_key"]);
}

#[test]
fn current_tool_schema_wins_and_responses_include_is_not_sent_to_bps() {
    let (request, bridge) = convert(json!({
        "model":"m", "include":["reasoning.encrypted_content"],
        "tools":[{"type":"function","name":"lookup","description":"current schema","parameters":{"type":"object","properties":{"newField":{"type":"string"}}}}],
        "input":[{"type":"additional_tools","tools":[{"type":"function","name":"lookup","description":"old schema"}]},{"role":"user","content":"continue"}]
    }));
    assert!(request.get("include").is_none());
    assert_eq!(
        bridge.tools["lookup"].declaration["description"],
        "current schema"
    );
    assert!(
        bridge.tools["lookup"].declaration["parameters"]["properties"]
            .get("newField")
            .is_some()
    );
}
