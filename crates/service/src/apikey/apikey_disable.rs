use crate::storage_helpers::open_storage;

/// 函数 `disable_api_key`
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
pub(crate) fn disable_api_key(key_id: &str) -> Result<(), String> {
    // 禁用平台 Key
    if key_id.is_empty() {
        return Err("missing id".to_string());
    }
    if crate::storage_helpers::seaorm_enabled() {
        return super::remote::set_status(key_id, "disabled");
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .update_api_key_status(key_id, "disabled")
        .map_err(|e| e.to_string())?;
    Ok(())
}
