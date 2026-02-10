use gpttools_core::rpc::types::{JsonRpcRequest, JsonRpcResponse, RequestLogListResult};
use serde_json::Value;

use crate::{requestlog_clear, requestlog_list};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "requestlog/list" => {
            let query = req
                .params
                .as_ref()
                .and_then(|v| v.get("query"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let limit = req
                .params
                .as_ref()
                .and_then(|v| v.get("limit"))
                .and_then(|v| v.as_i64());
            let result = RequestLogListResult {
                items: requestlog_list::read_request_logs(query, limit),
            };
            serde_json::to_value(result).unwrap_or(Value::Null)
        }
        "requestlog/clear" => match requestlog_clear::clear_request_logs() {
            Ok(_) => serde_json::json!({ "ok": true }),
            Err(err) => serde_json::json!({ "ok": false, "error": err }),
        },
        _ => return None,
    };

    Some(JsonRpcResponse { id: req.id, result })
}
