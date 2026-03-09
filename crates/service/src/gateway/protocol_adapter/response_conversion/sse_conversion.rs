#[path = "sse_conversion/anthropic_sse_writer.rs"]
mod anthropic_sse_writer;
#[path = "sse_conversion/anthropic_sse_reader.rs"]
mod anthropic_sse_reader;
#[path = "sse_conversion/openai_sse_anthropic_bridge.rs"]
mod openai_sse_anthropic_bridge;

pub(super) fn convert_anthropic_json_to_sse(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    anthropic_sse_writer::convert_anthropic_json_to_sse(body)
}

pub(super) fn convert_openai_sse_to_anthropic(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    openai_sse_anthropic_bridge::convert_openai_sse_to_anthropic(body)
}

pub(super) fn convert_anthropic_sse_to_json(
    body: &[u8],
) -> Result<(Vec<u8>, &'static str), String> {
    anthropic_sse_reader::convert_anthropic_sse_to_json(body)
}
