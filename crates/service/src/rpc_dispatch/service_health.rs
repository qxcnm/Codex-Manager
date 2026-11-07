use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
pub(super) fn try_handle(req: &JsonRpcRequest, actor: &crate::RpcActor) -> Option<JsonRpcResponse> {
    if !matches!(
        req.method.as_str(),
        "serviceHealth/summary" | "serviceHealth/report" | "serviceHealth/resolve"
    ) {
        return None;
    }
    Some(super::response(
        req,
        super::value_or_error(crate::service_health::handle(
            &req.method,
            req.params.as_ref(),
            actor,
        )),
    ))
}
