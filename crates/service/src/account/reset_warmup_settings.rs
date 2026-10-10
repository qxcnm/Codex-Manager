use std::collections::HashSet;

use codexmanager_core::storage::{now_ts, Event};
use serde::{Deserialize, Serialize};

use crate::storage_helpers::open_storage;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResetWarmupUpdate {
    account_ids: Vec<String>,
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct ResetWarmupUpdateResult {
    updated: usize,
}

pub(crate) fn update(params: ResetWarmupUpdate) -> Result<ResetWarmupUpdateResult, String> {
    let mut seen = HashSet::new();
    let mut account_ids = Vec::new();
    for id in params.account_ids {
        let id = id.trim();
        if id.is_empty() {
            return Err("missing accountId".to_string());
        }
        if seen.insert(id.to_string()) {
            account_ids.push(id.to_string());
        }
    }
    if account_ids.is_empty() {
        return Err("missing accountIds".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let updated = storage
        .set_account_reset_warmup_enabled(&account_ids, params.enabled)
        .map_err(|err| err.to_string())?;
    for id in account_ids {
        let _ = storage.insert_event(&Event {
            account_id: Some(id),
            event_type: "account_reset_warmup_update".to_string(),
            message: format!("enabled={}", params.enabled),
            created_at: now_ts(),
        });
    }
    Ok(ResetWarmupUpdateResult { updated })
}
