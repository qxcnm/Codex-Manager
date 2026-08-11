use super::openai::{
    apply_openai_stream_meta_defaults, extract_openai_completed_output_text, OpenAIStreamMeta,
};
use super::{
    collect_non_stream_json_from_sse_bytes, inspect_sse_frame, parse_sse_frame_json,
    parse_usage_from_json, parse_usage_from_sse_frame, AnthropicSseReader,
    ChatCompletionsFromResponsesSseReader, GeminiSseReader, ImagesFromResponsesSseReader,
    ImagesResponseFormat, OpenAIResponsesPassthroughSseReader, PassthroughSseCollector,
    PassthroughSseProtocol, PassthroughSseUsageReader, SseKeepAliveFrame,
};
use crate::gateway::upstream::{GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse};
use crate::gateway::GeminiStreamOutputMode;
use bytes::Bytes;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn unwrap_gemini_cli_response(event: &serde_json::Value) -> &serde_json::Value {
    event.get("response").unwrap_or(event)
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    /// 函数 `set`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - key: 参数 key
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    /// 函数 `drop`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 无
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

/// 函数 `open_mock_http_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - content_type: 参数 content_type
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn open_mock_http_response(content_type: &str, body: &str) -> reqwest::blocking::Response {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let content_type = content_type.to_string();
    let body = body.to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept mock client");
        let mut request_buf = [0_u8; 2048];
        let _ = stream.read(&mut request_buf);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.as_bytes().len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
        stream.flush().expect("flush mock response");
    });
    let response = reqwest::blocking::get(format!("http://{addr}")).expect("request mock upstream");
    server.join().expect("join mock upstream server");
    response
}

/// 函数 `open_streaming_mock_http_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - content_type: 参数 content_type
/// - chunks: 参数 chunks
///
/// # 返回
/// 返回函数执行结果
fn open_streaming_mock_http_response(
    content_type: &str,
    chunks: &[(&str, u64)],
) -> (reqwest::blocking::Response, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind streaming mock upstream");
    let addr = listener.local_addr().expect("streaming mock upstream addr");
    let content_type = content_type.to_string();
    let chunks = chunks
        .iter()
        .map(|(chunk, delay_ms)| ((*chunk).to_string(), *delay_ms))
        .collect::<Vec<_>>();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept mock client");
        let mut request_buf = [0_u8; 2048];
        let _ = stream.read(&mut request_buf);
        let response_header =
            format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(response_header.as_bytes())
            .expect("write streaming response headers");
        stream.flush().expect("flush streaming response headers");
        for (chunk, delay_ms) in chunks {
            if delay_ms > 0 {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            stream
                .write_all(chunk.as_bytes())
                .expect("write streaming response chunk");
            stream.flush().expect("flush streaming response chunk");
        }
    });
    let response = reqwest::blocking::get(format!("http://{addr}")).expect("request mock upstream");
    (response, server)
}

