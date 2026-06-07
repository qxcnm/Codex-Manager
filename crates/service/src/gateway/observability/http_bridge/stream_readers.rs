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
    classify_upstream_stream_read_error, mark_first_response_ms, should_emit_keepalive,
    stream_idle_timed_out, stream_idle_timeout_message, stream_reader_disconnected_message,
    stream_wait_timeout, upstream_hint_or_stream_incomplete_message,
};
use common::{mark_collector_terminal_success, mark_first_response_ms_on_usage};
pub(crate) use common::{
    PassthroughSseCollector, SseKeepAliveFrame, UpstreamSseFramePump, UpstreamSseFramePumpItem,
};
pub(crate) use gemini::GeminiSseReader;
pub(crate) use images::ImagesFromResponsesSseReader;
pub(crate) use openai_responses::OpenAIResponsesPassthroughSseReader;
pub(crate) use passthrough::PassthroughSseUsageReader;
pub(crate) use responses_from_anthropic::ResponsesFromAnthropicSseReader;

/// 函数 `reload_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn reload_from_env() {
    common::reload_from_env();
}

/// 函数 `current_sse_keepalive_interval_ms`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn current_sse_keepalive_interval_ms() -> u64 {
    common::current_sse_keepalive_interval_ms()
}

/// 函数 `set_sse_keepalive_interval_ms`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn set_sse_keepalive_interval_ms(interval_ms: u64) -> Result<u64, String> {
    common::set_sse_keepalive_interval_ms(interval_ms)
}
