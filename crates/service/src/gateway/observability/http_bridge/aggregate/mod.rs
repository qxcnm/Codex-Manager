pub(super) mod openai_responses_event;
mod output_text;
mod sse_aggregate;
mod sse_frame;

pub(super) use output_text::append_output_text;
pub(crate) use output_text::UpstreamResponseBridgeResult;
pub(super) use output_text::{
    append_output_text_raw, collect_output_text_from_event_fields, collect_response_output_text,
    collect_response_reasoning_summary_text, extract_error_hint_from_body,
    extract_error_message_from_json, merge_usage, parse_usage_from_json,
    reload_from_env as reload_output_text_from_env, usage_has_signal, UpstreamResponseUsage,
};
#[cfg(test)]
pub(super) use output_text::{output_text_limit_bytes, OUTPUT_TEXT_TRUNCATED_MARKER};
pub(super) use sse_aggregate::{collect_non_stream_json_from_sse_bytes, looks_like_sse_payload};
#[cfg(test)]
pub(super) use sse_frame::parse_usage_from_sse_frame;
pub(crate) use sse_frame::PassthroughSseProtocol;
pub(super) use sse_frame::{
    inspect_sse_frame, is_response_completed_event_name, parse_sse_frame_json,
};
pub(super) use sse_frame::{inspect_sse_frame_for_protocol, SseTerminal};
