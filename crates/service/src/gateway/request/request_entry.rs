use crate::http::gateway_request::GatewayRequest as Request;
use crate::http::gateway_response::Response;

/// 函数 `handle_gateway_request`
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
pub(crate) async fn handle_gateway_request_async(mut request: Request) -> Result<(), String> {
    // 处理代理请求（鉴权后转发到上游）
    let debug = super::DEFAULT_GATEWAY_DEBUG;
    if request.method().as_str() == "OPTIONS" {
        let response = Response::empty(204);
        let _ = request.respond(response);
        return Ok(());
    }

    if request.url() == "/health" {
        let response = Response::from_string("ok");
        let _ = request.respond(response);
        return Ok(());
    }

    request.hold_until_complete(super::begin_gateway_request());
    let trace_id = super::trace_log::next_trace_id();
    let request_path_for_log = super::normalize_models_path(request.url());
    let request_method_for_log = request.method().as_str().to_string();
    let validated =
        match super::local_validation::prepare_local_request(&mut request, trace_id.clone(), debug)
        {
            Ok(v) => v,
            Err(err) => {
                super::trace_log::log_request_start(
                    trace_id.as_str(),
                    "-",
                    request_method_for_log.as_str(),
                    request_path_for_log.as_str(),
                    None,
                    None,
                    None,
                    false,
                    "http",
                    "-",
                );
                super::trace_log::log_request_final(
                    trace_id.as_str(),
                    err.status_code,
                    None,
                    None,
                    Some(err.message.as_str()),
                    0,
                );
                super::record_gateway_request_outcome(
                    request_path_for_log.as_str(),
                    err.status_code,
                    None,
                );
                if let Some(storage) = super::open_storage() {
                    super::write_request_log(
                        &storage,
                        super::request_log::RequestLogTraceContext {
                            trace_id: Some(trace_id.as_str()),
                            original_path: Some(request_path_for_log.as_str()),
                            adapted_path: Some(request_path_for_log.as_str()),
                            response_adapter: None,
                            ..Default::default()
                        },
                        err.key_id.as_deref(),
                        None,
                        &request_path_for_log,
                        &request_method_for_log,
                        None,
                        None,
                        None,
                        Some(err.status_code),
                        super::request_log::RequestLogUsage::default(),
                        Some(err.message.as_str()),
                        None,
                    );
                }
                let response_message = super::error_message_for_client(
                    super::prefers_raw_errors_for_gateway_request(&request),
                    err.message.as_str(),
                );
                let response = super::error_response::terminal_text_response(
                    err.status_code,
                    response_message,
                    Some(trace_id.as_str()),
                );
                let _ = request.respond(response);
                return Ok(());
            }
        };

    let request = match super::maybe_respond_local_models(
        request,
        validated.trace_id.as_str(),
        validated.key_id.as_str(),
        validated.protocol_type.as_str(),
        validated.original_path.as_str(),
        validated.path.as_str(),
        validated.response_adapter,
        validated.request_method.as_str(),
        validated.model_for_log.as_deref(),
        validated.reasoning_for_log.as_deref(),
        &validated.storage,
    )
    .await?
    {
        Some(request) => request,
        None => return Ok(()),
    };

    let trace_id_for_count_tokens = validated.trace_id.clone();
    let key_id_for_count_tokens = validated.key_id.clone();
    let protocol_type_for_count_tokens = validated.protocol_type.clone();
    let path_for_count_tokens = validated.path.clone();
    let request_method_for_count_tokens = validated.request_method.clone();
    let model_for_count_tokens = validated.model_for_log.clone();
    let reasoning_for_count_tokens = validated.reasoning_for_log.clone();
    let request = if validated.rotation_strategy == crate::apikey_profile::ROTATION_AGGREGATE_API {
        request
    } else {
        match super::maybe_respond_local_count_tokens(
            request,
            trace_id_for_count_tokens.as_str(),
            key_id_for_count_tokens.as_str(),
            protocol_type_for_count_tokens.as_str(),
            validated.original_path.as_str(),
            path_for_count_tokens.as_str(),
            validated.response_adapter,
            request_method_for_count_tokens.as_str(),
            validated.passthrough_body.as_ref(),
            model_for_count_tokens.as_deref(),
            reasoning_for_count_tokens.as_deref(),
            &validated.storage,
        )? {
            Some(request) => request,
            None => return Ok(()),
        }
    };

    super::proxy_validated_request(request, validated, debug).await
}

#[cfg(test)]
pub(crate) fn handle_gateway_request(request: Request) -> Result<(), String> {
    super::run_upstream_io(handle_gateway_request_async(request))?
}
