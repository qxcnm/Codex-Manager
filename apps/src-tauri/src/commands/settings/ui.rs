use crate::app_storage::apply_runtime_storage_env;
use tauri_plugin_autostart::ManagerExt;

use super::tray_state::{
    effective_close_to_tray_requested, sync_window_runtime_state_from_settings, tray_available,
};

fn sync_auto_start_runtime_state_from_settings(
    app: &tauri::AppHandle,
    settings: &mut serde_json::Value,
) {
    let result = app.autolaunch().is_enabled();
    if let Err(err) = &result {
        log::warn!("read autostart state failed: {}", err);
    }
    let Some(object) = settings.as_object_mut() else {
        return;
    };
    object.insert("autoStartSupported".to_string(), result.is_ok().into());
    object.insert(
        "autoStartEnabled".to_string(),
        result
            .unwrap_or_else(|_| codexmanager_service::current_auto_start_enabled_setting())
            .into(),
    );
}

fn set_auto_start_enabled(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|err| format!("enable autostart failed: {err}"))?;
    } else {
        manager
            .disable()
            .map_err(|err| format!("disable autostart failed: {err}"))?;
    }
    Ok(())
}

/// 函数 `app_close_to_tray_on_close_get`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_close_to_tray_on_close_get(app: tauri::AppHandle) -> bool {
    apply_runtime_storage_env(&app);
    if let Ok(mut settings) = codexmanager_service::app_settings_get() {
        sync_window_runtime_state_from_settings(&mut settings);
    }
    codexmanager_service::current_close_to_tray_on_close_setting() && tray_available()
}

/// 函数 `app_close_to_tray_on_close_set`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - enabled: 参数 enabled
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub fn app_close_to_tray_on_close_set(app: tauri::AppHandle, enabled: bool) -> bool {
    apply_runtime_storage_env(&app);
    let payload = serde_json::json!({
        "closeToTrayOnClose": enabled
    });
    if let Ok(mut settings) = codexmanager_service::app_settings_set(Some(&payload)) {
        sync_window_runtime_state_from_settings(&mut settings);
    }
    codexmanager_service::current_close_to_tray_on_close_setting() && tray_available()
}

/// 函数 `app_settings_get`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn app_settings_get(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    let mut settings = tauri::async_runtime::spawn_blocking(move || {
        codexmanager_service::app_settings_get_with_overrides(
            Some(effective_close_to_tray_requested()),
            Some(tray_available()),
        )
    })
    .await
    .map_err(|err| format!("app_settings_get task failed: {err}"))??;
    sync_window_runtime_state_from_settings(&mut settings);
    sync_auto_start_runtime_state_from_settings(&app, &mut settings);
    Ok(settings)
}

/// 函数 `app_settings_set`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
/// - patch: 参数 patch
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn app_settings_set(
    app: tauri::AppHandle,
    patch: serde_json::Value,
) -> Result<serde_json::Value, String> {
    apply_runtime_storage_env(&app);
    if let Some(enabled) = patch
        .get("autoStartEnabled")
        .and_then(serde_json::Value::as_bool)
    {
        set_auto_start_enabled(&app, enabled)?;
    }
    let mut settings = tauri::async_runtime::spawn_blocking(move || {
        codexmanager_service::app_settings_set(Some(&patch))
    })
    .await
    .map_err(|err| format!("app_settings_set task failed: {err}"))??;
    sync_window_runtime_state_from_settings(&mut settings);
    sync_auto_start_runtime_state_from_settings(&app, &mut settings);
    Ok(settings)
}
