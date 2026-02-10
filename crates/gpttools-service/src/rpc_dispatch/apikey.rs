use gpttools_core::rpc::types::{ApiKeyListResult, JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;

use crate::{
    apikey_create, apikey_delete, apikey_disable, apikey_enable, apikey_list, apikey_models,
    apikey_update_model,
};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "apikey/list" => {
            let result = ApiKeyListResult {
                items: apikey_list::read_api_keys(),
            };
            serde_json::to_value(result).unwrap_or(Value::Null)
        }
        "apikey/create" => {
            let name = req
                .params
                .as_ref()
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let model_slug = req
                .params
                .as_ref()
                .and_then(|v| v.get("modelSlug"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let reasoning_effort = req
                .params
                .as_ref()
                .and_then(|v| v.get("reasoningEffort"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            match apikey_create::create_api_key(name, model_slug, reasoning_effort) {
                Ok(result) => serde_json::to_value(result).unwrap_or(Value::Null),
                Err(err) => serde_json::json!({ "error": err }),
            }
        }
        "apikey/models" => match apikey_models::read_model_options() {
            Ok(result) => serde_json::to_value(result).unwrap_or(Value::Null),
            Err(err) => serde_json::json!({ "error": err }),
        },
        "apikey/updateModel" => {
            let key_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let model_slug = req
                .params
                .as_ref()
                .and_then(|v| v.get("modelSlug"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            let reasoning_effort = req
                .params
                .as_ref()
                .and_then(|v| v.get("reasoningEffort"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            match apikey_update_model::update_api_key_model(key_id, model_slug, reasoning_effort) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            }
        }
        "apikey/delete" => {
            let key_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match apikey_delete::delete_api_key(key_id) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            }
        }
        "apikey/disable" => {
            let key_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match apikey_disable::disable_api_key(key_id) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            }
        }
        "apikey/enable" => {
            let key_id = req
                .params
                .as_ref()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match apikey_enable::enable_api_key(key_id) {
                Ok(_) => serde_json::json!({ "ok": true }),
                Err(err) => serde_json::json!({ "ok": false, "error": err }),
            }
        }
        _ => return None,
    };

    Some(JsonRpcResponse { id: req.id, result })
}
