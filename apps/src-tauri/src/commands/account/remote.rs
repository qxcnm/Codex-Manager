use crate::app_storage::apply_runtime_storage_env;
use crate::commands::shared::rpc_call_in_background;

/// 函数 `account_update_payload`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
/// - sort: 参数 sort
/// - status: 参数 status
/// - label: 参数 label
/// - note: 参数 note
/// - tags: 参数 tags
///
/// # 返回
/// 返回函数执行结果
fn account_update_payload(
    account_id: String,
    sort: Option<i64>,
    preferred: Option<bool>,
    status: Option<String>,
    label: Option<String>,
    note: Option<String>,
    tags: Option<String>,
    model_slugs: Option<Vec<String>>,
    quota_capacity_primary_window_tokens: Option<i64>,
    quota_capacity_secondary_window_tokens: Option<i64>,
) -> Option<serde_json::Value> {
    let mut params = serde_json::Map::new();
    params.insert("accountId".to_string(), serde_json::json!(account_id));
    if let Some(value) = sort {
        params.insert("sort".to_string(), serde_json::json!(value));
    }
    if let Some(value) = preferred {
        params.insert("preferred".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            params.insert("status".to_string(), serde_json::json!(trimmed));
        }
    }
    if let Some(value) = label {
        params.insert("label".to_string(), serde_json::json!(value));
    }
    if let Some(value) = note {
        params.insert("note".to_string(), serde_json::json!(value));
    }
    if let Some(value) = tags {
        params.insert("tags".to_string(), serde_json::json!(value));
    }
    if let Some(value) = model_slugs {
        params.insert("modelSlugs".to_string(), serde_json::json!(value));
    }
    if let Some(value) = quota_capacity_primary_window_tokens {
        params.insert(
            "quotaCapacityPrimaryWindowTokens".to_string(),
            serde_json::json!(value),
        );
    }
    if let Some(value) = quota_capacity_secondary_window_tokens {
        params.insert(
            "quotaCapacitySecondaryWindowTokens".to_string(),
            serde_json::json!(value),
        );
    }
    if params.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(params))
    }
}

/// 函数 `service_account_list`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_list(
    app: tauri::AppHandle,
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    rpc_call_in_background("account/list", addr, None).await
}

/// 函数 `service_account_delete`
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
pub async fn service_account_delete(
    addr: Option<String>,
    account_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "accountId": account_id });
    rpc_call_in_background("account/delete", addr, Some(params)).await
}

/// 函数 `service_account_delete_many`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - account_ids: 参数 account_ids
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_delete_many(
    addr: Option<String>,
    account_ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "accountIds": account_ids });
    rpc_call_in_background("account/deleteMany", addr, Some(params)).await
}

/// 函数 `service_account_delete_unavailable_free`
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
pub async fn service_account_delete_unavailable_free(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("account/deleteUnavailableFree", addr, None).await
}

/// 函数 `service_account_delete_by_statuses`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-04
///
/// # 参数
/// - addr: 参数 addr
/// - statuses: 参数 statuses
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_delete_by_statuses(
    addr: Option<String>,
    statuses: Vec<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "statuses": statuses });
    rpc_call_in_background("account/deleteByStatuses", addr, Some(params)).await
}

/// 函数 `service_account_update`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - account_id: 参数 account_id
/// - sort: 参数 sort
/// - status: 参数 status
/// - label: 参数 label
/// - note: 参数 note
/// - tags: 参数 tags
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_update(
    addr: Option<String>,
    account_id: String,
    sort: Option<i64>,
    preferred: Option<bool>,
    status: Option<String>,
    label: Option<String>,
    note: Option<String>,
    tags: Option<String>,
    model_slugs: Option<Vec<String>>,
    quota_capacity_primary_window_tokens: Option<i64>,
    quota_capacity_secondary_window_tokens: Option<i64>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "account/update",
        addr,
        account_update_payload(
            account_id,
            sort,
            preferred,
            status,
            label,
            note,
            tags,
            model_slugs,
            quota_capacity_primary_window_tokens,
            quota_capacity_secondary_window_tokens,
        ),
    )
    .await
}

/// 函数 `service_account_warmup`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-14
///
/// # 参数
/// - addr: 参数 addr
/// - account_ids: 参数 account_ids
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_account_warmup(
    addr: Option<String>,
    account_ids: Vec<String>,
    message: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "accountIds": account_ids,
        "message": message.unwrap_or_default(),
    });
    rpc_call_in_background("account/warmup", addr, Some(params)).await
}

#[cfg(test)]
mod tests {
    use super::account_update_payload;

    /// 函数 `account_update_payload_supports_status_only_updates`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn account_update_payload_supports_status_only_updates() {
        let actual = account_update_payload(
            "acc-1".to_string(),
            None,
            None,
            Some("active".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("payload");
        let expected = serde_json::json!({
            "accountId": "acc-1",
            "status": "active"
        });
        assert_eq!(actual, expected);
    }

    /// 函数 `account_update_payload_supports_sort_only_updates`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn account_update_payload_supports_sort_only_updates() {
        let actual = account_update_payload(
            "acc-1".to_string(),
            Some(5),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("payload");
        let expected = serde_json::json!({
            "accountId": "acc-1",
            "sort": 5
        });
        assert_eq!(actual, expected);
    }

    /// 函数 `account_update_payload_omits_blank_status`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 无
    #[test]
    fn account_update_payload_omits_blank_status() {
        let actual = account_update_payload(
            "acc-1".to_string(),
            None,
            None,
            Some("   ".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("payload");
        let expected = serde_json::json!({
            "accountId": "acc-1"
        });
        assert_eq!(actual, expected);
    }

    #[test]
    fn account_update_payload_supports_preferred_updates() {
        let actual = account_update_payload(
            "acc-1".to_string(),
            None,
            Some(true),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("payload");
        let expected = serde_json::json!({
            "accountId": "acc-1",
            "preferred": true
        });
        assert_eq!(actual, expected);
    }
}
