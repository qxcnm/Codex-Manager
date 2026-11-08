use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub(crate) const X_CODEX_TURN_STATE_HEADER: &str = "x-codex-turn-state";
pub(crate) const X_CODEX_TURN_METADATA_HEADER: &str = "x-codex-turn-metadata";
pub(crate) const X_CODEX_PARENT_THREAD_ID_HEADER: &str = "x-codex-parent-thread-id";
pub(crate) const X_CODEX_WINDOW_ID_HEADER: &str = "x-codex-window-id";
pub(crate) const X_OPENAI_SUBAGENT_HEADER: &str = "x-openai-subagent";
pub(crate) const X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER: &str =
    "x-responsesapi-include-timing-metrics";
pub(crate) const RESPONSES_ENDPOINT: &str = "/v1/responses";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseToolChoiceMode {
    Auto,
    Required,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", deny_unknown_fields)]
pub(crate) enum ResponseNamedToolChoice {
    #[serde(rename = "function")]
    Function {
        #[serde(deserialize_with = "deserialize_tool_choice_name")]
        name: String,
    },
}

/// Responses accepts either a mode or a flat named function selection. Keep
/// the same wire shape for native WS and HTTP-to-WS upstream conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum ResponseToolChoice {
    Mode(ResponseToolChoiceMode),
    Named(ResponseNamedToolChoice),
}

fn deserialize_tool_choice_name<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "function tool_choice name must not be empty",
        ));
    }
    Ok(name)
}

fn default_tool_choice() -> ResponseToolChoice {
    ResponseToolChoice::Mode(ResponseToolChoiceMode::Auto)
}

fn default_stream() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ResponseCreateWsRequest {
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub input: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Value>,
    #[serde(default = "default_tool_choice")]
    pub tool_choice: ResponseToolChoice,
    #[serde(default)]
    pub parallel_tool_calls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(default)]
    pub store: bool,
    // Official Codex includes `stream` in response.create frames. Older clients
    // that omit it retain the streaming behavior of the WebSocket bridge.
    #[serde(default = "default_stream")]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_metadata: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_programs: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub(crate) enum ResponsesWsRequest {
    #[serde(rename = "response.create")]
    ResponseCreate(ResponseCreateWsRequest),
}

pub(crate) fn response_create_client_metadata(
    client_metadata: Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    let client_metadata = client_metadata.unwrap_or_default();
    (!client_metadata.is_empty()).then_some(client_metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn websocket_tool_choice_modes_and_named_function_round_trip() {
        for choice in [
            json!("auto"),
            json!("required"),
            json!("none"),
            json!({"type":"function","name":"cm_lookup"}),
        ] {
            let value = json!({"model":"test","input":[],"tool_choice":choice});
            let request: ResponseCreateWsRequest = serde_json::from_value(value).unwrap();
            let encoded =
                serde_json::to_value(ResponsesWsRequest::ResponseCreate(request)).unwrap();
            assert_eq!(encoded["tool_choice"], choice);
            assert_eq!(encoded["type"], "response.create");
        }
        let request: ResponseCreateWsRequest =
            serde_json::from_value(json!({"model":"test","input":[]})).unwrap();
        assert_eq!(
            serde_json::to_value(request).unwrap()["tool_choice"],
            "auto"
        );
    }

    #[test]
    fn websocket_tool_choice_rejects_invalid_modes_and_object_shapes() {
        for choice in [
            json!(null),
            json!(true),
            json!(1),
            json!([]),
            json!("invalid"),
            json!({}),
            json!({"type":"function"}),
            json!({"type":"function","name":"  "}),
            json!({"type":"function","name":7}),
            json!({"type":"function","function":{"name":"cm_lookup"}}),
            json!({"type":"function","name":"cm_lookup","unexpected":true}),
        ] {
            assert!(
                serde_json::from_value::<ResponseCreateWsRequest>(
                    json!({"model":"test","input":[],"tool_choice":choice})
                )
                .is_err(),
                "accepted malformed tool_choice {choice}"
            );
        }
    }
}
