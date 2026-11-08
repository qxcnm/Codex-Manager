//! Loss-aware Responses request conversion for Chat Completions upstreams.
use serde_json::{json, Map, Value};
use std::collections::HashSet;

pub(crate) fn adapt_openai_responses_to_chat_completions(
    body: &[u8],
    model_override: Option<&str>,
) -> Result<Vec<u8>, String> {
    let payload: Value = serde_json::from_slice(body)
        .map_err(|error| format!("invalid Responses request JSON: {error}"))?;
    let input = payload
        .as_object()
        .ok_or("Responses request must be an object")?;
    for name in ["previous_response_id", "conversation"] {
        if input
            .get(name)
            .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
        {
            return Err(format!(
                "Chat Completions cannot resolve {name}; supply the complete input history"
            ));
        }
    }
    let model = model_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            input
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .ok_or("Responses model is required for Chat Completions")?;
    let stream = match input.get("stream") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("Responses stream must be a boolean".into()),
    };
    let mut output = Map::new();
    output.insert("model".into(), Value::String(model.into()));
    output.insert("stream".into(), Value::Bool(stream));
    let mut messages = Vec::new();
    if let Some(instructions) = input.get("instructions").filter(|value| !value.is_null()) {
        let instructions = instructions
            .as_str()
            .ok_or("Responses instructions must be a string")?;
        if !instructions.is_empty() {
            messages.push(json!({"role":"system","content":instructions}));
        }
    }
    let mut calls = HashSet::new();
    let mut results = HashSet::new();
    match input.get("input") {
        None | Some(Value::Null) => {}
        Some(Value::String(text)) => messages.push(json!({"role":"user","content":text})),
        Some(Value::Array(items)) => {
            for item in items {
                append_input_item(item, &mut messages, &mut calls, &mut results)?;
            }
        }
        Some(_) => return Err("Responses input must be text or an array of input items".into()),
    }
    if messages.is_empty() {
        // Chat requires a message even when Responses input is intentionally empty.
        messages.push(json!({"role":"user","content":""}));
    }
    output.insert("messages".into(), Value::Array(messages));

    if let Some(tools) = input.get("tools").filter(|value| !value.is_null()) {
        let tools = tools.as_array().ok_or("Responses tools must be an array")?;
        let mut names = HashSet::new();
        let mut converted = Vec::with_capacity(tools.len());
        for tool in tools {
            let tool = tool.as_object().ok_or("Responses tool must be an object")?;
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err(format!(
                    "Chat Completions cannot represent Responses tool type {}",
                    tool.get("type")
                        .and_then(Value::as_str)
                        .unwrap_or("<missing>")
                ));
            }
            let mut function = match tool.get("function") {
                Some(function) => function
                    .as_object()
                    .ok_or("function tool schema must be an object")?
                    .clone(),
                None => {
                    let mut function = tool.clone();
                    function.remove("type");
                    function
                }
            };
            let name = required_text(&function, "name", "function tool")?.to_string();
            if !names.insert(name) {
                return Err("duplicate function tool name".into());
            }
            if function
                .get("parameters")
                .is_some_and(|schema| !schema.is_object())
            {
                return Err("function tool parameters must be a JSON schema object".into());
            }
            // Retain description, the entire parameters schema, strict and any
            // vendor function-schema fields rather than rebuilding a lossy schema.
            function.remove("type");
            converted.push(json!({"type":"function","function":function}));
        }
        output.insert("tools".into(), Value::Array(converted));
    }
    if let Some(choice) = input.get("tool_choice").filter(|value| !value.is_null()) {
        output.insert("tool_choice".into(), convert_tool_choice(choice)?);
    }
    if let Some(value) = input.get("parallel_tool_calls") {
        if !value.is_boolean() {
            return Err("parallel_tool_calls must be a boolean".into());
        }
        output.insert("parallel_tool_calls".into(), value.clone());
    }
    if let Some(limit) = input
        .get("max_output_tokens")
        .or_else(|| input.get("max_completion_tokens"))
        .or_else(|| input.get("max_tokens"))
    {
        if limit.as_u64().is_none_or(|limit| limit == 0) {
            return Err("max_output_tokens must be a positive integer".into());
        }
        output.insert("max_completion_tokens".into(), limit.clone());
    }
    for name in [
        "temperature",
        "top_p",
        "seed",
        "frequency_penalty",
        "presence_penalty",
        "stop",
        "logit_bias",
        "logprobs",
        "top_logprobs",
        "user",
        "store",
        "metadata",
        "service_tier",
        "prompt_cache_key",
        "prompt_cache_retention",
        "safety_identifier",
    ] {
        if let Some(value) = input.get(name) {
            output.insert(name.into(), value.clone());
        }
    }
    if let Some(reasoning) = input.get("reasoning").filter(|value| !value.is_null()) {
        let reasoning = reasoning.as_object().ok_or("reasoning must be an object")?;
        if let Some(effort) = reasoning.get("effort") {
            if !effort.is_string() {
                return Err("reasoning effort must be a string".into());
            }
            output.insert("reasoning_effort".into(), effort.clone());
        }
    } else if let Some(effort) = input.get("reasoning_effort") {
        output.insert("reasoning_effort".into(), effort.clone());
    }
    if let Some(text) = input.get("text").filter(|value| !value.is_null()) {
        let text = text
            .as_object()
            .ok_or("Responses text options must be an object")?;
        if let Some(format) = text.get("format") {
            if let Some(format) = convert_response_format(format)? {
                output.insert("response_format".into(), format);
            }
        }
        if let Some(verbosity) = text.get("verbosity") {
            output.insert("verbosity".into(), verbosity.clone());
        }
    }
    if stream {
        let mut options = match input.get("stream_options") {
            None | Some(Value::Null) => Map::new(),
            Some(value) => value
                .as_object()
                .ok_or("stream_options must be an object")?
                .clone(),
        };
        options.entry("include_usage").or_insert(Value::Bool(true));
        output.insert("stream_options".into(), Value::Object(options));
    }
    serde_json::to_vec(&output)
        .map_err(|error| format!("serialize Chat Completions request failed: {error}"))
}

