use super::{
    append_output_text, json, mark_first_response_ms_on_usage,
    should_emit_keepalive_after_first_frame, stream_idle_timed_out, stream_wait_timeout, Arc,
    Cursor, Map, Mutex, Read, SseKeepAliveFrame, UpstreamResponseUsage, UpstreamSseFramePump,
    UpstreamSseFramePumpItem, Value,
};
use std::time::Instant;

pub(crate) struct ResponsesFromAnthropicSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    state: ResponsesFromAnthropicState,
    usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    saw_upstream_frame: bool,
}

#[derive(Default)]
struct ResponsesFromAnthropicState {
    response_id: Option<String>,
    model: Option<String>,
    started: bool,
    text_item_started: bool,
    text_part_started: bool,
    text_finished: bool,
    completed: bool,
    output_text: String,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: Option<i64>,
    reasoning_output_tokens: i64,
    stop_reason: String,
    current_tool: Option<PendingToolUse>,
    completed_tools: Vec<Value>,
}

#[derive(Default)]
struct PendingToolUse {
    id: String,
    name: String,
    input_json: String,
}

impl ResponsesFromAnthropicSseReader {
    #[cfg(test)]
    pub(crate) fn from_reader<R>(
        upstream: R,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        request_started_at: Instant,
    ) -> Self
    where
        R: Read + Send + 'static,
    {
        Self::from_pump(
            UpstreamSseFramePump::from_reader(upstream),
            usage_collector,
            fallback_model,
            request_started_at,
        )
    }

    pub(crate) fn from_stream_response(
        upstream: crate::gateway::upstream::GatewayStreamResponse,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_pump(
            UpstreamSseFramePump::from_stream(upstream.into_body()),
            usage_collector,
            fallback_model,
            request_started_at,
        )
    }

    pub(crate) fn from_pump(
        upstream: UpstreamSseFramePump,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        request_started_at: Instant,
    ) -> Self {
        let mut state = ResponsesFromAnthropicState {
            stop_reason: "stop".to_string(),
            ..Default::default()
        };
        state.model = fallback_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Self {
            upstream,
            out_cursor: Cursor::new(Vec::new()),
            state,
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            saw_upstream_frame: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_reader(
            upstream,
            usage_collector,
            fallback_model,
            request_started_at,
        )
    }

    async fn next_chunk(&mut self) -> std::io::Result<Vec<u8>> {
        loop {
            match self
                .upstream
                .recv_timeout_async(stream_wait_timeout(self.last_upstream_activity))
                .await
            {
                Ok(UpstreamSseFramePumpItem::Frame(frame)) => {
                    self.last_upstream_activity = Instant::now();
                    self.saw_upstream_frame = true;
                    mark_first_response_ms_on_usage(&self.usage_collector, self.request_started_at);
                    let mapped = self.process_sse_frame(&frame);
                    if !mapped.is_empty() {
                        mark_first_response_ms_on_usage(
                            &self.usage_collector,
                            self.request_started_at,
                        );
                        return Ok(mapped);
                    }
                }
                Ok(UpstreamSseFramePumpItem::Eof)
                | Ok(UpstreamSseFramePumpItem::Error(_))
                | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    let finished = self.finish_stream();
                    if !finished.is_empty() {
                        mark_first_response_ms_on_usage(
                            &self.usage_collector,
                            self.request_started_at,
                        );
                    }
                    return Ok(finished);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if stream_idle_timed_out(self.last_upstream_activity) {
                        let finished = self.finish_stream();
                        if !finished.is_empty() {
                            mark_first_response_ms_on_usage(
                                &self.usage_collector,
                                self.request_started_at,
                            );
                        }
                        return Ok(finished);
                    }
                    if should_emit_keepalive_after_first_frame(self.saw_upstream_frame) {
                        return Ok(SseKeepAliveFrame::Comment.bytes().to_vec());
                    }
                }
            }
        }
    }

