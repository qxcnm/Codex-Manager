use super::{
    adapt_openai_responses_to_anthropic_messages, adapt_request_for_protocol,
    backfill_empty_gemini_function_response_names, gemini_function_response_output,
};
use crate::apikey_profile::{PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_GEMINI_NATIVE};
use crate::gateway::{GeminiStreamOutputMode, ResponseAdapter};
use serde_json::{json, Value};

#[test]
fn anthropic_messages_are_rewritten_to_responses() {
    let body = br#"{"model":"claude-3-7-sonnet","system":"be helpful","messages":[{"role":"user","content":"hi"}],"stream":true}"#.to_vec();

    let adapted = adapt_request_for_protocol(PROTOCOL_ANTHROPIC_NATIVE, "/v1/messages", body)
        .expect("adapt anthropic request");

    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(
        adapted.response_adapter,
        ResponseAdapter::AnthropicMessagesFromResponses
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["model"], "claude-3-7-sonnet");
    assert_eq!(payload["instructions"], "");
    assert_eq!(payload["input"][0]["type"], "message");
    assert_eq!(payload["input"][0]["role"], "developer");
    assert_eq!(payload["input"][0]["content"][0]["text"], "be helpful");
    assert_eq!(payload["input"][1]["type"], "message");
    assert_eq!(payload["input"][1]["role"], "user");
    assert_eq!(payload["stream"], true);
    assert_eq!(payload["reasoning"]["effort"], "medium");
    assert_eq!(payload["include"][0], "reasoning.encrypted_content");
    assert_eq!(payload["parallel_tool_calls"], true);
}

#[test]
fn anthropic_mid_conversation_system_messages_map_to_developer() {
    let body = serde_json::json!({
        "model": "claude-sonnet",
        "messages": [
            { "role": "user", "content": "hi" },
            { "role": "system", "content": "# MCP Server Instructions" },
            { "role": "developer", "content": "runtime note" },
            { "role": "unknown", "content": "fallback note" }
        ],
        "stream": true
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/messages?beta=true",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt anthropic request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["input"][0]["role"], "user");
    assert_eq!(payload["input"][1]["role"], "developer");
    assert_eq!(
        payload["input"][1]["content"][0]["text"],
        "# MCP Server Instructions"
    );
    assert_eq!(payload["input"][2]["role"], "developer");
    assert_eq!(payload["input"][2]["content"][0]["text"], "runtime note");
    assert_eq!(payload["input"][3]["role"], "user");
    assert_eq!(payload["input"][3]["content"][0]["text"], "fallback note");
}

#[test]
fn gemini_generate_content_is_rewritten_to_responses() {
    let body = br#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#.to_vec();

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:generateContent",
        body,
    )
    .expect("adapt gemini request");

    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiJson);
    assert_eq!(adapted.gemini_stream_output_mode, None);
    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["model"], "gemini-2.5-pro");
    assert_eq!(payload["instructions"], "");
    assert_eq!(payload["input"][0]["type"], "message");
    assert_eq!(payload["input"][0]["role"], "user");
    assert_eq!(payload["reasoning"]["effort"], "medium");
    assert_eq!(payload["include"][0], "reasoning.encrypted_content");
    assert_eq!(payload["parallel_tool_calls"], true);
    assert!(payload.get("tools").is_none());
}

