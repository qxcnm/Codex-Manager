//! Administrator RPCs for local database space usage and reclamation.

use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "storage/spaceUsage" => {
            super::value_or_error(crate::storage_maintenance::space_usage_snapshot())
        }
        "storage/reclaim" => {
            let rebuild = super::bool_param(req, "rebuild").unwrap_or(false);
            super::value_or_error(crate::storage_maintenance::request_reclaim(rebuild))
        }
        _ => return None,
    };
    Some(super::response(req, result))
}