fn required_text<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    context: &str,
) -> Result<&'a str, String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{context} requires a non-empty {name}"))
}

fn append_input_item(
    item: &Value,
    messages: &mut Vec<Value>,
    calls: &mut HashSet<String>,
    results: &mut HashSet<String>,
) -> Result<(), String> {
    let object = item
        .as_object()
        .ok_or("Responses input item must be an object")?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    match kind {
        "reasoning" => {
            // Responses reasoning items carry provider-internal state. Chat has
            // no equivalent history item; never expose its encrypted content,
            // signature or summary as a visible assistant message. This narrow
            // exception does not apply to unknown/custom input item types.
        }
        "message" => {
            let role = required_text(object, "role", "message")?;
            if !matches!(role, "system" | "developer" | "user" | "assistant") {
                return Err(format!(
                    "Chat Completions cannot represent Responses message role {role}"
                ));
            }
            let content = convert_content(object.get("content").unwrap_or(&Value::Null), role)?;
            if role == "assistant" {
                if let Some(message) = messages.last_mut().filter(|message| {
                    message.get("role").and_then(Value::as_str) == Some("assistant")
                        && message.get("tool_calls").is_some()
                }) {
                    // A Chat stream can start a tool call before its text. Its
                    // Responses output then has function_call items followed by
                    // a message. Keep them in one assistant turn so that the
                    // next tool result immediately follows its tool_calls.
                    let message = message.as_object_mut().unwrap();
                    if let Some(name) = object.get("name") {
                        if message.get("name").is_some_and(|previous| previous != name) {
                            return Err("Chat Completions cannot merge tool calls with a different assistant name".into());
                        }
                        message.insert("name".into(), name.clone());
                    }
                    let previous = message.remove("content").unwrap_or(Value::Null);
                    message.insert("content".into(), merge_assistant_content(previous, content));
                    return Ok(());
                }
            }
            let mut message = json!({"role":role,"content":content});
            if let Some(name) = object.get("name") {
                message["name"] = name.clone();
            }
            messages.push(message);
        }
        "function_call" => {
            let call_id = required_text(object, "call_id", "function_call")?;
            let name = required_text(object, "name", "function_call")?;
            let arguments = object
                .get("arguments")
                .and_then(Value::as_str)
                .ok_or("function_call arguments must be a JSON string")?;
            if !calls.insert(call_id.to_string()) {
                return Err(format!("duplicate function_call call_id {call_id}"));
            }
            let call = json!({"id":call_id,"type":"function","function":{"name":name,"arguments":arguments}});
            if let Some(message) = messages
                .last_mut()
                .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
            {
                let object = message.as_object_mut().unwrap();
                object
                    .entry("tool_calls")
                    .or_insert_with(|| Value::Array(vec![]))
                    .as_array_mut()
                    .unwrap()
                    .push(call);
            } else {
                messages.push(json!({"role":"assistant","content":null,"tool_calls":[call]}));
            }
        }
        "function_call_output" => {
            let call_id = required_text(object, "call_id", "function_call_output")?;
            if !calls.contains(call_id) {
                return Err(format!("function_call_output {call_id} has no preceding function_call; supply the complete input history"));
            }
            if !results.insert(call_id.to_string()) {
                return Err(format!("duplicate function_call_output call_id {call_id}"));
            }
            let output = object
                .get("output")
                .ok_or("function_call_output is missing output")?;
            let content = match output {
                Value::String(_) => output.clone(),
                Value::Array(_) => convert_content(output, "tool")?,
                _ => {
                    return Err(
                        "function_call_output must contain text or text content parts".into(),
                    )
                }
            };
            messages.push(json!({"role":"tool","tool_call_id":call_id,"content":content}));
        }
        other => {
            return Err(format!(
                "Chat Completions cannot represent Responses input item type {other}"
            ))
        }
    }
    Ok(())
}

