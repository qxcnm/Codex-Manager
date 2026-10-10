use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};

/// Native OAuth network calls retain the same RPC permission and payload rules.
pub(crate) async fn try_handle_auth_request_async(
    req: &JsonRpcRequest,
    actor: &crate::RpcActor,
) -> Option<JsonRpcMessage> {
    if !matches!(
        req.method.as_str(),
        "account/login/start" | "account/login/complete"
    ) {
        return None;
    }
    if let Err(error) = super::ensure_method_allowed(actor, &req.method) {
        return Some(JsonRpcMessage::Response(super::response(
            req,
            super::value_or_error::<()>(Err(error)),
        )));
    }
    let result = if req.method == "account/login/complete" {
        let state = super::str_param(req, "state").unwrap_or("");
        let code = super::str_param(req, "code").unwrap_or("");
        if state.is_empty() || code.is_empty() {
            serde_json::json!({"ok": false, "error": "missing code/state"})
        } else {
            super::ok_or_error(
                crate::auth_tokens::complete_login_with_redirect_async(
                    state,
                    code,
                    super::str_param(req, "redirectUri"),
                )
                .await,
            )
        }
    } else {
        let login_type = super::str_param(req, "type").unwrap_or("chatgpt");
        if login_type.eq_ignore_ascii_case("chatgptAuthTokens") {
            return None;
        }
        super::value_or_error(
            crate::auth_login::login_start_async(
                login_type,
                super::bool_param(req, "openBrowser").unwrap_or(true),
                super::string_param(req, "note"),
                super::string_param(req, "tags"),
                super::string_param(req, "groupName"),
                super::string_param(req, "workspaceId").filter(|value| !value.trim().is_empty()),
            )
            .await,
        )
    };
    Some(JsonRpcMessage::Response(super::response(req, result)))
}
