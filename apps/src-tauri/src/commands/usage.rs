use crate::commands::shared::rpc_call_in_background;

/// 函数 `service_usage_read`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_read(
    addr: Option<String>,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = account_id.map(|id| serde_json::json!({ "accountId": id }));
    rpc_call_in_background("account/usage/read", addr, params).await
}

/// 函数 `service_usage_list`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_list(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("account/usage/list", addr, None).await
}

/// 函数 `service_usage_aggregate`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_aggregate(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("account/usage/aggregate", addr, None).await
}

/// 函数 `service_usage_refresh`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_refresh(
    addr: Option<String>,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = account_id.map(|id| serde_json::json!({ "accountId": id }));
    rpc_call_in_background("account/usage/refresh", addr, params).await
}

/// 函数 `service_usage_quota_reset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-07-13
///
/// # 参数
/// - addr: 参数 addr
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_usage_quota_reset(
    addr: Option<String>,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let account_id = account_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "accountId is required".to_string())?;
    rpc_call_in_background(
        "account/usage/resetQuota",
        addr,
        Some(serde_json::json!({ "accountId": account_id })),
    )
    .await
}
