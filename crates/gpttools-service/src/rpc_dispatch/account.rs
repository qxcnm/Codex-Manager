use gpttools_core::rpc::types::{AccountListResult, JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;

use crate::{account_delete, account_list, account_update, auth_login, auth_tokens};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "account/list" => {
            let items = account_list::read_accounts();
            let result = AccountListResult { items };
            serde_json::to_value(result).unwrap_or(Value::Null)
        }
        "account/delete" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match account_delete::delete_account(account_id) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            }
        }
        "account/update" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let sort = req
                .params
                .as_ref()
                .and_then(|v| v.get("sort"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            match account_update::update_account_sort(account_id, sort) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            }
        }
        "account/login/start" => {
            let login_type = req
                .params
                .as_ref()
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("chatgpt");
            let open_browser = req
                .params
                .as_ref()
                .and_then(|v| v.get("openBrowser"))
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let note = req
                .params
                .as_ref()
                .and_then(|v| v.get("note"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let tags = req
                .params
                .as_ref()
                .and_then(|v| v.get("tags"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let group_name = req
                .params
                .as_ref()
                .and_then(|v| v.get("groupName"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let workspace_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("workspaceId"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
                .and_then(|v| if v.trim().is_empty() { None } else { Some(v) });
            match auth_login::login_start(
                login_type,
                open_browser,
                note,
                tags,
                group_name,
                workspace_id,
            ) {
                Ok(result) => serde_json::to_value(result).unwrap_or(Value::Null),
                Err(err) => serde_json::json!({ "error": err }),
            }
        }
        "account/login/status" => {
            let login_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("loginId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let result = auth_login::login_status(login_id);
            serde_json::to_value(result).unwrap_or(Value::Null)
        }
        "account/login/complete" => {
            let state = req
                .params
                .as_ref()
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let code = req
                .params
                .as_ref()
                .and_then(|v| v.get("code"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let redirect_uri = req
                .params
                .as_ref()
                .and_then(|v| v.get("redirectUri"))
                .and_then(|v| v.as_str());
            if state.is_empty() || code.is_empty() {
                serde_json::json!({"ok": false, "error": "missing code/state"})
            } else {
                match auth_tokens::complete_login_with_redirect(state, code, redirect_uri) {
                    Ok(_) => serde_json::json!({ "ok": true }),
                    Err(err) => serde_json::json!({ "ok": false, "error": err }),
                }
            }
        }
        _ => return None,
    };

    Some(JsonRpcResponse { id: req.id, result })
}
