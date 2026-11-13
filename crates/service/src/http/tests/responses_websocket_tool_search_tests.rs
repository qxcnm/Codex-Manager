use super::*;
use serde_json::json;

fn search_call(execution: &str) -> Value {
    json!({
        "type": "tool_search_call",
        "id": "ts_item_distinct_from_call_id",
        "call_id": "call_search_1",
        "status": "completed",
        "execution": execution,
        "arguments": {"query": "find project tools", "limit": 2}
    })
}

fn search_output(execution: &str) -> Value {
    json!({
        "type": "tool_search_output",
        "id": "tso_item_distinct_from_call_id",
        "call_id": "call_search_1",
        "status": "completed",
        "execution": execution,
        "tools": [{"type": "function", "name": "lookup", "parameters": {"type": "object"}}]
    })
}

#[test]
fn websocket_tool_search_rebase_restores_calls_from_item_and_terminal_events() {
    for execution in ["client", "server"] {
        let call = search_call(execution);
        let output = search_output(execution);
        for event in [
            json!({"type": "response.output_item.added", "item": call}),
            json!({"type": "response.output_item.done", "item": call}),
            json!({"type": "response.completed", "response": {"id": "resp_search", "output": [call]}}),
        ] {
            let mut cache = CompletedWsToolCallCache::default();
            cache.observe_upstream_event(&event.to_string());
            let request = json!({
                "type": "response.create", "previous_response_id": "resp_old",
                "input": [output]
            });
            let recovered = rebase_response_create_for_account_change(&request.to_string(), &cache)
                .expect("observed tool-search call should be recoverable");
            let recovered: Value = serde_json::from_str(&recovered).unwrap();
            assert!(recovered.get("previous_response_id").is_none());
            assert_eq!(recovered["input"], json!([call, output]));
        }
    }
}

#[test]
fn websocket_tool_search_reorders_existing_call_and_deduplicates_matching_outputs() {
    let call = search_call("client");
    let output = search_output("client");
    let request = json!({
        "type": "response.create", "input": [output, call]
    });
    let recovered = rebase_response_create_for_account_change(
        &request.to_string(),
        &CompletedWsToolCallCache::default(),
    )
    .expect("the matching call in current input should be ordered before its output");
    let recovered: Value = serde_json::from_str(&recovered).unwrap();
    assert_eq!(recovered["input"], json!([call, output]));

    let distinct_kind = json!({"type": "function_call_output", "call_id": "call_search_1", "output": "function result"});
    let mut later_output = output.clone();
    later_output["tools"] = json!([]);
    let mut input = json!([output, later_output, distinct_kind]);
    assert_eq!(normalize_ws_tool_call_outputs(&mut input), 1);
    assert_eq!(input, json!([output, distinct_kind]));
}

#[test]
fn websocket_tool_search_without_call_id_preserves_items_without_inventing_a_pair() {
    for execution in ["client", "server"] {
        for call_id in [None, Some(Value::Null), Some(json!(""))] {
            let mut call = search_call(execution);
            let mut output = search_output(execution);
            for item in [&mut call, &mut output] {
                match &call_id {
                    Some(call_id) => item["call_id"] = call_id.clone(),
                    None => {
                        item.as_object_mut().unwrap().remove("call_id");
                    }
                }
            }
            let mut cache = CompletedWsToolCallCache::default();
            cache.observe_upstream_event(
                &json!({"type": "response.output_item.done", "item": call}).to_string(),
            );
            assert!(
                cache.is_empty(),
                "item id must not become a tool-search call id"
            );
            let mut input = json!([call, output, output]);
            assert_eq!(normalize_ws_tool_call_outputs(&mut input), 0);
            let request = json!({"type": "response.create", "input": input});
            let recovered = rebase_response_create_for_account_change(&request.to_string(), &cache)
                .expect("nullable tool-search call ids are valid protocol items");
            let recovered: Value = serde_json::from_str(&recovered).unwrap();
            assert_eq!(recovered["input"], input);
        }
    }
}

#[test]
fn websocket_tool_search_recovery_requires_an_observed_matching_call_id() {
    let call = search_call("client");
    let mut output = search_output("client");
    output["call_id"] = call["id"].clone();
    let mut cache = CompletedWsToolCallCache::default();
    cache.observe_input(&json!([call]));
    let request = json!({"type": "response.create", "input": [output]});
    assert!(rebase_response_create_for_account_change(&request.to_string(), &cache).is_err());
    assert!(rebase_response_create_for_missing_tool_call(
        &request.to_string(),
        &cache,
        WsToolCallKind::ToolSearch,
        "ts_item_distinct_from_call_id",
    )
    .unwrap()
    .is_none());
}

