use super::{
    apply_final_upstream_header_policy, apply_gemini_codex_compat_header_profile,
    encode_request_body, is_session_scoped_header, resolve_request_compression,
    resolve_request_compression_with_flag, send_async_stream_request,
    should_retry_transport_without_compression, should_wrap_upstream_as_stream_response,
    strip_compact_service_tier_for_transport, RequestCompression, CPA_GEMINI_CODEX_USER_AGENT,
};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Builder;

type WsServerRequest = tokio_tungstenite::tungstenite::handshake::server::Request;
type WsServerResponse = tokio_tungstenite::tungstenite::handshake::server::Response;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

struct RuntimeConfigReloadGuard;

impl Drop for RuntimeConfigReloadGuard {
    fn drop(&mut self) {
        let _ = std::panic::catch_unwind(crate::gateway::reload_runtime_config_from_env);
    }
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[test]
fn explicit_stateless_mode_targets_only_session_scoped_headers() {
    for name in [
        "session-id",
        "thread-id",
        "x-client-request-id",
        "x-codex-window-id",
        "x-codex-turn-state",
        "session_id",
    ] {
        assert!(
            is_session_scoped_header(name),
            "expected {name} to be removed"
        );
    }
    assert!(!is_session_scoped_header("x-codex-parent-thread-id"));
    assert!(!is_session_scoped_header("authorization"));
}

#[test]
fn explicit_stateless_mode_removes_session_id_added_by_gemini_profile() {
    let mut headers = vec![("session-id".to_string(), "incoming-session".to_string())];

    apply_final_upstream_header_policy(&mut headers, true, None, true);

    assert_eq!(header_value(&headers, "session-id"), None);
    assert_eq!(header_value(&headers, "session_id"), None);
}

fn spawn_raw_http_response(
    status: &'static str,
    headers: Vec<(&'static str, &'static str)>,
    body: Vec<u8>,
    hold_open: bool,
) -> (String, mpsc::Sender<()>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept mock upstream");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        let mut request = Vec::new();
        let mut buf = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => request.extend_from_slice(&buf[..read]),
                Err(_) => break,
            }
        }

        let mut response = format!("HTTP/1.1 {status}\r\n");
        for (name, value) in headers {
            response.push_str(name);
            response.push_str(": ");
            response.push_str(value);
            response.push_str("\r\n");
        }
        if hold_open {
            response.push_str("Connection: keep-alive\r\n\r\n");
        } else {
            response.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ));
        }
        stream
            .write_all(response.as_bytes())
            .expect("write mock response headers");
        stream.write_all(&body).expect("write mock response body");
        stream.flush().expect("flush mock response");
        if hold_open {
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        }
    });
    (format!("http://{addr}/v1/responses"), release_tx, handle)
}

fn spawn_active_streaming_http_response(
    chunks: Vec<Vec<u8>>,
    chunk_interval: Duration,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind streaming mock upstream");
    let addr = listener.local_addr().expect("streaming mock upstream addr");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept streaming mock upstream");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set streaming mock read timeout");
        let mut request = Vec::new();
        let mut buf = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => request.extend_from_slice(&buf[..read]),
                Err(_) => break,
            }
        }

        let body_len = chunks.iter().map(Vec::len).sum::<usize>();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n"
        )
        .expect("write streaming mock response headers");
        stream.flush().expect("flush streaming mock headers");

        for chunk in chunks {
            thread::sleep(chunk_interval);
            stream
                .write_all(chunk.as_slice())
                .expect("write streaming mock body chunk");
            stream.flush().expect("flush streaming mock body chunk");
        }
    });

    (format!("http://{addr}/v1/responses"), handle)
}

