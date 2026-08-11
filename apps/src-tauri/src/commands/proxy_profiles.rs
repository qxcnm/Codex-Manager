use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_proxy_profiles_list(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("proxyProfiles/list", addr, None).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn service_proxy_profile_upsert(
    addr: Option<String>,
    id: Option<String>,
    name: String,
    proxy_url: String,
    username: Option<String>,
    password: Option<String>,
    keep_existing_password: Option<bool>,
    status: Option<String>,
    fallback_mode: Option<String>,
    backup_proxy_id: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("proxyProfiles/upsert", addr, Some(serde_json::json!({
        "id": id, "name": name, "proxyUrl": proxy_url, "username": username,
        "password": password, "keepExistingPassword": keep_existing_password.unwrap_or(true),
        "status": status, "fallbackMode": fallback_mode, "backupProxyId": backup_proxy_id,
    }))).await
}

#[tauri::command]
pub async fn service_proxy_profile_delete(addr: Option<String>, id: String) -> Result<serde_json::Value, String> {
    rpc_call_in_background("proxyProfiles/delete", addr, Some(serde_json::json!({ "id": id }))).await
}

#[tauri::command]
pub async fn service_proxy_profile_probe(addr: Option<String>, id: String) -> Result<serde_json::Value, String> {
    rpc_call_in_background("proxyProfiles/probe", addr, Some(serde_json::json!({ "id": id }))).await
}

#[tauri::command]
pub async fn service_proxy_profile_bind_accounts(
    addr: Option<String>,
    account_ids: Vec<String>,
    mode: String,
    proxy_profile_id: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("proxyProfiles/bindAccounts", addr, Some(serde_json::json!({
        "accountIds": account_ids, "mode": mode, "proxyProfileId": proxy_profile_id,
    }))).await
}
