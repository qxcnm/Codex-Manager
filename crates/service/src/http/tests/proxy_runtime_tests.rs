use super::{
    build_backend_base_url, build_local_backend_client, front_proxy_max_blocking_threads,
    front_proxy_worker_threads, normalize_incoming_request_body, proxy_handler, zstd_body_limit,
    IncomingBodyDecodeError, ProxyState,
};
use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Request as HttpRequest, StatusCode};
use bytes::Bytes;
use codexmanager_core::storage::{
    Account, ApiKey, ManagedModelV2Upsert, ModelFastPolicyV2, RequestLog, RequestTokenStat,
    Storage, Token, UsageSnapshotRecord,
};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio_tungstenite::accept_hdr_async_with_config;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::extensions::compression::deflate::DeflateConfig;
use tokio_tungstenite::tungstenite::extensions::ExtensionsConfig;
use tokio_tungstenite::tungstenite::handshake::server::{Callback, Request, Response};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

static TEST_DB_COUNTER: AtomicU64 = AtomicU64::new(0);
const TEST_ZSTD_MAX_BODY_BYTES: usize = 256 * 1024 * 1024;
const TEST_LARGE_RESPONSES_WS_FRAME_BYTES: usize = 17 * 1024 * 1024;
const TEST_IMAGE_CONTEXT_RESPONSES_WS_FRAME_BYTES: usize = 34 * 1024 * 1024;

fn test_upstream_ws_config() -> WebSocketConfig {
    let mut config = WebSocketConfig::default()
        .max_message_size(Some(
            TEST_IMAGE_CONTEXT_RESPONSES_WS_FRAME_BYTES + 2 * 1024 * 1024,
        ))
        .max_frame_size(Some(
            TEST_IMAGE_CONTEXT_RESPONSES_WS_FRAME_BYTES + 2 * 1024 * 1024,
        ));
    let mut extensions = ExtensionsConfig::default();
    extensions.permessage_deflate = Some(DeflateConfig::default());
    config.extensions = extensions;
    config
}

async fn accept_hdr_async<S, C>(
    stream: S,
    callback: C,
) -> Result<tokio_tungstenite::WebSocketStream<S>, tokio_tungstenite::tungstenite::Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
    C: Callback + Unpin,
{
    accept_hdr_async_with_config(stream, callback, Some(test_upstream_ws_config())).await
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

    fn clear(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
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

fn normalize_body_for_test(
    headers: &mut HeaderMap,
    body: Bytes,
    max_body_bytes: usize,
) -> Result<Bytes, IncomingBodyDecodeError> {
    let decode_limit = zstd_body_limit(max_body_bytes, TEST_ZSTD_MAX_BODY_BYTES);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("zstd test runtime")
        .block_on(normalize_incoming_request_body(
            headers,
            body,
            decode_limit,
            None,
        ))
}

/// 函数 `backend_base_url_uses_http_scheme`
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
fn backend_base_url_uses_http_scheme() {
    assert_eq!(
        build_backend_base_url("127.0.0.1:18080"),
        "http://127.0.0.1:18080"
    );
}

/// 函数 `local_backend_client_builds_without_system_proxy`
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
fn local_backend_client_builds_without_system_proxy() {
    build_local_backend_client().expect("local backend client");
}

#[test]
fn front_proxy_blocking_threads_follow_storage_pool_default() {
    let _guard = crate::test_env_guard();
    let _front_guard = EnvGuard::clear("CODEXMANAGER_FRONT_PROXY_MAX_BLOCKING_THREADS");
    let _storage_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_CONNECTIONS", "7");

    assert_eq!(front_proxy_max_blocking_threads(), 7);
}

#[test]
fn front_proxy_blocking_threads_allow_explicit_override() {
    let _guard = crate::test_env_guard();
    let _storage_guard = EnvGuard::set("CODEXMANAGER_STORAGE_MAX_CONNECTIONS", "7");
    let _front_guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_MAX_BLOCKING_THREADS", "5");

    assert_eq!(front_proxy_max_blocking_threads(), 5);
}

#[test]
fn front_proxy_worker_threads_default_to_small_runtime() {
    let _guard = crate::test_env_guard();
    let _worker_guard = EnvGuard::clear("CODEXMANAGER_FRONT_PROXY_WORKER_THREADS");

    assert_eq!(front_proxy_worker_threads(), 2);
}

#[test]
fn front_proxy_worker_threads_allow_explicit_override() {
    let _guard = crate::test_env_guard();
    let _worker_guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_WORKER_THREADS", "3");

    assert_eq!(front_proxy_worker_threads(), 3);
}

#[test]
fn zstd_request_body_is_decoded_and_transport_headers_are_removed() {
    let plain = br#"{"model":"gpt-5.6-sol","input":"long resume"}"#;
    let compressed =
        zstd::stream::encode_all(std::io::Cursor::new(plain), 3).expect("compress request body");
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("zstd"));
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(compressed.len().to_string().as_str()).expect("content length"),
    );

    let decoded = normalize_body_for_test(
        &mut headers,
        Bytes::from(compressed),
        /*max_body_bytes*/ 0,
    )
    .expect("decode zstd request body");

    assert_eq!(decoded.as_ref(), plain);
    assert!(!headers.contains_key(header::CONTENT_ENCODING));
    assert!(!headers.contains_key(header::CONTENT_LENGTH));
}

#[test]
fn default_front_proxy_limit_decodes_official_image_request_above_64_mib() {
    const LEGACY_ZSTD_LIMIT: usize = 64 * 1024 * 1024;
    let prefix = br#"{"model":"gpt-5.6-sol","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"edit this image"},{"type":"input_image","image_url":"data:image/png;base64,"#;
    let suffix = br#""}]}]}"#;
    let target_len = LEGACY_ZSTD_LIMIT + 1;
    let mut plain = Vec::with_capacity(target_len);
    plain.extend_from_slice(prefix);
    plain.resize(target_len - suffix.len(), b'A');
    plain.extend_from_slice(suffix);
    let compressed =
        zstd::stream::encode_all(std::io::Cursor::new(&plain), 3).expect("compress image request");
    assert!(
        compressed.len() < LEGACY_ZSTD_LIMIT,
        "fixture must reach the decompressed-size path"
    );
    let expected_len = plain.len();
    drop(plain);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("zstd"));

    let decoded = normalize_body_for_test(
        &mut headers,
        Bytes::from(compressed),
        /*max_body_bytes*/ 0,
    )
    .expect("default zstd safety limit must allow official image request");

    assert_eq!(decoded.len(), expected_len);
    assert!(decoded.starts_with(prefix));
    assert!(decoded.ends_with(suffix));
}

#[test]
fn zstd_body_limit_combines_generic_and_decompression_limits() {
    assert_eq!(zstd_body_limit(0, 256), 256);
    assert_eq!(zstd_body_limit(128, 256), 128);
    assert_eq!(zstd_body_limit(512, 256), 256);
    assert_eq!(zstd_body_limit(0, 0), 1);
}

#[test]
fn zero_zstd_safety_limit_falls_back_to_safe_default() {
    let _guard = crate::test_env_guard();
    let _ = crate::gateway::front_proxy_zstd_max_body_bytes();
    let zstd_limit_guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_ZSTD_MAX_BODY_BYTES", "0");
    crate::gateway::reload_runtime_config_from_env();

    assert_eq!(
        crate::gateway::front_proxy_zstd_max_body_bytes(),
        TEST_ZSTD_MAX_BODY_BYTES
    );

    drop(zstd_limit_guard);
    crate::gateway::reload_runtime_config_from_env();
}

#[test]
fn zstd_magic_is_decoded_when_intermediate_proxy_drops_encoding_header() {
    let plain = br#"{"model":"gpt-5.6-sol","stream":true}"#;
    let compressed =
        zstd::stream::encode_all(std::io::Cursor::new(plain), 3).expect("compress request body");
    let mut headers = HeaderMap::new();

    let decoded = normalize_body_for_test(
        &mut headers,
        Bytes::from(compressed),
        /*max_body_bytes*/ 0,
    )
    .expect("decode zstd request body from magic");

    assert_eq!(decoded.as_ref(), plain);
}

#[test]
fn invalid_zstd_request_body_returns_400() {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("zstd"));

    let err = normalize_body_for_test(
        &mut headers,
        Bytes::from_static(b"not-zstd"),
        /*max_body_bytes*/ 0,
    )
    .expect_err("invalid zstd should fail locally");

    assert_eq!(err.status, StatusCode::BAD_REQUEST);
    assert!(err.message.contains("invalid zstd request body"));
}

#[test]
fn decompressed_zstd_request_body_respects_front_proxy_limit() {
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(vec![b'x'; 64]), 3)
        .expect("compress request body");
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("zstd"));

    let err = normalize_body_for_test(
        &mut headers,
        Bytes::from(compressed),
        /*max_body_bytes*/ 32,
    )
    .expect_err("decoded body above limit should fail locally");

    assert_eq!(err.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(err.message.contains("after zstd decompression"));
}

#[test]
fn zero_front_proxy_limit_does_not_reject_large_declared_zstd_body() {
    let _guard = crate::test_env_guard();
    let _ = crate::gateway::front_proxy_max_body_bytes();
    let body_limit_guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES", "0");
    let zstd_limit_guard = EnvGuard::set(
        "CODEXMANAGER_FRONT_PROXY_ZSTD_MAX_BODY_BYTES",
        (128_u64 * 1024 * 1024).to_string().as_str(),
    );
    crate::gateway::reload_runtime_config_from_env();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: build_local_backend_client().expect("client"),
    };
    let oversized = 64_u64 * 1024 * 1024 + 1;
    let request = HttpRequest::builder()
        .method("POST")
        .uri("/v1/responses")
        .header(header::CONTENT_ENCODING, "zstd")
        .header(header::CONTENT_LENGTH, oversized.to_string())
        .body(Body::empty())
        .expect("request");

    let response = runtime.block_on(proxy_handler(State(state), request));
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

    drop(zstd_limit_guard);
    drop(body_limit_guard);
    crate::gateway::reload_runtime_config_from_env();
}

#[test]
fn zstd_safety_limit_rejects_oversized_declared_body_when_generic_limit_is_disabled() {
    let _guard = crate::test_env_guard();
    let _ = crate::gateway::front_proxy_zstd_max_body_bytes();
    let body_limit_guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES", "0");
    let zstd_limit_guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_ZSTD_MAX_BODY_BYTES", "32");
    crate::gateway::reload_runtime_config_from_env();
    assert_eq!(crate::gateway::front_proxy_zstd_max_body_bytes(), 32);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: build_local_backend_client().expect("client"),
    };
    let request = HttpRequest::builder()
        .method("POST")
        .uri("/v1/responses")
        .header(header::CONTENT_ENCODING, "zstd")
        .header(header::CONTENT_LENGTH, "33")
        .body(Body::empty())
        .expect("request");

    let response = runtime.block_on(proxy_handler(State(state), request));
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    drop(zstd_limit_guard);
    drop(body_limit_guard);
    crate::gateway::reload_runtime_config_from_env();
}

/// 函数 `request_without_content_length_over_limit_returns_413`
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
fn request_without_content_length_over_limit_returns_413() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES", "8");
    crate::gateway::reload_runtime_config_from_env();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: build_local_backend_client().expect("client"),
    };
    let request = HttpRequest::builder()
        .method("POST")
        .uri("/rpc")
        .body(Body::from(vec![b'x'; 9]))
        .expect("request");

    let response = runtime.block_on(proxy_handler(State(state), request));
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body = runtime
        .block_on(to_bytes(response.into_body(), usize::MAX))
        .expect("read body");
    let text = String::from_utf8(body.to_vec()).expect("utf8");
    assert_eq!(text, "request body too large: content-length>8");
}

#[test]
fn zero_front_proxy_limit_disables_body_rejection() {
    let _guard = crate::test_env_guard();
    let _guard = EnvGuard::set("CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES", "0");
    crate::gateway::reload_runtime_config_from_env();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let request = HttpRequest::builder()
        .method("POST")
        .uri("/rpc")
        .body(Body::from(vec![b'x'; 64]))
        .expect("request");

    let response = runtime.block_on(proxy_handler(State(state), request));
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}