fn spawn_delayed_http_response_headers(delay: Duration) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind delayed-header mock upstream");
    let addr = listener
        .local_addr()
        .expect("delayed-header mock upstream addr");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("accept delayed-header mock upstream");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set delayed-header mock read timeout");
        let mut request = Vec::new();
        let mut buf = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => request.extend_from_slice(&buf[..read]),
                Err(_) => break,
            }
        }

        thread::sleep(delay);
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 14\r\nConnection: close\r\n\r\ndata: [DONE]\n\n",
        );
        let _ = stream.flush();
    });

    (format!("http://{addr}/v1/responses"), handle)
}

fn send_mock_stream_request(url: &str) -> super::super::super::GatewayStreamResponse {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .build()
        .expect("build reqwest client");
    send_async_stream_request(
        &client,
        &reqwest::Method::GET,
        url,
        "/v1/responses",
        Some(Instant::now() + Duration::from_secs(5)),
        &[],
        &Bytes::new(),
        true,
    )
    .expect("send stream request")
}

fn spawn_mock_websocket_upstream(
    response_text: &'static str,
) -> (
    String,
    Receiver<Vec<(String, String)>>,
    Receiver<String>,
    thread::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock websocket upstream");
    listener
        .set_nonblocking(true)
        .expect("set mock websocket listener nonblocking");
    let addr = listener.local_addr().expect("mock websocket upstream addr");
    let (headers_tx, headers_rx) = mpsc::channel();
    let (frame_tx, frame_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build mock websocket runtime");
        runtime.block_on(async move {
            let listener =
                tokio::net::TcpListener::from_std(listener).expect("convert websocket listener");
            let (stream, _) = listener.accept().await.expect("accept websocket client");
            let mut websocket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &WsServerRequest, response: WsServerResponse| {
                    let headers = request
                        .headers()
                        .iter()
                        .map(|(name, value)| {
                            (
                                name.as_str().to_ascii_lowercase(),
                                value.to_str().unwrap_or_default().to_string(),
                            )
                        })
                        .collect::<Vec<_>>();
                    let _ = headers_tx.send(headers);
                    Ok(response)
                },
            )
            .await
            .expect("accept websocket handshake");

            if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) =
                websocket.next().await
            {
                let _ = frame_tx.send(text.to_string());
            }
            let _ = websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    response_text.into(),
                ))
                .await;
        });
    });
    (
        format!("ws://{addr}/v1/responses"),
        headers_rx,
        frame_rx,
        handle,
    )
}

fn spawn_http_connect_proxy(
    target_addr: String,
) -> (String, Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock websocket proxy");
    let proxy_addr = listener.local_addr().expect("mock websocket proxy addr");
    let (connect_tx, connect_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut client, _) = listener.accept().expect("accept proxy client");
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set proxy client read timeout");
        let mut request = Vec::new();
        let mut buf = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = client.read(&mut buf).expect("read proxy CONNECT request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
        }
        let request_text = String::from_utf8_lossy(request.as_slice());
        let first_line = request_text.lines().next().unwrap_or_default().to_string();
        let _ = connect_tx.send(first_line);

        let mut upstream = TcpStream::connect(target_addr.as_str())
            .expect("connect proxy to mock websocket upstream");
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .expect("write proxy CONNECT response");

        let mut client_reader = client.try_clone().expect("clone proxy client reader");
        let mut upstream_writer = upstream.try_clone().expect("clone proxy upstream writer");
        let upstream_to_client = thread::spawn(move || {
            let _ = std::io::copy(&mut upstream, &mut client);
        });
        let client_to_upstream = thread::spawn(move || {
            let _ = std::io::copy(&mut client_reader, &mut upstream_writer);
        });
        let _ = client_to_upstream.join();
        let _ = upstream_to_client.join();
    });
    (format!("http://{proxy_addr}"), connect_rx, handle)
}

fn captured_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name == name)
        .map(|(_, value)| value.as_str())
}

