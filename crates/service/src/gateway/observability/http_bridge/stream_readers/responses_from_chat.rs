//! Chat Completions upstream to the Responses client contract.
use super::{
    json, mark_first_response_ms, mark_terminal_delivered, should_emit_keepalive_after_first_frame,
    stream_idle_timed_out, stream_wait_timeout, Arc, Cursor, Mutex, PassthroughSseCollector, Read,
    SseKeepAliveFrame, UpstreamSseFramePump, UpstreamSseFramePumpItem, Value,
};
use std::collections::BTreeMap;
use std::time::Instant;

const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_OUTPUT_ITEMS: usize = 256;

#[derive(Default)]
struct ChatResponseState {
    id: String,
    model: String,
    created: i64,
    started: bool,
    completed: bool,
    sequence: u64,
    output: Vec<Value>,
    text_index: Option<usize>,
    refusal_index: Option<usize>,
    tool_indices: BTreeMap<usize, usize>,
    finish_reason: Option<String>,
    usage: Value,
    output_bytes: usize,
}

impl ChatResponseState {
    fn new(model: Option<&str>) -> Self {
        Self {
            id: format!("resp_{:032x}", rand::random::<u128>()),
            model: model.unwrap_or_default().into(),
            ..Self::default()
        }
    }

    fn event(&mut self, out: &mut String, kind: &str, mut value: Value) {
        value["type"] = json!(kind);
        value["sequence_number"] = json!(self.sequence);
        self.sequence = self.sequence.saturating_add(1);
        out.push_str(&format!("event: {kind}\ndata: {value}\n\n"));
    }

