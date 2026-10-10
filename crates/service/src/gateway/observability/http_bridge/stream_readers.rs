use serde_json::{json, Map, Value};
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex};

use super::{
    append_output_text, collect_output_text_from_event_fields, collect_response_output_text,
    collect_response_reasoning_summary_text, extract_error_hint_from_body,
    extract_error_message_from_json, merge_usage,
};
use super::{
    build_images_api_response, chat_image_payload, collect_image_generation_data_urls,
    collect_image_generation_results, image_generation_result_payload, images_usage_value,
    inspect_sse_frame_for_protocol, ImagesResponseFormat, OpenAIResponsesEvent,
    OpenAIResponsesOutputTextState, PassthroughSseProtocol, SseTerminal, UpstreamResponseUsage,
};
#[path = "stream_readers/anthropic.rs"]
mod anthropic;
#[path = "stream_readers/chat_completions.rs"]
mod chat_completions;
#[path = "stream_readers/common.rs"]
mod common;
#[path = "stream_readers/gemini.rs"]
mod gemini;
#[path = "stream_readers/images.rs"]
mod images;
#[path = "stream_readers/openai_responses.rs"]
mod openai_responses;
#[path = "stream_readers/passthrough.rs"]
mod passthrough;
#[path = "stream_readers/responses_from_anthropic.rs"]
mod responses_from_anthropic;

pub(crate) use anthropic::AnthropicSseReader;
pub(crate) use chat_completions::ChatCompletionsFromResponsesSseReader;
use common::{
    classify_upstream_stream_read_error, mark_first_response_ms,
    should_emit_keepalive_after_first_frame, stream_idle_timed_out, stream_idle_timeout_message,
    stream_reader_disconnected_message, stream_wait_timeout,
    upstream_hint_or_stream_incomplete_message,
};
use common::{mark_collector_terminal_success, mark_first_response_ms_on_usage};
pub(crate) use common::{
    mark_terminal_delivered, PassthroughSseCollector, SseKeepAliveFrame, UpstreamSseFramePump,
    UpstreamSseFramePumpItem,
};
pub(crate) use gemini::GeminiSseReader;
pub(crate) use images::ImagesFromResponsesSseReader;
pub(crate) use openai_responses::OpenAIResponsesPassthroughSseReader;
pub(crate) use passthrough::PassthroughSseUsageReader;
pub(crate) use responses_from_anthropic::ResponsesFromAnthropicSseReader;

#[cfg(test)]
struct SseKeepaliveRuntimeGuard {
    enabled_env: Option<std::ffi::OsString>,
    interval_env: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl SseKeepaliveRuntimeGuard {
    fn enabled_with_interval(interval_ms: u64) -> Self {
        const ENABLED_ENV: &str = "CODEXMANAGER_SSE_KEEPALIVE_ENABLED";
        const INTERVAL_ENV: &str = "CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS";
        let guard = Self {
            enabled_env: std::env::var_os(ENABLED_ENV),
            interval_env: std::env::var_os(INTERVAL_ENV),
        };
        std::env::set_var(ENABLED_ENV, "1");
        std::env::set_var(INTERVAL_ENV, interval_ms.to_string());
        crate::gateway::reload_runtime_config_from_env();
        guard
    }
}

#[cfg(test)]
impl Drop for SseKeepaliveRuntimeGuard {
    fn drop(&mut self) {
        const ENABLED_ENV: &str = "CODEXMANAGER_SSE_KEEPALIVE_ENABLED";
        const INTERVAL_ENV: &str = "CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS";
        if let Some(value) = &self.enabled_env {
            std::env::set_var(ENABLED_ENV, value);
        } else {
            std::env::remove_var(ENABLED_ENV);
        }
        if let Some(value) = &self.interval_env {
            std::env::set_var(INTERVAL_ENV, value);
        } else {
            std::env::remove_var(INTERVAL_ENV);
        }
        let _ = std::panic::catch_unwind(crate::gateway::reload_runtime_config_from_env);
    }
}
