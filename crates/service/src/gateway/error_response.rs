use crate::http::gateway_response::{Header, Response};
use serde_json::json;

#[allow(clippy::too_many_arguments)]
pub(crate) fn record_rejected_request(
    storage: Option<&codexmanager_core::storage::Storage>,
    trace_id: &str,
    path: &str,
    method: &str,
    request_type: &str,
    status: u16,
    message: &str,
    key_id: Option<&str>,
) {
    super::trace_log::log_request_start(
        trace_id,
        "-",
        method,
        path,
        None,
        None,
        None,
        false,
        request_type,
        "-",
    );
    super::trace_log::log_request_final(trace_id, status, None, None, Some(message), 0);
    super::record_gateway_request_outcome(path, status, None);
    if let Some(storage) = storage {
        super::write_request_log(
            storage,
            super::request_log::RequestLogTraceContext {
                trace_id: Some(trace_id),
                original_path: Some(path),
                adapted_path: Some(path),
                request_type: Some(request_type),
                ..Default::default()
            },
            key_id,
            None,
            path,
            method,
            None,
            None,
            None,
            Some(status),
            super::request_log::RequestLogUsage::default(),
            Some(message),
            None,
        );
    }
}

/// 函数 `with_trace_id_header`
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
pub(super) fn with_trace_id_header<R: std::io::Read>(
    mut response: Response<R>,
    trace_id: Option<&str>,
) -> Response<R> {
    if let Some(trace_id) = trace_id.map(str::trim).filter(|value| !value.is_empty()) {
        if let Ok(header) = Header::from_bytes(
            crate::error_codes::TRACE_ID_HEADER_NAME.as_bytes(),
            trace_id.as_bytes(),
        ) {
            response.add_header(header);
        }
    }
    response
}

/// 函数 `terminal_text_response`
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
pub(crate) fn terminal_text_response(
    status_code: u16,
    message: impl Into<String>,
    trace_id: Option<&str>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let message = crate::gateway::error_message_for_client(false, message);
    let code = crate::error_codes::code_for_message(message.as_str()).to_string();
    let error_type = match status_code {
        400 => "invalid_request_error",
        401 => "authentication_error",
        402 => "insufficient_quota",
        403 => "permission_error",
        404 => "invalid_request_error",
        429 => "rate_limit_error",
        500..=599 => "server_error",
        _ => "api_error",
    };
    let body = json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
        }
    })
    .to_string();
    let mut response = Response::from_string(body).with_status_code(status_code);
    if let Ok(header) = Header::from_bytes(
        b"Content-Type".as_slice(),
        b"application/json; charset=utf-8".as_slice(),
    ) {
        response.add_header(header);
    }
    if let Ok(header) = Header::from_bytes(
        crate::error_codes::ERROR_CODE_HEADER_NAME.as_bytes(),
        code.as_bytes(),
    ) {
        response.add_header(header);
    }
    with_trace_id_header(response, trace_id)
}

#[cfg(test)]
#[path = "tests/error_response_tests.rs"]
mod tests;
