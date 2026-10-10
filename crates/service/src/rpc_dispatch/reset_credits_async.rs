use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) async fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    if !matches!(
        req.method.as_str(),
        "account/usage/resetCredits" | "account/usage/resetCredit/consume"
    ) {
        return None;
    }
    let account_id =
        super::str_param(req, "accountId").or_else(|| super::str_param(req, "account_id"));
    let result = match account_id {
        None => super::value_or_error::<()>(Err("accountId is required".to_owned())),
        Some(id) if req.method == "account/usage/resetCredits" => {
            super::value_or_error(crate::usage_reset_credits::read_reset_credits_async(id).await)
        }
        Some(id) => match super::str_param(req, "operationId")
            .or_else(|| super::str_param(req, "operation_id"))
        {
            Some(operation_id) => super::value_or_error(
                crate::usage_reset_credits::consume_reset_credit_async(id, operation_id).await,
            ),
            None => super::value_or_error::<()>(Err("operationId is required".to_owned())),
        },
    };
    Some(super::response(req, result))
}
