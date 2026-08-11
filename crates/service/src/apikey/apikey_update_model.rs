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
    has_quota_limit_tokens: bool,
    quota_limit_tokens: Option<i64>,
    has_policy: bool,
    allowed_models: Vec<String>,
    allowed_platforms: Vec<String>,
    model_visibility: Option<String>,
    expires_at: Option<i64>,
    concurrency_limit: Option<i64>,
) -> Result<(), String> {
    if key_id.is_empty() {
        return Err("key id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    if has_name {
        let normalized_name = name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        storage
            .update_api_key_name(key_id, normalized_name)
            .map_err(|e| e.to_string())?;
    }
    let normalized = model_slug
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let normalized_reasoning = reasoning_effort
        .as_deref()
        .and_then(normalize_reasoning_effort);
    let normalized_service_tier = normalize_service_tier_owned(service_tier)?;
    let normalized_rotation_strategy = normalize_rotation_strategy(rotation_strategy)?;
    let normalized_aggregate_api_id = if normalized_rotation_strategy == ROTATION_AGGREGATE_API {
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
    storage
        .update_api_key_model_config(
            key_id,
            normalized,
            normalized_reasoning,
            normalized_service_tier.as_deref(),
        )
        .map_err(|e| e.to_string())?;
    storage
        .update_api_key_rotation_config(
            key_id,
            normalized_rotation_strategy.as_str(),
            normalized_aggregate_api_id.as_deref(),
            normalized_account_plan_filter.as_deref(),
        )
        .map_err(|e| e.to_string())?;
    if has_quota_limit_tokens {
        storage
            .upsert_api_key_quota_limit(key_id, quota_limit_tokens)
            .map_err(|e| e.to_string())?;
    }
    if has_policy {
        let policy = crate::apikey::policy::normalize_policy(
            key_id,
            allowed_models,
            allowed_platforms,
            model_visibility,
            expires_at,
            concurrency_limit,
        )?;
        crate::apikey::policy::save_api_key_policy_with_storage(&storage, &policy)?;
    }

    let has_upstream_base_url = upstream_base_url.is_some();
    let has_static_headers_json = static_headers_json.is_some();
    let normalized_upstream_base_url = normalize_upstream_base_url(upstream_base_url)?;
    let normalized_static_headers_json = normalize_static_headers_json(static_headers_json)?;

    if protocol_type.is_some() || has_upstream_base_url || has_static_headers_json {
        let current = storage
            .find_api_key_profile_config_by_id(key_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "api key not found".to_string())?;
        let protocol = protocol_type.unwrap_or_else(|| current.protocol_type.clone());
        let normalized_protocol = normalize_protocol_type(Some(protocol))?;
        if normalized_protocol != crate::apikey_profile::PROTOCOL_OPENAI_COMPAT {
            return Err(
                "首版仅支持 OpenAI 格式平台 Key(protocol profile is not available in this release)"
                    .to_string(),
            );
        }
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
    Ok(())
}
