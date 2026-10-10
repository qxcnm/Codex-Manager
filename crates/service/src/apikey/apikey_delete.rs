use crate::storage_helpers::open_storage;

/// 函数 `delete_api_key`
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
pub(crate) fn delete_api_key(key_id: &str) -> Result<(), String> {
    // 删除平台 Key
    if key_id.is_empty() {
        return Err("missing id".to_string());
    }
    if crate::storage_helpers::seaorm_enabled() {
        return super::remote::delete(key_id);
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage.delete_api_key(key_id).map_err(|e| e.to_string())?;
    Ok(())
}
