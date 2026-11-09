use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_account_bps_update(
    addr: Option<String>,
    account_ids: Vec<String>,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "account/bps/update",
        addr,
        Some(serde_json::json!({ "accountIds": account_ids, "enabled": enabled })),
    )
    .await
}

#[tauri::command]
pub async fn service_account_bps_login_start(
    addr: Option<String>,
    account_id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "account/bps/login/start",
        addr,
        Some(serde_json::json!({ "accountId": account_id })),
    )
    .await
}

#[tauri::command]
pub async fn service_account_bps_login_status(
    addr: Option<String>,
    account_id: String,
    login_id: String,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "account/bps/login/status",
        addr,
        Some(serde_json::json!({ "accountId": account_id, "loginId": login_id })),
    )
    .await
}
