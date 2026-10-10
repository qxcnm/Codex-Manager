use codexmanager_core::storage::ModelPriceTierV2;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{EntityTrait, QueryOrder, Set};
use std::collections::HashSet;

/// A threshold-specific model price.  The composite key keeps tiers stable
/// across SQLite, MySQL and PostgreSQL without relying on floating point data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPriceTierRecord {
    pub model_id: String,
    pub tier: ModelPriceTierV2,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "model_price_tiers")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    model_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    min_input_tokens: i64,
    input_microusd_per_1m: i64,
    cached_input_microusd_per_1m: i64,
    cache_write_microusd_per_1m: Option<i64>,
    output_microusd_per_1m: i64,
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

pub struct ModelPriceTiersRepository;

impl ModelPriceTiersRepository {
    pub async fn list_for_model(
        db: &impl ConnectionTrait,
        model_id: &str,
    ) -> Result<Vec<CatalogPriceTierRecord>, DbErr> {
        Entity::find()
            .filter(Column::ModelId.eq(model_id))
            .order_by_asc(Column::MinInputTokens)
            .all(db)
            .await
            .map(|rows| rows.into_iter().map(Into::into).collect())
    }

    /// Replace all tiers for a model. Callers that need atomic replacement
    /// should pass a transaction connection (the same pattern as other
    /// catalog repositories).
    pub async fn replace_for_model(
        db: &impl ConnectionTrait,
        model_id: &str,
        tiers: &[CatalogPriceTierRecord],
    ) -> Result<(), DbErr> {
        if model_id.trim().is_empty() || tiers.iter().any(|row| row.model_id != model_id) {
            return Err(DbErr::Custom("invalid model price tier model".into()));
        }
        let mut thresholds = HashSet::new();
        for row in tiers {
            let tier = &row.tier;
            if tier.min_input_tokens < 0
                || tier.input_microusd_per_1m < 0
                || tier.cached_input_microusd_per_1m < 0
                || tier
                    .cache_write_microusd_per_1m
                    .is_some_and(|value| value < 0)
                || tier.output_microusd_per_1m < 0
                || !thresholds.insert(tier.min_input_tokens)
            {
                return Err(DbErr::Custom(
                    "invalid or duplicate model price tier".into(),
                ));
            }
        }
        Entity::delete_many()
            .filter(Column::ModelId.eq(model_id))
            .exec(db)
            .await?;
        for row in tiers {
            let model = ActiveModel {
                model_id: Set(row.model_id.clone()),
                min_input_tokens: Set(row.tier.min_input_tokens),
                input_microusd_per_1m: Set(row.tier.input_microusd_per_1m),
                cached_input_microusd_per_1m: Set(row.tier.cached_input_microusd_per_1m),
                cache_write_microusd_per_1m: Set(row.tier.cache_write_microusd_per_1m),
                output_microusd_per_1m: Set(row.tier.output_microusd_per_1m),
                created_at: Set(row.created_at),
                updated_at: Set(row.updated_at),
            };
            Entity::insert(model)
                .on_conflict(
                    OnConflict::columns([Column::ModelId, Column::MinInputTokens])
                        .update_columns([
                            Column::InputMicrousdPer1m,
                            Column::CachedInputMicrousdPer1m,
                            Column::CacheWriteMicrousdPer1m,
                            Column::OutputMicrousdPer1m,
                            Column::UpdatedAt,
                        ])
                        .to_owned(),
                )
                .exec(db)
                .await?;
        }
        Ok(())
    }
}

impl From<Model> for CatalogPriceTierRecord {
    fn from(model: Model) -> Self {
        Self {
            model_id: model.model_id,
            tier: ModelPriceTierV2 {
                min_input_tokens: model.min_input_tokens,
                input_microusd_per_1m: model.input_microusd_per_1m,
                cached_input_microusd_per_1m: model.cached_input_microusd_per_1m,
                cache_write_microusd_per_1m: model.cache_write_microusd_per_1m,
                output_microusd_per_1m: model.output_microusd_per_1m,
            },
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}