/// 函数 `backend_send_failure_returns_502`
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
fn backend_send_failure_returns_502() {
    let _ = crate::gateway::front_proxy_max_body_bytes();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let request = HttpRequest::builder()
        .method("GET")
        .uri("/backend-proxy-health")
        .body(Body::empty())
        .expect("request");

    let response = runtime.block_on(proxy_handler(State(state), request));
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let error_code = response
        .headers()
        .get(crate::error_codes::ERROR_CODE_HEADER_NAME)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = runtime
        .block_on(to_bytes(response.into_body(), usize::MAX))
        .expect("read body");
    let text = String::from_utf8(body.to_vec()).expect("utf8");
    let _ = error_code;
    let _ = text;
}

fn new_test_db_path(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix ts")
        .as_nanos();
    let counter = TEST_DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "{prefix}-{}-{counter}-{nonce}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
    path
}

fn init_test_storage(db_path: &PathBuf) -> Storage {
    let storage = Storage::open(db_path).expect("open storage");
    storage.init().expect("init storage");
    storage
}

fn insert_api_key_record(
    storage: &Storage,
    platform_key: &str,
    rotation_strategy: &str,
    upstream_base_url: Option<String>,
) {
    let now = chrono::Utc::now().timestamp();
    storage
        .insert_api_key(&ApiKey {
            id: "gk_proxy_runtime_ws".to_string(),
            name: Some("proxy-runtime-ws".to_string()),
            model_slug: Some("gpt-5.4-mini".to_string()),
            reasoning_effort: Some("high".to_string()),
            service_tier: Some("fast".to_string()),
            rotation_strategy: rotation_strategy.to_string(),
            aggregate_api_id: None,
            aggregate_api_url: None,
            account_plan_filter: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url,
            static_headers_json: None,
            key_hash: crate::storage_helpers::hash_platform_key(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");
}

fn insert_account_and_token(storage: &Storage) {
    let now = chrono::Utc::now().timestamp();
    storage
        .insert_account(&Account {
            id: "acc_proxy_runtime_ws".to_string(),
            label: "proxy-runtime-ws".to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some("chatgpt_proxy_runtime_ws".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc_proxy_runtime_ws".to_string(),
            id_token: "id_token_ws".to_string(),
            access_token: "access_token_ws".to_string(),
            refresh_token: "refresh_token_ws".to_string(),
            api_key_access_token: Some("access_token_ws".to_string()),
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: "acc_proxy_runtime_ws".to_string(),
            used_percent: Some(8.0),
            window_minutes: Some(180),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");
}

fn insert_account_and_token_with_id(
    storage: &Storage,
    account_id: &str,
    label: &str,
    chatgpt_account_id: &str,
    access_token: &str,
    sort: i64,
) {
    let now = chrono::Utc::now().timestamp();
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: label.to_string(),
            issuer: "https://auth.openai.com".to_string(),
            chatgpt_account_id: Some(chatgpt_account_id.to_string()),
            workspace_id: None,
            group_name: None,
            sort,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: format!("id_token_{account_id}"),
            access_token: access_token.to_string(),
            refresh_token: format!("refresh_token_{account_id}"),
            api_key_access_token: Some(access_token.to_string()),
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(8.0),
            window_minutes: Some(180),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");
}

async fn start_front_proxy_test_server(
    state: ProxyState,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let app = super::build_front_proxy_app(state);
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });
        server.await.expect("serve front proxy");
    });
    (addr.to_string(), shutdown_tx, handle)
}

#[derive(Debug)]
struct UpstreamWsCapture {
    path: String,
    headers: HashMap<String, String>,
    frames: Vec<String>,
}

#[derive(Debug)]
struct UpstreamWsCompressionFallbackCapture {
    first_headers: HashMap<String, String>,
    second_headers: HashMap<String, String>,
    frames: Vec<String>,
}

async fn read_raw_websocket_handshake_headers(
    stream: &mut tokio::net::TcpStream,
) -> HashMap<String, String> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 4096];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream
            .read(&mut chunk)
            .await
            .expect("read raw websocket handshake");
        assert!(
            count > 0,
            "upstream handshake ended before headers completed"
        );
        request.extend_from_slice(&chunk[..count]);
        assert!(
            request.len() <= 64 * 1024,
            "upstream websocket handshake exceeded test limit"
        );
    }

    String::from_utf8_lossy(&request)
        .split("\r\n")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

async fn start_mock_upstream_ws_rejects_compression_then_accepts() -> (
    String,
    oneshot::Receiver<UpstreamWsCompressionFallbackCapture>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind compression-fallback mock upstream");
    let addr = listener
        .local_addr()
        .expect("compression-fallback mock upstream addr");
    let (capture_tx, capture_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let (mut first_stream, _) = listener
            .accept()
            .await
            .expect("accept compressed upstream handshake");
        let first_headers = read_raw_websocket_handshake_headers(&mut first_stream).await;
        let body = b"unsupported extension: permessage-deflate";
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        );
        first_stream
            .write_all(response.as_bytes())
            .await
            .expect("send compression rejection");
        first_stream
            .shutdown()
            .await
            .expect("close rejected compressed websocket");

        let (second_stream, _) = listener
            .accept()
            .await
            .expect("accept uncompressed upstream handshake");
        let captured_headers = std::sync::Arc::new(std::sync::Mutex::new(None));
        let captured_headers_clone = captured_headers.clone();
        let mut websocket = accept_hdr_async_with_config(
            second_stream,
            move |request: &Request, response: Response| {
                let headers = request
                    .headers()
                    .iter()
                    .filter_map(|(name, value)| {
                        Some((
                            name.as_str().to_ascii_lowercase(),
                            value.to_str().ok()?.to_string(),
                        ))
                    })
                    .collect::<HashMap<_, _>>();
                let mut guard = captured_headers_clone
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard = Some(headers);
                Ok(response)
            },
            Some(test_upstream_ws_config()),
        )
        .await
        .expect("accept uncompressed websocket handshake");

        let frame = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected response.create after compression fallback, got {other:?}"),
        };
        for payload in [
            serde_json::json!({
                "type": "response.created",
                "response": { "id": "resp_ws_compression_fallback" }
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": { "id": "resp_ws_compression_fallback" }
            }),
        ] {
            websocket
                .send(Message::Text(payload.to_string().into()))
                .await
                .expect("send compression fallback response");
        }

        let second_headers = captured_headers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .expect("capture uncompressed handshake");
        let _ = capture_tx.send(UpstreamWsCompressionFallbackCapture {
            first_headers,
            second_headers,
            frames: vec![frame],
        });
    });
    (addr.to_string(), capture_rx, handle)
}

async fn start_mock_upstream_ws() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    oneshot::Receiver<UpstreamWsCapture>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (capture_tx, capture_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept mock upstream");
        let captured_headers = std::sync::Arc::new(std::sync::Mutex::new(
            None::<(String, HashMap<String, String>)>,
        ));
        let captured_headers_clone = captured_headers.clone();
        let upstream_config = test_upstream_ws_config();
        let mut websocket = accept_hdr_async_with_config(
            stream,
            move |request: &Request, response: Response| {
                let mut headers = HashMap::new();
                for (name, value) in request.headers() {
                    if let Ok(text) = value.to_str() {
                        headers.insert(name.as_str().to_ascii_lowercase(), text.to_string());
                    }
                }
                let mut guard = captured_headers_clone
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard = Some((request.uri().path().to_string(), headers));
                Ok(response)
            },
            Some(upstream_config),
        )
        .await
        .expect("accept websocket handshake");

        let mut frames = Vec::new();
        if let Some(Ok(Message::Text(text))) = websocket.next().await {
            frames.push(text.to_string());
            let _ = event_tx.send(text.to_string());
            websocket
                .send(Message::Text(
                    "{\"type\":\"response.created\",\"response\":{\"id\":\"resp_ws_1\"}}"
                        .to_string()
                        .into(),
                ))
                .await
                .expect("send response.created");
            websocket
                .send(Message::Text(
                    "{\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ws_1\"}}"
                        .to_string()
                        .into(),
                ))
                .await
                .expect("send first response.completed");
        }
        if let Some(Ok(Message::Text(text))) = websocket.next().await {
            frames.push(text.to_string());
            let _ = event_tx.send(text.to_string());
            websocket
                .send(Message::Text(
                    "{\"type\":\"response.completed\",\"response\":{\"id\":\"resp_ws_2\"}}"
                        .to_string()
                        .into(),
                ))
                .await
                .expect("send response.completed");
        }
        let (path, headers) = captured_headers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .expect("captured handshake");
        let _ = capture_tx.send(UpstreamWsCapture {
            path,
            headers,
            frames,
        });
    });
    (addr.to_string(), event_rx, capture_rx, handle)
}

async fn start_mock_upstream_ws_resets_before_first_frame() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind initial-send-reset mock upstream");
    let addr = listener
        .local_addr()
        .expect("initial-send-reset mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept initial-send-reset upstream");
        let websocket = accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
            .await
            .expect("accept initial-send-reset websocket handshake");
        let raw_stream = websocket
            .into_inner()
            .into_std()
            .expect("convert initial-send-reset stream");
        force_tcp_reset(&raw_stream);
        drop(raw_stream);

        let (replacement_stream, _) = listener
            .accept()
            .await
            .expect("accept replacement initial-send-reset upstream");
        let upstream_config = test_upstream_ws_config();
        let mut replacement = accept_hdr_async_with_config(
            replacement_stream,
            |_: &Request, response: Response| Ok(response),
            Some(upstream_config),
        )
        .await
        .expect("accept replacement initial-send-reset websocket handshake");
        let text = match replacement.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => {
                panic!("expected replacement initial-send-reset response.create, got {other:?}")
            }
        };
        event_tx
            .send((1, text))
            .expect("record replacement initial-send-reset request");
        for payload in [
            serde_json::json!({
                "type": "response.created",
                "response": { "id": "resp_ws_initial_send_recovery" }
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": { "id": "resp_ws_initial_send_recovery" }
            }),
        ] {
            replacement
                .send(Message::Text(payload.to_string().into()))
                .await
                .expect("send initial-send-reset recovery response");
        }
        let _ = replacement.next().await;
    });
    (addr.to_string(), event_rx, handle)
}

async fn start_mock_upstream_ws_resets_twice_before_first_frame() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind double-initial-send-reset mock upstream");
    let addr = listener
        .local_addr()
        .expect("double-initial-send-reset mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        let upstream_config = test_upstream_ws_config();

        for round in 1..=2 {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept double initial-send-reset upstream");
            let websocket = accept_hdr_async_with_config(
                stream,
                |_: &Request, response: Response| Ok(response),
                Some(upstream_config),
            )
            .await
            .expect("accept double initial-send-reset websocket handshake");
            let raw_stream = websocket
                .into_inner()
                .into_std()
                .expect("convert double initial-send-reset stream");
            force_tcp_reset(&raw_stream);
            drop(raw_stream);
            event_tx
                .send((round, String::new()))
                .expect("record double initial-send-reset connection");
        }

        let (replacement_stream, _) =
            match tokio::time::timeout(Duration::from_secs(3), listener.accept()).await {
                Ok(result) => result.expect("accept final double initial-send-reset upstream"),
                Err(_) => {
                    event_tx
                        .send((
                            0,
                            "final replacement connection was not attempted".to_string(),
                        ))
                        .expect("record missing final replacement connection");
                    return;
                }
            };
        let mut replacement = accept_hdr_async_with_config(
            replacement_stream,
            |_: &Request, response: Response| Ok(response),
            Some(upstream_config),
        )
        .await
        .expect("accept final double initial-send-reset websocket handshake");
        let text = match replacement.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected final replacement response.create, got {other:?}"),
        };
        event_tx
            .send((3, text))
            .expect("record final double initial-send-reset request");
        for payload in [
            serde_json::json!({
                "type": "response.created",
                "response": { "id": "resp_ws_double_initial_send_recovery" }
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": { "id": "resp_ws_double_initial_send_recovery" }
            }),
        ] {
            replacement
                .send(Message::Text(payload.to_string().into()))
                .await
                .expect("send double initial-send-reset recovery response");
        }
        let _ = replacement.next().await;
    });
    (addr.to_string(), event_rx, handle)
}

