use codexmanager_core::rpc::types::ApiKeySummary;
use codexmanager_core::storage::{ApiKeyListSummary, Storage};

use crate::storage_helpers::open_storage;
use crate::RpcActor;

pub(crate) fn read_api_keys_for_actor(actor: &RpcActor) -> Result<Vec<ApiKeySummary>, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return super::remote::list(actor);
    }
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

pub(super) fn map_seaorm_api_key(key: codexmanager_storage_seaorm::ApiKeyRecord) -> ApiKeySummary {
    ApiKeySummary {
        id: key.id,
        name: key.name,
        model_slug: key.model_slug,
        reasoning_effort: key.reasoning_effort,
        service_tier: key.service_tier,
        rotation_strategy: key.rotation_strategy,
        aggregate_api_id: key.aggregate_api_id,
        account_plan_filter: key.account_plan_filter,
        account_group_filter: key.account_group_filter,
        aggregate_api_url: None,
        quota_limit_tokens: None,
        client_type: key.client_type,
        protocol_type: key.protocol_type,
        auth_scheme: key.auth_scheme,
        upstream_base_url: key.upstream_base_url,
        static_headers_json: key.static_headers_json,
        status: key.status,
        created_at: key.created_at,
        last_used_at: key.last_used_at,
    }
}

pub(crate) fn read_api_keys_with_storage(storage: &Storage) -> Result<Vec<ApiKeySummary>, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return super::remote::list(&RpcActor::system_admin());
    }
    let keys = storage
        .list_api_key_summaries()
        .map_err(|err| format!("list api key summaries failed: {err}"))?;
    Ok(keys.into_iter().map(map_api_key_list_summary).collect())
}

pub(crate) fn read_api_keys_for_ids_with_storage(
    storage: &Storage,
    key_ids: &[String],
) -> Result<Vec<ApiKeySummary>, String> {
    if crate::storage_helpers::seaorm_enabled() {
        let mut keys = super::remote::list(&RpcActor::system_admin())?;
        keys.retain(|key| key_ids.contains(&key.id));
        return Ok(keys);
    }
    let keys = storage
        .list_api_key_summaries_for_ids(key_ids)
        .map_err(|err| format!("list api key summaries failed: {err}"))?;
    Ok(keys.into_iter().map(map_api_key_list_summary).collect())
}

pub(crate) fn map_api_key_list_summary(key: ApiKeyListSummary) -> ApiKeySummary {
    ApiKeySummary {
        quota_limit_tokens: key.quota_limit_tokens,
        id: key.id,
        name: key.name,
        model_slug: key.model_slug,
        reasoning_effort: key.reasoning_effort,
        service_tier: key.service_tier,
        rotation_strategy: key.rotation_strategy,
        aggregate_api_id: key.aggregate_api_id,
        account_plan_filter: key.account_plan_filter,
        account_group_filter: key.account_group_filter,
        aggregate_api_url: key.aggregate_api_url,
        client_type: key.client_type,
        protocol_type: key.protocol_type,
        auth_scheme: key.auth_scheme,
        upstream_base_url: key.upstream_base_url,
        static_headers_json: key.static_headers_json,
        status: key.status,
        created_at: key.created_at,
        last_used_at: key.last_used_at,
    }
}

#[cfg(test)]
mod tests {
    use super::map_seaorm_api_key;
    use codexmanager_storage_seaorm::ApiKeyRecord;

    #[test]
    fn seaorm_record_mapping_preserves_profile_and_redacts_secret_fields() {
        let summary = map_seaorm_api_key(ApiKeyRecord {
            id: "key-1".into(),
            name: Some("Gateway".into()),
            model_slug: Some("gpt-5".into()),
            reasoning_effort: Some("high".into()),
            service_tier: None,
            rotation_strategy: "account_rotation".into(),
            aggregate_api_id: None,
            account_plan_filter: Some("pro".into()),
            account_group_filter: None,
            client_type: "codex".into(),
            protocol_type: "openai_compat".into(),
            auth_scheme: "authorization_bearer".into(),
            upstream_base_url: Some("https://example.invalid".into()),
            static_headers_json: None,
            key_hash: "hashed".into(),
            status: "active".into(),
            created_at: 42,
            last_used_at: Some(43),
        });
        assert_eq!(summary.id, "key-1");
        assert_eq!(summary.model_slug.as_deref(), Some("gpt-5"));
        assert_eq!(summary.quota_limit_tokens, None);
        assert_eq!(summary.aggregate_api_url, None);
    }
}
