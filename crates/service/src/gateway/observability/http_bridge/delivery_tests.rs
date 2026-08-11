use super::super::body_conversion::{
    convert_chat_completions_body_to_compact, convert_responses_body_to_chat_completions,
    convert_responses_body_to_gemini_generate_content, convert_responses_body_to_images,
    gemini_cli_wrap_response_envelope, merge_usage_from_body_without_output_text,
};
use super::super::compact_errors::{
    build_passthrough_non_success_message, classify_compact_non_success_kind,
    compact_non_success_body_should_be_normalized, compact_success_body_is_valid,
};
use super::{
    collect_non_stream_json_from_sse_bytes, force_openai_responses_stream_content_type,
    response_adapter_uses_manual_chunked_streaming, write_streaming_chunked_response,
    ImagesResponseFormat, ResponseAdapter, StatusCode,
};
use serde_json::json;
use std::io::{Read, Write};
use std::sync::mpsc;
use tiny_http::{HTTPVersion, Header};

struct ChunkedTestReader {
    chunks: Vec<&'static [u8]>,
    index: usize,
}

impl ChunkedTestReader {
    fn new(chunks: Vec<&'static [u8]>) -> Self {
        Self { chunks, index: 0 }
    }
}

impl Read for ChunkedTestReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let Some(chunk) = self.chunks.get(self.index) else {
            return Ok(0);
        };
        self.index += 1;
        let read = chunk.len().min(buf.len());
        buf[..read].copy_from_slice(&chunk[..read]);
        Ok(read)
    }
}

#[derive(Default)]
struct FlushCountingWriter {
    bytes: Vec<u8>,
    flushes: usize,
}

impl Write for FlushCountingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

#[test]
fn streaming_chunked_response_flushes_each_read_chunk() {
    let mut writer = FlushCountingWriter::default();
    let headers = vec![
        Header::from_bytes("Content-Type", "text/event-stream").expect("content-type header"),
        Header::from_bytes("Content-Length", "999").expect("content-length header"),
    ];
    let body = ChunkedTestReader::new(vec![b"data: a\n\n", b"data: b\n\n"]);

    write_streaming_chunked_response(
        &mut writer,
        &HTTPVersion(1, 1),
        StatusCode(200),
        &headers,
        body,
        false,
    )
    .expect("write streaming response");

    let output = String::from_utf8(writer.bytes).expect("utf8 response");
    assert!(output.contains("HTTP/1.1 200 OK\r\n"));
    assert!(output.contains("Content-Type: text/event-stream\r\n"));
    assert!(output.contains("X-Accel-Buffering: no\r\n"));
    assert!(output.contains("Transfer-Encoding: chunked\r\n"));
    assert!(!output.to_ascii_lowercase().contains("content-length: 999"));
    assert!(output.contains("9\r\ndata: a\n\n\r\n"));
    assert!(output.contains("9\r\ndata: b\n\n\r\n"));
    assert!(output.ends_with("0\r\n\r\n"));
    assert!(writer.flushes >= 4);
}

#[test]
fn responses_from_anthropic_streaming_uses_manual_chunked_delivery() {
    assert!(response_adapter_uses_manual_chunked_streaming(
        ResponseAdapter::ResponsesFromAnthropicMessages
    ));
}

/// 函数 `compact_header_only_identity_error_is_normalized_and_classified`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn compact_header_only_identity_error_is_normalized_and_classified() {
    assert!(compact_non_success_body_should_be_normalized(
        403,
        Some("text/plain"),
        b"",
        None,
        Some("org_membership_required"),
    ));
    assert_eq!(
        classify_compact_non_success_kind(
            403,
            Some("text/plain"),
            b"",
            None,
            None,
            Some("org_membership_required"),
        ),
        "identity_error"
    );
}

/// 函数 `compact_header_only_cf_ray_is_classified_as_cloudflare_edge`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn compact_header_only_cf_ray_is_classified_as_cloudflare_edge() {
    assert_eq!(
        classify_compact_non_success_kind(
            502,
            Some("text/plain"),
            b"",
            Some("ray_compact_edge"),
            None,
            None,
        ),
        "cloudflare_edge"
    );
}