async fn start_mock_upstream_ws_switches_after_initial_reset() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(String, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind account-switch initial-send-reset mock upstream");
    let addr = listener
        .local_addr()
        .expect("account-switch initial-send-reset mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        let upstream_config = test_upstream_ws_config();

        for _ in 0..3 {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept account-switch upstream");
            let captured_account_id = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
            let captured_account_id_clone = captured_account_id.clone();
            let mut websocket = accept_hdr_async_with_config(
                stream,
                move |request: &Request, response: Response| {
                    let account_id = request
                        .headers()
                        .get("chatgpt-account-id")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string);
                    let mut guard = captured_account_id_clone
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *guard = account_id;
                    Ok(response)
                },
                Some(upstream_config),
            )
            .await
            .expect("accept account-switch websocket handshake");
            let account_id = captured_account_id
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
                .unwrap_or_else(|| "missing-account-id".to_string());

            if account_id == "workspace-failed" {
                let raw_stream = websocket
                    .into_inner()
                    .into_std()
                    .expect("convert account-switch reset stream");
                force_tcp_reset(&raw_stream);
                drop(raw_stream);
                event_tx
                    .send((account_id, String::new()))
                    .expect("record failed account reset");
                continue;
            }

            let text = match websocket.next().await {
                Some(Ok(Message::Text(text))) => text.to_string(),
                other => panic!(
                    "expected account-switch response.create on replacement account, got {other:?}"
                ),
            };
            event_tx
                .send((account_id, text))
                .expect("record successful replacement account frame");
            for payload in [
                serde_json::json!({
                    "type": "response.created",
                    "response": { "id": "resp_ws_account_switch" }
                }),
                serde_json::json!({
                    "type": "response.completed",
                    "response": { "id": "resp_ws_account_switch" }
                }),
            ] {
                websocket
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .expect("send account-switch replacement response");
            }
            let _ = websocket.next().await;
            return;
        }

        event_tx
            .send((
                "no-replacement-account".to_string(),
                "the failed account was reused for every bounded retry".to_string(),
            ))
            .expect("record missing account switch");
    });
    (addr.to_string(), event_rx, handle)
}

async fn start_mock_upstream_ws_holds_first_response() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind in-flight mock upstream");
    let addr = listener.local_addr().expect("in-flight mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (release_tx, release_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept in-flight mock upstream");
        let mut websocket =
            accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("accept in-flight mock upstream websocket handshake");
        let first = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected first in-flight response.create, got {other:?}"),
        };
        event_tx
            .send(first)
            .expect("record first in-flight response.create");
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.created",
                    "response": { "id": "resp_ws_in_flight" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send in-flight response.created");
        let _ = release_rx.await;
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": { "id": "resp_ws_in_flight" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send in-flight response.completed");
        let second = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected second in-flight response.create, got {other:?}"),
        };
        event_tx
            .send(second)
            .expect("record second in-flight response.create");
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.completed",
                    "response": { "id": "resp_ws_in_flight_follow_up" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send in-flight follow-up response.completed");
    });
    (addr.to_string(), event_rx, release_tx, handle)
}

async fn start_mock_upstream_ws_waits_for_heartbeat() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    oneshot::Receiver<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind heartbeat mock upstream");
    let addr = listener.local_addr().expect("heartbeat mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (heartbeat_tx, heartbeat_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept heartbeat mock upstream");
        let mut websocket =
            accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("accept heartbeat mock upstream websocket handshake");
        let request = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected heartbeat mock response.create, got {other:?}"),
        };
        event_tx
            .send(request)
            .expect("record heartbeat mock response.create");
        for payload in [
            serde_json::json!({
                "type": "response.created",
                "response": { "id": "resp_ws_heartbeat" }
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": { "id": "resp_ws_heartbeat" }
            }),
        ] {
            websocket
                .send(Message::Text(payload.to_string().into()))
                .await
                .expect("send heartbeat mock response event");
        }

        while let Some(result) = websocket.next().await {
            match result {
                Ok(Message::Ping(payload)) => {
                    websocket
                        .send(Message::Pong(payload))
                        .await
                        .expect("send heartbeat mock pong");
                    let _ = heartbeat_tx.send(());
                    break;
                }
                Ok(Message::Pong(_)) | Ok(Message::Text(_)) => {}
                Ok(Message::Close(_)) => break,
                Ok(Message::Binary(_)) | Ok(Message::Frame(_)) => {}
                Err(err) => panic!("heartbeat mock websocket read failed: {err}"),
            }
        }
    });
    (addr.to_string(), event_rx, heartbeat_rx, handle)
}

async fn start_mock_upstream_ws_closes_after_each_response() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reconnecting mock upstream");
    let addr = listener
        .local_addr()
        .expect("reconnecting mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        for round in 0..2 {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept reconnecting mock upstream");
            let mut websocket =
                accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                    .await
                    .expect("accept reconnecting websocket handshake");
            let text = match websocket.next().await {
                Some(Ok(Message::Text(text))) => text.to_string(),
                other => panic!("expected response.create for round {round}, got {other:?}"),
            };
            event_tx
                .send((round, text))
                .expect("record reconnecting upstream frame");
            for payload in [
                serde_json::json!({
                    "type": "response.created",
                    "response": { "id": format!("resp_ws_reconnect_{round}") }
                }),
                serde_json::json!({
                    "type": "response.completed",
                    "response": { "id": format!("resp_ws_reconnect_{round}") }
                }),
            ] {
                websocket
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .expect("send reconnecting upstream response");
            }
            websocket
                .close(None)
                .await
                .expect("close upstream after terminal response");
        }
    });
    (addr.to_string(), event_rx, handle)
}

async fn start_mock_upstream_ws_resets_after_preamble() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reset-after-preamble mock upstream");
    let addr = listener
        .local_addr()
        .expect("reset-after-preamble mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept initial reset-after-preamble upstream");
        let mut websocket =
            accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("accept initial reset-after-preamble websocket handshake");
        let initial_text = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected initial reset-after-preamble frame, got {other:?}"),
        };
        event_tx
            .send((0, initial_text))
            .expect("record initial reset-after-preamble frame");
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.created",
                    "response": { "id": "resp_ws_reset_initial" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send initial reset-after-preamble response.created");

        let raw_stream = websocket
            .into_inner()
            .into_std()
            .expect("convert reset-after-preamble stream");
        force_tcp_reset(&raw_stream);
        drop(raw_stream);

        let (replacement_stream, _) = listener
            .accept()
            .await
            .expect("accept replacement reset-after-preamble upstream");
        let mut replacement =
            accept_hdr_async(replacement_stream, |_: &Request, response: Response| {
                Ok(response)
            })
            .await
            .expect("accept replacement reset-after-preamble websocket handshake");

        for round in 1..=2 {
            let text = match replacement.next().await {
                Some(Ok(Message::Text(text))) => text.to_string(),
                other => panic!("expected replay/follow-up frame for round {round}, got {other:?}"),
            };
            event_tx
                .send((round, text))
                .expect("record replay/follow-up reset-after-preamble frame");
            for payload in [
                serde_json::json!({
                    "type": "response.created",
                    "response": { "id": format!("resp_ws_reset_{round}") }
                }),
                serde_json::json!({
                    "type": "response.completed",
                    "response": { "id": format!("resp_ws_reset_{round}") }
                }),
            ] {
                replacement
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .expect("send reset-after-preamble response");
            }
        }
        let _ = replacement.next().await;
    });
    (addr.to_string(), event_rx, handle)
}

async fn start_mock_upstream_ws_resets_after_preamble_twice() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind repeated reset-after-preamble mock upstream");
    let addr = listener
        .local_addr()
        .expect("repeated reset-after-preamble mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        for round in 0..=2 {
            let (stream, _) = listener
                .accept()
                .await
                .expect("accept repeated reset-after-preamble upstream");
            let mut websocket =
                accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                    .await
                    .expect("accept repeated reset-after-preamble websocket handshake");
            let text = match websocket.next().await {
                Some(Ok(Message::Text(text))) => text.to_string(),
                other => panic!(
                    "expected repeated reset-after-preamble response.create for round {round}, got {other:?}"
                ),
            };
            event_tx
                .send((round, text))
                .expect("record repeated reset-after-preamble frame");
            websocket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "response.created",
                        "response": { "id": format!("resp_ws_repeated_reset_{round}") }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("send repeated reset-after-preamble response.created");

            if round < 2 {
                let raw_stream = websocket
                    .into_inner()
                    .into_std()
                    .expect("convert repeated reset-after-preamble stream");
                force_tcp_reset(&raw_stream);
                drop(raw_stream);
                continue;
            }

            websocket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "response.completed",
                        "response": { "id": "resp_ws_repeated_reset_completed" }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .expect("send repeated reset-after-preamble response.completed");
            let _ = websocket.next().await;
        }
    });
    (addr.to_string(), event_rx, handle)
}

async fn start_mock_upstream_ws_connection_limit_then_success() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind connection-limit mock upstream");
    let addr = listener
        .local_addr()
        .expect("connection-limit mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept initial connection-limit upstream");
        let mut websocket =
            accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("accept initial connection-limit websocket handshake");
        let initial_text = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected initial connection-limit response.create, got {other:?}"),
        };
        event_tx
            .send((0, initial_text))
            .expect("record initial connection-limit request");
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.created",
                    "response": { "id": "resp_ws_connection_limit_initial" }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send connection-limit response.created");
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "error",
                    "status": 400,
                    "error": {
                        "code": "websocket_connection_limit_reached",
                        "message": "Responses websocket connection limit reached (60 minutes). Create a new websocket connection to continue."
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send connection-limit error");
        drop(websocket);

        let (replacement_stream, _) = listener
            .accept()
            .await
            .expect("accept replacement connection-limit upstream");
        let mut replacement =
            accept_hdr_async(replacement_stream, |_: &Request, response: Response| {
                Ok(response)
            })
            .await
            .expect("accept replacement connection-limit websocket handshake");
        for round in 1..=2 {
            let text = match replacement.next().await {
                Some(Ok(Message::Text(text))) => text.to_string(),
                other => panic!(
                    "expected replay/follow-up connection-limit frame for round {round}, got {other:?}"
                ),
            };
            event_tx
                .send((round, text))
                .expect("record replacement connection-limit request");
            for payload in [
                serde_json::json!({
                    "type": "response.created",
                    "response": { "id": format!("resp_ws_connection_limit_{round}") }
                }),
                serde_json::json!({
                    "type": "response.completed",
                    "response": { "id": format!("resp_ws_connection_limit_{round}") }
                }),
            ] {
                replacement
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .expect("send replacement connection-limit response");
            }
        }
        let _ = replacement.next().await;
    });
    (addr.to_string(), event_rx, handle)
}

fn force_tcp_reset(stream: &std::net::TcpStream) {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;

        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 0,
        };
        let result = unsafe {
            libc::setsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                (&linger as *const libc::linger).cast(),
                std::mem::size_of::<libc::linger>() as libc::socklen_t,
            )
        };
        assert_eq!(result, 0, "set reset-after-preamble linger");
    }

    #[cfg(not(unix))]
    {
        let _ = stream;
    }
}

async fn start_mock_upstream_ws_resets_after_output() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reset-after-output mock upstream");
    let addr = listener
        .local_addr()
        .expect("reset-after-output mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (reset_tx, reset_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept reset-after-output upstream");
        let mut websocket =
            accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("accept reset-after-output websocket handshake");
        let initial_text = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected reset-after-output frame, got {other:?}"),
        };
        event_tx
            .send(initial_text)
            .expect("record reset-after-output frame");
        for payload in [
            serde_json::json!({
                "type": "response.created",
                "response": { "id": "resp_ws_reset_after_output" }
            }),
            serde_json::json!({
                "type": "response.output_text.delta",
                "delta": "partial output"
            }),
        ] {
            websocket
                .send(Message::Text(payload.to_string().into()))
                .await
                .expect("send reset-after-output event");
        }
        let _ = reset_rx.await;
        let raw_stream = websocket
            .into_inner()
            .into_std()
            .expect("convert reset-after-output stream");
        force_tcp_reset(&raw_stream);
        drop(raw_stream);
    });
    (addr.to_string(), event_rx, reset_tx, handle)
}

