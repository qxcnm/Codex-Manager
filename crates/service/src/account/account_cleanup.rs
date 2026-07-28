use codexmanager_core::storage::{
    now_ts, AccountTokenPlan, Event, UsageSnapshotCleanupRow, UsageSnapshotRecord,
};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::account_availability::{evaluate_snapshot, Availability};
use crate::account_plan::ResolvedAccountPlan;
use crate::storage_helpers::open_storage;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteUnavailableFreeResult {
    scanned: usize,
    deleted: usize,
    skipped_available: usize,
    skipped_disabled: usize,
    skipped_non_free: usize,
    skipped_missing_usage: usize,
    skipped_missing_token: usize,
    deleted_account_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteBannedResult {
    scanned: usize,
    deleted: usize,
    skipped_disabled: usize,
    skipped_not_banned: usize,
    deleted_account_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeleteAccountsByStatusesResult {
    scanned: usize,
    deleted: usize,
    skipped_status: usize,
    target_statuses: Vec<String>,
    deleted_account_ids: Vec<String>,
}

const CLEANUP_STATUS_ALLOWLIST: &[&str] = &[
    "unavailable",
    "banned",
    "limited",
    "disabled",
    "inactive",
    "unknown",
];

/// 函数 `delete_unavailable_free_accounts`
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
pub(crate) fn delete_unavailable_free_accounts() -> Result<DeleteUnavailableFreeResult, String> {
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let scanned = storage.account_count().map_err(|err| err.to_string())? as usize;
    let accounts = storage
        .list_account_cleanup_candidates_by_statuses(&cleanup_status_allowlist_vec())
        .map_err(|err| err.to_string())?;
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let usage_by_account: HashMap<String, UsageSnapshotCleanupRow> = storage
        .latest_usage_cleanup_rows_for_accounts(&account_ids)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|snapshot| (snapshot.account_id.clone(), snapshot))
        .collect();
    let token_plans_by_account: HashMap<String, AccountTokenPlan> = storage
        .list_account_token_plans_for_accounts(&account_ids)
        .map_err(|err| err.to_string())?
        .into_iter()
        .map(|token| (token.account_id.clone(), token))
        .collect();

    let mut result = DeleteUnavailableFreeResult {
        scanned,
        deleted: 0,
        skipped_available: 0,
        skipped_disabled: 0,
        skipped_non_free: 0,
        skipped_missing_usage: 0,
        skipped_missing_token: 0,
        deleted_account_ids: Vec::new(),
    };

    let mut pending_deletes: Vec<(String, String)> = Vec::new();
    for account in accounts {
        let normalized_status = account.status.trim().to_ascii_lowercase();
        if normalized_status == "disabled" {
            result.skipped_disabled += 1;
            continue;
        }

        let snapshot = usage_by_account.get(&account.id);
        let snapshot_record = snapshot.map(cleanup_snapshot_record);
        if normalized_status != "unavailable" && normalized_status != "banned" {
            let Some(snapshot) = snapshot_record.as_ref() else {
                result.skipped_missing_usage += 1;
                continue;
            };
            if matches!(evaluate_snapshot(snapshot), Availability::Available) {
                result.skipped_available += 1;
                continue;
            }
        }

        let token = token_plans_by_account.get(&account.id);
        let resolved_plan =
            crate::account_plan::resolve_account_plan(token, snapshot_record.as_ref());
        let Some(plan) = resolved_plan.as_ref() else {
            if snapshot.is_none() && token.is_none() {
                result.skipped_missing_usage += 1;
            } else if token.is_none() {
                result.skipped_missing_token += 1;
            } else {
                result.skipped_non_free += 1;
            }
            continue;
        };
        if plan.normalized != "free" {
            result.skipped_non_free += 1;
            continue;
        }
        let Some(_token) = token else {
            result.skipped_missing_token += 1;
            continue;
        };

        let event_message = match plan_label_for_event(resolved_plan.as_ref()) {
            Some(plan) => format!("bulk delete unavailable free account: plan={plan}"),
            None => "bulk delete unavailable free account".to_string(),
        };
        pending_deletes.push((account.id, event_message));
    }

    let pending_ids = pending_deletes
        .iter()
        .map(|(account_id, _)| account_id.clone())
        .collect::<Vec<_>>();
    if !pending_ids.is_empty() {
        storage
            .delete_accounts(&pending_ids)
            .map_err(|err| err.to_string())?;
        for (account_id, event_message) in pending_deletes {
            crate::storage_helpers::insert_event_best_effort(
                &storage,
                &Event {
                    account_id: Some(account_id.clone()),
                    event_type: "account_bulk_delete_unavailable_free".to_string(),
                    message: event_message,
                    created_at: now_ts(),
                },
                "account.cleanup_unavailable_free",
            );

            result.deleted += 1;
            result.deleted_account_ids.push(account_id);
        }
    }

    Ok(result)
}

/// 函数 `delete_banned_accounts`
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
pub(crate) fn delete_banned_accounts() -> Result<DeleteBannedResult, String> {
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let scanned = storage.account_count().map_err(|err| err.to_string())? as usize;
    let accounts = storage
        .list_account_cleanup_candidates_by_statuses(&[
            "banned".to_string(),
            "disabled".to_string(),
        ])
        .map_err(|err| err.to_string())?;

    let mut result = DeleteBannedResult {
        scanned,
        deleted: 0,
        skipped_disabled: 0,
        skipped_not_banned: 0,
        deleted_account_ids: Vec::new(),
    };

    let mut pending_ids = Vec::new();
    for account in accounts {
        let normalized_status = account.status.trim().to_ascii_lowercase();
        if normalized_status == "disabled" {
            result.skipped_disabled += 1;
            continue;
        }
        if normalized_status != "banned" {
            result.skipped_not_banned += 1;
            continue;
        }

        pending_ids.push(account.id);
    }

    if !pending_ids.is_empty() {
        storage
            .delete_accounts(&pending_ids)
            .map_err(|err| err.to_string())?;
        for account_id in pending_ids {
            crate::storage_helpers::insert_event_best_effort(
                &storage,
                &Event {
                    account_id: Some(account_id.clone()),
                    event_type: "account_bulk_delete_banned".to_string(),
                    message: "bulk delete banned account".to_string(),
                    created_at: now_ts(),
                },
                "account.cleanup_banned",
            );

            result.deleted += 1;
            result.deleted_account_ids.push(account_id);
        }
    }
    result.skipped_not_banned = result
        .scanned
        .saturating_sub(result.deleted)
        .saturating_sub(result.skipped_disabled);

    Ok(result)
}

/// 函数 `delete_accounts_by_statuses`
///
/// 作者: gaohongshun
///
/// 时间: 2026-05-04
///
/// # 参数
/// - statuses: 参数 statuses
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn delete_accounts_by_statuses(
    statuses: Vec<String>,
) -> Result<DeleteAccountsByStatusesResult, String> {
    let target_statuses = normalize_cleanup_statuses(statuses)?;
    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let scanned = storage.account_count().map_err(|err| err.to_string())? as usize;
    let accounts = storage
        .list_account_cleanup_candidates_by_statuses(&target_statuses)
        .map_err(|err| err.to_string())?;

    let mut result = DeleteAccountsByStatusesResult {
        scanned,
        deleted: 0,
        skipped_status: 0,
        target_statuses,
        deleted_account_ids: Vec::new(),
    };

    let mut pending_deletes: Vec<(String, String)> = Vec::new();
    for account in accounts {
        let normalized_status = account.status.trim().to_ascii_lowercase();
        pending_deletes.push((
            account.id,
            format!("bulk delete account by status: status={normalized_status}"),
        ));
    }

    let pending_ids = pending_deletes
        .iter()
        .map(|(account_id, _)| account_id.clone())
        .collect::<Vec<_>>();
    if !pending_ids.is_empty() {
        storage
            .delete_accounts(&pending_ids)
            .map_err(|err| err.to_string())?;
        for (account_id, event_message) in pending_deletes {
            crate::storage_helpers::insert_event_best_effort(
                &storage,
                &Event {
                    account_id: Some(account_id.clone()),
                    event_type: "account_bulk_delete_by_status".to_string(),
                    message: event_message,
                    created_at: now_ts(),
                },
                "account.cleanup_by_status",
            );

            result.deleted += 1;
            result.deleted_account_ids.push(account_id);
        }
    }
    result.skipped_status = result.scanned.saturating_sub(result.deleted);

    Ok(result)
}

fn normalize_cleanup_statuses(statuses: Vec<String>) -> Result<Vec<String>, String> {
    let selected = statuses
        .into_iter()
        .filter_map(|status| {
            let normalized = status.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        return Err("missing cleanup statuses".to_string());
    }

    let allowed = CLEANUP_STATUS_ALLOWLIST
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let invalid = selected
        .iter()
        .filter(|status| !allowed.contains(status.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !invalid.is_empty() {
        return Err(format!(
            "unsupported cleanup statuses: {}",
            invalid.join(",")
        ));
    }

    Ok(CLEANUP_STATUS_ALLOWLIST
        .iter()
        .filter(|status| selected.contains(**status))
        .map(|status| (*status).to_string())
        .collect())
}

fn cleanup_status_allowlist_vec() -> Vec<String> {
    CLEANUP_STATUS_ALLOWLIST
        .iter()
        .map(|status| (*status).to_string())
        .collect()
}

fn cleanup_snapshot_record(row: &UsageSnapshotCleanupRow) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: row.account_id.clone(),
        used_percent: row.used_percent,
        window_minutes: row.window_minutes,
        resets_at: None,
        secondary_used_percent: row.secondary_used_percent,
        secondary_window_minutes: row.secondary_window_minutes,
        secondary_resets_at: None,
        credits_json: row.credits_json.clone(),
        captured_at: 0,
    }
}

/// 函数 `plan_label_for_event`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - plan: 参数 plan
///
/// # 返回
/// 返回函数执行结果
fn plan_label_for_event(plan: Option<&ResolvedAccountPlan>) -> Option<&str> {
    plan.and_then(|value| {
        if value.normalized == "unknown" {
            value.raw.as_deref()
        } else {
            Some(value.normalized.as_str())
        }
    })
}

#[cfg(test)]
#[path = "account_cleanup_tests.rs"]
mod tests;
