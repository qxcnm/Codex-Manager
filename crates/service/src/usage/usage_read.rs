use codexmanager_core::rpc::types::UsageSnapshotResult;
use codexmanager_core::storage::UsageSnapshotRecord;
use codexmanager_core::usage::has_usable_luna_reserve;

use crate::storage_helpers::open_storage;

/// 函数 `usage_snapshot_result_from_record`
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
pub(crate) fn usage_snapshot_result_from_record(snap: UsageSnapshotRecord) -> UsageSnapshotResult {
    let availability_status = classify_availability_status(&snap).to_string();
    // 将存储记录转换为 API 返回结构
    UsageSnapshotResult {
        account_id: Some(snap.account_id),
        availability_status: Some(availability_status),
        used_percent: snap.used_percent,
        window_minutes: snap.window_minutes,
        resets_at: snap.resets_at,
        secondary_used_percent: snap.secondary_used_percent,
        secondary_window_minutes: snap.secondary_window_minutes,
        secondary_resets_at: snap.secondary_resets_at,
        credits_json: snap.credits_json,
        captured_at: Some(snap.captured_at),
    }
}

/// 函数 `classify_availability_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - snap: 参数 snap
///
/// # 返回
/// 返回函数执行结果
fn classify_availability_status(snap: &UsageSnapshotRecord) -> &'static str {
    let primary_missing = snap.used_percent.is_none() || snap.window_minutes.is_none();
    if primary_missing {
        return "unknown";
    }
    if snap
        .used_percent
        .map(|value| value >= 100.0)
        .unwrap_or(false)
    {
        return if has_usable_luna_reserve(snap.credits_json.as_deref()) {
            "available_luna_reserve"
        } else {
            "unavailable"
        };
    }

    let secondary_present =
        snap.secondary_used_percent.is_some() || snap.secondary_window_minutes.is_some();
    let secondary_complete =
        snap.secondary_used_percent.is_some() && snap.secondary_window_minutes.is_some();

    if !secondary_present {
        return "primary_window_available_only";
    }
    if !secondary_complete {
        // 中文注释：secondary 只要不是完整可用数据，就按主窗口可用处理，
        // 避免半截快照把还有额度的账号展示成未知状态。
        return "primary_window_available_only";
    }
    if snap
        .secondary_used_percent
        .map(|value| value >= 100.0)
        .unwrap_or(false)
    {
        return if has_usable_luna_reserve(snap.credits_json.as_deref()) {
            "available_luna_reserve"
        } else {
            "unavailable"
        };
    }
    "available"
}

/// 函数 `read_usage_snapshot`
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
pub(crate) fn read_usage_snapshot(account_id: Option<&str>) -> Option<UsageSnapshotResult> {
    // 读取最新用量快照
    let storage = open_storage()?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let snap = match account_id {
        Some(account_id) => storage
            .latest_usage_snapshot_for_account(account_id)
            .ok()
            .flatten(),
        None => storage.latest_usage_snapshot().ok().flatten(),
    }?;
    Some(usage_snapshot_result_from_record(snap))
}

#[cfg(test)]
mod tests {
    use super::usage_snapshot_result_from_record;
    use codexmanager_core::storage::UsageSnapshotRecord;

    #[test]
    fn usage_read_exposes_luna_reserve_availability() {
        let result = usage_snapshot_result_from_record(UsageSnapshotRecord {
            account_id: "acc-luna-reserve".to_string(),
            used_percent: Some(100.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(100.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: Some(
                r#"{"_codexmanager_extra_rate_limits":[{"limit_name":"gpt-reserve","metered_feature":"base_model_inference","primary_window":{"used_percent":10.0}}]}"#.to_string(),
            ),
            captured_at: 1,
        });

        assert_eq!(
            result.availability_status.as_deref(),
            Some("available_luna_reserve")
        );
    }
}
