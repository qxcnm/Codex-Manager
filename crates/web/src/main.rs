#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod embedded_ui;

use serde::Deserialize;
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
use tokio::sync::{watch, Mutex};
use tower_http::services::{ServeDir, ServeFile};

const DEFAULT_WEB_ADDR: &str = "localhost:48761";
const WEB_AUTH_COOKIE_NAME: &str = "codexmanager_web_auth";

#[derive(Debug, Deserialize)]
struct LoginForm {
    password: String,
}

#[derive(Clone)]
struct AppState {
    client: reqwest::Client,
    service_rpc_url: String,
    service_addr: String,
    rpc_token: String,
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
    if value.contains(':') {
        return Some(value.to_string());
    }
    Some(format!("localhost:{value}"))
}

fn resolve_service_addr() -> String {
    read_env_trim("CODEXMANAGER_SERVICE_ADDR")
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| codexmanager_service::DEFAULT_ADDR.to_string())
}

fn resolve_web_addr() -> String {
    read_env_trim("CODEXMANAGER_WEB_ADDR")
        .and_then(|v| normalize_addr(&v))
        .unwrap_or_else(|| DEFAULT_WEB_ADDR.to_string())
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

fn builtin_missing_ui_html(detail: &str) -> String {
    let detail = escape_html(detail);
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>CodexManager Web</title>
    <style>
      body {{ font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Helvetica, Arial; padding: 40px; line-height: 1.5; color: #111; }}
      .box {{ max-width: 860px; margin: 0 auto; border: 1px solid #e5e7eb; border-radius: 12px; padding: 20px 24px; background: #fafafa; }}
      h1 {{ margin: 0 0 8px; font-size: 20px; }}
      p {{ margin: 10px 0; color: #374151; }}
      code {{ background: #111827; color: #f9fafb; padding: 2px 6px; border-radius: 6px; }}
      a {{ color: #2563eb; }}
    </style>
  </head>
  <body>
    <div class="box">
      <h1>前端资源未就绪</h1>
      <p>当前 <code>codexmanager-web</code> 没有找到可用的前端静态资源。</p>
      <p>详情：<code>{detail}</code></p>
      <p>解决方式：</p>
      <p>1) 使用官方发行物（已内置前端资源）；或</p>
      <p>2) 从源码运行：先执行 <code>pnpm -C apps build</code>，再设置 <code>CODEXMANAGER_WEB_ROOT=.../apps/dist</code> 启动。</p>
      <p>关闭：访问 <a href="/__quit">/__quit</a>。</p>
    </div>
  </body>
</html>
"#
    )
}

fn builtin_login_html(error: Option<&str>) -> String {
    let error_html = error
        .map(|text| format!(r#"<div class="error">{}</div>"#, escape_html(text)))
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>CodexManager Web 登录</title>
    <style>
      :root {{
        color-scheme: light;
        --bg: #eef3f8;
        --panel: rgba(255,255,255,.92);
        --text: #142033;
        --muted: #627389;
        --accent: #0f6fff;
        --accent-strong: #0a57ca;
        --border: rgba(20,32,51,.12);
        --error-bg: rgba(193, 45, 45, .1);
        --error-fg: #b42318;
      }}
      * {{ box-sizing: border-box; }}
      body {{
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 24px;
        font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
        background:
          radial-gradient(circle at top left, rgba(15,111,255,.18), transparent 32%),
          radial-gradient(circle at bottom right, rgba(45,164,78,.14), transparent 26%),
          linear-gradient(160deg, #f6f9fc 0%, #e8eef6 100%);
        color: var(--text);
      }}
      .card {{
        width: min(100%, 420px);
        padding: 28px;
        border: 1px solid var(--border);
        border-radius: 20px;
        background: var(--panel);
        box-shadow: 0 24px 60px rgba(15, 23, 42, .12);
        backdrop-filter: blur(14px);
      }}
      .mark {{
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 44px;
        height: 44px;
        border-radius: 14px;
        background: linear-gradient(135deg, #0f6fff, #2bb673);
        color: #fff;
        font-weight: 700;
      }}
      h1 {{ margin: 16px 0 6px; font-size: 22px; }}
      p {{ margin: 0 0 18px; color: var(--muted); line-height: 1.6; }}
      label {{ display: block; margin-bottom: 10px; font-size: 14px; color: var(--muted); }}
      input {{
        width: 100%;
        border: 1px solid rgba(20,32,51,.16);
        border-radius: 14px;
        padding: 13px 14px;
        font-size: 15px;
        outline: none;
        background: rgba(255,255,255,.92);
      }}
      input:focus {{
        border-color: rgba(15,111,255,.58);
        box-shadow: 0 0 0 4px rgba(15,111,255,.12);
      }}
      button {{
        width: 100%;
        margin-top: 16px;
        border: 0;
        border-radius: 14px;
        padding: 13px 16px;
        font-size: 15px;
        font-weight: 600;
        color: #fff;
        background: linear-gradient(135deg, var(--accent), var(--accent-strong));
        cursor: pointer;
      }}
      button:hover {{ filter: brightness(.98); }}
      .error {{
        margin-bottom: 14px;
        padding: 12px 14px;
        border-radius: 12px;
        background: var(--error-bg);
        color: var(--error-fg);
        font-size: 14px;
      }}
      .foot {{
        margin-top: 14px;
        font-size: 12px;
        color: var(--muted);
        text-align: center;
      }}
    </style>
  </head>
  <body>
    <form class="card" method="post" action="/__login">
      <div class="mark">CM</div>
      <h1>访问受保护</h1>
      <p>当前 CodexManager Web 已启用访问密码，请先验证后再进入管理页面。</p>
      {error_html}
      <label for="password">访问密码</label>
      <input id="password" name="password" type="password" autocomplete="current-password" autofocus />
      <button type="submit">进入控制台</button>
      <div class="foot">密码可在桌面端右上角安全入口或设置页中修改。</div>
    </form>
  </body>
</html>
"#
    )
}

fn should_spawn_service() -> bool {
    // 默认允许（双击就能用）。容器/特殊场景可通过该变量禁用。
    read_env_trim("CODEXMANAGER_WEB_NO_SPAWN_SERVICE").is_none()
}

async fn tcp_probe(addr: &str) -> bool {
    let addr = addr.trim();
    if addr.is_empty() {
        return false;
    }
    let addr = addr.strip_prefix("http://").unwrap_or(addr);
    let addr = addr.strip_prefix("https://").unwrap_or(addr);
    let addr = addr.split('/').next().unwrap_or(addr);
    tokio::time::timeout(
        Duration::from_millis(250),
        tokio::net::TcpStream::connect(addr),
    )
    .await
    .is_ok()
}

fn service_bin_path(dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        return dir.join("codexmanager-service.exe");
    }
    #[cfg(not(target_os = "windows"))]
    {
        return dir.join("codexmanager-service");
    }
}

fn spawn_service_detached(dir: &Path, service_addr: &str) -> std::io::Result<()> {
    let bin = service_bin_path(dir);
    let mut cmd = Command::new(bin);
    let bind_addr = codexmanager_service::listener_bind_addr(service_addr);
    cmd.env("CODEXMANAGER_SERVICE_ADDR", bind_addr);

    #[cfg(target_os = "windows")]
    {
        // 中文注释：从 web 双击启动时，不弹出 service 控制台窗口；用户也可以单独双击 service.exe 看控制台。
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let _child = cmd.spawn()?;
    Ok(())
}

async fn ensure_service_running(
    service_addr: &str,
    dir: &Path,
    spawned_service: &Arc<Mutex<bool>>,
) -> Option<String> {
    if tcp_probe(service_addr).await {
        return None;
    }
    if !should_spawn_service() {
        return Some(format!(
            "service not reachable at {service_addr} (spawn disabled)"
        ));
    }

    let bin = service_bin_path(dir);
    if !bin.is_file() {
        return Some(format!(
            "service not reachable at {service_addr} (missing {})",
            bin.display()
        ));
    }

    if let Err(err) = spawn_service_detached(dir, service_addr) {
        return Some(format!("failed to spawn service: {err}"));
    }
    *spawned_service.lock().await = true;

    for _ in 0..50 {
        if tcp_probe(service_addr).await {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Some(format!(
        "service still not reachable at {service_addr} after spawn"
    ))
}

async fn rpc_proxy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !is_json_content_type(&headers) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}").into_response();
    }
    let resp = state
        .client
        .post(&state.service_rpc_url)
        .header("content-type", "application/json")
        .header("x-codexmanager-rpc-token", &state.rpc_token)
        .body(body)
        .send()
        .await;
    let resp = match resp {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = resp.status();
    let bytes = match resp.bytes().await {
        Ok(v) => v,
        Err(err) => {
            let msg = format!("upstream read error: {err}");
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };
    let mut out = Response::new(axum::body::Body::from(bytes));
    *out.status_mut() = status;
    out.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_static("application/json"),
    );
    out
}

fn current_web_access_password_hash() -> Option<String> {
    codexmanager_service::current_web_access_password_hash()
}

fn build_web_auth_cookie_value(password_hash: &str, rpc_token: &str) -> String {
    codexmanager_service::build_web_access_session_token(password_hash, rpc_token)
}

fn parse_cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|segment| {
        let (name, value) = segment.trim().split_once('=')?;
        if name.trim() == cookie_name {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn set_cookie_header_value(value: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{WEB_AUTH_COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Lax"
    ))
    .ok()
}

fn clear_cookie_header_value() -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{WEB_AUTH_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
    ))
    .ok()
}

fn request_is_authenticated(headers: &HeaderMap, state: &AppState) -> bool {
    let Some(password_hash) = current_web_access_password_hash() else {
        return true;
    };
    let Some(cookie_value) = parse_cookie_value(headers, WEB_AUTH_COOKIE_NAME) else {
        return false;
    };
    let expected = build_web_auth_cookie_value(&password_hash, &state.rpc_token);
    cookie_value == expected
}

async fn web_auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    if path == "/__login" || path == "/__logout" {
        return next.run(request).await;
    }
    if request_is_authenticated(request.headers(), state.as_ref()) {
        return next.run(request).await;
    }
    if path.starts_with("/api/") {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "web_auth_required" })),
        )
            .into_response();
    }
    Redirect::to("/__login").into_response()
}

async fn login_page(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if current_web_access_password_hash().is_none()
        || request_is_authenticated(&headers, state.as_ref())
    {
        return Redirect::to("/").into_response();
    }
    Html(builtin_login_html(None)).into_response()
}

async fn login_submit(
    State(state): State<Arc<AppState>>,
    axum::Form(form): axum::Form<LoginForm>,
) -> impl IntoResponse {
    let Some(password_hash) = current_web_access_password_hash() else {
        return Redirect::to("/").into_response();
    };
    if !codexmanager_service::verify_web_access_password(&form.password) {
        return (
            StatusCode::UNAUTHORIZED,
            Html(builtin_login_html(Some("密码错误，请重试。"))),
        )
            .into_response();
    }
    let token = build_web_auth_cookie_value(&password_hash, &state.rpc_token);
    let mut response = Redirect::to("/").into_response();
    if let Some(header_value) = set_cookie_header_value(&token) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, header_value);
    }
    response
}

async fn logout() -> impl IntoResponse {
    let mut response = Redirect::to("/__login").into_response();
    if let Some(header_value) = clear_cookie_header_value() {
        response
            .headers_mut()
            .append(header::SET_COOKIE, header_value);
    }
    response
}

async fn quit(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if *state.spawned_service.lock().await {
        let addr = state.service_addr.clone();
        let _ = tokio::task::spawn_blocking(move || {
            codexmanager_service::request_shutdown(&addr);
        })
        .await;
    }
    let _ = state.shutdown_tx.send(true);
    Html("<html><body>OK</body></html>")
}

async fn serve_missing_ui(State(state): State<Arc<AppState>>) -> Html<String> {
    Html((*state.missing_ui_html).clone())
}

async fn serve_embedded_index() -> Response {
    serve_embedded_path("index.html")
}

async fn serve_embedded_asset(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    serve_embedded_path(&path)
}

fn serve_embedded_path(path: &str) -> Response {
    let raw = path.trim_start_matches('/');
    if raw.contains("..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }

    let wanted = if raw.is_empty() { "index.html" } else { raw };
    let bytes = embedded_ui::read_asset_bytes(wanted)
        .or_else(|| embedded_ui::read_asset_bytes("index.html"));
    let Some(bytes) = bytes else {
        return (StatusCode::NOT_FOUND, "missing ui").into_response();
    };
    let mime = embedded_ui::guess_mime(wanted);

    let mut out = Response::new(axum::body::Body::from(bytes));
    out.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_str(&mime)
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream")),
    );
    out
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
    // 中文注释：localhost 在 Windows/macOS 上可能只解析到单栈；双栈监听可减少连接差异。
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

#[tokio::main]
async fn main() {
    // 先加载同目录 env / 默认 DB / RPC token 文件，做到“解压即用”。
    codexmanager_service::portable::bootstrap_current_process();
    let _ = codexmanager_service::initialize_storage_if_needed();
    codexmanager_service::sync_runtime_settings_from_storage();

    let service_addr = resolve_service_addr();
    let web_addr = resolve_web_addr();
    let web_root = resolve_web_root();
    let index = web_root.join("index.html");

    let rpc_url = format!("http://{service_addr}/rpc");
    let rpc_token = codexmanager_service::rpc_auth_token().to_string();

    let spawned_service: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let spawn_err = ensure_service_running(&service_addr, &exe_dir(), &spawned_service).await;

    let mut missing_detail = format!(
        "web root invalid: {} (index.html missing)",
        web_root.display()
    );
    if let Some(err) = spawn_err {
        missing_detail = format!("{missing_detail}; {err}");
    }
    let missing_ui_html = Arc::new(builtin_missing_ui_html(&missing_detail));

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let state = Arc::new(AppState {
        client: reqwest::Client::new(),
        service_rpc_url: rpc_url,
        service_addr: service_addr.clone(),
        rpc_token,
        shutdown_tx,
        spawned_service: spawned_service.clone(),
        missing_ui_html,
    });

    let mut protected_app = Router::new()
        .route("/api/rpc", post(rpc_proxy))
        .route("/__quit", get(quit));

    // 静态资源：优先磁盘（显式 root 或同目录 web/ 存在），否则使用内嵌资源（embedded-ui）。
    let disk_ok = ensure_index_file(&index);
    let using_explicit_root = read_env_trim("CODEXMANAGER_WEB_ROOT").is_some();
    if using_explicit_root || disk_ok {
        if disk_ok {
            let static_service = ServeDir::new(&web_root).not_found_service(ServeFile::new(index));
            protected_app = protected_app.fallback_service(static_service);
        } else {
            protected_app = protected_app
                .route("/", get(serve_missing_ui))
                .route("/{*path}", get(serve_missing_ui));
        }
    } else if embedded_ui::has_embedded_ui() {
        protected_app = protected_app
            .route("/", get(serve_embedded_index))
            .route("/{*path}", get(serve_embedded_asset));
    } else {
        protected_app = protected_app
            .route("/", get(serve_missing_ui))
            .route("/{*path}", get(serve_missing_ui));
    }

    let protected_app = protected_app.layer(axum::middleware::from_fn_with_state(
        state.clone(),
        web_auth_middleware,
    ));
    let app = Router::new()
        .route("/__login", get(login_page).post(login_submit))
        .route("/__logout", get(logout))
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
