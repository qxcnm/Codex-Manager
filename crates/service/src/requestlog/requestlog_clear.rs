use crate::storage_helpers::open_storage;

/// 函数 `clear_request_logs`
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
pub(crate) fn clear_request_logs() -> Result<(), String> {
    if crate::storage_helpers::seaorm_enabled() {
        return crate::storage_helpers::seaorm_block_on(|storage| async move {
            codexmanager_storage_seaorm::RequestLogsRepository::clear(
                storage.connection(),
                codexmanager_core::storage::now_ts(),
            )
            .await
            .map_err(|e| format!("clear SeaORM request logs failed: {e}"))
        });
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    crate::gateway::guard_request_log_payload_clear(|| storage.clear_request_logs())
        .map_err(|e| e.to_string())
}
