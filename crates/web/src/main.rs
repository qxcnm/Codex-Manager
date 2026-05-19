#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod auth;
mod embedded_ui;
mod service_gateway;
mod ui_assets;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::{net::Ipv4Addr, net::ToSocketAddrs};

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use rand::RngCore;
use tokio::sync::{watch, Mutex};
use tower_http::services::{ServeDir, ServeFile};

const WEB_AUTH_COOKIE_NAME: &str = "codexmanager_web_auth";

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    service_rpc_url: String,
    service_addr: String,
    rpc_token: String,
    web_auth_session_key: String,
    shutdown_tx: watch::Sender<bool>,
    spawned_service: Arc<Mutex<bool>>,
    missing_ui_html: Arc<String>,
}

/// 函数 `read_env_trim`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn read_env_trim(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// 函数 `normalize_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_addr(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix("http://") {
        value = rest;
    }
    if let Some(rest) = value.strip_prefix("https://") {
        value = rest;
    }
    value = value.split('/').next().unwrap_or(value);
    if value.is_empty() {
        return None;
    }
    if value.parse::<u16>().is_ok() {
        return Some(format!("localhost:{value}"));
    }
    Some(value.to_string())
}

async fn ui_document_cache_middleware(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    ui_assets::append_no_store_for_html_documents(&mut response);
    response
}

/// 函数 `normalize_connect_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_connect_addr(raw: &str) -> Option<String> {
    let normalized = normalize_addr(raw)?;
    let Some((host, port)) = normalized.rsplit_once(':') else {
        return Some(rewrite_linux_docker_host_addr(normalized));
    };
    let normalized = match host {
        "0.0.0.0" | "::" | "[::]" => Some(format!("localhost:{port}")),
        _ => Some(normalized),
    }?;
    Some(rewrite_linux_docker_host_addr(normalized))
}

/// 函数 `browser_open_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn browser_open_addr(raw: &str) -> Option<String> {
    let normalized = normalize_addr(raw)?;
    let Some((host, port)) = normalized.rsplit_once(':') else {
        return Some(normalized);
    };
    match host {
        "0.0.0.0" | "::" | "[::]" => Some(format!("127.0.0.1:{port}")),
        _ => Some(normalized),
    }
}

/// 函数 `resolve_service_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn resolve_service_addr() -> String {
    read_env_trim("CODEXMANAGER_SERVICE_ADDR")
        .and_then(|v| normalize_connect_addr(&v))
        .unwrap_or_else(|| codexmanager_service::DEFAULT_ADDR.to_string())
}

/// 函数 `rewrite_linux_docker_host_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-10
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "linux")]
fn rewrite_linux_docker_host_addr(addr: String) -> String {
    let Some((host, port)) = addr.rsplit_once(':') else {
        return addr;
    };
    if !host.eq_ignore_ascii_case("host.docker.internal") {
        return addr;
    }
    if addr
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .is_some()
    {
        return addr;
    }
    linux_default_gateway_ipv4()
        .map(|gateway| format!("{gateway}:{port}"))
        .unwrap_or(addr)
}

/// 函数 `rewrite_linux_docker_host_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-10
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[cfg(not(target_os = "linux"))]
fn rewrite_linux_docker_host_addr(addr: String) -> String {
    addr
}

/// 函数 `linux_default_gateway_ipv4`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-10
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "linux")]
fn linux_default_gateway_ipv4() -> Option<Ipv4Addr> {
    let routes = std::fs::read_to_string("/proc/net/route").ok()?;
    parse_linux_default_gateway_ipv4(&routes)
}

/// 函数 `parse_linux_default_gateway_ipv4`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-10
///
/// # 参数
/// - routes: 参数 routes
///
/// # 返回
/// 返回函数执行结果
#[cfg(target_os = "linux")]
fn parse_linux_default_gateway_ipv4(routes: &str) -> Option<Ipv4Addr> {
    for line in routes.lines().skip(1) {
        let columns = line.split_whitespace().collect::<Vec<_>>();
        if columns.len() < 3 || columns[1] != "00000000" {
            continue;
        }
        let gateway = u32::from_str_radix(columns[2], 16).ok()?;
        let bytes = gateway.to_le_bytes();
        return Some(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]));
    }
    None
}