    fn response(&self, status: &str) -> Value {
        let usage = &self.usage;
        let input = usage["prompt_tokens"].as_i64().unwrap_or(0).max(0);
        let output = usage["completion_tokens"].as_i64().unwrap_or(0).max(0);
        json!({
            "id": self.id, "object": "response", "created_at": self.created,
            "model": self.model, "status": status,
            "error": null, "incomplete_details": null,
            "output": self.output,
            "usage": {
                "input_tokens": input, "output_tokens": output,
                "total_tokens": usage["total_tokens"].as_i64().unwrap_or_else(|| input.saturating_add(output)).max(0),
                "input_tokens_details": {"cached_tokens": usage.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_i64).unwrap_or(0).max(0)},
                "output_tokens_details": {"reasoning_tokens": usage.pointer("/completion_tokens_details/reasoning_tokens").and_then(Value::as_i64).unwrap_or(0).max(0)}
            }
        })
    }

    fn start(&mut self, out: &mut String) {
        if !self.started {
            self.started = true;
            self.event(
                out,
                "response.created",
                json!({"response": self.response("in_progress")}),
            );
            self.event(
                out,
                "response.in_progress",
                json!({"response": self.response("in_progress")}),
            );
        }
    }

    fn add_item(&mut self, out: &mut String, item: Value) -> Result<usize, String> {
        if self.output.len() >= MAX_OUTPUT_ITEMS {
            return Err("chat response exceeds output item limit".into());
        }
        let index = self.output.len();
        self.event(
            out,
            "response.output_item.added",
            json!({"output_index": index, "item": item}),
        );
        self.output.push(item);
        Ok(index)
    }

    fn text_delta(&mut self, out: &mut String, delta: &str, refusal: bool) -> Result<(), String> {
        if delta.is_empty() {
            return Ok(());
        }
        self.reserve_output(delta.len())?;
        let saved_index = if refusal {
            self.refusal_index
        } else {
            self.text_index
        };
        let index = match saved_index {
            Some(index) => index,
            None => {
                let index = self.add_item(
                    out,
                    json!({
                        "id": format!("msg_{}_{}", self.id, self.output.len()), "type": "message",
                        "role": "assistant", "status": "in_progress", "content": []
                    }),
                )?;
                let part = if refusal {
                    json!({"type": "refusal", "refusal": ""})
                } else {
                    json!({"type": "output_text", "text": "", "annotations": []})
                };
                self.output[index]["content"] = json!([part]);
                self.event(out, "response.content_part.added", json!({"output_index": index, "item_id": self.output[index]["id"], "content_index": 0, "part": part}));
                if refusal {
                    self.refusal_index = Some(index);
                } else {
                    self.text_index = Some(index);
                }
                index
            }
        };
        let field = if refusal { "refusal" } else { "text" };
        let text = self.output[index]["content"][0][field]
            .as_str()
            .unwrap_or_default()
            .to_owned()
            + delta;
        self.output[index]["content"][0][field] = json!(text);
        self.event(out, if refusal { "response.refusal.delta" } else { "response.output_text.delta" }, json!({"output_index": index, "item_id": self.output[index]["id"], "content_index": 0, "delta": delta}));
        Ok(())
    }

    fn reserve_output(&mut self, bytes: usize) -> Result<(), String> {
        self.output_bytes = self.output_bytes.saturating_add(bytes);
        if self.output_bytes > MAX_OUTPUT_BYTES {
            Err("chat response exceeds output byte limit".into())
        } else {
            Ok(())
        }
    }

    fn consume(&mut self, value: &Value) -> Result<String, String> {
        let mut out = String::new();
        if let Some(model) = value["model"].as_str() {
            self.model = model.into();
        }
        if let Some(created) = value["created"].as_i64() {
            self.created = created;
        }
        if let Some(usage) = value.get("usage").filter(|usage| usage.is_object()) {
            self.usage = usage.clone();
        }
        self.start(&mut out);
        let Some(choices) = value["choices"].as_array() else {
            return Ok(out);
        };
        // Responses has a single output, so the request adapter never asks for n > 1.
        let Some(choice) = choices
            .iter()
            .find(|choice| choice["index"].as_u64().unwrap_or(0) == 0)
        else {
            return Ok(out);
        };
        let delta = choice
            .get("delta")
            .or_else(|| choice.get("message"))
            .unwrap_or(&Value::Null);
        if let Some(text) = delta["content"].as_str() {
            self.text_delta(&mut out, text, false)?;
        }
        if let Some(parts) = delta["content"].as_array() {
            for part in parts {
                if let Some(text) = part["text"].as_str() {
                    self.text_delta(&mut out, text, false)?;
                }
            }
        }
        if let Some(text) = delta["refusal"].as_str() {
            self.text_delta(&mut out, text, true)?;
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for (position, call) in calls.iter().enumerate() {
                let tool_index = call["index"].as_u64().unwrap_or(position as u64) as usize;
                let new_call = !self.tool_indices.contains_key(&tool_index);
                let index = match self.tool_indices.get(&tool_index).copied() {
                    Some(index) => index,
                    None => {
                        let index = self.add_item(&mut out, json!({"id": format!("fc_{}_{}", self.id, tool_index), "type": "function_call", "status": "in_progress", "call_id": call["id"].as_str().unwrap_or_default(), "name": call.pointer("/function/name").and_then(Value::as_str).unwrap_or_default(), "arguments": ""}))?;
                        self.tool_indices.insert(tool_index, index);
                        index
                    }
                };
                if let Some(id) = call["id"].as_str().filter(|id| !id.is_empty()) {
                    self.reserve_output(id.len())?;
                    self.output[index]["call_id"] = json!(id);
                }
                if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                    self.reserve_output(name.len())?;
                    let name = if new_call {
                        name.to_owned()
                    } else {
                        self.output[index]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned()
                            + name
                    };
                    self.output[index]["name"] = json!(name);
                }
                if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str)
                {
                    self.reserve_output(arguments.len())?;
                    let text = self.output[index]["arguments"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                        + arguments;
                    self.output[index]["arguments"] = json!(text);
                    self.event(&mut out, "response.function_call_arguments.delta", json!({"output_index": index, "item_id": self.output[index]["id"], "delta": arguments}));
                }
            }
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            self.finish_reason = Some(reason.into());
        }
        Ok(out)
    }

    fn finish(&mut self, error: Option<Value>) -> (String, Value) {
        let mut out = String::new();
        self.start(&mut out);
        let mut error = error.or_else(|| if self.finish_reason.is_none() { Some(json!({"code": "upstream_stream_incomplete", "message": "Chat Completions stream ended before finish_reason"})) } else { None });
        let incomplete = match self.finish_reason.as_deref() {
            Some("length") => Some("max_output_tokens"),
            Some("content_filter") => Some("content_filter"),
            _ => None,
        };
        if error.is_none()
            && incomplete.is_some()
            && self.text_index.is_none()
            && self.refusal_index.is_none()
        {
            error = Some(
                json!({"code": "upstream_response_incomplete", "message": "Chat Completions stopped without usable output"}),
            );
        }
        if error.is_none()
            && incomplete.is_none()
            && self.output.iter().any(|item| {
                item["type"] == "function_call"
                    && (item["call_id"].as_str().is_none_or(str::is_empty)
                        || item["name"].as_str().is_none_or(str::is_empty)
                        || item["arguments"].as_str().is_none_or(|arguments| {
                            serde_json::from_str::<Value>(arguments).is_err()
                        }))
            })
        {
            error = Some(
                json!({"code": "invalid_upstream_tool_call", "message": "Chat Completions returned an incomplete function call"}),
            );
        }
        let status = if error.is_some() {
            "failed"
        } else if incomplete.is_some() {
            "incomplete"
        } else {
            "completed"
        };
        for index in 0..self.output.len() {
            self.output[index]["status"] = json!(if status == "completed" {
                "completed"
            } else {
                "incomplete"
            });
            let item = self.output[index].clone();
            if item["type"] == "message" {
                let part = &item["content"][0];
                if part["type"] == "refusal" {
                    self.event(&mut out, "response.refusal.done", json!({"output_index": index, "item_id": item["id"], "content_index": 0, "refusal": part["refusal"]}));
                } else {
                    self.event(&mut out, "response.output_text.done", json!({"output_index": index, "item_id": item["id"], "content_index": 0, "text": part["text"]}));
                }
                self.event(&mut out, "response.content_part.done", json!({"output_index": index, "item_id": item["id"], "content_index": 0, "part": part}));
            } else {
                // A tool becomes executable only after a successful complete
                // call. Do not turn truncated arguments into a done item.
                if status != "completed" {
                    continue;
                }
                self.event(&mut out, "response.function_call_arguments.done", json!({"output_index": index, "item_id": item["id"], "arguments": item["arguments"], "name": item["name"]}));
            }
            self.event(
                &mut out,
                "response.output_item.done",
                json!({"output_index": index, "item": item}),
            );
        }
        let mut response = self.response(status);
        if let Some(error) = error {
            response["error"] = error;
        }
        if let Some(reason) = incomplete {
            response["incomplete_details"] = json!({"reason": reason});
        }
        self.event(
            &mut out,
            &format!("response.{status}"),
            json!({"response": response}),
        );
        self.completed = true;
        (out, response)
    }
}

