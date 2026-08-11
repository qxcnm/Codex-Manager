use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

use crate::{
    kiro::{import, runtime},
    storage_helpers,
};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "kiro/credentials/list" => {
            let value = storage_helpers::open_storage()
                .ok_or_else(|| "storage_unavailable".to_string())
                .and_then(|storage| {
                    let records = storage
                        .list_kiro_credentials()
                        .map_err(|error| error.to_string())?;
                    let availability = storage
                        .list_kiro_credential_model_availability(None)
                        .map_err(|error| error.to_string())?;
                    records
                        .into_iter()
                        .map(|record| {
                            let id = record.id.clone();
                            let mut value =
                                serde_json::to_value(record).map_err(|error| error.to_string())?;
                            let available_models = availability
                                .iter()
                                .filter(|item| {
                                    item.credential_id == id && item.status == "available"
                                })
                                .map(|item| item.model_slug.clone())
                                .collect::<Vec<_>>();
                            let checked_at = availability
                                .iter()
                                .filter(|item| item.credential_id == id)
                                .map(|item| item.checked_at)
                                .max();
                            if let Some(object) = value.as_object_mut() {
                                object.insert(
                                    "availableModels".into(),
                                    serde_json::json!(available_models),
                                );
                                object.insert(
                                    "modelProbeCheckedAt".into(),
                                    serde_json::json!(checked_at),
                                );
                            }
                            Ok(value)
                        })
                        .collect::<Result<Vec<_>, String>>()
                });
            super::value_or_error(value)
        }
        "kiro/credentials/probeModels" => {
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
        "kiro/credentials/setEnabled" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let enabled = super::bool_param(req, "enabled").unwrap_or(false);
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| {
                        storage
                            .set_kiro_credential_enabled(id, enabled)
                            .map_err(|error| error.to_string())
                    })
            };
            super::value_or_error(value)
        }
        "kiro/credentials/delete" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| {
                        storage
                            .delete_kiro_credential(id)
                            .map_err(|error| error.to_string())
                    })
            };
            super::value_or_error(value)
        }
        "kiro/credentials/updateRouting" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let priority = super::i64_param(req, "priority").unwrap_or(0);
            let weight = req
                .params
                .as_ref()
                .and_then(|value| value.get("weight"))
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0);
            let optional_text = |key: &str| {
                super::str_param(req, key)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            };
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else if !(0..=10_000).contains(&priority) {
                Err("priority_out_of_range".to_string())
            } else if !weight.is_finite() || !(0.01..=100.0).contains(&weight) {
                Err("weight_out_of_range".to_string())
            } else {
                import::sanitize_proxy_settings(optional_text("proxyUrl")).and_then(
                    |(proxy_url, embedded_username, embedded_password)| {
                        storage_helpers::open_storage()
                            .ok_or_else(|| "storage_unavailable".to_string())
                            .and_then(|storage| {
                                storage
                                    .update_kiro_credential_routing(
                                        id,
                                        priority,
                                        weight,
                                        optional_text("authRegion"),
                                        optional_text("apiRegion"),
                                        proxy_url,
                                        optional_text("proxyUsername").or(embedded_username),
                                        embedded_password,
                                    )
                                    .map_err(|error| error.to_string())
                            })
                    },
                )
            };
            super::value_or_error(value)
        }
        "kiro/credentials/refresh" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| runtime::refresh_credential(&storage, id))
            };
            super::value_or_error(value)
        }
        "kiro/credentials/quota" => {
            let id = super::str_param(req, "id").unwrap_or("").trim();
            let value = if id.is_empty() {
                Err("credential_id_required".to_string())
            } else {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| runtime::query_credential_quota(&storage, id))
            };
            super::value_or_error(value)
        }
        "kiro/import/preview" => {
            let input = super::str_param(req, "json").unwrap_or("");
            let mapping = req
                .params
                .as_ref()
                .and_then(|value| value.get("mapping"))
                .filter(|value| !value.is_null())
                .map(|value| {
                    serde_json::from_value::<import::KiroImportMapping>(value.clone())
                        .map_err(|_| "invalid_import_mapping".to_string())
                })
                .transpose();
            super::value_or_error(mapping.and_then(|mapping| {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| {
                        import::preview_json_with_storage(&storage, input, mapping.as_ref())
                    })
            }))
        }
        "kiro/import/commit" => {
            let input = super::str_param(req, "json").unwrap_or("");
            let mapping = req
                .params
                .as_ref()
                .and_then(|value| value.get("mapping"))
                .filter(|value| !value.is_null())
                .map(|value| {
                    serde_json::from_value::<import::KiroImportMapping>(value.clone())
                        .map_err(|_| "invalid_import_mapping".to_string())
                })
                .transpose();
            let value = mapping.and_then(|mapping| {
                storage_helpers::open_storage()
                    .ok_or_else(|| "storage_unavailable".to_string())
                    .and_then(|storage| {
                        import::import_json_with_mapping(&storage, input, mapping.as_ref())
                    })
            });
            super::value_or_error(value)
        }
        _ => return None,
    };
    Some(super::response(req, result))
}