/// 函数 `request_compression_only_applies_to_streaming_chatgpt_responses`
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
fn request_compression_only_applies_to_streaming_chatgpt_responses() {
    assert_eq!(
        resolve_request_compression_with_flag(
            true,
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            true
        ),
        RequestCompression::Zstd
    );
    assert_eq!(
        resolve_request_compression_with_flag(
            true,
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses/compact",
            true
        ),
        RequestCompression::None
    );
    assert_eq!(
        resolve_request_compression_with_flag(
            true,
            "https://api.openai.com/v1/responses",
            "/v1/responses",
            true
        ),
        RequestCompression::None
    );
    assert_eq!(
        resolve_request_compression_with_flag(
            true,
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            false
        ),
        RequestCompression::None
    );
    assert_eq!(
        resolve_request_compression_with_flag(
            false,
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            true
        ),
        RequestCompression::None
    );
}

#[test]
fn gemini_codex_compat_disables_request_compression_like_cpa() {
    assert_eq!(
        resolve_request_compression(
            crate::apikey_profile::PROTOCOL_GEMINI_NATIVE,
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            true,
        ),
        RequestCompression::None
    );
}

#[test]
fn gemini_codex_compat_does_not_preserve_client_identity_like_cpa() {
    assert!(!super::should_preserve_client_identity(
        crate::apikey_profile::PROTOCOL_GEMINI_NATIVE
    ));
}

#[test]
fn gemini_codex_compat_header_profile_matches_cpa_executor_shape() {
    let mut headers = vec![
        (
            "User-Agent".to_string(),
            "gemini-cli/0.1.14 (Windows 11; x86_64)".to_string(),
        ),
        ("originator".to_string(), "gemini_cli".to_string()),
        ("x-codex-window-id".to_string(), "thread:0".to_string()),
        ("x-codex-turn-state".to_string(), "turn-state".to_string()),
        (
            "x-codex-parent-thread-id".to_string(),
            "parent-thread".to_string(),
        ),
        ("x-openai-subagent".to_string(), "subagent".to_string()),
        ("session-id".to_string(), "session-current".to_string()),
        ("thread-id".to_string(), "thread-current".to_string()),
    ];

    apply_gemini_codex_compat_header_profile(&mut headers, None);

    assert_eq!(
        header_value(&headers, "User-Agent"),
        Some(CPA_GEMINI_CODEX_USER_AGENT)
    );
    assert_eq!(header_value(&headers, "originator"), Some("codex-tui"));
    assert_eq!(header_value(&headers, "Connection"), Some("Keep-Alive"));
    assert_eq!(header_value(&headers, "x-codex-window-id"), None);
    assert_eq!(header_value(&headers, "x-codex-turn-state"), None);
    assert_eq!(header_value(&headers, "x-codex-parent-thread-id"), None);
    assert_eq!(header_value(&headers, "x-openai-subagent"), None);
    assert_eq!(header_value(&headers, "session-id"), None);
    assert_eq!(header_value(&headers, "thread-id"), None);
    assert_eq!(header_value(&headers, "session_id").map(str::len), Some(36));
}

/// 函数 `encode_request_body_adds_zstd_content_encoding`
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
fn encode_request_body_adds_zstd_content_encoding() {
    let body = Bytes::from_static(br#"{"model":"gpt-5.4","input":"compress me"}"#);
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];

    let actual = encode_request_body(
        "/v1/responses",
        &body,
        RequestCompression::Zstd,
        &mut headers,
    );

    assert!(headers
        .iter()
        .any(|(name, value)| { name.eq_ignore_ascii_case("Content-Encoding") && value == "zstd" }));
    let decoded =
        zstd::stream::decode_all(std::io::Cursor::new(actual.as_ref())).expect("decode zstd body");
    let value: serde_json::Value =
        serde_json::from_slice(&decoded).expect("parse decompressed json");
    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("gpt-5.4")
    );
}

#[test]
fn compact_transport_strips_service_tier_without_chatgpt_account_header() {
    let body = Bytes::from_static(
        br#"{"model":"gpt-5.4","input":[],"service_tier":"priority","prompt_cache_key":"thread-1"}"#,
    );

    let actual = strip_compact_service_tier_for_transport(&body, false);
    let value: serde_json::Value =
        serde_json::from_slice(&actual).expect("parse stripped compact body");

    assert!(value.get("service_tier").is_none());
    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("thread-1")
    );
}

