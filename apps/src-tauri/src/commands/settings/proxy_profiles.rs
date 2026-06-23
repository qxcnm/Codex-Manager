use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_system_proxy_list(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("system/proxy/list", addr, None).await
}

#[tauri::command]
pub async fn service_system_proxy_test_presets(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("system/proxy/test-presets", addr, None).await
}

#[tauri::command]
pub async fn service_system_proxy_create(
    addr: Option<String>,
    name: String,
    proxy_url: String,
    enabled: Option<bool>,
    tags_json: Option<String>,
    notes: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "name": name,
        "proxyUrl": proxy_url,
        "enabled": enabled,
        "tagsJson": tags_json,
        "notes": notes,
    });
    rpc_call_in_background("system/proxy/create", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_update(
    addr: Option<String>,
    id: String,
    name: Option<String>,
    proxy_url: Option<String>,
    enabled: Option<bool>,
    tags_json: Option<String>,
    notes: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "name": name,
        "proxyUrl": proxy_url,
        "enabled": enabled,
        "tagsJson": tags_json,
        "notes": notes,
    });
    rpc_call_in_background("system/proxy/update", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_delete(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": id });
    rpc_call_in_background("system/proxy/delete", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_test(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": id });
    rpc_call_in_background("system/proxy/test", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_test_latency(
    addr: Option<String>,
    id: String,
    preset_id: Option<String>,
    custom_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "presetId": preset_id,
        "customUrl": custom_url,
    });
    rpc_call_in_background("system/proxy/test-latency", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_speed_test(
    addr: Option<String>,
    id: String,
    provider_id: Option<String>,
    file_size_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "providerId": provider_id,
        "fileSizeId": file_size_id,
    });
    rpc_call_in_background("system/proxy/speed-test", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_cloudflare_speed_test(
    addr: Option<String>,
    id: String,
    config: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "config": config,
    });
    rpc_call_in_background("system/proxy/cloudflare-speed-test", addr, Some(params)).await
}


#[tauri::command]
pub async fn service_system_proxy_test_job(
    addr: Option<String>,
    job_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "jobId": job_id,
    });
    rpc_call_in_background("system/proxy/test-job", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_cancel_test(
    addr: Option<String>,
    job_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "jobId": job_id,
    });
    rpc_call_in_background("system/proxy/cancel-test", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_speed_test_history(
    addr: Option<String>,
    id: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "limit": limit,
    });
    rpc_call_in_background("system/proxy/speed-test-history", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_latency_test_history(
    addr: Option<String>,
    id: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "limit": limit,
    });
    rpc_call_in_background("system/proxy/latency-test-history", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_system_proxy_diagnostics_history(
    addr: Option<String>,
    id: String,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "limit": limit,
    });
    rpc_call_in_background("system/proxy/diagnostics-history", addr, Some(params)).await
}

