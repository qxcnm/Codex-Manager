use sea_orm::{
    entity::prelude::*, ActiveModelTrait, Condition, DatabaseConnection, EntityTrait, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelGroupRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub sort: i64,
    pub is_default: bool,
    pub rate_multiplier_millis: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelGroupModelRecord {
    pub group_id: String,
    pub platform_model_slug: String,
    pub enabled: bool,
    pub rate_multiplier_millis: Option<i64>,
    pub billing_model_slug: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserModelGroupRecord {
    pub user_id: String,
    pub group_id: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub(crate) mod groups {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_groups")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: String,
        pub name: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub description: Option<String>,
        pub status: String,
        pub sort: i64,
        pub is_default: bool,
        pub rate_multiplier_millis: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod group_models {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_group_models")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub group_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub platform_model_slug: String,
        pub enabled: bool,
        pub rate_multiplier_millis: Option<i64>,
        pub billing_model_slug: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub note: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::groups::Entity",
            from = "Column::GroupId",
            to = "super::groups::Column::Id",
            on_delete = "Cascade"
        )]
        Group,
    }
    impl ActiveModelBehavior for ActiveModel {}
}
pub(crate) mod users {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "user_model_groups")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub user_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub group_id: String,
        pub status: String,
        pub expires_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::groups::Entity",
            from = "Column::GroupId",
            to = "super::groups::Column::Id",
            on_delete = "Cascade"
        )]
        Group,
    }
    impl ActiveModelBehavior for ActiveModel {}
}