fn chat_sse_content_fragments(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| data.trim() != "[DONE]")
        .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
        .filter_map(|value| {
            value["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

fn chat_sse_reasoning_fragments(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| data.trim() != "[DONE]")
        .filter_map(|data| serde_json::from_str::<serde_json::Value>(data).ok())
        .filter_map(|value| {
            value["choices"][0]["delta"]["reasoning_content"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

/// 函数 `parse_usage_from_json_reads_cached_and_reasoning_details`
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
fn parse_usage_from_json_reads_cached_and_reasoning_details() {
    let payload = json!({
        "usage": {
            "input_tokens": 321,
            "input_tokens_details": { "cached_tokens": 280 },
            "output_tokens": 55,
            "total_tokens": 376,
            "output_tokens_details": { "reasoning_tokens": 21 }
        }
    });
    let usage = parse_usage_from_json(&payload);
    assert_eq!(usage.input_tokens, Some(321));
    assert_eq!(usage.cached_input_tokens, Some(280));
    assert_eq!(usage.output_tokens, Some(55));
    assert_eq!(usage.total_tokens, Some(376));
    assert_eq!(usage.reasoning_output_tokens, Some(21));
}

/// 函数 `parse_usage_from_json_reads_response_usage_compat_fields`
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
fn parse_usage_from_json_reads_response_usage_compat_fields() {
    let payload = json!({
        "type": "response.completed",
        "response": {
            "usage": {
                "prompt_tokens": 100,
                "prompt_tokens_details": { "cached_tokens": 75 },
                "completion_tokens": 20,
                "total_tokens": 120,
                "completion_tokens_details": { "reasoning_tokens": 9 }
            }
        }
    });
    let usage = parse_usage_from_json(&payload);
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cached_input_tokens, Some(75));
    assert_eq!(usage.output_tokens, Some(20));
    assert_eq!(usage.total_tokens, Some(120));
    assert_eq!(usage.reasoning_output_tokens, Some(9));
}

/// 函数 `parse_usage_from_json_reads_anthropic_compat_fields`
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
fn parse_usage_from_json_reads_anthropic_compat_fields() {
    let payload = json!({
        "usage": {
            "input_tokens": 42,
            "cache_read_input_tokens": 17,
            "output_tokens": 9,
            "total_tokens": 51,
            "reasoning_output_tokens": 4
        }
    });
    let usage = parse_usage_from_json(&payload);
    assert_eq!(usage.input_tokens, Some(42));
    assert_eq!(usage.cached_input_tokens, Some(17));
    assert_eq!(usage.output_tokens, Some(9));
    assert_eq!(usage.total_tokens, Some(51));
    assert_eq!(usage.reasoning_output_tokens, Some(4));
}

/// 函数 `parse_usage_from_json_merges_response_usage_over_top_level_usage`
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
fn parse_usage_from_json_merges_response_usage_over_top_level_usage() {
    let payload = json!({
        "usage": {
            "input_tokens": 11,
            "output_tokens": 7,
            "total_tokens": 18
        },
        "response": {
            "usage": {
                "prompt_tokens": 13,
                "prompt_tokens_details": { "cached_tokens": 5 },
                "completion_tokens": 9,
                "total_tokens": 22
            }
        }
    });
    let usage = parse_usage_from_json(&payload);
    assert_eq!(usage.input_tokens, Some(13));
    assert_eq!(usage.cached_input_tokens, Some(5));
    assert_eq!(usage.output_tokens, Some(9));
    assert_eq!(usage.total_tokens, Some(22));
    assert_eq!(usage.reasoning_output_tokens, None);
}

#[test]
fn parse_usage_from_json_prefers_output_text_over_duplicate_output() {
    let payload = json!({
        "response": {
            "output_text": "{\"answer\":true}",
            "output": [{
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": "{\"answer\":true}" }]
            }],
            "usage": {
                "input_tokens": 3,
                "output_tokens": 2,
                "total_tokens": 5
            }
        }
    });

    let usage = parse_usage_from_json(&payload);
    assert_eq!(usage.output_text.as_deref(), Some(r#"{"answer":true}"#));
    assert_eq!(usage.total_tokens, Some(5));
}

/// 函数 `parse_usage_from_sse_frame_reads_response_completed_usage`
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
fn parse_usage_from_sse_frame_reads_response_completed_usage() {
    let frame_lines = vec![
        "event: message\n".to_string(),
        r#"data: {"type":"response.completed","response":{"usage":{"input_tokens":88,"input_tokens_details":{"cached_tokens":61},"output_tokens":17,"total_tokens":105,"output_tokens_details":{"reasoning_tokens":6}}}}"#
            .to_string(),
        "\n".to_string(),
    ];
    let usage = parse_usage_from_sse_frame(&frame_lines).expect("extract usage from sse frame");
    assert_eq!(usage.input_tokens, Some(88));
    assert_eq!(usage.cached_input_tokens, Some(61));
    assert_eq!(usage.output_tokens, Some(17));
    assert_eq!(usage.total_tokens, Some(105));
    assert_eq!(usage.reasoning_output_tokens, Some(6));
}

/// 函数 `parse_usage_from_sse_frame_reads_top_level_and_response_usage`
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
fn parse_usage_from_sse_frame_reads_top_level_and_response_usage() {
    let frame_lines = vec![
        "event: message\n".to_string(),
        r#"data: {"type":"response.completed","usage":{"input_tokens":22,"input_tokens_details":{"cached_tokens":10},"output_tokens":11,"total_tokens":33,"output_tokens_details":{"reasoning_tokens":3}},"response":{"usage":{"prompt_tokens":26,"prompt_tokens_details":{"cached_tokens":12},"completion_tokens":15,"total_tokens":41,"completion_tokens_details":{"reasoning_tokens":4}}}}"#
            .to_string(),
        "\n".to_string(),
    ];
    let usage = parse_usage_from_sse_frame(&frame_lines).expect("extract usage from sse frame");
    assert_eq!(usage.input_tokens, Some(26));
    assert_eq!(usage.cached_input_tokens, Some(12));
    assert_eq!(usage.output_tokens, Some(15));
    assert_eq!(usage.total_tokens, Some(41));
    assert_eq!(usage.reasoning_output_tokens, Some(4));
}

/// 函数 `parse_usage_from_sse_frame_caps_output_text`
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
fn parse_usage_from_sse_frame_caps_output_text() {
    let limit = super::output_text_limit_bytes();
    if limit == 0 || limit <= super::OUTPUT_TEXT_TRUNCATED_MARKER.len() {
        return;
    }

    let long = "a".repeat(limit.saturating_mul(3));
    let payload = json!({
        "choices": [
            {"delta": {"content": long}}
        ]
    });
    let frame_lines = vec![
        "event: message\n".to_string(),
        format!("data: {}", payload.to_string()),
        "\n".to_string(),
    ];
    let usage = parse_usage_from_sse_frame(&frame_lines).expect("extract usage from sse frame");
    let text = usage.output_text.unwrap_or_default();
    assert!(
        text.len() <= limit,
        "output_text exceeded limit: {} > {limit}",
        text.len()
    );
    assert!(text.ends_with(super::OUTPUT_TEXT_TRUNCATED_MARKER));
}

/// 函数 `inspect_sse_frame_recognizes_done_marker`
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
fn inspect_sse_frame_recognizes_done_marker() {
    let frame_lines = vec![
        "event: message\n".to_string(),
        "data: [DONE]\n".to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame(&frame_lines);
    assert!(inspection.terminal.is_some());
}

/// 函数 `anthropic_sse_reader_final_usage_contains_input_cache_and_output_tokens`
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
fn anthropic_sse_reader_final_usage_contains_input_cache_and_output_tokens() {
    let (response, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_usage_1\",\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":31,\"input_tokens_details\":{\"cached_tokens\":7},\"output_tokens\":9,\"total_tokens\":40,\"output_tokens_details\":{\"reasoning_tokens\":2}},\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}]}}\n\n\
             data: [DONE]\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(super::UpstreamResponseUsage::default()));
    let mut reader = super::AnthropicSseReader::new(
        response,
        usage_collector,
        None,
        None,
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader
        .read_to_string(&mut out)
        .expect("read anthropic sse reader");
    server.join().expect("join streaming mock upstream");

    assert!(out.contains("\"type\":\"message_delta\""));
    assert!(out.contains("\"input_tokens\":31"));
    assert!(out.contains("\"cache_read_input_tokens\":7"));
    assert!(out.contains("\"output_tokens\":9"));
    assert!(out.contains("\"total_tokens\":40"));
    assert!(out.contains("\"reasoning_output_tokens\":2"));
}

#[test]
fn anthropic_sse_reader_does_not_replay_completed_snapshot_after_tool_call() {
    let (response, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "event: response.output_text.delta\n\
             data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello before tool\"}\n\n\
             event: response.output_item.done\n\
             data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_search_1\",\"name\":\"search\",\"arguments\":\"{\\\"query\\\":\\\"hello\\\"}\"}}\n\n\
             event: response.completed\n\
             data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_anthropic_dup\",\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5},\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello before tool\"}]},{\"type\":\"function_call\",\"call_id\":\"call_search_1\",\"name\":\"search\",\"arguments\":\"{\\\"query\\\":\\\"hello\\\"}\"}]}}\n\n\
             data: [DONE]\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(super::UpstreamResponseUsage::default()));
    let mut reader = AnthropicSseReader::new(
        response,
        Arc::clone(&usage_collector),
        None,
        None,
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader
        .read_to_string(&mut out)
        .expect("read anthropic sse reader");
    server.join().expect("join streaming mock upstream");

    assert_eq!(out.matches("\"text\":\"hello before tool\"").count(), 1);
    assert!(out.contains("\"type\":\"tool_use\""));
    assert!(out.contains("\"stop_reason\":\"tool_use\""));
    let usage = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert_eq!(usage.output_text.as_deref(), Some("hello before tool"));
    assert_eq!(usage.input_tokens, Some(3));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(5));
}

#[test]
fn anthropic_sse_reader_uses_request_model_when_upstream_stream_omits_model() {
    let (response, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "data: {\"type\":\"response.failed\",\"response\":{\"id\":\"resp_missing_model_1\"}}\n\n\
             data: [DONE]\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(super::UpstreamResponseUsage::default()));
    let mut reader = super::AnthropicSseReader::new(
        response,
        usage_collector,
        Some("gpt-5.4"),
        None,
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader
        .read_to_string(&mut out)
        .expect("read anthropic sse reader");
    server.join().expect("join streaming mock upstream");

    assert!(out.contains("\"id\":\"resp_missing_model_1\""));
    assert!(out.contains("\"model\":\"gpt-5.4\""));
    assert!(!out.contains("\"model\":\"gpt-5.3-codex\""));
}

/// 函数 `inspect_sse_frame_recognizes_response_failed_as_terminal_error`
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
fn inspect_sse_frame_recognizes_response_failed_as_terminal_error() {
    let frame_lines = vec![
        "event: response.failed\n".to_string(),
        r#"data: {"type":"response.failed","error":{"message":"Internal server error"}}"#
            .to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame(&frame_lines);
    let err = inspection
        .terminal
        .as_ref()
        .and_then(|t| match t {
            super::SseTerminal::Ok => None,
            super::SseTerminal::Err(msg) => Some(msg.as_str()),
        })
        .unwrap_or("");
    assert!(err.contains("Internal server error"));
}

/// 函数 `inspect_sse_frame_recognizes_response_done_as_terminal`
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
fn inspect_sse_frame_recognizes_response_done_as_terminal() {
    let frame_lines = vec![
        "event: response.done\n".to_string(),
        r#"data: {"type":"response.done","response":{"id":"resp_done_1"}}"#.to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame(&frame_lines);
    assert!(inspection.terminal.is_some());
}

/// 函数 `inspect_sse_frame_recognizes_chat_completion_finish_reason_as_terminal`
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
fn inspect_sse_frame_recognizes_chat_completion_finish_reason_as_terminal() {
    let frame_lines = vec![
        "event: message\n".to_string(),
        r#"data: {"id":"chatcmpl_1","object":"chat.completion.chunk","model":"gpt-5.3-codex","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":"stop"}]}"#
            .to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame(&frame_lines);
    assert!(inspection.terminal.is_some());
}

/// 函数 `inspect_sse_frame_recognizes_nested_response_error_message`
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
fn inspect_sse_frame_recognizes_nested_response_error_message() {
    let frame_lines = vec![
        "event: response.failed\n".to_string(),
        r#"data: {"type":"response.failed","response":{"status":"failed","error":{"message":"Model not found","type":"invalid_request_error","code":"model_not_found"}}}"#
            .to_string(),
        "\n".to_string(),
    ];
    let inspection = inspect_sse_frame(&frame_lines);
    let err = inspection
        .terminal
        .as_ref()
        .and_then(|t| match t {
            super::SseTerminal::Ok => None,
            super::SseTerminal::Err(msg) => Some(msg.as_str()),
        })
        .unwrap_or("");
    assert!(err.contains("Model not found"), "unexpected err: {err}");
    assert!(err.contains("model_not_found"), "unexpected err: {err}");
}

/// 函数 `collect_non_stream_json_from_sse_bytes_extracts_response_completed`
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
fn collect_non_stream_json_from_sse_bytes_extracts_response_completed() {
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.3-codex\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\"}]}],\"usage\":{\"input_tokens\":7,\"output_tokens\":3,\"total_tokens\":10}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    assert_eq!(value["id"], "resp_1");
    assert_eq!(value["output"][0]["role"], "assistant");
    assert_eq!(usage.input_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(3));
    assert_eq!(usage.total_tokens, Some(10));
}

/// 函数 `collect_non_stream_json_from_sse_bytes_extracts_response_done`
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
fn collect_non_stream_json_from_sse_bytes_extracts_response_done() {
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hello\"}\n\n",
        "data: {\"type\":\"response.done\",\"response\":{\"id\":\"resp_done_1\",\"model\":\"gpt-5.3-codex\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\"}]}],\"usage\":{\"input_tokens\":7,\"output_tokens\":3,\"total_tokens\":10}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    assert_eq!(value["id"], "resp_done_1");
    assert_eq!(value["output"][0]["role"], "assistant");
    assert_eq!(usage.input_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(3));
    assert_eq!(usage.total_tokens, Some(10));
}

/// 函数 `collect_non_stream_json_from_sse_bytes_synthesizes_chat_completion_chunks`
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
fn collect_non_stream_json_from_sse_bytes_synthesizes_chat_completion_chunks() {
    let sse = concat!(
        "data: {\"id\":\"chatcmpl_1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-5.3-codex\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hel\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"chatcmpl_1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"gpt-5.3-codex\",\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3,\"total_tokens\":10},\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"},\"finish_reason\":\"stop\"}]}\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized chat completion json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    assert_eq!(value["id"], "chatcmpl_1");
    assert_eq!(value["object"], "chat.completion");
    assert_eq!(value["choices"][0]["message"]["role"], "assistant");
    assert_eq!(value["choices"][0]["message"]["content"], "hello");
    assert_eq!(value["choices"][0]["finish_reason"], "stop");
    assert_eq!(usage.input_tokens, Some(7));
    assert_eq!(usage.output_tokens, Some(3));
    assert_eq!(usage.total_tokens, Some(10));
}

#[test]
fn chat_completions_reader_converts_responses_sse_to_chat_sse() {
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"好\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_chat_1\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":2,\"output_tokens\":2,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");
    assert!(out.contains("\"object\":\"chat.completion.chunk\""));
    assert!(out.contains("\"role\":\"assistant\""));
    assert!(out.contains("\"content\":\"你\""));
    assert!(out.contains("\"content\":\"好\""));
    assert!(out.contains("\"finish_reason\":\"stop\""));
    assert!(out.contains("\"prompt_tokens\":2"));
    assert!(out.contains("data: [DONE]"));
    let collector = collector.lock().expect("collector lock");
    assert!(collector.saw_terminal);
    assert_eq!(collector.usage.input_tokens, Some(2));
    assert_eq!(collector.usage.output_tokens, Some(2));
    assert_eq!(collector.usage.total_tokens, Some(4));
}

#[test]
fn chat_completions_reader_converts_reasoning_output_item_to_reasoning_delta() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_reason_1\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"先读配置\"}],\"encrypted_content\":\"sig_reason_1\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_reason_1\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert_eq!(chat_sse_reasoning_fragments(&out), vec!["先读配置"]);
    assert!(out.contains("\"reasoning\":\"先读配置\""));
    assert!(!out.contains("\"content\":\"先读配置\""));
    assert!(out.contains("\"finish_reason\":\"stop\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_converts_reasoning_summary_delta_to_reasoning_delta() {
    let sse = concat!(
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"response_id\":\"resp_reason_delta_1\",\"delta\":\"先读配置\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_reason_delta_1\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert_eq!(chat_sse_reasoning_fragments(&out), vec!["先读配置"]);
    assert!(out.contains("\"reasoning\":\"先读配置\""));
    assert!(out.contains("\"finish_reason\":\"stop\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_uses_output_text_done_text_when_delta_missing() {
    let json_text = r#"{"ok":true}"#;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.done\",\"text\":\"{\\\"ok\\\":true}\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_chat_done_only\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":2,\"output_tokens\":2,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert_eq!(chat_sse_content_fragments(&out), vec![json_text]);
    assert!(out.contains("\"finish_reason\":\"stop\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_uses_output_item_done_text_when_delta_missing() {
    let json_text = r#"{"ok":true}"#;
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_chat_item_only\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"ok\\\":true}\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_chat_item_only\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":2,\"output_tokens\":2,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert_eq!(chat_sse_content_fragments(&out), vec![json_text]);
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_does_not_duplicate_output_text_done_snapshot_after_delta() {
    let json_text = r#"{"ok":true}"#;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"{\\\"ok\\\":true}\"}\n\n",
        "data: {\"type\":\"response.output_text.done\",\"text\":\"{\\\"ok\\\":true}\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_chat_no_dup\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":2,\"output_tokens\":2,\"total_tokens\":4}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert_eq!(chat_sse_content_fragments(&out), vec![json_text]);
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_converts_image_generation_call_to_delta_images() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_img_1\",\"output_index\":0,\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_1\",\"status\":\"completed\",\"output_format\":\"png\",\"result\":\"aGVsbG8=\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_img_1\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert!(out.contains("\"images\""));
    assert!(out.contains("data:image/png;base64,aGVsbG8="));
    assert!(out.contains("\"finish_reason\":\"stop\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_dedupes_partial_image_and_done_image() {
    let sse = concat!(
        "data: {\"type\":\"response.image_generation_call.partial_image\",\"item_id\":\"ig_1\",\"output_format\":\"png\",\"partial_image_b64\":\"aGVsbG8=\",\"partial_image_index\":0}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_img_1\",\"output_index\":0,\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_1\",\"status\":\"completed\",\"output_format\":\"png\",\"result\":\"aGVsbG8=\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_img_1\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert_eq!(out.matches("data:image/png;base64,aGVsbG8=").count(), 1);
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_converts_function_call_to_tool_calls() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"get_answer\",\"arguments\":\"\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"delta\":\"{\\\"question\\\":\\\"2+2\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"get_answer\",\"arguments\":\"{\\\"question\\\":\\\"2+2\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_tool_1\",\"model\":\"gpt-5.4\",\"created\":1775900000,\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert!(out.contains("\"role\":\"assistant\""));
    assert!(out.contains("\"tool_calls\""));
    assert!(out.contains("\"id\":\"call_1\""));
    assert!(out.contains("\"name\":\"get_answer\""));
    assert!(out.contains("\"arguments\":\"{\\\"question\\\":\\\"2+2\\\"}\""));
    assert!(out.contains("\"finish_reason\":\"tool_calls\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn chat_completions_reader_uses_done_arguments_when_delta_is_missing() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"get_answer\",\"arguments\":\"\"}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"get_answer\",\"arguments\":\"{\\\"question\\\":\\\"2+2\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_tool_1\",\"model\":\"gpt-5.4\",\"created\":1775900000}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ChatCompletionsFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read chat sse");

    assert!(out.contains("\"id\":\"call_1\""));
    assert!(out.contains("\"arguments\":\"{\\\"question\\\":\\\"2+2\\\"}\""));
    assert!(out.contains("\"finish_reason\":\"tool_calls\""));
}

#[test]
fn images_reader_streams_partial_and_completed_events() {
    let sse = concat!(
        "data: {\"type\":\"response.image_generation_call.partial_image\",\"item_id\":\"ig_1\",\"output_format\":\"png\",\"partial_image_b64\":\"cGFydGlhbA==\",\"partial_image_index\":0}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_img_1\",\"created\":1775900000,\"model\":\"gpt-5.4\",\"output\":[{\"type\":\"image_generation_call\",\"id\":\"ig_1\",\"status\":\"completed\",\"output_format\":\"png\",\"result\":\"ZmluYWw=\"}],\"tool_usage\":{\"image_gen\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}}\n\n",
        "data: [DONE]\n\n"
    );
    let response = open_mock_http_response("text/event-stream", sse);
    let collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = ImagesFromResponsesSseReader::new(
        response,
        Arc::clone(&collector),
        std::time::Instant::now(),
        ImagesResponseFormat::B64Json,
    );
    let mut out = String::new();
    reader.read_to_string(&mut out).expect("read images sse");

    assert!(out.contains("event: image_generation.partial_image"));
    assert!(out.contains("\"partial_image_index\":0"));
    assert!(out.contains("\"b64_json\":\"cGFydGlhbA==\""));
    assert!(out.contains("event: image_generation.completed"));
    assert!(out.contains("\"b64_json\":\"ZmluYWw=\""));
    assert!(out.contains("\"total_tokens\":5"));
}

/// 函数 `extract_openai_completed_output_text_reads_completed_output_message_text`
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
fn extract_openai_completed_output_text_reads_completed_output_message_text() {
    let payload = json!({
        "type": "response.completed",
        "response": {
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "hello from completed"
                }]
            }]
        }
    });
    let text = extract_openai_completed_output_text(&payload).unwrap_or_default();
    assert_eq!(text, "hello from completed");
}

/// 函数 `apply_openai_stream_meta_defaults_fills_missing_chunk_meta`
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
fn apply_openai_stream_meta_defaults_fills_missing_chunk_meta() {
    let mut mapped = json!({
        "id": "",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "",
        "choices": [{
            "index": 0,
            "delta": {"content": "hello"},
            "finish_reason": null
        }]
    });
    let meta = OpenAIStreamMeta {
        response_id: Some("resp_meta_1".to_string()),
        model: Some("gpt-5.3-codex".to_string()),
        created: Some(1700000123),
    };
    apply_openai_stream_meta_defaults(&mut mapped, &meta);
    assert_eq!(mapped["id"], "resp_meta_1");
    assert_eq!(mapped["model"], "gpt-5.3-codex");
    assert_eq!(mapped["created"], 1700000123);
}

/// 函数 `collect_non_stream_json_from_sse_bytes_backfills_response_output_from_deltas`
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
fn collect_non_stream_json_from_sse_bytes_backfills_response_output_from_deltas() {
    let sse = concat!(
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_delta_1\",\"created\":2,\"model\":\"gpt-5.3-codex\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_delta_1\",\"delta\":\"hello \"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_delta_1\",\"delta\":\"world\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_delta_1\",\"created\":2,\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":9,\"output_tokens\":2,\"total_tokens\":11}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    assert_eq!(value["id"], "resp_delta_1");
    assert_eq!(value["object"], "response");
    assert_eq!(
        value["output"][0]["content"][0]["text"],
        serde_json::Value::String("hello world".to_string())
    );
    assert_eq!(
        value["output_text"],
        serde_json::Value::String("hello world".to_string())
    );
    assert_eq!(usage.input_tokens, Some(9));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(11));
}

/// 函数 `collect_non_stream_json_from_sse_bytes_backfills_reasoning_output_items`
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
fn collect_non_stream_json_from_sse_bytes_backfills_reasoning_output_items() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_reason_1\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"先读配置\"}],\"encrypted_content\":\"sig_reason_1\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_reason_1\",\"created\":5,\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    assert_eq!(value["id"], "resp_reason_1");
    assert_eq!(value["output"][0]["type"], "reasoning");
    assert_eq!(value["output"][0]["summary"][0]["text"], "先读配置");
    assert_eq!(value["output"][0]["encrypted_content"], "sig_reason_1");
    assert!(value.get("output_text").is_none());
    assert_eq!(usage.input_tokens, Some(4));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(6));
}

#[test]
fn collect_non_stream_json_from_sse_bytes_backfills_image_generation_output_items() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_image_1\",\"output_index\":0,\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_1\",\"status\":\"completed\",\"revised_prompt\":\"一只猫\",\"output_format\":\"png\",\"result\":\"aGVsbG8=\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_image_1\",\"created\":5,\"model\":\"gpt-5.4\",\"usage\":{\"input_tokens\":4,\"output_tokens\":1,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");

    assert_eq!(value["id"], "resp_image_1");
    assert_eq!(value["output"][0]["type"], "image_generation_call");
    assert_eq!(value["output"][0]["id"], "ig_1");
    assert_eq!(value["output"][0]["revised_prompt"], "一只猫");
    assert_eq!(value["output"][0]["result"], "aGVsbG8=");
    assert_eq!(usage.input_tokens, Some(4));
    assert_eq!(usage.output_tokens, Some(1));
    assert_eq!(usage.total_tokens, Some(5));
}

/// 函数 `collect_non_stream_json_from_sse_bytes_backfills_function_call_output_items`
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
fn collect_non_stream_json_from_sse_bytes_backfills_function_call_output_items() {
    let sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"response_id\":\"resp_tool_agg_1\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_read_1\",\"name\":\"read_file\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"response_id\":\"resp_tool_agg_1\",\"output_index\":0,\"delta\":\"{\\\"path\\\":\\\"REA\"}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"response_id\":\"resp_tool_agg_1\",\"output_index\":0,\"delta\":\"DME.md\\\"}\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_tool_agg_1\",\"created\":6,\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":6,\"output_tokens\":1,\"total_tokens\":7}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    assert_eq!(value["id"], "resp_tool_agg_1");
    assert_eq!(value["output"][0]["type"], "function_call");
    assert_eq!(value["output"][0]["call_id"], "call_read_1");
    assert_eq!(value["output"][0]["name"], "read_file");
    assert_eq!(value["output"][0]["arguments"], "{\"path\":\"README.md\"}");
    assert!(value.get("output_text").is_none());
    assert_eq!(usage.input_tokens, Some(6));
    assert_eq!(usage.output_tokens, Some(1));
    assert_eq!(usage.total_tokens, Some(7));
}

/// 函数 `parse_sse_frame_json_infers_type_from_event_name`
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
fn parse_sse_frame_json_infers_type_from_event_name() {
    let frame_lines = vec![
        "event: response.output_text.delta\n".to_string(),
        r#"data: {"response_id":"resp_evt_1","delta":"hello"}"#.to_string(),
        "\n".to_string(),
    ];
    let value = parse_sse_frame_json(&frame_lines).expect("parse sse frame");
    assert_eq!(value["type"], "response.output_text.delta");
    assert_eq!(value["delta"], "hello");
}

/// 函数 `collect_non_stream_json_from_sse_bytes_supports_event_only_type_frames`
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
fn collect_non_stream_json_from_sse_bytes_supports_event_only_type_frames() {
    let sse = concat!(
        "event: response.output_text.delta\n",
        "data: {\"response_id\":\"resp_evt_1\",\"delta\":\"hello \"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"response_id\":\"resp_evt_1\",\"delta\":\"world\"}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"id\":\"resp_evt_1\",\"created\":3,\"model\":\"gpt-5.3-codex\",\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    assert_eq!(value["id"], "resp_evt_1");
    assert_eq!(
        value["output_text"],
        serde_json::Value::String("hello world".to_string())
    );
    assert_eq!(usage.input_tokens, Some(3));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(5));
}

#[test]
fn sse_responses_json_text_not_duplicated_across_delta_item_completed() {
    let json_text = r#"{"answer":true}"#;
    let sse = concat!(
        "event: response.output_text.delta\n",
        "data: {\"response_id\":\"resp_json_single\",\"delta\":\"{\\\"answer\\\":true}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"response_id\":\"resp_json_single\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"id\":\"resp_json_single\",\"created\":3,\"model\":\"gpt-5.3-codex\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    let content = value["output"][0]["content"][0]["text"]
        .as_str()
        .expect("assistant text");

    assert_eq!(content, json_text);
    serde_json::from_str::<serde_json::Value>(content).expect("parse json mode content");
    assert!(value.get("output_text").is_none());
    assert_eq!(usage.output_text.as_deref(), Some(json_text));
    assert_eq!(usage.input_tokens, Some(3));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(5));
}

#[test]
fn sse_completed_response_without_usable_output_falls_back_to_delta_text() {
    let sse = concat!(
        "event: response.output_text.delta\n",
        "data: {\"response_id\":\"resp_json_fallback\",\"delta\":\"{\\\"ok\\\":true}\"}\n\n",
        "event: response.completed\n",
        "data: {\"response\":{\"id\":\"resp_json_fallback\",\"created\":4,\"model\":\"gpt-5.3-codex\",\"output\":[],\"usage\":{\"input_tokens\":4,\"output_tokens\":2,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n"
    );
    let (body, usage) = collect_non_stream_json_from_sse_bytes(sse.as_bytes());
    let body = body.expect("synthesized response json");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("parse synthesized body");
    let content = value["output"][0]["content"][0]["text"]
        .as_str()
        .expect("assistant text");

    assert_eq!(content, r#"{"ok":true}"#);
    assert_eq!(value["output_text"], r#"{"ok":true}"#);
    assert_eq!(usage.output_text.as_deref(), Some(r#"{"ok":true}"#));
    assert_eq!(usage.input_tokens, Some(4));
    assert_eq!(usage.output_tokens, Some(2));
    assert_eq!(usage.total_tokens, Some(6));
}

/// 函数 `parse_sse_frame_json_supports_json_lines_without_data_prefix`
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
fn parse_sse_frame_json_supports_json_lines_without_data_prefix() {
    let frame_lines = vec![
        r#"{"type":"response.output_text.delta","response_id":"resp_jsonl_1","delta":"hi"}"#
            .to_string(),
        "\n".to_string(),
    ];
    let value = parse_sse_frame_json(&frame_lines).expect("parse jsonl frame");
    assert_eq!(value["type"], "response.output_text.delta");
    assert_eq!(value["delta"], "hi");
}

#[test]
fn gemini_sse_reader_waits_for_completed_full_arguments_before_emitting_tool_call() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_item.added\",\"response_id\":\"resp_gemini_reader_1\",\"model\":\"gpt-5.4\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_linux_do_1\",\"name\":\"chrome_devtools_new_page\"}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_gemini_reader_1\",\"model\":\"gpt-5.4\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_linux_do_1\",\"call_id\":\"call_linux_do_1\",\"name\":\"chrome_devtools_new_page\",\"arguments\":\"{}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_reader_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_linux_do_1\",\"call_id\":\"call_linux_do_1\",\"name\":\"chrome_devtools_new_page\",\"arguments\":\"{\\\"url\\\":\\\"https://linux.do\\\"}\"}],\"usage\":{\"input_tokens\":4,\"input_tokens_details\":{\"cached_tokens\":2},\"output_tokens\":5,\"total_tokens\":9,\"output_tokens_details\":{\"reasoning_tokens\":1}}}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        false,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini mapped sse");

    let events = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .filter(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .collect::<Vec<_>>();
    let tool_events = events
        .iter()
        .filter(|event| {
            event["candidates"][0]["content"]["parts"]
                .as_array()
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part.get("functionCall").is_some_and(|call| !call.is_null()))
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_events.len(), 1);
    assert_eq!(
        tool_events[0]["candidates"][0]["content"]["parts"][0]["functionCall"]["name"],
        "chrome_devtools_new_page"
    );
    assert_eq!(
        tool_events[0]["candidates"][0]["content"]["parts"][0]["functionCall"]["args"]["url"],
        "https://linux.do"
    );
    assert_eq!(
        tool_events[0]["candidates"][0]["content"]["parts"][0]["functionCall"]["id"],
        "call_linux_do_1"
    );
    assert_eq!(tool_events[0]["functionCalls"][0]["id"], "call_linux_do_1");
    assert_eq!(
        tool_events[0]["functionCalls"][0]["name"],
        "chrome_devtools_new_page"
    );
    assert_eq!(
        tool_events[0]["candidates"][0]["finishReason"], "STOP",
        "Gemini CLI expects the tool-call chunk to also close the model turn"
    );
    assert_eq!(tool_events[0]["modelVersion"], "gpt-5.4");
    assert_eq!(tool_events[0]["responseId"], "resp_gemini_reader_1");
    assert_eq!(
        tool_events[0]["usageMetadata"]["trafficType"],
        "PROVISIONED_THROUGHPUT"
    );

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    let usage = collector.usage;
    assert!(collector.saw_terminal);
    assert_eq!(collector.terminal_error, None);
    assert_eq!(usage.input_tokens, Some(4));
    assert_eq!(usage.cached_input_tokens, Some(2));
    assert_eq!(usage.output_tokens, Some(5));
    assert_eq!(usage.reasoning_output_tokens, Some(1));
    assert_eq!(usage.total_tokens, Some(9));
}

#[test]
fn gemini_sse_reader_prefers_completed_full_arguments_over_partial_stream_arguments() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_item.added\",\"response_id\":\"resp_gemini_reader_partial_args_1\",\"model\":\"gpt-5.4\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_write_plan_1\",\"name\":\"write_file\"}}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_gemini_reader_partial_args_1\",\"model\":\"gpt-5.4\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_write_plan_1\",\"call_id\":\"call_write_plan_partial_1\",\"name\":\"write_file\",\"arguments\":\"{\\\"file_path\\\":\\\"C:/Users/test/Desktop/test/gemini/plan.md\\\"}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_reader_partial_args_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_write_plan_1\",\"call_id\":\"call_write_plan_partial_1\",\"name\":\"write_file\",\"arguments\":\"{\\\"file_path\\\":\\\"C:/Users/test/Desktop/test/gemini/plan.md\\\",\\\"content\\\":\\\"plan body\\\"}\"}],\"usage\":{\"input_tokens\":4,\"output_tokens\":5,\"total_tokens\":9}}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini mapped sse");

    let event = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .filter(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .find(|event| {
            unwrap_gemini_cli_response(event)["candidates"][0]["content"]["parts"]
                .as_array()
                .is_some_and(|parts| {
                    parts
                        .iter()
                        .any(|part| part.get("functionCall").is_some_and(|call| !call.is_null()))
                })
        })
        .expect("tool event");
    let event = unwrap_gemini_cli_response(&event);
    let function_call = &event["candidates"][0]["content"]["parts"][0]["functionCall"];
    assert_eq!(function_call["id"], "call_write_plan_partial_1");
    assert_eq!(
        function_call["args"]["file_path"],
        "C:/Users/test/Desktop/test/gemini/plan.md"
    );
    assert_eq!(function_call["args"]["content"], "plan body");
    assert_eq!(event["functionCalls"][0]["args"]["content"], "plan body");
}

#[test]
fn gemini_sse_reader_does_not_treat_function_call_output_as_final_text() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_reader_tool_output_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[{\"type\":\"function_call_output\",\"call_id\":\"call_edit_1\",\"output\":\"已修改 Desktop\\\\gemini.txt。\"}],\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        false,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini mapped sse");

    let events = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .filter(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .collect::<Vec<_>>();
    let text_events = events
        .iter()
        .filter(|event| {
            unwrap_gemini_cli_response(event)["candidates"][0]["content"]["parts"]
                .as_array()
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.get("text")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|text| !text.is_empty())
                    })
                })
        })
        .collect::<Vec<_>>();
    assert!(text_events.is_empty());
    assert_eq!(
        events.last().expect("finish event present")["candidates"][0]["finishReason"],
        serde_json::Value::String("STOP".to_string())
    );

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    let usage = collector.usage;
    assert!(collector.saw_terminal);
    assert_eq!(collector.terminal_error, None);
    assert_eq!(usage.output_text, None);
    assert_eq!(usage.total_tokens, Some(5));
}

#[test]
fn gemini_sse_reader_completed_message_output_still_emits_final_text() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_reader_message_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"已修改 Desktop\\\\gemini.txt。\"}]}],\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        false,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini mapped sse");

    let events = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .filter(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .collect::<Vec<_>>();
    let text_events = events
        .iter()
        .filter_map(|event| {
            unwrap_gemini_cli_response(event)["candidates"][0]["content"]["parts"]
                .as_array()
                .and_then(|parts| parts.first())
                .and_then(|part| part.get("text"))
                .and_then(serde_json::Value::as_str)
        })
        .collect::<Vec<_>>();
    assert_eq!(text_events, vec!["已修改 Desktop\\gemini.txt。"]);

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    let usage = collector.usage;
    assert!(collector.saw_terminal);
    assert_eq!(collector.terminal_error, None);
    assert_eq!(
        usage.output_text.as_deref(),
        Some("已修改 Desktop\\gemini.txt。")
    );
    assert_eq!(usage.total_tokens, Some(5));
}

#[test]
fn gemini_cli_sse_reader_emits_raw_gemini_chunks() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_gemini_cli_reader_1\",\"model\":\"gpt-5.4\",\"delta\":\"已完成\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_cli_reader_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini cli mapped sse");

    let events = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .filter(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .collect::<Vec<_>>();
    let first_event = unwrap_gemini_cli_response(&events[0]);
    let second_event = unwrap_gemini_cli_response(&events[1]);
    assert_eq!(
        first_event["candidates"][0]["content"]["parts"][0]["text"],
        "已完成"
    );
    assert_eq!(
        second_event["usageMetadata"]["totalTokenCount"],
        serde_json::Value::from(5)
    );

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(collector.saw_terminal);
    assert_eq!(collector.terminal_error, None);
    assert_eq!(collector.usage.output_text.as_deref(), Some("已完成"));
}

#[test]
fn gemini_cli_sse_reader_does_not_emit_comment_keepalive_frames() {
    let _guard = crate::test_env_guard();
    let _keepalive_guard = EnvGuard::set("CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS", "5");
    super::reload_from_env();

    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_cli_keepalive_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"ok\"}]}],\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n\
             data: [DONE]\n\n",
            30,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini cli keepalive stream");
    server.join().expect("join streaming mock upstream");

    assert!(!mapped.contains(": keep-alive"));
    assert!(mapped.contains("\"response\""));
    assert!(mapped.contains("\"candidates\""));
}

#[test]
fn gemini_cli_sse_reader_synthesizes_stop_when_done_follows_text_without_completed() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_gemini_partial_text_1\",\"model\":\"gpt-5.4\",\"delta\":\"我会写入计划。\"}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini partial text stream");

    let events = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .filter(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .collect::<Vec<_>>();
    let first_event = unwrap_gemini_cli_response(&events[0]);
    let last_event = unwrap_gemini_cli_response(events.last().expect("final event"));
    assert_eq!(
        first_event["candidates"][0]["content"]["parts"][0]["text"],
        "我会写入计划。"
    );
    assert_eq!(last_event["candidates"][0]["finishReason"], "STOP");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(collector.saw_terminal);
    assert_eq!(collector.terminal_error, None);
    assert_eq!(
        collector.usage.output_text.as_deref(),
        Some("我会写入计划。")
    );
}

#[test]
fn gemini_cli_sse_reader_synthesizes_tool_call_when_done_follows_function_call_without_completed() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_gemini_partial_tool_1\",\"model\":\"gpt-5.4\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_write_plan_1\",\"name\":\"write_file\",\"arguments\":\"{\\\"file_path\\\":\\\"plans/site.md\\\",\\\"content\\\":\\\"plan\\\"}\"}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini partial tool stream");

    let event = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .find(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .expect("tool event");
    let event = unwrap_gemini_cli_response(&event);
    let part = &event["candidates"][0]["content"]["parts"][0]["functionCall"];
    assert_eq!(part["name"], "write_file");
    assert_eq!(part["id"], "call_write_plan_1");
    assert_eq!(part["args"]["file_path"], "plans/site.md");
    assert_eq!(event["functionCalls"][0]["id"], "call_write_plan_1");
    assert_eq!(event["candidates"][0]["finishReason"], "STOP");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(collector.saw_terminal);
    assert_eq!(collector.terminal_error, None);
}

#[test]
fn gemini_cli_sse_reader_decodes_double_encoded_tool_arguments() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_item.done\",\"response_id\":\"resp_gemini_double_encoded_tool_1\",\"model\":\"gpt-5.4\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_write_plan_2\",\"name\":\"write_file\",\"arguments\":\"\\\"{\\\\\\\"file_path\\\\\\\":\\\\\\\"C:/Users/test/Desktop/test/gemini/plan.md\\\\\\\",\\\\\\\"content\\\\\\\":\\\\\\\"plan\\\\\\\"}\\\"\"}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini double encoded tool stream");

    let event = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .find(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .expect("tool event");
    let event = unwrap_gemini_cli_response(&event);
    let part = &event["candidates"][0]["content"]["parts"][0]["functionCall"];
    assert_eq!(part["id"], "call_write_plan_2");
    assert_eq!(
        part["args"]["file_path"],
        "C:/Users/test/Desktop/test/gemini/plan.md"
    );
    assert_eq!(part["args"]["content"], "plan");
    assert_eq!(
        event["functionCalls"][0]["args"]["file_path"],
        "C:/Users/test/Desktop/test/gemini/plan.md"
    );
}

#[test]
fn gemini_cli_sse_reader_merges_custom_tool_call_input_events() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_item.added\",\"response_id\":\"resp_gemini_custom_tool_1\",\"model\":\"gpt-5.4\",\"output_index\":0,\"item\":{\"type\":\"custom_tool_call\",\"id\":\"call_write_plan_custom_1\",\"name\":\"write_file\"}}\n\n",
            "data: {\"type\":\"response.custom_tool_call_input.delta\",\"response_id\":\"resp_gemini_custom_tool_1\",\"output_index\":0,\"item_id\":\"call_write_plan_custom_1\",\"delta\":\"{\\\"file_path\\\":\\\"plans/site.md\\\",\"}\n\n",
            "data: {\"type\":\"response.custom_tool_call_input.done\",\"response_id\":\"resp_gemini_custom_tool_1\",\"output_index\":0,\"item_id\":\"call_write_plan_custom_1\",\"input\":\"{\\\"file_path\\\":\\\"plans/site.md\\\",\\\"content\\\":\\\"plan\\\"}\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_custom_tool_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[{\"type\":\"custom_tool_call\",\"id\":\"call_write_plan_custom_1\",\"name\":\"write_file\"}],\"usage\":{\"input_tokens\":4,\"output_tokens\":5,\"total_tokens\":9}}}\n\n",
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini custom tool stream");

    let event = mapped
        .split("\n\n")
        .filter_map(|frame| frame.strip_prefix("data: "))
        .find(|frame| !frame.trim().is_empty() && frame.trim() != "[DONE]")
        .map(|frame| serde_json::from_str::<serde_json::Value>(frame).expect("parse sse json"))
        .expect("tool event");
    let event = unwrap_gemini_cli_response(&event);
    let part = &event["candidates"][0]["content"]["parts"][0]["functionCall"];
    assert_eq!(part["name"], "write_file");
    assert_eq!(part["id"], "call_write_plan_custom_1");
    assert_eq!(part["args"]["file_path"], "plans/site.md");
    assert_eq!(part["args"]["content"], "plan");
    assert_eq!(
        event["functionCalls"][0]["args"]["file_path"],
        "plans/site.md"
    );
}

#[test]
fn gemini_sse_reader_requires_response_completed_before_done() {
    let upstream = open_mock_http_response("text/event-stream", "data: [DONE]\n\n");
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        false,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini done-only stream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.starts_with("data: "));
    assert!(!mapped.contains("event: error"));
    assert!(!collector.saw_terminal);
    assert_eq!(
        collector.terminal_error.as_deref(),
        Some("连接中断（可能是网络波动或客户端主动取消）")
    );
    assert_eq!(collector.last_event_type, None);
}

#[test]
fn gemini_sse_reader_marks_incomplete_trailing_json_as_stream_error() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_partial_1\"",
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        false,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini partial stream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.starts_with("data: "));
    assert!(!mapped.contains("event: error"));
    assert!(!collector.saw_terminal);
    assert_eq!(
        collector.terminal_error.as_deref(),
        Some("连接中断（可能是网络波动或客户端主动取消）")
    );
    assert_eq!(collector.last_event_type, None);
}

#[test]
fn gemini_raw_reader_outputs_plain_json_chunks() {
    let upstream = open_mock_http_response(
        "text/event-stream",
        concat!(
            "data: {\"type\":\"response.output_text.delta\",\"response_id\":\"resp_gemini_raw_1\",\"model\":\"gpt-5.4\",\"delta\":\"你好\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_gemini_raw_1\",\"model\":\"gpt-5.4\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
            "data: [DONE]\n\n"
        ),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Raw,
        false,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini raw stream");

    assert!(!mapped.contains("data: "));
    assert!(mapped.ends_with('\n'));
    let raw_frames = mapped
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    assert!(
        raw_frames.len() >= 2,
        "raw stream should stay line-delimited"
    );
    for frame in &raw_frames {
        let value: serde_json::Value = serde_json::from_str(frame).expect("parse raw json line");
        assert!(value.get("candidates").is_some() || value.get("error").is_some());
    }
}

#[test]
fn gemini_sse_reader_emits_structured_error_frame_for_incomplete_stream() {
    let upstream = open_mock_http_response("text/event-stream", "data: [DONE]\n\n");
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        false,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini incomplete sse");

    assert!(mapped.starts_with("data: "));
    assert!(!mapped.contains("event: error"));
    assert!(mapped.contains("\"error\""));
}

#[test]
fn gemini_cli_sse_error_frame_leaves_no_unconsumed_event_prefix() {
    let upstream = open_mock_http_response("text/event-stream", "data: [DONE]\n\n");
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini cli incomplete sse");

    assert!(mapped.starts_with("data: "));
    assert!(mapped.ends_with("\n\n"));
    assert!(!mapped.contains("event:"));
    assert!(mapped.trim_start().starts_with("data: {\"error\""));
}

#[test]
fn gemini_sse_reader_converts_raw_html_challenge_to_data_error_frame() {
    let upstream = open_mock_http_response(
        "text/html",
        "<html><head><title>Just a moment...</title></head><body>Cloudflare</body></html>",
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Sse,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini raw html challenge");

    assert!(mapped.starts_with("data: "));
    assert!(mapped.ends_with("\n\n"));
    assert!(!mapped.contains("event:"));
    assert!(!mapped.contains("<html"));
    assert!(mapped.contains("Cloudflare"));
    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert_eq!(
        collector.terminal_error.as_deref(),
        Some("Cloudflare 安全验证页（title=Just a moment...）")
    );
    assert_eq!(
        collector.upstream_error_hint.as_deref(),
        Some("Cloudflare 安全验证页（title=Just a moment...）")
    );
}

#[test]
fn gemini_raw_reader_emits_plain_json_error_for_incomplete_stream() {
    let upstream = open_mock_http_response("text/event-stream", "data: [DONE]\n\n");
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = GeminiSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        None,
        GeminiStreamOutputMode::Raw,
        true,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read gemini incomplete raw");

    assert!(!mapped.starts_with("data: "));
    assert!(mapped.ends_with('\n'));
    let value: serde_json::Value = serde_json::from_str(&mapped).expect("parse raw error json");
    assert!(value.get("error").is_some());
}

/// 函数 `passthrough_sse_reader_emits_keepalive_for_responses_stream`
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
fn passthrough_sse_reader_emits_keepalive_for_responses_stream() {
    let _guard = crate::test_env_guard();
    let _keepalive_guard = EnvGuard::set("CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS", "15");
    super::reload_from_env();

    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[
            (
                "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_keepalive_1\"}}\n\n",
                0,
            ),
            // Leave enough wall-clock time for the 15 ms keepalive timer to
            // fire even when Windows/reqwest coalesces the first read.
            ("data: [DONE]\n\n", 250),
        ],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = PassthroughSseUsageReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        PassthroughSseProtocol::Generic,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read passthrough sse");
    server.join().expect("join streaming mock upstream");
    super::reload_from_env();

    assert!(mapped.contains("\"type\":\"codexmanager.keepalive\""));
    assert!(mapped.contains("\"type\":\"response.created\""));
    assert!(mapped.contains("data: [DONE]"));
}

#[test]
fn passthrough_sse_reader_waits_for_first_upstream_frame_before_keepalive() {
    let _guard = crate::test_env_guard();
    let _keepalive_guard = EnvGuard::set("CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS", "5");
    super::reload_from_env();

    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[
            (
                "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_wait_first_frame\"}}\n\n",
                25,
            ),
            ("data: [DONE]\n\n", 0),
        ],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = PassthroughSseUsageReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        PassthroughSseProtocol::Generic,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read passthrough sse without initial keepalive");
    server.join().expect("join delayed first-frame upstream");
    super::reload_from_env();

    assert!(!mapped.contains("\"type\":\"codexmanager.keepalive\""));
    assert!(mapped.contains("\"type\":\"response.created\""));
    assert!(mapped.contains("data: [DONE]"));
}

#[test]
fn openai_responses_passthrough_reader_emits_keepalive_during_silent_gap() {
    let _guard = crate::test_env_guard();
    let _keepalive_guard = EnvGuard::set("CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS", "15");
    super::reload_from_env();

    let (stream_tx, stream_rx) = std::sync::mpsc::sync_channel(8);
    let producer = thread::spawn(move || {
        stream_tx
            .send(GatewayByteStreamItem::Chunk(Bytes::from_static(
                b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_eventsource_keepalive\"}}\n\n",
            )))
            .expect("send created frame");
        thread::sleep(Duration::from_millis(80));
        stream_tx
            .send(GatewayByteStreamItem::Chunk(Bytes::from_static(
                b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_eventsource_keepalive\"}}\n\n",
            )))
            .expect("send completed frame");
        stream_tx
            .send(GatewayByteStreamItem::Eof)
            .expect("send stream eof");
    });
    let upstream = GatewayStreamResponse::new(
        reqwest::StatusCode::OK,
        reqwest::header::HeaderMap::new(),
        GatewayByteStream::from_receiver(stream_rx),
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::from_stream_response(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read openai responses passthrough sse");
    producer
        .join()
        .expect("join openai responses stream producer");
    super::reload_from_env();

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("\"type\":\"codexmanager.keepalive\""));
    assert!(mapped.contains("event: response.created"));
    assert!(mapped.contains("event: response.completed"));
    let created_at = mapped
        .find("event: response.created")
        .expect("created event position");
    let keepalive_at = mapped
        .find("\"type\":\"codexmanager.keepalive\"")
        .expect("keepalive position");
    let completed_at = mapped
        .find("event: response.completed")
        .expect("completed event position");
    assert!(created_at < keepalive_at);
    assert!(keepalive_at < completed_at);
    assert!(collector.saw_terminal);
    assert_eq!(
        collector.last_event_type.as_deref(),
        Some("response.completed")
    );
}

#[test]
fn openai_responses_passthrough_reader_emits_keepalive_before_delayed_first_frame() {
    let _guard = crate::test_env_guard();
    let _keepalive_guard = EnvGuard::set("CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS", "10");
    super::reload_from_env();

    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[
            (
                "event: response.created\n\
                 data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_delayed_first\"}}\n\n",
                60,
            ),
            (
                "event: response.completed\n\
                 data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_delayed_first\"}}\n\n",
                0,
            ),
        ],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read delayed first openai responses stream");
    server.join().expect("join delayed first-frame upstream");
    super::reload_from_env();

    let keepalive_at = mapped
        .find("\"type\":\"codexmanager.keepalive\"")
        .expect("keepalive position");
    let created_at = mapped
        .find("event: response.created")
        .expect("created event position");
    assert!(keepalive_at < created_at);
    assert!(mapped.contains("event: response.completed"));
}

#[test]
fn openai_responses_passthrough_reader_parses_split_events_with_eventsource_stream() {
    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[
            ("event: response.output_text.delta\n", 0),
            ("data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel", 0),
            ("lo\"}\n\n", 0),
            (
                "event: response.completed\n\
                 data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_split\",\"usage\":{\"input_tokens\":1,\"input_tokens_details\":null,\"output_tokens\":1,\"output_tokens_details\":null,\"total_tokens\":2}}}\n\n",
                0,
            ),
        ],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read split openai responses stream");
    server.join().expect("join split openai responses upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("event: response.output_text.delta"));
    assert!(mapped.contains("\"delta\":\"hello\""));
    assert!(mapped.contains("event: response.completed"));
    assert_eq!(collector.usage.output_text.as_deref(), Some("hello"));
    assert_eq!(collector.usage.total_tokens, Some(2));
    assert!(collector.saw_terminal);
    assert_eq!(
        collector.last_event_type.as_deref(),
        Some("response.completed")
    );
}

#[test]
fn openai_responses_passthrough_reader_collects_output_item_field_text() {
    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "event: response.output_item.done\n\
             data: {\"output_item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello from output_item\"}]}}\n\n\
             event: response.completed\n\
             data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_output_item\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read output_item openai responses stream");
    server
        .join()
        .expect("join output_item openai responses upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("event: response.output_item.done"));
    assert_eq!(
        collector.usage.output_text.as_deref(),
        Some("hello from output_item")
    );
    assert_eq!(collector.usage.total_tokens, Some(2));
    assert!(collector.saw_terminal);
}

#[test]
fn openai_responses_passthrough_reader_usage_text_not_duplicated_across_delta_item_completed() {
    let json_text = r#"{"answer":true}"#;
    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "event: response.output_text.delta\n\
             data: {\"response_id\":\"resp_json_single\",\"delta\":\"{\\\"answer\\\":true}\"}\n\n\
             event: response.output_item.done\n\
             data: {\"response_id\":\"resp_json_single\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}}\n\n\
             event: response.completed\n\
             data: {\"response\":{\"id\":\"resp_json_single\",\"created\":3,\"model\":\"gpt-5.3-codex\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read json mode openai responses stream");
    server
        .join()
        .expect("join json mode openai responses upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("event: response.output_item.done"));
    assert_eq!(collector.usage.output_text.as_deref(), Some(json_text));
    assert_eq!(collector.usage.input_tokens, Some(3));
    assert_eq!(collector.usage.output_tokens, Some(2));
    assert_eq!(collector.usage.total_tokens, Some(5));
    assert!(collector.saw_terminal);
}

#[test]
fn openai_responses_passthrough_reader_usage_text_dedupes_snapshot_only_events() {
    let json_text = r#"{"answer":true}"#;
    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "event: response.output_text.done\n\
             data: {\"response_id\":\"resp_snapshot_single\",\"output_index\":0,\"content_index\":0,\"text\":\"{\\\"answer\\\":true}\"}\n\n\
             event: response.content_part.done\n\
             data: {\"response_id\":\"resp_snapshot_single\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}}\n\n\
             event: response.output_item.done\n\
             data: {\"response_id\":\"resp_snapshot_single\",\"output_index\":0,\"item\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}}\n\n\
             event: response.completed\n\
             data: {\"response\":{\"id\":\"resp_snapshot_single\",\"created\":3,\"model\":\"gpt-5.3-codex\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"{\\\"answer\\\":true}\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2,\"total_tokens\":5}}}\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read snapshot-only openai responses stream");
    server
        .join()
        .expect("join snapshot-only openai responses upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("event: response.output_item.done"));
    assert_eq!(collector.usage.output_text.as_deref(), Some(json_text));
    assert_eq!(collector.usage.input_tokens, Some(3));
    assert_eq!(collector.usage.output_tokens, Some(2));
    assert_eq!(collector.usage.total_tokens, Some(5));
    assert!(collector.saw_terminal);
}

#[test]
fn openai_responses_passthrough_reader_marks_incomplete_terminal_error_from_status_details() {
    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "event: response.output_text.delta\n\
             data: {\"delta\":{\"text\":\"partial answer\"}}\n\n\
             event: response.incomplete\n\
             data: {\"response\":{\"status\":\"incomplete\",\"status_details\":{\"error\":{\"message\":\"stream timeout at upstream\",\"code\":\"stream_timeout\"}}}}\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read incomplete openai responses stream");
    server
        .join()
        .expect("join incomplete openai responses upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("event: response.incomplete"));
    assert_eq!(
        collector.usage.output_text.as_deref(),
        Some("partial answer")
    );
    assert_eq!(
        collector.terminal_error.as_deref(),
        Some("上游流式空闲超时")
    );
    assert_eq!(
        collector.upstream_error_hint.as_deref(),
        Some("code=stream_timeout stream timeout at upstream")
    );
    assert!(collector.saw_terminal);
    assert_eq!(
        collector.last_event_type.as_deref(),
        Some("response.incomplete")
    );
}

#[test]
fn openai_responses_passthrough_reader_maps_bare_incomplete_to_disconnect_message() {
    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "event: response.output_text.delta\n\
             data: {\"delta\":{\"text\":\"partial answer\"}}\n\n\
             event: response.incomplete\n\
             data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\"}}\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = OpenAIResponsesPassthroughSseReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read incomplete openai responses stream");
    server
        .join()
        .expect("join incomplete openai responses upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("event: response.incomplete"));
    assert_eq!(
        collector.terminal_error.as_deref(),
        Some("连接中断（可能是网络波动或客户端主动取消）")
    );
    assert_eq!(collector.upstream_error_hint, None);
    assert!(collector.saw_terminal);
}

/// 函数 `passthrough_sse_reader_captures_raw_html_error_body`
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
fn passthrough_sse_reader_captures_raw_html_error_body() {
    let (upstream, server) = open_streaming_mock_http_response(
        "text/html",
        &[(
            "<html><title>Just a moment...</title><body>cf</body></html>",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = PassthroughSseUsageReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::OpenAIResponses,
        PassthroughSseProtocol::Generic,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read passthrough html body");
    server.join().expect("join html mock upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("Just a moment"));
    assert_eq!(
        collector.upstream_error_hint.as_deref(),
        Some("Cloudflare 安全验证页（title=Just a moment...）")
    );
    assert_eq!(
        collector.terminal_error.as_deref(),
        Some("Cloudflare 安全验证页（title=Just a moment...）")
    );
}

#[test]
fn passthrough_sse_reader_treats_message_stop_as_terminal_for_anthropic_native() {
    let (upstream, server) = open_streaming_mock_http_response(
        "text/event-stream",
        &[(
            "event: message_start\n\
             data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\"}}\n\n\
             event: content_block_delta\n\
             data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你好\"}}\n\n\
             event: message_stop\n\
             data: {\"type\":\"message_stop\"}\n\n",
            0,
        )],
    );
    let usage_collector = Arc::new(Mutex::new(PassthroughSseCollector::default()));
    let mut reader = PassthroughSseUsageReader::new(
        upstream,
        Arc::clone(&usage_collector),
        SseKeepAliveFrame::Anthropic,
        PassthroughSseProtocol::AnthropicNative,
        std::time::Instant::now(),
    );
    let mut mapped = String::new();
    reader
        .read_to_string(&mut mapped)
        .expect("read anthropic passthrough sse");
    server.join().expect("join anthropic passthrough upstream");

    let collector = usage_collector
        .lock()
        .expect("lock usage collector")
        .clone();
    assert!(mapped.contains("event: message_stop"));
    assert!(collector.saw_terminal);
    assert_eq!(collector.last_event_type.as_deref(), Some("message_stop"));
    assert_eq!(collector.terminal_error, None);
}
