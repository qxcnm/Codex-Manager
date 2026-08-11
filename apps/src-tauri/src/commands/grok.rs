use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_grok_credentials_list(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("grok/credentials/list", addr, None).await
}

#[tauri::command]
pub async fn service_grok_credential_probe_models(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "grok/credentials/probeModels",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}

#[tauri::command]
pub async fn service_grok_credential_set_enabled(
    addr: Option<String>,
    id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "grok/credentials/setEnabled",
        addr,
        Some(serde_json::json!({ "id": id, "enabled": enabled })),
    )
    .await
}

#[tauri::command]
pub async fn service_grok_credential_delete(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "grok/credentials/delete",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}

#[tauri::command]
pub async fn service_grok_import_preview(
    addr: Option<String>,
    text: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "grok/import/preview",
        addr,
        Some(serde_json::json!({ "text": text })),
    )
    .await
}

#[tauri::command]
pub async fn service_grok_import_commit(
    addr: Option<String>,
    text: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "grok/import/commit",
        addr,
        Some(serde_json::json!({ "text": text })),
    )
    .await
}
