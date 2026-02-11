use tiny_http::{Request, Response};

pub(crate) fn handle_gateway_request(mut request: Request) -> Result<(), String> {
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

    let _request_guard = super::begin_gateway_request();
    let request_path_for_log = super::normalize_models_path(request.url());
    let request_method_for_log = request.method().as_str().to_string();
    let validated = match super::local_validation::prepare_local_request(&mut request, debug) {
        Ok(v) => v,
        Err(err) => {
            if let Some(storage) = super::open_storage() {
                super::write_request_log(
                    &storage,
                    None,
                    &request_path_for_log,
                    &request_method_for_log,
                    None,
                    None,
                    None,
                    Some(err.status_code),
                    Some(err.message.as_str()),
                );
            }
            let response = Response::from_string(err.message).with_status_code(err.status_code);
            let _ = request.respond(response);
            return Ok(());
        }
    };

    super::proxy_validated_request(request, validated, debug)
}

