use super::*;

/// 函数 `should_spawn_service`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn should_spawn_service() -> bool {
    read_env_trim("CODEXMANAGER_WEB_NO_SPAWN_SERVICE").is_none()
}

static SERVICE_PROBE_CLIENT: std::sync::OnceLock<Result<reqwest::Client, String>> =
    std::sync::OnceLock::new();

fn build_service_probe_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(1200))
        .build()
        .map_err(|err| format!("probe client init failed: {err}"))
}

fn service_probe_client() -> Result<reqwest::Client, String> {
    SERVICE_PROBE_CLIENT
        .get_or_init(build_service_probe_client)
        .clone()
}

/// 函数 `service_rpc_probe`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - service_addr: 参数 service_addr
/// - rpc_token: 参数 rpc_token
///
/// # 返回
/// 返回函数执行结果
async fn service_rpc_probe(
    client: &reqwest::Client,
    service_addr: &str,
    rpc_token: &str,
) -> Result<(), String> {
    let trimmed = service_addr.trim();
    if trimmed.is_empty() {
        return Err("service address is empty".to_string());
    }

    let response = client
        .post(format!("http://{trimmed}/rpc"))
        .header("content-type", "application/json")
        .header("x-codexmanager-rpc-token", rpc_token)
        .body(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {}
            })
            .to_string(),
        )
        .send()
        .await
        .map_err(|err| format!("probe request failed: {err}"))?;

    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("rpc_token_mismatch".to_string());
    }
    if !response.status().is_success() {
        return Err(format!("probe http {}", response.status()));
    }

    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|err| format!("probe response parse failed: {err}"))?;
    let result = payload.get("result").and_then(|value| value.as_object());
    let user_agent = result
        .and_then(|value| value.get("userAgent").or_else(|| value.get("user_agent")))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let codex_home = result
        .and_then(|value| value.get("codexHome").or_else(|| value.get("codex_home")))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !user_agent.contains("codex_cli_rs/") || codex_home.is_empty() {
        return Err("unexpected service on target port".to_string());
    }
    Ok(())
}

/// 函数 `shutdown_existing_service`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - service_addr: 参数 service_addr
///
/// # 返回
/// 返回函数执行结果
async fn shutdown_existing_service(service_addr: &str) -> bool {
    let addr = service_addr.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        codexmanager_service::request_shutdown(&addr);
    })
    .await;

    for _ in 0..30 {
        if !tcp_probe(service_addr).await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

/// 函数 `tcp_probe`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) async fn tcp_probe(addr: &str) -> bool {
    let addr = addr.trim();
    if addr.is_empty() {
        return false;
    }
    let addr = addr.strip_prefix("http://").unwrap_or(addr);
    let addr = addr.strip_prefix("https://").unwrap_or(addr);
    let addr = addr.split('/').next().unwrap_or(addr);
    matches!(
        tokio::time::timeout(
            Duration::from_millis(250),
            tokio::net::TcpStream::connect(addr),
        )
        .await,
        Ok(Ok(_))
    )
}

/// 函数 `service_bin_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - dir: 参数 dir
///
/// # 返回
/// 返回函数执行结果
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

/// 函数 `spawn_service_detached`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - dir: 参数 dir
/// - service_addr: 参数 service_addr
///
/// # 返回
/// 返回函数执行结果
fn spawn_service_detached(dir: &Path, service_addr: &str) -> std::io::Result<()> {
    let bin = service_bin_path(dir);
    let mut cmd = Command::new(bin);
    let bind_addr = codexmanager_service::listener_bind_addr(service_addr);
    cmd.env("CODEXMANAGER_SERVICE_ADDR", bind_addr);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let _child = cmd.spawn()?;
    Ok(())
}

