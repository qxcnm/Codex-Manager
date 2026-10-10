use crate::http::gateway_request::GatewayRequest as Request;
use crate::http::gateway_response::Response;

pub(super) struct LocalResponseContext<'a> {
    pub(super) trace_id: &'a str,
    pub(super) key_id: &'a str,
    pub(super) protocol_type: &'a str,
    pub(super) original_path: &'a str,
    pub(super) path: &'a str,
    pub(super) response_adapter: super::ResponseAdapter,
    pub(super) request_method: &'a str,
    pub(super) model_for_log: Option<&'a str>,
    pub(super) reasoning_for_log: Option<&'a str>,
    pub(super) storage: &'a codexmanager_core::storage::Storage,
}

/// 函数 `record_local_result`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - ctx: 参数 ctx
/// - status_code: 参数 status_code
/// - usage: 参数 usage
/// - error: 参数 error
///
/// # 返回
/// 无
pub(super) fn record_local_result(
    ctx: &LocalResponseContext<'_>,
    status_code: u16,
    usage: super::request_log::RequestLogUsage,
    error: Option<&str>,
) {
    super::trace_log::log_attempt_result(ctx.trace_id, "-", None, status_code, error);
    super::trace_log::log_request_final(ctx.trace_id, status_code, None, None, error, 0);
    super::record_gateway_request_outcome(ctx.path, status_code, Some(ctx.protocol_type));
    super::write_request_log(
        ctx.storage,
        super::request_log::RequestLogTraceContext {
            trace_id: Some(ctx.trace_id),
            original_path: Some(ctx.original_path),
            adapted_path: Some(ctx.path),
            response_adapter: Some(ctx.response_adapter),
            ..Default::default()
        },
        Some(ctx.key_id),
        None,
        ctx.path,
        ctx.request_method,
        ctx.model_for_log,
        ctx.reasoning_for_log,
        None,
        Some(status_code),
        usage,
        error,
        None,
    );
}

/// 函数 `respond_local_json`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - request: 参数 request
/// - ctx: 参数 ctx
/// - body: 参数 body
/// - usage: 参数 usage
///
/// # 返回
/// 返回函数执行结果
pub(super) fn respond_local_json(
    request: Request,
    ctx: &LocalResponseContext<'_>,
    body: String,
    usage: super::request_log::RequestLogUsage,
) -> Result<(), String> {
    respond_local_json_with_headers(request, ctx, body, usage, Vec::new())
}

pub(super) fn respond_local_json_with_headers(
    request: Request,
    ctx: &LocalResponseContext<'_>,
    body: String,
    usage: super::request_log::RequestLogUsage,
    extra_headers: Vec<crate::http::gateway_response::Header>,
) -> Result<(), String> {
    record_local_result(ctx, 200, usage, None);
    let mut response = super::error_response::with_trace_id_header(
        Response::from_string(body)
            .with_status_code(200)
            .with_header(
                crate::http::gateway_response::Header::from_bytes(
                    b"content-type".as_slice(),
                    b"application/json".as_slice(),
                )
                .map_err(|_| "build content-type header failed".to_string())?,
            ),
        Some(ctx.trace_id),
    );
    for header in extra_headers {
        response = response.with_header(header);
    }
    let _ = request.respond(response);
    Ok(())
}

/// 函数 `respond_local_terminal_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - request: 参数 request
/// - ctx: 参数 ctx
/// - status_code: 参数 status_code
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
pub(super) fn respond_local_terminal_error(
    request: Request,
    ctx: &LocalResponseContext<'_>,
    status_code: u16,
    message: String,
) -> Result<(), String> {
    record_local_result(
        ctx,
        status_code,
        super::request_log::RequestLogUsage::default(),
        Some(message.as_str()),
    );
    let response_message = super::error_message_for_client(
        super::prefers_raw_errors_for_gateway_request(&request),
        message,
    );
    let response = super::error_response::terminal_text_response(
        status_code,
        response_message,
        Some(ctx.trace_id),
    );
    let _ = request.respond(response);
    Ok(())
}
