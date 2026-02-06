use tiny_http::{Request, Response};

pub fn handle_rpc(mut request: Request) {
    let mut body = String::new();
    if request.as_reader().read_to_string(&mut body).is_err() {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }
    if body.trim().is_empty() {
        let _ = request.respond(Response::from_string("{}").with_status_code(400));
        return;
    }

    let req: gpttools_core::rpc::types::JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            let _ = request.respond(Response::from_string("{}").with_status_code(400));
            return;
        }
    };
    let resp = crate::handle_request(req);
    let json = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string());
    let _ = request.respond(Response::from_string(json));
}