/// 函数 `ensure_service_running`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) async fn ensure_service_running(
    service_addr: &str,
    rpc_token: &str,
    dir: &Path,
    spawned_service: &Arc<Mutex<bool>>,
) -> Option<String> {
    let probe_client = match service_probe_client() {
        Ok(client) => client,
        Err(err) => return Some(err),
    };

    if tcp_probe(service_addr).await {
        match service_rpc_probe(&probe_client, service_addr, rpc_token).await {
            Ok(()) => return None,
            Err(err) if err == "rpc_token_mismatch" && should_spawn_service() => {
                if !shutdown_existing_service(service_addr).await {
                    return Some(format!(
                        "service reachable at {service_addr} but rejected rpc token; old instance is still occupying the port"
                    ));
                }
            }
            Err(err) => {
                return Some(format!(
                    "service reachable at {service_addr} but startup handshake failed: {err}"
                ));
            }
        }
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

    let mut last_probe_error: Option<String> = None;
    for _ in 0..50 {
        if tcp_probe(service_addr).await {
            match service_rpc_probe(&probe_client, service_addr, rpc_token).await {
                Ok(()) => return None,
                Err(err) => {
                    last_probe_error = Some(format!(
                        "service became reachable but startup handshake failed: {err}"
                    ));
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Some(
        last_probe_error.unwrap_or_else(|| {
            format!("service still not reachable at {service_addr} after spawn")
        }),
    )
}

/// 函数 `rpc_proxy`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) async fn rpc_proxy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > codexmanager_service::RPC_BODY_LIMIT_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "{}").into_response();
    }
    if !is_json_content_type(&headers) {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "{}").into_response();
    }
    let mut request = state
        .client
        .post(&state.service_rpc_url)
        .header("content-type", "application/json")
        .header("x-codexmanager-rpc-token", &state.rpc_token);
    if let Some(session) = auth::current_app_session_from_headers(&headers) {
        request = request
            .header("x-codexmanager-rpc-actor-role", session.user.role)
            .header("x-codexmanager-rpc-actor-user-id", session.user.id);
    }
    let resp = request.body(body).send().await;
    let resp = match resp {
        Ok(v) => v,
        Err(err) => {
            let msg = format_upstream_error_message(state.service_addr.as_str(), &err);
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

pub(super) async fn author_content(State(state): State<Arc<AppState>>) -> Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "authorContent/get",
        "params": {}
    })
    .to_string();

    let resp = state
        .client
        .post(&state.service_rpc_url)
        .header("content-type", "application/json")
        .header("x-codexmanager-rpc-token", &state.rpc_token)
        .body(body)
        .send()
        .await;
    let resp = match resp {
        Ok(value) => value,
        Err(err) => {
            let msg = format_upstream_error_message(state.service_addr.as_str(), &err);
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = resp.status();
    let payload = match resp.json::<serde_json::Value>().await {
        Ok(value) => value,
        Err(err) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("author content response parse failed: {err}"),
            )
                .into_response();
        }
    };

    if !status.is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            payload
                .get("error")
                .and_then(|value| value.get("message"))
                .and_then(|value| value.as_str())
                .unwrap_or("author content upstream request failed")
                .to_string(),
        )
            .into_response();
    }

    let result = payload
        .get("result")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut out = Response::new(axum::body::Body::from(result.to_string()));
    *out.status_mut() = StatusCode::OK;
    out.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_static("application/json"),
    );
    out
}

pub(super) async fn usage_refresh_events(State(state): State<Arc<AppState>>) -> Response {
    let target_url = format!("http://{}/events/usage-refresh", state.service_addr.trim());
    let resp = state
        .client
        .get(&target_url)
        .header("accept", "text/event-stream")
        .header("x-codexmanager-rpc-token", &state.rpc_token)
        .send()
        .await;
    let resp = match resp {
        Ok(value) => value,
        Err(err) => {
            let msg = format_upstream_error_message(state.service_addr.as_str(), &err);
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out = Response::new(Body::from_stream(resp.bytes_stream()));
    *out.status_mut() = status;
    out.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_static("text/event-stream"),
    );
    out.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-cache"),
    );
    out.headers_mut().insert(
        "x-accel-buffering",
        axum::http::HeaderValue::from_static("no"),
    );
    out
}

fn account_test_events_target_url(service_addr: &str, uri: &axum::http::Uri) -> String {
    let mut target_url = format!("http://{}/events/account-test", service_addr.trim());
    if let Some(query) = uri.query().filter(|query| !query.is_empty()) {
        target_url.push('?');
        target_url.push_str(query);
    }
    target_url
}

fn account_test_events_role_allowed(web_auth_mode: &str, role: Option<&str>) -> bool {
    web_auth_mode != "accounts"
        || matches!(
            role,
            Some(codexmanager_service::ROLE_ADMIN | codexmanager_service::ROLE_SYSTEM_ADMIN)
        )
}

pub(super) async fn account_test_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Response {
    let web_auth_mode = codexmanager_service::current_web_auth_mode();
    let session = auth::current_app_session_from_headers(&headers);
    if !account_test_events_role_allowed(
        &web_auth_mode,
        session.as_ref().map(|session| session.user.role.as_str()),
    ) {
        return (StatusCode::FORBIDDEN, "{}").into_response();
    }

    let target_url = account_test_events_target_url(&state.service_addr, &uri);
    let resp = state
        .client
        .get(&target_url)
        .header("accept", "text/event-stream")
        .header("x-codexmanager-rpc-token", &state.rpc_token)
        .send()
        .await;
    let resp = match resp {
        Ok(value) => value,
        Err(err) => {
            let msg = format_upstream_error_message(state.service_addr.as_str(), &err);
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out = Response::new(Body::from_stream(resp.bytes_stream()));
    *out.status_mut() = status;
    out.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_static("text/event-stream"),
    );
    out.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-cache"),
    );
    out.headers_mut().insert(
        "x-accel-buffering",
        axum::http::HeaderValue::from_static("no"),
    );
    out
}

