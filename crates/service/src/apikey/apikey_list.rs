use codexmanager_core::rpc::types::ApiKeySummary;
use codexmanager_core::storage::{ApiKeyListSummary, Storage};

use crate::storage_helpers::open_storage;
use crate::RpcActor;

pub(crate) fn read_api_keys_for_actor(actor: &RpcActor) -> Result<Vec<ApiKeySummary>, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    if actor.is_admin() {
        return read_api_keys_with_storage(&storage);
    }
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: apikey requires user session".to_string())?;
    let keys = storage
        .list_api_key_summaries_for_user(user_id)
        .map_err(|err| format!("list user api key summaries failed: {err}"))?;
    Ok(keys.into_iter().map(map_api_key_list_summary).collect())
}

pub(crate) fn read_api_keys_with_storage(storage: &Storage) -> Result<Vec<ApiKeySummary>, String> {
    let keys = storage
        .list_api_key_summaries()
        .map_err(|err| format!("list api key summaries failed: {err}"))?;
    Ok(keys.into_iter().map(map_api_key_list_summary).collect())
}

pub(crate) fn read_api_keys_for_ids_with_storage(
    storage: &Storage,
    key_ids: &[String],
) -> Result<Vec<ApiKeySummary>, String> {
    let keys = storage
        .list_api_key_summaries_for_ids(key_ids)
        .map_err(|err| format!("list api key summaries failed: {err}"))?;
    Ok(keys.into_iter().map(map_api_key_list_summary).collect())
}

fn map_api_key_list_summary(key: ApiKeyListSummary) -> ApiKeySummary {
    ApiKeySummary {
        quota_limit_tokens: key.quota_limit_tokens,
        allowed_models: key.allowed_models,
        allowed_platforms: key.allowed_platforms,
        model_visibility: key.model_visibility,
        expires_at: key.expires_at,
        concurrency_limit: key.concurrency_limit,
        id: key.id,
        name: key.name,
        model_slug: key.model_slug,
        reasoning_effort: key.reasoning_effort,
        service_tier: key.service_tier,
        rotation_strategy: key.rotation_strategy,
        aggregate_api_id: key.aggregate_api_id,
        account_plan_filter: key.account_plan_filter,
        aggregate_api_url: key.aggregate_api_url,
        client_type: key.client_type,
        protocol_type: key.protocol_type,
        protocol_profile: "openai".to_string(),
        auth_scheme: key.auth_scheme,
        upstream_base_url: key.upstream_base_url,
        static_headers_json: key.static_headers_json,
        status: key.status,
        created_at: key.created_at,
        last_used_at: key.last_used_at,
    }
}