#[test]
fn websocket_missing_tool_recovery_preserves_response_history_and_account_fields() {
    for (kind, call, output) in [
        (
            WsToolCallKind::Function,
            json!({"type": "function_call", "call_id": "call_search_1", "name": "lookup", "arguments": "{}"}),
            json!({"type": "function_call_output", "call_id": "call_search_1", "output": "done"}),
        ),
        (
            WsToolCallKind::Custom,
            json!({"type": "custom_tool_call", "call_id": "call_search_1", "name": "apply_patch", "input": "patch"}),
            json!({"type": "custom_tool_call_output", "call_id": "call_search_1", "output": "done"}),
        ),
        (
            WsToolCallKind::ToolSearch,
            search_call("client"),
            search_output("client"),
        ),
    ] {
        let reasoning = json!({"type": "reasoning", "id": "rs_1", "encrypted_content": "same-account-reasoning", "summary": []});
        let mut cache = CompletedWsToolCallCache::default();
        cache.observe_input(&json!([call]));
        let request = json!({
            "type": "response.create", "previous_response_id": "resp_live",
            "session_id": "session_live", "turn_state": "turn_live",
            "client_metadata": {"session_id": "session_live", "turn_metadata": "metadata_live"},
            "input": [reasoning, output]
        });
        let recovered = rebase_response_create_for_missing_tool_call(
            &request.to_string(),
            &cache,
            kind,
            "call_search_1",
        )
        .unwrap()
        .expect("observed matching call should repair the input");
        let recovered: Value = serde_json::from_str(&recovered).unwrap();
        assert_eq!(
            recovered["previous_response_id"],
            request["previous_response_id"]
        );
        assert_eq!(recovered["session_id"], request["session_id"]);
        assert_eq!(recovered["turn_state"], request["turn_state"]);
        assert_eq!(recovered["client_metadata"], request["client_metadata"]);
        assert_eq!(recovered["input"], json!([reasoning, call, output]));
        assert!(
            rebase_response_create_for_missing_tool_call(
                &recovered.to_string(),
                &cache,
                kind,
                "call_search_1",
            )
            .unwrap()
            .is_none(),
            "an unchanged request must not be retried"
        );
    }
}

#[test]
fn websocket_missing_tool_recovery_leaves_other_previous_response_outputs_untouched() {
    let call = json!({
        "type": "function_call", "call_id": "call_target", "name": "lookup", "arguments": "{}"
    });
    let target_output = json!({
        "type": "function_call_output", "call_id": "call_target", "output": "target result"
    });
    for previous_output in [
        json!({"type": "function_call_output", "call_id": "call_only_in_previous", "output": "previous result"}),
        json!({"type": "tool_search_output", "call_id": "call_only_in_previous", "status": "completed", "execution": "client", "tools": []}),
    ] {
        let unrelated_call = json!({
            "type": "function_call", "id": "legacy_other_call", "name": "other", "arguments": "{}"
        });
        let mut cache = CompletedWsToolCallCache::default();
        cache.observe_input(&json!([call]));
        let request = json!({
            "type": "response.create", "previous_response_id": "resp_live",
            "input": [unrelated_call, previous_output, target_output, previous_output]
        });
        let recovered = rebase_response_create_for_missing_tool_call(
            &request.to_string(),
            &cache,
            WsToolCallKind::Function,
            "call_target",
        )
        .unwrap()
        .expect("only the named missing call needs local recovery");
        let recovered: Value = serde_json::from_str(&recovered).unwrap();
        assert_eq!(recovered["previous_response_id"], "resp_live");
        assert_eq!(
            recovered["input"],
            json!([
                unrelated_call,
                previous_output,
                call,
                target_output,
                previous_output
            ])
        );
        assert!(
            rebase_response_create_for_account_change(&request.to_string(), &cache).is_err(),
            "a fresh account still requires every output to have an observed call"
        );
    }
}

#[test]
fn websocket_tool_search_interruption_removes_partial_calls_from_recovery() {
    let mut call = search_call("client");
    call["status"] = json!("in_progress");
    let mut cache = CompletedWsToolCallCache::default();
    cache.observe_upstream_event(
        &json!({"type": "response.output_item.added", "item": call}).to_string(),
    );
    assert!(!cache.is_empty());
    cache.observe_upstream_event(
        &json!({
            "type": "response.incomplete",
            "response": {"id": "resp_interrupted", "status": "incomplete",
                "incomplete_details": {"reason": "interrupted"}, "output": []}
        })
        .to_string(),
    );
    assert!(
        cache.is_empty(),
        "discarded tool searches must not be replayed"
    );
}
