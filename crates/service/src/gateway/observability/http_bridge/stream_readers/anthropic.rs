use super::{
    append_output_text, collect_output_text_from_event_fields, collect_response_output_text, json,
    mark_first_response_ms_on_usage, should_emit_keepalive_after_first_frame,
    stream_idle_timed_out, stream_wait_timeout, Arc, Cursor, Map, Mutex, Read, SseKeepAliveFrame,
    UpstreamResponseUsage, UpstreamSseFramePump, UpstreamSseFramePumpItem, Value,
};
use std::time::Instant;

pub(crate) struct AnthropicSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    state: AnthropicSseState,
    tool_name_restore_map: Option<crate::gateway::ToolNameRestoreMap>,
    usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    saw_upstream_frame: bool,
}

#[derive(Default)]
struct AnthropicSseState {
    started: bool,
    finished: bool,
    text_block_index: Option<usize>,
    next_block_index: usize,
    response_id: Option<String>,
    model: Option<String>,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    total_tokens: Option<i64>,
    reasoning_output_tokens: i64,
    output_text: String,
    emitted_text_to_client: bool,
    stop_reason: Option<&'static str>,
}

impl AnthropicSseReader {
    #[cfg(test)]
    pub(crate) fn from_reader<R>(
        upstream: R,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        tool_name_restore_map: Option<crate::gateway::ToolNameRestoreMap>,
        request_started_at: Instant,
    ) -> Self
    where
        R: Read + Send + 'static,
    {
        Self::from_pump(
            UpstreamSseFramePump::from_reader(upstream),
            usage_collector,
            fallback_model,
            tool_name_restore_map,
            request_started_at,
        )
    }

    pub(crate) fn from_stream_response(
        upstream: crate::gateway::upstream::GatewayStreamResponse,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        tool_name_restore_map: Option<crate::gateway::ToolNameRestoreMap>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_pump(
            UpstreamSseFramePump::from_stream(upstream.into_body()),
            usage_collector,
            fallback_model,
            tool_name_restore_map,
            request_started_at,
        )
    }

    pub(crate) fn from_pump(
        upstream: UpstreamSseFramePump,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        tool_name_restore_map: Option<crate::gateway::ToolNameRestoreMap>,
        request_started_at: Instant,
    ) -> Self {
        let mut state = AnthropicSseState::default();
        state.model = fallback_model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        Self {
            upstream,
            out_cursor: Cursor::new(Vec::new()),
            state,
            tool_name_restore_map,
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            saw_upstream_frame: false,
        }
    }

