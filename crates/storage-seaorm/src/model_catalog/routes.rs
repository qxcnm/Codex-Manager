use codexmanager_core::storage::ModelRouteV2;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, QueryOrder, QuerySelect, Set};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRouteRecord {
    pub model_id: String,
    pub route: ModelRouteV2,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "model_routes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    id: String,
    model_id: String,
    source_kind: String,
    #[sea_orm(column_type = "Text")]
    source_id: String,
    #[sea_orm(column_type = "Text")]
    upstream_model: String,
    #[sea_orm(unique)]
    route_key: String,
    enabled: bool,
    priority: i64,
    weight: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::models::Entity",
        from = "Column::ModelId",
        to = "super::models::Column::Id",
        on_delete = "Cascade"
    )]
    CatalogModel,
}
impl ActiveModelBehavior for ActiveModel {}

pub struct ModelRoutesRepository;

impl ModelRoutesRepository {
    pub async fn get(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<CatalogRouteRecord>, DbErr> {
        Ok(Entity::find_by_id(id).one(db).await?.map(Into::into))
    }

    pub async fn list_for_model(
        db: &impl ConnectionTrait,
        model_id: &str,
        enabled_only: bool,
        limit: u64,
    ) -> Result<Vec<CatalogRouteRecord>, DbErr> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut query = Entity::find()
            .filter(Column::ModelId.eq(model_id))
            .order_by_desc(Column::Priority)
            .order_by_asc(Column::Id)
            .limit(limit.min(1000));
        if enabled_only {
            query = query.filter(Column::Enabled.eq(true));
        }
        Ok(query.all(db).await?.into_iter().map(Into::into).collect())
    }

    pub async fn put(db: &impl ConnectionTrait, record: CatalogRouteRecord) -> Result<(), DbErr> {
        let route = record.route;
        if record.model_id.trim().is_empty()
            || route.id.trim().is_empty()
            || !matches!(route.source_kind.as_str(), "account_pool" | "aggregate_api")
            || route.source_id.trim().is_empty()
            || route.upstream_model.trim().is_empty()
            || route.weight <= 0
        {
            return Err(DbErr::Custom("invalid model route".into()));
        }
        let exists = Entity::find_by_id(&route.id).one(db).await?.is_some();
        let model = ActiveModel {
            route_key: Set(super::comparison_key(&[
                &record.model_id,
                &route.source_kind,
                &route.source_id,
                &route.upstream_model,
            ])),
            id: Set(route.id),
            model_id: Set(record.model_id),
            source_kind: Set(route.source_kind),
            source_id: Set(route.source_id),
            upstream_model: Set(route.upstream_model),
            enabled: Set(route.enabled),
            priority: Set(route.priority),
            weight: Set(route.weight),
            created_at: Set(record.created_at),
            updated_at: Set(record.updated_at),
        };
        if exists {
            model.update(db).await?;
        } else {
            model.insert(db).await?;
        }
        Ok(())
    }

    pub async fn delete(db: &impl ConnectionTrait, id: &str) -> Result<bool, DbErr> {
        Ok(Entity::delete_by_id(id).exec(db).await?.rows_affected > 0)
    }
}

impl From<Model> for CatalogRouteRecord {
    fn from(model: Model) -> Self {
        Self {
            model_id: model.model_id,
            route: ModelRouteV2 {
                id: model.id,
                source_kind: model.source_kind,
                source_id: model.source_id,
                upstream_model: model.upstream_model,
                enabled: model.enabled,
                priority: model.priority,
                weight: model.weight,
            },
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}