    fn process_sse_frame(&mut self, lines: &[String]) -> Vec<u8> {
        let mut event_name = String::new();
        let mut data_lines = Vec::new();
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("event:") {
                event_name = rest.trim().to_string();
            } else if let Some(rest) = trimmed.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_string());
            }
        }
        if data_lines.is_empty() {
            return Vec::new();
        }
        let data = data_lines.join("\n");
        if data.trim() == "[DONE]" {
            return self.finish_stream();
        }
        let value = match serde_json::from_str::<Value>(&data) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        };
        self.consume_anthropic_event(event_name.as_str(), &value)
    }

    fn consume_anthropic_event(&mut self, event_name: &str, value: &Value) -> Vec<u8> {
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or(event_name);
        let mut out = String::new();
        match event_type {
            "message_start" => {
                if let Some(message) = value.get("message").and_then(Value::as_object) {
                    self.capture_message_start(message);
                }
                self.ensure_response_started(&mut out);
            }
            "content_block_start" => {
                self.ensure_response_started(&mut out);
                if let Some(block) = value.get("content_block").and_then(Value::as_object) {
                    self.start_content_block(block);
                }
            }
            "content_block_delta" => {
                self.ensure_response_started(&mut out);
                if let Some(delta) = value.get("delta").and_then(Value::as_object) {
                    self.consume_content_delta(delta, &mut out);
                }
            }
            "content_block_stop" => {
                self.finish_tool_block(&mut out);
            }
            "message_delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_object) {
                    if let Some(stop_reason) = delta.get("stop_reason").and_then(Value::as_str) {
                        self.state.stop_reason = stop_reason.to_string();
                    }
                }
                if let Some(usage) = value.get("usage").and_then(Value::as_object) {
                    self.capture_usage(usage);
                }
            }
            "message_stop" => {
                out.push_str(String::from_utf8_lossy(&self.finish_stream()).as_ref());
            }
            _ => {}
        }
        out.into_bytes()
    }

    fn capture_message_start(&mut self, message: &Map<String, Value>) {
        if let Some(id) = message.get("id").and_then(Value::as_str) {
            self.state.response_id = Some(id.to_string());
        }
        if let Some(model) = message.get("model").and_then(Value::as_str) {
            self.state.model = Some(model.to_string());
        }
        if let Some(stop_reason) = message.get("stop_reason").and_then(Value::as_str) {
            self.state.stop_reason = stop_reason.to_string();
        }
        if let Some(usage) = message.get("usage").and_then(Value::as_object) {
            self.capture_usage(usage);
        }
    }

    fn capture_usage(&mut self, usage: &Map<String, Value>) {
        if let Some(value) = usage_i64(usage, &["input_tokens", "prompt_tokens"]) {
            self.state.input_tokens = value;
        }
        if let Some(value) = usage_i64(
            usage,
            &[
                "cache_read_input_tokens",
                "cached_input_tokens",
                "input_tokens_details.cached_tokens",
                "prompt_tokens_details.cached_tokens",
            ],
        ) {
            self.state.cached_input_tokens = value;
        }
        if let Some(value) = usage_i64(
            usage,
            &[
                "cache_creation_input_tokens",
                "cache_write_input_tokens",
                "input_tokens_details.cache_write_tokens",
                "prompt_tokens_details.cache_write_tokens",
            ],
        ) {
            self.state.cache_write_tokens = value;
        }
        if let Some(value) = usage_i64(usage, &["output_tokens", "completion_tokens"]) {
            self.state.output_tokens = value;
        }
        if let Some(value) = usage_i64(
            usage,
            &[
                "reasoning_output_tokens",
                "output_tokens_details.reasoning_tokens",
                "completion_tokens_details.reasoning_tokens",
            ],
        ) {
            self.state.reasoning_output_tokens = value;
        }
        self.state.total_tokens = usage_i64(usage, &["total_tokens"]).or_else(|| {
            Some(
                self.state.input_tokens
                    + self.state.cached_input_tokens
                    + self.state.cache_write_tokens
                    + self.state.output_tokens,
            )
        });
    }

    fn start_content_block(&mut self, block: &Map<String, Value>) {
        if block
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "tool_use")
        {
            let id = block
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("toolu_unknown")
                .to_string();
            let name = block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let input_json = block.get("input").cloned().unwrap_or_else(|| json!({}));
            let input_json = if input_json.as_object().is_some_and(Map::is_empty) {
                String::new()
            } else {
                input_json.to_string()
            };
            self.state.current_tool = Some(PendingToolUse {
                id,
                name,
                input_json,
            });
        }
    }

    fn consume_content_delta(&mut self, delta: &Map<String, Value>, out: &mut String) {
        match delta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text_delta" => {
                let fragment = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if fragment.is_empty() {
                    return;
                }
                append_output_text(&mut self.state.output_text, fragment);
                self.ensure_text_part_started(out);
                append_sse_event(
                    out,
                    "response.output_text.delta",
                    &json!({
                        "type": "response.output_text.delta",
                        "delta": fragment,
                        "item_id": self.text_item_id(),
                        "output_index": 0,
                        "content_index": 0,
                    }),
                );
            }
            "input_json_delta" => {
                if let Some(partial) = delta.get("partial_json").and_then(Value::as_str) {
                    if let Some(tool) = self.state.current_tool.as_mut() {
                        tool.input_json.push_str(partial);
                    }
                }
            }
            _ => {}
        }
    }

    fn ensure_response_started(&mut self, out: &mut String) {
        if self.state.started {
            return;
        }
        self.state.started = true;
        let response = self.response_payload("in_progress");
        append_sse_event(
            out,
            "response.created",
            &json!({
                "type": "response.created",
                "response": response,
            }),
        );
        append_sse_event(
            out,
            "response.in_progress",
            &json!({
                "type": "response.in_progress",
                "response": self.response_payload("in_progress"),
            }),
        );
    }

    fn ensure_text_part_started(&mut self, out: &mut String) {
        self.ensure_response_started(out);
        if !self.state.text_item_started {
            self.state.text_item_started = true;
            append_sse_event(
                out,
                "response.output_item.added",
                &json!({
                    "type": "response.output_item.added",
                    "output_index": 0,
                    "item": {
                        "id": self.text_item_id(),
                        "type": "message",
                        "status": "in_progress",
                        "role": "assistant",
                        "content": [],
                    }
                }),
            );
        }
        if !self.state.text_part_started {
            self.state.text_part_started = true;
            append_sse_event(
                out,
                "response.content_part.added",
                &json!({
                    "type": "response.content_part.added",
                    "item_id": self.text_item_id(),
                    "output_index": 0,
                    "content_index": 0,
                    "part": { "type": "output_text", "text": "" },
                }),
            );
        }
    }

    fn finish_text_item(&mut self, out: &mut String) {
        if !self.state.text_part_started || self.state.text_finished {
            return;
        }
        self.state.text_finished = true;
        append_sse_event(
            out,
            "response.output_text.done",
            &json!({
                "type": "response.output_text.done",
                "text": self.state.output_text,
                "item_id": self.text_item_id(),
                "output_index": 0,
                "content_index": 0,
            }),
        );
        append_sse_event(
            out,
            "response.content_part.done",
            &json!({
                "type": "response.content_part.done",
                "item_id": self.text_item_id(),
                "output_index": 0,
                "content_index": 0,
                "part": { "type": "output_text", "text": self.state.output_text },
            }),
        );
        append_sse_event(
            out,
            "response.output_item.done",
            &json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": {
                    "id": self.text_item_id(),
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": self.state.output_text }],
                }
            }),
        );
    }

    fn finish_tool_block(&mut self, out: &mut String) {
        let Some(tool) = self.state.current_tool.take() else {
            return;
        };
        self.state.stop_reason = "tool_use".to_string();
        let output_index = self.completed_output_len();
        let item = json!({
            "id": tool.id,
            "type": "function_call",
            "status": "completed",
            "call_id": tool.id,
            "name": tool.name,
            "arguments": normalize_json_fragment(tool.input_json.as_str()),
        });
        append_sse_event(
            out,
            "response.output_item.added",
            &json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": item.clone(),
            }),
        );
        append_sse_event(
            out,
            "response.output_item.done",
            &json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item.clone(),
            }),
        );
        self.state.completed_tools.push(item);
    }

    fn finish_stream(&mut self) -> Vec<u8> {
        if self.state.completed {
            return Vec::new();
        }
        let mut out = String::new();
        self.ensure_response_started(&mut out);
        self.finish_text_item(&mut out);
        self.finish_tool_block(&mut out);
        self.state.completed = true;
        self.publish_usage();
        append_sse_event(
            &mut out,
            "response.completed",
            &json!({
                "type": "response.completed",
                "response": self.response_payload("completed"),
            }),
        );
        out.into_bytes()
    }

    fn publish_usage(&self) {
        if let Ok(mut usage) = self.usage_collector.lock() {
            usage.input_tokens = Some(self.state.input_tokens);
            usage.cached_input_tokens = Some(self.state.cached_input_tokens);
            usage.cache_write_tokens = Some(self.state.cache_write_tokens);
            usage.output_tokens = Some(self.state.output_tokens);
            usage.total_tokens = self.state.total_tokens;
            usage.reasoning_output_tokens = Some(self.state.reasoning_output_tokens);
            if !self.state.output_text.trim().is_empty() {
                usage.output_text = Some(self.state.output_text.clone());
            }
        }
    }

    fn response_payload(&self, status: &str) -> Value {
        json!({
            "id": self.response_id(),
            "object": "response",
            "created_at": 0,
            "status": status,
            "model": self.model(),
            "output": if status == "completed" { self.completed_output() } else { Value::Array(Vec::new()) },
            "usage": self.usage_payload(),
        })
    }

    fn completed_output(&self) -> Value {
        let mut output = Vec::new();
        if !self.state.output_text.is_empty() {
            output.push(json!({
                "id": self.text_item_id(),
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": self.state.output_text }],
            }));
        }
        output.extend(self.state.completed_tools.iter().cloned());
        Value::Array(output)
    }

    fn completed_output_len(&self) -> usize {
        usize::from(!self.state.output_text.is_empty()) + self.state.completed_tools.len()
    }

    fn usage_payload(&self) -> Value {
        json!({
            "input_tokens": self.state.input_tokens,
            "output_tokens": self.state.output_tokens,
            "total_tokens": self
                .state
                .total_tokens
                .unwrap_or(self.state.input_tokens + self.state.output_tokens),
            "input_tokens_details": {
                "cached_tokens": self.state.cached_input_tokens,
                "cache_write_tokens": self.state.cache_write_tokens,
            },
            "output_tokens_details": { "reasoning_tokens": self.state.reasoning_output_tokens },
        })
    }

    fn response_id(&self) -> String {
        self.state
            .response_id
            .clone()
            .unwrap_or_else(|| "resp_codexmanager".to_string())
    }

    fn text_item_id(&self) -> String {
        format!("msg_{}", self.response_id())
    }

    fn model(&self) -> String {
        self.state.model.clone().unwrap_or_default()
    }
}

