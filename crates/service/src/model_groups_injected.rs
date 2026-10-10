//! Model-group application rules shared by injected SQLite and SeaORM stores.
use super::*;
use codexmanager_core::storage::DomainStorage;

pub(crate) async fn list(storage: &dyn DomainStorage) -> Result<ModelGroupListResult, String> {
    Ok(ModelGroupListResult {
        groups: storage
            .groups()
            .await?
            .into_iter()
            .map(group_entry)
            .collect(),
        models: storage
            .group_models()
            .await?
            .into_iter()
            .map(group_model_entry)
            .collect(),
        user_assignments: storage
            .group_users()
            .await?
            .into_iter()
            .map(user_group_entry)
            .collect(),
    })
}

pub(crate) async fn save(
    storage: &dyn DomainStorage,
    params: ModelGroupUpsertParams,
) -> Result<ModelGroupEntry, String> {
    let id = normalize_optional_text(params.id.as_deref()).unwrap_or_else(|| generate_id("mg", 8));
    let existing = storage.group(id.clone()).await?;
    let now = now_ts();
    let group = ModelGroup {
        id: id.clone(),
        name: normalize_optional_text(Some(&params.name)).ok_or("模型组名称不能为空")?,
        description: normalize_optional_text(params.description.as_deref()),
        status: normalize_status(params.status.as_deref())?,
        sort: params
            .sort
            .unwrap_or_else(|| existing.as_ref().map(|v| v.sort).unwrap_or(0)),
        is_default: params
            .is_default
            .unwrap_or_else(|| existing.as_ref().is_some_and(|v| v.is_default)),
        rate_multiplier_millis: normalize_rate(
            params
                .rate_multiplier_millis
                .or_else(|| existing.as_ref().map(|v| v.rate_multiplier_millis)),
        ),
        created_at: existing.as_ref().map(|v| v.created_at).unwrap_or(now),
        updated_at: now,
    };
    storage.save_group(group).await?;
    storage
        .group(id)
        .await?
        .map(group_entry)
        .ok_or_else(|| "模型组保存结果为空".to_owned())
}

pub(crate) async fn delete(
    storage: &dyn DomainStorage,
    id: String,
) -> Result<ModelGroupListResult, String> {
    let group = storage.group(id.clone()).await?.ok_or("模型组不存在")?;
    if group.is_default {
        return Err("默认模型组不能删除".into());
    }
    storage.remove_group(id).await?;
    list(storage).await
}

pub(crate) async fn models(
    storage: &dyn DomainStorage,
    params: ModelGroupModelsSetParams,
) -> Result<ModelGroupListResult, String> {
    let id = normalize_optional_text(Some(&params.group_id)).ok_or("模型组 ID 不能为空")?;
    let group = storage.group(id.clone()).await?.ok_or("模型组不存在")?;
    let mut rows = Vec::new();
    if !group.is_default {
        let available = storage
            .models(true)
            .await?
            .into_iter()
            .filter(|model| model.enabled && model.supported_in_api)
            .map(|model| model.slug)
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let now = now_ts();
        for item in params.models {
            let slug = normalize_optional_text(Some(&item.platform_model_slug))
                .ok_or("平台模型不能为空")?;
            if !seen.insert(slug.clone()) {
                continue;
            }
            if !available.contains(&slug) {
                return Err(format!("平台模型 `{slug}` 不存在"));
            }
            rows.push(ModelGroupModel {
                group_id: id.clone(),
                platform_model_slug: slug,
                enabled: item.enabled.unwrap_or(true),
                rate_multiplier_millis: item.rate_multiplier_millis.map(|v| v.clamp(0, 100_000)),
                billing_model_slug: normalize_optional_text(item.billing_model_slug.as_deref()),
                note: normalize_optional_text(item.note.as_deref()),
                created_at: now,
                updated_at: now,
            });
        }
    }
    storage.set_group_models(id, rows).await?;
    list(storage).await
}

pub(crate) async fn users(
    storage: &dyn DomainStorage,
    params: ModelGroupUsersSetParams,
) -> Result<ModelGroupListResult, String> {
    let id = normalize_optional_text(Some(&params.group_id)).ok_or("模型组 ID 不能为空")?;
    storage.group(id.clone()).await?.ok_or("模型组不存在")?;
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let now = now_ts();
    for user_id in params.user_ids {
        let user_id = normalize_optional_text(Some(&user_id)).ok_or("用户 ID 不能为空")?;
        if !seen.insert(user_id.clone()) {
            continue;
        }
        let user = storage
            .user(user_id.clone())
            .await?
            .ok_or_else(|| format!("用户 `{user_id}` 不存在"))?;
        if user.role != "member" {
            return Err(format!("用户 `{}` 不是成员账号", user.username));
        }
        rows.push(UserModelGroup {
            user_id,
            group_id: id.clone(),
            status: "active".into(),
            expires_at: None,
            created_at: now,
            updated_at: now,
        });
    }
    storage.set_group_users(id, rows).await?;
    list(storage).await
}
