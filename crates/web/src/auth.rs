use super::*;

use axum::extract::Query;
use serde::Deserialize;

const WEB_AUTH_TAB_SESSION_STORAGE_KEY: &str = "codexmanager_web_auth_tab";

#[derive(Debug, Deserialize)]
pub(super) struct LoginForm {
    username: Option<String>,
    password: Option<String>,
    display_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct LoginQuery {
    force: Option<String>,
}

/// 函数 `current_web_access_password_hash`
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
fn current_web_access_password_hash() -> Option<String> {
    codexmanager_service::current_web_access_password_hash()
}

/// 函数 `generate_web_auth_session_key`
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
pub(super) fn generate_web_auth_session_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 函数 `build_web_auth_cookie_value`
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
pub(super) fn build_web_auth_cookie_value(
    password_hash: &str,
    rpc_token: &str,
    session_key: &str,
) -> String {
    let scoped_rpc_token = format!("{rpc_token}:{session_key}");
    codexmanager_service::build_web_access_session_token(password_hash, &scoped_rpc_token)
}

/// 函数 `parse_cookie_value`
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
pub(super) fn parse_cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
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

/// 函数 `set_cookie_header_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn set_cookie_header_value(value: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{WEB_AUTH_COOKIE_NAME}={value}; Path=/; HttpOnly; SameSite=Lax"
    ))
    .ok()
}

/// 函数 `clear_cookie_header_value`
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
fn clear_cookie_header_value() -> Option<HeaderValue> {
    HeaderValue::from_str(&format!(
        "{WEB_AUTH_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
    ))
    .ok()
}

/// 函数 `append_no_store_headers`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - response: 参数 response
///
/// # 返回
/// 无
fn append_no_store_headers(response: &mut Response) {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, no-cache, must-revalidate"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
        .headers_mut()
        .insert(header::EXPIRES, HeaderValue::from_static("0"));
}

/// 函数 `login_force_requested`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - query: 参数 query
///
/// # 返回
/// 返回函数执行结果
fn login_force_requested(query: &LoginQuery) -> bool {
    query
        .force
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
}

fn is_public_static_asset_path(path: &str) -> bool {
    path.starts_with("/_next/")
        || path.starts_with("/static/")
        || path == "/favicon.ico"
        || path == "/robots.txt"
        || path == "/manifest.json"
        || path.ends_with(".js")
        || path.ends_with(".css")
        || path.ends_with(".map")
        || path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".webp")
        || path.ends_with(".svg")
        || path.ends_with(".ico")
        || path.ends_with(".woff")
        || path.ends_with(".woff2")
}

/// 函数 `request_is_authenticated`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - state: 参数 state
///
/// # 返回
/// 返回函数执行结果
fn request_is_authenticated(headers: &HeaderMap, state: &AppState) -> bool {
    match codexmanager_service::current_web_auth_mode().as_str() {
        "none" => true,
        "accounts" => current_app_session_from_headers(headers).is_some(),
        _ => {
            let Some(password_hash) = current_web_access_password_hash() else {
                return true;
            };
            let Some(cookie_value) = parse_cookie_value(headers, WEB_AUTH_COOKIE_NAME) else {
                return false;
            };
            let expected = build_web_auth_cookie_value(
                &password_hash,
                &state.rpc_token,
                &state.web_auth_session_key,
            );
            cookie_value == expected
        }
    }
}

pub(super) fn current_app_session_from_headers(
    headers: &HeaderMap,
) -> Option<codexmanager_service::AppSessionUserResult> {
    if codexmanager_service::current_web_auth_mode() != "accounts" {
        return None;
    }
    parse_cookie_value(headers, WEB_AUTH_COOKIE_NAME).and_then(|token| {
        codexmanager_service::resolve_app_user_session(&token)
            .ok()
            .flatten()
    })
}

/// 函数 `builtin_login_html`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - error: 参数 error
///
/// # 返回
/// 返回函数执行结果
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
      <div class="foot">密码可在桌面端或 Web 端右上角的“密码”入口中修改。</div>
    </form>
  </body>