impl crate::http::gateway_response_body::GatewayResponseBody for ResponsesFromAnthropicSseReader {
    fn read_async<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> crate::http::gateway_response_body::BodyReadFuture<'a> {
        Box::pin(async move {
            loop {
                let n = self.out_cursor.read(buf)?;
                if n > 0 {
                    return Ok(n);
                }
                let chunk = self.next_chunk().await?;
                if chunk.is_empty() {
                    return Ok(0);
                }
                self.out_cursor = Cursor::new(chunk);
            }
        })
    }
}

#[cfg(test)]
impl Read for ResponsesFromAnthropicSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        crate::gateway::response_test_runtime()?.block_on(
            crate::http::gateway_response_body::GatewayResponseBody::read_async(self, buf),
        )
    }
}

fn normalize_json_fragment(value: &str) -> String {
    if value.trim().is_empty() {
        return "{}".to_string();
    }
    serde_json::from_str::<Value>(value)
        .map(|json| json.to_string())
        .unwrap_or_else(|_| value.to_string())
}

fn append_sse_event(buffer: &mut String, event: &str, payload: &Value) {
    buffer.push_str("event: ");
    buffer.push_str(event);
    buffer.push('\n');
    buffer.push_str("data: ");
    buffer.push_str(payload.to_string().as_str());
    buffer.push_str("\n\n");
}

fn usage_i64(usage: &Map<String, Value>, paths: &[&str]) -> Option<i64> {
    for path in paths {
        let mut current: Option<&Value> = None;
        let mut found = true;
        for (index, segment) in path.split('.').enumerate() {
            current = if index == 0 {
                usage.get(segment)
            } else {
                current
                    .and_then(Value::as_object)
                    .and_then(|object| object.get(segment))
            };
            if current.is_none() {
                found = false;
                break;
            }
        }
        if !found {
            continue;
        }
        if let Some(value) = current.and_then(Value::as_i64) {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
#[path = "responses_from_anthropic_tests.rs"]
mod tests;
