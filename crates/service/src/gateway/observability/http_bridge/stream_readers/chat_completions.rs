use super::{
    chat_image_payload, classify_upstream_stream_read_error, collect_image_generation_data_urls,
    collect_output_text_from_event_fields, collect_response_output_text,
    collect_response_reasoning_summary_text, mark_first_response_ms, merge_usage,
    should_emit_keepalive_after_first_frame, stream_idle_timed_out, stream_idle_timeout_message,
    stream_reader_disconnected_message, stream_wait_timeout,
    upstream_hint_or_stream_incomplete_message, Arc, Cursor, Mutex, PassthroughSseCollector, Read,
    SseKeepAliveFrame, UpstreamSseFramePump, UpstreamSseFramePumpItem,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub(crate) struct ChatCompletionsFromResponsesSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    saw_upstream_frame: bool,
    finished: bool,
    emitted_assistant_role: bool,
    emitted_text: bool,
    emitted_reasoning: bool,
    emitted_tool_calls: bool,
    emitted_tool_call_indices: HashSet<usize>,
    tool_call_arguments: HashMap<usize, String>,
    emitted_image_urls: HashSet<String>,
    id: Option<String>,
    model: Option<String>,
    created: Option<i64>,
}

impl ChatCompletionsFromResponsesSseReader {
    #[cfg(test)]
    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_pump(
            UpstreamSseFramePump::new(upstream),
            usage_collector,
            request_started_at,
        )
    }

    pub(crate) fn from_stream_response(
        upstream: crate::gateway::upstream::GatewayStreamResponse,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_pump(
            UpstreamSseFramePump::from_stream(upstream.into_body()),
            usage_collector,
            request_started_at,
        )
    }

    pub(crate) fn from_pump(
        upstream: UpstreamSseFramePump,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
    ) -> Self {
        Self {
            upstream,
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            saw_upstream_frame: false,
            finished: false,
            emitted_assistant_role: false,
            emitted_text: false,
            emitted_reasoning: false,
            emitted_tool_calls: false,
            emitted_tool_call_indices: HashSet::new(),
            tool_call_arguments: HashMap::new(),
            emitted_image_urls: HashSet::new(),
            id: None,
            model: None,
            created: None,
        }
    }

    fn data_json(lines: &[String]) -> Option<Value> {
        let mut data = String::new();
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        if data.is_empty() || data.trim() == "[DONE]" {
            return None;
        }
        serde_json::from_str(data.as_str()).ok()
    }

    fn event_type(lines: &[String], value: &Value) -> Option<String> {
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']).trim_start();
            if let Some(rest) = trimmed.strip_prefix("event:") {
                let event = rest.trim();
                if !event.is_empty() {
                    return Some(event.to_string());
                }
            }
        }
        value
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn remember_meta(&mut self, value: &Value) {
        let response = value.get("response");
        if self.id.is_none() {
            self.id = response
                .and_then(|v| v.get("id"))
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if self.model.is_none() {
            self.model = response
                .and_then(|v| v.get("model"))
                .or_else(|| value.get("model"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if self.created.is_none() {
            self.created = response
                .and_then(|v| v.get("created_at"))
                .or_else(|| response.and_then(|v| v.get("created")))
                .or_else(|| value.get("created_at"))
                .or_else(|| value.get("created"))
                .and_then(Value::as_i64);
        }
        if let Some(usage) = response
            .and_then(|v| v.get("usage"))
            .or_else(|| value.get("usage"))
            .cloned()
        {
            if let Ok(mut collector) = self.usage_collector.lock() {
                merge_usage(
                    &mut collector.usage,
                    super::super::parse_usage_from_json(&serde_json::json!({ "usage": usage })),
                );
            }
        }
    }

    fn chat_id(&self) -> String {
        self.id
            .clone()
            .unwrap_or_else(|| "chatcmpl_codexmanager".to_string())
    }

    fn chat_model(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| super::super::DEFAULT_BRIDGE_MODEL.to_string())
    }

    fn chat_created(&self) -> i64 {
        self.created.unwrap_or(0)
    }

    fn chunk(&self, delta: Value, finish_reason: Option<&str>, usage: Option<Value>) -> Vec<u8> {
        let mut chunk = serde_json::json!({
            "id": self.chat_id(),
            "object": "chat.completion.chunk",
            "created": self.chat_created(),
            "model": self.chat_model(),
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason
            }]
        });
        if let Some(usage) = usage {
            chunk["usage"] = usage;
        }
        format!(
            "data: {}\n\n",
            serde_json::to_string(&chunk).unwrap_or_else(|_| "{}".to_string())
        )
        .into_bytes()
    }

    fn final_chunk(&self) -> Vec<u8> {
        let usage = self.usage_collector.lock().ok().map(|collector| {
            serde_json::json!({
                "prompt_tokens": collector.usage.input_tokens.unwrap_or(0),
                "completion_tokens": collector.usage.output_tokens.unwrap_or(0),
                "total_tokens": collector.usage.total_tokens.unwrap_or(0)
            })
        });
        let finish_reason = if self.emitted_tool_calls {
            "tool_calls"
        } else {
            "stop"
        };
        let mut out = self.chunk(serde_json::json!({}), Some(finish_reason), usage);
        out.extend_from_slice(b"data: [DONE]\n\n");
        out
    }

    fn assistant_delta_chunk(&mut self, mut delta: Value) -> Vec<u8> {
        if !self.emitted_assistant_role {
            if let Some(object) = delta.as_object_mut() {
                object.insert("role".to_string(), serde_json::json!("assistant"));
            }
            self.emitted_assistant_role = true;
        }
        self.chunk(delta, None, None)
    }

    fn output_index(value: &Value) -> usize {
        value
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize
    }

    fn function_call_item(value: &Value) -> Option<&Value> {
        let item = value.get("item").or_else(|| value.get("output_item"))?;
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            Some(item)
        } else {
            None
        }
    }

    fn tool_call_added_chunk(&mut self, value: &Value) -> Option<Vec<u8>> {
        let item = Self::function_call_item(value)?;
        let index = Self::output_index(value);
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let name = item.get("name").and_then(Value::as_str).unwrap_or_default();
        let arguments = item
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or_default();
        self.emitted_tool_calls = true;
        self.emitted_tool_call_indices.insert(index);
        self.tool_call_arguments
            .entry(index)
            .or_insert_with(|| arguments.to_string());
        Some(self.assistant_delta_chunk(serde_json::json!({
            "tool_calls": [{
                "index": index,
                "id": call_id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": arguments
                }
            }]
        })))
    }

    fn tool_call_arguments_delta_chunk(&mut self, value: &Value) -> Option<Vec<u8>> {
        let delta = value.get("delta").and_then(Value::as_str)?;
        let index = Self::output_index(value);
        self.emitted_tool_calls = true;
        self.tool_call_arguments
            .entry(index)
            .or_default()
            .push_str(delta);
        Some(self.assistant_delta_chunk(serde_json::json!({
            "tool_calls": [{
                "index": index,
                "function": {
                    "arguments": delta
                }
            }]
        })))
    }

    fn tool_call_arguments_done_chunk(&mut self, value: &Value) -> Option<Vec<u8>> {
        let arguments = value.get("arguments").and_then(Value::as_str)?;
        let index = Self::output_index(value);
        self.remaining_tool_call_arguments_chunk(index, arguments)
    }

    fn tool_call_done_chunk(&mut self, value: &Value) -> Option<Vec<u8>> {
        let index = Self::output_index(value);
        if self.emitted_tool_call_indices.contains(&index) {
            let item = Self::function_call_item(value)?;
            let arguments = item.get("arguments").and_then(Value::as_str)?;
            return self.remaining_tool_call_arguments_chunk(index, arguments);
        }
        self.tool_call_added_chunk(value)
    }

    fn remaining_tool_call_arguments_chunk(
        &mut self,
        index: usize,
        arguments: &str,
    ) -> Option<Vec<u8>> {
        let accumulated = self.tool_call_arguments.entry(index).or_default();
        let remaining = arguments
            .strip_prefix(accumulated.as_str())
            .unwrap_or(arguments);
        if remaining.is_empty() {
            return None;
        }
        accumulated.push_str(remaining);
        self.emitted_tool_calls = true;
        Some(self.assistant_delta_chunk(serde_json::json!({
            "tool_calls": [{
                "index": index,
                "function": {
                    "arguments": remaining
                }
            }]
        })))
    }

    fn completed_tool_calls_chunk(&mut self, response: &Value) -> Option<Vec<u8>> {
        let output = response.get("output").and_then(Value::as_array)?;
        let mut out = Vec::new();
        for (index, item) in output.iter().enumerate() {
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                continue;
            }
            if self.emitted_tool_call_indices.contains(&index) {
                continue;
            }
            if let Some(chunk) = self.tool_call_added_chunk(&serde_json::json!({
                "output_index": index,
                "item": item
            })) {
                out.extend(chunk);
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    fn image_delta_chunk(&mut self, value: &Value) -> Option<Vec<u8>> {
        let images = collect_image_generation_data_urls(value)
            .into_iter()
            .filter(|url| self.emitted_image_urls.insert(url.clone()))
            .enumerate()
            .map(|(index, url)| chat_image_payload(url, index))
            .collect::<Vec<_>>();
        if images.is_empty() {
            None
        } else {
            Some(self.assistant_delta_chunk(serde_json::json!({ "images": images })))
        }
    }

    fn reasoning_delta_chunk(&mut self, text: String) -> Option<Vec<u8>> {
        if text.trim().is_empty() {
            return None;
        }
        self.emitted_reasoning = true;
        let reasoning_content = text.clone();
        Some(self.assistant_delta_chunk(serde_json::json!({
            "reasoning": text,
            "reasoning_content": reasoning_content
        })))
    }

    fn reasoning_text_from_event(event_type: Option<&str>, value: &Value) -> Option<String> {
        let mut reasoning = String::new();
        match event_type {
            Some("response.reasoning_summary_text.delta")
            | Some("response.reasoning_text.delta") => {
                if let Some(delta) = value.get("delta") {
                    collect_response_output_text(delta, &mut reasoning);
                }
            }
            Some("response.reasoning_summary_text.done") | Some("response.reasoning_text.done") => {
                if let Some(text) = value.get("text").or_else(|| value.get("delta")) {
                    collect_response_output_text(text, &mut reasoning);
                }
            }
            _ => collect_response_reasoning_summary_text(value, &mut reasoning),
        }
        if reasoning.trim().is_empty() {
            None
        } else {
            Some(reasoning)
        }
    }

    fn is_reasoning_delta_event(event_type: Option<&str>) -> bool {
        matches!(
            event_type,
            Some("response.reasoning_summary_text.delta") | Some("response.reasoning_text.delta")
        )
    }

    fn update_terminal_success(&self, event_type: Option<&str>) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(event_type) = event_type {
                collector.last_event_type = Some(event_type.to_string());
            }
            collector.saw_terminal = true;
        }
    }

    fn handle_frame(&mut self, lines: &[String]) -> Option<Vec<u8>> {
        let value = Self::data_json(lines)?;
        self.remember_meta(&value);
        let event_type = Self::event_type(lines, &value);
        let is_terminal_event = matches!(
            event_type.as_deref(),
            Some("response.completed") | Some("response.done")
        );
        if !is_terminal_event
            && (Self::is_reasoning_delta_event(event_type.as_deref()) || !self.emitted_reasoning)
        {
            if let Some(reasoning) = Self::reasoning_text_from_event(event_type.as_deref(), &value)
            {
                if let Some(chunk) = self.reasoning_delta_chunk(reasoning) {
                    return Some(chunk);
                }
            }
        }
        let mut text = String::new();
        match event_type.as_deref() {
            Some("response.output_text.delta") | Some("response.content_part.delta") => {
                if let Some(delta) = value.get("delta") {
                    collect_response_output_text(delta, &mut text);
                }
            }
            Some("response.output_text.done") => {
                if !self.emitted_text {
                    if let Some(done_text) = value.get("text") {
                        collect_response_output_text(done_text, &mut text);
                    } else if let Some(delta) = value.get("delta") {
                        collect_response_output_text(delta, &mut text);
                    }
                }
            }
            Some("response.content_part.done") => {
                if !self.emitted_text {
                    collect_output_text_from_event_fields(&value, &mut text);
                    if text.is_empty() {
                        if let Some(delta) = value.get("delta") {
                            collect_response_output_text(delta, &mut text);
                        }
                    }
                }
            }
            _ => {}
        }
        if matches!(
            event_type.as_deref(),
            Some("response.completed") | Some("response.done")
        ) {
            let mut out = Vec::new();
            if !self.emitted_text {
                if let Some(response) = value.get("response") {
                    collect_response_output_text(response, &mut text);
                }
                if !text.is_empty() {
                    out.extend(self.assistant_delta_chunk(serde_json::json!({ "content": text })));
                    self.emitted_text = true;
                }
            }
            if let Some(response) = value.get("response") {
                if !self.emitted_reasoning {
                    if let Some(reasoning) =
                        Self::reasoning_text_from_event(event_type.as_deref(), response)
                    {
                        if let Some(chunk) = self.reasoning_delta_chunk(reasoning) {
                            out.extend(chunk);
                        }
                    }
                }
                if let Some(tool_calls) = self.completed_tool_calls_chunk(response) {
                    out.extend(tool_calls);
                }
                if let Some(images) = self.image_delta_chunk(response) {
                    out.extend(images);
                }
            }
            self.update_terminal_success(event_type.as_deref());
            self.finished = true;
            out.extend(self.final_chunk());
            return Some(out);
        }
        if event_type.as_deref() == Some("response.output_item.done") {
            if let Some(tool_call) = self.tool_call_done_chunk(&value) {
                return Some(tool_call);
            }
            if let Some(images) = self.image_delta_chunk(&value) {
                return Some(images);
            }
            if !self.emitted_text {
                collect_output_text_from_event_fields(&value, &mut text);
            }
        }
        if event_type.as_deref() == Some("response.output_item.added") {
            if let Some(tool_call) = self.tool_call_added_chunk(&value) {
                return Some(tool_call);
            }
        }
        if event_type.as_deref() == Some("response.function_call_arguments.delta") {
            if let Some(tool_call) = self.tool_call_arguments_delta_chunk(&value) {
                return Some(tool_call);
            }
        }
        if event_type.as_deref() == Some("response.function_call_arguments.done") {
            if let Some(tool_call) = self.tool_call_arguments_done_chunk(&value) {
                return Some(tool_call);
            }
        }
        if event_type.as_deref() == Some("response.image_generation_call.partial_image") {
            if let Some(images) = self.image_delta_chunk(&value) {
                return Some(images);
            }
        }
        if text.is_empty() {
            if let Some(response) = value.get("response") {
                collect_response_output_text(response, &mut text);
            }
        }
        if !text.is_empty() {
            self.emitted_text = true;
            return Some(self.assistant_delta_chunk(serde_json::json!({ "content": text })));
        }
        None
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
                    mark_first_response_ms(&self.usage_collector, self.request_started_at);
                    if let Some(chunk) = self.handle_frame(&frame) {
                        return Ok(chunk);
                    }
                    continue;
                }
                Ok(UpstreamSseFramePumpItem::Eof) => {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        if !collector.saw_terminal {
                            let hint = collector.upstream_error_hint.clone();
                            collector.terminal_error.get_or_insert_with(|| {
                                upstream_hint_or_stream_incomplete_message(hint.as_deref())
                            });
                        }
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
                Ok(UpstreamSseFramePumpItem::Error(err)) => {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        collector
                            .terminal_error
                            .get_or_insert_with(|| classify_upstream_stream_read_error(&err));
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if stream_idle_timed_out(self.last_upstream_activity) {
                        if let Ok(mut collector) = self.usage_collector.lock() {
                            collector
                                .terminal_error
                                .get_or_insert_with(stream_idle_timeout_message);
                        }
                        self.finished = true;
                        return Ok(Vec::new());
                    }
                    if should_emit_keepalive_after_first_frame(self.saw_upstream_frame) {
                        return Ok(SseKeepAliveFrame::Comment.bytes().to_vec());
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    if let Ok(mut collector) = self.usage_collector.lock() {
                        let hint = collector.upstream_error_hint.clone();
                        collector.terminal_error.get_or_insert_with(|| {
                            hint.unwrap_or_else(stream_reader_disconnected_message)
                        });
                    }
                    self.finished = true;
                    return Ok(Vec::new());
                }
            }
        }
    }
}

impl crate::http::gateway_response_body::GatewayResponseBody
    for ChatCompletionsFromResponsesSseReader
{
    fn read_async<'a>(
        &'a mut self,
        buf: &'a mut [u8],
    ) -> crate::http::gateway_response_body::BodyReadFuture<'a> {
        Box::pin(async move {
            loop {
                let read = self.out_cursor.read(buf)?;
                if read > 0 {
                    return Ok(read);
                }
                if self.finished {
                    return Ok(0);
                }
                self.out_cursor = Cursor::new(self.next_chunk().await?);
            }
        })
    }
}

#[cfg(test)]
impl Read for ChatCompletionsFromResponsesSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        crate::gateway::response_test_runtime()?.block_on(
            crate::http::gateway_response_body::GatewayResponseBody::read_async(self, buf),
        )
    }
}
