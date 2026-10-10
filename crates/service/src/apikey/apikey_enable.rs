use crate::storage_helpers::open_storage;

/// 函数 `enable_api_key`
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
pub(crate) fn enable_api_key(key_id: &str) -> Result<(), String> {
    // 启用平台 Key，恢复网关鉴权可用。
    if key_id.is_empty() {
        return Err("missing id".to_string());
    }
    if crate::storage_helpers::seaorm_enabled() {
        return super::remote::set_status(key_id, "active");
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .update_api_key_status(key_id, "active")
        .map_err(|e| e.to_string())?;
    Ok(())
}
