use codexmanager_core::storage::ModelPriceV2;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::Set;

/// Base model prices only. Tiered price selection and ledger charging remain
/// the responsibility of their independent domains; all rates are integer micro-USD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogPriceRecord {
    pub model_id: String,
    pub price: ModelPriceV2,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "model_prices")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    model_id: String,
    currency: String,
    input_microusd_per_1m: Option<i64>,
    cached_input_microusd_per_1m: Option<i64>,
    cache_write_microusd_per_1m: Option<i64>,
    output_microusd_per_1m: Option<i64>,
    price_status: String,
    #[sea_orm(column_type = "Text", nullable)]
    price_source: Option<String>,
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

pub struct ModelPricesRepository;

impl ModelPricesRepository {
    pub async fn get(
        db: &impl ConnectionTrait,
        model_id: &str,
    ) -> Result<Option<CatalogPriceRecord>, DbErr> {
        Ok(Entity::find_by_id(model_id).one(db).await?.map(Into::into))
    }

    pub async fn put(db: &impl ConnectionTrait, record: CatalogPriceRecord) -> Result<(), DbErr> {
        let price = record.price;
        let required = [
            price.input_microusd_per_1m,
            price.cached_input_microusd_per_1m,
            price.output_microusd_per_1m,
        ];
        if !matches!(
            price.price_status.as_str(),
            "official" | "estimated" | "custom" | "missing"
        ) || required.iter().flatten().any(|rate| *rate < 0)
            || price
                .cache_write_microusd_per_1m
                .is_some_and(|rate| rate < 0)
            || (price.price_status == "missing"
                && (required.iter().any(Option::is_some)
                    || price.cache_write_microusd_per_1m.is_some()))
            || (price.price_status != "missing" && required.iter().any(Option::is_none))
        {
            return Err(DbErr::Custom("invalid model base price".into()));
        }
        let model = ActiveModel {
            model_id: Set(record.model_id),
            currency: Set("USD".into()),
            input_microusd_per_1m: Set(price.input_microusd_per_1m),
            cached_input_microusd_per_1m: Set(price.cached_input_microusd_per_1m),
            cache_write_microusd_per_1m: Set(price.cache_write_microusd_per_1m),
            output_microusd_per_1m: Set(price.output_microusd_per_1m),
            price_status: Set(price.price_status),
            price_source: Set(price.price_source),
            created_at: Set(record.created_at),
            updated_at: Set(record.updated_at),
        };
        Entity::insert(model)
            .on_conflict(
                OnConflict::column(Column::ModelId)
                    .update_columns([
                        Column::InputMicrousdPer1m,
                        Column::CachedInputMicrousdPer1m,
                        Column::CacheWriteMicrousdPer1m,
                        Column::OutputMicrousdPer1m,
                        Column::PriceStatus,
                        Column::PriceSource,
                        Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await
            .map(|_| ())
    }
}

impl From<Model> for CatalogPriceRecord {
    fn from(model: Model) -> Self {
        Self {
            model_id: model.model_id,
            price: ModelPriceV2 {
                price_status: model.price_status,
                price_source: model.price_source,
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
