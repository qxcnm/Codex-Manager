#[path = "model_groups_injected.rs"]
pub(crate) mod injected;
#[path = "model_groups_seaorm.rs"]
mod seaorm;
use std::collections::{HashMap, HashSet};

use codexmanager_core::rpc::types::{
    ModelGroupEntry, ModelGroupListResult, ModelGroupModelEntry, ModelGroupModelsSetParams,
    ModelGroupUpsertParams, ModelGroupUsersSetParams, UserModelGroupEntry,
};
use codexmanager_core::storage::{
    now_ts, ApiKeyOwner, ModelGroup, ModelGroupAccess, ModelGroupModel, Storage, UserModelGroup,
};
use rand::RngCore;

use crate::storage_helpers;

fn generate_id(prefix: &str, bytes_len: usize) -> String {
    let mut bytes = vec![0u8; bytes_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    format!(
        "{prefix}_{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_status(value: Option<&str>) -> Result<String, String> {
    match value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("active")
    {
        "active" => Ok("active".to_string()),
        "disabled" => Ok("disabled".to_string()),
        other => Err(format!("unsupported model group status: {other}")),
    }
}

fn normalize_rate(value: Option<i64>) -> i64 {
    value.unwrap_or(1000).clamp(0, 100_000)
}

fn group_entry(group: ModelGroup) -> ModelGroupEntry {
    ModelGroupEntry {
        id: group.id,
        name: group.name,
        description: group.description,
        status: group.status,
        sort: group.sort,
        is_default: group.is_default,
        rate_multiplier_millis: group.rate_multiplier_millis,
        created_at: group.created_at,
        updated_at: group.updated_at,
    }
}

fn group_model_entry(model: ModelGroupModel) -> ModelGroupModelEntry {
    ModelGroupModelEntry {
        group_id: model.group_id,
        platform_model_slug: model.platform_model_slug,
        enabled: model.enabled,
        rate_multiplier_millis: model.rate_multiplier_millis,
        billing_model_slug: model.billing_model_slug,
        note: model.note,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

fn user_group_entry(assignment: UserModelGroup) -> UserModelGroupEntry {
    UserModelGroupEntry {
        user_id: assignment.user_id,
        group_id: assignment.group_id,
        status: assignment.status,
        expires_at: assignment.expires_at,
        created_at: assignment.created_at,
        updated_at: assignment.updated_at,
    }
}

fn result_from_storage(storage: &Storage) -> Result<ModelGroupListResult, String> {
    let groups = storage
        .list_model_groups()
        .map_err(|err| format!("list model groups failed: {err}"))?;
    let models = storage
        .list_model_group_models_v2()
        .map_err(|err| format!("list model group models V2 failed: {err}"))?;
    let user_assignments = storage
        .list_user_model_groups()
        .map_err(|err| format!("list user model groups failed: {err}"))?;
    Ok(ModelGroupListResult {
        groups: groups.into_iter().map(group_entry).collect(),
        models: models.into_iter().map(group_model_entry).collect(),
        user_assignments: user_assignments.into_iter().map(user_group_entry).collect(),
    })
}

pub(crate) fn read_model_groups() -> Result<ModelGroupListResult, String> {
    if storage_helpers::seaorm_enabled() {
        return storage_helpers::seaorm_block_on(seaorm::list);
    }
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    result_from_storage(&storage)
}

pub(crate) fn upsert_model_group(
    params: ModelGroupUpsertParams,
) -> Result<ModelGroupEntry, String> {
    if storage_helpers::seaorm_enabled() {
        return storage_helpers::seaorm_block_on(move |s| seaorm::upsert(s, params));
    }
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let id = normalize_optional_text(params.id.as_deref()).unwrap_or_else(|| generate_id("mg", 8));
    let existing = storage
        .find_model_group(id.as_str())
        .map_err(|err| format!("read model group failed: {err}"))?;
    let name = normalize_optional_text(Some(params.name.as_str()))
        .ok_or_else(|| "模型组名称不能为空".to_string())?;
    let now = now_ts();
    let group = ModelGroup {
        id: id.clone(),
        name,
        description: normalize_optional_text(params.description.as_deref()),
        status: normalize_status(params.status.as_deref())?,
        sort: params
            .sort
            .unwrap_or_else(|| existing.as_ref().map(|item| item.sort).unwrap_or(0)),
        is_default: params
            .is_default
            .unwrap_or_else(|| existing.as_ref().is_some_and(|item| item.is_default)),
        rate_multiplier_millis: normalize_rate(
            params
                .rate_multiplier_millis
                .or_else(|| existing.as_ref().map(|item| item.rate_multiplier_millis)),
        ),
        created_at: existing.as_ref().map(|item| item.created_at).unwrap_or(now),
        updated_at: now,
    };
    storage
        .upsert_model_group(&group)
        .map_err(|err| format!("save model group failed: {err}"))?;
    storage
        .find_model_group(id.as_str())
        .map_err(|err| format!("read model group failed: {err}"))?
        .map(group_entry)
        .ok_or_else(|| "模型组保存结果为空".to_string())
}

pub(crate) fn delete_model_group(id: &str) -> Result<ModelGroupListResult, String> {
    if storage_helpers::seaorm_enabled() {
        let id = id.to_owned();
        return storage_helpers::seaorm_block_on(move |s| seaorm::delete(s, id));
    }
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let group = storage
        .find_model_group(id)
        .map_err(|err| format!("read model group failed: {err}"))?
        .ok_or_else(|| "模型组不存在".to_string())?;
    if group.is_default {
        return Err("默认模型组不能删除".to_string());
    }
    storage
        .delete_model_group(id)
        .map_err(|err| format!("delete model group failed: {err}"))?;
    result_from_storage(&storage)
}

pub(crate) fn set_model_group_models(
    params: ModelGroupModelsSetParams,
) -> Result<ModelGroupListResult, String> {
    if storage_helpers::seaorm_enabled() {
        return storage_helpers::seaorm_block_on(move |s| seaorm::models(s, params));
    }
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let group_id = normalize_optional_text(Some(params.group_id.as_str()))
        .ok_or_else(|| "模型组 ID 不能为空".to_string())?;
    let group = storage
        .find_model_group(group_id.as_str())
        .map_err(|err| format!("read model group failed: {err}"))?
        .ok_or_else(|| "模型组不存在".to_string())?;
    if group.is_default {
        storage
            .replace_model_group_models_v2(group_id.as_str(), &[])
            .map_err(|err| format!("save model group models V2 failed: {err}"))?;
        return result_from_storage(&storage);
    }
    let now = now_ts();
    let mut seen = HashSet::new();
    let mut requested_slugs = Vec::new();
    let mut unique_items = Vec::new();
    for item in params.models {
        let slug = normalize_optional_text(Some(item.platform_model_slug.as_str()))
            .ok_or_else(|| "平台模型不能为空".to_string())?;
        if seen.insert(slug.clone()) {
            requested_slugs.push(slug.clone());
            unique_items.push((item, slug));
        }
    }
    let platform_slugs = storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("list model catalog V2 failed: {err}"))?
        .into_iter()
        .filter(|model| model.enabled && model.supported_in_api)
        .map(|model| model.slug)
        .collect::<HashSet<_>>();
    let mut models = Vec::new();
    for (item, slug) in unique_items {
        if !platform_slugs.contains(slug.as_str()) {
            return Err(format!("平台模型 `{slug}` 不存在"));
        }
        models.push(ModelGroupModel {
            group_id: group_id.clone(),
            platform_model_slug: slug,
            enabled: item.enabled.unwrap_or(true),
            rate_multiplier_millis: item
                .rate_multiplier_millis
                .map(|value| value.clamp(0, 100_000)),
            billing_model_slug: normalize_optional_text(item.billing_model_slug.as_deref()),
            note: normalize_optional_text(item.note.as_deref()),
            created_at: now,
            updated_at: now,
        });
    }
    storage
        .replace_model_group_models_v2(group_id.as_str(), models.as_slice())
        .map_err(|err| format!("save model group models V2 failed: {err}"))?;
    result_from_storage(&storage)
}

pub(crate) fn set_model_group_users(
    params: ModelGroupUsersSetParams,
) -> Result<ModelGroupListResult, String> {
    if storage_helpers::seaorm_enabled() {
        return storage_helpers::seaorm_block_on(move |s| seaorm::users(s, params));
    }
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let group_id = normalize_optional_text(Some(params.group_id.as_str()))
        .ok_or_else(|| "模型组 ID 不能为空".to_string())?;
    let _ = storage
        .find_model_group(group_id.as_str())
        .map_err(|err| format!("read model group failed: {err}"))?
        .ok_or_else(|| "模型组不存在".to_string())?;
    let now = now_ts();
    let mut seen = HashSet::new();
    let mut user_ids = Vec::new();
    for raw_user_id in params.user_ids {
        let user_id = normalize_optional_text(Some(raw_user_id.as_str()))
            .ok_or_else(|| "用户 ID 不能为空".to_string())?;
        if !seen.insert(user_id.clone()) {
            continue;
        }
        user_ids.push(user_id);
    }
    let users = storage
        .list_app_user_access_summaries_for_ids(&user_ids)
        .map_err(|err| format!("read app users failed: {err}"))?
        .into_iter()
        .map(|user| (user.id.clone(), user))
        .collect::<HashMap<_, _>>();
    let mut assignments = Vec::new();
    for user_id in user_ids {
        let user = users
            .get(user_id.as_str())
            .ok_or_else(|| format!("用户 `{user_id}` 不存在"))?;
        if user.role != "member" {
            return Err(format!("用户 `{}` 不是成员账号", user.username));
        }
        assignments.push(UserModelGroup {
            user_id,
            group_id: group_id.clone(),
            status: "active".to_string(),
            expires_at: None,
            created_at: now,
            updated_at: now,
        });
    }
    storage
        .replace_user_model_groups_for_group(group_id.as_str(), assignments.as_slice())
        .map_err(|err| format!("save model group users failed: {err}"))?;
    result_from_storage(&storage)
}

fn user_owner(owner: &ApiKeyOwner) -> Option<&str> {
    if owner.owner_kind != "user" {
        return None;
    }
    owner
        .owner_user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn resolve_api_key_model_group_access(
    storage: &Storage,
    key_id: &str,
    platform_model_slug: &str,
) -> Result<Option<ModelGroupAccess>, String> {
    if storage_helpers::seaorm_enabled() {
        let key = key_id.to_owned();
        let slug = platform_model_slug.to_owned();
        return storage_helpers::seaorm_block_on(move |s| seaorm::access(s, key, slug));
    }
    let requested_model_slug = platform_model_slug.trim();
    let catalog_model_slug = crate::models_v2::policy_catalog_slug(requested_model_slug);
    let owner = storage
        .find_api_key_owner(key_id)
        .map_err(|err| format!("read api key owner failed: {err}"))?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    let Some(user_id) = user_owner(&owner) else {
        return Ok(None);
    };
    let user = storage
        .find_app_user_access_summary_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "API Key 归属用户不存在".to_string())?;
    if user.role == "admin" {
        return Ok(None);
    }
    if user.status != "active" {
        return Err("API Key 归属用户已停用".to_string());
    }
    let access = storage
        .resolve_model_group_access_for_user_v2(user_id, catalog_model_slug, now_ts())
        .map_err(|err| format!("read model group access failed: {err}"))?;
    match access {
        Some(access) => Ok(Some(access)),
        None => Err(format!("model_not_allowed: {requested_model_slug}")),
    }
}

pub(crate) fn allowed_model_slugs_for_api_key(
    storage: &Storage,
    key_id: &str,
) -> Result<Option<HashSet<String>>, String> {
    if storage_helpers::seaorm_enabled() {
        let key = key_id.to_owned();
        return storage_helpers::seaorm_block_on(move |s| seaorm::allowed(s, key));
    }
    let owner = storage
        .find_api_key_owner(key_id)
        .map_err(|err| format!("read api key owner failed: {err}"))?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    let Some(user_id) = user_owner(&owner) else {
        return Ok(None);
    };
    let user = storage
        .find_app_user_access_summary_by_id(user_id)
        .map_err(|err| format!("read app user failed: {err}"))?
        .ok_or_else(|| "API Key 归属用户不存在".to_string())?;
    if user.role == "admin" {
        return Ok(None);
    }
    let slugs = storage
        .allowed_model_slugs_for_user_v2(user_id, now_ts())
        .map_err(|err| format!("read allowed model groups failed: {err}"))?
        .into_iter()
        .collect::<HashSet<_>>();
    Ok(Some(slugs))
}
