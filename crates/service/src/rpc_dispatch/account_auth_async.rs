use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) async fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "account/read" => {
            let refresh = super::bool_param(req, "refreshToken")
                .or_else(|| super::bool_param(req, "refresh_token"))
                .unwrap_or(false);
            super::value_or_error(crate::auth_account::read_current_account_async(refresh).await)
        }
        "account/chatgptAuthTokens/refresh" => {
            let id = [
                "accountId",
                "account_id",
                "previousAccountId",
                "previous_account_id",
            ]
            .iter()
            .find_map(|key| super::str_param(req, key));
            super::value_or_error(
                crate::auth_account::refresh_current_chatgpt_auth_tokens_async(id).await,
            )
        }
        "account/chatgptAuthTokens/refreshAll" => super::value_or_error(
            crate::auth_account::refresh_all_chatgpt_auth_tokens_async().await,
        ),
        _ => return None,
    };
    Some(super::response(req, result))
}
