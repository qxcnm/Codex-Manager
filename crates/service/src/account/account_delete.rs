use codexmanager_core::storage::{now_ts, Event};

use crate::storage_helpers::open_storage;

/// 函数 `delete_account`
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
pub(crate) fn delete_account(account_id: &str) -> Result<(), String> {
    // 删除账号并记录事件
    if account_id.is_empty() {
        return Err("missing accountId".to_string());
    }
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    storage
        .delete_account(account_id)
        .map_err(|e| e.to_string())?;
    crate::gateway::invalidate_account_proxy_cache(account_id);
    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "account_delete".to_string(),
        message: "account deleted".to_string(),
        created_at: now_ts(),
    });
    Ok(())
}