#[test]
fn gemini_cli_wrapped_generate_content_rewrites_request_tools_and_history() {
    let body = serde_json::json!({
        "model": "gpt-5.4",
        "request": {
            "systemInstruction": {
                "parts": [{ "text": "use tools when writing files" }]
            },
            "contents": [
                { "role": "user", "parts": [{ "text": "write the plan" }] },
                {
                    "role": "model",
                    "parts": [{
                        "functionCall": {
                            "id": "WriteFile_1",
                            "name": "WriteFile",
                            "args": { "file_path": "plans/site.md", "content": "plan" }
                        }
                    }]
                },
                {
                    "role": "user",
                    "parts": [{
                        "functionResponse": {
                            "id": "WriteFile_1",
                            "name": "WriteFile",
                            "response": { "output": "ok" }
                        }
                    }]
                }
            ],
            "tools": [{
                "function_declarations": [{
                    "name": "WriteFile",
                    "description": "Write a file",
                    "parameters": {
                        "type": "OBJECT",
                        "properties": {
                            "file_path": { "type": "STRING" },
                            "content": { "type": "STRING" }
                        }
                    }
                }]
            }],
            "generationConfig": {
                "thinkingConfig": { "thinkingBudget": 1024 }
            }
        }
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1internal:streamGenerateContent?alt=sse",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini cli request");

    assert_eq!(adapted.path, "/v1/responses");
    assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiCliSse);
    assert_eq!(
        adapted.gemini_stream_output_mode,
        Some(GeminiStreamOutputMode::Sse)
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["model"], "gpt-5.4");
    assert_eq!(payload["input"][0]["role"], "developer");
    assert_eq!(payload["input"][1]["role"], "user");
    assert_eq!(payload["input"][2]["type"], "function_call");
    assert_eq!(payload["input"][2]["name"], "WriteFile");
    assert_eq!(payload["input"][3]["type"], "function_call_output");
    assert_eq!(
        payload["input"][3]["call_id"],
        payload["input"][2]["call_id"]
    );
    assert_eq!(payload["tools"][0]["type"], "function");
    assert_eq!(payload["tools"][0]["name"], "WriteFile");
    assert_eq!(
        payload["tools"][0]["parameters"]["additionalProperties"],
        false
    );
    assert_eq!(payload["tools"][0]["parameters"]["type"], "object");
    assert_eq!(
        payload["tools"][0]["parameters"]["properties"]["file_path"]["type"],
        "string"
    );
    assert_eq!(payload["tools"].as_array().expect("tools").len(), 1);
    assert_eq!(payload["tool_choice"], "auto");
    assert_eq!(payload["reasoning"]["effort"], "low");
}

#[test]
fn gemini_stream_generate_content_uses_sse_stream_mode_without_alt_sse() {
    let body = br#"{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}"#.to_vec();

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:streamGenerateContent",
        body,
    )
    .expect("adapt gemini request");

    assert_eq!(
        adapted.gemini_stream_output_mode,
        Some(GeminiStreamOutputMode::Sse)
    );
    assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiSse);
}

#[test]
fn gemini_cli_internal_stream_defaults_to_sse_wrapped_mode() {
    let body =
        br#"{"model":"gpt-5.4","request":{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}}"#
            .to_vec();

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1internal:streamGenerateContent",
        body,
    )
    .expect("adapt gemini cli request");

    assert_eq!(
        adapted.gemini_stream_output_mode,
        Some(GeminiStreamOutputMode::Sse)
    );
    assert_eq!(adapted.response_adapter, ResponseAdapter::GeminiCliSse);
}

