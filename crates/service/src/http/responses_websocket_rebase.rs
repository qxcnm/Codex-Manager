use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

use super::responses_websocket_control::{is_interrupted_response, is_retained_interrupted_output};

const MAX_CACHED_WS_TOOL_CALLS: usize = 256;
const MAX_CACHED_WS_RESPONSES: usize = 64;
const MAX_CACHED_WS_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const WS_ACCOUNT_AFFINITY_KEYS: &[&str] = &[
    "session-id",
    "session_id",
    "conversation-id",
    "conversation_id",
    "thread-id",
    "thread_id",
    "client-request-id",
    "client_request_id",
    "window-id",
    "window_id",
    "turn-state",
    "turn_state",
    "parent-thread-id",
    "parent_thread_id",
    "turn-metadata",
    "turn_metadata",
    "x-client-request-id",
    "x-codex-conversation-id",
    "x-codex-window-id",
    "x-codex-turn-state",
    "x-codex-parent-thread-id",
    "x-codex-turn-metadata",
];

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum WsToolCallKind {
    Function,
    Custom,
    ToolSearch,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct WsToolCallKey {
    kind: WsToolCallKind,
    call_id: String,
}

#[derive(Clone, Default)]
pub(super) struct CompletedWsToolCallCache {
    calls: HashMap<WsToolCallKey, Value>,
    insertion_order: VecDeque<WsToolCallKey>,
}

pub(super) enum WsToolCallCacheEvent {
    Item(Value),
    InterruptedItem {
        item_id: Option<String>,
        call_id: Option<String>,
    },
    Terminal {
        tools: Vec<Value>,
        interrupted: bool,
    },
}

#[derive(Clone)]
struct CompletedWsResponse {
    previous_response_id: Option<String>,
    input: Vec<Value>,
    output: Vec<Value>,
    retained_bytes: usize,
}

#[derive(Default)]
pub(super) struct CompletedWsResponseCache {
    responses: HashMap<String, CompletedWsResponse>,
    insertion_order: VecDeque<String>,
    retained_bytes: usize,
}

impl WsToolCallKind {
    fn call_item_type(&self) -> &'static str {
        match self {
            Self::Function => "function_call",
            Self::Custom => "custom_tool_call",
            Self::ToolSearch => "tool_search_call",
        }
    }

    fn output_item_type(&self) -> &'static str {
        match self {
            Self::Function => "function_call_output",
            Self::Custom => "custom_tool_call_output",
            Self::ToolSearch => "tool_search_output",
        }
    }
}