pub(crate) fn convert_chat_completions_body_to_responses(body: &[u8]) -> Option<Vec<u8>> {
    let value: Value = serde_json::from_slice(body).ok()?;
    // A synthesized Responses envelope (e.g. a nonstream request served with SSE)
    // must not be interpreted as an empty Chat completion.
    if value["object"] == "response" {
        return Some(body.to_vec());
    }
    let mut state = ChatResponseState::new(None);
    let conversion_error = state
        .consume(&value)
        .err()
        .map(|message| json!({"code": "invalid_upstream_response", "message": message}));
    let (_, response) = state.finish(
        value
            .get("error")
            .filter(|error| !error.is_null())
            .cloned()
            .or(conversion_error),
    );
    serde_json::to_vec(&response).ok()
}

pub(crate) struct ResponsesFromChatCompletionsSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    state: ChatResponseState,
    collector: Arc<Mutex<PassthroughSseCollector>>,
    started_at: Instant,
    last_activity: Instant,
    saw_frame: bool,
}

impl ResponsesFromChatCompletionsSseReader {
    pub(crate) fn from_stream_response(
        upstream: crate::gateway::upstream::GatewayStreamResponse,
        collector: Arc<Mutex<PassthroughSseCollector>>,
        model: Option<&str>,
        started_at: Instant,
    ) -> Self {
        Self::from_pump(
            UpstreamSseFramePump::from_stream(upstream.into_body()),
            collector,
            model,
            started_at,
        )
    }

    pub(crate) fn from_pump(
        upstream: UpstreamSseFramePump,
        collector: Arc<Mutex<PassthroughSseCollector>>,
        model: Option<&str>,
        started_at: Instant,
    ) -> Self {
        Self {
            upstream,
            collector,
            state: ChatResponseState::new(model),
            started_at,
            last_activity: Instant::now(),
            saw_frame: false,
            out_cursor: Cursor::new(Vec::new()),
        }
    }