#[test]
fn compact_transport_preserves_service_tier_with_chatgpt_account_header() {
    let body = Bytes::from_static(
        br#"{"model":"gpt-5.4","input":[],"service_tier":"priority","prompt_cache_key":"thread-1"}"#,
    );

    let actual = strip_compact_service_tier_for_transport(&body, true);
    let value: serde_json::Value =
        serde_json::from_slice(&actual).expect("parse preserved compact body");

    assert_eq!(
        value
            .get("service_tier")
            .and_then(serde_json::Value::as_str),
        Some("priority")
    );
}

#[test]
fn transport_retry_without_compression_only_targets_streaming_chatgpt_responses() {
    assert!(should_retry_transport_without_compression(
        "https://chatgpt.com/backend-api/codex/responses",
        "/v1/responses",
        true,
        RequestCompression::Zstd
    ));
    assert!(!should_retry_transport_without_compression(
        "https://chatgpt.com/backend-api/codex/responses",
        "/v1/responses/compact",
        true,
        RequestCompression::Zstd
    ));
    assert!(!should_retry_transport_without_compression(
        "https://api.openai.com/v1/responses",
        "/v1/responses",
        true,
        RequestCompression::Zstd
    ));
    assert!(!should_retry_transport_without_compression(
        "https://chatgpt.com/backend-api/codex/responses",
        "/v1/responses",
        false,
        RequestCompression::Zstd
    ));
    assert!(!should_retry_transport_without_compression(
        "https://chatgpt.com/backend-api/codex/responses",
        "/v1/responses",
        true,
        RequestCompression::None
    ));
}

#[test]
fn anthropic_codex_compat_disables_request_compression_without_touching_codex() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _compression_guard = EnvGuard::set("CODEXMANAGER_ENABLE_REQUEST_COMPRESSION", "1");
    crate::gateway::reload_runtime_config_from_env();

    assert_eq!(
        resolve_request_compression(
            crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            true,
        ),
        RequestCompression::None
    );
    assert_eq!(
        resolve_request_compression(
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            true,
        ),
        RequestCompression::Zstd
    );
}

#[test]
fn transport_wraps_non_compact_responses_streams_into_stream_variant() {
    assert!(should_wrap_upstream_as_stream_response(
        "/v1/responses",
        true
    ));
    assert!(should_wrap_upstream_as_stream_response(
        "/v1/responses?stream=false",
        true
    ));
    assert!(!should_wrap_upstream_as_stream_response(
        "/v1/responses/compact",
        true
    ));
    assert!(!should_wrap_upstream_as_stream_response(
        "/v1/chat/completions",
        true
    ));
    assert!(!should_wrap_upstream_as_stream_response(
        "/v1/responses",
        false
    ));
}

#[test]
fn stream_transport_fast_closes_html_challenge_without_waiting_for_eof() {
    let body = b"<html><title>Just a moment...</title><body>Cloudflare</body></html>".to_vec();
    let (url, release, handle) = spawn_raw_http_response(
        "403 Forbidden",
        vec![
            ("Content-Type", "text/html; charset=utf-8"),
            ("cf-ray", "ray-fast-close"),
        ],
        body,
        true,
    );

    let started = Instant::now();
    let response = send_mock_stream_request(url.as_str());
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    let body = response.read_all_bytes().expect("read fast-closed body");

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "fast-close path waited {:?}",
        started.elapsed()
    );
    assert!(String::from_utf8_lossy(body.as_ref()).contains("Just a moment"));
    let _ = release.send(());
    handle.join().expect("join mock upstream");
}

