use super::{
    account_test_events_role_allowed, account_test_events_target_url, ensure_service_running,
    format_upstream_error_message, gateway_proxy_max_body_bytes, gateway_proxy_target_url,
    service_bin_path, service_probe_client, should_restart_service_after_probe_failure,
    should_skip_gateway_request_header, should_skip_gateway_response_header, tcp_probe,
    validate_service_initialize_response, ENV_GATEWAY_PROXY_MAX_BODY_BYTES,
    ENV_WEB_NO_SPAWN_SERVICE, SERVICE_VERSION_MISMATCH,
};
use axum::http::{header, HeaderValue, Uri};
use axum::{body::Bytes, extract::State, http::HeaderMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, MutexGuard};
use std::time::Duration;

fn env_test_lock() -> MutexGuard<'static, ()> {
    crate::auth::test_env_guard()
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

fn initialize_response(version: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "version": version,
            "userAgent": "codex_cli_rs/1.0.0",
            "codexHome": "C:/tmp/codexmanager"
        }
    })
}

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codexmanager-web-{label}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ShutdownFlagReset;

impl ShutdownFlagReset {
    fn new() -> Self {
        codexmanager_service::clear_shutdown_flag();
        Self
    }
}

impl Drop for ShutdownFlagReset {
    fn drop(&mut self) {
        codexmanager_service::clear_shutdown_flag();
    }
}

async fn spawn_stale_service() -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stale service");
    let addr = listener.local_addr().expect("read stale service address");
    let app = axum::Router::new().route(
        "/rpc",
        axum::routing::post(|| async { axum::Json(initialize_response("0.0.0-old")) }),
    );
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("serve stale service");
    });
    (addr.to_string(), shutdown_tx, task)
}

async fn assert_stale_service_survives_binary_preflight_failure(
    dir: &std::path::Path,
    expected_error: &str,
) {
    let (addr, shutdown_tx, task) = spawn_stale_service().await;
    let spawned_service = std::sync::Arc::new(tokio::sync::Mutex::new(false));

    let error = ensure_service_running(&addr, "test-token", dir, &spawned_service)
        .await
        .expect("replacement should fail preflight");

    assert!(error.contains("service_version_mismatch"), "{error}");
    assert!(error.contains("old instance was left running"), "{error}");
    assert!(error.contains(expected_error), "{error}");
    assert!(!codexmanager_service::shutdown_requested());
    assert!(tcp_probe(&addr).await);
    assert!(!*spawned_service.lock().await);

    let _ = shutdown_tx.send(());
    task.await.expect("join stale service");
}

#[test]
fn service_initialize_response_accepts_current_version() {
    let payload = initialize_response(env!("CARGO_PKG_VERSION"));

    validate_service_initialize_response(&payload).expect("accept current service version");
}

#[test]
fn service_version_mismatch_requests_service_restart() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::clear(ENV_WEB_NO_SPAWN_SERVICE);
    let error = validate_service_initialize_response(&initialize_response("0.0.0-old"))
        .expect_err("reject stale service version");

    assert!(error.starts_with(SERVICE_VERSION_MISMATCH));
    assert!(error.contains(env!("CARGO_PKG_VERSION")));
    assert!(error.contains("0.0.0-old"));
    assert!(should_restart_service_after_probe_failure(&error));
}

#[test]
fn service_version_mismatch_is_reported_when_spawn_is_disabled() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::set(ENV_WEB_NO_SPAWN_SERVICE, "1");
    let error = validate_service_initialize_response(&initialize_response("0.0.0-old"))
        .expect_err("reject stale service version");

    assert!(error.starts_with(SERVICE_VERSION_MISMATCH));
    assert!(!should_restart_service_after_probe_failure(&error));
    let startup_error =
        format!("service reachable at 127.0.0.1:48760 but startup handshake failed: {error}");
    assert!(startup_error.contains("service_version_mismatch"));
    assert!(startup_error.contains("expected"));
    assert!(startup_error.contains("got 0.0.0-old"));
}

#[tokio::test]
async fn stale_service_is_not_shutdown_when_replacement_binary_is_missing() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::clear(ENV_WEB_NO_SPAWN_SERVICE);
    let _shutdown_reset = ShutdownFlagReset::new();
    let dir = TestDir::new("missing-service-binary");

    assert_stale_service_survives_binary_preflight_failure(dir.path(), "missing").await;
}

