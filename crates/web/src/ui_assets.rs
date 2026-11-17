use super::*;

/// 函数 `builtin_missing_ui_html`
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
pub(super) fn builtin_missing_ui_html(detail: &str) -> String {
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
      <p>2) 从源码运行：先执行 <code>pnpm -C apps run build:desktop</code>，再设置 <code>CODEXMANAGER_WEB_ROOT=.../apps/out</code> 启动。</p>
      <p>关闭：管理员可打开 <a href="/__quit">关闭确认页</a>，确认后停止服务。</p>
    </div>
  </body>
</html>
"#
    )
}

/// 函数 `serve_missing_ui`
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
pub(super) async fn serve_missing_ui(State(state): State<Arc<AppState>>) -> Html<String> {
    Html((*state.missing_ui_html).clone())
}

/// 函数 `serve_embedded_index`
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
pub(super) async fn serve_embedded_index() -> Response {
    serve_embedded_path("index.html")
}

/// 函数 `serve_embedded_asset`
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
pub(super) async fn serve_embedded_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Response {
    serve_embedded_path(&path)
}

/// 函数 `looks_like_asset_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn looks_like_asset_path(path: &str) -> bool {
    path.rsplit('/').next().unwrap_or(path).contains('.')
}

/// 函数 `serve_embedded_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn serve_embedded_path(path: &str) -> Response {
    let raw = path.trim_start_matches('/');
    if raw.contains("..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }

    let wanted = if raw.is_empty() { "index.html" } else { raw };
    let Some((served_path, bytes)) = resolve_embedded_asset(wanted) else {
        return (StatusCode::NOT_FOUND, "missing ui").into_response();
    };
    let mime = embedded_ui::guess_mime(&served_path);

    let mut out = Response::new(axum::body::Body::from(bytes));
    out.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_str(&mime)
            .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream")),
    );
    append_no_store_for_html_documents(&mut out);
    out
}

fn is_html_document_response(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .map(str::trim)
                .is_some_and(|mime| mime.eq_ignore_ascii_case("text/html"))
        })
        .unwrap_or(false)
}

pub(super) fn append_no_store_for_html_documents(response: &mut Response) {
    if !is_html_document_response(response) {
        return;
    }

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

/// 函数 `resolve_embedded_asset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn resolve_embedded_asset(path: &str) -> Option<(String, &'static [u8])> {
    let raw = path.trim_start_matches('/');
    let trimmed = raw.trim_end_matches('/');
    let mut candidates = Vec::with_capacity(3);

    if raw.is_empty() {
        candidates.push("index.html".to_string());
    } else {
        candidates.push(raw.to_string());
        if !trimmed.is_empty() && !looks_like_asset_path(trimmed) {
            candidates.push(format!("{trimmed}/index.html"));
        }
    }

    for candidate in candidates {
        if let Some(bytes) = embedded_ui::read_asset_bytes(&candidate) {
            return Some((candidate, bytes));
        }
    }

    if looks_like_asset_path(raw) {
        return None;
    }

    embedded_ui::read_asset_bytes("index.html").map(|bytes| ("index.html".to_string(), bytes))
}

#[cfg(all(test, feature = "embedded-ui"))]
#[path = "ui_assets_tests.rs"]
mod tests;