#[test]
fn stream_transport_does_not_fast_close_json_error_body() {
    let body = vec![b'a'; 70 * 1024];
    let expected_len = body.len();
    let (url, release, handle) = spawn_raw_http_response(
        "403 Forbidden",
        vec![("Content-Type", "application/json")],
        body,
        false,
    );

    let response = send_mock_stream_request(url.as_str());
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    let body = response.read_all_bytes().expect("read json error body");

    assert_eq!(body.len(), expected_len);
    let _ = release.send(());
    handle.join().expect("join mock upstream");
}

#[test]
fn stream_transport_does_not_fast_close_successful_sse_body() {
    let expected = b"data: {\"type\":\"response.completed\"}\n\n".to_vec();
    let (url, release, handle) = spawn_raw_http_response(
        "200 OK",
        vec![("Content-Type", "text/event-stream")],
        expected.clone(),
        false,
    );

    let response = send_mock_stream_request(url.as_str());
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response.read_all_bytes().expect("read sse body");

    assert_eq!(body.as_ref(), expected.as_slice());
    let _ = release.send(());
    handle.join().expect("join mock upstream");
}

// ── WebSocket upstream selection & fallback ────────────────────────────────

#[test]
fn stream_transport_timeout_does_not_cap_active_body_duration() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _stream_timeout_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS", "200");
    crate::gateway::reload_runtime_config_from_env();

    let chunks = vec![
        b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"one\"}\n\n".to_vec(),
        b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"two\"}\n\n".to_vec(),
        b"data: {\"type\":\"response.completed\"}\n\n".to_vec(),
    ];
    let expected = chunks.concat();
    let (url, handle) = spawn_active_streaming_http_response(chunks, Duration::from_millis(80));

    let response = send_mock_stream_request(url.as_str());
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response
        .read_all_bytes()
        .expect("active response body may outlive the header deadline");

    assert_eq!(body.as_ref(), expected.as_slice());
    handle.join().expect("join streaming mock upstream");
}

#[test]
fn stream_transport_reports_response_headers_timeout() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _stream_timeout_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS", "50");
    crate::gateway::reload_runtime_config_from_env();

    let (url, handle) = spawn_delayed_http_response_headers(Duration::from_millis(200));
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .build()
        .expect("build reqwest client");
    let started_at = Instant::now();
    let result = send_async_stream_request(
        &client,
        &reqwest::Method::GET,
        url.as_str(),
        "/v1/responses",
        Some(Instant::now() + Duration::from_secs(5)),
        &[],
        &Bytes::new(),
        true,
    );

    assert!(
        matches!(
            result,
            Err(super::AsyncStreamRequestError::ResponseHeadersTimeout(timeout))
                if timeout == Duration::from_millis(50)
        ),
        "delayed response headers must report ResponseHeadersTimeout, got {result:?}"
    );
    assert!(
        started_at.elapsed() < Duration::from_millis(500),
        "configured response-header timeout was not enforced: {:?}",
        started_at.elapsed()
    );
    handle.join().expect("join delayed-header mock upstream");
}

#[test]
fn websocket_upstream_not_selected_when_flag_disabled() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _ws_guard = EnvGuard::set("CODEXMANAGER_USE_WEBSOCKET_UPSTREAM", "0");
    crate::gateway::reload_runtime_config_from_env();
    // Runtime config flag is false by default (DEFAULT_USE_WEBSOCKET_UPSTREAM = false),
    // so should_use_websocket_upstream must return false regardless of the URL.
    assert!(!super::should_use_websocket_upstream(
        "https://chatgpt.com/backend-api/codex/responses"
    ));
    assert!(!super::should_use_websocket_upstream(
        "https://api.chatgpt.com/v1/responses"
    ));
    assert!(!super::should_use_websocket_upstream(
        "https://example.com/api"
    ));
}