#[tokio::test]
async fn stale_service_is_not_shutdown_when_replacement_binary_is_unavailable() {
    let _lock = env_test_lock();
    let _guard = EnvGuard::clear(ENV_WEB_NO_SPAWN_SERVICE);
    let _shutdown_reset = ShutdownFlagReset::new();
    let dir = TestDir::new("unavailable-service-binary");
    let bin = service_bin_path(dir.path());

    #[cfg(unix)]
    let expected_error = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&bin, b"not executable").expect("create service file");
        let mut permissions = std::fs::metadata(&bin)
            .expect("read service file metadata")
            .permissions();
        permissions.set_mode(0o644);
        std::fs::set_permissions(&bin, permissions).expect("remove service execute permission");
        "not executable"
    };

    #[cfg(not(unix))]
    let expected_error = {
        std::fs::create_dir(&bin).expect("create non-file service path");
        "not a regular file"
    };

    assert_stale_service_survives_binary_preflight_failure(dir.path(), expected_error).await;
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

    let response = super::rpc_proxy(
        State(state),
        axum::Extension(codexmanager_service::RpcActor::system_admin()),
        headers,
        body,
    )
    .await;

    assert_eq!(response.status(), axum::http::StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn rpc_proxy_preserves_authenticated_member_without_reloading_session() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind actor mock");
    let addr = listener.local_addr().unwrap();
    let mock = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route(
                "/rpc",
                axum::routing::post(|headers: HeaderMap| async move {
                    assert_eq!(headers["x-codexmanager-rpc-token"], "actor-test-token");
                    axum::Json(serde_json::json!({
                        "role": headers["x-codexmanager-rpc-actor-role"].to_str().unwrap(),
                        "userId": headers["x-codexmanager-rpc-actor-user-id"].to_str().unwrap(),
                    }))
                }),
            ),
        )
        .await
        .unwrap();
    });
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(crate::AppState {
        client: super::build_service_proxy_client().unwrap(),
        service_rpc_url: format!("http://{addr}/rpc"),
        service_addr: addr.to_string(),
        rpc_token: "actor-test-token".to_string(),
        web_auth_session_key: "actor-test-session".to_string(),
        shutdown_tx,
        spawned_service: Arc::new(tokio::sync::Mutex::new(false)),
        missing_ui_html: Arc::new(String::new()),
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        "x-codexmanager-rpc-actor-role",
        HeaderValue::from_static("system_admin"),
    );
    // The session cookie is absent by the time the proxy runs. The identity
    // authenticated by middleware must survive session revocation or a mode
    // change, and an incoming role header must not override that identity.
    let response = super::rpc_proxy(
        State(state),
        axum::Extension(codexmanager_service::RpcActor::from_parts(
            Some("member"),
            Some("member-user"),
        )),
        headers,
        Bytes::from_static(
            br#"{"jsonrpc":"2.0","id":1,"method":"account/chatgptAuthTokens/refresh"}"#,
        ),
    )
    .await;
    let body = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .unwrap();
    mock.abort();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["role"], "member");
    assert_eq!(payload["userId"], "member-user");
}

#[tokio::test]
async fn rpc_proxy_rejects_a_request_without_a_trusted_actor_extension() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind untrusted actor route");
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(crate::AppState {
        client: super::build_service_proxy_client().unwrap(),
        service_rpc_url: "http://127.0.0.1:1/rpc".to_string(),
        service_addr: "127.0.0.1:1".to_string(),
        rpc_token: "actor-test-token".to_string(),
        web_auth_session_key: "actor-test-session".to_string(),
        shutdown_tx,
        spawned_service: Arc::new(tokio::sync::Mutex::new(false)),
        missing_ui_html: Arc::new(String::new()),
    });
    let app = axum::Router::new()
        .route("/api/rpc", axum::routing::post(super::rpc_proxy))
        .with_state(state);
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("http://{addr}/api/rpc"))
        .header("content-type", "application/json")
        .header("x-codexmanager-rpc-actor-role", "system_admin")
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"account/list"}"#)
        .send()
        .await
        .unwrap();
    server.abort();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
    assert!(response.text().await.unwrap().contains("Extension"));
}