/// 函数 `resolve_web_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn resolve_web_addr() -> String {
    read_env_trim("CODEXMANAGER_WEB_ADDR")
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(codexmanager_service::default_web_listener_addr)
}

/// 函数 `resolve_web_root`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn resolve_web_root() -> PathBuf {
    if let Some(v) = read_env_trim("CODEXMANAGER_WEB_ROOT") {
        let p = PathBuf::from(v);
        if p.is_absolute() {
            return p;
        }
        return exe_dir().join(p);
    }
    exe_dir().join("web")
}

/// 函数 `exe_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 函数 `ensure_index_file`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - index: 参数 index
///
/// # 返回
/// 返回函数执行结果
fn ensure_index_file(index: &Path) -> bool {
    index.is_file()
}

/// 函数 `is_json_content_type`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

/// 函数 `escape_html`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - text: 参数 text
///
/// # 返回
/// 返回函数执行结果
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}

/// 函数 `runtime_info`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
async fn runtime_info() -> impl IntoResponse {
    let author_content_url = read_env_trim("CODEXMANAGER_AUTHOR_CONTENT_URL").unwrap_or_default();
    Json(serde_json::json!({
        "mode": "web-gateway",
        "rpcBaseUrl": "/api/rpc",
        "authorContentUrl": author_content_url,
        "canManageService": false,
        "canSelfUpdate": false,
        "canCloseToTray": false,
        "canOpenLocalDir": false,
        "canUseBrowserFileImport": true,
        "canUseBrowserDownloadExport": true
    }))
}

/// 函数 `serve_on_listener`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - listener: 参数 listener
/// - app: 参数 app
/// - shutdown_rx: 参数 shutdown_rx
///
/// # 返回
/// 返回函数执行结果
async fn serve_on_listener(
    listener: tokio::net::TcpListener,
    app: Router,
    mut shutdown_rx: watch::Receiver<bool>,
) -> std::io::Result<()> {
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            while !*shutdown_rx.borrow() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))
}

/// 函数 `run_web_server`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - app: 参数 app
/// - shutdown_rx: 参数 shutdown_rx
///
/// # 返回
/// 返回函数执行结果
async fn run_web_server(
    addr: &str,
    app: Router,
    shutdown_rx: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let trimmed = addr.trim();
    if trimmed.len() > "localhost:".len()
        && trimmed[..("localhost:".len())].eq_ignore_ascii_case("localhost:")
    {
        let port = &trimmed["localhost:".len()..];
        let v4 = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await;
        let v6 = tokio::net::TcpListener::bind(format!("[::1]:{port}")).await;
        return match (v4, v6) {
            (Ok(v4_listener), Ok(v6_listener)) => {
                let v4_task = serve_on_listener(v4_listener, app.clone(), shutdown_rx.clone());
                let v6_task = serve_on_listener(v6_listener, app, shutdown_rx);
                let (v4_result, v6_result) = tokio::join!(v4_task, v6_task);
                v4_result.and(v6_result)
            }
            (Ok(listener), Err(_)) | (Err(_), Ok(listener)) => {
                serve_on_listener(listener, app, shutdown_rx).await
            }
            (Err(err), Err(_)) => Err(err),
        };
    }

    let listener = tokio::net::TcpListener::bind(trimmed).await?;
    serve_on_listener(listener, app, shutdown_rx).await
}

