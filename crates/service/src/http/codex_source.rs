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

fn default_tool_choice() -> String {
    "auto".to_string()
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
    pub tool_choice: String,
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