#[test]
fn anthropic_tool_use_and_result_are_rewritten_as_responses_tool_items() {
    let long_tool_name = "mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore";
    let body = br#"{
        "model":"claude-3-7-sonnet",
        "messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","input":{"q":"hi"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":[{"type":"text","text":"ok"}]}]}
        ],
        "tools":[{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","input_schema":{"type":"object","properties":{"q":{"type":"string"}}}}]
    }"#.to_vec();

    let adapted = adapt_request_for_protocol(PROTOCOL_ANTHROPIC_NATIVE, "/v1/messages", body)
        .expect("adapt anthropic request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["input"][0]["type"], "function_call");
    assert_eq!(payload["input"][0]["call_id"], "toolu_1");
    assert_eq!(payload["input"][1]["type"], "function_call_output");
    assert_eq!(payload["input"][1]["call_id"], "toolu_1");
    assert_eq!(payload["tools"][0]["type"], "function");
    assert_ne!(payload["tools"][0]["name"], long_tool_name);
    assert_eq!(payload["tool_choice"], "auto");
    assert_eq!(
        adapted
            .tool_name_restore_map
            .get(payload["tools"][0]["name"].as_str().unwrap_or("")),
        Some(&long_tool_name.to_string())
    );
}

#[test]
fn gemini_function_call_and_response_are_rewritten_as_responses_tool_items() {
    let long_tool_name = "mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore";
    let body = br#"{
        "contents":[
            {"role":"model","parts":[{"functionCall":{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","args":{"q":"hi"}}}]},
            {"role":"user","parts":[{"functionResponse":{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","response":{"result":"ok"}}}]}
        ],
        "tools":[{"functionDeclarations":[{"name":"mcp__context7__query_docs_with_a_very_long_suffix_that_exceeds_sixty_four_chars_for_restore","parameters":{"type":"object","properties":{"q":{"type":"string"}}}}]}
        ]
    }"#.to_vec();

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:generateContent",
        body,
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["input"][0]["type"], "function_call");
    assert_eq!(payload["input"][1]["type"], "function_call_output");
    assert_eq!(
        payload["input"][0]["call_id"],
        payload["input"][1]["call_id"]
    );
    assert_eq!(payload["tools"][0]["type"], "function");
    assert_ne!(payload["tools"][0]["name"], long_tool_name);
    assert_eq!(payload["tools"].as_array().expect("tools").len(), 1);
    assert_eq!(payload["tool_choice"], "auto");
    assert_eq!(
        adapted
            .tool_name_restore_map
            .get(payload["tools"][0]["name"].as_str().unwrap_or("")),
        Some(&long_tool_name.to_string())
    );
}

#[test]
fn gemini_function_call_id_pairs_exactly_with_function_response_id() {
    let body = serde_json::json!({
        "contents": [
            {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "id": "call_exact_from_gemini",
                        "name": "ReadFolder",
                        "args": { "path": "." }
                    }
                }]
            },
            {
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "id": "call_exact_from_gemini",
                        "name": "ReadFolder",
                        "response": { "output": "Directory is empty." }
                    }
                }]
            }
        ]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gpt-5.4:streamGenerateContent?alt=sse",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["input"][0]["type"], "function_call");
    assert_eq!(payload["input"][1]["type"], "function_call_output");
    assert_eq!(
        payload["input"][1]["call_id"],
        payload["input"][0]["call_id"]
    );
    assert_eq!(payload["input"][0]["call_id"], "call_exact_from_gemini");
    assert_eq!(payload["input"][1]["output"], "Directory is empty.");
}

#[test]
fn gemini_function_response_without_id_uses_fifo_for_user_and_function_roles() {
    let body = serde_json::json!({
        "contents": [
            {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "id": "call_user_role",
                        "name": "ReadFile",
                        "args": { "path": "a.md" }
                    }
                }]
            },
            {
                "role": "user",
                "parts": [{
                    "functionResponse": {
                        "name": "",
                        "response": { "result": "A" }
                    }
                }]
            },
            {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "id": "call_function_role",
                        "name": "ReadFile",
                        "args": { "path": "b.md" }
                    }
                }]
            },
            {
                "role": "function",
                "parts": [{
                    "functionResponse": {
                        "response": { "result": "B" }
                    }
                }]
            }
        ]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1internal:streamGenerateContent",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["input"][0]["call_id"], "call_user_role");
    assert_eq!(payload["input"][1]["type"], "function_call_output");
    assert_eq!(payload["input"][1]["call_id"], "call_user_role");
    assert_eq!(payload["input"][2]["call_id"], "call_function_role");
    assert_eq!(payload["input"][3]["type"], "function_call_output");
    assert_eq!(payload["input"][3]["call_id"], "call_function_role");
    assert_eq!(payload["input"][1]["output"], "A");
    assert_eq!(payload["input"][3]["output"], "B");
}

#[test]
fn gemini_function_response_output_prefers_output_and_error_fields() {
    let output_value = json!({
        "response": {
            "output": "Directory is empty."
        }
    });
    let error_value = json!({
        "response": {
            "error": {
                "message": "params must have required property 'command'"
            }
        }
    });

    let output =
        gemini_function_response_output(output_value.as_object().expect("functionResponse object"));
    let error =
        gemini_function_response_output(error_value.as_object().expect("functionResponse object"));

    assert_eq!(output, Value::String("Directory is empty.".to_string()));
    assert_eq!(
        error,
        Value::String("{\"message\":\"params must have required property 'command'\"}".to_string())
    );
}

