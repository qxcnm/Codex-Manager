use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};

/// Preserve the regular RPC authorization before any plugin URL is fetched or
/// any script is run. HTTP awaits are native; the Rhai host remains synchronous.
pub(crate) async fn try_handle_plugin_request_async(
    req: &JsonRpcRequest,
    actor: &crate::RpcActor,
) -> Option<JsonRpcMessage> {
    if !matches!(
        req.method.as_str(),
        "plugin/catalog/list"
            | "plugin/catalog/refresh"
            | "plugin/install"
            | "plugin/update"
            | "plugin/tasks/run"
    ) {
        return None;
    }
    if let Err(error) = super::ensure_method_allowed(actor, &req.method) {
        return Some(JsonRpcMessage::Response(super::response(
            req,
            super::value_or_error::<()>(Err(error)),
        )));
    }
    crate::plugin::try_handle_async(req)
        .await
        .map(JsonRpcMessage::Response)
}
