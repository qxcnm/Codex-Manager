use super::super::response_outcome::{is_output_limit_incomplete, TextDeliveryTracker};
use super::common::mark_anthropic_terminal_delivered;
use super::{
    append_output_text, classify_upstream_stream_read_error, collect_output_text_from_event_fields,
    collect_response_output_text, inspect_sse_frame_for_protocol, json, mark_first_response_ms,
    should_emit_keepalive_after_first_frame, stream_idle_timed_out, stream_idle_timeout_message,
    stream_reader_disconnected_message, stream_wait_timeout,
    upstream_hint_or_stream_incomplete_message, Arc, Cursor, Map, Mutex, PassthroughSseCollector,
    PassthroughSseProtocol, Read, SseKeepAliveFrame, SseTerminal, UpstreamSseFramePump,
    UpstreamSseFramePumpItem, Value,
};
use std::collections::HashSet;
use std::time::Instant;

const MAX_TRACKED_TOOL_CALLS: usize = 1024;
const MAX_TRACKED_TOOL_ID_BYTES: usize = 256 * 1024;

pub(crate) struct AnthropicSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    state: AnthropicSseState,
    tool_name_restore_map: Option<crate::gateway::ToolNameRestoreMap>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
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
    delivered_text: TextDeliveryTracker,
    output_limited: bool,
    stop_reason: Option<&'static str>,
    emitted_tool_ids: HashSet<String>,
    emitted_tool_count: usize,
    emitted_tool_id_bytes: usize,
}