    /// 函数 `new`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 返回函数执行结果
    #[cfg(test)]
    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<UpstreamResponseUsage>>,
        fallback_model: Option<&str>,
        tool_name_restore_map: Option<crate::gateway::ToolNameRestoreMap>,
        request_started_at: Instant,
    ) -> Self {
        Self::from_reader(
            upstream,
            usage_collector,
            fallback_model,
            tool_name_restore_map,
            request_started_at,
        )
    }

    /// 函数 `next_chunk`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
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
                    continue;
                }
                Ok(UpstreamSseFramePumpItem::Eof) => {
                    self.last_upstream_activity = Instant::now();
                    let finished = self.finish_stream();
                    if !finished.is_empty() {
                        mark_first_response_ms_on_usage(
                            &self.usage_collector,
                            self.request_started_at,
                        );
                    }
                    return Ok(finished);
                }
                Ok(UpstreamSseFramePumpItem::Error(_err)) => {
                    self.last_upstream_activity = Instant::now();
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
                        return Ok(SseKeepAliveFrame::Anthropic.bytes().to_vec());
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    let finished = self.finish_stream();
                    if !finished.is_empty() {
                        mark_first_response_ms_on_usage(
                            &self.usage_collector,
                            self.request_started_at,
                        );
                    }
                    return Ok(finished);
                }
            }
        }
    }

    /// 函数 `process_sse_frame`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - lines: 参数 lines
    ///
    /// # 返回
    /// 返回函数执行结果
    fn process_sse_frame(&mut self, lines: &[String]) -> Vec<u8> {
        let mut data_lines = Vec::new();
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
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
        self.consume_openai_event(&value)
    }

    /// 函数 `consume_openai_event`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn consume_openai_event(&mut self, value: &Value) -> Vec<u8> {
        self.capture_response_meta(value);
        let mut out = String::new();
        let Some(event_type) = value.get("type").and_then(Value::as_str) else {
            return Vec::new();
        };
        match event_type {
            "response.output_text.delta" => {
                let fragment = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if fragment.is_empty() {
                    return Vec::new();
                }
                append_output_text(&mut self.state.output_text, fragment);
                self.ensure_message_start(&mut out);
                self.ensure_text_block_start(&mut out);
                let text_index = self.state.text_block_index.unwrap_or(0);
                append_sse_event(
                    &mut out,
                    "content_block_delta",
                    &json!({
                        "type": "content_block_delta",
                        "index": text_index,
                        "delta": {
                            "type": "text_delta",
                            "text": fragment
                        }
                    }),
                );
                self.state.emitted_text_to_client = true;
                self.state.stop_reason.get_or_insert("end_turn");
            }
            "response.output_item.done" => {
                collect_output_text_from_event_fields(value, &mut self.state.output_text);
                let Some(item_obj) = value
                    .get("item")
                    .or_else(|| value.get("output_item"))
                    .and_then(Value::as_object)
                else {
                    return Vec::new();
                };
                if item_obj
                    .get("type")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind != "function_call")
                {
                    return Vec::new();
                }
                self.ensure_message_start(&mut out);
                self.close_text_block(&mut out);
                let block_index = self.state.next_block_index;
                self.state.next_block_index = self.state.next_block_index.saturating_add(1);
                let tool_use_id = item_obj
                    .get("call_id")
                    .or_else(|| item_obj.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or("toolu_unknown");
                let tool_name = item_obj
                    .get("name")
                    .and_then(Value::as_str)
                    .map(|name| restore_tool_name(name, self.tool_name_restore_map.as_ref()))
                    .unwrap_or_else(|| "tool".to_string());
                append_sse_event(
                    &mut out,
                    "content_block_start",
                    &json!({
                        "type": "content_block_start",
                        "index": block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": tool_use_id,
                            "name": tool_name,
                            "input": {}
                        }
                    }),
                );
                if let Some(partial_json) =
                    extract_function_call_input(item_obj).and_then(tool_input_partial_json)
                {
                    append_sse_event(
                        &mut out,
                        "content_block_delta",
                        &json!({
                            "type": "content_block_delta",
                            "index": block_index,
                            "delta": {
                                "type": "input_json_delta",
                                "partial_json": partial_json,
                            }
                        }),
                    );
                }
                append_sse_event(
                    &mut out,
                    "content_block_stop",
                    &json!({
                        "type": "content_block_stop",
                        "index": block_index
                    }),
                );
                self.state.stop_reason = Some("tool_use");
            }
            _ if event_type.starts_with("response.output_item.")
                || event_type.starts_with("response.content_part.") =>
            {
                collect_output_text_from_event_fields(value, &mut self.state.output_text);
            }
            "response.completed" | "response.done" => {
                if let Some(response) = value.get("response") {
                    let mut extracted_output_text = String::new();
                    collect_response_output_text(response, &mut extracted_output_text);
                    if !extracted_output_text.trim().is_empty() {
                        // Completed carries a full text snapshot. If any text delta was already
                        // delivered, do not replay that snapshot after tool blocks close text.
                        if !self.state.emitted_text_to_client {
                            append_output_text(
                                &mut self.state.output_text,
                                extracted_output_text.as_str(),
                            );
                            self.ensure_message_start(&mut out);
                            self.ensure_text_block_start(&mut out);
                            let text_index = self.state.text_block_index.unwrap_or(0);
                            append_sse_event(
                                &mut out,
                                "content_block_delta",
                                &json!({
                                    "type": "content_block_delta",
                                    "index": text_index,
                                    "delta": {
                                        "type": "text_delta",
                                        "text": extracted_output_text
                                    }
                                }),
                            );
                            self.state.emitted_text_to_client = true;
                        }
                        self.state.stop_reason.get_or_insert("end_turn");
                    }
                }
            }
            _ => {}
        }
        out.into_bytes()
    }

    /// 函数 `capture_response_meta`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - value: 参数 value
    ///
    /// # 返回
    /// 无
    fn capture_response_meta(&mut self, value: &Value) {
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            self.state.response_id = Some(id.to_string());
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            self.state.model = Some(model.to_string());
        }
        if let Some(response) = value.get("response").and_then(Value::as_object) {
            if let Some(id) = response.get("id").and_then(Value::as_str) {
                self.state.response_id = Some(id.to_string());
            }
            if let Some(model) = response.get("model").and_then(Value::as_str) {
                self.state.model = Some(model.to_string());
            }
            if let Some(usage) = response.get("usage").and_then(Value::as_object) {
                self.state.input_tokens = usage
                    .get("input_tokens")
                    .and_then(Value::as_i64)
                    .or_else(|| usage.get("prompt_tokens").and_then(Value::as_i64))
                    .unwrap_or(self.state.input_tokens);
                self.state.cached_input_tokens = usage
                    .get("cache_read_input_tokens")
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        usage
                            .get("input_tokens_details")
                            .and_then(Value::as_object)
                            .and_then(|details| details.get("cached_tokens"))
                            .and_then(Value::as_i64)
                    })
                    .or_else(|| {
                        usage
                            .get("prompt_tokens_details")
                            .and_then(Value::as_object)
                            .and_then(|details| details.get("cached_tokens"))
                            .and_then(Value::as_i64)
                    })
                    .unwrap_or(self.state.cached_input_tokens);
                self.state.cache_write_tokens = usage
                    .get("input_tokens_details")
                    .and_then(Value::as_object)
                    .and_then(|details| details.get("cache_write_tokens"))
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        usage
                            .get("prompt_tokens_details")
                            .and_then(Value::as_object)
                            .and_then(|details| details.get("cache_write_tokens"))
                            .and_then(Value::as_i64)
                    })
                    .or_else(|| {
                        usage
                            .get("cache_write_input_tokens")
                            .and_then(Value::as_i64)
                    })
                    .unwrap_or(self.state.cache_write_tokens);
                self.state.output_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_i64)
                    .or_else(|| usage.get("completion_tokens").and_then(Value::as_i64))
                    .unwrap_or(self.state.output_tokens);
                self.state.total_tokens = usage
                    .get("total_tokens")
                    .and_then(Value::as_i64)
                    .or(self.state.total_tokens);
                self.state.reasoning_output_tokens = usage
                    .get("reasoning_output_tokens")
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        usage
                            .get("output_tokens_details")
                            .and_then(Value::as_object)
                            .and_then(|details| details.get("reasoning_tokens"))
                            .and_then(Value::as_i64)
                    })
                    .or_else(|| {
                        usage
                            .get("completion_tokens_details")
                            .and_then(Value::as_object)
                            .and_then(|details| details.get("reasoning_tokens"))
                            .and_then(Value::as_i64)
                    })
                    .unwrap_or(self.state.reasoning_output_tokens);
            }
        }
    }

    /// 函数 `ensure_message_start`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - out: 参数 out
    ///
    /// # 返回
    /// 无
    fn ensure_message_start(&mut self, out: &mut String) {
        if self.state.started {
            return;
        }
        self.state.started = true;
        append_sse_event(
            out,
            "message_start",
            &json!({
                "type": "message_start",
                "message": {
                    "id": self.state.response_id.clone().unwrap_or_else(|| "msg_proxy".to_string()),
                    "type": "message",
                    "role": "assistant",
                    "model": self.state.model.clone().unwrap_or_else(|| super::super::DEFAULT_BRIDGE_MODEL.to_string()),
                    "content": [],
                    "stop_reason": Value::Null,
                    "stop_sequence": Value::Null,
                    "usage": build_anthropic_usage(
                        self.state.input_tokens.max(0),
                        0,
                        self.state.cached_input_tokens.max(0),
                        self.state.cache_write_tokens.max(0),
                        None,
                        None,
                    )
                }
            }),
        );
    }

    /// 函数 `ensure_text_block_start`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - out: 参数 out
    ///
    /// # 返回
    /// 无
    fn ensure_text_block_start(&mut self, out: &mut String) {
        if self.state.text_block_index.is_some() {
            return;
        }
        let index = self.state.next_block_index;
        self.state.next_block_index = self.state.next_block_index.saturating_add(1);
        self.state.text_block_index = Some(index);
        append_sse_event(
            out,
            "content_block_start",
            &json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "text",
                    "text": ""
                }
            }),
        );
    }

    /// 函数 `close_text_block`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - out: 参数 out
    ///
    /// # 返回
    /// 无
    fn close_text_block(&mut self, out: &mut String) {
        let Some(index) = self.state.text_block_index.take() else {
            return;
        };
        append_sse_event(
            out,
            "content_block_stop",
            &json!({
                "type": "content_block_stop",
                "index": index
            }),
        );
    }

    /// 函数 `finish_stream`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn finish_stream(&mut self) -> Vec<u8> {
        if self.state.finished {
            return Vec::new();
        }
        self.state.finished = true;
        if let Ok(mut usage) = self.usage_collector.lock() {
            usage.input_tokens = Some(self.state.input_tokens.max(0));
            usage.cached_input_tokens = Some(self.state.cached_input_tokens.max(0));
            usage.cache_write_tokens = Some(self.state.cache_write_tokens.max(0));
            usage.output_tokens = Some(self.state.output_tokens.max(0));
            usage.total_tokens = self.state.total_tokens.map(|value| value.max(0));
            usage.reasoning_output_tokens = Some(self.state.reasoning_output_tokens.max(0));
            if !self.state.output_text.trim().is_empty() {
                usage.output_text = Some(self.state.output_text.clone());
            }
        }
        let mut out = String::new();
        self.ensure_message_start(&mut out);
        self.close_text_block(&mut out);
        append_sse_event(
            &mut out,
            "message_delta",
            &json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": self.state.stop_reason.unwrap_or("end_turn"),
                    "stop_sequence": Value::Null
                },
                "usage": build_anthropic_usage(
                    self.state.input_tokens.max(0),
                    self.state.output_tokens.max(0),
                    self.state.cached_input_tokens.max(0),
                    self.state.cache_write_tokens.max(0),
                    self.state.total_tokens.map(|value| value.max(0)),
                    Some(self.state.reasoning_output_tokens.max(0)),
                )
            }),
        );
        append_sse_event(&mut out, "message_stop", &json!({ "type": "message_stop" }));
        out.into_bytes()
    }
}

