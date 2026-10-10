use std::collections::HashMap;
use std::io;
use std::net::TcpListener;
#[cfg(test)]
use tiny_http::Header;
#[cfg(test)]
use tiny_http::Request;
#[cfg(test)]
use tiny_http::Response;
use url::Url;

#[cfg(test)]
use crate::auth_tokens::complete_login;
use crate::storage_helpers::open_storage;

/// 函数 `resolve_redirect_uri`
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
pub(crate) fn resolve_redirect_uri() -> Option<String> {
    // 优先使用显式配置的回调地址
    if let Ok(uri) = std::env::var("CODEXMANAGER_REDIRECT_URI") {
        if let Ok(url) = Url::parse(&uri) {
            let host = url.host_str().unwrap_or("localhost");
            let port = url.port_or_known_default().unwrap_or(1455);
            let _ = ensure_login_server_with_addr(&format!("{host}:{port}"));
        }
        return Some(uri);
    }
    let info = ensure_login_server().ok()?;
    Some(format!("http://localhost:{}/auth/callback", info.port))
}

/// 函数 `handle_login_request`
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
#[cfg(test)]
pub(crate) fn handle_login_request(request: Request) -> Result<(), String> {
    // 解析回调地址与参数
    let url = Url::parse(&format!("http://localhost{}", request.url()))
        .map_err(|e| format!("invalid url: {e}"))?;
    if url.path() != "/auth/callback" {
        let _ = request.respond(Response::from_string("Not Found").with_status_code(404));
        return Ok(());
    }

    // 完成登录流程并响应浏览器
    let result = process_login_callback_url(request.url());
    match result {
        Ok(_) => {
            let _ = request.respond(html_response(build_callback_success_page()));
        }
        Err(err) => {
            let _ = request
                .respond(html_response(build_callback_error_page(&err)).with_status_code(500));
        }
    }
    Ok(())
}

/// Process an OAuth callback URL without coupling the login flow to a
/// particular HTTP server.  Both the dedicated OAuth listener and the service router use this
/// function so callback validation and state handling remain identical.
#[cfg(test)]
pub(crate) fn process_login_callback_url(raw_url: &str) -> Result<(), String> {
    let url = Url::parse(&format!("http://localhost{raw_url}"))
        .map_err(|e| format!("invalid url: {e}"))?;
    if url.path() != "/auth/callback" {
        return Err("not found".to_string());
    }
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    handle_login_callback_query(&params)
}

/// Native listeners await token exchange; only the legacy Storage preflight
/// is scheduled as short bounded blocking work.
pub(crate) async fn process_login_callback_url_async(raw_url: &str) -> Result<(), String> {
    let url = Url::parse(&format!("http://localhost{raw_url}"))
        .map_err(|e| format!("invalid url: {e}"))?;
    if url.path() != "/auth/callback" {
        return Err("not found".to_owned());
    }
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let (code, state) =
        crate::auth_tokens::run_auth_storage(move || prepare_login_callback_query(&params)).await?;
    crate::auth_tokens::complete_login_async(&state, &code)
        .await
        .map_err(|err| {
            if err == "unknown login session" {
                "State mismatch or expired login session.".to_owned()
            } else {
                err
            }
        })
}

pub(crate) fn callback_success_page() -> String {
    build_callback_success_page()
}

pub(crate) fn callback_error_page(err: &str) -> String {
    build_callback_error_page(err)
}

/// 函数 `handle_login_callback_query`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - params: 参数 params
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn handle_login_callback_query(params: &HashMap<String, String>) -> Result<(), String> {
    let (code, state) = prepare_login_callback_query(params)?;
    handle_login_callback_params(&code, &state)
}

fn prepare_login_callback_query(
    params: &HashMap<String, String>,
) -> Result<(String, String), String> {
    let state = params
        .get("state")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(error_code) = params
        .get("error")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let error_description = params
            .get("error_description")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let message = oauth_callback_error_message(error_code, error_description);
        update_login_session_failed(state, &message);
        return Err(message);
    }

    let state =
        state.ok_or_else(|| "Missing login state. Sign-in could not be completed.".to_string())?;
    ensure_login_session_exists(state)?;
    let code = params
        .get("code")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            let message = "Missing authorization code. Sign-in could not be completed.".to_string();
            update_login_session_failed(Some(state), &message);
            message
        })?;
    Ok((code.to_owned(), state.to_owned()))
}

/// 函数 `handle_login_callback_params`
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
#[cfg(test)]
pub(crate) fn handle_login_callback_params(code: &str, state: &str) -> Result<(), String> {
    complete_login(state, code).map_err(|err| {
        if err == "unknown login session" {
            "State mismatch or expired login session.".to_string()
        } else {
            err
        }
    })
}