</html>
"#
    )
}

fn account_login_html(error: Option<&str>, bootstrap: bool) -> String {
    let error_html = error
        .map(|text| format!(r#"<div class="error">{}</div>"#, escape_html(text)))
        .unwrap_or_default();
    let title = if bootstrap {
        "初始化管理员"
    } else {
        "账号登录"
    };
    let desc = if bootstrap {
        "首次启用账号系统，请创建管理员账号。后续成员、额度和 Key 归属都由管理员维护。"
    } else {
        "当前 CodexManager Web 已启用账号系统，请使用管理员或成员账号进入。"
    };
    let display_name_field = if bootstrap {
        r#"<label for="display_name">显示名称</label>
      <input id="display_name" name="display_name" type="text" autocomplete="name" />"#
    } else {
        ""
    };
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
        width: min(100%, 440px);
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
      label {{ display: block; margin: 14px 0 10px; font-size: 14px; color: var(--muted); }}
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
        margin-top: 18px;
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
      <h1>{title}</h1>
      <p>{desc}</p>
      {error_html}
      <label for="username">用户名</label>
      <input id="username" name="username" type="text" autocomplete="username" autofocus />
      {display_name_field}
      <label for="password">密码</label>
      <input id="password" name="password" type="password" autocomplete="current-password" />
      <button type="submit">{title}</button>
      <div class="foot">账号模式用于团队额度分发；可在设置中切换回个人模式或访问密码模式。</div>
    </form>
  </body>
</html>
"#
    )
}

/// 函数 `login_success_html`
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
fn login_success_html() -> String {
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>CodexManager Web 登录</title>
  </head>
  <body>
    <script>
      try {{
        window.sessionStorage.setItem("{WEB_AUTH_TAB_SESSION_STORAGE_KEY}", "1");
      }} catch (_err) {{}}
      window.location.replace("/");
    </script>
  </body>
</html>
"#
    )
}

/// 函数 `logout_success_html`
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
fn logout_success_html() -> String {
    format!(
        r#"<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8"/>
    <meta name="viewport" content="width=device-width, initial-scale=1"/>
    <title>CodexManager Web 已退出</title>
  </head>
  <body>
    <script>
      try {{
        window.sessionStorage.removeItem("{WEB_AUTH_TAB_SESSION_STORAGE_KEY}");
      }} catch (_err) {{}}
      window.location.replace("/__login?force=1");
    </script>
  </body>
</html>
"#
    )
}

/// 函数 `web_auth_middleware`
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
pub(super) async fn web_auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    if path == "/__login" || path == "/__logout" || is_public_static_asset_path(&path) {
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

/// 函数 `login_page`
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
pub(super) async fn login_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LoginQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let mode = codexmanager_service::current_web_auth_mode();
    if mode == "none" {
        return Redirect::to("/").into_response();
    }
    if request_is_authenticated(&headers, state.as_ref()) && !login_force_requested(&query) {
        return Redirect::to("/").into_response();
    }
    let html = if mode == "accounts" {
        let bootstrap = codexmanager_service::app_auth_status_value()
            .ok()
            .and_then(|value| {
                value
                    .get("appUsersConfigured")
                    .and_then(|configured| configured.as_bool())
                    .map(|configured| !configured)
            })
            .unwrap_or(true);
        account_login_html(None, bootstrap)
    } else {
        builtin_login_html(None)
    };
    let mut response = Html(html).into_response();
    append_no_store_headers(&mut response);
    response
}