#[test]
fn gemini_function_response_names_are_backfilled_from_previous_model_call() {
    let contents = json!([
        {
            "role": "model",
            "parts": [{
                "functionCall": {
                    "name": "ReadFile",
                    "args": { "path": "a.md" }
                }
            }]
        },
        {
            "role": "user",
            "parts": [{
                "functionResponse": {
                    "name": "",
                    "response": { "output": "A" }
                }
            }]
        },
        {
            "role": "model",
            "parts": [{
                "functionCall": {
                    "name": "ReadFile",
                    "args": { "path": "b.md" }
                }
            }]
        },
        {
            "role": "function",
            "parts": [{
                "functionResponse": {
                    "response": { "output": "B" }
                }
            }]
        }
    ]);

    let backfilled = backfill_empty_gemini_function_response_names(&contents);
    assert_eq!(
        backfilled[1]["parts"][0]["functionResponse"]["name"],
        "ReadFile"
    );
    assert_eq!(
        backfilled[3]["parts"][0]["functionResponse"]["name"],
        "ReadFile"
    );
}

#[test]
fn anthropic_disable_parallel_tool_use_and_unique_short_names_follow_cpa_shape() {
    let tool_a =
        "mcp__workspace__ThisIsAnExtremelyLongToolNameThatNeedsToBeShortenedForCodexRouteAlpha";
    let tool_b =
        "mcp__workspace__ThisIsAnExtremelyLongToolNameThatNeedsToBeShortenedForCodexRouteBeta";
    let body = serde_json::json!({
        "model": "claude-sonnet",
        "tool_choice": {
            "type": "tool",
            "name": tool_b,
            "disable_parallel_tool_use": true
        },
        "tools": [
            { "name": tool_a, "input_schema": {"description":"x"} },
            { "name": tool_b, "input_schema": {"$schema":"http://json-schema.org/draft-07/schema#"} }
        ],
        "messages": [{
            "role": "assistant",
            "content": [{ "type": "tool_use", "id": "toolu_1", "name": tool_b, "input": {} }]
        }]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/messages",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt anthropic request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["parallel_tool_calls"], false);
    assert_ne!(payload["tools"][0]["name"], payload["tools"][1]["name"]);
    assert_eq!(payload["tool_choice"], "auto");
    assert_eq!(payload["input"][0]["name"], payload["tools"][1]["name"]);
    assert_eq!(payload["tools"][0]["parameters"]["type"], "object");
    assert!(payload["tools"][0]["parameters"]["properties"].is_object());
    assert!(payload["tools"][1]["parameters"].get("$schema").is_none());
}

#[test]
fn gemini_any_mode_allowed_function_name_maps_to_specific_function_tool_choice() {
    let long_tool_name = "mcp__workspace__ReadFileLongLongLongLongLongLongLongLongLongLong";
    let body = serde_json::json!({
        "tools": [{
            "functionDeclarations": [{
                "name": long_tool_name,
                "parameters": {"description":"x"}
            }]
        }],
        "toolConfig": {
            "functionCallingConfig": {
                "mode": "ANY",
                "allowedFunctionNames": [long_tool_name]
            }
        },
        "contents": [{"role":"user","parts":[{"text":"hi"}]}]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:generateContent",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["tool_choice"]["type"], "function");
    assert_eq!(payload["tool_choice"]["name"], payload["tools"][0]["name"]);
    assert_eq!(payload["tools"][0]["parameters"]["description"], "x");
    assert_eq!(
        payload["tools"][0]["parameters"]["additionalProperties"],
        false
    );
    assert!(payload["tools"][0]["parameters"].get("type").is_none());
    assert!(payload["tools"][0]["parameters"]
        .get("properties")
        .is_none());
    assert_eq!(payload["tools"].as_array().expect("tools").len(), 1);
}

#[test]
fn gemini_any_mode_without_single_allowed_function_requires_tool_call() {
    let body = serde_json::json!({
        "tools": [{
            "functionDeclarations": [{
                "name": "WriteFile",
                "parameters": {"type":"object","properties":{}}
            }]
        }],
        "toolConfig": {
            "functionCallingConfig": { "mode": "ANY" }
        },
        "contents": [{"role":"user","parts":[{"text":"write it"}]}]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1internal:streamGenerateContent",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["tool_choice"], "required");
}

#[test]
fn gemini_none_mode_disables_tool_choice() {
    let body = serde_json::json!({
        "tools": [{
            "functionDeclarations": [{
                "name": "WriteFile",
                "parameters": {"type":"object","properties":{}}
            }]
        }],
        "toolConfig": {
            "functionCallingConfig": { "mode": "NONE" }
        },
        "contents": [{"role":"user","parts":[{"text":"do not use tools"}]}]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1internal:streamGenerateContent",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["tool_choice"], "none");
}

#[test]
fn anthropic_enabled_thinking_adds_reasoning_and_include_only_when_explicit() {
    let body = serde_json::json!({
        "model": "claude-sonnet",
        "thinking": {
            "type": "enabled",
            "budget_tokens": 4096
        },
        "messages": [{"role":"user","content":"hi"}]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/messages",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt anthropic request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["reasoning"]["effort"], "medium");
    assert_eq!(payload["include"][0], "reasoning.encrypted_content");
}

#[test]
fn responses_request_rewrites_to_anthropic_messages_for_claude_upstream() {
    let body = json!({
        "model": "gpt-5.3",
        "instructions": "be direct",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            },
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{\"path\":\"/tmp/a\"}"
            },
            {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "done"
            }
        ],
        "tools": [{
            "type": "function",
            "name": "read_file",
            "description": "Read a file",
            "parameters": {
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }
        }],
        "reasoning": { "effort": "high" },
        "stream": true
    });

    let adapted = adapt_openai_responses_to_anthropic_messages(
        serde_json::to_vec(&body).expect("body").as_slice(),
        Some("deepseek/deepseek-v4-pro"),
    )
    .expect("adapt responses request");
    let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

    assert_eq!(payload["model"], "deepseek/deepseek-v4-pro");
    assert_eq!(payload["system"], "be direct");
    assert_eq!(payload["stream"], true);
    assert_eq!(payload["messages"][0]["role"], "user");
    assert_eq!(payload["messages"][0]["content"][0]["type"], "text");
    assert_eq!(payload["messages"][0]["content"][0]["text"], "hello");
    assert_eq!(payload["messages"][1]["role"], "assistant");
    assert_eq!(payload["messages"][1]["content"][0]["type"], "tool_use");
    assert_eq!(payload["messages"][1]["content"][0]["id"], "call_1");
    assert_eq!(
        payload["messages"][1]["content"][0]["input"]["path"],
        "/tmp/a"
    );
    assert_eq!(payload["messages"][2]["role"], "user");
    assert_eq!(payload["messages"][2]["content"][0]["type"], "tool_result");
    assert_eq!(payload["tools"][0]["name"], "read_file");
    assert_eq!(payload["thinking"]["type"], "enabled");
}

#[test]
fn responses_web_search_tool_maps_to_kiro_anthropic_shape() {
    let body = json!({
        "model": "kiro/claude-sonnet-4.5",
        "input": "latest Rust release",
        "tools": [{ "type": "web_search", "max_uses": 3 }],
        "stream": true
    });

    let adapted = adapt_openai_responses_to_anthropic_messages(
        serde_json::to_vec(&body).expect("body").as_slice(),
        Some("claude-sonnet-4.5"),
    )
    .expect("adapt responses request");
    let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

    assert_eq!(payload["tools"][0]["type"], "web_search_20250305");
    assert_eq!(payload["tools"][0]["name"], "web_search");
    assert_eq!(payload["tools"][0]["max_uses"], 3);
}

#[test]
fn responses_anthropic_bridge_preserves_tool_choice_parallel_policy() {
    let body = json!({
        "model": "gpt-5.3",
        "input": "read a file",
        "tools": [{
            "type": "function",
            "name": "read_file",
            "parameters": {
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }
        }],
        "tool_choice": {
            "type": "function",
            "function": { "name": "read_file" }
        },
        "parallel_tool_calls": false
    });

    let adapted = adapt_openai_responses_to_anthropic_messages(
        serde_json::to_vec(&body).expect("body").as_slice(),
        Some("claude-sonnet-4"),
    )
    .expect("adapt responses request");
    let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

    assert_eq!(payload["tool_choice"]["type"], "tool");
    assert_eq!(payload["tool_choice"]["name"], "read_file");
    assert_eq!(payload["tool_choice"]["disable_parallel_tool_use"], true);
}

#[test]
fn responses_reasoning_budget_stays_below_max_tokens() {
    let body = json!({
        "model": "gpt-5.3",
        "max_output_tokens": 2048,
        "input": "think carefully",
        "reasoning": { "effort": "high" }
    });

    let adapted = adapt_openai_responses_to_anthropic_messages(
        serde_json::to_vec(&body).expect("body").as_slice(),
        Some("claude-sonnet-4"),
    )
    .expect("adapt responses request");
    let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

    let max_tokens = payload["max_tokens"].as_i64().expect("max tokens");
    let budget_tokens = payload["thinking"]["budget_tokens"]
        .as_i64()
        .expect("budget tokens");
    assert!(
        budget_tokens < max_tokens,
        "Anthropic thinking budget_tokens must be lower than max_tokens"
    );
}

#[test]
fn responses_request_drops_images_for_deepseek_anthropic_bridge() {
    let body = json!({
        "model": "gpt-5.3",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "describe this" },
                { "type": "input_image", "image_url": "data:image/png;base64,aGVsbG8=" }
            ]
        }]
    });

    let adapted = adapt_openai_responses_to_anthropic_messages(
        serde_json::to_vec(&body).expect("body").as_slice(),
        Some("deepseek-v4-pro"),
    )
    .expect("adapt responses request");
    let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");

    assert_eq!(
        payload["messages"][0]["content"].as_array().unwrap().len(),
        1
    );
    assert_eq!(payload["messages"][0]["content"][0]["type"], "text");
}

#[test]
fn responses_request_preserves_base64_images_for_claude_upstream() {
    let body = json!({
        "model": "kiro/claude-sonnet-4.6",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "describe this" },
                { "type": "input_image", "image_url": "data:image/png;base64,aGVsbG8=" }
            ]
        }]
    });

    let adapted = adapt_openai_responses_to_anthropic_messages(
        serde_json::to_vec(&body).expect("body").as_slice(),
        Some("claude-sonnet-4.6"),
    )
    .expect("adapt responses request");
    let payload: Value = serde_json::from_slice(&adapted).expect("parse adapted body");
    let image = &payload["messages"][0]["content"][1];

    assert_eq!(image["type"], "image");
    assert_eq!(image["source"]["type"], "base64");
    assert_eq!(image["source"]["media_type"], "image/png");
    assert_eq!(image["source"]["data"], "aGVsbG8=");
}