    fn finish(&mut self, error: Option<Value>) -> Vec<u8> {
        let (out, response) = self.state.finish(error);
        if let Ok(mut collector) = self.collector.lock() {
            let first_response_ms = collector.usage.first_response_ms;
            collector.usage = super::super::parse_usage_from_json(&response);
            collector.usage.first_response_ms = first_response_ms;
            collector.usage.completed_successfully = response["status"] == "completed";
            collector.usage.explicit_failure = response["status"] == "failed";
            if collector.usage.completed_successfully {
                collector.usage.response_id = Some(self.state.id.clone());
            }
            collector.saw_terminal = true;
            collector.last_event_type = Some(format!(
                "response.{}",
                response["status"].as_str().unwrap_or("failed")
            ));
            if response["status"] == "failed" {
                let message = response
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("upstream Chat Completions failed")
                    .to_owned();
                collector.terminal_error = Some(message.clone());
                collector.upstream_error_hint = Some(message);
            }
        }
        out.into_bytes()
    }

    fn fail(&mut self, message: String) -> Vec<u8> {
        self.finish(Some(
            json!({"code": "upstream_stream_read_error", "message": message}),
        ))
    }

    async fn next_chunk(&mut self) -> Vec<u8> {
        loop {
            match self
                .upstream
                .recv_timeout_async(stream_wait_timeout(self.last_activity))
                .await
            {
                Ok(UpstreamSseFramePumpItem::Frame(frame)) => {
                    self.last_activity = Instant::now();
                    self.saw_frame = true;
                    mark_first_response_ms(&self.collector, self.started_at);
                    let data = frame
                        .iter()
                        .filter_map(|line| {
                            line.trim_start_matches('\u{feff}')
                                .strip_prefix("data:")
                                .map(str::trim_start)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    if data.trim().is_empty() {
                        continue;
                    }
                    if data.trim() == "[DONE]" {
                        return self.finish(None);
                    }
                    let value = match serde_json::from_str::<Value>(&data) {
                        Ok(value) => value,
                        Err(error) => {
                            return self
                                .fail(format!("invalid upstream Chat Completions SSE: {error}"))
                        }
                    };
                    if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
                        return self.finish(Some(error.clone()));
                    }
                    match self.state.consume(&value) {
                        Ok(out) if !out.is_empty() => return out.into_bytes(),
                        Ok(_) => {}
                        Err(error) => return self.fail(error),
                    }
                }
                Ok(UpstreamSseFramePumpItem::Error(error)) => return self.fail(error),
                Ok(UpstreamSseFramePumpItem::Eof)
                | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return self.finish(None),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    self.last_activity = self
                        .last_activity
                        .max(self.upstream.last_activity().unwrap_or(self.last_activity));
                    if stream_idle_timed_out(self.last_activity) {
                        return self.fail("upstream Chat Completions stream idle timeout".into());
                    }
                    if should_emit_keepalive_after_first_frame(self.saw_frame) {
                        return SseKeepAliveFrame::Comment.bytes().to_vec();
                    }
                }
            }
        }
    }
}

impl crate::http::gateway_response_body::GatewayResponseBody
    for ResponsesFromChatCompletionsSseReader
{
    fn is_end_stream(&self) -> bool {
        self.state.completed && self.out_cursor.position() >= self.out_cursor.get_ref().len() as u64
    }
    fn finish_async(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.upstream.stop_input().await;
        })
    }
    fn read_async<'a>(
        &'a mut self,
        buffer: &'a mut [u8],
    ) -> crate::http::gateway_response_body::BodyReadFuture<'a> {
        Box::pin(async move {
            loop {
                let count = self.out_cursor.read(buffer)?;
                if count > 0 {
                    mark_terminal_delivered(&self.collector, &buffer[..count]);
                    return Ok(count);
                }
                if self.state.completed {
                    return Ok(0);
                }
                self.out_cursor = Cursor::new(self.next_chunk().await);
            }
        })
    }
}

#[cfg(test)]
#[path = "responses_from_chat_tests.rs"]
mod tests;