fn merge_assistant_content(previous: Value, next: Value) -> Value {
    match (previous, next) {
        (Value::Null, content) | (content, Value::Null) => content,
        (previous, next) => {
            let mut parts = Vec::new();
            for content in [previous, next] {
                match content {
                    Value::Array(content) => parts.extend(content),
                    Value::String(text) => parts.push(json!({"type":"text","text":text})),
                    _ => unreachable!("assistant content was already validated"),
                }
            }
            Value::Array(parts)
        }
    }
}

fn convert_content(content: &Value, role: &str) -> Result<Value, String> {
    match content {
        Value::String(_) => Ok(content.clone()),
        Value::Null if role == "assistant" => Ok(Value::Null),
        Value::Array(parts) => {
            let mut converted = Vec::with_capacity(parts.len());
            for part in parts {
                let object = part
                    .as_object()
                    .ok_or("message content part must be an object")?;
                match object.get("type").and_then(Value::as_str).unwrap_or("") {
                    "input_text" | "output_text" | "text" => {
                        let text = object
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or("text content part requires string text")?;
                        converted.push(json!({"type":"text","text":text}));
                    }
                    "input_image" if role == "user" => {
                        let url = required_text(object, "image_url", "input_image")?;
                        let mut image = json!({"url":url});
                        if let Some(detail) = object.get("detail") {
                            image["detail"] = detail.clone();
                        }
                        converted.push(json!({"type":"image_url","image_url":image}));
                    }
                    "refusal" if role == "assistant" => {
                        let refusal = object
                            .get("refusal")
                            .and_then(Value::as_str)
                            .ok_or("refusal content part requires string refusal")?;
                        converted.push(json!({"type":"refusal","refusal":refusal}));
                    }
                    kind => {
                        return Err(format!(
                            "Chat Completions cannot represent {role} content part type {kind}"
                        ))
                    }
                }
            }
            Ok(Value::Array(converted))
        }
        _ => Err(format!("invalid {role} message content")),
    }
}

fn convert_tool_choice(choice: &Value) -> Result<Value, String> {
    if let Some(choice) = choice.as_str() {
        return match choice {
            "auto" | "none" | "required" => Ok(Value::String(choice.into())),
            _ => Err(format!("unsupported Chat Completions tool_choice {choice}")),
        };
    }
    let choice = choice
        .as_object()
        .ok_or("tool_choice must be a string or function choice")?;
    if choice.get("type").and_then(Value::as_str) != Some("function") {
        return Err("Chat Completions supports only a named function tool_choice".into());
    }
    let function = choice
        .get("function")
        .and_then(Value::as_object)
        .unwrap_or(choice);
    let name = required_text(function, "name", "function tool_choice")?;
    Ok(json!({"type":"function","function":{"name":name}}))
}

