use std::collections::{BTreeMap, HashMap};

use codexmanager_core::rpc::types::{
    QuotaApiKeyModelUsageItem, QuotaApiKeyUsageItem, QuotaApiKeyUsageResult,
};
use codexmanager_core::storage::{ApiKeyQuotaSummary, ApiKeyTokenUsageSummary, Storage};

use super::model_pricing;
use crate::storage_helpers::open_storage;

pub(super) struct ApiKeyQuotaContext {
    pub(super) api_keys: Vec<ApiKeyQuotaSummary>,
    pub(super) usage_by_key: Vec<ApiKeyTokenUsageSummary>,
}

pub(super) fn load_api_key_quota_context(storage: &Storage) -> Result<ApiKeyQuotaContext, String> {
    if crate::storage_helpers::seaorm_enabled() {
        let api_keys = crate::apikey::list::read_api_keys_with_storage(storage)?
            .into_iter()
            .map(|key| ApiKeyQuotaSummary {
                id: key.id,
                name: key.name,
                model_slug: key.model_slug,
                status: key.status,
                quota_limit_tokens: key.quota_limit_tokens,
                last_used_at: key.last_used_at,
            })
            .collect::<Vec<_>>();
        let key_ids = api_keys
            .iter()
            .map(|key| key.id.clone())
            .collect::<Vec<_>>();
        let usage_by_key = crate::storage_helpers::seaorm_block_on(move |storage| async move {
            let mut rows = codexmanager_storage_seaorm::ApiKeyDetailsRepository::usage_by_key(
                storage.connection(),
                None,
                None,
            )
            .await
            .map_err(|error| error.to_string())?;
            rows.retain(|row| key_ids.contains(&row.key_id));
            Ok(rows)
        })?;
        return Ok(ApiKeyQuotaContext {
            api_keys,
            usage_by_key,
        });
    }
    let api_keys = storage
        .list_api_key_quota_summaries()
        .map_err(|err| format!("list api key quota summaries failed: {err}"))?;
    let key_ids = api_keys
        .iter()
        .map(|key| key.id.clone())
        .collect::<Vec<_>>();
    let usage_by_key = storage
        .summarize_request_token_stats_by_key_for_keys(&key_ids)
        .map_err(|err| format!("summarize api key usage failed: {err}"))?;
    Ok(ApiKeyQuotaContext {
        api_keys,
        usage_by_key,
    })
}

pub(crate) fn read_quota_api_key_usage() -> Result<QuotaApiKeyUsageResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    read_quota_api_key_usage_with_storage(&storage)
}

pub(crate) fn read_quota_api_key_usage_with_storage(
    storage: &Storage,
) -> Result<QuotaApiKeyUsageResult, String> {
    let api_key_context = load_api_key_quota_context(storage)?;
    if api_key_context.api_keys.is_empty() {
        return Ok(QuotaApiKeyUsageResult { items: Vec::new() });
    }
    let catalog_prices = model_pricing::load_catalog_prices(storage)?;
    let usage_map = api_key_context
        .usage_by_key
        .iter()
        .map(|item| (item.key_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let key_ids = api_key_context
        .api_keys
        .iter()
        .map(|key| key.id.clone())
        .collect::<Vec<_>>();
    let model_usage = if crate::storage_helpers::seaorm_enabled() {
        crate::storage_helpers::seaorm_block_on(move |storage| async move {
            codexmanager_storage_seaorm::ApiKeyDetailsRepository::usage_by_key_model(
                storage.connection(),
                None,
                None,
                Some(&key_ids),
            )
            .await
            .map_err(|error| error.to_string())
        })?
    } else {
        storage
            .summarize_request_token_stats_by_key_and_model_for_keys(None, None, &key_ids)
            .map_err(|err| format!("summarize api key model usage failed: {err}"))?
    };
    let mut models_by_key: BTreeMap<String, Vec<QuotaApiKeyModelUsageItem>> = BTreeMap::new();
    for item in model_usage {
        let price = model_pricing::resolve_model_price_from_catalog(
            &catalog_prices,
            item.model.as_str(),
            item.input_tokens,
        );
        models_by_key
            .entry(item.key_id)
            .or_default()
            .push(QuotaApiKeyModelUsageItem {
                model: item.model,
                input_tokens: item.input_tokens,
                cached_input_tokens: item.cached_input_tokens,
                output_tokens: item.output_tokens,
                reasoning_output_tokens: item.reasoning_output_tokens,
                total_tokens: item.total_tokens,
                estimated_cost_usd: (price.is_some() || item.estimated_cost_usd > 0.0)
                    .then_some(item.estimated_cost_usd.max(0.0)),
                price_status: if price.is_some() { "ok" } else { "missing" }.to_string(),
            });
    }

    Ok(QuotaApiKeyUsageResult {
        items: api_key_context
            .api_keys
            .into_iter()
            .map(|key| {
                let used = usage_map
                    .get(key.id.as_str())
                    .map(|item| item.total_tokens.max(0))
                    .unwrap_or(0);
                let limit = key.quota_limit_tokens;
                QuotaApiKeyUsageItem {
                    key_id: key.id.clone(),
                    name: key.name,
                    model_slug: key.model_slug,
                    quota_limit_tokens: limit,
                    used_tokens: used,
                    remaining_tokens: limit.map(|value| value.saturating_sub(used)),
                    estimated_cost_usd: usage_map
                        .get(key.id.as_str())
                        .map(|item| item.estimated_cost_usd.max(0.0))
                        .unwrap_or(0.0),
                    models: models_by_key.remove(key.id.as_str()).unwrap_or_default(),
                }
            })
            .collect(),
    })
}