#[test]
fn compact_success_body_matches_official_compact_response_shape() {
    assert!(compact_success_body_is_valid(
        json!({
            "output": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "keep context" }]
                },
                {
                    "type": "compaction",
                    "encrypted_content": "summary_payload"
                }
            ]
        })
        .to_string()
        .as_bytes()
    ));
    assert!(compact_success_body_is_valid(
        json!({
            "output": [
                {
                    "type": "context_compaction",
                    "encrypted_content": "summary_payload"
                }
            ]
        })
        .to_string()
        .as_bytes()
    ));
    assert!(compact_success_body_is_valid(
        json!({ "output": [] }).to_string().as_bytes()
    ));
    assert!(compact_success_body_is_valid(
        json!({
            "output": [
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": "done" }]
                }
            ]
        })
        .to_string()
        .as_bytes()
    ));
    assert!(!compact_success_body_is_valid(
        json!({ "id": "resp_missing_output" })
            .to_string()
            .as_bytes()
    ));
    assert!(!compact_success_body_is_valid(
        json!({
            "output": [
                {
                    "type": "message",
                    "role": "assistant"
                }
            ]
        })
        .to_string()
        .as_bytes()
    ));
}

#[test]
fn chat_completions_body_converts_to_compact_response_shape() {
    let body = json!({
        "id": "chatcmpl_custom_compact",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "压缩摘要"
            },
            "finish_reason": "stop"
        }]
    });
    let converted = convert_chat_completions_body_to_compact(body.to_string().as_bytes())
        .expect("convert chat completions response");

    assert!(compact_success_body_is_valid(converted.as_slice()));
    let value: serde_json::Value =
        serde_json::from_slice(converted.as_slice()).expect("compact json");
    assert_eq!(value["output"][0]["type"], "message");
    assert_eq!(value["output"][0]["role"], "assistant");
    assert_eq!(value["output"][0]["content"][0]["type"], "output_text");
    assert_eq!(value["output"][0]["content"][0]["text"], "压缩摘要");
}

#[test]
fn header_only_cloudflare_challenge_uses_stable_hint() {
    let message = build_passthrough_non_success_message(
        502,
        Some("text/html; charset=utf-8"),
        b"",
        Some("req-header-only"),
        Some("ray-header-only"),
        None,
        None,
    );

    assert!(message.contains("Cloudflare 安全验证页"));
    assert!(message.contains("cf_ray=ray-header-only"));
}

#[test]
fn cloudflare_html_preview_keeps_title_hint() {
    let message = build_passthrough_non_success_message(
        502,
        Some("text/html; charset=utf-8"),
        b"<html><head><title>Just a moment...</title></head><body>Cloudflare</body></html>",
        Some("req-preview"),
        Some("ray-preview"),
        None,
        None,
    );

    assert!(message.contains("Cloudflare 安全验证页（title=Just a moment...）"));
    assert!(message.contains("cf_ray=ray-preview"));
}

#[test]
fn streaming_responses_passthrough_forces_sse_content_type() {
    let mut headers = vec![
        Header::from_bytes(
            b"Content-Type".as_slice(),
            b"application/json; charset=utf-8".as_slice(),
        )
        .expect("content-type header"),
        Header::from_bytes(b"x-request-id".as_slice(), b"req_test".as_slice())
            .expect("request id header"),
    ];

    force_openai_responses_stream_content_type(&mut headers, "/v1/responses", true);

    let content_type = headers
        .iter()
        .find(|header| {
            header
                .field
                .as_str()
                .as_str()
                .eq_ignore_ascii_case("Content-Type")
        })
        .map(|header| header.value.as_str());
    assert_eq!(content_type, Some("text/event-stream"));
    assert!(headers
        .iter()
        .any(|header| header.field.as_str().as_str() == "x-request-id"));
}