/// 函数 `ensure_login_session_exists`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - state: 参数 state
///
/// # 返回
/// 返回函数执行结果
fn ensure_login_session_exists(state: &str) -> Result<(), String> {
    let Some(storage) = open_storage() else {
        return Err("storage unavailable".to_string());
    };
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    match storage
        .get_login_session(state)
        .map_err(|e| e.to_string())?
    {
        Some(_) => Ok(()),
        None => Err("State mismatch or expired login session.".to_string()),
    }
}

/// 函数 `update_login_session_failed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - state: 参数 state
/// - error: 参数 error
///
/// # 返回
/// 无
fn update_login_session_failed(state: Option<&str>, error: &str) {
    let Some(state) = state else {
        return;
    };
    let Some(storage) = open_storage() else {
        return;
    };
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let _ = storage.fail_pending_login_session(state, Some(error));
}

/// 函数 `is_missing_codex_entitlement_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - error_code: 参数 error_code
/// - error_description: 参数 error_description
///
/// # 返回
/// 返回函数执行结果
fn is_missing_codex_entitlement_error(error_code: &str, error_description: Option<&str>) -> bool {
    error_code == "access_denied"
        && error_description.is_some_and(|description| {
            description
                .to_ascii_lowercase()
                .contains("missing_codex_entitlement")
        })
}

/// 函数 `oauth_callback_error_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - error_code: 参数 error_code
/// - error_description: 参数 error_description
///
/// # 返回
/// 返回函数执行结果
fn oauth_callback_error_message(error_code: &str, error_description: Option<&str>) -> String {
    if is_missing_codex_entitlement_error(error_code, error_description) {
        return "Codex is not enabled for your workspace. Contact your workspace administrator to request access to Codex.".to_string();
    }

    if let Some(description) = error_description {
        if !description.trim().is_empty() {
            return format!("Sign-in failed: {description}");
        }
    }

    format!("Sign-in failed: {error_code}")
}

#[derive(Clone, Debug)]
pub(crate) struct LoginServerInfo {
    port: u16,
}

struct LoginServerState {
    info: LoginServerInfo,
    task: tokio::task::JoinHandle<()>,
}

static LOGIN_SERVER_STATE: std::sync::OnceLock<std::sync::Mutex<Option<LoginServerState>>> =
    std::sync::OnceLock::new();

pub(crate) async fn drain_login_server() {
    let task = LOGIN_SERVER_STATE.get().and_then(|cell| {
        crate::lock_utils::lock_recover(cell, "login_server_state")
            .take()
            .map(|state| state.task)
    });
    if let Some(task) = task {
        let _ = task.await;
    }
}

/// 函数 `ensure_login_server`
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
pub(crate) fn ensure_login_server() -> Result<LoginServerInfo, String> {
    let addr =
        std::env::var("CODEXMANAGER_LOGIN_ADDR").unwrap_or_else(|_| "localhost:1455".to_string());
    ensure_login_server_with_addr(&addr)
}

/// 函数 `ensure_login_server_with_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
fn ensure_login_server_with_addr(addr: &str) -> Result<LoginServerInfo, String> {
    let cell = LOGIN_SERVER_STATE.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = crate::lock_utils::lock_recover(cell, "login_server_state");
    if crate::shutdown_requested() {
        return Err("OAuth listener rejected during shutdown".to_owned());
    }
    if let Some(state) = guard.as_ref().filter(|state| !state.task.is_finished()) {
        return Ok(state.info.clone());
    }
    let (servers, info) = bind_login_server(addr)?;
    let runtime = crate::runtime::service_runtime::process_runtime()?;
    for server in &servers {
        server
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
    }
    let task = runtime.spawn(async move {
        let tasks = servers.into_iter().map(run_login_server);
        for result in futures_util::future::join_all(tasks).await {
            if result.is_err() {
                log::warn!("event=oauth_listener_failed");
            }
        }
    });
    *guard = Some(LoginServerState {
        info: info.clone(),
        task,
    });
    Ok(info)
}

