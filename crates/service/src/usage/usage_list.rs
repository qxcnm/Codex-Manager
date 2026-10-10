use codexmanager_core::rpc::types::UsageSnapshotResult;

use crate::storage_helpers::open_storage;
use crate::usage_read::usage_snapshot_result_from_record;

pub(crate) fn read_usage_snapshots_limited(
    limit: Option<usize>,
) -> Result<Vec<UsageSnapshotResult>, String> {
    if limit == Some(0) {
        return Ok(Vec::new());
    }
    // 读取所有账号最新用量
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let items = storage
        .latest_usage_snapshots_by_account_limited(limit)
        .map_err(|err| format!("list usage snapshots failed: {err}"))?;
    Ok(items
        .into_iter()
        .map(usage_snapshot_result_from_record)
        .collect())
}
