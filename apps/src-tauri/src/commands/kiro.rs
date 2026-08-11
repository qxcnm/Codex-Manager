use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_kiro_credentials_list(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("kiro/credentials/list", addr, None).await
}

#[tauri::command]
pub async fn service_kiro_credential_probe_models(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/credentials/probeModels",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}

#[tauri::command]
pub async fn service_kiro_credential_set_enabled(
    addr: Option<String>,
    id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/credentials/setEnabled",
        addr,
        Some(serde_json::json!({ "id": id, "enabled": enabled })),
    )
    .await
}

#[tauri::command]
pub async fn service_kiro_credential_delete(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/credentials/delete",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn service_kiro_credential_update_routing(
    addr: Option<String>,
    id: String,
    priority: i64,
    weight: f64,
    auth_region: Option<String>,
    api_region: Option<String>,
    proxy_url: Option<String>,
    proxy_username: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/credentials/updateRouting",
        addr,
        Some(serde_json::json!({
            "id": id,
            "priority": priority,
            "weight": weight,
            "authRegion": auth_region,
            "apiRegion": api_region,
            "proxyUrl": proxy_url,
            "proxyUsername": proxy_username,
        })),
    )
    .await
}

#[tauri::command]
pub async fn service_kiro_credential_refresh(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/credentials/refresh",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}

#[tauri::command]
pub async fn service_kiro_credential_quota(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/credentials/quota",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}

#[tauri::command]
pub async fn service_kiro_import_preview(
    addr: Option<String>,
    json: String,
    mapping: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/import/preview",
        addr,
        Some(serde_json::json!({ "json": json, "mapping": mapping })),
    )
    .await
}

#[tauri::command]
pub async fn service_kiro_import_commit(
    addr: Option<String>,
    json: String,
    mapping: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "kiro/import/commit",
        addr,
        Some(serde_json::json!({ "json": json, "mapping": mapping })),
    )
    .await
}