impl CompletedWsToolCallCache {
    pub(super) fn parse_upstream_event(text: &str) -> Option<WsToolCallCacheEvent> {
        let value = serde_json::from_str::<Value>(text).ok()?;
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        match event_type.as_str() {
            "response.output_item.added" | "response.output_item.done" => {
                let item = value.get("item")?;
                normalize_ws_tool_call_item(item)?;
                Some(WsToolCallCacheEvent::Item(item.clone()))
            }
            "response.output_item.interrupted" => {
                let item_id = value
                    .get("item_id")
                    .and_then(Value::as_str)
                    .or_else(|| value.pointer("/item/id").and_then(Value::as_str));
                let call_id = value
                    .get("call_id")
                    .and_then(Value::as_str)
                    .or_else(|| value.pointer("/item/call_id").and_then(Value::as_str));
                (item_id.is_some() || call_id.is_some()).then(|| {
                    WsToolCallCacheEvent::InterruptedItem {
                        item_id: item_id.map(str::to_string),
                        call_id: call_id.map(str::to_string),
                    }
                })
            }
            "response.completed" | "response.incomplete"
                if event_type == "response.completed" || is_interrupted_response(&value) =>
            {
                let interrupted = is_interrupted_response(&value);
                let tools = value
                    .pointer("/response/output")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|item| normalize_ws_tool_call_item(item).is_some())
                    .cloned()
                    .collect::<Vec<_>>();
                // An interrupted terminal also removes old partial calls even
                // if its final output is empty. Other text-only events have no
                // registry work and must not contend with active tool sessions.
                (interrupted || !tools.is_empty())
                    .then_some(WsToolCallCacheEvent::Terminal { tools, interrupted })
            }
            _ => None,
        }
    }

    pub(super) fn observe_upstream_event(&mut self, text: &str) {
        if let Some(event) = Self::parse_upstream_event(text) {
            self.observe_parsed_upstream_event(&event);
        }
    }

    pub(super) fn observe_parsed_upstream_event(&mut self, event: &WsToolCallCacheEvent) {
        match event {
            WsToolCallCacheEvent::Item(item) => self.insert(item),
            WsToolCallCacheEvent::InterruptedItem { item_id, call_id } => {
                self.calls.retain(|key, item| {
                    !item_id
                        .as_deref()
                        .is_some_and(|id| item.get("id").and_then(Value::as_str) == Some(id))
                        && !call_id.as_deref().is_some_and(|id| key.call_id == id)
                });
                self.insertion_order
                    .retain(|key| self.calls.contains_key(key));
            }
            WsToolCallCacheEvent::Terminal { tools, interrupted } => {
                if *interrupted {
                    self.calls
                        .retain(|_, item| is_retained_interrupted_output(item));
                    self.insertion_order
                        .retain(|key| self.calls.contains_key(key));
                }
                for item in tools {
                    if !*interrupted || is_retained_interrupted_output(item) {
                        self.insert(item);
                    } else if let Some((key, _)) = normalize_ws_tool_call_item(item) {
                        self.calls.remove(&key);
                        self.insertion_order.retain(|existing| existing != &key);
                    }
                }
            }
        }
    }

    pub(super) fn observe_input(&mut self, input: &Value) {
        match input {
            Value::Array(items) => {
                for item in items {
                    self.insert(item);
                }
            }
            Value::Null => {}
            item => self.insert(item),
        }
    }

    pub(super) fn merge_from(&mut self, other: &Self) {
        for key in &other.insertion_order {
            if let Some(item) = other.calls.get(key) {
                self.insert(item);
            }
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    fn insert(&mut self, item: &Value) {
        let Some((key, normalized)) = normalize_ws_tool_call_item(item) else {
            return;
        };
        if !self.calls.contains_key(&key) {
            while self.calls.len() >= MAX_CACHED_WS_TOOL_CALLS {
                let Some(oldest) = self.insertion_order.pop_front() else {
                    break;
                };
                self.calls.remove(&oldest);
            }
            self.insertion_order.push_back(key.clone());
        }
        self.calls.insert(key, normalized);
    }

    fn get(&self, key: &WsToolCallKey) -> Option<&Value> {
        self.calls.get(key)
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        self.calls.iter().fold(0usize, |total, (key, value)| {
            total.saturating_add(key.call_id.len()).saturating_add(
                serde_json::to_vec(value)
                    .map(|bytes| bytes.len())
                    .unwrap_or_default(),
            )
        })
    }
}

impl CompletedWsResponseCache {
    pub(super) fn observe_completed_response(
        &mut self,
        terminal_text: &str,
        previous_response_id: Option<&str>,
        input: &Value,
    ) -> Result<bool, String> {
        let terminal = serde_json::from_str::<Value>(terminal_text)
            .map_err(|err| format!("parse completed websocket response failed: {err}"))?;
        let event_type = terminal
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if event_type != "response.completed" && !is_interrupted_response(&terminal) {
            return Ok(false);
        }
        let response = terminal
            .get("response")
            .and_then(Value::as_object)
            .ok_or_else(|| "completed websocket event is missing response object".to_string())?;
        let response_id = response
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| "completed websocket response is missing a non-empty id".to_string())?
            .to_string();
        let mut output = response
            .get("output")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| {
                format!("completed websocket response {response_id} is missing output array")
            })?;
        if is_interrupted_response(&terminal) {
            output.retain(is_retained_interrupted_output);
        }
        let input = normalize_ws_response_input(input);
        let previous_response_id = previous_response_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let retained_bytes = serde_json::to_vec(&input)
            .map_err(|err| format!("serialize cached websocket response input failed: {err}"))?
            .len()
            .saturating_add(
                serde_json::to_vec(&output)
                    .map_err(|err| {
                        format!("serialize cached websocket response output failed: {err}")
                    })?
                    .len(),
            )
            .saturating_add(response_id.len())
            .saturating_add(
                previous_response_id
                    .as_ref()
                    .map(String::len)
                    .unwrap_or_default(),
            );
        if retained_bytes > MAX_CACHED_WS_RESPONSE_BYTES {
            return Err(format!(
                "completed websocket response {response_id} requires {retained_bytes} bytes, exceeding the {} byte recovery cache limit",
                MAX_CACHED_WS_RESPONSE_BYTES
            ));
        }

        if let Some(previous) = self.responses.remove(&response_id) {
            self.retained_bytes = self.retained_bytes.saturating_sub(previous.retained_bytes);
            self.insertion_order
                .retain(|cached_id| cached_id != &response_id);
        }
        while self.responses.len() >= MAX_CACHED_WS_RESPONSES
            || self.retained_bytes.saturating_add(retained_bytes) > MAX_CACHED_WS_RESPONSE_BYTES
        {
            let Some(oldest_id) = self.insertion_order.pop_front() else {
                break;
            };
            if let Some(oldest) = self.responses.remove(&oldest_id) {
                self.retained_bytes = self.retained_bytes.saturating_sub(oldest.retained_bytes);
            }
        }

        self.retained_bytes = self.retained_bytes.saturating_add(retained_bytes);
        self.insertion_order.push_back(response_id.clone());
        self.responses.insert(
            response_id,
            CompletedWsResponse {
                previous_response_id,
                input,
                output,
                retained_bytes,
            },
        );
        Ok(true)
    }

    #[cfg(test)]
    pub(super) fn contains(&self, response_id: &str) -> bool {
        self.responses.contains_key(response_id)
    }

    fn collect_history_items(&self, response_id: &str) -> Result<Vec<Value>, String> {
        let mut current_response_id = response_id.to_string();
        let mut visited = HashSet::new();
        let mut chain = Vec::new();
        loop {
            if !visited.insert(current_response_id.clone()) {
                return Err(format!(
                    "cached websocket response history contains a cycle at {current_response_id}"
                ));
            }
            let response = self.responses.get(&current_response_id).ok_or_else(|| {
                format!(
                    "previous_response_id {current_response_id} is not available in this websocket session recovery cache"
                )
            })?;
            chain.push(response);
            let Some(previous_response_id) = response.previous_response_id.as_deref() else {
                break;
            };
            current_response_id = previous_response_id.to_string();
            if chain.len() >= MAX_CACHED_WS_RESPONSES {
                return Err(format!(
                    "websocket response history for {response_id} exceeds the {MAX_CACHED_WS_RESPONSES} response recovery limit"
                ));
            }
        }

        let capacity = chain.iter().fold(0usize, |total, response| {
            total
                .saturating_add(response.input.len())
                .saturating_add(response.output.len())
        });
        let mut history = Vec::with_capacity(capacity);
        for response in chain.into_iter().rev() {
            history.extend(response.input.iter().cloned());
            history.extend(response.output.iter().cloned());
        }
        Ok(history)
    }
}

