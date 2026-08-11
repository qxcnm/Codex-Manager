use std::io::Read;
use std::sync::{mpsc, Arc, Mutex};
use tiny_http::Request;

use crate::gateway::upstream::GatewayUpstreamResponse;

mod aggregate;
mod body_conversion;
mod compact_delivery;
mod compact_errors;
mod images;
mod manual_chunked;
mod metadata;
#[cfg(test)]
mod openai;
mod response_helpers;
use aggregate::openai_responses_event::{CanonicalEvent, OpenAIResponsesOutputTextState};
pub(crate) use aggregate::PassthroughSseProtocol;
#[allow(unused_imports)]
use aggregate::{
    append_output_text, collect_output_text_from_event_fields, collect_response_output_text,
    collect_response_reasoning_summary_text,
};
use aggregate::{
    collect_non_stream_json_from_sse_bytes, extract_error_hint_from_body,
    extract_error_message_from_json, inspect_sse_frame_for_protocol, looks_like_sse_payload,
    merge_usage, parse_usage_from_json, reload_output_text_from_env, usage_has_signal, SseTerminal,
    UpstreamResponseBridgeResult, UpstreamResponseUsage,
};
#[cfg(test)]
use aggregate::{
    inspect_sse_frame, output_text_limit_bytes, parse_sse_frame_json, parse_usage_from_sse_frame,
    OUTPUT_TEXT_TRUNCATED_MARKER,
};
use images::{
    build_images_api_response, chat_image_payload, collect_image_generation_chat_images,
    collect_image_generation_data_urls, collect_image_generation_results,
    image_generation_result_payload, images_usage_value, mime_type_from_codex_output_format,
    ImagesResponseFormat,
};

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
    reload_output_text_from_env();
    stream_readers::reload_from_env();
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
    stream_readers::current_sse_keepalive_interval_ms()
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
    stream_readers::set_sse_keepalive_interval_ms(interval_ms)
}

/// 函数 `summarize_upstream_error_hint_from_body`
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
pub(crate) fn summarize_upstream_error_hint_from_body(
    status_code: u16,
    body: &[u8],
) -> Option<String> {
    aggregate::extract_error_hint_from_body(status_code, body)
}

/// Converts the vendored Kiro Anthropic SSE stream into canonical OpenAI
/// Responses SSE before the normal client-facing adapter runs.
pub(crate) fn adapt_anthropic_gateway_stream_to_responses(
    stream: crate::gateway::upstream::GatewayByteStream,
    fallback_model: &str,
    request_started_at: std::time::Instant,
) -> crate::gateway::upstream::GatewayByteStream {
    struct StreamReader {
        stream: crate::gateway::upstream::GatewayByteStream,
        current: std::io::Cursor<Vec<u8>>,
        finished: bool,
    }

    impl Read for StreamReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            loop {
                let read = self.current.read(output)?;
                if read > 0 {
                    return Ok(read);
                }
                if self.finished {
                    return Ok(0);
                }
                match self.stream.recv() {
                    Ok(crate::gateway::upstream::GatewayByteStreamItem::Chunk(bytes)) => {
                        self.current = std::io::Cursor::new(bytes.to_vec());
                    }
                    Ok(crate::gateway::upstream::GatewayByteStreamItem::Eof) | Err(_) => {
                        self.finished = true;
                    }
                    Ok(crate::gateway::upstream::GatewayByteStreamItem::Error(error)) => {
                        return Err(std::io::Error::other(error));
                    }
                }
            }
        }
    }

    let (tx, rx) = mpsc::sync_channel(128);
    let spawn_error_tx = tx.clone();
    let fallback_model = fallback_model.to_string();
    if let Err(error) = std::thread::Builder::new()
        .name("codexmanager-kiro-responses-adapter".into())
        .spawn(move || {
            let reader = StreamReader {
                stream,
                current: std::io::Cursor::new(Vec::new()),
                finished: false,
            };
            let usage = Arc::new(Mutex::new(UpstreamResponseUsage::default()));
            let mut adapter = ResponsesFromAnthropicSseReader::from_reader(
                reader,
                usage,
                Some(fallback_model.as_str()),
                request_started_at,
            );
            let mut buffer = vec![0_u8; 8 * 1024];
            loop {
                match adapter.read(&mut buffer) {
                    Ok(0) => {
                        let _ = tx.send(crate::gateway::upstream::GatewayByteStreamItem::Eof);
                        return;
                    }
                    Ok(read) => {
                        if tx
                            .send(crate::gateway::upstream::GatewayByteStreamItem::Chunk(
                                bytes::Bytes::copy_from_slice(&buffer[..read]),
                            ))
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(crate::gateway::upstream::GatewayByteStreamItem::Error(
                            error.to_string(),
                        ));
                        return;
                    }
                }
            }
        })
    {
        let _ = spawn_error_tx.send(crate::gateway::upstream::GatewayByteStreamItem::Error(
            format!("spawn Kiro Responses adapter failed: {error}"),
        ));
    }
    crate::gateway::upstream::GatewayByteStream::from_receiver(rx)
}

mod delivery;
mod stream_readers;
/// 函数 `respond_with_upstream`
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
pub(super) fn respond_with_upstream(
    request: Request,
    upstream: GatewayUpstreamResponse,
    inflight_guard: super::AccountInFlightGuard,
    response_adapter: super::ResponseAdapter,
    passthrough_sse_protocol: Option<PassthroughSseProtocol>,
    gemini_stream_output_mode: Option<super::GeminiStreamOutputMode>,
    request_path: &str,
    tool_name_restore_map: Option<&super::ToolNameRestoreMap>,
    is_stream: bool,
    allow_failover_for_deactivation: bool,
    trace_id: Option<&str>,
    fallback_model: Option<&str>,
    request_started_at: std::time::Instant,
) -> Result<UpstreamResponseBridgeResult, String> {
    match upstream {
        GatewayUpstreamResponse::Blocking(upstream) => delivery::respond_with_upstream(
            request,
            upstream,
            inflight_guard,
            response_adapter,
            passthrough_sse_protocol,
            gemini_stream_output_mode,
            request_path,
            tool_name_restore_map,
            is_stream,
            allow_failover_for_deactivation,
            trace_id,
            fallback_model,
            request_started_at,
        ),
        GatewayUpstreamResponse::Stream(upstream) => delivery::respond_with_stream_upstream(
            request,
            upstream,
            inflight_guard,
            response_adapter,
            passthrough_sse_protocol,
            gemini_stream_output_mode,
            request_path,
            tool_name_restore_map,
            is_stream,
            allow_failover_for_deactivation,
            trace_id,
            fallback_model,
            request_started_at,
        ),
    }
}
pub(super) use stream_readers::{
    ChatCompletionsFromResponsesSseReader, ImagesFromResponsesSseReader,
    OpenAIResponsesPassthroughSseReader, PassthroughSseCollector, PassthroughSseUsageReader,
    ResponsesFromAnthropicSseReader, SseKeepAliveFrame,
};

pub(super) use stream_readers::{AnthropicSseReader, GeminiSseReader};

#[cfg(test)]
#[path = "../tests/http_bridge_tests.rs"]
mod tests;
