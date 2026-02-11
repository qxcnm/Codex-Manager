use gpttools_core::storage::{now_ts, Event, Storage};

use crate::account_status::set_account_status;

pub(super) fn record_usage_refresh_failure(storage: &Storage, account_id: &str, message: &str) {
    let _ = storage.insert_event(&Event {
        account_id: Some(account_id.to_string()),
        event_type: "usage_refresh_failed".to_string(),
        message: message.to_string(),
        created_at: now_ts(),
    });
}

pub(super) fn mark_usage_unreachable_if_needed(storage: &Storage, account_id: &str, err: &str) {
    // 中文注释：仅当上游明确返回 usage endpoint 状态错误才降级账号，
    // 否则网络抖动等瞬态错误也会误标 inactive，导致可用账号被过早摘除。
    if err.starts_with("usage endpoint status") {
        set_account_status(storage, account_id, "inactive", "usage_unreachable");
    }
}

pub(super) fn should_retry_with_refresh(err: &str) -> bool {
    err.contains("401") || err.contains("403")
}
