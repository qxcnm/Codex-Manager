use crate::apikey::service_tier::normalize_service_tier_owned;
use crate::apikey_profile::{
    normalize_protocol_type, normalize_rotation_strategy, normalize_static_headers_json,
    normalize_upstream_base_url, profile_from_protocol, ROTATION_AGGREGATE_API,
};
use crate::reasoning_effort::normalize_reasoning_effort;
use crate::storage_helpers::open_storage;

/// 函数 `update_api_key_model`
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
pub(crate) fn update_api_key_model(
    key_id: &str,
    name: Option<String>,
    has_name: bool,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    protocol_type: Option<String>,
    upstream_base_url: Option<String>,
    static_headers_json: Option<String>,
    rotation_strategy: Option<String>,
    aggregate_api_id: Option<String>,
    account_plan_filter: Option<String>,
    account_group_filter: Option<String>,
    update_model_config: bool,
    update_routing_config: bool,
    update_account_group_filter: bool,
    has_quota_limit_tokens: bool,
    quota_limit_tokens: Option<i64>,
) -> Result<(), String> {
    if key_id.is_empty() {
        return Err("key id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    if update_model_config {
        crate::models_v2::ensure_text_generation_model(&storage, model_slug.as_deref())?;
    }
    let normalized_name = name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized = model_slug
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let normalized_reasoning = reasoning_effort
        .as_deref()
        .and_then(normalize_reasoning_effort);
    let normalized_service_tier = normalize_service_tier_owned(service_tier)?;
    // Validate every admin-only routing input before the first write. Member updates skip this
    // branch entirely and therefore preserve all administrator-managed routing fields.
    let normalized_routing_config = if update_routing_config {
        let normalized_rotation_strategy = normalize_rotation_strategy(rotation_strategy)?;
        let normalized_aggregate_api_id = if normalized_rotation_strategy == ROTATION_AGGREGATE_API
        {
            aggregate_api_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        } else {
            None
        };
        let normalized_account_plan_filter = if normalized_rotation_strategy
            == crate::apikey_profile::ROTATION_ACCOUNT
            || normalized_rotation_strategy == crate::apikey_profile::ROTATION_HYBRID
        {
            crate::account_plan::normalize_account_plan_filter(account_plan_filter)?
        } else {
            None
        };
        Some((
            normalized_rotation_strategy,
            normalized_aggregate_api_id,
            normalized_account_plan_filter,
        ))
    } else {
        None
    };
    let effective_group_rotation_strategy =
        if let Some((strategy, _, _)) = normalized_routing_config.as_ref() {
            Some(strategy.clone())
        } else if update_account_group_filter {
            Some(
                storage
                    .find_api_key_by_id(key_id)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "api key not found".to_string())?
                    .rotation_strategy,
            )
        } else {
            None
        };
    let normalized_account_group_filter_update = match effective_group_rotation_strategy.as_deref()
    {
        Some(ROTATION_AGGREGATE_API) => Some(None),
        Some(crate::apikey_profile::ROTATION_ACCOUNT)
        | Some(crate::apikey_profile::ROTATION_HYBRID)
            if update_account_group_filter =>
        {
            Some(crate::account_group::normalize_account_group_filter(
                account_group_filter,
            ))
        }
        _ => None,
    };

    if has_name {
        storage
            .update_api_key_name(key_id, normalized_name)
            .map_err(|e| e.to_string())?;
    }
    if update_model_config {
        storage
            .update_api_key_model_config(
                key_id,
                normalized,
                normalized_reasoning,
                normalized_service_tier.as_deref(),
            )
            .map_err(|e| e.to_string())?;
    }
    if let Some((
        normalized_rotation_strategy,
        normalized_aggregate_api_id,
        normalized_account_plan_filter,
    )) = normalized_routing_config
    {
        storage
            .update_api_key_rotation_config(
                key_id,
                normalized_rotation_strategy.as_str(),
                normalized_aggregate_api_id.as_deref(),
                normalized_account_plan_filter.as_deref(),
            )
            .map_err(|e| e.to_string())?;
    }
    if let Some(normalized_account_group_filter) = normalized_account_group_filter_update {
        storage
            .update_api_key_account_group_filter(key_id, normalized_account_group_filter.as_deref())
            .map_err(|e| e.to_string())?;
    }
    if has_quota_limit_tokens {
        storage
            .upsert_api_key_quota_limit(key_id, quota_limit_tokens)
            .map_err(|e| e.to_string())?;
    }

    let has_protocol_type = protocol_type.is_some();
    let has_upstream_base_url = upstream_base_url.is_some();
    let has_static_headers_json = static_headers_json.is_some();
    let normalized_upstream_base_url = normalize_upstream_base_url(upstream_base_url)?;
    let normalized_static_headers_json = normalize_static_headers_json(static_headers_json)?;

    if has_protocol_type || has_upstream_base_url || has_static_headers_json {
        let current = storage
            .find_api_key_profile_config_by_id(key_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "api key not found".to_string())?;
        let protocol = protocol_type.unwrap_or_else(|| current.protocol_type.clone());
        let normalized_protocol = normalize_protocol_type(Some(protocol))?;
        let (next_client, next_protocol, next_auth) = profile_from_protocol(&normalized_protocol)?;
        let next_upstream_base_url = if has_upstream_base_url {
            normalized_upstream_base_url.as_deref()
        } else {
            current.upstream_base_url.as_deref()
        };
        let next_static_headers_json = if has_static_headers_json {
            normalized_static_headers_json.as_deref()
        } else {
            current.static_headers_json.as_deref()
        };
        storage
            .update_api_key_profile_config(
                key_id,
                &next_client,
                &next_protocol,
                &next_auth,
                next_upstream_base_url,
                next_static_headers_json,
                normalized_service_tier
                    .as_deref()
                    .or(current.service_tier.as_deref()),
            )
            .map_err(|e| e.to_string())?;
    }
    if update_routing_config || has_protocol_type || has_upstream_base_url {
        crate::codex_profile::sync_active_gateway_profile_for_api_key(&storage, key_id)?;
    }
    Ok(())
}