#[test]
fn gemini_thinking_config_adds_reasoning_and_include_only_when_explicit() {
    let body = serde_json::json!({
        "contents": [{"role":"user","parts":[{"text":"hi"}]}],
        "generationConfig": {
            "thinkingConfig": {
                "thinkingBudget": 2048
            }
        }
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:generateContent",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["reasoning"]["effort"], "medium");
    assert_eq!(payload["include"][0], "reasoning.encrypted_content");
}

#[test]
fn anthropic_disabled_thinking_maps_to_none_effort_like_cpa() {
    let body = serde_json::json!({
        "model": "claude-sonnet",
        "thinking": { "type": "disabled" },
        "messages": [{"role":"user","content":"hi"}]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/messages",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt anthropic request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["reasoning"]["effort"], "none");
}

#[test]
fn gemini_assistant_text_maps_to_output_text_like_cpa() {
    let body = serde_json::json!({
        "contents": [{"role":"model","parts":[{"text":"hello"}]}]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:generateContent",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["input"][0]["role"], "assistant");
    assert_eq!(payload["input"][0]["content"][0]["type"], "output_text");
}

#[test]
fn gemini_thought_text_maps_to_reasoning_history_not_visible_output() {
    let body = serde_json::json!({
        "contents": [{
            "role":"model",
            "parts":[{
                "text":"internal plan",
                "thought":true,
                "thoughtSignature":"sig_reasoning"
            }]
        }]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:generateContent",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt gemini request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["input"][0]["type"], "reasoning");
    assert_eq!(payload["input"][0]["summary"][0]["text"], "internal plan");
    assert_eq!(payload["input"][0]["encrypted_content"], "sig_reasoning");
}

#[test]
fn anthropic_web_search_tool_maps_to_codex_web_search_like_cpa() {
    let body = serde_json::json!({
        "model": "claude-sonnet",
        "messages": [{"role":"user","content":"hi"}],
        "tools": [{ "type": "web_search_20250305", "name": "search" }]
    });

    let adapted = adapt_request_for_protocol(
        PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/messages",
        serde_json::to_vec(&body).expect("body"),
    )
    .expect("adapt anthropic request");

    let payload: serde_json::Value =
        serde_json::from_slice(&adapted.body).expect("parse adapted body");
    assert_eq!(payload["tools"][0]["type"], "web_search");
    assert!(payload["tools"][0].get("name").is_none());
}
