use codexmanager_core::storage::ModelFastPolicyV2;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, QueryOrder, QuerySelect, Set};

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogModelRecord {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub description: Option<String>,
    pub provider: Option<String>,
    pub family: Option<String>,
    pub category: Option<String>,
    pub origin: String,
    pub enabled: bool,
    pub supported_in_api: bool,
    pub visibility: String,
    pub sort_order: i64,
    pub context_window: Option<i64>,
    pub max_context_window: Option<i64>,
    pub default_reasoning_effort: Option<String>,
    pub instructions_mode: String,
    pub instructions_text: Option<String>,
    pub builtin_revision: Option<i64>,
    pub user_edited: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub tags: Vec<String>,
    pub capabilities: serde_json::Value,
    pub fast_policy: ModelFastPolicyV2,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    id: String,
    #[sea_orm(column_type = "Text")]
    slug: String,
    #[sea_orm(column_type = "Text")]
    display_name: String,
    #[sea_orm(column_type = "Text", nullable)]
    description: Option<String>,
    provider: Option<String>,
    family: Option<String>,
    category: Option<String>,
    origin: String,
    enabled: bool,
    supported_in_api: bool,
    visibility: String,
    sort_order: i64,
    context_window: Option<i64>,
    max_context_window: Option<i64>,
    default_reasoning_effort: Option<String>,
    instructions_mode: String,
    #[sea_orm(column_type = "Text", nullable)]
    instructions_text: Option<String>,
    builtin_revision: Option<i64>,
    user_edited: bool,
    created_at: i64,
    updated_at: i64,
    #[sea_orm(unique)]
    slug_key: String,
    #[sea_orm(column_type = "Text")]
    tags_json: String,
    #[sea_orm(column_type = "Text")]
    capabilities_json: String,
    fast_policy: String,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}

pub struct ModelCatalogRepository;

impl ModelCatalogRepository {
    pub async fn get(
        db: &impl ConnectionTrait,
        id: &str,
    ) -> Result<Option<CatalogModelRecord>, DbErr> {
        Entity::find_by_id(id)
            .one(db)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    pub async fn find_by_slug(
        db: &impl ConnectionTrait,
        slug: &str,
    ) -> Result<Option<CatalogModelRecord>, DbErr> {
        Entity::find()
            .filter(Column::SlugKey.eq(slug_key(slug)))
            .one(db)
            .await?
            .map(TryInto::try_into)
            .transpose()
    }

    pub async fn list(
        db: &impl ConnectionTrait,
        include_hidden: bool,
        limit: u64,
    ) -> Result<Vec<CatalogModelRecord>, DbErr> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut query = Entity::find()
            .order_by_asc(Column::SortOrder)
            .order_by_asc(Column::Slug)
            .order_by_asc(Column::Id)
            .limit(limit.min(1000));
        if !include_hidden {
            query = query.filter(Column::Visibility.eq("list"));
        }
        query
            .all(db)
            .await?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }

    /// Stable IDs are supplied by the application. A conflicting slug fails,
    /// including on MySQL where ON DUPLICATE KEY would target any unique index.
    pub async fn put(db: &impl ConnectionTrait, record: CatalogModelRecord) -> Result<(), DbErr> {
        validate(&record)?;
        let exists = Entity::find_by_id(&record.id).one(db).await?.is_some();
        let model = ActiveModel {
            slug_key: Set(slug_key(&record.slug)),
            tags_json: Set(serde_json::to_string(&record.tags)
                .map_err(|_| DbErr::Custom("invalid model tags".into()))?),
            capabilities_json: Set(serde_json::to_string(&record.capabilities)
                .map_err(|_| DbErr::Custom("invalid model capabilities".into()))?),
            fast_policy: Set(record.fast_policy.as_str().into()),
            id: Set(record.id),
            slug: Set(record.slug),
            display_name: Set(record.display_name),
            description: Set(record.description),
            provider: Set(record.provider),
            family: Set(record.family),
            category: Set(record.category),
            origin: Set(record.origin),
            enabled: Set(record.enabled),
            supported_in_api: Set(record.supported_in_api),
            visibility: Set(record.visibility),
            sort_order: Set(record.sort_order),
            context_window: Set(record.context_window),
            max_context_window: Set(record.max_context_window),
            default_reasoning_effort: Set(record.default_reasoning_effort),
            instructions_mode: Set(record.instructions_mode),
            instructions_text: Set(record.instructions_text),
            builtin_revision: Set(record.builtin_revision),
            user_edited: Set(record.user_edited),
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

fn slug_key(slug: &str) -> String {
    super::comparison_key(&[&slug.trim().to_ascii_lowercase()])
}

fn validate(model: &CatalogModelRecord) -> Result<(), DbErr> {
    let invalid = model.id.trim().is_empty()
        || model.slug.trim().is_empty()
        || model.display_name.trim().is_empty()
        || !matches!(model.origin.as_str(), "builtin" | "custom")
        || !matches!(model.visibility.as_str(), "list" | "hide")
        || !matches!(
            model.instructions_mode.as_str(),
            "passthrough" | "fallback" | "override"
        )
        || model.context_window.is_some_and(|value| value <= 0)
        || model.max_context_window.is_some_and(|value| value <= 0)
        || model.builtin_revision.is_some_and(|value| value <= 0)
        || (model.origin == "builtin" && model.builtin_revision.is_none())
        || (model.instructions_mode == "override"
            && model
                .instructions_text
                .as_deref()
                .is_none_or(|text| text.trim().is_empty()));
    if invalid {
        return Err(DbErr::Custom("invalid model metadata".into()));
    }
    Ok(())
}

impl TryFrom<Model> for CatalogModelRecord {
    type Error = DbErr;
    fn try_from(model: Model) -> Result<Self, DbErr> {
        Ok(Self {
            tags: serde_json::from_str(&model.tags_json)
                .map_err(|_| DbErr::Custom("invalid stored model tags".into()))?,
            capabilities: serde_json::from_str(&model.capabilities_json)
                .map_err(|_| DbErr::Custom("invalid stored model capabilities".into()))?,
            fast_policy: match model.fast_policy.as_str() {
                "passthrough" => ModelFastPolicyV2::Passthrough,
                "filter" => ModelFastPolicyV2::Filter,
                "force" => ModelFastPolicyV2::Force,
                "block" => ModelFastPolicyV2::Block,
                _ => return Err(DbErr::Custom("invalid stored fast policy".into())),
            },
            id: model.id,
            slug: model.slug,
            display_name: model.display_name,
            description: model.description,
            provider: model.provider,
            family: model.family,
            category: model.category,
            origin: model.origin,
            enabled: model.enabled,
            supported_in_api: model.supported_in_api,
            visibility: model.visibility,
            sort_order: model.sort_order,
            context_window: model.context_window,
            max_context_window: model.max_context_window,
            default_reasoning_effort: model.default_reasoning_effort,
            instructions_mode: model.instructions_mode,
            instructions_text: model.instructions_text,
            builtin_revision: model.builtin_revision,
            user_edited: model.user_edited,
            created_at: model.created_at,
            updated_at: model.updated_at,
        })
    }
}
