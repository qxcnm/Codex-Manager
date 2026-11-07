use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_model_discovery_list(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("modelDiscovery/list", addr, None).await
}

#[tauri::command]
pub async fn service_model_discovery_update(
    addr: Option<String>,
    source_kind: String,
    source_id: String,
    enabled: bool,
    interval_minutes: Option<u64>,
) -> Result<serde_json::Value, String> {
    let mut params = serde_json::json!({
        "sourceKind": source_kind,
        "sourceId": source_id,
        "enabled": enabled,
    });
    if let Some(interval) = interval_minutes {
        params["intervalMinutes"] = interval.into();
    }
    rpc_call_in_background("modelDiscovery/update", addr, Some(params)).await
}