impl AnthropicSseReader {
    #[cfg(test)]
    pub(crate) fn from_reader<R>(
        upstream: R,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
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
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
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
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
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
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
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
                    mark_first_response_ms(&self.usage_collector, self.request_started_at);
                    let mapped = self.process_sse_frame(&frame);
                    if !mapped.is_empty() {
                        mark_first_response_ms(&self.usage_collector, self.request_started_at);
                        return Ok(mapped);
                    }
                    continue;
                }
                Ok(UpstreamSseFramePumpItem::Eof) => {
                    return Ok(self.transport_error_chunk(
                        "upstream_stream_incomplete",
                        upstream_hint_or_stream_incomplete_message(None),
                    ));
                }
                Ok(UpstreamSseFramePumpItem::Error(err)) => {
                    let classified = classify_upstream_stream_read_error(&err);
                    let message = if err.trim().is_empty() || classified == err {
                        classified
                    } else {
                        format!("{classified}: {err}")
                    };
                    return Ok(self.transport_error_chunk("upstream_stream_read_error", message));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    self.last_upstream_activity = self.last_upstream_activity.max(
                        self.upstream
                            .last_activity()
                            .unwrap_or(self.last_upstream_activity),
                    );
                    if stream_idle_timed_out(self.last_upstream_activity) {
                        return Ok(self.transport_error_chunk(
                            "upstream_stream_idle_timeout",
                            stream_idle_timeout_message(),
                        ));
                    }
                    if should_emit_keepalive_after_first_frame(self.saw_upstream_frame) {
                        return Ok(SseKeepAliveFrame::Anthropic.bytes().to_vec());
                    }
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Ok(self.transport_error_chunk(
                        "upstream_stream_disconnected",
                        stream_reader_disconnected_message(),
                    ));
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
        if self.state.finished {
            return Vec::new();
        }
        let mut data_lines = Vec::new();
        let mut event_name = None;
        for line in lines {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = trimmed.strip_prefix("data:") {
                data_lines.push(rest.trim_start().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("event:") {
                event_name = Some(rest.trim());
            }
        }
        let data = data_lines.join("\n");
        let value = serde_json::from_str::<Value>(&data).unwrap_or(Value::Null);
        self.capture_response_meta(&value);
        let payload_event = value.get("type").and_then(Value::as_str);
        let event_type = event_name
            .filter(|name| !name.is_empty() && *name != "message")
            .or(payload_event);
        if let Ok(mut collector) = self.usage_collector.lock() {
            if let Some(event_type) = event_type {
                collector.last_event_type = Some(event_type.to_string());
            }
        }
        if is_output_limit_incomplete(&value) {
            let mut snapshot = String::new();
            collect_response_output_text(value.get("response").unwrap_or(&value), &mut snapshot);
            if !snapshot.trim().is_empty() || self.state.delivered_text.has_visible_text() {
                let mut out = String::new();
                if let Some(suffix) = self
                    .state
                    .delivered_text
                    .missing_suffix(&snapshot)
                    .filter(|s| !s.is_empty())
                {
                    self.emit_text_delta(&mut out, suffix);
                }
                self.state.output_limited = true;
                self.state.stop_reason = Some("max_tokens");
                // Do not synthesize tools from an incomplete terminal snapshot.
                let mut bytes = out.into_bytes();
                bytes.extend(self.finish_stream());
                return bytes;
            }
        }
        let inspection = inspect_sse_frame_for_protocol(lines, PassthroughSseProtocol::Generic);
        let failure_event = [event_name, payload_event]
            .into_iter()
            .flatten()
            .find(|name| Self::is_response_failure_event(name));
        let completion_event = [event_name, payload_event]
            .into_iter()
            .flatten()
            .any(|name| matches!(name, "response.completed" | "response.done"));
        // A terminal event name cannot override an explicitly failed snapshot.
        // Older compatible upstreams omit status; retain that compatibility.
        let invalid_completion = completion_event
            && (value
                .pointer("/response/status")
                .and_then(Value::as_str)
                .is_some_and(|status| status != "completed")
                || [
                    "/response/error",
                    "/response/status_details/error",
                    "/error",
                ]
                .iter()
                .any(|pointer| value.pointer(pointer).is_some_and(|error| !error.is_null())));
        if failure_event.is_some()
            || invalid_completion
            || matches!(inspection.terminal, Some(SseTerminal::Err(_)))
        {
            let upstream_error = value
                .pointer("/response/error")
                .filter(|error| !error.is_null())
                .or_else(|| {
                    value
                        .pointer("/response/status_details/error")
                        .filter(|error| !error.is_null())
                })
                .or_else(|| value.get("error").filter(|error| !error.is_null()))
                .or_else(|| (event_type == Some("error")).then_some(&value));
            let incomplete = failure_event == Some("response.incomplete")
                || value.pointer("/response/status").and_then(Value::as_str) == Some("incomplete");
            let code = if incomplete {
                "upstream_response_incomplete"
            } else {
                "upstream_response_failed"
            };
            let field = |name: &str| {
                upstream_error
                    .and_then(|error| error.get(name))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            };
            let detail = value
                .pointer("/response/incomplete_details/reason")
                .and_then(Value::as_str)
                .or_else(|| upstream_error.and_then(Value::as_str));
            let fallback_message = match inspection.terminal {
                Some(SseTerminal::Err(message)) => message,
                _ => failure_event.unwrap_or(code).to_string(),
            };
            return self.error_chunk(json!({
                "type": field("type").filter(|kind| kind != "error").unwrap_or_else(|| "api_error".to_string()),
                "code": field("code").unwrap_or_else(|| code.to_string()),
                "message": field("message").unwrap_or_else(|| detail.unwrap_or(&fallback_message).to_string()),
                "param": field("param"),
            }), true);
        }
        if data.trim() == "[DONE]" {
            return self.transport_error_chunk(
                "upstream_stream_incomplete",
                upstream_hint_or_stream_incomplete_message(None),
            );
        }
        if value.is_null() {
            return Vec::new();
        }
        self.consume_openai_event(&value, event_type)
    }

    fn is_response_failure_event(name: &str) -> bool {
        matches!(
            name.trim().to_ascii_lowercase().as_str(),
            "error"
                | "response.error"
                | "response.failed"
                | "response.incomplete"
                | "response.cancelled"
                | "response.canceled"
        )
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
    fn consume_openai_event(&mut self, value: &Value, event_type: Option<&str>) -> Vec<u8> {
        let mut out = String::new();
        let Some(event_type) = event_type else {
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
                self.emit_text_delta(&mut out, fragment);
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
                if let Err((code, message)) = self.append_tool_call(item_obj, &mut out) {
                    return self.transport_error_chunk(code, message.to_string());
                }
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
                        if let Some(suffix) = self
                            .state
                            .delivered_text
                            .missing_suffix(&extracted_output_text)
                            .filter(|s| !s.is_empty())
                        {
                            self.emit_text_delta(&mut out, suffix);
                        }
                        self.state.stop_reason.get_or_insert("end_turn");
                    }
                    if let Some(items) = response.get("output").and_then(Value::as_array) {
                        for item in items {
                            if let Some(item) = item.as_object() {
                                if let Err((code, message)) = self.append_tool_call(item, &mut out)
                                {
                                    return self.transport_error_chunk(code, message.to_string());
                                }
                            }
                        }
                    }
                }
                let mut completed = out.into_bytes();
                completed.extend(self.finish_stream());
                return completed;
            }
            _ => {}
        }
        out.into_bytes()
    }