pub struct ModelGroupsRepository;
impl ModelGroupsRepository {
    pub async fn get(db: &DatabaseConnection, id: &str) -> Result<Option<ModelGroupRecord>, DbErr> {
        groups::Entity::find_by_id(id)
            .one(db)
            .await
            .map(|v| v.map(Into::into))
    }
    pub async fn list(db: &DatabaseConnection, limit: u64) -> Result<Vec<ModelGroupRecord>, DbErr> {
        if limit == 0 {
            return Ok(vec![]);
        };
        Ok(groups::Entity::find()
            .order_by_asc(groups::Column::Sort)
            .order_by_asc(groups::Column::Name)
            .order_by_asc(groups::Column::CreatedAt)
            .limit(limit.min(1000))
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn upsert(db: &DatabaseConnection, r: ModelGroupRecord) -> Result<(), DbErr> {
        if r.id.trim().is_empty()
            || r.name.trim().is_empty()
            || !matches!(r.status.as_str(), "active" | "disabled")
            || r.rate_multiplier_millis < 0
        {
            return Err(DbErr::Custom("invalid model group".into()));
        };
        let t = db.begin().await?;
        crate::UsersRepository::lock(&t, "model_groups").await?;
        if r.is_default {
            groups::Entity::update_many()
                .col_expr(groups::Column::IsDefault, Expr::value(false))
                .filter(groups::Column::Id.ne(r.id.clone()))
                .exec(&t)
                .await?;
        }
        let previous = groups::Entity::find_by_id(&r.id).one(&t).await?;
        let exists = previous.is_some();
        let m = groups::ActiveModel {
            id: Set(r.id),
            name: Set(r.name),
            description: Set(r.description),
            status: Set(r.status),
            sort: Set(r.sort),
            is_default: Set(r.is_default),
            rate_multiplier_millis: Set(r.rate_multiplier_millis),
            created_at: Set(previous
                .as_ref()
                .map(|v| v.created_at)
                .unwrap_or(r.created_at)),
            updated_at: Set(r.updated_at),
        };
        if exists {
            m.update(&t).await?
        } else {
            m.insert(&t).await?
        };
        t.commit().await
    }
    pub async fn delete(db: &DatabaseConnection, id: &str) -> Result<bool, DbErr> {
        Ok(groups::Entity::delete_many()
            .filter(groups::Column::Id.eq(id))
            .filter(groups::Column::IsDefault.eq(false))
            .exec(db)
            .await?
            .rows_affected
            > 0)
    }
    pub async fn replace_models(
        db: &DatabaseConnection,
        gid: &str,
        rs: &[ModelGroupModelRecord],
    ) -> Result<(), DbErr> {
        if rs.iter().any(|r| {
            r.group_id != gid
                || r.platform_model_slug.trim().is_empty()
                || r.rate_multiplier_millis.is_some_and(|v| v < 0)
        }) {
            return Err(DbErr::Custom("invalid model group model".into()));
        };
        let t = db.begin().await?;
        group_models::Entity::delete_many()
            .filter(group_models::Column::GroupId.eq(gid))
            .exec(&t)
            .await?;
        for r in rs {
            group_models::ActiveModel {
                group_id: Set(r.group_id.clone()),
                platform_model_slug: Set(r.platform_model_slug.clone()),
                enabled: Set(r.enabled),
                rate_multiplier_millis: Set(r.rate_multiplier_millis),
                billing_model_slug: Set(r.billing_model_slug.clone()),
                note: Set(r.note.clone()),
                created_at: Set(r.created_at),
                updated_at: Set(r.updated_at),
            }
            .insert(&t)
            .await?;
        }
        t.commit().await
    }
    pub async fn list_models(
        db: &DatabaseConnection,
        gid: &str,
        limit: u64,
    ) -> Result<Vec<ModelGroupModelRecord>, DbErr> {
        if limit == 0 {
            return Ok(vec![]);
        };
        Ok(group_models::Entity::find()
            .filter(group_models::Column::GroupId.eq(gid))
            .order_by_asc(group_models::Column::PlatformModelSlug)
            .limit(limit.min(1000))
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn replace_user_assignments(
        db: &DatabaseConnection,
        gid: &str,
        rs: &[UserModelGroupRecord],
    ) -> Result<(), DbErr> {
        if rs.iter().any(|r| {
            r.group_id != gid
                || r.user_id.trim().is_empty()
                || !matches!(r.status.as_str(), "active" | "disabled")
        }) {
            return Err(DbErr::Custom("invalid user model group".into()));
        };
        let t = db.begin().await?;
        users::Entity::delete_many()
            .filter(users::Column::GroupId.eq(gid))
            .exec(&t)
            .await?;
        for r in rs {
            users::ActiveModel {
                user_id: Set(r.user_id.clone()),
                group_id: Set(r.group_id.clone()),
                status: Set(r.status.clone()),
                expires_at: Set(r.expires_at),
                created_at: Set(r.created_at),
                updated_at: Set(r.updated_at),
            }
            .insert(&t)
            .await?;
        }
        t.commit().await
    }
    pub async fn list_user_assignments(
        db: &DatabaseConnection,
        uid: &str,
        limit: u64,
    ) -> Result<Vec<UserModelGroupRecord>, DbErr> {
        if limit == 0 {
            return Ok(vec![]);
        };
        Ok(users::Entity::find()
            .filter(users::Column::UserId.eq(uid))
            .order_by_asc(users::Column::GroupId)
            .limit(limit.min(1000))
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
}
impl From<groups::Model> for ModelGroupRecord {
    fn from(m: groups::Model) -> Self {
        Self {
            id: m.id,
            name: m.name,
            description: m.description,
            status: m.status,
            sort: m.sort,
            is_default: m.is_default,
            rate_multiplier_millis: m.rate_multiplier_millis,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}
impl From<group_models::Model> for ModelGroupModelRecord {
    fn from(m: group_models::Model) -> Self {
        Self {
            group_id: m.group_id,
            platform_model_slug: m.platform_model_slug,
            enabled: m.enabled,
            rate_multiplier_millis: m.rate_multiplier_millis,
            billing_model_slug: m.billing_model_slug,
            note: m.note,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}
impl From<users::Model> for UserModelGroupRecord {
    fn from(m: users::Model) -> Self {
        Self {
            user_id: m.user_id,
            group_id: m.group_id,
            status: m.status,
            expires_at: m.expires_at,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::StorageBackendKind;

    #[tokio::test]
    async fn model_groups_round_trip_and_replace_assignments() {
        let storage = crate::SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        ModelGroupsRepository::upsert(
            storage.connection(),
            ModelGroupRecord {
                id: "group-a".into(),
                name: "默认组".into(),
                description: Some("fixture".into()),
                status: "active".into(),
                sort: 1,
                is_default: true,
                rate_multiplier_millis: 1000,
                created_at: 1,
                updated_at: 1,
            },
        )
        .await
        .unwrap();
        ModelGroupsRepository::upsert(
            storage.connection(),
            ModelGroupRecord {
                id: "group-b".into(),
                name: "付费组".into(),
                description: None,
                status: "active".into(),
                sort: 2,
                is_default: true,
                rate_multiplier_millis: 1250,
                created_at: 2,
                updated_at: 2,
            },
        )
        .await
        .unwrap();
        assert!(
            !ModelGroupsRepository::get(storage.connection(), "group-a")
                .await
                .unwrap()
                .unwrap()
                .is_default
        );
        let model = ModelGroupModelRecord {
            group_id: "group-b".into(),
            platform_model_slug: "gpt-5".into(),
            enabled: true,
            rate_multiplier_millis: Some(1100),
            billing_model_slug: Some("gpt-5".into()),
            note: Some("fixture".into()),
            created_at: 3,
            updated_at: 3,
        };
        ModelGroupsRepository::replace_models(storage.connection(), "group-b", &[model.clone()])
            .await
            .unwrap();
        assert_eq!(
            ModelGroupsRepository::list_models(storage.connection(), "group-b", 10)
                .await
                .unwrap(),
            vec![model]
        );
        let assignment = UserModelGroupRecord {
            user_id: "user-a".into(),
            group_id: "group-b".into(),
            status: "active".into(),
            expires_at: None,
            created_at: 4,
            updated_at: 4,
        };
        ModelGroupsRepository::replace_user_assignments(
            storage.connection(),
            "group-b",
            std::slice::from_ref(&assignment),
        )
        .await
        .unwrap();
        assert_eq!(
            ModelGroupsRepository::list_user_assignments(storage.connection(), "user-a", 10)
                .await
                .unwrap(),
            vec![assignment]
        );
        assert!(ModelGroupsRepository::replace_models(
            storage.connection(),
            "group-b",
            &[ModelGroupModelRecord {
                group_id: "other".into(),
                platform_model_slug: "bad".into(),
                enabled: true,
                rate_multiplier_millis: None,
                billing_model_slug: None,
                note: None,
                created_at: 0,
                updated_at: 0,
            }]
        )
        .await
        .is_err());
    }
}
pub(crate) mod group_models_v2 {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_group_models_v2")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub group_id: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub model_id: String,
        pub enabled: bool,
        pub rate_multiplier_millis: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::groups::Entity",
            from = "Column::GroupId",
            to = "super::groups::Column::Id",
            on_delete = "Cascade"
        )]
        Group,
        #[sea_orm(
            belongs_to = "crate::model_catalog::models::Entity",
            from = "Column::ModelId",
            to = "crate::model_catalog::models::Column::Id",
            on_delete = "Cascade"
        )]
        Model,
    }
    impl ActiveModelBehavior for ActiveModel {}
}

impl ModelGroupsRepository {
    pub async fn list_all(db: &impl ConnectionTrait) -> Result<Vec<ModelGroupRecord>, DbErr> {
        Ok(groups::Entity::find()
            .order_by_asc(groups::Column::Sort)
            .order_by_asc(groups::Column::Name)
            .order_by_asc(groups::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn list_all_user_assignments(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<UserModelGroupRecord>, DbErr> {
        Ok(users::Entity::find()
            .order_by_asc(users::Column::UserId)
            .order_by_asc(users::Column::GroupId)
            .all(db)
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub async fn list_models_v2(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<ModelGroupModelRecord>, DbErr> {
        let rows = group_models_v2::Entity::find()
            .order_by_asc(group_models_v2::Column::GroupId)
            .all(db)
            .await?;
        let mut result = Vec::new();
        for row in rows {
            if let Some(model) = crate::ModelCatalogRepository::get(db, &row.model_id).await? {
                result.push(ModelGroupModelRecord {
                    group_id: row.group_id,
                    platform_model_slug: model.slug,
                    enabled: row.enabled,
                    rate_multiplier_millis: row.rate_multiplier_millis,
                    billing_model_slug: None,
                    note: Some("model_catalog_v2".into()),
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                });
            }
        }
        Ok(result)
    }
    pub async fn replace_models_v2(
        db: &DatabaseConnection,
        group_id: &str,
        models: &[ModelGroupModelRecord],
    ) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        let group = groups::Entity::find_by_id(group_id)
            .one(&tx)
            .await?
            .ok_or_else(|| DbErr::Custom("model_group_not_found".into()))?;
        if group.is_default && !models.is_empty() {
            return Err(DbErr::Custom(
                "default model group uses all_enabled_models".into(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        let mut records = Vec::new();
        for row in models {
            if row.group_id != group_id || row.rate_multiplier_millis.is_some_and(|rate| rate < 0) {
                return Err(DbErr::Custom("invalid model group model".into()));
            }
            let model = crate::ModelCatalogRepository::find_by_slug(&tx, &row.platform_model_slug)
                .await?
                .filter(|m| m.enabled && m.supported_in_api)
                .ok_or_else(|| DbErr::Custom("model_not_found".into()))?;
            if !seen.insert(model.id.clone()) {
                return Err(DbErr::Custom("duplicate model group model".into()));
            }
            let price = crate::ModelPricesRepository::get(&tx, &model.id)
                .await?
                .ok_or_else(|| DbErr::Custom("model_price_missing".into()))?;
            if price.price.price_status == "missing" {
                return Err(DbErr::Custom("model_price_missing".into()));
            }
            records.push(group_models_v2::ActiveModel {
                group_id: Set(group_id.to_owned()),
                model_id: Set(model.id),
                enabled: Set(row.enabled),
                rate_multiplier_millis: Set(row.rate_multiplier_millis),
                created_at: Set(row.created_at),
                updated_at: Set(row.updated_at),
            });
        }
        group_models_v2::Entity::delete_many()
            .filter(group_models_v2::Column::GroupId.eq(group_id))
            .exec(&tx)
            .await?;
        for row in records {
            row.insert(&tx).await?;
        }
        tx.commit().await
    }
    pub async fn resolve_access_v2(
        db: &impl ConnectionTrait,
        user_id: &str,
        slug: &str,
        now: i64,
    ) -> Result<Option<codexmanager_core::storage::ModelGroupAccess>, DbErr> {
        let user = crate::UsersRepository::get(db, user_id)
            .await?
            .ok_or_else(|| DbErr::Custom("model_group_user_missing".into()))?;
        if user.status != "active" {
            return Ok(None);
        }
        let Some(model) = crate::ModelCatalogRepository::find_by_slug(db, slug)
            .await?
            .filter(|m| m.enabled && m.supported_in_api)
        else {
            return Ok(None);
        };
        let Some(price) = crate::ModelPricesRepository::get(db, &model.id).await? else {
            return Ok(None);
        };
        let assignments = users::Entity::find()
            .filter(users::Column::UserId.eq(user_id))
            .filter(users::Column::Status.eq("active"))
            .filter(
                Condition::any()
                    .add(users::Column::ExpiresAt.is_null())
                    .add(users::Column::ExpiresAt.gt(now)),
            )
            .order_by_asc(users::Column::GroupId)
            .all(db)
            .await?;
        let mut best: Option<codexmanager_core::storage::ModelGroupAccess> = None;
        for assignment in assignments {
            let Some(group) = groups::Entity::find_by_id(&assignment.group_id)
                .one(db)
                .await?
                .filter(|g| g.status == "active")
            else {
                continue;
            };
            let membership =
                group_models_v2::Entity::find_by_id((group.id.clone(), model.id.clone()))
                    .one(db)
                    .await?
                    .filter(|m| m.enabled);
            if !group.is_default && (membership.is_none() || price.price.price_status == "missing")
            {
                continue;
            }
            let rate = i128::from(group.rate_multiplier_millis.max(0))
                * i128::from(
                    membership
                        .and_then(|r| r.rate_multiplier_millis)
                        .unwrap_or(1000)
                        .max(0),
                )
                / 1000;
            let rate = i64::try_from(rate)
                .map_err(|_| DbErr::Custom("model group multiplier overflow".into()))?;
            let access = codexmanager_core::storage::ModelGroupAccess {
                group_id: group.id,
                group_name: group.name,
                platform_model_slug: model.slug.clone(),
                rate_multiplier_millis: rate,
                billing_model_slug: None,
            };
            if best
                .as_ref()
                .is_none_or(|v| rate < v.rate_multiplier_millis)
            {
                best = Some(access);
            }
        }
        Ok(best)
    }
    pub async fn allowed_slugs_v2(
        db: &impl ConnectionTrait,
        user_id: &str,
        now: i64,
    ) -> Result<Vec<String>, DbErr> {
        // Do not apply a UI pagination limit to an authorization decision.
        let models = crate::model_catalog::models::Entity::find().all(db).await?;
        let mut slugs = Vec::new();
        for model in models {
            let record: crate::CatalogModelRecord = model.try_into()?;
            if Self::resolve_access_v2(db, user_id, &record.slug, now)
                .await?
                .is_some()
            {
                slugs.push(record.slug);
            }
        }
        slugs.sort();
        slugs.dedup();
        Ok(slugs)
    }
}
impl ModelGroupsRepository {
    pub async fn assign_default(
        db: &impl ConnectionTrait,
        uid: &str,
        now: i64,
    ) -> Result<(), DbErr> {
        crate::UsersRepository::lock(db, "model_groups").await?;
        if groups::Entity::find().one(db).await?.is_none() {
            groups::ActiveModel {
                id: Set("mg_default".into()),
                name: Set("默认模型组".into()),
                description: Set(Some("全部已启用的 API 模型".into())),
                status: Set("active".into()),
                sort: Set(0),
                is_default: Set(true),
                rate_multiplier_millis: Set(1000),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(db)
            .await?;
        }
        if let Some(group) = groups::Entity::find()
            .filter(groups::Column::IsDefault.eq(true))
            .one(db)
            .await?
        {
            users::Entity::insert(users::ActiveModel {
                user_id: Set(uid.into()),
                group_id: Set(group.id),
                status: Set("active".into()),
                expires_at: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .on_conflict(
                sea_orm::sea_query::OnConflict::columns([
                    users::Column::UserId,
                    users::Column::GroupId,
                ])
                .do_nothing_on([users::Column::UserId, users::Column::GroupId])
                .to_owned(),
            )
            .do_nothing()
            .exec(db)
            .await?;
        }
        Ok(())
    }
}
