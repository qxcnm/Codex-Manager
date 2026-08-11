use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

use crate::{
    grok::{import, runtime},
    storage_helpers,
};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "grok/credentials/list" => {
            let value = storage_helpers::open_storage()
                .ok_or_else(|| "storage_unavailable".to_string())
                .and_then(|storage| {
                    let records = storage
                        .list_grok_credentials()
                        .map_err(|_| "credential storage operation failed".to_string())?;
                    let models = storage
                        .list_grok_credential_model_availability(None)
                        .map_err(|_| "credential storage operation failed".to_string())?;
                    let quota = storage
                        .list_grok_quota_windows(None)
                        .map_err(|_| "credential storage operation failed".to_string())?;
                    records
                        .into_iter()
                        .map(|record| {
                            let id = record.id.clone();
                            let mut value = serde_json::to_value(record)
                                .map_err(|_| "serialize_grok_credential_failed".to_string())?;
                            if let Some(object) = value.as_object_mut() {
                                object.insert(
                                    "availableModels".into(),
                                    serde_json::json!(models
                                        .iter()
                                        .filter(|item| item.credential_id == id
                                            && item.status == "available")
                                        .map(|item| item.model_slug.clone())
                                        .collect::<Vec<_>>()),
                                );
                                object.insert(
                                    "quotaWindows".into(),
                                    serde_json::json!(quota
                                        .iter()
                                        .filter(|item| item.credential_id == id)
                                        .cloned()
                                        .collect::<Vec<_>>()),
                                );
                            }
                            Ok(value)
                        })
                        .collect::<Result<Vec<_>, String>>()
                });
            super::value_or_error(value)
        }
        "grok/credentials/probeModels" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| runtime::probe_credential_models(&storage, id))
            };
            super::value_or_error(value)
        }
        "grok/credentials/setEnabled" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let enabled = super::bool_param(req, "enabled").unwrap_or(false);
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| {
                        storage
                            .set_grok_credential_enabled(id, enabled)
                            .map_err(|_| "credential storage operation failed".to_string())
                    })
            };
            super::value_or_error(value)
        }
        "grok/credentials/delete" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| {
                        storage
                            .delete_grok_credential(id)
                            .map_err(|_| "credential storage operation failed".to_string())
                    })
            };
            super::value_or_error(value)
        }
        "grok/import/preview" => {
            let input = super::str_param(req, "text").unwrap_or("");
            super::value_or_error(
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| import::preview_text_with_storage(&storage, input)),
            )
        }
        "grok/import/commit" => {
            let input = super::str_param(req, "text").unwrap_or("");
            super::value_or_error(
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| import::import_text(&storage, input)),
            )
        }
        _ => return None,
    };
    Some(super::response(req, result))
}