#[tokio::test]
async fn rpc_proxy_does_not_replay_committed_writes_through_307_or_308_redirects() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    for status in [
        axum::http::StatusCode::TEMPORARY_REDIRECT,
        axum::http::StatusCode::PERMANENT_REDIRECT,
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect mock");
        let addr = listener.local_addr().expect("redirect mock address");
        let commits = Arc::new(AtomicUsize::new(0));
        let replays = Arc::new(AtomicUsize::new(0));
        let original_commits = commits.clone();
        let redirected_commits = replays.clone();
        let app = axum::Router::new()
            .route(
                "/rpc",
                axum::routing::post(move |body: Bytes| {
                    let original_commits = original_commits.clone();
                    async move {
                        let request: serde_json::Value =
                            serde_json::from_slice(&body).expect("RPC mock request");
                        assert_eq!(request["method"], "apikey/create");
                        // Commit first, then redirect while preserving the POST body.
                        original_commits.fetch_add(1, Ordering::SeqCst);
                        (status, [(header::LOCATION, "/redirected")], "")
                    }
                }),
            )
            .route(
                "/redirected",
                axum::routing::post(move || {
                    let redirected_commits = redirected_commits.clone();
                    async move {
                        redirected_commits.fetch_add(1, Ordering::SeqCst);
                        axum::Json(serde_json::json!({ "result": { "id": "duplicate-key" } }))
                    }
                }),
            );
        let (mock_shutdown_tx, mock_shutdown_rx) = tokio::sync::oneshot::channel();
        let mock = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = mock_shutdown_rx.await;
                })
                .await
                .expect("serve redirect mock");
        });
        let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
        let state = Arc::new(crate::AppState {
            client: super::build_service_proxy_client().expect("service proxy client"),
            service_rpc_url: format!("http://{addr}/rpc"),
            service_addr: addr.to_string(),
            rpc_token: "mock-rpc-token".to_string(),
            web_auth_session_key: "mock-session".to_string(),
            shutdown_tx,
            spawned_service: Arc::new(tokio::sync::Mutex::new(false)),
            missing_ui_html: Arc::new(String::new()),
        });
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let body =
            Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"apikey/create","params":{}}"#);
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            super::rpc_proxy(
                State(state),
                axum::Extension(codexmanager_service::RpcActor::system_admin()),
                headers,
                body,
            ),
        )
        .await
        .expect("RPC proxy must finish");
        let _ = mock_shutdown_tx.send(());
        mock.await.expect("join redirect mock");

        assert_eq!(response.status(), status);
        assert_eq!(commits.load(Ordering::SeqCst), 1);
        assert_eq!(
            replays.load(Ordering::SeqCst),
            0,
            "redirect status {status}"
        );
    }
}

enum RpcMockFailure {
    LostResponse,
    StalledBody,
}

struct RpcFailureMock {
    listener: Arc<tokio::net::TcpListener>,
    commits: Arc<AtomicUsize>,
    ready: tokio::sync::oneshot::Receiver<()>,
    task: tokio::task::JoinHandle<()>,
}

async fn read_mock_rpc_request(stream: &mut tokio::net::TcpStream) -> serde_json::Value {
    use tokio::io::AsyncReadExt;

    tokio::time::timeout(Duration::from_secs(2), async {
        let mut request = Vec::new();
        loop {
            let mut bytes = [0_u8; 1024];
            let read = stream
                .read(&mut bytes)
                .await
                .expect("read mock RPC request");
            assert!(read > 0, "request ended before its complete body");
            request.extend_from_slice(&bytes[..read]);
            assert!(request.len() <= 8192, "mock RPC request exceeded its bound");
            let Some(split) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                continue;
            };
            let headers = std::str::from_utf8(&request[..split]).expect("mock HTTP headers");
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().expect("content length"))
                })
                .expect("mock content length header");
            if request.len() >= split + 4 + length {
                return serde_json::from_slice(&request[split + 4..split + 4 + length])
                    .expect("mock JSON RPC request");
            }
        }
    })
    .await
    .expect("complete mock RPC request must arrive within its bound")
}

async fn spawn_rpc_failure_mock(failure: RpcMockFailure) -> RpcFailureMock {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = Arc::new(
        tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind RPC failure mock"),
    );
    let server_listener = listener.clone();
    let commits = Arc::new(AtomicUsize::new(0));
    let server_commits = commits.clone();
    let (ready_tx, ready) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) =
            tokio::time::timeout(Duration::from_secs(2), server_listener.accept())
                .await
                .expect("RPC must connect within its bound")
                .expect("accept RPC failure mock");
        assert_eq!(
            read_mock_rpc_request(&mut stream).await["method"],
            "apikey/create"
        );
        server_commits.fetch_add(1, Ordering::SeqCst);
        match failure {
            RpcMockFailure::LostResponse => {
                ready_tx.send(()).expect("report committed request");
                // Commit before dropping the connection without any response.
            }
            RpcMockFailure::StalledBody => {
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n",
                    )
                    .await
                    .expect("send mock headers without a body");
                ready_tx.send(()).expect("report sent response headers");
                let mut byte = [0_u8; 1];
                match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut byte))
                    .await
                    .expect("proxy must release its stalled upstream socket")
                {
                    Ok(0) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
                    other => panic!("unexpected data while observing upstream release: {other:?}"),
                }
            }
        }
    });
    RpcFailureMock {
        listener,
        commits,
        ready,
        task,
    }
}

