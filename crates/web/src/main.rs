#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod auth;
mod embedded_ui;
mod service_gateway;
mod ui_assets;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use rand::RngCore;
use tokio::sync::{watch, Mutex};
use tower_http::services::{ServeDir, ServeFile};

const DEFAULT_WEB_ADDR: &str = "localhost:48761";
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

fn read_env_trim(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn capture_explicit_env(keys: &[&str]) -> Vec<(String, String)> {
    keys.iter()
        .filter_map(|key| read_env_trim(key).map(|value| (key.to_string(), value)))
        .collect()
}

fn restore_env(items: Vec<(String, String)>) {
    for (key, value) in items {
        std::env::set_var(key, value);
    }
}

fn is_running_in_container() -> bool {
    if cfg!(target_os = "linux") {
        if Path::new("/.dockerenv").is_file() || Path::new("/run/.containerenv").is_file() {
            return true;
        }
        if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup") {
            let cgroup = cgroup.to_lowercase();
            if cgroup.contains("docker")
                || cgroup.contains("containerd")
                || cgroup.contains("kubepods")
                || cgroup.contains("podman")
            {
                return true;
            }
        }
    }
    false
}

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

fn resolve_service_addr() -> String {
    read_env_trim("CODEXMANAGER_SERVICE_ADDR")
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| codexmanager_service::DEFAULT_ADDR.to_string())
}

fn resolve_web_addr() -> String {
    read_env_trim("CODEXMANAGER_WEB_ADDR")
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| {
            if is_running_in_container() {
                "0.0.0.0:48761".to_string()
            } else {
                DEFAULT_WEB_ADDR.to_string()
            }
        })
}

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

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn ensure_index_file(index: &Path) -> bool {
    index.is_file()
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().eq_ignore_ascii_case("application/json"))
        .unwrap_or(false)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&#39;")
}

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

async fn async_main() {
    let service_addr = resolve_service_addr();
    let web_addr = resolve_web_addr();
    let web_root = resolve_web_root();
    let index = web_root.join("index.html");

    let rpc_url = format!("http://{service_addr}/rpc");
    let rpc_token = codexmanager_service::rpc_auth_token().to_string();

    let spawned_service: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let spawn_err =
        service_gateway::ensure_service_running(&service_addr, &exe_dir(), &spawned_service).await;

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
        .route("/__quit", get(service_gateway::quit));

    let disk_ok = ensure_index_file(&index);
    let using_explicit_root = read_env_trim("CODEXMANAGER_WEB_ROOT").is_some();
    if using_explicit_root || disk_ok {
        if disk_ok {
            let static_service = ServeDir::new(&web_root).not_found_service(ServeFile::new(index));
            protected_app = protected_app.fallback_service(static_service);
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

    let protected_app = protected_app.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        auth::web_auth_middleware,
    ));
    let app = Router::new()
        .route("/__auth_status", get(auth::auth_status))
        .route("/__login", get(auth::login_page).post(auth::login_submit))
        .route("/__logout", get(auth::logout).post(auth::logout))
        .merge(protected_app)
        .with_state(state);

    println!("codexmanager-web listening on {web_addr} (service={service_addr})");

    let open_url = format!("http://{}", web_addr.trim());
    if read_env_trim("CODEXMANAGER_WEB_NO_OPEN").is_none() {
        let _ = webbrowser::open(&open_url);
    }

    if let Err(err) = run_web_server(&web_addr, app, shutdown_rx).await {
        eprintln!("web stopped: {err}");
        std::process::exit(1);
    }
}

fn main() {
    let preserved_env = if is_running_in_container() {
        capture_explicit_env(&[
            "CODEXMANAGER_WEB_ADDR",
            "CODEXMANAGER_WEB_ROOT",
            "CODEXMANAGER_WEB_NO_OPEN",
            "CODEXMANAGER_WEB_NO_SPAWN_SERVICE",
            "CODEXMANAGER_SERVICE_ADDR",
        ])
    } else {
        Vec::new()
    };

    codexmanager_service::portable::bootstrap_current_process();
    let _ = codexmanager_service::initialize_storage_if_needed();
    codexmanager_service::sync_runtime_settings_from_storage();
    if !preserved_env.is_empty() {
        restore_env(preserved_env);
    }

    let runtime = tokio::runtime::Runtime::new().expect("create tokio runtime");
    runtime.block_on(async_main());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_auth_cookie_is_scoped_by_process_session_key() {
        let password_hash = "sha256$abc$def";
        let rpc_token = "rpc-token";

        let first = auth::build_web_auth_cookie_value(password_hash, rpc_token, "session-a");
        let second = auth::build_web_auth_cookie_value(password_hash, rpc_token, "session-b");

        assert_ne!(first, second);
    }

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
}
