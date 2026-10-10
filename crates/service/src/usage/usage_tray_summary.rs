use codexmanager_core::storage::now_ts;

use crate::storage_helpers::open_storage;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayUsageResetSummary {
    pub primary_resets_at: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub primary_known_count: usize,
    pub secondary_known_count: usize,
}

pub fn read_tray_usage_reset_summary() -> TrayUsageResetSummary {
    let Some(storage) = open_storage() else {
        return TrayUsageResetSummary::default();
    };
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let Ok(items) = storage.latest_usage_snapshots_by_account() else {
        return TrayUsageResetSummary::default();
    };
    let now = now_ts();
    let mut summary = TrayUsageResetSummary::default();
    for item in items {
        if let Some(resets_at) = future_ts(item.resets_at, now) {
            summary.primary_known_count += 1;
            summary.primary_resets_at = min_ts(summary.primary_resets_at, resets_at);
        }
        if let Some(resets_at) = future_ts(item.secondary_resets_at, now) {
            summary.secondary_known_count += 1;
            summary.secondary_resets_at = min_ts(summary.secondary_resets_at, resets_at);
        }
    }
    summary
}

fn future_ts(value: Option<i64>, now: i64) -> Option<i64> {
    value.filter(|ts| *ts > now)
}

fn min_ts(current: Option<i64>, candidate: i64) -> Option<i64> {
    Some(
        current
            .map(|value| value.min(candidate))
            .unwrap_or(candidate),
    )
}

#[cfg(test)]
mod tests {
    use super::{future_ts, min_ts};

    #[test]
    fn future_ts_ignores_missing_or_elapsed_values() {
        assert_eq!(future_ts(None, 100), None);
        assert_eq!(future_ts(Some(99), 100), None);
        assert_eq!(future_ts(Some(100), 100), None);
        assert_eq!(future_ts(Some(101), 100), Some(101));
    }

    #[test]
    fn min_ts_keeps_earliest_value() {
        assert_eq!(min_ts(None, 120), Some(120));
        assert_eq!(min_ts(Some(180), 120), Some(120));
        assert_eq!(min_ts(Some(90), 120), Some(90));
    }
}
