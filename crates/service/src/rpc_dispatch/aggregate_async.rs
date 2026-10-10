//! Network-bearing aggregate RPC methods await HTTP without a domain worker.
use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};

pub(crate) async fn try_handle_aggregate_request_async(
    req: &JsonRpcRequest,
    actor: &crate::RpcActor,
) -> Option<JsonRpcMessage> {
    if !matches!(
        req.method.as_str(),
        "aggregateApi/testConnection" | "aggregateApi/refreshBalance" | "aggregateApi/fetchModels"
    ) {
        return None;
    }
    if let Err(error) = super::ensure_method_allowed(actor, &req.method) {
        return Some(JsonRpcMessage::Response(super::response(
            req,
            super::value_or_error::<()>(Err(error)),
        )));
    }
    // Keep the explicit model-discovery permission check in addition to the
    // common method allowlist, matching the synchronous dispatcher contract.
    if req.method == "aggregateApi/fetchModels" && !actor.is_admin() {
        return Some(JsonRpcMessage::Response(super::response(
            req,
            super::value_or_error::<()>(Err(
                "permission_denied: aggregateApi/fetchModels".to_owned()
            )),
        )));
    }
    let api_id = super::str_param(req, "id")
        .or_else(|| super::str_param(req, "apiId"))
        .unwrap_or("");
    let result = match req.method.as_str() {
        "aggregateApi/testConnection" => super::value_or_error(
            crate::aggregate_api::test_aggregate_api_connection_async(api_id).await,
        ),
        "aggregateApi/refreshBalance" => super::value_or_error(
            crate::aggregate_api::refresh_aggregate_api_balance_async(api_id).await,
        ),
        _ => super::value_or_error(
            crate::aggregate_api::fetch_aggregate_api_models_async(api_id).await,
        ),
    };
    Some(JsonRpcMessage::Response(super::response(req, result)))
}