#[test]
fn non_stream_gemini_response_preserves_function_call_id_and_top_level_function_calls() {
    let body = json!({
        "id": "resp_non_stream_tool",
        "model": "gpt-5.4",
        "output": [{
            "type": "function_call",
            "call_id": "call_non_stream_write",
            "name": "write_file",
            "arguments": "{\"path\":\"plan.md\"}"
        }],
        "usage": { "input_tokens": 1, "output_tokens": 1, "total_tokens": 2 }
    });

    let mapped = convert_responses_body_to_gemini_generate_content(
        serde_json::to_vec(&body).expect("body").as_slice(),
        false,
        None,
    )
    .expect("convert gemini body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(
        value["candidates"][0]["content"]["parts"][0]["functionCall"]["id"],
        "call_non_stream_write"
    );
    assert_eq!(value["functionCalls"][0]["id"], "call_non_stream_write");
    assert_eq!(value["functionCalls"][0]["args"]["path"], "plan.md");
}

#[test]
fn non_stream_chat_completion_response_adds_image_generation_message_images() {
    let body = json!({
        "id": "resp_non_stream_image",
        "model": "gpt-5.4",
        "output": [{
            "type": "image_generation_call",
            "id": "ig_non_stream_1",
            "status": "completed",
            "output_format": "png",
            "result": "aGVsbG8="
        }],
        "usage": { "input_tokens": 2, "output_tokens": 1, "total_tokens": 3 }
    });

    let mapped = convert_responses_body_to_chat_completions(
        serde_json::to_vec(&body).expect("body").as_slice(),
    )
    .expect("convert chat completion body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(
        value["choices"][0]["message"]["images"][0]["type"],
        "image_url"
    );
    assert_eq!(
        value["choices"][0]["message"]["images"][0]["image_url"]["url"],
        "data:image/png;base64,aGVsbG8="
    );
    assert_eq!(
        value["usage"]["prompt_tokens"],
        serde_json::Value::Number(2.into())
    );
}