/// 函数 `async_main`
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
async fn async_main() {
    let service_addr = resolve_service_addr();
    let web_addr = resolve_web_addr();
    let web_root = resolve_web_root();
    let index = web_root.join("index.html");

    let rpc_url = format!("http://{service_addr}/rpc");
    let rpc_token = codexmanager_service::rpc_auth_token().to_string();

    let spawned_service: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let spawn_err = service_gateway::ensure_service_running(
        &service_addr,
        &rpc_token,
        &exe_dir(),
        &spawned_service,
    )
    .await;

    let mut missing_detail = format!(
        "web root invalid: {} (index.html missing)",
        web_root.display()
    );
    if let Some(err) = spawn_err {
        missing_detail = format!("{missing_detail}; {err}");
    }
    let missing_ui_html = Arc::new(ui_assets::builtin_missing_ui_html(&missing_detail));

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let state = Arc::new(AppState {
        client: reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new()),
        service_rpc_url: rpc_url,
        service_addr: service_addr.clone(),
        rpc_token,
        web_auth_session_key: auth::generate_web_auth_session_key(),
        shutdown_tx,
        spawned_service: spawned_service.clone(),
        missing_ui_html,
    });

    let mut protected_app = Router::new()
        .route("/api/rpc", post(service_gateway::rpc_proxy))
        .route(
            "/api/events/usage-refresh",
            get(service_gateway::usage_refresh_events),
        )
        .route("/__quit", get(service_gateway::quit));

    let disk_ok = ensure_index_file(&index);
    let using_explicit_root = read_env_trim("CODEXMANAGER_WEB_ROOT").is_some();
    if using_explicit_root || disk_ok {
        if disk_ok {
            let static_service = ServeDir::new(&web_root)
                .append_index_html_on_directories(true)
                .not_found_service(ServeFile::new(index));
            protected_app = protected_app
                .nest_service("/_next", ServeDir::new(web_root.join("_next")))
                .fallback_service(static_service);
        } else {
            protected_app = protected_app
                .route("/", get(ui_assets::serve_missing_ui))
                .route("/{*path}", get(ui_assets::serve_missing_ui));
        }
    } else if embedded_ui::has_embedded_ui() {
        protected_app = protected_app
            .route("/", get(ui_assets::serve_embedded_index))
            .route("/{*path}", get(ui_assets::serve_embedded_asset));
    } else {
        protected_app = protected_app
            .route("/", get(ui_assets::serve_missing_ui))
            .route("/{*path}", get(ui_assets::serve_missing_ui));
    }

    let protected_app = protected_app
        .layer(axum::middleware::from_fn(ui_document_cache_middleware))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::web_auth_middleware,
        ));
    let app = Router::new()
        .route("/health", get(service_gateway::gateway_proxy))
        .route("/metrics", get(service_gateway::gateway_proxy))
        .route("/auth/callback", get(service_gateway::gateway_proxy))
        .route(
            "/auth/callback/{*path}",
            get(service_gateway::gateway_proxy),
        )
        .route("/v1", any(service_gateway::gateway_proxy))
        .route("/v1/{*path}", any(service_gateway::gateway_proxy))
        .route("/v1alpha/{*path}", any(service_gateway::gateway_proxy))
        .route("/v1beta/{*path}", any(service_gateway::gateway_proxy))
        .route(
            "/v1internal:generateContent",
            any(service_gateway::gateway_proxy),
        )
        .route(
            "/v1internal:streamGenerateContent",
            any(service_gateway::gateway_proxy),
        )
        .route(
            "/v1internal:countTokens",
            any(service_gateway::gateway_proxy),
        )
        .route("/api/runtime", get(runtime_info))
        .route("/api/author-content", get(service_gateway::author_content))
        .route("/__auth_status", get(auth::auth_status))
        .route("/__login", get(auth::login_page).post(auth::login_submit))
        .route("/__logout", get(auth::logout).post(auth::logout))
        .merge(protected_app)
        .with_state(state);

    println!("codexmanager-web listening on {web_addr} (service={service_addr})");

    let open_url = format!("http://{}", web_addr.trim());
    let open_url = browser_open_addr(&web_addr)
        .map(|addr| format!("http://{addr}"))
        .unwrap_or(open_url);
    if read_env_trim("CODEXMANAGER_WEB_NO_OPEN").is_none() {
        let _ = webbrowser::open(&open_url);
    }

    if let Err(err) = run_web_server(&web_addr, app, shutdown_rx).await {
        eprintln!("web stopped: {err}");
        std::process::exit(1);
    }
}

