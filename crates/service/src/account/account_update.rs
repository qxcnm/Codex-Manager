use codexmanager_core::storage::{now_ts, Event};
use serde::Serialize;
use std::collections::HashSet;

use crate::{account_status, storage_helpers::open_storage};

#[derive(Debug, Clone)]
pub(crate) struct AccountSortUpdate {
    pub account_id: String,
    pub sort: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct AccountSortUpdateResult {
    updated: usize,
}

/// 函数 `update_account`
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
pub(crate) fn update_account(
    account_id: &str,
    sort: Option<i64>,
    preferred: Option<bool>,
    status: Option<&str>,
    label: Option<&str>,
    group_name: Option<&str>,
    has_group_name: bool,
    note: Option<&str>,
    tags: Option<&str>,
    quota_capacity_primary_window_tokens: Option<i64>,
    quota_capacity_secondary_window_tokens: Option<i64>,
) -> Result<(), String> {
    // 更新账号排序或状态并记录事件
    let normalized_account_id = account_id.trim();
    if normalized_account_id.is_empty() {
        return Err("missing accountId".to_string());
    }

    let normalized_status = status.map(normalize_account_status).transpose()?;
    let normalized_label = normalize_optional_label(label)?;
    let normalized_group_name = group_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let normalized_note = normalize_optional_text(note);
    let normalized_tags = normalize_optional_tags(tags);
    let metadata_requested = note.is_some() || tags.is_some();
    let quota_override_requested = quota_capacity_primary_window_tokens.is_some()
        || quota_capacity_secondary_window_tokens.is_some();

    if sort.is_none()
        && preferred.is_none()
        && normalized_status.is_none()
        && normalized_label.is_none()
        && !has_group_name
        && !metadata_requested
        && !quota_override_requested
    {
        return Err("missing account update fields".to_string());
    }

    let mut storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let now = now_ts();
    if let Some(preferred) = preferred {
        if preferred {
            let found = storage
                .account_exists(normalized_account_id)
                .map_err(|err| err.to_string())?;
            if !found {
                return Err("account not found".to_string());
            }
            storage
                .set_preferred_account(Some(normalized_account_id))
                .map_err(|e| e.to_string())?;
        } else {
            storage
                .clear_preferred_account_if(normalized_account_id)
                .map_err(|e| e.to_string())?;
        }
        crate::storage_helpers::insert_event_best_effort(
            &storage,
            &Event {
                account_id: Some(normalized_account_id.to_string()),
                event_type: "account_preferred_update".to_string(),
                message: format!("preferred={preferred}"),
                created_at: now,
            },
            "account.update_preferred",
        );
    }

    if let Some(sort) = sort {
        storage
            .update_account_sort(normalized_account_id, sort)
            .map_err(|e| e.to_string())?;
        crate::storage_helpers::insert_event_best_effort(
            &storage,
            &Event {
                account_id: Some(normalized_account_id.to_string()),
                event_type: "account_sort_update".to_string(),
                message: format!("sort={sort}"),
                created_at: now,
            },
            "account.update_sort",
        );
    }

    if let Some(status) = normalized_status {
        let reason = if status == "disabled" {
            "manual_disable"
        } else {
            "manual_enable"
        };
        account_status::set_account_status(&storage, normalized_account_id, status, reason);
    }

    if let Some(label) = normalized_label {
        storage
            .update_account_label(normalized_account_id, label)
            .map_err(|e| e.to_string())?;
        crate::storage_helpers::insert_event_best_effort(
            &storage,
            &Event {
                account_id: Some(normalized_account_id.to_string()),
                event_type: "account_profile_update".to_string(),
                message: format!("label={label}"),
                created_at: now,
            },
            "account.update_label",
        );
    }

    if has_group_name {
        storage
            .update_account_group_name(normalized_account_id, normalized_group_name.as_deref())
            .map_err(|e| e.to_string())?;
        crate::storage_helpers::insert_event_best_effort(
            &storage,
            &Event {
                account_id: Some(normalized_account_id.to_string()),
                event_type: "account_group_update".to_string(),
                message: format!(
                    "group_name={}",
                    normalized_group_name.as_deref().unwrap_or("-")
                ),
                created_at: now,
            },
            "account.update_group",
        );
    }

    if metadata_requested {
        storage
            .upsert_account_metadata(
                normalized_account_id,
                normalized_note.as_deref(),
                normalized_tags.as_deref(),
            )
            .map_err(|e| e.to_string())?;
        storage
            .touch_account_updated_at(normalized_account_id)
            .map_err(|e| e.to_string())?;
        crate::storage_helpers::insert_event_best_effort(
            &storage,
            &Event {
                account_id: Some(normalized_account_id.to_string()),
                event_type: "account_profile_update".to_string(),
                message: format!(
                    "note={} tags={}",
                    normalized_note.as_deref().unwrap_or("-"),
                    normalized_tags.as_deref().unwrap_or("-"),
                ),
                created_at: now,
            },
            "account.update_metadata",
        );
    }

    if quota_override_requested {
        storage
            .upsert_account_quota_capacity_override(
                normalized_account_id,
                quota_capacity_primary_window_tokens,
                quota_capacity_secondary_window_tokens,
            )
            .map_err(|e| e.to_string())?;
        storage
            .touch_account_updated_at(normalized_account_id)
            .map_err(|e| e.to_string())?;
        crate::storage_helpers::insert_event_best_effort(
            &storage,
            &Event {
                account_id: Some(normalized_account_id.to_string()),
                event_type: "account_quota_capacity_update".to_string(),
                message: format!(
                    "primary={:?} secondary={:?}",
                    quota_capacity_primary_window_tokens, quota_capacity_secondary_window_tokens
                ),
                created_at: now,
            },
            "account.update_quota_capacity",
        );
    }

    Ok(())
}

pub(crate) fn update_account_sorts(
    updates: Vec<AccountSortUpdate>,
) -> Result<AccountSortUpdateResult, String> {
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for update in updates {
        let account_id = update.account_id.trim().to_string();
        if account_id.is_empty() {
            return Err("missing accountId".to_string());
        }
        if !seen.insert(account_id.clone()) {
            return Err(format!("duplicate accountId: {account_id}"));
        }
        normalized.push((account_id, update.sort));
    }
    if normalized.is_empty() {
        return Err("missing account sort updates".to_string());
    }

    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let updated = storage
        .update_account_sorts(normalized.as_slice(), now_ts())
        .map_err(|e| {
            let message = e.to_string();
            message
                .strip_prefix("Invalid parameter name: ")
                .unwrap_or(message.as_str())
                .to_string()
        })?;
    Ok(AccountSortUpdateResult { updated })
}

/// 函数 `normalize_account_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
///
/// # 返回
/// 返回函数执行结果
fn normalize_account_status(status: &str) -> Result<&'static str, String> {
    let normalized = status.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "active" => Ok("active"),
        "disabled" | "inactive" => Ok("disabled"),
        _ => Err(format!("unsupported account status: {status}")),
    }
}

/// 函数 `normalize_optional_label`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - label: 参数 label
///
/// # 返回
/// 返回函数执行结果
fn normalize_optional_label(label: Option<&str>) -> Result<Option<&str>, String> {
    let Some(label) = label else {
        return Ok(None);
    };
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err("label cannot be empty".to_string());
    }
    Ok(Some(trimmed))
}

/// 函数 `normalize_optional_text`
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
fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

/// 函数 `normalize_optional_tags`
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
fn normalize_optional_tags(value: Option<&str>) -> Option<String> {
    let Some(value) = value else {
        return None;
    };
    let parts = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

#[cfg(test)]
#[path = "account_update_tests.rs"]
mod tests;
