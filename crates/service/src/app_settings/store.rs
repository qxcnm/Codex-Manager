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
    let storage = open_app_settings_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let text = normalize_optional_text(value).unwrap_or_default();
    storage
        .set_app_setting(key, &text, now_ts())
        .map_err(|err| format!("save {key} failed: {err}"))?;
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