#[test]
fn non_stream_chat_completion_response_preserves_reasoning_content() {
    let body = json!({
        "id": "resp_non_stream_reasoning",
        "model": "gpt-5.4",
        "output": [{
            "type": "reasoning",
            "id": "rs_non_stream_1",
            "summary": [{
                "type": "summary_text",
                "text": "先读配置"
            }]
        }],
        "usage": { "input_tokens": 4, "output_tokens": 2, "total_tokens": 6 }
    });

    let mapped = convert_responses_body_to_chat_completions(
        serde_json::to_vec(&body).expect("body").as_slice(),
    )
    .expect("convert chat completion body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(value["choices"][0]["message"]["content"], "");
    assert_eq!(
        value["choices"][0]["message"]["reasoning_content"],
        "先读配置"
    );
    assert_eq!(value["choices"][0]["message"]["reasoning"], "先读配置");
    assert_eq!(
        value["usage"]["prompt_tokens"],
        serde_json::Value::Number(4.into())
    );
}

#[test]
fn non_stream_chat_completion_response_preserves_answer_and_reasoning_content() {
    let body = json!({
        "id": "resp_non_stream_text_and_reasoning",
        "model": "gpt-5.4",
        "output_text": "OK",
        "output": [{
            "type": "reasoning",
            "id": "rs_non_stream_1",
            "summary": [{
                "type": "summary_text",
                "text": "先想一下"
            }]
        }]
    });

    let mapped = convert_responses_body_to_chat_completions(
        serde_json::to_vec(&body).expect("body").as_slice(),
    )
    .expect("convert chat completion body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(value["choices"][0]["message"]["content"], "OK");
    assert_eq!(
        value["choices"][0]["message"]["reasoning_content"],
        "先想一下"
    );
}

#[test]
fn non_stream_chat_completion_response_preserves_function_calls() {
    let body = json!({
        "id": "resp_non_stream_tool",
        "model": "claude-sonnet-4.6",
        "output": [{
            "type": "function_call",
            "id": "fc_1",
            "call_id": "toolu_1",
            "name": "read_file",
            "arguments": "{\"path\":\"/tmp/a\"}"
        }],
        "usage": { "input_tokens": 4, "output_tokens": 1, "total_tokens": 5 }
    });

    let mapped = convert_responses_body_to_chat_completions(
        serde_json::to_vec(&body).expect("body").as_slice(),
    )
    .expect("convert chat completion body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(
        value["choices"][0]["message"]["content"],
        serde_json::Value::Null
    );
    assert_eq!(value["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        value["choices"][0]["message"]["tool_calls"][0]["id"],
        "toolu_1"
    );
    assert_eq!(
        value["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "read_file"
    );
    assert_eq!(
        value["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"/tmp/a\"}"
    );
}

#[test]
fn kiro_anthropic_tool_stream_reaches_responses_and_non_stream_chat() {
    use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem};

    let (tx, rx) = mpsc::sync_channel(8);
    for frame in [
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_kiro_tool\",\"model\":\"claude-sonnet-4.6\",\"usage\":{\"input_tokens\":4}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_kiro_1\",\"name\":\"read_file\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"/tmp/a\\\"}\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":1}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    ] {
        tx.send(GatewayByteStreamItem::Chunk(bytes::Bytes::from(frame)))
            .expect("queue Kiro SSE frame");
    }
    tx.send(GatewayByteStreamItem::Eof)
        .expect("queue Kiro SSE eof");

    let responses_sse = crate::gateway::adapt_anthropic_gateway_stream_to_responses(
        GatewayByteStream::from_receiver(rx),
        "claude-sonnet-4.6",
        std::time::Instant::now(),
    )
    .read_all_bytes()
    .expect("canonicalize Kiro stream");
    let responses_text = String::from_utf8(responses_sse.to_vec()).expect("Responses SSE UTF-8");
    assert!(responses_text.contains("response.output_item.done"));
    assert!(responses_text.contains("toolu_kiro_1"), "{responses_text}");

    let (responses_body, _) = collect_non_stream_json_from_sse_bytes(responses_sse.as_ref());
    let responses_body = responses_body.expect("aggregate Responses body");
    let responses: serde_json::Value =
        serde_json::from_slice(&responses_body).expect("parse Responses body");
    assert_eq!(responses["output"][0]["type"], "function_call");
    assert_eq!(responses["output"][0]["call_id"], "toolu_kiro_1");

    let chat_body = convert_responses_body_to_chat_completions(&responses_body)
        .expect("convert non-stream Chat body");
    let chat: serde_json::Value = serde_json::from_slice(&chat_body).expect("parse Chat body");
    assert_eq!(chat["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        chat["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "read_file"
    );
    assert_eq!(
        chat["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"/tmp/a\"}"
    );
}

#[test]
fn non_stream_chat_responses_sse_json_mode_returns_single_parseable_content() {
    let sse = concat!(
        "event: response.output_text.delta\n",
        "data: {\"response_id\":\"resp_non_stream_json\",\"delta\":\"{\\\"answer\\\":true}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"response_id\":\"resp_non_stream_json\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"id\":\"resp_non_stream_json\",\"created\":3,\"model\":\"gpt-5.3-codex\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, _) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let mapped = convert_responses_body_to_chat_completions(body.as_slice())
        .expect("convert chat completion body");
    let value: serde_json::Value =
        serde_json::from_slice(&mapped).expect("parse chat completion body");
    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .expect("chat message content");

    assert_eq!(content, r#"{"answer":true}"#);
    let parsed: serde_json::Value =
        serde_json::from_str(content).expect("chat content is a single json document");
    assert_eq!(parsed["answer"], true);
}

#[test]
fn sse_synthesized_body_usage_merge_does_not_duplicate_output_text() {
    let sse = concat!(
        "event: response.output_text.delta\n",
        "data: {\"response_id\":\"resp_usage_no_dup\",\"delta\":\"{\\\"answer\\\":true}\"}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"id\":\"resp_usage_no_dup\",\"created\":3,\"model\":\"gpt-5.3-codex\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, mut usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");

    merge_usage_from_body_without_output_text(&mut usage, body.as_slice());

    assert_eq!(usage.output_text.as_deref(), Some(r#"{"answer":true}"#));
    assert_eq!(usage.input_tokens, Some(3));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(5));
}

#[test]
fn non_stream_images_response_builds_b64_json_payload() {
    let body = json!({
        "id": "resp_images_1",
        "created_at": 1775900000,
        "model": "gpt-5.4",
        "output": [{
            "type": "image_generation_call",
            "id": "ig_1",
            "status": "completed",
            "revised_prompt": "一只极简猫",
            "output_format": "png",
            "size": "1024x1024",
            "quality": "high",
            "background": "transparent",
            "result": "aGVsbG8="
        }],
        "tool_usage": {
            "image_gen": { "input_tokens": 1, "output_tokens": 2, "total_tokens": 3 }
        }
    });

    let mapped = convert_responses_body_to_images(
        serde_json::to_vec(&body).expect("body").as_slice(),
        ImagesResponseFormat::B64Json,
    )
    .expect("convert images body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse images body");

    assert_eq!(value["created"], 1775900000);
    assert_eq!(value["data"][0]["b64_json"], "aGVsbG8=");
    assert_eq!(value["data"][0]["revised_prompt"], "一只极简猫");
    assert_eq!(value["size"], "1024x1024");
    assert_eq!(value["quality"], "high");
    assert_eq!(value["background"], "transparent");
    assert_eq!(value["output_format"], "png");
    assert_eq!(value["usage"]["total_tokens"], 3);
}

#[test]
fn non_stream_images_response_builds_url_payload() {
    let body = json!({
        "created": 1775900001,
        "output": [{
            "type": "image_generation_call",
            "output_format": "webp",
            "result": "aGVsbG8="
        }]
    });

    let mapped = convert_responses_body_to_images(
        serde_json::to_vec(&body).expect("body").as_slice(),
        ImagesResponseFormat::Url,
    )
    .expect("convert images body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse images body");

    assert_eq!(value["data"][0]["url"], "data:image/webp;base64,aGVsbG8=");
}

#[test]
fn non_stream_gemini_response_decodes_double_encoded_function_call_arguments() {
    let body = json!({
        "id": "resp_non_stream_double_encoded_tool",
        "model": "gpt-5.4",
        "output": [{
            "type": "function_call",
            "call_id": "call_non_stream_double_encoded_write",
            "name": "write_file",
            "arguments": "\"{\\\"file_path\\\":\\\"C:/Users/test/Desktop/test/gemini/plan.md\\\",\\\"content\\\":\\\"plan\\\"}\""
        }],
        "usage": { "input_tokens": 1, "output_tokens": 1, "total_tokens": 2 }
    });

    let mapped = convert_responses_body_to_gemini_generate_content(
        serde_json::to_vec(&body).expect("body").as_slice(),
        false,
        None,
    )
    .expect("convert gemini body");
    let value: serde_json::Value = serde_json::from_slice(&mapped).expect("parse mapped body");

    assert_eq!(
        value["candidates"][0]["content"]["parts"][0]["functionCall"]["args"]["file_path"],
        "C:/Users/test/Desktop/test/gemini/plan.md"
    );
    assert_eq!(value["functionCalls"][0]["args"]["content"], "plan");
}

#[test]
fn gemini_cli_wrap_response_envelope_is_enabled_for_gemini_adapter_only() {
    assert!(gemini_cli_wrap_response_envelope(
        ResponseAdapter::GeminiCliJson
    ));
    assert!(gemini_cli_wrap_response_envelope(
        ResponseAdapter::GeminiCliSse
    ));
    assert!(!gemini_cli_wrap_response_envelope(
        ResponseAdapter::AnthropicMessagesFromResponses
    ));
    assert!(!gemini_cli_wrap_response_envelope(
        ResponseAdapter::ChatCompletionsFromResponses
    ));
    assert!(!gemini_cli_wrap_response_envelope(
        ResponseAdapter::Passthrough
    ));
}
