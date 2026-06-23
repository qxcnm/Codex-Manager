use codexmanager_core::storage::{now_ts, Event};
use serde::Serialize;
use std::collections::HashSet;

use crate::storage_helpers::open_storage;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteManyError {
    account_id: String,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteManyResult {
    requested: usize,
    deleted: usize,
    failed: usize,
    deleted_account_ids: Vec<String>,
    errors: Vec<DeleteManyError>,
}

/// 函数 `delete_accounts`
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
pub(crate) fn delete_accounts(account_ids: Vec<String>) -> Result<DeleteManyResult, String> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for account_id in account_ids {
        let normalized = account_id.trim();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.to_string()) {
            unique.push(normalized.to_string());
        }
    }

    if unique.is_empty() {
        return Err("missing accountIds".to_string());
    }

    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut result = DeleteManyResult {
        requested: unique.len(),
        deleted: 0,
        failed: 0,
        deleted_account_ids: Vec::new(),
        errors: Vec::new(),
    };

    let existing_account_ids = storage
        .list_account_ids_for_ids(&unique)
        .map_err(|err| err.to_string())?
        .into_iter()
        .collect::<HashSet<_>>();
    let existing_unique = unique
        .iter()
        .filter(|account_id| existing_account_ids.contains(account_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    for account_id in unique {
        if !existing_account_ids.contains(&account_id) {
            result.failed += 1;
            result.errors.push(DeleteManyError {
                account_id,
                message: "account not found".to_string(),
            });
        }
    }

    if !existing_unique.is_empty() {
        storage
            .delete_accounts(&existing_unique)
            .map_err(|err| err.to_string())?;
        result.deleted = existing_unique.len();
        for account_id in existing_unique {
            crate::gateway::invalidate_account_proxy_cache(account_id.as_str());
            let _ = storage.insert_event(&Event {
                account_id: Some(account_id.clone()),
                event_type: "account_delete_many".to_string(),
                message: "account deleted via bulk action".to_string(),
                created_at: now_ts(),
            });
            result.deleted_account_ids.push(account_id);
        }
    }

    Ok(result)
}

#[cfg(test)]
#[path = "account_delete_many_tests.rs"]
mod tests;
