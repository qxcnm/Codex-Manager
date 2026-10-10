use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

// Actor checks are performed by network_async before entering this dispatcher.
pub(super) async fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    if req.method != "account/usage/refresh" {
        return None;
    }
    let account_id =
        super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
    let result = match account_id {
        Some(id) => crate::usage_refresh::refresh_usage_for_account_result_async(id).await,
        None => crate::usage_refresh::refresh_usage_for_all_accounts_result_async().await,
    };
    Some(super::response(req, super::value_or_error(result)))
}