const DEFAULT_GATEWAY_PROXY_MAX_BODY_BYTES: usize = 0;
const ENV_GATEWAY_PROXY_MAX_BODY_BYTES: &str = "CODEXMANAGER_GATEWAY_PROXY_MAX_BODY_BYTES";

fn env_usize_or(name: &str, default: usize) -> usize {
    read_env_trim(name)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn gateway_proxy_max_body_bytes() -> usize {
    env_usize_or(
        ENV_GATEWAY_PROXY_MAX_BODY_BYTES,
        DEFAULT_GATEWAY_PROXY_MAX_BODY_BYTES,
    )
}

/// 函数 `gateway_proxy_target_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// - service_addr: service 地址
/// - uri: 原始请求 URI
///
/// # 返回
/// 返回转发目标地址
fn gateway_proxy_target_url(service_addr: &str, uri: &axum::http::Uri) -> String {
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    format!("http://{service_addr}{path_and_query}")
}

/// 函数 `is_hop_by_hop_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// - name: header 名
///
/// # 返回
/// 是否为逐跳 header
fn is_hop_by_hop_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("connection")
        || name.eq_ignore_ascii_case("keep-alive")
        || name.eq_ignore_ascii_case("proxy-authenticate")
        || name.eq_ignore_ascii_case("proxy-authorization")
        || name.eq_ignore_ascii_case("te")
        || name.eq_ignore_ascii_case("trailer")
        || name.eq_ignore_ascii_case("transfer-encoding")
        || name.eq_ignore_ascii_case("upgrade")
}

/// 函数 `should_skip_gateway_request_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// - name: header 名
/// - value: header 值
///
/// # 返回
/// 是否过滤请求 header
fn should_skip_gateway_request_header(name: &header::HeaderName, value: &HeaderValue) -> bool {
    let lower = name.as_str();
    is_hop_by_hop_header(lower)
        || lower.eq_ignore_ascii_case("host")
        || lower.eq_ignore_ascii_case("content-length")
        || value.to_str().is_err()
}

/// 函数 `should_skip_gateway_response_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// - name: header 名
///
/// # 返回
/// 是否过滤响应 header
fn should_skip_gateway_response_header(name: &header::HeaderName) -> bool {
    let lower = name.as_str();
    is_hop_by_hop_header(lower) || lower.eq_ignore_ascii_case("content-length")
}

/// 函数 `gateway_proxy`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-08
///
/// # 参数
/// - state: 应用状态
/// - request: 请求
///
/// # 返回
/// 返回 service 网关响应
pub(super) async fn gateway_proxy(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    if parts.method == axum::http::Method::GET
        && super::gateway_websocket::is_upgrade_request(&parts.headers)
    {
        drop(body);
        return super::gateway_websocket::proxy(state, parts).await;
    }
    let target_url = gateway_proxy_target_url(state.service_addr.as_str(), &parts.uri);
    let max_body_bytes = gateway_proxy_max_body_bytes();

    let outbound_body = if max_body_bytes == 0 {
        reqwest::Body::wrap_stream(body.into_data_stream())
    } else {
        match to_bytes(body, max_body_bytes).await {
            Ok(body) => reqwest::Body::from(body),
            Err(err) => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    format!("gateway proxy request body error: {err}"),
                )
                    .into_response();
            }
        }
    };

    let mut outbound = state
        .client
        .request(parts.method, target_url)
        .body(outbound_body);
    for (name, value) in parts.headers.iter() {
        if should_skip_gateway_request_header(name, value) {
            continue;
        }
        outbound = outbound.header(name, value);
    }

    let resp = match outbound.send().await {
        Ok(resp) => resp,
        Err(err) => {
            let msg = format_upstream_error_message(state.service_addr.as_str(), &err);
            return (StatusCode::BAD_GATEWAY, msg).into_response();
        }
    };

    let status = resp.status();
    let headers = resp.headers().clone();
    let mut out = Response::new(Body::from_stream(resp.bytes_stream()));
    *out.status_mut() = status;
    for (name, value) in headers.iter() {
        if should_skip_gateway_response_header(name) {
            continue;
        }
        out.headers_mut().append(name.clone(), value.clone());
    }
    out
}

/// 函数 `format_upstream_error_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-10
///
/// # 参数
/// - service_addr: 参数 service_addr
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
fn format_upstream_error_message(service_addr: &str, err: impl std::fmt::Display) -> String {
    let mut message = format!("upstream error: {err}");
    if service_addr.contains("host.docker.internal") {
        message.push_str(
            "; Linux Docker 中 `host.docker.internal` 可能不可解析，建议让 web 与 service 加入同一 Docker 网络，并使用容器地址如 `codexmanager-service:48760`；如果必须走宿主机端口，请增加 `--add-host=host.docker.internal:host-gateway`",
        );
    }
    message
}

/// 函数 `quit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) async fn quit(State(state): State<Arc<AppState>>) -> impl IntoResponse {
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

#[cfg(test)]
#[path = "service_gateway_tests.rs"]
mod tests;