fn convert_response_format(format: &Value) -> Result<Option<Value>, String> {
    let format = format.as_object().ok_or("text.format must be an object")?;
    match format.get("type").and_then(Value::as_str) {
        Some("text") => Ok(None),
        Some("json_object") => Ok(Some(json!({"type":"json_object"}))),
        Some("json_schema") => {
            required_text(format, "name", "json_schema format")?;
            if !format.get("schema").is_some_and(Value::is_object) {
                return Err("json_schema format requires a schema object".into());
            }
            let mut schema = format.clone();
            schema.remove("type");
            Ok(Some(json!({"type":"json_schema","json_schema":schema})))
        }
        _ => Err("unsupported Responses text.format for Chat Completions".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn convert(value: Value) -> Result<Value, String> {
        serde_json::from_slice(&adapt_openai_responses_to_chat_completions(
            &serde_json::to_vec(&value).unwrap(),
            Some("vendor-model"),
        )?)
        .map_err(|error| error.to_string())
    }

    #[test]
    fn responses_to_chat_preserves_instructions_images_and_parallel_tool_history() {
        let body = json!({"model":"public-model","stream":true,"instructions":"system instruction","input":[
            {"role":"developer","content":[{"type":"input_text","text":"developer instruction"}]},
            {"role":"user","content":[{"type":"input_text","text":"read image"},{"type":"input_image","image_url":"data:image/png;base64,AQID","detail":"high"}]},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"checking"}]},
            {"type":"function_call","id":"fc_1","call_id":"call_1","name":"lookup","arguments":"{\"city\":\"上海\"}"},
            {"type":"function_call","call_id":"call_2","name":"lookup","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"sunny"},
            {"type":"function_call_output","call_id":"call_2","output":[{"type":"input_text","text":"cloudy"}]},
            {"role":"user","content":"continue"}
        ],"tools":[{"type":"function","name":"lookup","description":"query city","parameters":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"],"additionalProperties":false},"strict":true}],"tool_choice":{"type":"function","name":"lookup"},"parallel_tool_calls":true});
        let converted = convert(body).unwrap();
        assert_eq!(converted["model"], "vendor-model");
        assert_eq!(
            converted["messages"][0],
            json!({"role":"system","content":"system instruction"})
        );
        assert_eq!(converted["messages"][1]["role"], "developer");
        assert_eq!(
            converted["messages"][2]["content"][1],
            json!({"type":"image_url","image_url":{"url":"data:image/png;base64,AQID","detail":"high"}})
        );
        assert_eq!(converted["messages"][3]["content"][0]["text"], "checking");
        assert_eq!(
            converted["messages"][3]["tool_calls"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(converted["messages"][3]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            converted["messages"][3]["tool_calls"][0]["function"]["arguments"],
            "{\"city\":\"上海\"}"
        );
        assert_eq!(
            converted["messages"][4],
            json!({"role":"tool","tool_call_id":"call_1","content":"sunny"})
        );
        assert_eq!(converted["messages"][5]["tool_call_id"], "call_2");
        assert_eq!(
            converted["tools"][0]["function"]["parameters"]["additionalProperties"],
            false
        );
        assert_eq!(converted["tools"][0]["function"]["strict"], true);
        assert_eq!(
            converted["tool_choice"],
            json!({"type":"function","function":{"name":"lookup"}})
        );
        assert_eq!(converted["stream_options"]["include_usage"], true);
    }

    #[test]
    fn responses_to_chat_maps_generation_and_json_schema_options() {
        let converted=convert(json!({"input":"hello","stream":false,"max_output_tokens":321,"temperature":0.4,"top_p":0.8,"reasoning":{"effort":"high","summary":"auto"},"text":{"verbosity":"low","format":{"type":"json_schema","name":"answer","strict":true,"schema":{"type":"object","properties":{"answer":{"type":"string"}}}}},"metadata":{"task":"test"},"service_tier":"priority"})).unwrap();
        assert_eq!(
            converted["messages"],
            json!([{"role":"user","content":"hello"}])
        );
        assert_eq!(converted["max_completion_tokens"], 321);
        assert_eq!(converted["temperature"], 0.4);
        assert_eq!(converted["top_p"], 0.8);
        assert_eq!(converted["reasoning_effort"], "high");
        assert_eq!(
            converted["response_format"]["json_schema"]["name"],
            "answer"
        );
        assert_eq!(
            converted["response_format"]["json_schema"]["schema"]["properties"]["answer"]["type"],
            "string"
        );
        assert_eq!(converted["verbosity"], "low");
        assert!(converted.get("stream_options").is_none());
        assert_eq!(
            convert(json!({"input":[]})).unwrap()["messages"],
            json!([{"role":"user","content":""}])
        );
    }

    #[test]
    fn responses_to_chat_omits_opaque_reasoning_without_changing_visible_tool_history() {
        let visible = vec![
            json!({"role":"user","content":"look up the weather"}),
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Checking now."}]}),
            json!({"type":"function_call","call_id":"call_weather","name":"weather","arguments":"{\"city\":\"上海\"}"}),
            json!({"type":"function_call_output","call_id":"call_weather","output":"sunny"}),
            json!({"role":"user","content":"continue"}),
        ];
        let reasoning = json!({
            "type":"reasoning",
            "id":"rs_private",
            "encrypted_content":"opaque_ciphertext_must_not_leak",
            "signature":"provider_signature_must_not_leak",
            "summary":[{"type":"summary_text","text":"private_summary_must_not_leak"}]
        });
        let with_reasoning = visible
            .iter()
            .flat_map(|item| [reasoning.clone(), item.clone()])
            .collect::<Vec<_>>();
        let expected = convert(json!({"input":visible})).unwrap();
        let actual = convert(json!({"input":with_reasoning})).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual["messages"][1]["tool_calls"][0]["id"], "call_weather");
        assert_eq!(actual["messages"][2]["tool_call_id"], "call_weather");
        let serialized = actual.to_string();
        for private_value in [
            "rs_private",
            "opaque_ciphertext_must_not_leak",
            "provider_signature_must_not_leak",
            "private_summary_must_not_leak",
        ] {
            assert!(!serialized.contains(private_value));
        }
        let error = convert(json!({"input":[{
            "type":"unknown_history_item",
            "encrypted_content":"opaque_ciphertext_must_not_leak"
        }]}))
        .unwrap_err();
        assert!(error.contains("unknown_history_item"));
        assert!(!error.contains("opaque_ciphertext_must_not_leak"));
    }

    #[test]
    fn responses_to_chat_keeps_late_text_with_its_parallel_tool_calls() {
        let converted = convert(json!({"input":[
            {"role":"user","content":"query both tools"},
            {"type":"function_call","call_id":"call_b","name":"second","arguments":"{\"b\":2}"},
            {"type":"function_call","call_id":"call_a","name":"first","arguments":"{\"a\":1}"},
            {"type":"message","role":"assistant","content":"Checking "},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"both tools."},{"type":"refusal","refusal":"No private data."}]},
            {"type":"function_call_output","call_id":"call_b","output":"second result"},
            {"type":"function_call_output","call_id":"call_a","output":"first result"}
        ]})).unwrap();
        let messages = converted["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_b");
        assert_eq!(messages[1]["tool_calls"][1]["id"], "call_a");
        assert_eq!(
            messages[1]["content"],
            json!([
                {"type":"text","text":"Checking "},
                {"type":"text","text":"both tools."},
                {"type":"refusal","refusal":"No private data."}
            ])
        );
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_b");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call_a");
    }

    #[test]
    fn responses_to_chat_rejects_unrepresentable_tools_items_and_missing_history() {
        for payload in [
            json!({"input":"hi","tools":[{"type":"custom","name":"shell"}]}),
            json!({"input":"hi","tools":[{"type":"web_search"}]}),
            json!({"input":[{"type":"custom_tool_call","call_id":"a","name":"shell","input":"pwd"}]}),
            json!({"input":[{"type":"unknown_history_item","encrypted_content":"opaque"}]}),
            json!({"input":[{"role":"user","content":[{"type":"input_file","file_id":"file_1"}]}]}),
            json!({"input":[{"type":"function_call_output","call_id":"missing","output":"result"}]}),
            json!({"input":"hi","previous_response_id":"resp_unknown"}),
            json!({"input":"hi","tool_choice":{"type":"allowed_tools","tools":[]}}),
        ] {
            assert!(
                convert(payload.clone()).is_err(),
                "silently accepted {payload}"
            );
        }
    }
}
