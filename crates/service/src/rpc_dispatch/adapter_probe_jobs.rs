use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

fn string_array_param(req: &JsonRpcRequest, key: &str) -> Vec<String> {
    req.params
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "adapterProbe/job/start" => {
            let pool_id = super::str_param(req, "poolId").unwrap_or("");
            let credential_ids = string_array_param(req, "credentialIds");
            let concurrency =
                super::i64_param(req, "concurrency").and_then(|value| usize::try_from(value).ok());
            super::value_or_error(crate::adapter_probe_jobs::start_adapter_probe_job(
                pool_id,
                credential_ids,
                concurrency,
            ))
        }
        "adapterProbe/job/read" => {
            let id = super::str_param(req, "id").unwrap_or("");
            super::value_or_error(crate::adapter_probe_jobs::get_adapter_probe_job(id))
        }
        "adapterProbe/job/latest" => {
            let pool_id = super::str_param(req, "poolId").unwrap_or("");
            super::value_or_error(crate::adapter_probe_jobs::latest_adapter_probe_job(pool_id))
        }
        "adapterProbe/job/cancel" => {
            let id = super::str_param(req, "id").unwrap_or("");
            super::value_or_error(crate::adapter_probe_jobs::cancel_adapter_probe_job(id))
        }
        _ => return None,
    };
    Some(super::response(req, result))
}
