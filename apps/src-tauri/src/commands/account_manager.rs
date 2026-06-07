use crate::commands::shared::rpc_call_in_background;

#[tauri::command]
pub async fn service_account_manager_status(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("accountManager/status", addr, None).await
}

#[tauri::command]
pub async fn service_account_manager_session_current(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("accountManager/session/current", addr, None).await
}

#[tauri::command]
pub async fn service_account_manager_profile_update(
    addr: Option<String>,
    display_name: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "displayName": display_name,
    });
    rpc_call_in_background("accountManager/profile/update", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_account_manager_password_change(
    addr: Option<String>,
    current_password: String,
    new_password: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "currentPassword": current_password,
        "newPassword": new_password,
    });
    rpc_call_in_background("accountManager/password/change", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_account_manager_users_list(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("accountManager/users/list", addr, None).await
}

#[tauri::command]
pub async fn service_account_manager_user_create(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("accountManager/users/create", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_account_manager_user_update(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("accountManager/users/update", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_account_manager_user_delete(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": id });
    rpc_call_in_background("accountManager/users/delete", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_account_manager_wallet_top_up(
    addr: Option<String>,
    owner_kind: String,
    owner_id: String,
    amount_credit_micros: i64,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "ownerKind": owner_kind,
        "ownerId": owner_id,
        "amountCreditMicros": amount_credit_micros,
        "note": note,
    });
    rpc_call_in_background("accountManager/wallet/topUp", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_account_manager_wallet_set_available(
    addr: Option<String>,
    owner_kind: String,
    owner_id: String,
    available_credit_micros: i64,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "ownerKind": owner_kind,
        "ownerId": owner_id,
        "availableCreditMicros": available_credit_micros,
        "note": note,
    });
    rpc_call_in_background("accountManager/wallet/setAvailable", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_account_manager_api_key_owners_list(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("accountManager/apiKeyOwners/list", addr, None).await
}

#[tauri::command]
pub async fn service_account_manager_api_key_owner_set(
    addr: Option<String>,
    key_id: String,
    owner_kind: String,
    owner_user_id: Option<String>,
    project_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "keyId": key_id,
        "ownerKind": owner_kind,
        "ownerUserId": owner_user_id,
        "projectId": project_id,
    });
    rpc_call_in_background("accountManager/apiKeyOwners/set", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_groups_list(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("modelGroups/list", addr, None).await
}

#[tauri::command]
pub async fn service_model_group_save(
    addr: Option<String>,
    name: String,
    id: Option<String>,
    description: Option<String>,
    status: Option<String>,
    sort: Option<i64>,
    is_default: Option<bool>,
    rate_multiplier_millis: Option<i64>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "id": id,
        "name": name,
        "description": description,
        "status": status,
        "sort": sort,
        "isDefault": is_default,
        "rateMultiplierMillis": rate_multiplier_millis,
    });
    rpc_call_in_background("modelGroups/save", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_group_delete(
    addr: Option<String>,
    id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": id });
    rpc_call_in_background("modelGroups/delete", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_group_models_set(
    addr: Option<String>,
    group_id: String,
    models: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "groupId": group_id,
        "models": models,
    });
    rpc_call_in_background("modelGroups/setModels", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_group_users_set(
    addr: Option<String>,
    group_id: String,
    user_ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "groupId": group_id,
        "userIds": user_ids,
    });
    rpc_call_in_background("modelGroups/setUsers", addr, Some(params)).await
}