/// 函数 `is_loopback_host`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - host: 参数 host
///
/// # 返回
/// 返回函数执行结果
fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// 函数 `allow_non_loopback_login_addr`
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
fn allow_non_loopback_login_addr() -> bool {
    matches!(
        std::env::var("CODEXMANAGER_ALLOW_NON_LOOPBACK_LOGIN_ADDR")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

/// 函数 `server_port`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - server: 参数 server
///
/// # 返回
/// 返回函数执行结果
fn server_port(server: &TcpListener) -> Result<u16, String> {
    server
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| error.to_string())
}

/// 函数 `try_bind_login_server`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - servers: 参数 servers
/// - addr_in_use: 参数 addr_in_use
/// - last_err: 参数 last_err
///
/// # 返回
/// 返回函数执行结果
fn try_bind_login_server(
    addr: &str,
    servers: &mut Vec<TcpListener>,
    addr_in_use: &mut bool,
    last_err: &mut Option<String>,
) -> Result<Option<u16>, String> {
    match TcpListener::bind(addr) {
        Ok(server) => {
            let port = server_port(&server)?;
            servers.push(server);
            Ok(Some(port))
        }
        Err(err) => {
            *addr_in_use |= is_addr_in_use(&err);
            if last_err.is_none() {
                *last_err = Some(err.to_string());
            }
            Ok(None)
        }
    }
}

/// 函数 `bind_localhost_login_servers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - port: 参数 port
///
/// # 返回
/// 返回函数执行结果
fn bind_localhost_login_servers(port: u16) -> Result<(Vec<TcpListener>, LoginServerInfo), String> {
    let mut addr_in_use = false;
    let mut last_err: Option<String> = None;
    let mut servers: Vec<TcpListener> = Vec::new();
    let mut selected_port = port;

    if port == 0 {
        if let Some(v4_port) =
            try_bind_login_server("127.0.0.1:0", &mut servers, &mut addr_in_use, &mut last_err)?
        {
            selected_port = v4_port;
            let _ = try_bind_login_server(
                &format!("[::1]:{selected_port}"),
                &mut servers,
                &mut addr_in_use,
                &mut last_err,
            )?;
        } else if let Some(v6_port) =
            try_bind_login_server("[::1]:0", &mut servers, &mut addr_in_use, &mut last_err)?
        {
            selected_port = v6_port;
            let _ = try_bind_login_server(
                &format!("127.0.0.1:{selected_port}"),
                &mut servers,
                &mut addr_in_use,
                &mut last_err,
            )?;
        }
    } else {
        let _ = try_bind_login_server(
            &format!("127.0.0.1:{port}"),
            &mut servers,
            &mut addr_in_use,
            &mut last_err,
        )?;
        let _ = try_bind_login_server(
            &format!("[::1]:{port}"),
            &mut servers,
            &mut addr_in_use,
            &mut last_err,
        )?;
    }

    if !servers.is_empty() {
        if selected_port == 0 {
            selected_port = server_port(&servers[0])?;
        }
        return Ok((
            servers,
            LoginServerInfo {
                port: selected_port,
            },
        ));
    }
    if addr_in_use {
        return Err(format!(
            "登录回调端口 {port} 已被占用，请关闭占用程序或修改 CODEXMANAGER_LOGIN_ADDR"
        ));
    }
    if let Some(err) = last_err {
        return Err(err);
    }
    Err("failed to bind login server".to_string())
}

/// 函数 `bind_login_server`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
fn bind_login_server(addr: &str) -> Result<(Vec<TcpListener>, LoginServerInfo), String> {
    if let Ok(url) = Url::parse(&format!("http://{addr}")) {
        let host = url.host_str().unwrap_or("localhost");
        let port = url.port_or_known_default().unwrap_or(1455);
        if host == "localhost" {
            // 中文注释：localhost 绑定双栈，避免浏览器在 IPv4/IPv6 间切换时回调命中失败。
            return bind_localhost_login_servers(port);
        } else if !is_loopback_host(host) && !allow_non_loopback_login_addr() {
            return Err(format!(
                "登录回调地址仅允许 loopback（localhost/127.0.0.1/::1），当前为 {host}"
            ));
        }
    }

    let server = TcpListener::bind(addr).map_err(|e| e.to_string())?;
    let port = server_port(&server)?;
    Ok((vec![server], LoginServerInfo { port }))
}

/// 函数 `is_addr_in_use`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
fn is_addr_in_use(err: &(dyn std::error::Error + 'static)) -> bool {
    err.downcast_ref::<io::Error>()
        .map(|io_err| io_err.kind() == io::ErrorKind::AddrInUse)
        .unwrap_or(false)
}

/// 函数 `run_login_server`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - server: 参数 server
///
/// # 返回
/// 无
async fn run_login_server(server: TcpListener) -> io::Result<()> {
    let listener = tokio::net::TcpListener::from_std(server)?;
    let app = axum::Router::new()
        .route(
            "/auth/callback",
            axum::routing::get(crate::http::callback_endpoint::handle_callback_http),
        )
        .layer(axum::middleware::from_fn(
            crate::http::middleware::request_timeout,
        ))
        .layer(axum::middleware::from_fn(
            crate::http::middleware::request_id,
        ));
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            while !crate::shutdown_requested() {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
}

/// 函数 `html_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn html_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_string(body);
    if let Ok(header) = Header::from_bytes(
        b"Content-Type".as_slice(),
        b"text/html; charset=utf-8".as_slice(),
    ) {
        response = response.with_header(header);
    }
    if let Ok(header) = Header::from_bytes(b"Connection".as_slice(), b"close".as_slice()) {
        response = response.with_header(header);
    }
    response
}

/// 函数 `build_callback_success_page`
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
fn build_callback_success_page() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Login Success</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; padding: 32px; color: #111827; background: #f8fafc; }
    .card { max-width: 560px; margin: 40px auto; background: #fff; border: 1px solid #dbe3ee; border-radius: 16px; padding: 24px; box-shadow: 0 12px 32px rgba(15, 23, 42, 0.08); }
    h1 { margin: 0 0 12px; font-size: 24px; }
    p { margin: 8px 0; line-height: 1.6; }
    .muted { color: #64748b; font-size: 14px; }
    button { margin-top: 16px; padding: 10px 16px; border: 0; border-radius: 10px; background: #2563eb; color: #fff; font-size: 14px; cursor: pointer; }
  </style>
</head>
<body>
  <div class="card">
    <h1>Login Success</h1>
    <p>Authorization completed. This window will try to close automatically.</p>
    <p class="muted">If the browser blocks auto-close, you can close this window manually.</p>
    <button type="button" onclick="window.close()">Close Window</button>
  </div>
  <script>
    (() => {
      const tryClose = () => {
        try { window.open('', '_self'); } catch (_) {}
        try { window.close(); } catch (_) {}
      };
      tryClose();
      setTimeout(tryClose, 120);
      setTimeout(tryClose, 500);
    })();
  </script>
</body>
</html>"#
        .to_string()
}

/// 函数 `build_callback_error_page`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
fn build_callback_error_page(err: &str) -> String {
    let escaped = err
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Login Failed</title>
  <style>
    body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; padding: 32px; color: #111827; background: #f8fafc; }}
    .card {{ max-width: 560px; margin: 40px auto; background: #fff; border: 1px solid #fecaca; border-radius: 16px; padding: 24px; box-shadow: 0 12px 32px rgba(15, 23, 42, 0.08); }}
    h1 {{ margin: 0 0 12px; font-size: 24px; color: #b91c1c; }}
    p {{ margin: 8px 0; line-height: 1.6; }}
    code {{ display: block; margin-top: 12px; white-space: pre-wrap; word-break: break-word; background: #fff1f2; padding: 12px; border-radius: 10px; }}
  </style>
</head>
<body>
  <div class="card">
    <h1>Login Failed</h1>
    <p>The callback was received, but completing login failed.</p>
    <code>{escaped}</code>
  </div>
</body>
</html>"#
    )
}

#[cfg(test)]
#[path = "../../tests/auth/auth_callback_tests.rs"]
mod tests;

#[cfg(test)]
mod native_listener_tests {
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_runtime_oauth_listener_drains_and_rebinds_after_shutdown() {
        let _guard = crate::test_env_guard();
        crate::clear_shutdown_flag();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let first = super::ensure_login_server_with_addr("127.0.0.1:0").unwrap();
        let addr = format!("127.0.0.1:{}", first.port);
        for iteration in 0..2 {
            if iteration == 1 {
                let restarted = super::ensure_login_server_with_addr(&addr).unwrap();
                assert_eq!(restarted.port, first.port);
            }
            assert_eq!(
                client
                    .get(format!("http://{addr}/unknown"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                reqwest::StatusCode::NOT_FOUND
            );
            crate::request_shutdown("");
            assert!(super::ensure_login_server_with_addr(&addr).is_err());
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                super::drain_login_server(),
            )
            .await
            .unwrap();
            assert!(
                std::net::TcpStream::connect(&addr).is_err(),
                "shutdown must close the callback port"
            );
            crate::clear_shutdown_flag();
        }
    }

    #[tokio::test]
    async fn oauth_listener_uses_axum_for_callback_method_and_error_responses() {
        let (listeners, info) =
            super::bind_login_server("localhost:0").expect("bind oauth listeners");
        assert_ne!(info.port, 0);
        let mut tasks = Vec::new();
        let mut addresses = Vec::new();
        for listener in listeners {
            addresses.push(listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            tasks.push(tokio::spawn(super::run_login_server(listener)));
        }
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for address in addresses {
            assert_eq!(
                address.port(),
                info.port,
                "dual-stack listeners share one redirect port"
            );
            let response = client
                .get(format!("http://{address}/auth/callback?code=fixture"))
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                reqwest::StatusCode::INTERNAL_SERVER_ERROR
            );
            assert!(response.headers().contains_key("x-request-id"));
            assert!(response.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html"));
            assert!(response
                .text()
                .await
                .unwrap()
                .contains("Missing login state"));
            assert_eq!(
                client
                    .post(format!("http://{address}/auth/callback"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                reqwest::StatusCode::METHOD_NOT_ALLOWED
            );
            assert_eq!(
                client
                    .get(format!("http://{address}/unknown"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                reqwest::StatusCode::NOT_FOUND
            );
        }
        for task in tasks {
            task.abort();
            let _ = task.await;
        }
    }
}
