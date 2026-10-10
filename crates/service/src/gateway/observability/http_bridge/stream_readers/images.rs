use super::{
    build_images_api_response, classify_upstream_stream_read_error,
    collect_image_generation_results, image_generation_result_payload, images_usage_value,
    mark_first_response_ms, merge_usage, stream_idle_timed_out, stream_idle_timeout_message,
    stream_reader_disconnected_message, stream_wait_timeout,
    upstream_hint_or_stream_incomplete_message, Arc, Cursor, ImagesResponseFormat, Mutex,
    PassthroughSseCollector, Read, SseKeepAliveFrame, UpstreamSseFramePump,
    UpstreamSseFramePumpItem,
};
use serde_json::Value;
use std::time::Instant;

pub(crate) struct ImagesFromResponsesSseReader {
    upstream: UpstreamSseFramePump,
    out_cursor: Cursor<Vec<u8>>,
    usage_collector: Arc<Mutex<PassthroughSseCollector>>,
    request_started_at: Instant,
    last_upstream_activity: Instant,
    response_format: ImagesResponseFormat,
    finished: bool,
}

impl ImagesFromResponsesSseReader {
    #[cfg(test)]
    pub(crate) fn new(
        upstream: reqwest::blocking::Response,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
        response_format: ImagesResponseFormat,
    ) -> Self {
        Self::from_pump(
            UpstreamSseFramePump::new(upstream),
            usage_collector,
            request_started_at,
            response_format,
        )
    }

    pub(crate) fn from_stream_response(
        upstream: crate::gateway::upstream::GatewayStreamResponse,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
        response_format: ImagesResponseFormat,
    ) -> Self {
        Self::from_pump(
            UpstreamSseFramePump::from_stream(upstream.into_body()),
            usage_collector,
            request_started_at,
            response_format,
        )
    }

    pub(crate) fn from_pump(
        upstream: UpstreamSseFramePump,
        usage_collector: Arc<Mutex<PassthroughSseCollector>>,
        request_started_at: Instant,
        response_format: ImagesResponseFormat,
    ) -> Self {
        Self {
            upstream,
            out_cursor: Cursor::new(Vec::new()),
            usage_collector,
            request_started_at,
            last_upstream_activity: Instant::now(),
            response_format,
            finished: false,
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

    fn sse_event(event_name: &str, payload: Value) -> Vec<u8> {
        format!(
            "event: {event_name}\ndata: {}\n\n",
            serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
        )
        .into_bytes()
    }

    fn partial_image_chunk(&self, value: &Value) -> Option<Vec<u8>> {
        let b64 = value
            .get("partial_image_b64")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let event_name = "image_generation.partial_image";
        let mut payload = serde_json::json!({
            "type": event_name,
            "partial_image_index": value
                .get("partial_image_index")
                .and_then(Value::as_i64)
                .unwrap_or(0)
        });
        match self.response_format {
            ImagesResponseFormat::Url => {
                let mime_type = super::super::mime_type_from_codex_output_format(
                    value.get("output_format").and_then(Value::as_str),
                );
                payload["url"] = Value::String(format!("data:{mime_type};base64,{b64}"));
            }
            ImagesResponseFormat::B64Json => {
                payload["b64_json"] = Value::String(b64.to_string());
            }
        }
        Some(Self::sse_event(event_name, payload))
    }

    fn completed_chunks(&mut self, response: &Value) -> Vec<u8> {
        if let Some(usage) = images_usage_value(response) {
            if let Ok(mut collector) = self.usage_collector.lock() {
                merge_usage(
                    &mut collector.usage,
                    super::super::parse_usage_from_json(&serde_json::json!({ "usage": usage })),
                );
            }
        }

        let results = collect_image_generation_results(response);
        let event_name = "image_generation.completed";
        let mut out = Vec::new();
        for result in results {
            let mut payload = image_generation_result_payload(&result, self.response_format);
            if let Some(payload_obj) = payload.as_object_mut() {
                payload_obj.insert("type".to_string(), Value::String(event_name.to_string()));
                if let Some(usage) = images_usage_value(response) {
                    payload_obj.insert("usage".to_string(), usage);
                }
            }
            out.extend(Self::sse_event(event_name, payload));
        }
        out
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
        let event_type = Self::event_type(lines, &value);
        match event_type.as_deref() {
            Some("response.image_generation_call.partial_image") => {
                self.partial_image_chunk(&value)
            }
            Some("response.completed") | Some("response.done") => {
                let mut out = Vec::new();
                if let Some(response) = value.get("response") {
                    out.extend(self.completed_chunks(response));
                } else {
                    out.extend(Self::sse_event(
                        "image_generation.completed",
                        build_images_api_response(&value, self.response_format),
                    ));
                }
                self.update_terminal_success(event_type.as_deref());
                self.finished = true;
                Some(out)
            }
            _ => None,
        }
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
                    if crate::gateway::current_sse_keepalive_enabled() {
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

impl crate::http::gateway_response_body::GatewayResponseBody for ImagesFromResponsesSseReader {
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
impl Read for ImagesFromResponsesSseReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        crate::gateway::response_test_runtime()?.block_on(
            crate::http::gateway_response_body::GatewayResponseBody::read_async(self, buf),
        )
    }
}
