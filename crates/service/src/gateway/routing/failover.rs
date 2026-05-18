use codexmanager_core::storage::Storage;

use crate::account_availability::{evaluate_snapshot, Availability};

/// 函数 `should_failover_after_refresh`
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
#[allow(dead_code)]
pub(crate) fn should_failover_after_refresh(
    storage: &Storage,
    account_id: &str,
    refresh_result: Result<(), String>,
) -> bool {
    match refresh_result {
        Ok(_) => should_failover_by_snapshot(storage, account_id, true),
        Err(err) => {
            if err.starts_with("usage endpoint status") {
                true
            } else {
                false
            }
        }
    }
}

/// 函数 `should_failover_from_cached_snapshot`
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
pub(crate) fn should_failover_from_cached_snapshot(storage: &Storage, account_id: &str) -> bool {
    should_failover_by_snapshot(storage, account_id, false)
}

pub(crate) fn should_failover_from_low_quota_snapshot(storage: &Storage, account_id: &str) -> bool {
    storage
        .latest_usage_snapshot_for_account(account_id)
        .ok()
        .flatten()
        .as_ref()
        .is_some_and(super::selection::is_low_quota_snapshot)
}

/// 函数 `should_failover_by_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
/// - fail_on_missing: 参数 fail_on_missing
///
/// # 返回
/// 返回函数执行结果
fn should_failover_by_snapshot(storage: &Storage, account_id: &str, fail_on_missing: bool) -> bool {
    let snap = storage
        .latest_usage_snapshot_for_account(account_id)
        .ok()
        .flatten();
    match snap.as_ref().map(evaluate_snapshot) {
        Some(Availability::Unavailable(_reason)) => true,
        Some(Availability::Available) => false,
        None if fail_on_missing => true,
        None => false,
    }
}