impl crate::http::gateway_response_body::GatewayResponseBody for AnthropicSseReader {
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
                if self.state.finished {
                    return Ok(0);
                }
                let next = self.next_chunk().await?;
                self.out_cursor = Cursor::new(next);
            }
        })
    }
}

#[cfg(test)]
impl Read for AnthropicSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        crate::gateway::response_test_runtime()?.block_on(
            crate::http::gateway_response_body::GatewayResponseBody::read_async(self, buf),
        )
    }
}

/// 函数 `append_sse_event`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - buffer: 参数 buffer
/// - event_name: 参数 event_name
/// - payload: 参数 payload
///
/// # 返回
/// 无
fn append_sse_event(buffer: &mut String, event_name: &str, payload: &Value) {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    buffer.push_str("event: ");
    buffer.push_str(event_name);
    buffer.push('\n');
    buffer.push_str("data: ");
    buffer.push_str(&data);
    buffer.push_str("\n\n");
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;

/// 函数 `build_anthropic_usage`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - input_tokens: 参数 input_tokens
/// - output_tokens: 参数 output_tokens
/// - cache_read_input_tokens: 参数 cache_read_input_tokens
/// - total_tokens: 参数 total_tokens
/// - reasoning_output_tokens: 参数 reasoning_output_tokens
///
/// # 返回
/// 返回函数执行结果
fn build_anthropic_usage(
    input_tokens: i64,
    output_tokens: i64,
    cache_read_input_tokens: i64,
    cache_creation_input_tokens: i64,
    total_tokens: Option<i64>,
    reasoning_output_tokens: Option<i64>,
) -> Value {
    let mut usage = Map::new();
    usage.insert("input_tokens".to_string(), Value::from(input_tokens));
    usage.insert("output_tokens".to_string(), Value::from(output_tokens));
    usage.insert(
        "cache_read_input_tokens".to_string(),
        Value::from(cache_read_input_tokens),
    );
    usage.insert(
        "cache_creation_input_tokens".to_string(),
        Value::from(cache_creation_input_tokens),
    );
    if let Some(value) = total_tokens {
        usage.insert("total_tokens".to_string(), Value::from(value));
    }
    if let Some(value) = reasoning_output_tokens {
        usage.insert("reasoning_output_tokens".to_string(), Value::from(value));
    }
    Value::Object(usage)
}

/// 函数 `extract_function_call_input`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - item_obj: 参数 item_obj
///
/// # 返回
/// 返回函数执行结果
fn extract_function_call_input(item_obj: &Map<String, Value>) -> Option<Value> {
    const ARGUMENT_KEYS: [&str; 5] = [
        "arguments",
        "input",
        "arguments_json",
        "parsed_arguments",
        "args",
    ];
    for key in ARGUMENT_KEYS {
        let Some(value) = item_obj.get(key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        if let Some(text) = value.as_str() {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                return Some(parsed);
            }
            return Some(Value::String(trimmed.to_string()));
        }
        return Some(value.clone());
    }
    None
}

/// 函数 `tool_input_partial_json`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn tool_input_partial_json(value: Value) -> Option<String> {
    let serialized = serde_json::to_string(&value).ok()?;
    let trimmed = serialized.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return None;
    }
    Some(trimmed.to_string())
}

fn restore_tool_name(
    name: &str,
    tool_name_restore_map: Option<&crate::gateway::ToolNameRestoreMap>,
) -> String {
    tool_name_restore_map
        .and_then(|map| map.get(name))
        .cloned()
        .unwrap_or_else(|| name.to_string())
}
