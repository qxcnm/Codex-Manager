use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_adapter_probe_job_start(
    addr: Option<String>,
    pool_id: String,
    credential_ids: Vec<String>,
    concurrency: Option<usize>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "adapterProbe/job/start",
        addr,
        Some(serde_json::json!({
            "poolId": pool_id,
            "credentialIds": credential_ids,
            "concurrency": concurrency,
        })),
    )
    .await
}

#[tauri::command]
pub async fn service_adapter_probe_job_read(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "adapterProbe/job/read",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}

#[tauri::command]
pub async fn service_adapter_probe_job_latest(
    addr: Option<String>,
    pool_id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "adapterProbe/job/latest",
        addr,
        Some(serde_json::json!({ "poolId": pool_id })),
    )
    .await
}

#[tauri::command]
pub async fn service_adapter_probe_job_cancel(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "adapterProbe/job/cancel",
        addr,
        Some(serde_json::json!({ "id": id })),
    )
    .await
}
