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
      <p>关闭：访问 <a href="/__quit">/__quit</a>。</p>
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
mod tests {
    use super::*;

    /// 函数 `spa_route_fallback_uses_html_content_type`
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
    fn spa_route_fallback_uses_html_content_type() {
        let response = serve_embedded_path("accounts");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/html")
        );
    }

    /// 函数 `directory_route_prefers_embedded_directory_index`
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
    fn html_document_uses_no_store_cache_headers() {
        let response = serve_embedded_path("settings");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store, no-cache, must-revalidate")
        );
        assert_eq!(
            response
                .headers()
                .get(header::PRAGMA)
                .and_then(|value| value.to_str().ok()),
            Some("no-cache")
        );
        assert_eq!(
            response
                .headers()
                .get(header::EXPIRES)
                .and_then(|value| value.to_str().ok()),
            Some("0")
        );
    }

    #[test]
    fn static_asset_does_not_get_no_store_cache_headers() {
        let response = serve_embedded_path("favicon.ico");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(header::CACHE_CONTROL).is_none());
    }

    #[test]
    fn directory_route_prefers_embedded_directory_index() {
        let (served_path, _) = resolve_embedded_asset("accounts/").expect("accounts asset");
        assert_eq!(served_path, "accounts/index.html");

        let (served_path, _) = resolve_embedded_asset("accounts").expect("accounts asset");
        assert_eq!(served_path, "accounts/index.html");
    }

    #[test]
    fn missing_embedded_asset_does_not_fallback_to_index_html() {
        assert!(resolve_embedded_asset("_next/static/chunks/missing-test.js").is_none());
        assert!(resolve_embedded_asset("missing-image.png").is_none());
    }
}
