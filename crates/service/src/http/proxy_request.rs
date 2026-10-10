use axum::http::HeaderMap;
#[cfg(test)]
use axum::http::Uri;

use crate::http::header_filter::should_skip_request_header;

/// 函数 `build_target_url`
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
pub(crate) fn build_target_url(backend_base_url: &str, uri: &Uri) -> String {
    // Keep the query when rewriting a test-fixture backend target.
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    format!("{backend_base_url}{path_and_query}")
}

/// 函数 `filter_request_headers`
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
pub(crate) fn filter_request_headers(headers: &HeaderMap) -> HeaderMap {
    let mut outbound_headers = HeaderMap::new();
    for (name, value) in headers.iter() {
        if should_skip_request_header(name, value) {
            continue;
        }
        let _ = outbound_headers.insert(name.clone(), value.clone());
    }
    outbound_headers
}

#[cfg(test)]
#[path = "tests/proxy_request_tests.rs"]
mod tests;
