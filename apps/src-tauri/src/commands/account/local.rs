use codexmanager_core::storage::Storage;

use crate::app_storage::resolve_db_path_with_legacy_migration;

/// 函数 `local_account_delete`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn local_account_delete(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<serde_json::Value, String> {
    let db_path = resolve_db_path_with_legacy_migration(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let storage = Storage::open(db_path).map_err(|e| e.to_string())?;
        storage
            .delete_account(&account_id)
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({ "ok": true }))
    })
    .await
    .map_err(|err| format!("local_account_delete task failed: {err}"))?
}
