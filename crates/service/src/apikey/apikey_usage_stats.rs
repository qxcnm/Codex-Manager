use std::collections::{BTreeSet, HashMap};

use codexmanager_core::{rpc::types::ApiKeyUsageStatSummary, storage::ApiKeyTokenUsageSummary};

use crate::storage_helpers::open_storage;
use crate::{time_bounds, RpcActor};

pub(crate) fn read_api_key_usage_stats_for_actor(
    actor: &RpcActor,
) -> Result<Vec<ApiKeyUsageStatSummary>, String> {
    let (today_start, today_end) = time_bounds::local_day_bounds_ts()?;
    if crate::storage_helpers::seaorm_enabled() {
        let keys = if actor.is_admin() {
            None
        } else {
            Some(
                super::remote::list(actor)?
                    .into_iter()
                    .map(|key| key.id)
                    .collect::<Vec<_>>(),
            )
        };
        return crate::storage_helpers::seaorm_block_on(move |storage| async move {
            use codexmanager_storage_seaorm::ApiKeyDetailsRepository;
            let mut totals =
                ApiKeyDetailsRepository::usage_by_key(storage.connection(), None, None)
                    .await
                    .map_err(|error| error.to_string())?;
            if let Some(keys) = keys.as_ref() {
                totals.retain(|row| keys.contains(&row.key_id));
            }
            let today = ApiKeyDetailsRepository::usage_by_key_model(
                storage.connection(),
                Some(today_start),
                Some(today_end),
                keys.as_deref(),
            )
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|row| ApiKeyTokenUsageSummary {
                key_id: row.key_id,
                total_tokens: row.total_tokens,
                estimated_cost_usd: row.estimated_cost_usd,
            })
            .collect();
            Ok(map_api_key_usage_stats(totals, today))
        });
    }
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;

    if actor.is_admin() {
        let total_items = storage
            .summarize_request_token_stats_by_key()
            .map_err(|err| format!("summarize api key token stats failed: {err}"))?;
        let today_items = storage
            .summarize_request_token_stats_by_key_and_model(Some(today_start), Some(today_end))
            .map_err(|err| format!("summarize api key today token stats failed: {err}"))?
            .into_iter()
            .map(|item| ApiKeyTokenUsageSummary {
                key_id: item.key_id,
                total_tokens: item.total_tokens,
                estimated_cost_usd: item.estimated_cost_usd,
            })
            .collect();
        return Ok(map_api_key_usage_stats(total_items, today_items));
    }

    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: apikey usage requires user session".to_string())?;
    let total_items = storage
        .summarize_request_token_stats_by_key_for_user(user_id)
        .map_err(|err| format!("summarize api key token stats failed: {err}"))?;
    let key_ids: Vec<String> = total_items
        .iter()
        .map(|item| item.key_id.clone())
        .filter(|key_id| !key_id.trim().is_empty())
        .collect();
    let today_items = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(
            Some(today_start),
            Some(today_end),
            &key_ids,
        )
        .map_err(|err| format!("summarize api key today token stats failed: {err}"))?
        .into_iter()
        .map(|item| ApiKeyTokenUsageSummary {
            key_id: item.key_id,
            total_tokens: item.total_tokens,
            estimated_cost_usd: item.estimated_cost_usd,
        })
        .collect();

    Ok(map_api_key_usage_stats(total_items, today_items))
}

fn merge_usage_by_key(items: Vec<ApiKeyTokenUsageSummary>) -> HashMap<String, (i64, f64)> {
    let mut by_key: HashMap<String, (i64, f64)> = HashMap::new();
    for item in items {
        let key_id = item.key_id.trim().to_string();
        if key_id.is_empty() {
            continue;
        }
        let entry = by_key.entry(key_id).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(item.total_tokens.max(0));
        entry.1 += item.estimated_cost_usd.max(0.0);
    }
    by_key
}

fn map_api_key_usage_stats(
    total_items: Vec<ApiKeyTokenUsageSummary>,
    today_items: Vec<ApiKeyTokenUsageSummary>,
) -> Vec<ApiKeyUsageStatSummary> {
    let total_by_key = merge_usage_by_key(total_items);
    let today_by_key = merge_usage_by_key(today_items);
    let keys: BTreeSet<String> = total_by_key
        .keys()
        .chain(today_by_key.keys())
        .cloned()
        .collect();

    keys.into_iter()
        .map(|key_id| {
            let (total_tokens, estimated_cost_usd) =
                total_by_key.get(&key_id).copied().unwrap_or((0, 0.0));
            let (today_tokens, today_estimated_cost_usd) =
                today_by_key.get(&key_id).copied().unwrap_or((0, 0.0));
            ApiKeyUsageStatSummary {
                key_id,
                today_tokens: today_tokens.max(0),
                today_estimated_cost_usd: today_estimated_cost_usd.max(0.0),
                total_tokens: total_tokens.max(0),
                estimated_cost_usd: estimated_cost_usd.max(0.0),
            }
        })
        .collect()
}