async fn start_mock_upstream_ws_closes_after_accepting_follow_up() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<(usize, String)>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stale-follow-up mock upstream");
    let addr = listener
        .local_addr()
        .expect("stale-follow-up mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let handle = tokio::spawn(async move {
        let (stream, _) = listener
            .accept()
            .await
            .expect("accept initial stale-follow-up upstream");
        let mut websocket =
            accept_hdr_async(stream, |_: &Request, response: Response| Ok(response))
                .await
                .expect("accept initial stale-follow-up websocket handshake");

        let first_text = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected initial response.create, got {other:?}"),
        };
        event_tx
            .send((0, first_text))
            .expect("record initial upstream frame");
        for payload in [
            serde_json::json!({
                "type": "response.created",
                "response": { "id": "resp_ws_stale_follow_up_0" }
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_ws_stale_follow_up_0",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": "historical seed answer"
                        }]
                    }]
                }
            }),
        ] {
            websocket
                .send(Message::Text(payload.to_string().into()))
                .await
                .expect("send initial stale-follow-up response");
        }

        let stale_follow_up = match websocket.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected follow-up on stale upstream, got {other:?}"),
        };
        event_tx
            .send((1, stale_follow_up))
            .expect("record follow-up accepted by stale upstream");
        websocket
            .close(None)
            .await
            .expect("close stale upstream after accepting follow-up");

        let (replacement_stream, _) = listener
            .accept()
            .await
            .expect("accept replacement stale-follow-up upstream");
        let mut replacement =
            accept_hdr_async(replacement_stream, |_: &Request, response: Response| {
                Ok(response)
            })
            .await
            .expect("accept replacement stale-follow-up websocket handshake");
        let resent_follow_up = match replacement.next().await {
            Some(Ok(Message::Text(text))) => text.to_string(),
            other => panic!("expected resent follow-up on replacement upstream, got {other:?}"),
        };
        event_tx
            .send((2, resent_follow_up))
            .expect("record resent follow-up upstream frame");
        for payload in [
            serde_json::json!({
                "type": "response.created",
                "response": { "id": "resp_ws_stale_follow_up_1" }
            }),
            serde_json::json!({
                "type": "response.completed",
                "response": { "id": "resp_ws_stale_follow_up_1", "output": [] }
            }),
        ] {
            replacement
                .send(Message::Text(payload.to_string().into()))
                .await
                .expect("send replacement stale-follow-up response");
        }
    });
    (addr.to_string(), event_rx, handle)
}

async fn start_mock_upstream_ws_usage_limit_then_success() -> (
    String,
    tokio::sync::mpsc::UnboundedReceiver<String>,
    oneshot::Receiver<Vec<UpstreamWsCapture>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (capture_tx, capture_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let mut captures = Vec::new();

        for round in 0..2 {
            let (stream, _) = listener.accept().await.expect("accept mock upstream");
            let captured_headers = std::sync::Arc::new(std::sync::Mutex::new(
                None::<(String, HashMap<String, String>)>,
            ));
            let captured_headers_clone = captured_headers.clone();
            let mut websocket =
                accept_hdr_async(stream, move |request: &Request, response: Response| {
                    let mut headers = HashMap::new();
                    for (name, value) in request.headers() {
                        if let Ok(text) = value.to_str() {
                            headers.insert(name.as_str().to_ascii_lowercase(), text.to_string());
                        }
                    }
                    let mut guard = captured_headers_clone
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *guard = Some((request.uri().path().to_string(), headers));
                    Ok(response)
                })
                .await
                .expect("accept websocket handshake");

            let mut frames = Vec::new();
            if round == 0 {
                if let Some(Ok(Message::Text(text))) = websocket.next().await {
                    frames.push(text.to_string());
                    let _ = event_tx.send(text.to_string());
                    for response_payload in [
                        serde_json::json!({
                            "type": "response.created",
                            "response": { "id": "resp_ws_tool_seed" }
                        }),
                        serde_json::json!({
                            "type": "response.output_item.done",
                            "response_id": "resp_ws_tool_seed",
                            "output_index": 0,
                            "item": {
                                "type": "custom_tool_call",
                                "id": "ctc_ws_tool_seed",
                                "call_id": "call_ws_tool_rebase",
                                "name": "apply_patch",
                                "input": "*** Begin Patch",
                                "status": "completed"
                            }
                        }),
                        serde_json::json!({
                            "type": "response.completed",
                            "response": {
                                "id": "resp_ws_tool_seed",
                                "status": "completed",
                                "output": [{
                                    "type": "custom_tool_call",
                                    "id": "ctc_ws_tool_seed",
                                    "call_id": "call_ws_tool_rebase",
                                    "name": "apply_patch",
                                    "input": "*** Begin Patch",
                                    "status": "completed"
                                }]
                            }
                        }),
                    ] {
                        websocket
                            .send(Message::Text(response_payload.to_string().into()))
                            .await
                            .expect("send upstream tool-call response");
                    }
                }

                if let Some(Ok(Message::Text(text))) = websocket.next().await {
                    frames.push(text.to_string());
                    let _ = event_tx.send(text.to_string());
                    websocket
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "response.created",
                                "response": { "id": "resp_ws_limited_a" }
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .expect("send limited account response.created");
                    tokio::time::sleep(Duration::from_millis(120)).await;
                    websocket
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "response.failed",
                                "response": {
                                    "id": "resp_ws_limited_a",
                                    "status": "failed",
                                    "error": {
                                        "code": "usage_limit_reached",
                                        "message": "The usage limit has been reached"
                                    }
                                }
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .expect("send limited account response.failed");
                }
            } else if let Some(Ok(Message::Text(text))) = websocket.next().await {
                frames.push(text.to_string());
                let _ = event_tx.send(text.to_string());
                for response_payload in [
                    serde_json::json!({
                        "type": "response.created",
                        "response": { "id": "resp_ws_failover_ok" }
                    }),
                    serde_json::json!({
                        "type": "response.completed",
                        "response": { "id": "resp_ws_failover_ok" }
                    }),
                ] {
                    websocket
                        .send(Message::Text(response_payload.to_string().into()))
                        .await
                        .expect("send successful failover response");
                }
            }
            let (path, headers) = captured_headers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
                .expect("captured handshake");
            captures.push(UpstreamWsCapture {
                path,
                headers,
                frames,
            });
        }

        let _ = capture_tx.send(captures);
    });
    (addr.to_string(), event_rx, capture_rx, handle)
}

