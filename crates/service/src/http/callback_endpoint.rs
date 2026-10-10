#[cfg(test)]
use tiny_http::Request;

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

static CALLBACK_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

/// 函数 `handle_callback`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
///
/// # 返回
/// 无
#[cfg(test)]
pub fn handle_callback(request: Request) {
    if let Err(err) = crate::auth_callback::handle_login_request(request) {
        log::warn!("callback request error: {err}");
    }
}

/// Axum callback endpoint shares login state validation and error rendering
/// with the standalone Axum OAuth callback listener.
pub(crate) async fn handle_callback_http(uri: axum::http::Uri) -> Response {
    let permit = match CALLBACK_WORKERS.try_acquire() {
        Ok(permit) => permit,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, "service busy").into_response(),
    };
    let raw_url = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(uri.path())
        .to_owned();
    let _permit = permit;
    let result = crate::auth_callback::process_login_callback_url_async(&raw_url).await;
    match result {
        Ok(()) => Html(crate::auth_callback::callback_success_page()).into_response(),
        Err(err) if err == "not found" => (StatusCode::NOT_FOUND, "Not Found").into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(crate::auth_callback::callback_error_page(&err)),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::handle_callback_http;
    use axum::body::to_bytes;
    use axum::http::{StatusCode, Uri};

    #[tokio::test]
    async fn axum_callback_rejects_non_callback_path() {
        let response = handle_callback_http(Uri::from_static("/auth/other")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn axum_callback_returns_html_error_for_missing_state() {
        let response = handle_callback_http(Uri::from_static("/auth/callback?code=abc")).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some("text/html; charset=utf-8")
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(String::from_utf8_lossy(&body).contains("Login Failed"));
    }
}
