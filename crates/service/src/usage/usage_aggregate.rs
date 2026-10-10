use std::collections::HashMap;

use codexmanager_core::rpc::types::UsageAggregateSummaryResult;
use codexmanager_core::storage::{UsageSnapshotRecord, UsageSnapshotSummaryRow};
use serde_json::Value;

use crate::storage_helpers::open_storage;

const MINUTES_PER_HOUR: i64 = 60;
const MINUTES_PER_DAY: i64 = 24 * MINUTES_PER_HOUR;
const ROUNDING_BIAS: i64 = 3;

/// 函数 `read_usage_aggregate_summary`
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
pub(crate) fn read_usage_aggregate_summary() -> Result<UsageAggregateSummaryResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    read_usage_aggregate_summary_with_storage(&storage)
}

pub(crate) fn read_usage_aggregate_summary_with_storage(
    storage: &codexmanager_core::storage::Storage,
) -> Result<UsageAggregateSummaryResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let usage_items = storage
        .latest_usage_snapshot_summary_rows()
        .map_err(|err| format!("list usage snapshot summary rows failed: {err}"))?;
    Ok(compute_usage_aggregate_summary_for_known_items(
        &usage_items,
    ))
}

trait UsageAggregateItem {
    fn account_id(&self) -> &str;
    fn used_percent(&self) -> Option<f64>;
    fn window_minutes(&self) -> Option<i64>;
    fn secondary_used_percent(&self) -> Option<f64>;
    fn secondary_window_minutes(&self) -> Option<i64>;
    fn credits_json(&self) -> Option<&str>;
}

impl UsageAggregateItem for UsageSnapshotRecord {
    fn account_id(&self) -> &str {
        self.account_id.as_str()
    }

    fn used_percent(&self) -> Option<f64> {
        self.used_percent
    }

    fn window_minutes(&self) -> Option<i64> {
        self.window_minutes
    }

    fn secondary_used_percent(&self) -> Option<f64> {
        self.secondary_used_percent
    }

    fn secondary_window_minutes(&self) -> Option<i64> {
        self.secondary_window_minutes
    }

    fn credits_json(&self) -> Option<&str> {
        self.credits_json.as_deref()
    }
}

impl UsageAggregateItem for UsageSnapshotSummaryRow {
    fn account_id(&self) -> &str {
        self.account_id.as_str()
    }

    fn used_percent(&self) -> Option<f64> {
        self.used_percent
    }

    fn window_minutes(&self) -> Option<i64> {
        self.window_minutes
    }

    fn secondary_used_percent(&self) -> Option<f64> {
        self.secondary_used_percent
    }

    fn secondary_window_minutes(&self) -> Option<i64> {
        self.secondary_window_minutes
    }

    fn credits_json(&self) -> Option<&str> {
        self.credits_json.as_deref()
    }
}

/// 函数 `compute_usage_aggregate_summary`
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
#[cfg(test)]
pub(crate) fn compute_usage_aggregate_summary(
    accounts: &[codexmanager_core::storage::Account],
    usage_items: &[UsageSnapshotRecord],
) -> UsageAggregateSummaryResult {
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    compute_usage_aggregate_summary_for_account_ids(&account_ids, usage_items)
}

pub(crate) fn compute_usage_aggregate_summary_for_account_ids_list(
    account_ids: &[String],
    usage_items: &[UsageSnapshotRecord],
) -> UsageAggregateSummaryResult {
    compute_usage_aggregate_summary_for_account_ids(account_ids, usage_items)
}

fn compute_usage_aggregate_summary_for_known_items<T: UsageAggregateItem>(
    usage_items: &[T],
) -> UsageAggregateSummaryResult {
    let mut accumulator = UsageAggregateAccumulator::default();
    for usage in usage_items {
        if usage.account_id().trim().is_empty() {
            continue;
        }
        accumulator.add_usage(Some(usage));
    }
    accumulator.finish()
}

fn compute_usage_aggregate_summary_for_account_ids<T: UsageAggregateItem>(
    account_ids: &[String],
    usage_items: &[T],
) -> UsageAggregateSummaryResult {
    let usage_map = usage_items
        .iter()
        .map(|item| (item.account_id(), item))
        .collect::<HashMap<_, _>>();

    let mut accumulator = UsageAggregateAccumulator::default();

    for account_id in account_ids {
        let usage = usage_map.get(account_id.as_str()).copied();
        accumulator.add_usage(usage);
    }

    accumulator.finish()
}