    fn emit_text_delta(&mut self, out: &mut String, fragment: &str) {
        append_output_text(&mut self.state.output_text, fragment);
        self.ensure_message_start(out);
        self.ensure_text_block_start(out);
        append_sse_event(
            out,
            "content_block_delta",
            &json!({
                "type": "content_block_delta", "index": self.state.text_block_index.unwrap_or(0),
                "delta": {"type": "text_delta", "text": fragment}
            }),
        );
        self.state.delivered_text.record(fragment);
        self.state.stop_reason.get_or_insert("end_turn");
    }

    fn append_tool_call(
        &mut self,
        item: &Map<String, Value>,
        out: &mut String,
    ) -> Result<(), (&'static str, &'static str)> {
        if !matches!(
            item.get("type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call")
        ) {
            return Ok(());
        }
        let call_id = item
            .get("call_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let item_id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        if call_id.is_none() && item_id.is_none() {
            return Err((
                "upstream_tool_call_missing_id",
                "upstream tool call has no usable call_id or id",
            ));
        }
        let identity_bytes = call_id
            .map_or(0, |id| id.len().saturating_add(5))
            .saturating_add(item_id.map_or(0, |id| id.len().saturating_add(5)));
        // Check before allocating owned ID copies, including a single oversized ID.
        if identity_bytes > MAX_TRACKED_TOOL_ID_BYTES {
            return Err((
                "upstream_tool_tracking_limit",
                "upstream tool ID tracking byte limit exceeded",
            ));
        }
        let identities = call_id
            .map(|id| format!("call:{id}"))
            .into_iter()
            .chain(item_id.map(|id| format!("item:{id}")))
            .collect::<Vec<_>>();
        if identities
            .iter()
            .any(|id| self.state.emitted_tool_ids.contains(id))
        {
            return Ok(());
        }
        if self.state.emitted_tool_count >= MAX_TRACKED_TOOL_CALLS
            || self
                .state
                .emitted_tool_id_bytes
                .saturating_add(identity_bytes)
                > MAX_TRACKED_TOOL_ID_BYTES
        {
            return Err((
                "upstream_tool_tracking_limit",
                "upstream tool ID tracking limit exceeded",
            ));
        }
        self.state.emitted_tool_count += 1;
        self.state.emitted_tool_id_bytes += identity_bytes;
        self.state.emitted_tool_ids.extend(identities);
        self.ensure_message_start(out);
        self.close_text_block(out);
        let index = self.state.next_block_index;
        self.state.next_block_index = self.state.next_block_index.saturating_add(1);
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(|name| restore_tool_name(name, self.tool_name_restore_map.as_ref()))
            .unwrap_or_else(|| "tool".to_string());
        append_sse_event(
            out,
            "content_block_start",
            &json!({
                "type": "content_block_start", "index": index,
                "content_block": {"type": "tool_use", "id": call_id.or(item_id).unwrap_or("toolu_unknown"), "name": name, "input": {}}
            }),
        );
        if let Some(partial_json) =
            extract_function_call_input(item).and_then(tool_input_partial_json)
        {
            append_sse_event(
                out,
                "content_block_delta",
                &json!({
                    "type": "content_block_delta", "index": index,
                    "delta": {"type": "input_json_delta", "partial_json": partial_json}
                }),
            );
        }
        append_sse_event(
            out,
            "content_block_stop",
            &json!({"type": "content_block_stop", "index": index}),
        );
        self.state.stop_reason = Some("tool_use");
        Ok(())
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

    fn persist_usage(&self) {
        if let Ok(mut collector) = self.usage_collector.lock() {
            let usage = &mut collector.usage;
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
    }

    fn error_chunk(&mut self, error: Value, saw_terminal: bool) -> Vec<u8> {
        if self.state.finished {
            return Vec::new();
        }
        self.state.finished = true;
        self.persist_usage();
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.saw_terminal |= saw_terminal;
            collector.usage.explicit_failure |= saw_terminal;
            collector.usage.completed_successfully = false;
            collector.terminal_error = error
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        let mut out = String::new();
        append_sse_event(
            &mut out,
            "error",
            &json!({ "type": "error", "error": error }),
        );
        out.into_bytes()
    }

    fn transport_error_chunk(&mut self, code: &str, message: String) -> Vec<u8> {
        self.error_chunk(
            json!({
                "type": "api_error", "code": code, "message": message, "param": null,
            }),
            false,
        )
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
        self.persist_usage();
        if let Ok(mut collector) = self.usage_collector.lock() {
            collector.saw_terminal = true;
            collector.usage.completed_successfully = !self.state.output_limited;
            if self.state.output_limited {
                collector.usage.explicit_failure = false;
            }
            collector.usage.response_id = self.state.response_id.clone();
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
                    mark_anthropic_terminal_delivered(&self.usage_collector, &buf[..read]);
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
