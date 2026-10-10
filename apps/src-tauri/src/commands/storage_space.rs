use crate::commands::shared::rpc_call_in_background;

/// Local database space usage (administrator only).
#[tauri::command]
pub async fn service_storage_space_usage(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("storage/spaceUsage", addr, None).await
}

/// Reclaim free database pages. `rebuild` runs the one-time conversion of an
/// older database to incremental auto-vacuum.
#[tauri::command]
pub async fn service_storage_reclaim(
    addr: Option<String>,
    rebuild: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "rebuild": rebuild.unwrap_or(false) });
    rpc_call_in_background("storage/reclaim", addr, Some(params)).await
}