#[test]
fn chatgpt_target_url_accepts_valid_hosts_and_rejects_lookalikes() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _ws_guard = EnvGuard::set("CODEXMANAGER_USE_WEBSOCKET_UPSTREAM", "0");
    crate::gateway::reload_runtime_config_from_env();
    // Positive: exact match and subdomain
    assert!(super::is_chatgpt_target_url(
        "https://chatgpt.com/backend-api/codex/responses"
    ));
    assert!(super::is_chatgpt_target_url(
        "https://api.chatgpt.com/v1/responses"
    ));
    assert!(super::is_chatgpt_target_url(
        "https://backend.chatgpt.com/path"
    ));

    // Negative: substring lookalikes must NOT match
    assert!(!super::is_chatgpt_target_url(
        "https://evilchatgpt.com/path"
    ));
    assert!(!super::is_chatgpt_target_url(
        "https://chatgpt.com.evil.com/path"
    ));
    assert!(!super::is_chatgpt_target_url("https://notchatgpt.com/path"));
    assert!(!super::is_chatgpt_target_url("https://example.com/api"));

    // should_use_websocket_upstream with flag disabled always returns false —
    // this confirms the guard is evaluated FIRST, before the host check.
    assert!(!super::should_use_websocket_upstream(
        "https://chatgpt.com/backend-api/codex/responses"
    ));
}

#[test]
fn websocket_upstream_terminal_detection_parses_json_type() {
    assert!(super::is_websocket_upstream_terminal_text(
        r#"{"type":"response.completed"}"#
    ));
    assert!(!super::is_websocket_upstream_terminal_text(
        r#"{"type":"response.done"}"#
    ));
    assert!(super::is_websocket_upstream_terminal_text(
        r#"{"type":"response.incomplete"}"#
    ));
    assert!(super::is_websocket_upstream_terminal_text(
        r#"{"type":"error","error":{"message":"boom"}}"#
    ));
    assert!(!super::is_websocket_upstream_terminal_text(
        r#"{"type":"response.output_text.delta","delta":"response.done"}"#
    ));
    assert!(!super::is_websocket_upstream_terminal_text(
        r#"not json response.completed"#
    ));
}

#[test]
fn websocket_upstream_recovery_requires_one_probe_and_completed_event() {
    let start = Instant::now();
    let cooldown = Duration::from_secs(30);
    let mut state = super::WebsocketUpstreamRecoveryState::default();

    assert!(state.try_acquire_probe(start, cooldown));
    assert!(
        !state.try_acquire_probe(start + Duration::from_secs(1), cooldown),
        "only one recovery probe may be in flight"
    );

    state.mark_failed(start);
    assert!(
        !state.try_acquire_probe(start + Duration::from_secs(29), cooldown),
        "failed probe must keep subsequent requests on HTTP during cooldown"
    );
    assert!(state.try_acquire_probe(start + cooldown, cooldown));
    state.mark_completed();
    assert!(
        state.try_acquire_probe(start + cooldown, cooldown),
        "response.completed must clear the cooldown after the probe"
    );

    state.mark_failed(start + cooldown);
    assert!(
        !state.try_acquire_probe(start + cooldown + Duration::from_secs(1), cooldown),
        "an incomplete recovery must continue using HTTP"
    );
}

#[test]
fn websocket_upstream_recovery_success_is_only_response_completed() {
    assert!(super::is_websocket_upstream_completed_text(
        r#"{"type":"response.completed"}"#
    ));
    assert!(!super::is_websocket_upstream_completed_text(
        r#"{"type":"response.done"}"#
    ));
    assert!(!super::is_websocket_upstream_completed_text(
        r#"{"type":"response.failed"}"#
    ));
}

