use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, Response, StatusCode};

#[cfg(test)]
use crate::http::header_filter::should_skip_response_header;

/// 函数 `text_response`
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
pub(crate) fn text_response(status: StatusCode, body: impl Into<String>) -> Response<Body> {
    let mut response = Response::new(Body::from(body.into()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

/// 函数 `text_error_response`
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
pub(crate) fn text_error_response(status: StatusCode, body: impl Into<String>) -> Response<Body> {
    let body = crate::gateway::error_message_for_client(false, body);
    let mut response = text_response(status, body.clone());
    response.headers_mut().insert(
        crate::error_codes::ERROR_CODE_HEADER_NAME,
        HeaderValue::from_static(crate::error_codes::code_for_message(body.as_str())),
    );
    response
}

/// Preserve the domain gateway's JSON error and diagnostic headers for errors
/// rejected before an upload is read.
pub(crate) fn gateway_error_response(
    status: StatusCode,
    message: impl Into<String>,
    trace_id: &str,
) -> Response<Body> {
    let error = crate::gateway::terminal_text_response(status.as_u16(), message, Some(trace_id));
    let mut response = Response::builder().status(status);
    for header in error.headers() {
        response = response.header(header.field.as_str(), header.value.as_str());
    }
    response
        .body(Body::from(error.into_reader().into_inner()))
        .expect("validated gateway response headers")
}

/// 函数 `merge_upstream_headers`
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
pub(crate) fn merge_upstream_headers(
    mut builder: axum::http::response::Builder,
    headers: &reqwest::header::HeaderMap,
) -> axum::http::response::Builder {
    for (name, value) in headers.iter() {
        if should_skip_response_header(name) {
            continue;
        }
        builder = builder.header(name, value);
    }
    builder
}

#[cfg(test)]
#[path = "tests/proxy_response_tests.rs"]
mod tests;
