use crate::commands::shared::rpc_call_in_background;

/// 函数 `service_login_start`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - login_type: 参数 login_type
/// - open_browser: 参数 open_browser
/// - note: 参数 note
/// - tags: 参数 tags
/// - group_name: 参数 group_name
/// - workspace_id: 参数 workspace_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_login_start(
    addr: Option<String>,
    login_type: String,
    open_browser: Option<bool>,
    note: Option<String>,
    tags: Option<String>,
    group_name: Option<String>,
    workspace_id: Option<String>,
    proxy_profile_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "type": login_type,
      "openBrowser": open_browser.unwrap_or(true),
      "note": note,
      "tags": tags,
      "groupName": group_name,
      "workspaceId": workspace_id,
      "proxyProfileId": proxy_profile_id
    });
    rpc_call_in_background("account/login/start", addr, Some(params)).await
}

/// 函数 `service_login_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - login_id: 参数 login_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_login_status(
    addr: Option<String>,
    login_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "loginId": login_id
    });
    rpc_call_in_background("account/login/status", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_login_cancel(
    addr: Option<String>,
    login_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "loginId": login_id
    });
    rpc_call_in_background("account/login/cancel", addr, Some(params)).await
}

/// 函数 `service_login_complete`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - state: 参数 state
/// - code: 参数 code
/// - redirect_uri: 参数 redirect_uri
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_login_complete(
    addr: Option<String>,
    state: String,
    code: String,
    redirect_uri: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "state": state,
      "code": code,
      "redirectUri": redirect_uri
    });
    rpc_call_in_background("account/login/complete", addr, Some(params)).await
}

/// 函数 `service_login_chatgpt_auth_tokens`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - access_token: 参数 access_token
/// - refresh_token: 参数 refresh_token
/// - id_token: 参数 id_token
/// - chatgpt_account_id: 参数 chatgpt_account_id
/// - workspace_id: 参数 workspace_id
/// - chatgpt_plan_type: 参数 chatgpt_plan_type
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_login_chatgpt_auth_tokens(
    addr: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    chatgpt_account_id: Option<String>,
    workspace_id: Option<String>,
    chatgpt_plan_type: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "type": "chatgptAuthTokens",
      "accessToken": access_token,
      "refreshToken": refresh_token,
      "idToken": id_token,
      "chatgptAccountId": chatgpt_account_id,
      "workspaceId": workspace_id,
      "chatgptPlanType": chatgpt_plan_type
    });
    rpc_call_in_background("account/login/start", addr, Some(params)).await
}

/// 函数 `service_account_read`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - refresh_token: 参数 refresh_token
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_read(
    addr: Option<String>,
    refresh_token: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "refreshToken": refresh_token.unwrap_or(false)
    });
    rpc_call_in_background("account/read", addr, Some(params)).await
}

/// 函数 `service_account_logout`
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
pub async fn service_account_logout(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("account/logout", addr, None).await
}

/// 函数 `service_chatgpt_auth_tokens_refresh`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - reason: 参数 reason
/// - account_id: 参数 account_id
/// - previous_account_id: 参数 previous_account_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_chatgpt_auth_tokens_refresh(
    addr: Option<String>,
    reason: Option<String>,
    account_id: Option<String>,
    previous_account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "reason": reason.unwrap_or_else(|| "unauthorized".to_string()),
      "accountId": account_id,
      "previousAccountId": previous_account_id
    });
    rpc_call_in_background("account/chatgptAuthTokens/refresh", addr, Some(params)).await
}

/// 函数 `service_chatgpt_auth_tokens_refresh_all`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-03
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_chatgpt_auth_tokens_refresh_all(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("account/chatgptAuthTokens/refreshAll", addr, None).await
}