fn build_ws_request(
    url: &str,
    platform_key: &str,
    extra_headers: &[(&str, &str)],
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut request = url.into_client_request().expect("build ws request");
    request.headers_mut().insert(
        axum::http::header::AUTHORIZATION,
        axum::http::HeaderValue::from_str(&format!("Bearer {platform_key}"))
            .expect("authorization header"),
    );
    for (name, value) in extra_headers {
        request.headers_mut().insert(
            axum::http::header::HeaderName::from_bytes(name.as_bytes()).expect("header name"),
            axum::http::HeaderValue::from_str(value).expect("header value"),
        );
    }
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_responses_websocket_returns_426() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-unsupported");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    insert_api_key_record(
        &storage,
        "platform_key_ws_unsupported",
        crate::apikey_profile::ROTATION_AGGREGATE_API,
        None,
    );
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_unsupported",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );

    let err = connect_async(request)
        .await
        .expect_err("websocket should fail");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
        }
        other => panic!("unexpected websocket error: {other}"),
    }

    let _ = shutdown_tx.send(());
    server_handle.await.expect("join front proxy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hybrid_responses_websocket_returns_426() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-hybrid-unsupported");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    insert_api_key_record(
        &storage,
        "platform_key_ws_hybrid_unsupported",
        crate::apikey_profile::ROTATION_HYBRID,
        None,
    );
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_hybrid_unsupported",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );

    let err = connect_async(request)
        .await
        .expect_err("websocket should fail");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
        }
        other => panic!("unexpected websocket error: {other}"),
    }

    let _ = shutdown_tx.send(());
    server_handle.await.expect("join front proxy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_responses_websocket_key_returns_403() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-disabled-key");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    insert_api_key_record(
        &storage,
        "platform_key_ws_disabled",
        crate::apikey_profile::ROTATION_ACCOUNT,
        None,
    );
    storage
        .update_api_key_status("gk_proxy_runtime_ws", "disabled")
        .expect("disable websocket api key");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_disabled",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );

    let err = connect_async(request)
        .await
        .expect_err("disabled websocket key should fail");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        other => panic!("unexpected websocket error: {other}"),
    }

    let _ = shutdown_tx.send(());
    server_handle.await.expect("join front proxy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exhausted_responses_websocket_key_quota_returns_429() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-quota-exhausted");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    insert_api_key_record(
        &storage,
        "platform_key_ws_quota_exhausted",
        crate::apikey_profile::ROTATION_ACCOUNT,
        None,
    );
    storage
        .upsert_api_key_quota_limit("gk_proxy_runtime_ws", Some(10))
        .expect("set websocket api key quota");
    storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                key_id: Some("gk_proxy_runtime_ws".to_string()),
                request_path: "/v1/responses".to_string(),
                method: "POST".to_string(),
                status_code: Some(200),
                created_at: chrono::Utc::now().timestamp(),
                ..Default::default()
            },
            &RequestTokenStat {
                key_id: Some("gk_proxy_runtime_ws".to_string()),
                total_tokens: Some(10),
                created_at: chrono::Utc::now().timestamp(),
                ..Default::default()
            },
        )
        .expect("insert exhausted websocket api key usage");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_quota_exhausted",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );

    let err = connect_async(request)
        .await
        .expect_err("exhausted websocket key should fail");
    match err {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }
        other => panic!("unexpected websocket error: {other}"),
    }

    let _ = shutdown_tx.send(());
    server_handle.await.expect("join front proxy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn responses_websocket_rechecks_quota_for_follow_up_request() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-follow-up-quota");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, _capture_rx, upstream_handle) =
        start_mock_upstream_ws().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_follow_up_quota",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    storage
        .upsert_api_key_quota_limit("gk_proxy_runtime_ws", Some(10))
        .expect("set websocket api key quota");
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_follow_up_quota",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "first request"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send first response.create");
    tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("first upstream frame timeout")
        .expect("first upstream frame channel");
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("first client event timeout")
            .expect("first client event")
            .expect("first client event result");
        if matches!(event, Message::Text(ref text) if text.contains("\"response.completed\"")) {
            break;
        }
    }

    storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                key_id: Some("gk_proxy_runtime_ws".to_string()),
                request_path: "/v1/responses".to_string(),
                method: "POST".to_string(),
                status_code: Some(200),
                created_at: chrono::Utc::now().timestamp(),
                ..Default::default()
            },
            &RequestTokenStat {
                key_id: Some("gk_proxy_runtime_ws".to_string()),
                total_tokens: Some(10),
                created_at: chrono::Utc::now().timestamp(),
                ..Default::default()
            },
        )
        .expect("exhaust websocket quota after first request");

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "must be rejected"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send follow-up response.create");
    let error = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("quota error timeout")
        .expect("quota error event")
        .expect("quota error result");
    match error {
        Message::Text(text) => {
            let payload: serde_json::Value =
                serde_json::from_str(&text).expect("parse quota error event");
            assert_eq!(payload["status"], 429);
        }
        other => panic!("unexpected quota error event: {other:?}"),
    }
    let unexpected_upstream =
        tokio::time::timeout(Duration::from_millis(200), upstream_events.recv()).await;
    assert!(
        !matches!(unexpected_upstream, Ok(Some(_))),
        "quota-exhausted follow-up must not reach upstream"
    );

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    server_handle.await.expect("join front proxy");
    upstream_handle.await.expect("join mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_proxies_frames_and_headers() {
    let _guard = crate::test_env_guard();
    let _org_guard = EnvGuard::set("OPENAI_ORGANIZATION", "org_ws_test");
    let _project_guard = EnvGuard::set("OPENAI_PROJECT", "proj_ws_test");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-supported");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, capture_rx, upstream_handle) =
        start_mock_upstream_ws().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_supported",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_supported",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_1"),
            ("x-codex-window-id", "session_ws_1:7"),
            ("x-client-request-id", "client_req_ws_1"),
            ("x-openai-subagent", "review"),
            ("x-codex-parent-thread-id", "thread_parent_ws_1"),
            ("x-codex-other-limit-name", "promo_header_ws"),
            ("x-codex-turn-state", "turn_state_ws_1"),
            ("x-codex-turn-metadata", "turn_meta_ws_1"),
            ("x-responsesapi-include-timing-metrics", "true"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "hello",
                "stream": false,
                "store": true,
                "service_tier": "Fast",
                "generate": false
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send first frame");

    let first_upstream_frame = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("first upstream frame timeout")
        .expect("first upstream frame channel");
    let first_payload: serde_json::Value =
        serde_json::from_str(&first_upstream_frame).expect("parse first upstream frame");
    assert_eq!(first_payload["type"], "response.create");
    assert_eq!(first_payload["model"], "gpt-5.4-mini");
    assert!(first_payload.get("stream").is_none());
    assert!(first_payload.get("background").is_none());
    assert_eq!(first_payload["store"], true);
    assert_eq!(first_payload["service_tier"], "priority");
    assert_eq!(first_payload["generate"], false);
    assert!(first_payload.get("prompt_cache_key").is_none());

    let first_client_event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("first client event timeout")
        .expect("first client event");
    let first_client_event = first_client_event.expect("first client event result");
    match first_client_event {
        Message::Text(text) => {
            assert!(
                text.contains("\"response.created\""),
                "unexpected event: {text}"
            );
        }
        other => panic!("unexpected first client event: {other:?}"),
    }
    let first_completed_event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("first completed event timeout")
        .expect("first completed event")
        .expect("first completed event result");
    match first_completed_event {
        Message::Text(text) => {
            assert!(
                text.contains("\"response.completed\""),
                "unexpected first completed event: {text}"
            );
        }
        other => panic!("unexpected first completed event: {other:?}"),
    }

    let mut model = storage
        .get_managed_model_v2("gpt-5.4-mini")
        .expect("read websocket model")
        .expect("websocket model");
    model.fast_policy = ModelFastPolicyV2::Filter;
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some("gpt-5.4-mini".to_string()),
            model,
        })
        .expect("update websocket model fast policy");

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "previous_response_id": "resp_prev_ws_1",
                "input": "follow up",
                "client_metadata": {
                    "source": "proxy-runtime-test"
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send second frame");

    let second_upstream_frame =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("second upstream frame timeout")
            .expect("second upstream frame channel");
    let second_payload: serde_json::Value =
        serde_json::from_str(&second_upstream_frame).expect("parse second upstream frame");
    assert_eq!(second_payload["type"], "response.create");
    assert_eq!(second_payload["previous_response_id"], "resp_prev_ws_1");
    assert_eq!(
        second_payload["client_metadata"]["source"],
        "proxy-runtime-test"
    );
    assert_eq!(
        second_payload["client_metadata"]["x-codex-turn-metadata"],
        "turn_meta_ws_1"
    );
    assert!(second_payload.get("service_tier").is_none());
    assert!(second_payload.get("prompt_cache_key").is_none());

    let second_client_event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("second client event timeout")
        .expect("second client event");
    let second_client_event = second_client_event.expect("second client event result");
    match second_client_event {
        Message::Text(text) => {
            assert!(
                text.contains("\"response.completed\""),
                "unexpected event: {text}"
            );
        }
        other => panic!("unexpected second client event: {other:?}"),
    }

    let capture = tokio::time::timeout(Duration::from_secs(2), capture_rx)
        .await
        .expect("capture timeout")
        .expect("capture result");
    assert_eq!(capture.path, "/chatgpt.com/backend-api/codex/responses");
    assert_eq!(
        capture.headers.get("authorization").map(String::as_str),
        Some("Bearer access_token_ws")
    );
    assert_eq!(
        capture
            .headers
            .get("chatgpt-account-id")
            .map(String::as_str),
        Some("chatgpt_proxy_runtime_ws")
    );
    assert_eq!(
        capture.headers.get("openai-beta").map(String::as_str),
        Some("responses_websockets=2026-02-06")
    );
    assert!(
        capture
            .headers
            .get("sec-websocket-extensions")
            .is_some_and(|value| value.contains("permessage-deflate")),
        "official-compatible upstream websocket transport must offer permessage-deflate"
    );
    assert_eq!(capture.headers.get("version").map(String::as_str), None);
    assert_eq!(
        capture
            .headers
            .get("openai-organization")
            .map(String::as_str),
        None
    );
    assert_eq!(
        capture.headers.get("openai-project").map(String::as_str),
        None
    );
    assert_eq!(
        capture.headers.get("session_id").map(String::as_str),
        Some("session_ws_1")
    );
    assert_eq!(
        capture.headers.get("x-codex-window-id").map(String::as_str),
        Some("session_ws_1:7")
    );
    assert_eq!(
        capture
            .headers
            .get("x-client-request-id")
            .map(String::as_str),
        Some("client_req_ws_1")
    );
    assert_eq!(
        capture.headers.get("x-openai-subagent").map(String::as_str),
        Some("review")
    );
    assert_eq!(
        capture
            .headers
            .get("x-codex-parent-thread-id")
            .map(String::as_str),
        Some("thread_parent_ws_1")
    );
    assert_eq!(
        capture
            .headers
            .get("x-codex-other-limit-name")
            .map(String::as_str),
        None
    );
    assert_eq!(
        capture
            .headers
            .get("x-codex-turn-state")
            .map(String::as_str),
        Some("turn_state_ws_1")
    );
    assert_eq!(
        capture
            .headers
            .get("x-codex-turn-metadata")
            .map(String::as_str),
        Some("turn_meta_ws_1")
    );
    assert_eq!(
        capture
            .headers
            .get("x-responsesapi-include-timing-metrics")
            .map(String::as_str),
        Some("true")
    );
    assert_eq!(capture.frames.len(), 2);

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list request logs");
    let ws_logs: Vec<_> = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect();
    assert_eq!(
        ws_logs.len(),
        2,
        "expected two websocket request log entries"
    );
    assert!(
        ws_logs
            .iter()
            .any(|item| item.service_tier.as_deref() == Some("fast")),
        "expected websocket request log to keep explicit fast service tier"
    );
    assert!(
        ws_logs
            .iter()
            .any(|item| item.effective_service_tier.as_deref() == Some("fast")),
        "expected websocket request log to persist effective fast service tier"
    );
    assert!(
        ws_logs.iter().any(|item| item.service_tier.is_none()),
        "expected follow-up websocket request without explicit service tier to stay empty"
    );
    assert!(
        ws_logs
            .iter()
            .filter(|item| item.service_tier.is_none())
            .any(|item| {
                item.effective_service_tier.is_none()
                    && item.service_tier_source.as_deref() == Some("model_policy")
            }),
        "expected follow-up websocket request to apply the model filter policy"
    );

    client_ws.close(None).await.expect("close client websocket");
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy shutdown timeout")
        .expect("join front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock upstream shutdown timeout")
        .expect("join mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_retries_without_compression_after_upstream_rejection() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-compression-fallback");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, capture_rx, upstream_handle) =
        start_mock_upstream_ws_rejects_compression_then_accepts().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_compression_fallback",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_compression_fallback",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-5.6-sol",
                "input": "retry without compression after an upstream rejection"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send compression fallback response.create");

    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("compression fallback client event timeout")
            .expect("compression fallback client event")
            .expect("compression fallback client event result");
        match event {
            Message::Text(text) if text.contains("\"response.completed\"") => break,
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("compression fallback error escaped to client: {text}");
            }
            Message::Text(_) => {}
            other => panic!("unexpected compression fallback event: {other:?}"),
        }
    }

    let capture = tokio::time::timeout(Duration::from_secs(5), capture_rx)
        .await
        .expect("compression fallback capture timeout")
        .expect("compression fallback capture result");
    assert!(
        capture
            .first_headers
            .get("sec-websocket-extensions")
            .is_some_and(|value| value.contains("permessage-deflate")),
        "the first upstream handshake must offer the official compression extension"
    );
    assert_eq!(
        capture.second_headers.get("sec-websocket-extensions"),
        None,
        "a compression rejection must retry with an uncompressed handshake"
    );
    assert_eq!(capture.frames.len(), 1);
    assert!(capture.frames[0].contains("retry without compression"));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy compression fallback shutdown timeout")
        .expect("join compression fallback front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("compression fallback upstream shutdown timeout")
        .expect("join compression fallback mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn official_responses_websocket_block_policy_rejects_initial_frame() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-block-initial");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    insert_api_key_record(
        &storage,
        "platform_key_ws_block_initial",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some("http://127.0.0.1:1/chatgpt.com/backend-api/codex".to_string()),
    );
    let mut model = storage
        .get_managed_model_v2("gpt-5.4-mini")
        .expect("read websocket block model")
        .expect("websocket block model");
    model.fast_policy = ModelFastPolicyV2::Block;
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some("gpt-5.4-mini".to_string()),
            model,
        })
        .expect("update websocket initial block policy");
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_block_initial",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-5.4-mini",
                "input": "blocked initial request",
                "service_tier": "fast"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send blocked initial frame");
    let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("blocked initial event timeout")
        .expect("blocked initial event")
        .expect("blocked initial event result");
    let Message::Text(text) = event else {
        panic!("expected blocked initial websocket error text frame");
    };
    let payload: serde_json::Value =
        serde_json::from_str(text.as_ref()).expect("parse blocked initial event");
    assert_eq!(payload["type"], "error");
    assert_eq!(payload["status"], 400);
    assert_eq!(payload["error"]["code"], "fast_request_blocked");

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list blocked initial request logs");
    let blocked_logs = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect::<Vec<_>>();
    assert_eq!(blocked_logs.len(), 1);
    assert_eq!(blocked_logs[0].status_code, Some(400));
    assert!(blocked_logs[0]
        .error
        .as_deref()
        .is_some_and(|error| error.contains("does not allow Fast requests")));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    server_handle.await.expect("join front proxy");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_rejects_overlapping_response_create() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-in-flight");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, release_tx, upstream_handle) =
        start_mock_upstream_ws_holds_first_response().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_in_flight",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_in_flight",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    let first_request = serde_json::json!({
        "type": "response.create",
        "model": "gpt-5.6-sol",
        "input": "first in-flight request"
    });
    client_ws
        .send(Message::Text(first_request.to_string().into()))
        .await
        .expect("send first in-flight request");
    let first_upstream = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("first in-flight upstream frame timeout")
        .expect("first in-flight upstream frame");
    assert!(first_upstream.contains("first in-flight request"));

    let first_created = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("first in-flight response.created timeout")
        .expect("first in-flight response.created")
        .expect("first in-flight response.created result");
    assert!(matches!(first_created, Message::Text(text) if text.contains("response.created")));

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-5.6-sol",
                "input": "overlapping request"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send overlapping request");
    let overlap_error = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("overlapping request error timeout")
        .expect("overlapping request error")
        .expect("overlapping request error result");
    match overlap_error {
        Message::Text(text) => {
            let value: serde_json::Value =
                serde_json::from_str(text.as_ref()).expect("parse overlapping request error");
            assert_eq!(value["type"], "error");
            assert_eq!(value["error"]["code"], "response_in_flight");
        }
        other => panic!("unexpected overlapping request result: {other:?}"),
    }
    assert!(
        upstream_events.try_recv().is_err(),
        "overlapping response.create must not reach the upstream"
    );

    client_ws
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .expect("send unsupported binary request");
    let binary_error = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("binary request error timeout")
        .expect("binary request error")
        .expect("binary request error result");
    assert!(
        matches!(binary_error, Message::Text(text) if text.contains("response.create text frames only"))
    );

    release_tx
        .send(())
        .expect("release first in-flight response");
    let first_completed = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("first in-flight response.completed timeout")
        .expect("first in-flight response.completed")
        .expect("first in-flight response.completed result");
    assert!(matches!(first_completed, Message::Text(text) if text.contains("response.completed")));

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-5.6-sol",
                "input": "follow-up after completion"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send follow-up after completion");
    let second_upstream = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("follow-up upstream frame timeout")
        .expect("follow-up upstream frame");
    assert!(second_upstream.contains("follow-up after completion"));
    let second_completed = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("follow-up response.completed timeout")
        .expect("follow-up response.completed")
        .expect("follow-up response.completed result");
    assert!(matches!(second_completed, Message::Text(text) if text.contains("response.completed")));

    client_ws.close(None).await.expect("close client websocket");
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy in-flight shutdown timeout")
        .expect("join front proxy in-flight");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock upstream in-flight shutdown timeout")
        .expect("join mock upstream in-flight");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_accepts_large_image_context_frame() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-large-image-context");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, _capture_rx, upstream_handle) =
        start_mock_upstream_ws().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_large_image_context",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_large_image_context",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    let image_data = "A".repeat(TEST_IMAGE_CONTEXT_RESPONSES_WS_FRAME_BYTES);
    let payload = serde_json::json!({
        "type": "response.create",
        "model": "gpt-5.6-sol",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "continue with the existing image context" },
                {
                    "type": "input_image",
                    "image_url": format!("data:image/png;base64,{image_data}")
                }
            ]
        }]
    })
    .to_string();
    assert!(payload.len() > TEST_IMAGE_CONTEXT_RESPONSES_WS_FRAME_BYTES);

    client_ws
        .send(Message::Text(payload.into()))
        .await
        .expect("send large image context frame");

    let forwarded = tokio::time::timeout(Duration::from_secs(10), upstream_events.recv())
        .await
        .expect("large image context frame timeout")
        .expect("large image context frame channel");
    assert!(forwarded.contains("data:image/png;base64,"));
    assert!(forwarded.len() > 16 * 1024 * 1024);

    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), client_ws.next())
            .await
            .expect("large image response event timeout")
            .expect("large image response event")
            .expect("large image response event result");
        if matches!(event, Message::Text(ref text) if text.contains("\"response.completed\"")) {
            break;
        }
    }

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "previous_response_id": "resp_ws_1",
                "input": "follow up"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send follow-up frame");
    let _ = tokio::time::timeout(Duration::from_secs(10), upstream_events.recv())
        .await
        .expect("follow-up frame timeout")
        .expect("follow-up frame channel");

    client_ws.close(None).await.expect("close client websocket");
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), server_handle)
        .await
        .expect("front proxy shutdown timeout")
        .expect("join front proxy");
    tokio::time::timeout(Duration::from_secs(10), upstream_handle)
        .await
        .expect("mock upstream shutdown timeout")
        .expect("join mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_recovers_after_initial_upstream_send_failure() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-initial-send-recovery");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_resets_before_first_frame().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_initial_send_recovery",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_initial_send_recovery",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    let image_data = "A".repeat(TEST_LARGE_RESPONSES_WS_FRAME_BYTES);
    let payload = serde_json::json!({
        "type": "response.create",
        "model": "gpt-5.6-sol",
        "store": true,
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "continue after reconnecting the image-heavy thread" },
                {
                    "type": "input_image",
                    "image_url": format!("data:image/png;base64,{image_data}")
                }
            ]
        }]
    })
    .to_string();
    assert!(payload.len() > 16 * 1024 * 1024);
    client_ws
        .send(Message::Text(payload.into()))
        .await
        .expect("send initial image-heavy response.create");

    let (round, forwarded) = tokio::time::timeout(Duration::from_secs(30), upstream_events.recv())
        .await
        .expect("initial-send recovery frame timeout")
        .expect("initial-send recovery frame channel");
    assert_eq!(
        round, 1,
        "the first upstream socket must not receive a frame"
    );
    assert!(forwarded.contains("continue after reconnecting the image-heavy thread"));
    assert!(forwarded.contains("data:image/png;base64,"));

    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), client_ws.next())
            .await
            .expect("initial-send recovery response timeout")
            .expect("initial-send recovery client event")
            .expect("initial-send recovery client event result");
        match event {
            Message::Text(text) if text.contains("\"response.completed\"") => break,
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("initial-send recovery error escaped to client: {text}");
            }
            Message::Text(_) => {}
            other => panic!("unexpected initial-send recovery event: {other:?}"),
        }
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list initial-send recovery request logs");
    let ws_logs = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect::<Vec<_>>();
    assert_eq!(ws_logs.len(), 1);
    assert_eq!(ws_logs[0].status_code, Some(200));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), server_handle)
        .await
        .expect("front proxy initial-send recovery shutdown timeout")
        .expect("join initial-send recovery front proxy");
    tokio::time::timeout(Duration::from_secs(10), upstream_handle)
        .await
        .expect("mock upstream initial-send recovery shutdown timeout")
        .expect("join initial-send recovery mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_retries_when_reconnected_socket_breaks_before_send() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-double-initial-send-recovery");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_resets_twice_before_first_frame().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_double_initial_send_recovery",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_double_initial_send_recovery",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    let image_data = "A".repeat(TEST_LARGE_RESPONSES_WS_FRAME_BYTES);
    let payload = serde_json::json!({
        "type": "response.create",
        "model": "gpt-5.6-sol",
        "store": true,
        "input": [{
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "recover after two pre-send websocket resets" },
                {
                    "type": "input_image",
                    "image_url": format!("data:image/png;base64,{image_data}")
                }
            ]
        }]
    })
    .to_string();
    assert!(payload.len() > 16 * 1024 * 1024);
    client_ws
        .send(Message::Text(payload.into()))
        .await
        .expect("send double-reset image-heavy response.create");

    let mut forwarded = None;
    for _ in 0..3 {
        let (round, text) = tokio::time::timeout(Duration::from_secs(10), upstream_events.recv())
            .await
            .expect("double-reset recovery frame timeout")
            .expect("double-reset recovery frame channel");
        match round {
            1 | 2 => assert!(text.is_empty()),
            3 => {
                forwarded = Some(text);
                break;
            }
            0 => {
                panic!("double-reset recovery did not attempt a final upstream connection: {text}")
            }
            other => panic!("unexpected double-reset upstream round {other}"),
        }
    }
    let forwarded = forwarded.expect("final replacement must receive response.create");
    assert!(forwarded.contains("recover after two pre-send websocket resets"));
    assert!(forwarded.contains("data:image/png;base64,"));

    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), client_ws.next())
            .await
            .expect("double-reset recovery response timeout")
            .expect("double-reset recovery client event")
            .expect("double-reset recovery client event result");
        match event {
            Message::Text(text) if text.contains("\"response.completed\"") => break,
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("double-reset recovery error escaped to client: {text}");
            }
            Message::Text(_) => {}
            other => panic!("unexpected double-reset recovery event: {other:?}"),
        }
    }

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), server_handle)
        .await
        .expect("front proxy double-reset recovery shutdown timeout")
        .expect("join double-reset recovery front proxy");
    tokio::time::timeout(Duration::from_secs(10), upstream_handle)
        .await
        .expect("mock upstream double-reset recovery shutdown timeout")
        .expect("join double-reset recovery mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_switches_account_after_initial_send_reset() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-initial-send-account-switch");
    let mut storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_switches_after_initial_reset().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_initial_send_account_switch",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token_with_id(
        &storage,
        "acc_ws_failed",
        "failed-account",
        "workspace-failed",
        "failed-token",
        0,
    );
    insert_account_and_token_with_id(
        &storage,
        "acc_ws_replacement",
        "replacement-account",
        "workspace-replacement",
        "replacement-token",
        1,
    );
    storage
        .set_preferred_account(Some("acc_ws_failed"))
        .expect("prefer failed account for initial websocket attempt");
    crate::gateway::invalidate_candidate_cache();
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let routed = crate::gateway::gateway_collect_routed_candidates_for_ws(
        &storage,
        "gk_proxy_runtime_ws",
        Some("gpt-5.6-sol"),
        None,
        None,
    )
    .expect("collect account-switch websocket candidates");
    assert_eq!(
        routed
            .candidates
            .first()
            .map(|(account, _)| account.id.as_str()),
        Some("acc_ws_failed")
    );

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_initial_send_account_switch",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-5.6-sol",
                "store": true,
                "input": "switch accounts after the initial upstream socket resets"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send account-switch response.create");

    let mut replacement_frame = None;
    for _ in 0..4 {
        let event = tokio::time::timeout(Duration::from_secs(10), upstream_events.recv())
            .await
            .expect("account-switch upstream event timeout")
            .expect("account-switch upstream event");
        if event.0 == "workspace-replacement" {
            replacement_frame = Some(event.1);
            break;
        }
        assert_eq!(
            event.0, "workspace-failed",
            "the initial failed account may be attempted only before failover"
        );
        assert!(event.1.is_empty());
    }
    let replacement_frame = replacement_frame
        .expect("a bounded initial-send recovery must try the next eligible account");
    assert!(replacement_frame.contains("switch accounts after the initial upstream socket resets"));

    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), client_ws.next())
            .await
            .expect("account-switch response timeout")
            .expect("account-switch client event")
            .expect("account-switch client event result");
        match event {
            Message::Text(text) if text.contains("\"response.completed\"") => break,
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("account-switch recovery error escaped to client: {text}");
            }
            Message::Text(_) => {}
            other => panic!("unexpected account-switch event: {other:?}"),
        }
    }

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), server_handle)
        .await
        .expect("front proxy account-switch shutdown timeout")
        .expect("join account-switch front proxy");
    tokio::time::timeout(Duration::from_secs(10), upstream_handle)
        .await
        .expect("mock upstream account-switch shutdown timeout")
        .expect("join account-switch mock upstream");
    storage
        .set_preferred_account(None)
        .expect("clear preferred account after account-switch test");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_keeps_idle_session_alive_with_heartbeat() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-heartbeat");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_heartbeat_rx, upstream_handle) =
        start_mock_upstream_ws_waits_for_heartbeat().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_heartbeat",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_heartbeat",
        &[("OpenAI-Beta", "responses_websockets=2026-02-06")],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "keep this websocket alive"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send heartbeat test request");
    let upstream_request = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("heartbeat upstream request timeout")
        .expect("heartbeat upstream request channel");
    assert!(upstream_request.contains("keep this websocket alive"));

    let mut completed = false;
    while !completed {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("heartbeat initial response timeout")
            .expect("heartbeat client websocket must remain open")
            .expect("heartbeat initial response result");
        match event {
            Message::Text(text) => completed = text.contains("\"response.completed\""),
            other => panic!("unexpected heartbeat initial response: {other:?}"),
        }
    }

    let heartbeat_wait = tokio::time::timeout(Duration::from_secs(35), async {
        loop {
            let event = client_ws
                .next()
                .await
                .expect("heartbeat client websocket unexpectedly closed")
                .expect("heartbeat client websocket read failed");
            match event {
                Message::Ping(payload) => {
                    assert!(payload.is_empty(), "heartbeat ping should have no payload");
                    break;
                }
                Message::Pong(_) => {}
                other => panic!("unexpected idle websocket event: {other:?}"),
            }
        }
    })
    .await;
    heartbeat_wait.expect("downstream websocket heartbeat timeout");
    tokio::time::timeout(Duration::from_secs(5), upstream_heartbeat_rx)
        .await
        .expect("upstream websocket heartbeat timeout")
        .expect("upstream websocket heartbeat signal");

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy heartbeat shutdown timeout")
        .expect("join heartbeat front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock heartbeat upstream shutdown timeout")
        .expect("join heartbeat mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_replays_after_upstream_reset_after_preamble() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-reset-after-preamble");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_resets_after_preamble().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_reset_after_preamble",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_reset_after_preamble",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_reset_after_preamble"),
            ("x-client-request-id", "client_req_ws_reset_after_preamble"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "reset after preamble first"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send first reset-after-preamble request");
    let (first_round, first_text) =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("initial reset-after-preamble frame timeout")
            .expect("initial reset-after-preamble frame channel");
    assert_eq!(first_round, 0);
    assert!(first_text.contains("reset after preamble first"));

    let mut first_created_events = 0;
    let mut first_completed = false;
    while !first_completed {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("first reset-after-preamble client event timeout")
            .expect("client websocket must survive upstream reset")
            .expect("first reset-after-preamble client event result");
        match event {
            Message::Text(text) if text.contains("\"response.created\"") => {
                first_created_events += 1;
            }
            Message::Text(text) if text.contains("\"response.completed\"") => {
                first_completed = true;
            }
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("upstream reset escaped to client: {text}");
            }
            other => panic!("unexpected first reset-after-preamble event: {other:?}"),
        }
    }
    assert_eq!(
        first_created_events, 1,
        "replayed preamble must not be duplicated"
    );
    let (replay_round, replay_text) =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("replay after reset frame timeout")
            .expect("replay after reset frame channel");
    assert_eq!(replay_round, 1);
    assert!(replay_text.contains("reset after preamble first"));

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "reset after preamble follow-up"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send follow-up after reset recovery");
    let (follow_up_round, follow_up_text) =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("follow-up after reset frame timeout")
            .expect("follow-up after reset frame channel");
    assert_eq!(follow_up_round, 2);
    assert!(follow_up_text.contains("reset after preamble follow-up"));

    let mut follow_up_completed = false;
    while !follow_up_completed {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("follow-up after reset client event timeout")
            .expect("client websocket must remain open after reset recovery")
            .expect("follow-up after reset client event result");
        match event {
            Message::Text(text) if text.contains("\"response.completed\"") => {
                follow_up_completed = true;
            }
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("follow-up after reset escaped to client: {text}");
            }
            Message::Text(_) => {}
            other => panic!("unexpected follow-up after reset event: {other:?}"),
        }
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list reset-after-preamble request logs");
    let ws_logs = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect::<Vec<_>>();
    assert_eq!(ws_logs.len(), 2);
    assert!(ws_logs.iter().all(|item| item.status_code == Some(200)));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy reset-after-preamble shutdown timeout")
        .expect("join reset-after-preamble front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock reset-after-preamble upstream shutdown timeout")
        .expect("join reset-after-preamble mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_recovers_after_repeated_preamble_disconnects() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-repeated-preamble-recovery");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_resets_after_preamble_twice().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_repeated_preamble_recovery",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_repeated_preamble_recovery",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_repeated_preamble_recovery"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "store": true,
                "previous_response_id": "resp_prior_for_repeated_preamble",
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{
                        "type": "input_text",
                        "text": "continue after repeated preamble disconnects"
                    }]
                }]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send repeated preamble continuation");

    for round in 0..=2 {
        let (upstream_round, upstream_text) =
            tokio::time::timeout(Duration::from_secs(10), upstream_events.recv())
                .await
                .expect("repeated preamble upstream frame timeout")
                .expect("repeated preamble upstream frame channel");
        assert_eq!(upstream_round, round);
        assert!(upstream_text.contains("continue after repeated preamble disconnects"));
        assert!(upstream_text.contains("resp_prior_for_repeated_preamble"));
    }

    let mut created_events = 0;
    loop {
        let event = tokio::time::timeout(Duration::from_secs(10), client_ws.next())
            .await
            .expect("repeated preamble client event timeout")
            .expect("repeated preamble client event")
            .expect("repeated preamble client event result");
        match event {
            Message::Text(text) if text.contains("\"response.created\"") => {
                created_events += 1;
            }
            Message::Text(text) if text.contains("\"response.completed\"") => break,
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("repeated preamble recovery error escaped to client: {text}");
            }
            Message::Text(_) => {}
            other => panic!("unexpected repeated preamble event: {other:?}"),
        }
    }
    assert_eq!(created_events, 1, "replayed preambles must stay suppressed");

    tokio::time::sleep(Duration::from_millis(100)).await;
    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list repeated preamble request logs");
    let ws_logs = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect::<Vec<_>>();
    assert_eq!(ws_logs.len(), 1);
    assert_eq!(ws_logs[0].status_code, Some(200));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(10), server_handle)
        .await
        .expect("front proxy repeated preamble shutdown timeout")
        .expect("join repeated preamble front proxy");
    tokio::time::timeout(Duration::from_secs(10), upstream_handle)
        .await
        .expect("mock upstream repeated preamble shutdown timeout")
        .expect("join repeated preamble mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_recovers_after_connection_limit_error() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-connection-limit");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_connection_limit_then_success().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_connection_limit",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_connection_limit",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_connection_limit"),
            ("x-client-request-id", "client_req_ws_connection_limit"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "recover after connection limit"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send connection-limit request");
    let (initial_round, initial_text) =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("initial connection-limit frame timeout")
            .expect("initial connection-limit frame channel");
    assert_eq!(initial_round, 0);
    assert!(initial_text.contains("recover after connection limit"));

    let mut created_events = 0;
    let mut completed = false;
    while !completed {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("connection-limit client event timeout")
            .expect("connection-limit client websocket event")
            .expect("connection-limit client event result");
        match event {
            Message::Text(text) if text.contains("response.created") => {
                created_events += 1;
            }
            Message::Text(text) if text.contains("response.completed") => {
                completed = true;
            }
            Message::Text(text) if text.contains("websocket_connection_limit_reached") => {
                panic!("connection-limit transport error escaped to client: {text}");
            }
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("unexpected connection-limit error escaped to client: {text}");
            }
            other => panic!("unexpected connection-limit client event: {other:?}"),
        }
    }
    assert_eq!(
        created_events, 1,
        "replacement preamble must not be duplicated"
    );

    let (replacement_round, replacement_text) =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("replacement connection-limit frame timeout")
            .expect("replacement connection-limit frame channel");
    assert_eq!(replacement_round, 1);
    assert!(replacement_text.contains("recover after connection limit"));

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "follow up after connection limit"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send connection-limit follow-up");
    let (follow_up_round, follow_up_text) =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("connection-limit follow-up frame timeout")
            .expect("connection-limit follow-up frame channel");
    assert_eq!(follow_up_round, 2);
    assert!(follow_up_text.contains("follow up after connection limit"));

    let mut follow_up_completed = false;
    while !follow_up_completed {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("connection-limit follow-up client event timeout")
            .expect("connection-limit follow-up client websocket event")
            .expect("connection-limit follow-up client event result");
        match event {
            Message::Text(text) if text.contains("response.completed") => {
                follow_up_completed = true;
            }
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                panic!("connection-limit follow-up error escaped to client: {text}");
            }
            Message::Text(_) => {}
            other => panic!("unexpected connection-limit follow-up event: {other:?}"),
        }
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list connection-limit request logs");
    let ws_logs = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect::<Vec<_>>();
    assert_eq!(ws_logs.len(), 2);
    assert!(ws_logs.iter().all(|item| item.status_code == Some(200)));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy connection-limit shutdown timeout")
        .expect("join connection-limit front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock connection-limit upstream shutdown timeout")
        .expect("join connection-limit mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_does_not_replay_after_output_before_reset() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-reset-after-output");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, reset_upstream_tx, upstream_handle) =
        start_mock_upstream_ws_resets_after_output().await;
    let mut reset_upstream_tx = Some(reset_upstream_tx);
    insert_api_key_record(
        &storage,
        "platform_key_ws_reset_after_output",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_reset_after_output",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_reset_after_output"),
            ("x-client-request-id", "client_req_ws_reset_after_output"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "do not replay after output"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send reset-after-output request");
    let initial_text = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("initial reset-after-output frame timeout")
        .expect("initial reset-after-output frame channel");
    assert!(initial_text.contains("do not replay after output"));

    let mut received_output = false;
    let mut received_error = false;
    while !received_error {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("reset-after-output client event timeout")
            .expect("reset-after-output client websocket event")
            .expect("reset-after-output client event result");
        match event {
            Message::Text(text) if text.contains("response.output_text.delta") => {
                received_output = true;
                reset_upstream_tx
                    .take()
                    .expect("reset-after-output release sender")
                    .send(())
                    .expect("release reset-after-output upstream");
            }
            Message::Text(text) if text.contains("\"type\":\"error\"") => {
                received_error = true;
            }
            Message::Text(text) => {
                assert!(
                    text.contains("response.created"),
                    "unexpected reset-after-output event: {text}"
                );
            }
            Message::Close(_) => break,
            other => panic!("unexpected reset-after-output event: {other:?}"),
        }
    }
    assert!(received_output, "substantive output must reach the client");
    assert!(received_error, "the reset must be surfaced after output");
    assert!(
        upstream_events.try_recv().is_err(),
        "substantive output must disable request replay"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list reset-after-output request logs");
    let ws_logs = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect::<Vec<_>>();
    assert_eq!(ws_logs.len(), 1);
    assert_eq!(ws_logs[0].status_code, Some(502));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy reset-after-output shutdown timeout")
        .expect("join reset-after-output front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock reset-after-output shutdown timeout")
        .expect("join reset-after-output mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_reconnects_upstream_without_closing_client() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-reconnect");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_closes_after_each_response().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_reconnect",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_reconnect",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_reconnect"),
            ("x-client-request-id", "client_req_ws_reconnect"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    for round in 0..2 {
        client_ws
            .send(Message::Text(
                serde_json::json!({
                    "type": "response.create",
                    "model": "gpt-4.1",
                    "input": format!("resume round {round}")
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap_or_else(|err| panic!("send response.create round {round}: {err}"));

        let (upstream_round, upstream_text) =
            tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
                .await
                .expect("upstream reconnect frame timeout")
                .expect("upstream reconnect frame channel");
        assert_eq!(upstream_round, round);
        assert!(upstream_text.contains(format!("resume round {round}").as_str()));

        let mut completed = false;
        while !completed {
            let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
                .await
                .expect("client reconnect event timeout")
                .expect("client websocket must remain open")
                .expect("client reconnect event result");
            match event {
                Message::Text(text) => {
                    completed = text.contains("\"response.completed\"");
                }
                other => panic!("unexpected reconnect client event: {other:?}"),
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list reconnect request logs");
    assert_eq!(
        request_logs
            .iter()
            .filter(|item| item.request_type.as_deref() == Some("ws"))
            .count(),
        2,
        "both response.create frames must stay on the client websocket"
    );

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy reconnect shutdown timeout")
        .expect("join reconnect front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock reconnect upstream shutdown timeout")
        .expect("join reconnect mock upstream");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_replays_follow_up_accepted_by_closing_upstream() {
    let _guard = crate::test_env_guard();
    let _http_proxy = EnvGuard::clear("http_proxy");
    let _https_proxy = EnvGuard::clear("https_proxy");
    let _all_proxy = EnvGuard::clear("all_proxy");
    let _upper_http_proxy = EnvGuard::clear("HTTP_PROXY");
    let _upper_https_proxy = EnvGuard::clear("HTTPS_PROXY");
    let _upper_all_proxy = EnvGuard::clear("ALL_PROXY");
    let _no_proxy = EnvGuard::set("NO_PROXY", "127.0.0.1,localhost");
    let _lower_no_proxy = EnvGuard::clear("no_proxy");
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-stale-follow-up");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, upstream_handle) =
        start_mock_upstream_ws_closes_after_accepting_follow_up().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_stale_follow_up",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_stale_follow_up",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_stale_follow_up"),
            ("x-client-request-id", "client_req_ws_stale_follow_up"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "historical resume seed"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send initial historical response.create");
    let (initial_phase, initial_text) =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("initial upstream frame timeout")
            .expect("initial upstream frame channel");
    assert_eq!(initial_phase, 0);
    assert!(initial_text.contains("historical resume seed"));

    let mut initial_completed = false;
    while !initial_completed {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("initial historical client event timeout")
            .expect("historical client websocket must remain open")
            .expect("initial historical client event result");
        match event {
            Message::Text(text) => {
                initial_completed = text.contains("\"response.completed\"");
            }
            other => panic!("unexpected initial historical client event: {other:?}"),
        }
    }

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "store": false,
                "previous_response_id": "resp_ws_stale_follow_up_0",
                "input": "historical resume follow-up"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send historical follow-up response.create");

    for expected_phase in [1, 2] {
        let (phase, text) = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("historical follow-up replay timeout")
            .expect("historical follow-up replay channel");
        assert_eq!(phase, expected_phase);
        assert!(text.contains("historical resume follow-up"));
        let payload: serde_json::Value =
            serde_json::from_str(&text).expect("parse historical follow-up frame");
        if expected_phase == 1 {
            assert_eq!(payload["previous_response_id"], "resp_ws_stale_follow_up_0");
        } else {
            assert!(payload.get("previous_response_id").is_none());
            let input = payload["input"]
                .as_array()
                .expect("replacement request carries full context");
            assert_eq!(input.len(), 3);
            assert_eq!(input[0]["content"][0]["text"], "historical resume seed");
            assert_eq!(input[1]["content"][0]["text"], "historical seed answer");
            assert_eq!(
                input[2]["content"][0]["text"],
                "historical resume follow-up"
            );
        }
    }

    let mut follow_up_completed = false;
    while !follow_up_completed {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("historical follow-up client event timeout")
            .expect("historical client websocket must survive stale upstream close")
            .expect("historical follow-up client event result");
        match event {
            Message::Text(text) => {
                follow_up_completed = text.contains("\"response.completed\"");
            }
            other => panic!("unexpected historical follow-up client event: {other:?}"),
        }
    }

    let request_logs = storage
        .list_request_logs(None, 10)
        .expect("list stale-follow-up request logs");
    let ws_logs = request_logs
        .iter()
        .filter(|item| item.request_type.as_deref() == Some("ws"))
        .collect::<Vec<_>>();
    assert_eq!(ws_logs.len(), 2);
    assert!(ws_logs.iter().all(|item| item.status_code == Some(200)));

    let _ = client_ws.close(None).await;
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy stale-follow-up shutdown timeout")
        .expect("join stale-follow-up front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock stale-follow-up upstream shutdown timeout")
        .expect("join stale-follow-up mock upstream");
}

#[tokio::test]
async fn official_responses_websocket_aligns_prompt_cache_key_with_resolved_session_anchor() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-explicit-prompt-cache-key");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, capture_rx, upstream_handle) =
        start_mock_upstream_ws().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_explicit_prompt_cache_key",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token(&storage);
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_explicit_prompt_cache_key",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_explicit_prompt_cache_key"),
            (
                "x-client-request-id",
                "client_req_ws_explicit_prompt_cache_key",
            ),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "hello",
                "prompt_cache_key": "client_ws_thread_123"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send frame");

    let upstream_frame = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("upstream frame timeout")
        .expect("upstream frame channel");
    let payload: serde_json::Value =
        serde_json::from_str(&upstream_frame).expect("parse upstream frame");
    assert_eq!(payload["type"], "response.create");
    let upstream_prompt_cache_key = payload
        .get("prompt_cache_key")
        .and_then(serde_json::Value::as_str)
        .expect("resolved session anchor is forwarded as prompt_cache_key");
    assert_ne!(upstream_prompt_cache_key, "client_ws_thread_123");
    assert!(
        upstream_prompt_cache_key
            .split('-')
            .map(str::len)
            .eq([8, 4, 4, 4, 12])
            && upstream_prompt_cache_key
                .chars()
                .all(|ch| ch == '-' || ch.is_ascii_hexdigit()),
        "resolved session anchor should use the generated conversation id"
    );

    let client_event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("client event timeout")
        .expect("client event")
        .expect("client event result");
    match client_event {
        Message::Text(text) => {
            assert!(
                text.contains("\"response.created\""),
                "unexpected event: {text}"
            );
        }
        other => panic!("unexpected client event: {other:?}"),
    }

    let _ = client_ws.close(None).await;
    shutdown_tx.send(()).ok();
    server_handle.await.expect("front proxy join");
    upstream_handle.abort();
    let _ = capture_rx.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_responses_websocket_rebases_tool_output_on_next_account() {
    let _guard = crate::test_env_guard();
    let db_path = new_test_db_path("codexmanager-proxy-runtime-ws-failover");
    let storage = init_test_storage(&db_path);
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let (upstream_addr, mut upstream_events, capture_rx, upstream_handle) =
        start_mock_upstream_ws_usage_limit_then_success().await;
    insert_api_key_record(
        &storage,
        "platform_key_ws_failover",
        crate::apikey_profile::ROTATION_ACCOUNT,
        Some(format!(
            "http://{upstream_addr}/chatgpt.com/backend-api/codex"
        )),
    );
    insert_account_and_token_with_id(
        &storage,
        "acc_proxy_runtime_ws_a",
        "proxy-runtime-ws-a",
        "chatgpt_proxy_runtime_ws_a",
        "access_token_ws_a",
        0,
    );
    insert_account_and_token_with_id(
        &storage,
        "acc_proxy_runtime_ws_b",
        "proxy-runtime-ws-b",
        "chatgpt_proxy_runtime_ws_b",
        "access_token_ws_b",
        2,
    );
    insert_account_and_token_with_id(
        &storage,
        "acc_proxy_runtime_ws_forbidden",
        "proxy-runtime-ws-forbidden",
        "chatgpt_proxy_runtime_ws_forbidden",
        "access_token_ws_forbidden",
        1,
    );
    storage
        .update_account_group_name("acc_proxy_runtime_ws_a", Some("team-a"))
        .expect("group first websocket account");
    storage
        .update_account_group_name("acc_proxy_runtime_ws_b", Some("team-a"))
        .expect("group failover websocket account");
    storage
        .update_account_group_name("acc_proxy_runtime_ws_forbidden", Some("team-b"))
        .expect("group forbidden websocket account");
    storage
        .update_api_key_account_group_filter("gk_proxy_runtime_ws", Some("team-a"))
        .expect("restrict websocket api key group");
    tokio::task::spawn_blocking(|| {
        crate::gateway::reload_runtime_config_from_env();
        let _ = crate::gateway::front_proxy_max_body_bytes();
    })
    .await
    .expect("reload runtime config");

    let state = ProxyState {
        backend_base_url: "http://127.0.0.1:1".to_string(),
        client: Client::new(),
    };
    let (front_addr, shutdown_tx, server_handle) = start_front_proxy_test_server(state).await;
    let request = build_ws_request(
        &format!("ws://{front_addr}/v1/responses"),
        "platform_key_ws_failover",
        &[
            ("OpenAI-Beta", "responses_websockets=2026-02-06"),
            ("session_id", "session_ws_failover"),
            ("x-client-request-id", "request_ws_failover"),
            ("x-codex-window-id", "window_ws_failover"),
            ("x-codex-turn-state", "turn_state_ws_failover"),
            ("x-codex-turn-metadata", "turn_meta_ws_failover"),
            ("x-codex-parent-thread-id", "parent_ws_failover"),
            ("x-openai-subagent", "review"),
            ("x-codex-beta-features", "beta-a"),
        ],
    );
    let (mut client_ws, response) = connect_async(request).await.expect("websocket connects");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "input": "make a patch"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send first frame");

    let first_upstream_frame = tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
        .await
        .expect("first upstream frame timeout")
        .expect("first upstream frame channel");
    let first_payload: serde_json::Value =
        serde_json::from_str(&first_upstream_frame).expect("parse first upstream frame");
    assert_eq!(first_payload["type"], "response.create");
    assert_eq!(first_payload["input"], "make a patch");

    let mut seed_client_events = Vec::new();
    for _ in 0..3 {
        let event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
            .await
            .expect("seed client event timeout")
            .expect("seed client event")
            .expect("seed client event result");
        match event {
            Message::Text(text) => seed_client_events.push(text.to_string()),
            other => panic!("unexpected seed client event: {other:?}"),
        }
    }
    assert!(seed_client_events[0].contains("\"response.created\""));
    assert!(seed_client_events[1].contains("\"custom_tool_call\""));
    assert!(seed_client_events[1].contains("call_ws_tool_rebase"));
    assert!(seed_client_events[2].contains("\"response.completed\""));

    client_ws
        .send(Message::Text(
            serde_json::json!({
                "type": "response.create",
                "model": "gpt-4.1",
                "previous_response_id": "resp_ws_tool_seed",
                "input": [
                    {
                        "type": "reasoning",
                        "id": "reasoning_old_account",
                        "summary": [],
                        "encrypted_content": "encrypted_old_account"
                    },
                    {
                        "type": "custom_tool_call_output",
                        "call_id": "call_ws_tool_rebase",
                        "output": "patch applied"
                    }
                ]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send incremental tool output frame");

    let second_upstream_frame =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("limited account tool-output frame timeout")
            .expect("limited account tool-output frame channel");
    let second_payload: serde_json::Value =
        serde_json::from_str(&second_upstream_frame).expect("parse second upstream frame");
    assert_eq!(second_payload["type"], "response.create");
    assert_eq!(second_payload["previous_response_id"], "resp_ws_tool_seed");
    assert_eq!(
        second_payload["input"][1]["type"],
        "custom_tool_call_output"
    );

    let rebased_upstream_frame =
        tokio::time::timeout(Duration::from_secs(5), upstream_events.recv())
            .await
            .expect("rebased upstream frame timeout")
            .expect("rebased upstream frame channel");
    let rebased_payload: serde_json::Value =
        serde_json::from_str(&rebased_upstream_frame).expect("parse rebased upstream frame");
    assert!(rebased_payload.get("previous_response_id").is_none());
    let rebased_input = rebased_payload["input"]
        .as_array()
        .expect("rebased tool input array");
    assert_eq!(rebased_input.len(), 3);
    assert_eq!(rebased_input[0]["type"], "message");
    assert_eq!(rebased_input[0]["role"], "user");
    assert_eq!(rebased_input[0]["content"][0]["text"], "make a patch");
    assert_eq!(rebased_input[1]["type"], "custom_tool_call");
    assert_eq!(rebased_input[1]["call_id"], "call_ws_tool_rebase");
    assert_eq!(rebased_input[2]["type"], "custom_tool_call_output");
    assert_eq!(rebased_input[2]["call_id"], "call_ws_tool_rebase");
    assert!(rebased_input
        .iter()
        .all(|item| item.get("encrypted_content").is_none()));
    for key in [
        "x-codex-window-id",
        "x-codex-turn-metadata",
        "x-codex-parent-thread-id",
    ] {
        assert!(
            rebased_payload["client_metadata"].get(key).is_none(),
            "cross-account response.create must strip {key}"
        );
    }
    assert_eq!(
        rebased_payload["client_metadata"]["x-openai-subagent"],
        "review"
    );

    let first_client_event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("client retry created event timeout")
        .expect("client retry created event")
        .expect("client retry created event result");
    let second_client_event = tokio::time::timeout(Duration::from_secs(5), client_ws.next())
        .await
        .expect("client retry completed event timeout")
        .expect("client retry completed event")
        .expect("client retry completed event result");
    let client_events = [first_client_event, second_client_event]
        .into_iter()
        .map(|event| match event {
            Message::Text(text) => text.to_string(),
            other => panic!("unexpected retry client event: {other:?}"),
        })
        .collect::<Vec<_>>();
    assert!(client_events[0].contains("\"response.created\""));
    assert!(client_events[0].contains("resp_ws_failover_ok"));
    assert!(client_events[1].contains("\"response.completed\""));
    assert!(client_events[1].contains("resp_ws_failover_ok"));
    assert!(client_events.iter().all(|event| {
        !event.contains("resp_ws_limited_a")
            && !event.contains("usage_limit_reached")
            && !event.contains("The usage limit has been reached")
    }));

    let captures = tokio::time::timeout(Duration::from_secs(5), capture_rx)
        .await
        .expect("capture timeout")
        .expect("capture result");
    assert_eq!(
        captures.len(),
        2,
        "expected two upstream websocket sessions"
    );
    assert_eq!(
        captures[0].headers.get("authorization").map(String::as_str),
        Some("Bearer access_token_ws_a")
    );
    assert_eq!(
        captures[1].headers.get("authorization").map(String::as_str),
        Some("Bearer access_token_ws_b")
    );
    assert_eq!(
        captures[0]
            .headers
            .get("chatgpt-account-id")
            .map(String::as_str),
        Some("chatgpt_proxy_runtime_ws_a")
    );
    assert_eq!(
        captures[1]
            .headers
            .get("chatgpt-account-id")
            .map(String::as_str),
        Some("chatgpt_proxy_runtime_ws_b")
    );
    assert_eq!(
        captures[0].headers.get("session_id").map(String::as_str),
        Some("session_ws_failover")
    );
    assert!(
        captures[1].headers.get("session_id").is_none(),
        "cross-account websocket must not reuse the previous account session"
    );
    for header in [
        "x-client-request-id",
        "x-codex-window-id",
        "x-codex-turn-state",
        "x-codex-turn-metadata",
        "x-codex-parent-thread-id",
    ] {
        assert!(
            captures[1].headers.get(header).is_none(),
            "cross-account websocket must strip {header}"
        );
    }
    assert_eq!(
        captures[1]
            .headers
            .get("x-openai-subagent")
            .map(String::as_str),
        Some("review")
    );
    assert_eq!(
        captures[1]
            .headers
            .get("x-codex-beta-features")
            .map(String::as_str),
        Some("beta-a")
    );

    let limited_account = storage
        .find_account_by_id("acc_proxy_runtime_ws_a")
        .expect("find exhausted websocket account")
        .expect("exhausted websocket account exists");
    assert_eq!(limited_account.status, "limited");
    let status_reasons = storage
        .latest_account_status_reasons(&["acc_proxy_runtime_ws_a".to_string()])
        .expect("load exhausted websocket account status reason");
    assert_eq!(
        status_reasons
            .get("acc_proxy_runtime_ws_a")
            .map(String::as_str),
        Some("usage_limit_exhausted")
    );

    client_ws.close(None).await.expect("close client websocket");
    let _ = shutdown_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("front proxy shutdown timeout")
        .expect("join front proxy");
    tokio::time::timeout(Duration::from_secs(5), upstream_handle)
        .await
        .expect("mock upstream shutdown timeout")
        .expect("join mock upstream");
}
