use codexmanager_core::rpc::types::ApiKeyCreateResult;
use codexmanager_core::storage::{
    now_ts, ApiKey, ApiKeyConfigPatch, ApiKeyCreate, ApiKeyModelConfig, ApiKeyProtocolConfig,
    ApiKeyRoutingConfig, DomainStorage,
};

use crate::apikey::service_tier::normalize_service_tier_owned;
use crate::apikey_profile::{
    normalize_protocol_type, normalize_rotation_strategy, normalize_static_headers_json,
    normalize_upstream_base_url, profile_from_protocol, ROTATION_ACCOUNT, ROTATION_AGGREGATE_API,
    ROTATION_HYBRID, ROTATION_HYBRID_AGGREGATE_FIRST,
};
use crate::reasoning_effort::normalize_reasoning_effort_owned;
use crate::storage_helpers::{generate_key_id, generate_platform_key, hash_platform_key};
use crate::RpcActor;

fn custom_key(value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value else { return Ok(None) };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(
            "自定义 API Key 不能包含空白字符(custom api key must not contain whitespace)".into(),
        );
    }
    if value.len() > 512 {
        return Err("自定义 API Key 过长(custom api key is too long)".into());
    }
    Ok(Some(value.to_owned()))
}

async fn text_model(storage: &dyn DomainStorage, slug: Option<&str>) -> Result<(), String> {
    let Some(slug) = slug.map(str::trim).filter(|slug| !slug.is_empty()) else {
        return Ok(());
    };
    let Some(model) = storage.model(slug.to_owned()).await? else {
        // Keep the guard useful while a fresh SeaORM catalog is still
        // materializing built-in rows. Unknown external slugs remain allowed.
        let normalized_slug = slug.to_ascii_lowercase();
        if normalized_slug.starts_with("gpt-image-")
            || normalized_slug.starts_with("chatgpt-image-")
        {
            return Err(format!("图片专用模型不能作为文本主模型(image-only model cannot be used as a text-generation primary model): {slug}"));
        }
        return Ok(());
    };
    let supports_text = model
        .capabilities
        .get("supports_text_generation")
        .or_else(|| model.capabilities.get("supportsTextGeneration"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    if !supports_text {
        return Err(format!("图片专用模型不能作为文本主模型(image-only model cannot be used as a text-generation primary model): {}", model.slug));
    }
    Ok(())
}

async fn unique_key(
    storage: &dyn DomainStorage,
    requested: Option<String>,
) -> Result<String, String> {
    if let Some(key) = custom_key(requested)? {
        if storage
            .api_key_by_hash(hash_platform_key(&key))
            .await?
            .is_some()
        {
            return Err("自定义 API Key 已存在(custom api key already exists)".into());
        }
        return Ok(key);
    }
    for _ in 0..4 {
        let key = generate_platform_key();
        if storage
            .api_key_by_hash(hash_platform_key(&key))
            .await?
            .is_none()
        {
            return Ok(key);
        }
    }
    Err("生成平台密钥失败(failed to generate unique api key)".into())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn create(
    storage: &dyn DomainStorage,
    actor: &RpcActor,
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
    account_group_filter: Option<String>,
    quota_limit_tokens: Option<i64>,
    custom_key: Option<String>,
) -> Result<ApiKeyCreateResult, String> {
    text_model(storage, model_slug.as_deref()).await?;
    let key = unique_key(storage, custom_key).await?;
    let protocol = normalize_protocol_type(protocol_type)?;
    let (client_type, protocol_type, auth_scheme) = profile_from_protocol(&protocol)?;
    let rotation_strategy = normalize_rotation_strategy(rotation_strategy)?;
    let account_plan_filter = if [
        ROTATION_ACCOUNT,
        ROTATION_HYBRID,
        ROTATION_HYBRID_AGGREGATE_FIRST,
    ]
    .contains(&rotation_strategy.as_str())
    {
        crate::account_plan::normalize_account_plan_filter(account_plan_filter)?
    } else {
        None
    };
    let account_group_filter = if [
        ROTATION_ACCOUNT,
        ROTATION_HYBRID,
        ROTATION_HYBRID_AGGREGATE_FIRST,
    ]
    .contains(&rotation_strategy.as_str())
    {
        crate::account_group::normalize_account_group_filter(account_group_filter)
    } else {
        None
    };
    let aggregate_api_id = (rotation_strategy == ROTATION_AGGREGATE_API)
        .then(|| aggregate_api_id)
        .flatten()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let secret_hash = hash_platform_key(&key);
    let record = ApiKey {
        id: generate_key_id(),
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
        upstream_base_url: normalize_upstream_base_url(upstream_base_url)?,
        static_headers_json: normalize_static_headers_json(static_headers_json)?,
        key_hash: secret_hash,
        status: "active".into(),
        created_at: now_ts(),
        last_used_at: None,
    };
    let owner_user_id = if actor.is_admin() {
        None
    } else {
        Some(
            actor
                .user_id
                .clone()
                .ok_or_else(|| "permission_denied: apikey requires user session".to_owned())?,
        )
    };
    storage
        .create_api_key(ApiKeyCreate {
            key: record.clone(),
            secret: key.clone(),
            account_group_filter,
            quota_limit_tokens,
            owner_user_id,
        })
        .await?;
    Ok(ApiKeyCreateResult { id: record.id, key })
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn update(
    storage: &dyn DomainStorage,
    actor: &RpcActor,
    id: &str,
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
    update_model: bool,
    update_routing: bool,
    update_group: bool,
    has_quota: bool,
    quota: Option<i64>,
) -> Result<(), String> {
    if id.is_empty() {
        return Err("key id required".into());
    }
    let existing = storage
        .api_key(id.to_owned())
        .await?
        .ok_or_else(|| "api key not found".to_owned())?;
    if !actor.is_admin() {
        let user = actor
            .user_id
            .clone()
            .ok_or_else(|| "permission_denied: apikey requires user session".to_owned())?;
        let owner = storage.api_key_owner(id.to_owned()).await?;
        if !owner.is_some_and(|owner| {
            owner.owner_kind == "user" && owner.owner_user_id.as_deref() == Some(user.as_str())
        }) {
            return Err("permission_denied: apikey".into());
        }
    }
    if update_model {
        text_model(storage, model_slug.as_deref()).await?;
    }
    let mut patch = ApiKeyConfigPatch {
        owner_user_id: actor.user_id.clone(),
        ..Default::default()
    };
    if has_name {
        patch.name = Some(name.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()));
    }
    if update_model {
        patch.model = Some(ApiKeyModelConfig {
            model_slug: model_slug
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
            reasoning_effort: reasoning_effort
                .as_deref()
                .and_then(crate::reasoning_effort::normalize_reasoning_effort)
                .map(str::to_owned),
            service_tier: normalize_service_tier_owned(service_tier.clone())?,
        });
    }
    if update_routing {
        let rotation = normalize_rotation_strategy(rotation_strategy)?;
        let plan = if rotation == ROTATION_AGGREGATE_API {
            None
        } else {
            crate::account_plan::normalize_account_plan_filter(account_plan_filter)?
        };
        patch.routing = Some(ApiKeyRoutingConfig {
            rotation_strategy: rotation.clone(),
            aggregate_api_id: (rotation == ROTATION_AGGREGATE_API)
                .then(|| aggregate_api_id)
                .flatten()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
            account_plan_filter: plan,
        });
    }
    if update_group {
        patch.account_group_filter = Some(crate::account_group::normalize_account_group_filter(
            account_group_filter,
        ));
    }
    if has_quota {
        patch.quota_limit_tokens = Some(quota);
    }
    if protocol_type.is_some() || upstream_base_url.is_some() || static_headers_json.is_some() {
        let protocol = normalize_protocol_type(
            protocol_type.or_else(|| Some(existing.protocol_type.clone())),
        )?;
        let (client_type, protocol_type, auth_scheme) = profile_from_protocol(&protocol)?;
        patch.protocol = Some(ApiKeyProtocolConfig {
            client_type,
            protocol_type,
            auth_scheme,
        });
        if upstream_base_url.is_some() {
            patch.upstream_base_url = Some(normalize_upstream_base_url(upstream_base_url)?);
        }
        if static_headers_json.is_some() {
            patch.static_headers_json = Some(normalize_static_headers_json(static_headers_json)?);
        }
    }
    storage.update_api_key(id.to_owned(), patch).await
}
