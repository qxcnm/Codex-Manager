use super::{string, tools::is_call, Bridge, ToolChoice};
use serde_json::{json, Value};
use std::collections::HashSet;

impl Bridge {
    /// Native tool arguments are withheld until the completed response validates
    /// every call. A partial or failed generation can never trigger client tools.
    pub(crate) fn translate_event(&mut self, mut event: Value) -> Result<Vec<Value>, String> {
        if self.terminal {
            return Err("BPS sent an event after the terminal response".into());
        }
        let kind = string(&event, "type").to_owned();
        match kind.as_str() {
            "basispoints.response.metadata"
            | "basispoints.response.heartbeat"
            | "basispoints.response.upstream_sent"
            | "basispoints.response.resume_token" => return Ok(Vec::new()),
            "basispoints.response.server_draining" => {
                return Err("BPS server is draining; this request will not be replayed".into())
            }
            _ => {}
        }
        if kind != "error" && !kind.starts_with("response.") {
            return Err("unexpected BPS event type".into());
        }
        if kind.len() > 128
            || !kind
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'_')
        {
            return Err("invalid BPS event type".into());
        }
        event
            .as_object_mut()
            .ok_or("invalid BPS event")?
            .remove("basispoints_replay_cursor");
        if kind.starts_with("response.function_call_arguments.")
            || kind.starts_with("response.custom_tool_call_input.")
        {
            if let Some(id) = event["item_id"].as_str().filter(|s| !s.is_empty()) {
                self.pending_tool_ids.insert(id.to_owned());
            }
            return Ok(Vec::new());
        }
        if matches!(
            kind.as_str(),
            "response.output_item.added" | "response.output_item.done"
        ) && is_call(&event["item"])
        {
            if let Some(id) = event["item"]["id"].as_str().filter(|s| !s.is_empty()) {
                self.pending_tool_ids.insert(id.to_owned());
            }
            return Ok(Vec::new());
        }
        let mut emitted = Vec::new();
        if kind == "response.completed" {
            if event["response"]["status"]
                .as_str()
                .is_some_and(|s| s != "completed")
            {
                return Err("BPS completed event has an inconsistent response status".into());
            }
            let items = event["response"]["output"]
                .as_array()
                .ok_or("BPS completed response is missing output")?
                .clone();
            let mut translated = Vec::with_capacity(items.len());
            let mut native_ids = HashSet::new();
            let mut call_ids = HashSet::new();
            let mut calls = Vec::new();
            for (index, item) in items.iter().enumerate() {
                if is_call(item) {
                    if let Some(id) = item["id"].as_str() {
                        native_ids.insert(id.to_owned());
                    }
                    let call = self.translate_call(item)?;
                    if !call_ids.insert(string(&call, "call_id").to_owned()) {
                        return Err("BPS returned duplicate tool call IDs".into());
                    }
                    calls.push((index, call.clone()));
                    translated.push(call);
                } else {
                    if !matches!(
                        string(item, "type"),
                        "message" | "reasoning" | "compaction" | "agent_message"
                    ) {
                        return Err("BPS returned an unsupported native output item".into());
                    }
                    translated.push(item.clone());
                }
            }
            if !self.pending_tool_ids.is_subset(&native_ids) {
                return Err("BPS completed response omitted a pending tool call".into());
            }
            if calls.is_empty()
                && matches!(self.choice, ToolChoice::Required | ToolChoice::Named(_))
            {
                return Err("BPS did not produce the required client tool call".into());
            }
            if calls.len() > 1 {
                return Err("BPS returned parallel tools on a sequential tool route".into());
            }
            // Emit only after the entire authoritative output passes validation.
            for (index, call) in calls {
                emitted.extend(tool_events(index, call));
            }
            event["response"]["output"] = Value::Array(translated);
            self.terminal = true;
        } else {
            if let Some(output) = event
                .pointer_mut("/response/output")
                .and_then(Value::as_array_mut)
            {
                output.retain(|item| !is_call(item));
            }
            self.terminal = matches!(
                kind.as_str(),
                "response.failed" | "response.incomplete" | "error"
            );
        }
        if let Some(response) = event.get_mut("response").and_then(Value::as_object_mut) {
            response.insert("parallel_tool_calls".into(), json!(false));
            // Preserve upstream model/reasoning/usage; never manufacture evidence
            // that the requested reasoning effort was actually honored.
            if !self.omitted_tools.is_empty() {
                if !response.get("metadata").is_some_and(Value::is_object) {
                    response.insert("metadata".into(), json!({}));
                }
                response["metadata"]["codexmanager_bps_omitted_tools"] =
                    json!(self.omitted_tools.join(","));
            }
        }
        emitted.push(event);
        for item in &mut emitted {
            item["sequence_number"] = json!(self.sequence);
            self.sequence += 1;
        }
        Ok(emitted)
    }
}

fn tool_events(index: usize, call: Value) -> Vec<Value> {
    let custom = string(&call, "type") == "custom_tool_call";
    let field = if custom { "input" } else { "arguments" };
    let event_prefix = if custom {
        "response.custom_tool_call_input"
    } else {
        "response.function_call_arguments"
    };
    let mut initial = call.clone();
    initial[field] = json!("");
    initial["status"] = json!("in_progress");
    let mut done =
        json!({"type":format!("{event_prefix}.done"),"output_index":index,"item_id":call["id"]});
    done[field] = call[field].clone();
    vec![
        json!({"type":"response.output_item.added","output_index":index,"item":initial}),
        json!({"type":format!("{event_prefix}.delta"),"output_index":index,"item_id":call["id"],"delta":call[field]}),
        done,
        json!({"type":"response.output_item.done","output_index":index,"item":call}),
    ]
}