/// 函数 `login_submit`
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
pub(super) async fn login_submit(
    State(state): State<Arc<AppState>>,
    axum::Form(form): axum::Form<LoginForm>,
) -> impl IntoResponse {
    let mode = codexmanager_service::current_web_auth_mode();
    if mode == "none" {
        return Redirect::to("/").into_response();
    }
    if mode == "accounts" {
        let bootstrap = codexmanager_service::app_auth_status_value()
            .ok()
            .and_then(|value| {
                value
                    .get("appUsersConfigured")
                    .and_then(|configured| configured.as_bool())
                    .map(|configured| !configured)
            })
            .unwrap_or(true);
        let username = form.username.as_deref().unwrap_or("");
        let password = form.password.as_deref().unwrap_or("");
        let result = if bootstrap {
            codexmanager_service::bootstrap_app_admin(
                username,
                password,
                form.display_name.as_deref(),
            )
        } else {
            codexmanager_service::login_app_user(username, password)
        };
        match result {
            Ok(login) => {
                let mut response = Html(login_success_html()).into_response();
                if let Some(header_value) = set_cookie_header_value(&login.token) {
                    response
                        .headers_mut()
                        .append(header::SET_COOKIE, header_value);
                }
                append_no_store_headers(&mut response);
                return response;
            }
            Err(err) => {
                let mut response = (
                    StatusCode::UNAUTHORIZED,
                    Html(account_login_html(Some(&err), bootstrap)),
                )
                    .into_response();
                append_no_store_headers(&mut response);
                return response;
            }
        }
    }
    let Some(password_hash) = current_web_access_password_hash() else {
        return Redirect::to("/").into_response();
    };
    let password = form.password.as_deref().unwrap_or("");
    if !codexmanager_service::verify_web_access_password(password) {
        let mut response = (
            StatusCode::UNAUTHORIZED,
            Html(builtin_login_html(Some("密码错误，请重试。"))),
        )
            .into_response();
        append_no_store_headers(&mut response);
        return response;
    }
    let token = build_web_auth_cookie_value(
        &password_hash,
        &state.rpc_token,
        &state.web_auth_session_key,
    );
    let mut response = Html(login_success_html()).into_response();
    if let Some(header_value) = set_cookie_header_value(&token) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, header_value);
    }
    append_no_store_headers(&mut response);
    response
}

/// 函数 `logout`
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
pub(super) async fn logout(headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = parse_cookie_value(&headers, WEB_AUTH_COOKIE_NAME) {
        if let Err(err) = codexmanager_service::logout_app_user_session(&token) {
            log::warn!("event=web_logout_session_revoke_failed error={err}");
        }
    }
    let mut response = Html(logout_success_html()).into_response();
    if let Some(header_value) = clear_cookie_header_value() {
        response
            .headers_mut()
            .append(header::SET_COOKIE, header_value);
    }
    append_no_store_headers(&mut response);
    response
}

/// 函数 `auth_status`
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
pub(super) async fn auth_status(headers: HeaderMap) -> impl IntoResponse {
    let mut status = codexmanager_service::app_auth_status_value().unwrap_or_else(|_| {
        serde_json::json!({
            "mode": codexmanager_service::current_web_auth_mode(),
            "passwordConfigured": current_web_access_password_hash().is_some(),
            "appUsersConfigured": false,
            "distributionEnabled": false,
            "billingModeLock": {
                "accountModeLocked": false,
                "distributionLocked": false,
                "reasons": []
            },
        })
    });
    let actor = current_app_session_from_headers(&headers)
        .as_ref()
        .map(|session| {
            codexmanager_service::RpcActor::from_parts(
                Some(session.user.role.as_str()),
                Some(session.user.id.as_str()),
            )
        })
        .unwrap_or_else(codexmanager_service::RpcActor::system_admin);
    if let Some(object) = status.as_object_mut() {
        if let Some(session) = current_app_session_from_headers(&headers) {
            object.insert(
                "currentUser".to_string(),
                serde_json::to_value(session.user).unwrap_or(serde_json::Value::Null),
            );
        } else {
            object.insert("currentUser".to_string(), serde_json::Value::Null);
        }
        object.insert("role".to_string(), serde_json::json!(actor.role));
        object.insert(
            "permissions".to_string(),
            serde_json::json!(actor
                .permissions()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()),
        );
    }
    let mut response = axum::Json(status).into_response();
    append_no_store_headers(&mut response);
    response
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
