use super::{
    account_test_events_role_allowed, account_test_events_target_url,
    format_upstream_error_message, gateway_proxy_max_body_bytes, gateway_proxy_target_url,
    service_probe_client, should_skip_gateway_request_header, should_skip_gateway_response_header,
    tcp_probe, ENV_GATEWAY_PROXY_MAX_BODY_BYTES,
};
use axum::http::{header, HeaderValue, Uri};
use axum::{body::Bytes, extract::State, http::HeaderMap};
use std::sync::{Mutex, MutexGuard};

static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

fn env_test_lock() -> MutexGuard<'static, ()> {
    ENV_TEST_LOCK.lock().expect("env test lock")
}

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

    fn clear(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
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

#[test]
fn format_upstream_error_message_adds_docker_hint_for_host_internal() {
    let err = std::io::Error::other("dns failed");
    let message = format_upstream_error_message("host.docker.internal:9760", &err);
    assert!(message.contains("host.docker.internal"));
    assert!(message.contains("codexmanager-service:48760"));
}

#[test]
fn service_probe_client_builds_with_dedicated_config_and_reuses_cache() {
    let first = service_probe_client().expect("build first probe client");
    let second = service_probe_client().expect("reuse probe client");
    drop((first, second));
}

#[tokio::test]
async fn tcp_probe_reports_listening_socket_as_reachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe listener");
    let addr = listener.local_addr().expect("read probe listener address");

    assert!(tcp_probe(&format!("http://{addr}/rpc")).await);
}

#[tokio::test]
async fn tcp_probe_reports_refused_connection_as_unreachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve probe port");
    let addr = listener.local_addr().expect("read reserved probe address");
    drop(listener);

    assert!(!tcp_probe(&addr.to_string()).await);
}

#[test]
fn gateway_proxy_target_url_preserves_path_and_query() {
    let uri: Uri = "/v1/responses?stream=true".parse().expect("valid uri");
    assert_eq!(
        gateway_proxy_target_url("localhost:48760", &uri),
        "http://localhost:48760/v1/responses?stream=true"
    );
}

#[test]
fn gateway_proxy_body_limit_defaults_to_unbounded() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::clear(ENV_GATEWAY_PROXY_MAX_BODY_BYTES);

    assert_eq!(gateway_proxy_max_body_bytes(), 0);
}

#[test]
fn gateway_proxy_body_limit_allows_env_override() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::set(ENV_GATEWAY_PROXY_MAX_BODY_BYTES, "536870912");

    assert_eq!(gateway_proxy_max_body_bytes(), 536_870_912);
}

#[test]
fn gateway_proxy_header_filters_skip_hop_by_hop_headers() {
    assert!(should_skip_gateway_request_header(
        &header::HOST,
        &HeaderValue::from_static("example.com")
    ));
    assert!(should_skip_gateway_response_header(&header::CONTENT_LENGTH));
    assert!(!should_skip_gateway_request_header(
        &header::AUTHORIZATION,
        &HeaderValue::from_static("Bearer key")
    ));
}

#[tokio::test]
async fn rpc_proxy_rejects_body_over_the_bounded_upload_limit() {
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let state = std::sync::Arc::new(crate::AppState {
        client: reqwest::Client::new(),
        service_rpc_url: "http://127.0.0.1:1/rpc".to_string(),
        service_addr: "127.0.0.1:1".to_string(),
        rpc_token: "test-token".to_string(),
        web_auth_session_key: "test-session".to_string(),
        shutdown_tx,
        spawned_service: std::sync::Arc::new(tokio::sync::Mutex::new(false)),
        missing_ui_html: std::sync::Arc::new(String::new()),
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let body = Bytes::from(vec![b'x'; codexmanager_service::RPC_BODY_LIMIT_BYTES + 1]);

    let response = super::rpc_proxy(State(state), headers, body).await;

    assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
}

/// 在独立线程里起一个最小 HTTP 服务，返回固定 SSE 响应后关闭连接，供代理测试当上游用。
fn spawn_mock_sse_upstream(body: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut socket, _) = listener.accept().expect("accept mock request");
        let mut buf = [0u8; 4096];
        let mut total = 0;
        loop {
            let n = socket.read(&mut buf[total..]).expect("read mock request");
            if n == 0 {
                break;
            }
            total += n;
            if buf[..total].windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
        );
        let _ = socket.write_all(response.as_bytes());
        let _ = socket.shutdown(std::net::Shutdown::Both);
    });
    addr.to_string()
}

#[tokio::test]
async fn account_test_events_proxies_sse_stream_from_service() {
    let upstream =
        spawn_mock_sse_upstream("event: account-test-event\ndata: {\"testId\":\"t1\"}\n\n");
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let state = std::sync::Arc::new(crate::AppState {
        client: reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client"),
        service_rpc_url: "http://127.0.0.1:1/rpc".to_string(),
        service_addr: upstream,
        rpc_token: "test-token".to_string(),
        web_auth_session_key: "test-session".to_string(),
        shutdown_tx,
        spawned_service: std::sync::Arc::new(tokio::sync::Mutex::new(false)),
        missing_ui_html: std::sync::Arc::new(String::new()),
    });

    let uri: Uri = "/api/events/account-test?testId=t1"
        .parse()
        .expect("valid account test URI");
    let response = super::account_test_events(State(state), HeaderMap::new(), uri).await;

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream")
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("account-test-event"),
        "unexpected body: {text}"
    );
    assert!(text.contains("t1"), "unexpected body: {text}");
}

#[test]
fn account_test_events_require_admin_in_accounts_mode() {
    assert!(account_test_events_role_allowed("none", None));
    assert!(account_test_events_role_allowed("password", None));
    assert!(account_test_events_role_allowed("accounts", Some("admin")));
    assert!(account_test_events_role_allowed(
        "accounts",
        Some("system_admin")
    ));
    assert!(!account_test_events_role_allowed(
        "accounts",
        Some("member")
    ));
    assert!(!account_test_events_role_allowed("accounts", None));
}

#[test]
fn account_test_events_target_preserves_test_id_query() {
    let uri: Uri = "/api/events/account-test?testId=550e8400-e29b-41d4-a716-446655440000"
        .parse()
        .expect("valid URI");
    assert_eq!(
        account_test_events_target_url("127.0.0.1:48760", &uri),
        "http://127.0.0.1:48760/events/account-test?testId=550e8400-e29b-41d4-a716-446655440000"
    );
}
