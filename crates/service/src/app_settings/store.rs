use codexmanager_core::storage::now_ts;
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
    if let Err(err) = crate::storage_helpers::initialize_storage() {
        log::error!("event=app_settings_storage_initialize_failed error={err}");
        return None;
    }
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
    let Some(storage) = open_app_settings_storage() else {
        log::warn!("event=app_settings_list_skipped reason=storage_unavailable");
        return HashMap::new();
    };
    match storage.list_app_settings() {
        Ok(settings) => settings.into_iter().collect(),
        Err(err) => {
            log::error!("event=app_settings_list_failed error={err}");
            HashMap::new()
        }
    }
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
    let Some(storage) = open_app_settings_storage() else {
        log::warn!("event=app_setting_get_skipped key={key} reason=storage_unavailable");
        return None;
    };
    match storage.get_app_setting(key) {
        Ok(value) => value.and_then(|value| normalize_optional_text(Some(&value))),
        Err(err) => {
            log::error!("event=app_setting_get_failed key={key} error={err}");
            None
        }
    }
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
    let Some(storage) = open_app_settings_storage() else {
        log::error!("event=app_setting_save_failed key={key} reason=storage_unavailable");
        return Err("storage unavailable".to_string());
    };
    let text = normalize_optional_text(value).unwrap_or_default();
    if let Err(err) = storage.set_app_setting(key, &text, now_ts()) {
        log::error!("event=app_setting_save_failed key={key} error={err}");
        return Err(format!("save {key} failed: {err}"));
    }
    Ok(())
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
