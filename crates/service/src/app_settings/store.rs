use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::now_ts;
use codexmanager_storage_seaorm::{AppSetting, SettingsRepository};
use std::collections::HashMap;

use super::normalize_optional_text;

/// 函数 `open_app_settings_storage`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn open_app_settings_storage() -> Option<crate::storage_helpers::StorageHandle> {
    crate::process_env::ensure_default_db_path();
    let _ = crate::storage_helpers::initialize_storage();
    crate::storage_helpers::open_storage()
}

/// 函数 `list_app_settings_map`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn list_app_settings_map() -> HashMap<String, String> {
    if seaorm_enabled() {
        return remote_settings().unwrap_or_else(|err| {
            log::error!("{err}");
            REMOTE_SETTINGS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        });
    }
    open_app_settings_storage()
        .and_then(|storage| storage.list_app_settings().ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// 函数 `get_persisted_app_setting`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn get_persisted_app_setting(key: &str) -> Option<String> {
    if seaorm_enabled() {
        return list_app_settings_map()
            .remove(key)
            .and_then(|value| normalize_optional_text(Some(&value)));
    }
    open_app_settings_storage()
        .and_then(|storage| storage.get_app_setting(key).ok().flatten())
        .and_then(|value| normalize_optional_text(Some(&value)))
}

/// 函数 `save_persisted_app_setting`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn save_persisted_app_setting(key: &str, value: Option<&str>) -> Result<(), String> {
    if seaorm_enabled() {
        let key = key.to_owned();
        let text = normalize_optional_text(value).unwrap_or_default();
        let record = AppSetting {
            key: key.clone(),
            value: text.clone(),
            updated_at: now_ts(),
        };
        seaorm_block_on(move |storage| async move {
            SettingsRepository::set(storage.connection(), record)
                .await
                .map_err(|_| "save remote setting failed".to_owned())
        })?;
        REMOTE_SETTINGS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, text);
        return Ok(());
    }
    let storage = open_app_settings_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let text = normalize_optional_text(value).unwrap_or_default();
    storage
        .set_app_setting(key, &text, now_ts())
        .map_err(|err| format!("save {key} failed: {err}"))?;
    Ok(())
}

// Preserve the last successfully loaded security settings during a transient
// DB outage. Startup eagerly loads them and fails before binding on an error.
static REMOTE_SETTINGS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

pub(crate) fn remote_settings() -> Result<HashMap<String, String>, String> {
    let settings = seaorm_block_on(|storage| async move {
        SettingsRepository::list(storage.connection())
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|r| (r.key, r.value))
                    .collect::<HashMap<_, _>>()
            })
            .map_err(|_| "load remote application settings failed".to_owned())
    })?;
    *REMOTE_SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    Ok(settings)
}

/// 函数 `save_persisted_bool_setting`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn save_persisted_bool_setting(key: &str, value: bool) -> Result<(), String> {
    save_persisted_app_setting(key, Some(if value { "1" } else { "0" }))
}
