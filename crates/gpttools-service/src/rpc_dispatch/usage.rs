use gpttools_core::rpc::types::{JsonRpcRequest, JsonRpcResponse, UsageListResult, UsageReadResult};
use serde_json::Value;

use crate::{usage_list, usage_read, usage_refresh};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "account/usage/read" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str());
            let result = UsageReadResult {
                snapshot: usage_read::read_usage_snapshot(account_id),
            };
            serde_json::to_value(result).unwrap_or(Value::Null)
        }
        "account/usage/list" => {
            let result = UsageListResult {
                items: usage_list::read_usage_snapshots(),
            };
            serde_json::to_value(result).unwrap_or(Value::Null)
        }
        "account/usage/refresh" => {
            let account_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("accountId"))
                .and_then(|v| v.as_str());
            let result = match account_id {
                Some(account_id) => usage_refresh::refresh_usage_for_account(account_id),
                None => usage_refresh::refresh_usage_for_all_accounts(),
            };
            match result {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            }
        }
        _ => return None,
    };

    Some(JsonRpcResponse { id: req.id, result })
}
