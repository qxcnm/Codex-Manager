use codexmanager_core::rpc::types::ApiKeyCreateResult;
use codexmanager_core::storage::{now_ts, ApiKey, Storage};

use crate::apikey::service_tier::normalize_service_tier_owned;
use crate::apikey_profile::{
    normalize_protocol_type, normalize_rotation_strategy, normalize_static_headers_json,
    normalize_upstream_base_url, profile_from_protocol,
};
use crate::reasoning_effort::normalize_reasoning_effort_owned;
use crate::storage_helpers::{
    generate_key_id, generate_platform_key, hash_platform_key, open_storage,
};

fn normalize_custom_key(custom_key: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = custom_key else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(
            "自定义 API Key 不能包含空白字符(custom api key must not contain whitespace)"
                .to_string(),
        );
    }
    if value.len() > 512 {
        return Err("自定义 API Key 过长(custom api key is too long)".to_string());
    }
    Ok(Some(value.to_string()))
}

fn platform_key_exists(storage: &Storage, key: &str) -> Result<bool, String> {
    let key_hash = hash_platform_key(key);
    Ok(storage
        .api_key_hash_exists(&key_hash)
        .map_err(|err| format!("check api key uniqueness failed: {err}"))?)
}

fn ensure_platform_key_not_exists(storage: &Storage, key: &str) -> Result<(), String> {
    if platform_key_exists(storage, key)? {
        return Err("自定义 API Key 已存在(custom api key already exists)".to_string());
    }
    Ok(())
}

fn ensure_first_release_protocol(protocol_type: &str) -> Result<(), String> {
    if protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT {
        return Ok(());
    }
    Err(
        "首版仅支持 OpenAI 格式平台 Key(protocol profile is not available in this release)"
            .to_string(),
    )
}

fn resolve_platform_key(storage: &Storage, custom_key: Option<String>) -> Result<String, String> {
    if let Some(key) = normalize_custom_key(custom_key)? {
        ensure_platform_key_not_exists(storage, &key)?;
        return Ok(key);
    }

    for _ in 0..4 {
        let key = generate_platform_key();
        if !platform_key_exists(storage, &key)? {
            return Ok(key);
        }
    }

    Err("生成平台密钥失败(failed to generate unique api key)".to_string())
}

/// 函数 `create_api_key`
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
pub(crate) fn create_api_key(
    name: Option<String>,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    protocol_type: Option<String>,
    upstream_base_url: Option<String>,
    static_headers_json: Option<String>,
    rotation_strategy: Option<String>,
    aggregate_api_id: Option<String>,
    account_plan_filter: Option<String>,
    quota_limit_tokens: Option<i64>,
    allowed_models: Vec<String>,
    allowed_platforms: Vec<String>,
    model_visibility: Option<String>,
    expires_at: Option<i64>,
    concurrency_limit: Option<i64>,
    custom_key: Option<String>,
) -> Result<ApiKeyCreateResult, String> {
    // 创建平台 Key 并写入存储
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let key = resolve_platform_key(&storage, custom_key)?;
    let key_hash = hash_platform_key(&key);
    let key_id = generate_key_id();
    let policy = crate::apikey::policy::normalize_policy(
        &key_id,
        allowed_models,
        allowed_platforms,
        model_visibility,
        expires_at,
        concurrency_limit,
    )?;
    let protocol_type = normalize_protocol_type(protocol_type)?;
    ensure_first_release_protocol(&protocol_type)?;
    let (client_type, protocol_type, auth_scheme) = profile_from_protocol(&protocol_type)?;
    let upstream_base_url = normalize_upstream_base_url(upstream_base_url)?;
    let static_headers_json = normalize_static_headers_json(static_headers_json)?;
    let rotation_strategy = normalize_rotation_strategy(rotation_strategy)?;
    let aggregate_api_id = if rotation_strategy == crate::apikey_profile::ROTATION_AGGREGATE_API {
        aggregate_api_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    } else {
        None
    };
    let account_plan_filter = if rotation_strategy == crate::apikey_profile::ROTATION_ACCOUNT
        || rotation_strategy == crate::apikey_profile::ROTATION_HYBRID
    {
        crate::account_plan::normalize_account_plan_filter(account_plan_filter)?
    } else {
        None
    };
    let record = ApiKey {
        id: key_id.clone(),
        name,
        model_slug,
        reasoning_effort: normalize_reasoning_effort_owned(reasoning_effort),
        service_tier: normalize_service_tier_owned(service_tier)?,
        rotation_strategy,
        aggregate_api_id,
        account_plan_filter,
        aggregate_api_url: None,
        client_type,
        protocol_type,
        auth_scheme,
        upstream_base_url,
        static_headers_json,
        key_hash,
        status: "active".to_string(),
        created_at: now_ts(),
        last_used_at: None,
    };
    storage.insert_api_key(&record).map_err(|e| e.to_string())?;
    if let Err(err) = storage.upsert_api_key_quota_limit(&key_id, quota_limit_tokens) {
        let _ = storage.delete_api_key(&key_id);
        return Err(format!("persist api key quota limit failed: {err}"));
    }
    if let Err(err) = crate::apikey::policy::save_api_key_policy_with_storage(&storage, &policy) {
        let _ = storage.delete_api_key(&key_id);
        return Err(err);
    }
    if let Err(err) = storage.upsert_api_key_secret(&key_id, &key) {
        let _ = storage.delete_api_key(&key_id);
        return Err(format!("persist api key secret failed: {err}"));
    }
    Ok(ApiKeyCreateResult { id: key_id, key })
}

#[cfg(test)]
mod tests {
    use super::ensure_first_release_protocol;

    #[test]
    fn first_release_accepts_only_openai_key_protocol() {
        assert!(ensure_first_release_protocol("openai_compat").is_ok());
        assert!(ensure_first_release_protocol("anthropic_native").is_err());
        assert!(ensure_first_release_protocol("gemini_native").is_err());
    }
}