fn normalize_ws_response_input(input: &Value) -> Vec<Value> {
    match input {
        Value::Array(items) => items.clone(),
        Value::Null => Vec::new(),
        Value::String(text) => vec![serde_json::json!({
            "type": "message",
            "role": "user",
            "content": [{ "type": "input_text", "text": text }]
        })],
        item => vec![item.clone()],
    }
}

/// Drops repeated tool outputs for the same `(kind, call_id)` pair.
///
/// The Codex tool runner can emit a second `custom_tool_call_output` when a
/// host-side progress notification is attached to the same custom tool call.
/// Responses accepts one output item per tool call, so preserve the first
/// output and remove later duplicates before forwarding the request upstream.
pub(super) fn normalize_ws_tool_call_outputs(input: &mut Value) -> usize {
    let Some(items) = input.as_array_mut() else {
        return 0;
    };

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(items.len());
    let mut removed = 0;
    for item in items.drain(..) {
        let Some(key) = ws_tool_output_key(&item).ok().flatten() else {
            normalized.push(item);
            continue;
        };
        if seen.insert(key) {
            normalized.push(item);
        } else {
            removed += 1;
        }
    }
    *items = normalized;
    removed
}

pub(super) fn expand_response_create_previous_response(
    text: &str,
    completed_responses: &CompletedWsResponseCache,
) -> Result<Option<String>, String> {
    let mut value = serde_json::from_str::<Value>(text)
        .map_err(|err| format!("parse response.create for history recovery failed: {err}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "response.create for history recovery must be a JSON object".to_string())?;
    if object.get("type").and_then(Value::as_str) != Some("response.create") {
        return Err("history recovery only supports type=response.create".to_string());
    }
    let Some(previous_response_id) = object
        .get("previous_response_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return Ok(None);
    };

    let mut input = completed_responses.collect_history_items(&previous_response_id)?;
    if let Some(current_input) = object.get("input") {
        input.extend(normalize_ws_response_input(current_input));
    }
    object.remove("previous_response_id");
    object.insert("input".to_string(), Value::Array(input));
    serde_json::to_string(&value)
        .map(Some)
        .map_err(|err| format!("serialize response.create after history recovery failed: {err}"))
}

fn normalize_ws_tool_call_item(item: &Value) -> Option<(WsToolCallKey, Value)> {
    let object = item.as_object()?;
    let kind = match object
        .get("type")
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "function_call" => WsToolCallKind::Function,
        "custom_tool_call" => WsToolCallKind::Custom,
        "tool_search_call" => WsToolCallKind::ToolSearch,
        _ => return None,
    };
    // Server-executed tool searches may have no call_id. Their item id is not
    // a call identifier and must never be invented as one for replay.
    let call_id = object
        .get("call_id")
        .or_else(|| {
            object
                .get("id")
                .filter(|_| kind != WsToolCallKind::ToolSearch)
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let key = WsToolCallKey {
        kind,
        call_id: call_id.clone(),
    };
    let mut normalized = item.clone();
    if let Some(object) = normalized.as_object_mut() {
        object
            .entry("call_id".to_string())
            .or_insert_with(|| Value::String(call_id));
    }
    Some((key, normalized))
}

fn ws_tool_output_key(item: &Value) -> Result<Option<WsToolCallKey>, String> {
    let Some(object) = item.as_object() else {
        return Ok(None);
    };
    let kind = match object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "function_call_output" => WsToolCallKind::Function,
        "custom_tool_call_output" => WsToolCallKind::Custom,
        "tool_search_output" => WsToolCallKind::ToolSearch,
        _ => return Ok(None),
    };
    if kind == WsToolCallKind::ToolSearch
        && object.get("call_id").is_none_or(|call_id| {
            call_id.is_null() || call_id.as_str().is_some_and(|id| id.trim().is_empty())
        })
    {
        return Ok(None);
    }
    let output_type = kind.output_item_type();
    let call_id = object
        .get("call_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{output_type} is missing a non-empty call_id"))?;
    Ok(Some(WsToolCallKey {
        kind,
        call_id: call_id.to_string(),
    }))
}

fn ws_tool_output_matches_key(item: &Value, key: &WsToolCallKey) -> bool {
    item.get("type")
        .and_then(Value::as_str)
        .is_some_and(|item_type| {
            item_type
                .trim()
                .eq_ignore_ascii_case(key.kind.output_item_type())
        })
        && item.get("call_id").and_then(Value::as_str).map(str::trim) == Some(key.call_id.as_str())
}

fn remove_ws_account_affinity_fields(object: &mut serde_json::Map<String, Value>) {
    let keys = object
        .keys()
        .filter(|key| {
            WS_ACCOUNT_AFFINITY_KEYS
                .iter()
                .any(|candidate| key.eq_ignore_ascii_case(candidate))
        })
        .cloned()
        .collect::<Vec<_>>();
    for key in keys {
        object.remove(&key);
    }
}

pub(super) fn rebase_response_create_for_account_change(
    text: &str,
    completed_tool_calls: &CompletedWsToolCallCache,
) -> Result<String, String> {
    let mut value = serde_json::from_str::<Value>(text)
        .map_err(|err| format!("parse response.create for account rebase failed: {err}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "response.create for account rebase must be a JSON object".to_string())?;
    if object.get("type").and_then(Value::as_str) != Some("response.create") {
        return Err("account rebase only supports type=response.create".to_string());
    }

    object.remove("previous_response_id");
    remove_ws_account_affinity_fields(object);
    let mut remove_empty_client_metadata = false;
    if let Some(client_metadata) = object.get_mut("client_metadata") {
        if let Some(client_metadata) = client_metadata.as_object_mut() {
            remove_ws_account_affinity_fields(client_metadata);
            remove_empty_client_metadata = client_metadata.is_empty();
        }
    }
    if remove_empty_client_metadata {
        object.remove("client_metadata");
    }

    restore_ws_tool_call_pairs(object, completed_tool_calls, None)?;

    let serialized = serde_json::to_vec(&value)
        .map_err(|err| format!("serialize response.create after account rebase failed: {err}"))?;
    let serialized =
        crate::gateway::strip_cross_account_encrypted_content(&serialized).unwrap_or(serialized);
    String::from_utf8(serialized)
        .map_err(|err| format!("serialize response.create after account rebase failed: {err}"))
}

fn restore_ws_tool_call_pairs(
    object: &mut serde_json::Map<String, Value>,
    completed_tool_calls: &CompletedWsToolCallCache,
    target: Option<&WsToolCallKey>,
) -> Result<(), String> {
    if let Some(input) = object.get("input").cloned() {
        let input_was_array = input.is_array();
        let items = match input {
            Value::Array(items) => items,
            Value::Null => Vec::new(),
            item => vec![item],
        };
        let current_calls = items
            .iter()
            .filter_map(normalize_ws_tool_call_item)
            .collect::<HashMap<_, _>>();
        let mut emitted_calls = HashSet::new();
        let mut rewritten_items = Vec::with_capacity(items.len());
        let mut input_changed = false;

        for item in items {
            let call = normalize_ws_tool_call_item(&item);
            if let Some(target) = target {
                // Other results may refer to calls already retained by a live
                // previous_response_id. Leave them and their calls untouched;
                // only fresh-account recovery must validate every pair.
                if !call.as_ref().is_some_and(|(key, _)| key == target)
                    && !ws_tool_output_matches_key(&item, target)
                {
                    rewritten_items.push(item);
                    continue;
                }
            }
            if let Some((key, normalized)) = call {
                if emitted_calls.insert(key) {
                    input_changed |= normalized != item;
                    rewritten_items.push(normalized);
                } else {
                    input_changed = true;
                }
                continue;
            }

            if let Some(key) = ws_tool_output_key(&item)? {
                if !emitted_calls.contains(&key) {
                    let matching_call = current_calls
                        .get(&key)
                        .or_else(|| completed_tool_calls.get(&key))
                        .ok_or_else(|| {
                            format!(
                                "cannot rebase {} with call_id '{}': matching {} was not observed in the current input or completed upstream output",
                                key.kind.output_item_type(),
                                key.call_id,
                                key.kind.call_item_type(),
                            )
                        })?;
                    rewritten_items.push(matching_call.clone());
                    emitted_calls.insert(key);
                    input_changed = true;
                }
            }
            rewritten_items.push(item);
        }

        if input_changed {
            object.insert("input".to_string(), Value::Array(rewritten_items));
        } else if !input_was_array && rewritten_items.is_empty() {
            object.insert("input".to_string(), Value::Null);
        }
    }

    Ok(())
}

pub(super) fn rebase_response_create_for_missing_tool_call(
    text: &str,
    completed_tool_calls: &CompletedWsToolCallCache,
    kind: WsToolCallKind,
    call_id: &str,
) -> Result<Option<String>, String> {
    let original = serde_json::from_str::<Value>(text)
        .map_err(|err| format!("parse response.create for tool context recovery failed: {err}"))?;
    let object = original.as_object().ok_or_else(|| {
        "response.create for tool context recovery must be a JSON object".to_string()
    })?;
    if object.get("type").and_then(Value::as_str) != Some("response.create") {
        return Err("tool context recovery only supports type=response.create".to_string());
    }

    let expected = WsToolCallKey {
        kind,
        call_id: call_id.to_string(),
    };
    let items = match object.get("input") {
        Some(Value::Array(items)) => items.as_slice(),
        Some(Value::Null) | None => &[],
        Some(item) => std::slice::from_ref(item),
    };
    let has_expected_output = items
        .iter()
        .any(|item| ws_tool_output_matches_key(item, &expected));
    if !has_expected_output {
        return Ok(None);
    }
    let current_calls = items
        .iter()
        .filter_map(normalize_ws_tool_call_item)
        .collect::<HashMap<_, _>>();
    if !current_calls.contains_key(&expected) && completed_tool_calls.get(&expected).is_none() {
        return Ok(None);
    }

    let mut rebased = original.clone();
    let object = rebased.as_object_mut().ok_or_else(|| {
        "response.create for tool context recovery must be a JSON object".to_string()
    })?;
    // A missing call on an otherwise live socket does not make its response
    // chain or account affinity invalid. Repair only the observed call pairs;
    // stale response ids use the separate full-history recovery path.
    restore_ws_tool_call_pairs(object, completed_tool_calls, Some(&expected))?;
    if rebased == original {
        return Ok(None);
    }
    serde_json::to_string(&rebased).map(Some).map_err(|err| {
        format!("serialize response.create after tool context recovery failed: {err}")
    })
}

#[cfg(test)]
#[path = "tests/responses_websocket_tool_search_tests.rs"]
mod tool_search_tests;