#[derive(Default)]
struct UsageAggregateAccumulator {
    primary_bucket_count: i64,
    primary_known_count: i64,
    primary_remaining_total: f64,
    secondary_bucket_count: i64,
    secondary_known_count: i64,
    secondary_remaining_total: f64,
}

impl UsageAggregateAccumulator {
    fn add_usage<T: UsageAggregateItem>(&mut self, usage: Option<&T>) {
        let has_primary_signal = usage
            .map(|value| value.used_percent().is_some() || value.window_minutes().is_some())
            .unwrap_or(false);
        let has_secondary_signal = usage
            .map(|value| {
                value.secondary_used_percent().is_some()
                    || value.secondary_window_minutes().is_some()
            })
            .unwrap_or(false);
        let primary_belongs_to_secondary = usage
            .map(|value| {
                !has_secondary_signal
                    && (is_long_window(value.window_minutes())
                        || is_free_plan_usage(value.credits_json()))
            })
            .unwrap_or(false);

        if has_primary_signal {
            if primary_belongs_to_secondary {
                self.secondary_bucket_count += 1;
            } else {
                self.primary_bucket_count += 1;
            }
        }

        if let Some(primary_remain) =
            usage.and_then(|value| remaining_percent(value.used_percent()))
        {
            if primary_belongs_to_secondary {
                self.secondary_known_count += 1;
                self.secondary_remaining_total += primary_remain;
            } else {
                self.primary_known_count += 1;
                self.primary_remaining_total += primary_remain;
            }
        }

        if has_secondary_signal {
            self.secondary_bucket_count += 1;
        }
        if let Some(secondary_remain) =
            usage.and_then(|value| remaining_percent(value.secondary_used_percent()))
        {
            self.secondary_known_count += 1;
            self.secondary_remaining_total += secondary_remain;
        }
    }

    fn finish(self) -> UsageAggregateSummaryResult {
        UsageAggregateSummaryResult {
            primary_bucket_count: self.primary_bucket_count,
            primary_known_count: self.primary_known_count,
            primary_unknown_count: (self.primary_bucket_count - self.primary_known_count).max(0),
            primary_remain_percent: average_percent(
                self.primary_remaining_total,
                self.primary_known_count,
            ),
            secondary_bucket_count: self.secondary_bucket_count,
            secondary_known_count: self.secondary_known_count,
            secondary_unknown_count: (self.secondary_bucket_count - self.secondary_known_count)
                .max(0),
            secondary_remain_percent: average_percent(
                self.secondary_remaining_total,
                self.secondary_known_count,
            ),
        }
    }
}

/// 函数 `normalize_percent`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_percent(value: Option<f64>) -> Option<f64> {
    value.map(|parsed| parsed.clamp(0.0, 100.0))
}

/// 函数 `remaining_percent`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn remaining_percent(value: Option<f64>) -> Option<f64> {
    normalize_percent(value).map(|used| (100.0 - used).max(0.0))
}

/// 函数 `average_percent`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - total: 参数 total
/// - count: 参数 count
///
/// # 返回
/// 返回函数执行结果
fn average_percent(total: f64, count: i64) -> Option<i64> {
    if count <= 0 {
        return None;
    }
    Some((total / count as f64).round() as i64)
}

/// 函数 `is_long_window`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - window_minutes: 参数 window_minutes
///
/// # 返回
/// 返回函数执行结果
fn is_long_window(window_minutes: Option<i64>) -> bool {
    window_minutes.is_some_and(|value| value > MINUTES_PER_DAY + ROUNDING_BIAS)
}

/// 函数 `is_free_plan_usage`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn is_free_plan_usage(raw: Option<&str>) -> bool {
    let Some(value) = parse_credits(raw) else {
        return false;
    };
    extract_plan_type_recursive(&value)
        .map(|value| value.contains("free"))
        .unwrap_or(false)
}

/// 函数 `parse_credits`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn parse_credits(raw: Option<&str>) -> Option<Value> {
    let text = raw?.trim();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// 函数 `extract_plan_type_recursive`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn extract_plan_type_recursive(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => items.iter().find_map(extract_plan_type_recursive),
        Value::Object(map) => {
            for key in [
                "plan_type",
                "planType",
                "subscription_tier",
                "subscriptionTier",
                "tier",
                "account_type",
                "accountType",
                "type",
            ] {
                if let Some(text) = map.get(key).and_then(Value::as_str) {
                    let normalized = text.trim().to_ascii_lowercase();
                    if !normalized.is_empty() {
                        return Some(normalized);
                    }
                }
            }
            map.values().find_map(extract_plan_type_recursive)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "usage_aggregate_tests.rs"]
mod tests;