#[test]
fn websocket_upstream_request_text_from_http_body_wraps_response_create() {
    let body = Bytes::from_static(
        br#"{"model":"codex","input":"hello","stream":true,"reasoning":{"effort":"high"}}"#,
    );

    let text = super::websocket_upstream_request_text_from_http_body(&body)
        .expect("wrap valid HTTP body")
        .expect("non-empty body produces websocket frame");
    let value: serde_json::Value =
        serde_json::from_str(text.as_str()).expect("parse wrapped websocket frame");

    assert_eq!(
        value.get("type").and_then(serde_json::Value::as_str),
        Some("response.create")
    );
    assert_eq!(
        value.get("model").and_then(serde_json::Value::as_str),
        Some("codex")
    );
    assert_eq!(
        value.get("input").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    assert_eq!(
        value.get("stream").and_then(serde_json::Value::as_bool),
        None
    );
    assert_eq!(
        value
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(serde_json::Value::as_str),
        Some("high")
    );
}

#[test]
fn websocket_upstream_request_text_from_http_body_rejects_invalid_payload() {
    let body = Bytes::from_static(br#"{"model":"codex"}"#);

    let err = super::websocket_upstream_request_text_from_http_body(&body)
        .expect_err("missing input is not a valid response.create payload");

    assert!(
        err.contains("valid WebSocket response.create payload"),
        "unexpected error: {err}"
    );
}

#[test]
fn send_websocket_upstream_request_builds_valid_handshake_and_stops_on_completed() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
    crate::gateway::reload_runtime_config_from_env();
    let (url, headers_rx, frame_rx, handle) =
        spawn_mock_websocket_upstream(r#"{"type":"response.completed"}"#);
    let body = Bytes::from(r#"{"model":"codex","input":"hello"}"#);
    let response = super::send_websocket_upstream_request(
        url.as_str(),
        "acct_ws_direct",
        Some(Instant::now() + Duration::from_secs(5)),
        &[
            ("Authorization".to_string(), "Bearer token_ws".to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Connection".to_string(), "close".to_string()),
        ],
        &body,
    )
    .expect("websocket upstream request connects");
    let response_body = response.read_all_bytes().expect("read websocket body");
    assert_eq!(
        response_body.as_ref(),
        b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n"
    );

    let headers = headers_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("capture websocket headers");
    assert_eq!(captured_header(&headers, "upgrade"), Some("websocket"));
    assert!(captured_header(&headers, "connection")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .split(',')
        .any(|token| token.trim() == "upgrade"));
    assert_eq!(
        captured_header(&headers, "sec-websocket-version"),
        Some("13")
    );
    assert!(captured_header(&headers, "sec-websocket-key").is_some());
    assert!(captured_header(&headers, "host").is_some());
    assert_eq!(
        captured_header(&headers, "authorization"),
        Some("Bearer token_ws")
    );
    assert_eq!(
        captured_header(&headers, "openai-beta"),
        Some("responses_websockets=2026-02-06")
    );
    assert!(captured_header(&headers, "content-type").is_none());
    let frame = frame_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("capture request frame");
    let frame: serde_json::Value =
        serde_json::from_str(frame.as_str()).expect("parse request frame");
    assert_eq!(
        frame.get("type").and_then(serde_json::Value::as_str),
        Some("response.create")
    );
    assert_eq!(
        frame.get("model").and_then(serde_json::Value::as_str),
        Some("codex")
    );
    assert_eq!(
        frame.get("input").and_then(serde_json::Value::as_str),
        Some("hello")
    );
    handle.join().expect("join websocket upstream");
}

#[test]
fn send_websocket_upstream_request_does_not_mark_recovery_completed_after_application_failure() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
    crate::gateway::reload_runtime_config_from_env();
    let (url, _headers_rx, _frame_rx, handle) =
        spawn_mock_websocket_upstream(r#"{"type":"response.failed"}"#);
    let body = Bytes::from(r#"{"model":"codex","input":"hello"}"#);

    let response = super::send_websocket_upstream_request(
        url.as_str(),
        "acct_ws_incomplete_probe",
        Some(Instant::now() + Duration::from_secs(5)),
        &[],
        &body,
    )
    .expect("incomplete websocket probe still returns its terminal event");
    let response_body = response
        .read_all_bytes()
        .expect("read incomplete websocket probe body");
    assert!(String::from_utf8_lossy(response_body.as_ref()).contains("response.failed"));

    assert!(
        super::is_websocket_upstream_transport_healthy_terminal_text(
            r#"{"type":"response.completed"}"#
        )
    );
    assert!(
        !super::is_websocket_upstream_transport_healthy_terminal_text(
            r#"{"type":"response.failed"}"#
        )
    );
    handle.join().expect("join incomplete websocket upstream");
}

#[test]
fn send_websocket_upstream_request_uses_system_environment_proxy() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let (target_url, headers_rx, _frame_rx, target_handle) =
        spawn_mock_websocket_upstream(r#"{"type":"response.completed"}"#);
    let parsed_target = reqwest::Url::parse(target_url.as_str()).expect("parse target url");
    let target_addr = parsed_target
        .socket_addrs(|| None)
        .expect("target socket addr")
        .first()
        .expect("target has addr")
        .to_string();
    let (proxy_url, connect_rx, proxy_handle) = spawn_http_connect_proxy(target_addr);
    let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
    let _upper_http_proxy_guard = EnvGuard::set("HTTP_PROXY", "");
    let _http_proxy_guard = EnvGuard::set("http_proxy", proxy_url.as_str());
    let _all_proxy_guard = EnvGuard::set("all_proxy", "");
    let _upper_all_proxy_guard = EnvGuard::set("ALL_PROXY", "");
    let _no_proxy_guard = EnvGuard::set("no_proxy", "");
    let _upper_no_proxy_guard = EnvGuard::set("NO_PROXY", "");
    crate::gateway::reload_runtime_config_from_env();

    let body = Bytes::from(r#"{"model":"codex","input":"hello"}"#);
    let response = super::send_websocket_upstream_request(
        "ws://example.invalid/v1/responses",
        "acct_ws_proxy",
        Some(Instant::now() + Duration::from_secs(5)),
        &[],
        &body,
    )
    .expect("websocket upstream request connects through proxy");
    let response_body = response
        .read_all_bytes()
        .expect("read proxy websocket body");
    assert_eq!(
        response_body.as_ref(),
        b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n"
    );
    assert_eq!(
        connect_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("proxy CONNECT request"),
        "CONNECT example.invalid:80 HTTP/1.1"
    );
    let headers = headers_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("capture proxied websocket headers");
    assert_eq!(captured_header(&headers, "host"), Some("example.invalid"));
    proxy_handle.join().expect("join websocket proxy");
    target_handle.join().expect("join websocket target");
}

#[test]
fn send_websocket_upstream_request_returns_err_for_invalid_body_before_handshake() {
    let _env_lock = crate::test_env_guard();
    // Invalid bodies must return Err immediately (before handshake) so the
    // caller can fall back to HTTP streaming.
    let invalid_body = Bytes::from_static(br#"{"model":"codex"}"#);
    let result = super::send_websocket_upstream_request(
        "wss://chatgpt.com/backend-api/codex/responses",
        "acct_ws_invalid_body",
        None,
        &[],
        &invalid_body,
    );
    assert!(result.is_err(), "expected Err for invalid body");
    let msg = result.unwrap_err();
    assert!(
        msg.contains("valid WebSocket response.create payload"),
        "error message should mention response.create payload, got: {msg}"
    );
}

#[test]
fn send_websocket_upstream_request_falls_back_on_unreachable_host() {
    let _env_lock = crate::test_env_guard();
    let _reload_guard = RuntimeConfigReloadGuard;
    let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
    crate::gateway::reload_runtime_config_from_env();
    // Attempting to connect to a port that is not listening must return Err quickly
    // (i.e., the handshake_timeout path works and does not block indefinitely).
    // We use a deadline 2 seconds from now so the test completes quickly.
    let deadline = Instant::now() + Duration::from_secs(2);
    let body = Bytes::from(r#"{"model":"codex","input":"hello"}"#);
    let result = super::send_websocket_upstream_request(
        "wss://127.0.0.1:1", // port 1 is not open
        "acct_ws_unreachable",
        Some(deadline),
        &[],
        &body,
    );
    assert!(result.is_err(), "expected Err when host is unreachable");
}
