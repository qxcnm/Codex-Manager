use super::*;
use axum::Extension;

const ACTION_HEADER: &str = "x-codexmanager-action";
const RPC_TOKEN_HEADER: &str = "x-codexmanager-rpc-token";

fn valid_launcher_token(headers: &HeaderMap) -> bool {
    headers
        .get(RPC_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(codexmanager_service::rpc_auth_token_matches)
}

// The launcher has no Web session. Its secret authorizes only this exact
// lifecycle action, never arbitrary Web RPCs or an actor header supplied by a client.
pub(super) fn is_launcher_request(request: &Request) -> bool {
    request.method() == axum::http::Method::POST
        && request.uri().path() == "/__quit"
        && valid_launcher_token(request.headers())
}

fn browser_action_allowed(headers: &HeaderMap) -> bool {
    // A form or no-CORS fetch cannot add this non-safelisted header. Do not
    // enable CORS for this endpoint or accept its value from a form/query/body.
    if headers
        .get(ACTION_HEADER)
        .and_then(|value| value.to_str().ok())
        != Some("shutdown")
    {
        return false;
    }
    if let Some(site) = headers.get("sec-fetch-site") {
        if !matches!(site.to_str(), Ok("same-origin" | "none")) {
            return false;
        }
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let Some(origin) = origin
            .to_str()
            .ok()
            .and_then(|value| value.parse::<axum::http::Uri>().ok())
        else {
            return false;
        };
        let Some(host) = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };
        if !matches!(origin.scheme_str(), Some("http" | "https"))
            || !origin
                .authority()
                .is_some_and(|authority| authority.as_str().eq_ignore_ascii_case(host))
            || origin.path() != "/"
            || origin.query().is_some()
        {
            return false;
        }
    }
    true
}

pub(super) async fn confirmation(
    Extension(actor): Extension<codexmanager_service::RpcActor>,
) -> Response {
    if !actor.is_admin() {
        return StatusCode::FORBIDDEN.into_response();
    }
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::X_FRAME_OPTIONS, "DENY"),
            (header::CONTENT_SECURITY_POLICY, "frame-ancestors 'none'"),
        ],
        Html(
            r#"<!doctype html>
<html lang="zh-CN">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>关闭 CodexManager</title></head>
<body>
<h1>关闭 CodexManager</h1>
<p>关闭 Web 服务；由 Web 或启动器管理的 Service 也会随之关闭。</p>
<button id="quit" type="button">确认关闭</button> <a href="/">返回</a>
<p id="status" role="status"></p>
<script>
document.getElementById("quit").addEventListener("click", async function () {
  this.disabled = true;
  const status = document.getElementById("status");
  try {
    const response = await fetch("/__quit", {
      method: "POST", credentials: "same-origin", redirect: "error",
      headers: {"X-CodexManager-Action": "shutdown"}
    });
    if (!response.ok) throw new Error("关闭失败，请重新登录管理员账号后重试。");
    status.textContent = "已请求关闭。";
  } catch (error) {
    status.textContent = error.message || "关闭失败。";
    this.disabled = false;
  }
});
</script>
</body></html>"#,
        ),
    )
        .into_response()
}

pub(super) async fn quit(
    State(state): State<Arc<AppState>>,
    Extension(actor): Extension<codexmanager_service::RpcActor>,
    headers: HeaderMap,
) -> Response {
    if !actor.is_admin() || !(valid_launcher_token(&headers) || browser_action_allowed(&headers)) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if *state.spawned_service.lock().await {
        let addr = state.service_addr.clone();
        let _ = tokio::task::spawn_blocking(move || {
            codexmanager_service::request_shutdown(&addr);
        })
        .await;
    }
    let _ = state.shutdown_tx.send(true);
    Html("<html><body>OK</body></html>").into_response()
}