async fn call_rpc_proxy_for_mock(
    client: reqwest::Client,
    addr: std::net::SocketAddr,
) -> axum::response::Response {
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    let state = Arc::new(crate::AppState {
        client,
        service_rpc_url: format!("http://{addr}/rpc"),
        service_addr: addr.to_string(),
        rpc_token: "mock-rpc-token".to_string(),
        web_auth_session_key: "mock-session".to_string(),
        shutdown_tx,
        spawned_service: Arc::new(tokio::sync::Mutex::new(false)),
        missing_ui_html: Arc::new(String::new()),
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    super::rpc_proxy(
        State(state),
        axum::Extension(codexmanager_service::RpcActor::system_admin()),
        headers,
        Bytes::from_static(
            br#"{"jsonrpc":"2.0","id":1,"method":"apikey/create","params":{"retries":99}}"#,
        ),
    )
    .await
}

async fn assert_mock_rpc_was_not_replayed(mock: &RpcFailureMock) {
    assert_eq!(mock.commits.load(Ordering::SeqCst), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), mock.listener.accept())
            .await
            .is_err(),
        "proxy must not send a second request after the mock committed"
    );
}

#[tokio::test]
async fn rpc_proxy_reports_bad_gateway_without_replaying_committed_write_after_lost_response() {
    let mut mock = spawn_rpc_failure_mock(RpcMockFailure::LostResponse).await;
    let addr = mock
        .listener
        .local_addr()
        .expect("RPC failure mock address");
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        call_rpc_proxy_for_mock(
            super::build_service_proxy_client().expect("service proxy client"),
            addr,
        ),
    )
    .await
    .expect("closed upstream must finish the proxy request");
    (&mut mock.ready).await.expect("mock committed the request");
    (&mut mock.task).await.expect("join lost-response mock");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
    assert_mock_rpc_was_not_replayed(&mock).await;
}

#[tokio::test]
async fn rpc_proxy_reports_body_timeout_as_bad_gateway_with_bounded_test_client() {
    let mut mock = spawn_rpc_failure_mock(RpcMockFailure::StalledBody).await;
    let addr = mock
        .listener
        .local_addr()
        .expect("RPC failure mock address");
    // Inject a deadline only into this test client. The production proxy client
    // has no body deadline; this verifies its existing read-error mapping.
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(250))
        .build()
        .expect("bounded test proxy client");
    let response = tokio::time::timeout(
        Duration::from_secs(2),
        call_rpc_proxy_for_mock(client, addr),
    )
    .await
    .expect("bounded test client must finish reading the stalled body");
    (&mut mock.ready).await.expect("mock sent response headers");
    (&mut mock.task).await.expect("join stalled-body mock");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
    let body = axum::body::to_bytes(response.into_body(), 8192)
        .await
        .expect("read proxy error");
    assert!(
        String::from_utf8_lossy(&body).starts_with("upstream read error:"),
        "the timeout must occur after headers, while reading the body"
    );
    assert_mock_rpc_was_not_replayed(&mock).await;
}

#[tokio::test]
async fn rpc_proxy_cancelled_future_releases_stalled_upstream_without_replaying() {
    let mut mock = spawn_rpc_failure_mock(RpcMockFailure::StalledBody).await;
    let addr = mock
        .listener
        .local_addr()
        .expect("RPC failure mock address");
    let mut proxy = tokio::spawn(call_rpc_proxy_for_mock(
        super::build_service_proxy_client().expect("service proxy client"),
        addr,
    ));
    tokio::time::timeout(Duration::from_secs(2), &mut mock.ready)
        .await
        .expect("mock must send response headers within its bound")
        .expect("mock sent response headers");
    assert!(
        tokio::time::timeout(Duration::from_millis(25), &mut proxy)
            .await
            .is_err(),
        "headers without a body must leave the proxy future pending"
    );
    proxy.abort();
    let error = tokio::time::timeout(Duration::from_secs(2), proxy)
        .await
        .expect("cancelled proxy future must finish")
        .expect_err("proxy future must be cancelled");
    assert!(error.is_cancelled());
    (&mut mock.task).await.expect("join cancelled-body mock");
    assert_mock_rpc_was_not_replayed(&mock).await;
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