/// 函数 `main`
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
fn main() {
    codexmanager_service::portable::bootstrap_current_process();
    codexmanager_service::init_logging();
    let _ = codexmanager_service::initialize_storage_if_needed();
    codexmanager_service::sync_runtime_settings_from_storage();

    let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");
    runtime.block_on(async_main());
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    /// 函数 `web_auth_cookie_is_scoped_by_process_session_key`
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
    fn web_auth_cookie_is_scoped_by_process_session_key() {
        let password_hash = "sha256$abc$def";
        let rpc_token = "rpc-token";

        let first = auth::build_web_auth_cookie_value(password_hash, rpc_token, "session-a");
        let second = auth::build_web_auth_cookie_value(password_hash, rpc_token, "session-b");

        assert_ne!(first, second);
    }

    /// 函数 `parse_cookie_value_returns_matching_cookie`
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
    fn parse_cookie_value_returns_matching_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; codexmanager_web_auth=token-123; b=2"),
        );

        let actual = auth::parse_cookie_value(&headers, WEB_AUTH_COOKIE_NAME);

        assert_eq!(actual.as_deref(), Some("token-123"));
    }

    /// 函数 `normalize_connect_addr_maps_all_interfaces_to_localhost`
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
    fn normalize_connect_addr_maps_all_interfaces_to_localhost() {
        assert_eq!(
            normalize_connect_addr("0.0.0.0:48760").as_deref(),
            Some("localhost:48760")
        );
        assert_eq!(
            normalize_connect_addr("[::]:48760").as_deref(),
            Some("localhost:48760")
        );
        assert_eq!(
            normalize_connect_addr("192.168.1.8:48760").as_deref(),
            Some("192.168.1.8:48760")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_linux_default_gateway_ipv4_reads_default_route() {
        let routes = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\neth0\t00000000\t010011AC\t0003\t0\t0\t0\t00000000\t0\t0\t0\n";
        assert_eq!(
            parse_linux_default_gateway_ipv4(routes),
            Some(Ipv4Addr::new(172, 17, 0, 1))
        );
    }

    /// 函数 `browser_open_addr_maps_all_interfaces_to_loopback`
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
    fn browser_open_addr_maps_all_interfaces_to_loopback() {
        assert_eq!(
            browser_open_addr("0.0.0.0:48761").as_deref(),
            Some("127.0.0.1:48761")
        );
        assert_eq!(
            browser_open_addr("[::]:48761").as_deref(),
            Some("127.0.0.1:48761")
        );
        assert_eq!(
            browser_open_addr("192.168.1.8:48761").as_deref(),
            Some("192.168.1.8:48761")
        );
    }

    /// 函数 `runtime_info_reports_web_gateway_capabilities`
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
    #[tokio::test]
    async fn runtime_info_reports_web_gateway_capabilities() {
        let response = runtime_info().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read runtime response body");
        let payload: serde_json::Value =
            serde_json::from_slice(&bytes).expect("parse runtime response json");

        assert_eq!(payload["mode"], "web-gateway");
        assert_eq!(payload["rpcBaseUrl"], "/api/rpc");
        assert_eq!(payload["authorContentUrl"], "");
        assert_eq!(payload["canManageService"], false);
        assert_eq!(payload["canSelfUpdate"], false);
        assert_eq!(payload["canCloseToTray"], false);
        assert_eq!(payload["canOpenLocalDir"], false);
        assert_eq!(payload["canUseBrowserFileImport"], true);
        assert_eq!(payload["canUseBrowserDownloadExport"], true);
    }
}
