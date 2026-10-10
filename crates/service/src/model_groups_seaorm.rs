use super::*;
use codexmanager_storage_seaorm::{
    ModelCatalogRepository as Catalog, ModelGroupModelRecord, ModelGroupRecord,
    ModelGroupsRepository as Groups, SeaOrmStorage, UserModelGroupRecord, UsersRepository as Users,
};

fn failure(error: impl std::fmt::Display) -> String {
    format!("model group storage failed: {error}")
}
fn entry(g: ModelGroupRecord) -> ModelGroupEntry {
    ModelGroupEntry {
        id: g.id,
        name: g.name,
        description: g.description,
        status: g.status,
        sort: g.sort,
        is_default: g.is_default,
        rate_multiplier_millis: g.rate_multiplier_millis,
        created_at: g.created_at,
        updated_at: g.updated_at,
    }
}
pub(super) async fn list(storage: SeaOrmStorage) -> Result<ModelGroupListResult, String> {
    let db = storage.connection();
    Ok(ModelGroupListResult {
        groups: Groups::list_all(db)
            .await
            .map_err(failure)?
            .into_iter()
            .map(entry)
            .collect(),
        models: Groups::list_models_v2(db)
            .await
            .map_err(failure)?
            .into_iter()
            .map(|m| ModelGroupModelEntry {
                group_id: m.group_id,
                platform_model_slug: m.platform_model_slug,
                enabled: m.enabled,
                rate_multiplier_millis: m.rate_multiplier_millis,
                billing_model_slug: m.billing_model_slug,
                note: m.note,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect(),
        user_assignments: Groups::list_all_user_assignments(db)
            .await
            .map_err(failure)?
            .into_iter()
            .map(|m| UserModelGroupEntry {
                user_id: m.user_id,
                group_id: m.group_id,
                status: m.status,
                expires_at: m.expires_at,
                created_at: m.created_at,
                updated_at: m.updated_at,
            })
            .collect(),
    })
}
pub(super) async fn upsert(
    storage: SeaOrmStorage,
    params: ModelGroupUpsertParams,
) -> Result<ModelGroupEntry, String> {
    let db = storage.connection();
    let id = normalize_optional_text(params.id.as_deref()).unwrap_or_else(|| generate_id("mg", 8));
    let previous = Groups::get(db, &id).await.map_err(failure)?;
    let now = now_ts();
    let row = ModelGroupRecord {
        id: id.clone(),
        name: normalize_optional_text(Some(&params.name)).ok_or("模型组名称不能为空")?,
        description: normalize_optional_text(params.description.as_deref()),
        status: normalize_status(params.status.as_deref())?,
        sort: params
            .sort
            .unwrap_or_else(|| previous.as_ref().map(|v| v.sort).unwrap_or(0)),
        is_default: params
            .is_default
            .unwrap_or_else(|| previous.as_ref().is_some_and(|v| v.is_default)),
        rate_multiplier_millis: normalize_rate(
            params
                .rate_multiplier_millis
                .or_else(|| previous.as_ref().map(|v| v.rate_multiplier_millis)),
        ),
        created_at: previous.as_ref().map(|v| v.created_at).unwrap_or(now),
        updated_at: now,
    };
    Groups::upsert(db, row.clone()).await.map_err(failure)?;
    Ok(entry(row))
}
pub(super) async fn delete(
    storage: SeaOrmStorage,
    id: String,
) -> Result<ModelGroupListResult, String> {
    let group = Groups::get(storage.connection(), &id)
        .await
        .map_err(failure)?
        .ok_or("模型组不存在")?;
    if group.is_default {
        return Err("默认模型组不能删除".into());
    }
    Groups::delete(storage.connection(), &id)
        .await
        .map_err(failure)?;
    list(storage).await
}
pub(super) async fn models(
    storage: SeaOrmStorage,
    params: ModelGroupModelsSetParams,
) -> Result<ModelGroupListResult, String> {
    let db = storage.connection();
    let group = Groups::get(db, params.group_id.trim())
        .await
        .map_err(failure)?
        .ok_or("模型组不存在")?;
    let now = now_ts();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    if !group.is_default {
        for m in params.models {
            let slug =
                normalize_optional_text(Some(&m.platform_model_slug)).ok_or("平台模型不能为空")?;
            if !seen.insert(slug.to_ascii_lowercase()) {
                continue;
            }
            let model = Catalog::find_by_slug(db, &slug)
                .await
                .map_err(failure)?
                .filter(|v| v.enabled && v.supported_in_api)
                .ok_or("平台模型不存在")?;
            rows.push(ModelGroupModelRecord {
                group_id: group.id.clone(),
                platform_model_slug: model.slug,
                enabled: m.enabled.unwrap_or(true),
                rate_multiplier_millis: m.rate_multiplier_millis.map(|v| v.clamp(0, 100_000)),
                billing_model_slug: None,
                note: Some("model_catalog_v2".into()),
                created_at: now,
                updated_at: now,
            });
        }
    }
    Groups::replace_models_v2(db, &group.id, &rows)
        .await
        .map_err(failure)?;
    list(storage).await
}
pub(super) async fn users(
    storage: SeaOrmStorage,
    params: ModelGroupUsersSetParams,
) -> Result<ModelGroupListResult, String> {
    let db = storage.connection();
    let group = Groups::get(db, params.group_id.trim())
        .await
        .map_err(failure)?
        .ok_or("模型组不存在")?;
    let now = now_ts();
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for id in params.user_ids {
        let id = id.trim();
        if id.is_empty() {
            return Err("用户 ID 不能为空".into());
        }
        if !seen.insert(id.to_owned()) {
            continue;
        }
        let user = Users::get(db, id)
            .await
            .map_err(failure)?
            .ok_or("用户不存在")?;
        if user.role != "member" {
            return Err("用户不是成员账号".into());
        }
        rows.push(UserModelGroupRecord {
            user_id: id.into(),
            group_id: group.id.clone(),
            status: "active".into(),
            expires_at: None,
            created_at: now,
            updated_at: now,
        });
    }
    Groups::replace_user_assignments(db, &group.id, &rows)
        .await
        .map_err(failure)?;
    list(storage).await
}
pub(super) async fn access(
    storage: SeaOrmStorage,
    key: String,
    slug: String,
) -> Result<Option<ModelGroupAccess>, String> {
    let db = storage.connection();
    let Some(owner) = Users::owner(db, &key).await.map_err(failure)? else {
        return Ok(None);
    };
    let Some(uid) = user_owner(&owner) else {
        return Ok(None);
    };
    let user = Users::get(db, uid)
        .await
        .map_err(failure)?
        .ok_or("API Key 归属用户不存在")?;
    if user.status != "active" {
        return Err("API Key 归属用户已停用".into());
    }
    if user.role == "admin" {
        return Ok(None);
    }
    Groups::resolve_access_v2(
        db,
        uid,
        crate::models_v2::policy_catalog_slug(&slug),
        now_ts(),
    )
    .await
    .map_err(failure)?
    .map(Some)
    .ok_or_else(|| format!("model_not_allowed: {slug}"))
}
pub(super) async fn allowed(
    storage: SeaOrmStorage,
    key: String,
) -> Result<Option<HashSet<String>>, String> {
    let db = storage.connection();
    let Some(owner) = Users::owner(db, &key).await.map_err(failure)? else {
        return Ok(None);
    };
    let Some(uid) = user_owner(&owner) else {
        return Ok(None);
    };
    let user = Users::get(db, uid)
        .await
        .map_err(failure)?
        .ok_or("API Key 归属用户不存在")?;
    if user.status != "active" {
        return Err("API Key 归属用户已停用".into());
    }
    if user.role == "admin" {
        return Ok(None);
    }
    Ok(Some(
        Groups::allowed_slugs_v2(db, uid, now_ts())
            .await
            .map_err(failure)?
            .into_iter()
            .collect(),
    ))
}
