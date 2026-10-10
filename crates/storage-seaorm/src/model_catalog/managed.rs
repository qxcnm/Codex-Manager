//! Transactional model aggregate, sharing the desktop validation contract.
use super::{models, routes};
use crate::{
    CatalogModelRecord, CatalogPriceRecord, CatalogPriceTierRecord, CatalogRouteRecord,
    ModelCatalogRepository, ModelPriceTiersRepository, ModelPricesRepository,
    ModelRoutesRepository,
};
use codexmanager_core::storage::{
    now_ts, validate_managed_model_price_v2, validate_managed_model_v2,
    ManagedModelBatchStateV2Update, ManagedModelPriceV2Update, ManagedModelV2,
    ManagedModelV2Upsert, ModelCatalogV2Stats,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set, TransactionTrait,
};
use sha2::{Digest, Sha256};

pub struct ManagedModelsRepository;

impl ManagedModelsRepository {
    pub async fn ensure_routes(
        db: &DatabaseConnection,
        candidates: &[ManagedModelV2Upsert],
        inputs: &[codexmanager_core::storage::ManagedModelRouteEnsureV2],
    ) -> Result<codexmanager_core::storage::ManagedModelRouteEnsureResultV2, DbErr> {
        let mut seen = std::collections::HashSet::new();
        for candidate in candidates {
            if candidate.model.slug.trim().is_empty()
                || !seen.insert(candidate.model.slug.trim().to_ascii_lowercase())
                || candidate.previous_slug.is_some()
                || !candidate.model.id.trim().is_empty()
                || !candidate.model.routes.is_empty()
            {
                return Err(DbErr::Custom(
                    "invalid managed model creation candidate".into(),
                ));
            }
        }
        let tx = db.begin().await?;
        crate::UsersRepository::lock(&tx, "model_groups").await?;
        let mut result = codexmanager_core::storage::ManagedModelRouteEnsureResultV2::default();
        for input in inputs {
            let slug = input.model_slug.trim();
            if slug.is_empty() {
                return Err(DbErr::Custom("managed model route slug is required".into()));
            }
            let mut route = input.route.clone();
            route.source_kind = route.source_kind.trim().into();
            route.source_id = route.source_id.trim().into();
            route.upstream_model = route.upstream_model.trim().into();
            if !matches!(route.source_kind.as_str(), "account_pool" | "aggregate_api")
                || route.source_id.is_empty()
                || route.upstream_model.is_empty()
                || route.weight <= 0
            {
                return Err(DbErr::Custom("invalid model route".into()));
            }
            let model = match Self::get(&tx, slug).await? {
                Some(model) => model,
                None => {
                    let candidate = candidates
                        .iter()
                        .find(|c| c.model.slug.trim().eq_ignore_ascii_case(slug))
                        .ok_or_else(|| {
                            DbErr::Custom("missing managed model creation candidate".into())
                        })?;
                    let model = Self::write(&tx, candidate).await?;
                    result.created_models.push(model.slug.clone());
                    model
                }
            };
            if model.routes.iter().any(|r| {
                r.source_kind == route.source_kind
                    && r.source_id == route.source_id
                    && r.upstream_model.eq_ignore_ascii_case(&route.upstream_model)
            }) {
                result.unchanged_routes.push(model.slug);
                continue;
            }
            route.id = format!(
                "route:{}",
                stable_hash(&format!(
                    "{}\0{}\0{}\0{}",
                    model.id, route.source_kind, route.source_id, route.upstream_model
                ))
            );
            if ModelRoutesRepository::get(&tx, &route.id).await?.is_some() {
                return Err(DbErr::Custom("managed model route id collision".into()));
            }
            let now = now_ts();
            ModelRoutesRepository::put(
                &tx,
                CatalogRouteRecord {
                    model_id: model.id,
                    route,
                    created_at: now,
                    updated_at: now,
                },
            )
            .await?;
            result.added_routes.push(model.slug);
        }
        tx.commit().await?;
        Ok(result)
    }
    pub async fn get(
        db: &impl ConnectionTrait,
        slug: &str,
    ) -> Result<Option<ManagedModelV2>, DbErr> {
        let Some(record) = ModelCatalogRepository::find_by_slug(db, slug).await? else {
            return Ok(None);
        };
        Self::assemble(db, record).await.map(Some)
    }

    pub async fn list(
        db: &impl ConnectionTrait,
        include_hidden: bool,
    ) -> Result<Vec<ManagedModelV2>, DbErr> {
        let mut query = models::Entity::find()
            .order_by_asc(models::Column::SortOrder)
            .order_by_asc(models::Column::Slug);
        if !include_hidden {
            query = query.filter(models::Column::Visibility.eq("list"));
        }
        let mut result = Vec::new();
        for record in query.all(db).await? {
            result.push(Self::assemble(db, record.try_into()?).await?);
        }
        Ok(result)
    }

    async fn assemble(
        db: &impl ConnectionTrait,
        record: CatalogModelRecord,
    ) -> Result<ManagedModelV2, DbErr> {
        let price = ModelPricesRepository::get(db, &record.id)
            .await?
            .ok_or_else(|| DbErr::Custom("model price is missing".into()))?
            .price;
        let price_tiers = ModelPriceTiersRepository::list_for_model(db, &record.id)
            .await?
            .into_iter()
            .map(|r| r.tier)
            .collect();
        let routes = routes::Entity::find()
            .filter(routes::Column::ModelId.eq(&record.id))
            .order_by_desc(routes::Column::Priority)
            .order_by_asc(routes::Column::Id)
            .all(db)
            .await?
            .into_iter()
            .map(|r| CatalogRouteRecord::from(r).route)
            .collect();
        let permission_group_ids = crate::model_groups::group_models_v2::Entity::find()
            .filter(crate::model_groups::group_models_v2::Column::ModelId.eq(&record.id))
            .filter(crate::model_groups::group_models_v2::Column::Enabled.eq(true))
            .order_by_asc(crate::model_groups::group_models_v2::Column::GroupId)
            .all(db)
            .await?
            .into_iter()
            .map(|r| r.group_id)
            .collect();
        Ok(ManagedModelV2 {
            id: record.id,
            slug: record.slug,
            display_name: record.display_name,
            description: record.description,
            provider: record.provider,
            family: record.family,
            category: record.category,
            tags: record.tags,
            origin: record.origin,
            enabled: record.enabled,
            supported_in_api: record.supported_in_api,
            visibility: record.visibility,
            sort_order: record.sort_order,
            context_window: record.context_window,
            max_context_window: record.max_context_window,
            default_reasoning_effort: record.default_reasoning_effort,
            capabilities: record.capabilities,
            instructions_mode: record.instructions_mode,
            instructions_text: record.instructions_text,
            fast_policy: record.fast_policy,
            builtin_revision: record.builtin_revision,
            user_edited: record.user_edited,
            created_at: record.created_at,
            updated_at: record.updated_at,
            price,
            price_tiers,
            routes,
            permission_group_ids,
        })
    }

    pub async fn stats(db: &impl ConnectionTrait) -> Result<ModelCatalogV2Stats, DbErr> {
        let models = Self::list(db, true).await?;
        Ok(ModelCatalogV2Stats {
            total: models.len() as i64,
            enabled: models.iter().filter(|m| m.enabled).count() as i64,
            builtin: models.iter().filter(|m| m.origin == "builtin").count() as i64,
            custom: models.iter().filter(|m| m.origin == "custom").count() as i64,
            price_missing: models
                .iter()
                .filter(|m| m.price.price_status == "missing")
                .count() as i64,
            missing_route: models
                .iter()
                .filter(|m| !m.routes.iter().any(|r| r.enabled))
                .count() as i64,
        })
    }

    pub async fn upsert_many(
        db: &DatabaseConnection,
        inputs: &[ManagedModelV2Upsert],
    ) -> Result<Vec<ManagedModelV2>, DbErr> {
        let tx = db.begin().await?;
        // Serialize aggregate rewrites with group assignments as both update
        // the same junction table. A late invalid route rolls back every model.
        crate::UsersRepository::lock(&tx, "model_groups").await?;
        let mut result = Vec::new();
        for input in inputs {
            result.push(Self::write(&tx, input).await?);
        }
        tx.commit().await?;
        Ok(result)
    }

    pub async fn update_prices(
        db: &DatabaseConnection,
        updates: &[ManagedModelPriceV2Update],
    ) -> Result<Vec<String>, DbErr> {
        Self::update_prices_with_custom_override(db, updates, false).await
    }

    pub async fn update_prices_with_custom_override(
        db: &DatabaseConnection,
        updates: &[ManagedModelPriceV2Update],
        allow_custom_override: bool,
    ) -> Result<Vec<String>, DbErr> {
        let mut seen = std::collections::HashSet::new();
        for update in updates {
            let slug = update.slug.trim();
            if slug.is_empty() || !seen.insert(slug.to_ascii_lowercase()) {
                return Err(DbErr::Custom(
                    "invalid or duplicate managed model price slug".into(),
                ));
            }
            validate_managed_model_price_v2(&update.price, &update.price_tiers)
                .map_err(|error| DbErr::Custom(error.to_string()))?;
        }

        let tx = db.begin().await?;
        // Full model writes use the same lock, so the custom-price policy
        // cannot be interleaved between this check and tier replacement.
        crate::UsersRepository::lock(&tx, "model_groups").await?;
        let now = now_ts();
        let mut updated_slugs = Vec::new();
        for update in updates {
            let model = Self::get(&tx, update.slug.trim())
                .await?
                .ok_or_else(|| DbErr::Custom(format!("model_not_found: {}", update.slug)))?;
            if model.price.price_status == "custom" && !allow_custom_override {
                continue;
            }
            if update.price.price_status == "missing" {
                use crate::model_groups::group_models_v2 as gm;
                gm::Entity::delete_many()
                    .filter(gm::Column::ModelId.eq(&model.id))
                    .exec(&tx)
                    .await?;
            }
            ModelPricesRepository::put(
                &tx,
                CatalogPriceRecord {
                    model_id: model.id.clone(),
                    price: update.price.clone(),
                    created_at: model.created_at,
                    updated_at: now,
                },
            )
            .await?;
            let tiers = update
                .price_tiers
                .iter()
                .cloned()
                .map(|tier| CatalogPriceTierRecord {
                    model_id: model.id.clone(),
                    tier,
                    created_at: model.created_at,
                    updated_at: now,
                })
                .collect::<Vec<_>>();
            ModelPriceTiersRepository::replace_for_model(&tx, &model.id, &tiers).await?;
            updated_slugs.push(model.slug);
        }
        tx.commit().await?;
        Ok(updated_slugs)
    }

    async fn write(
        db: &impl ConnectionTrait,
        input: &ManagedModelV2Upsert,
    ) -> Result<ManagedModelV2, DbErr> {
        let mut model = input.model.clone();
        model.slug = model.slug.trim().into();
        model.display_name = model.display_name.trim().into();
        for value in [
            &mut model.description,
            &mut model.provider,
            &mut model.family,
            &mut model.category,
            &mut model.instructions_text,
        ] {
            if value.as_ref().is_some_and(|v| v.trim().is_empty()) {
                *value = None;
            }
        }
        validate_managed_model_v2(&model).map_err(|e| DbErr::Custom(e.to_string()))?;
        let previous = input.previous_slug.as_deref().unwrap_or(&model.slug).trim();
        let existing = ModelCatalogRepository::find_by_slug(db, previous).await?;
        let now = now_ts();
        if let Some(existing) = existing {
            if existing.origin == "builtin" && !model.slug.eq_ignore_ascii_case(previous) {
                return Err(DbErr::Custom("builtin model slug cannot be renamed".into()));
            }
            if model.origin != existing.origin {
                return Err(DbErr::Custom("model origin cannot be changed".into()));
            }
            model.id = existing.id;
            model.created_at = existing.created_at;
            // Preserve immutable seed identity/revision as the SQLite upsert does.
            model.builtin_revision = existing.builtin_revision;
        } else {
            if model.origin != "custom" {
                return Err(DbErr::Custom("only custom models can be created".into()));
            }
            if model.id.trim().is_empty() {
                model.id = format!("custom:{}", stable_hash(&model.slug.to_ascii_lowercase()));
            }
            if ModelCatalogRepository::get(db, &model.id).await?.is_some() {
                return Err(DbErr::Custom("model id already exists".into()));
            }
            model.created_at = now;
        }
        model.updated_at = now;
        model.user_edited = true;
        ModelCatalogRepository::put(db, CatalogModelRecord::from(model.clone())).await?;
        ModelPricesRepository::put(
            db,
            CatalogPriceRecord {
                model_id: model.id.clone(),
                price: model.price.clone(),
                created_at: model.created_at,
                updated_at: now,
            },
        )
        .await?;
        let tiers = model
            .price_tiers
            .iter()
            .cloned()
            .map(|tier| CatalogPriceTierRecord {
                model_id: model.id.clone(),
                tier,
                created_at: model.created_at,
                updated_at: now,
            })
            .collect::<Vec<_>>();
        ModelPriceTiersRepository::replace_for_model(db, &model.id, &tiers).await?;
        routes::Entity::delete_many()
            .filter(routes::Column::ModelId.eq(&model.id))
            .exec(db)
            .await?;
        for route in &mut model.routes {
            if route.id.trim().is_empty() {
                route.id = format!(
                    "route:{}",
                    stable_hash(&format!(
                        "{}\0{}\0{}\0{}",
                        model.id, route.source_kind, route.source_id, route.upstream_model
                    ))
                );
            }
            // A route ID belonging to another model must never be overwritten.
            if ModelRoutesRepository::get(db, &route.id).await?.is_some() {
                return Err(DbErr::Custom("model route id already exists".into()));
            }
            ModelRoutesRepository::put(
                db,
                CatalogRouteRecord {
                    model_id: model.id.clone(),
                    route: route.clone(),
                    created_at: now,
                    updated_at: now,
                },
            )
            .await?;
        }
        use crate::model_groups::group_models_v2 as gm;
        gm::Entity::delete_many()
            .filter(gm::Column::ModelId.eq(&model.id))
            .exec(db)
            .await?;
        for group_id in &model.permission_group_ids {
            if group_id == "mg_default" {
                continue;
            }
            gm::Entity::insert(gm::ActiveModel {
                group_id: Set(group_id.clone()),
                model_id: Set(model.id.clone()),
                enabled: Set(true),
                rate_multiplier_millis: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .exec(db)
            .await?;
        }
        Self::get(db, &model.slug)
            .await?
            .ok_or_else(|| DbErr::Custom("model_not_found".into()))
    }

    pub async fn update_states(
        db: &DatabaseConnection,
        input: &ManagedModelBatchStateV2Update,
    ) -> Result<Vec<ManagedModelV2>, DbErr> {
        if !matches!(input.visibility.trim(), "list" | "hide") {
            return Err(DbErr::Custom("invalid model visibility".into()));
        }
        let mut seen = std::collections::HashSet::new();
        let slugs = input
            .slugs
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && seen.insert(s.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        if slugs.is_empty() {
            return Err(DbErr::Custom(
                "managed model slugs must not be empty".into(),
            ));
        }
        let tx = db.begin().await?;
        crate::UsersRepository::lock(&tx, "model_groups").await?;
        let mut result = Vec::new();
        for slug in slugs {
            let mut record = ModelCatalogRepository::find_by_slug(&tx, slug)
                .await?
                .ok_or_else(|| DbErr::Custom("model_not_found".into()))?;
            record.enabled = input.enabled;
            record.visibility = input.visibility.trim().into();
            record.user_edited = true;
            record.updated_at = now_ts();
            ModelCatalogRepository::put(&tx, record).await?;
            result.push(
                Self::get(&tx, slug)
                    .await?
                    .ok_or_else(|| DbErr::Custom("model_not_found".into()))?,
            );
        }
        tx.commit().await?;
        Ok(result)
    }

    pub async fn delete(db: &DatabaseConnection, slug: &str) -> Result<(), DbErr> {
        let tx = db.begin().await?;
        crate::UsersRepository::lock(&tx, "model_groups").await?;
        let record = ModelCatalogRepository::find_by_slug(&tx, slug)
            .await?
            .ok_or_else(|| DbErr::Custom("model_not_found".into()))?;
        if record.origin == "builtin" {
            super::reconcile::mark_builtin_deleted(&tx, &record.slug, now_ts()).await?;
        }
        ModelCatalogRepository::delete(&tx, &record.id).await?;
        tx.commit().await
    }
}

fn stable_hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .take(12)
        .map(|v| format!("{v:02x}"))
        .collect()
}

impl From<ManagedModelV2> for CatalogModelRecord {
    fn from(m: ManagedModelV2) -> Self {
        Self {
            id: m.id,
            slug: m.slug,
            display_name: m.display_name,
            description: m.description,
            provider: m.provider,
            family: m.family,
            category: m.category,
            tags: m.tags,
            origin: m.origin,
            enabled: m.enabled,
            supported_in_api: m.supported_in_api,
            visibility: m.visibility,
            sort_order: m.sort_order,
            context_window: m.context_window,
            max_context_window: m.max_context_window,
            default_reasoning_effort: m.default_reasoning_effort,
            capabilities: m.capabilities,
            instructions_mode: m.instructions_mode,
            instructions_text: m.instructions_text,
            fast_policy: m.fast_policy,
            builtin_revision: m.builtin_revision,
            user_edited: m.user_edited,
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SeaOrmStorage;
    use codexmanager_core::storage::{ModelPriceV2, ModelRouteV2, StorageBackendKind};

    async fn exercise(db: &DatabaseConnection) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let slug = format!("managed-fixture-{stamp}");
        let mut model = ManagedModelV2 {
            slug: slug.clone(),
            display_name: "Managed fixture".into(),
            origin: "custom".into(),
            enabled: true,
            supported_in_api: true,
            visibility: "list".into(),
            instructions_mode: "passthrough".into(),
            capabilities: serde_json::json!({"tools":true}),
            price: ModelPriceV2 {
                price_status: "missing".into(),
                ..Default::default()
            },
            routes: vec![ModelRouteV2 {
                source_kind: "account_pool".into(),
                source_id: "default".into(),
                upstream_model: "fixture".into(),
                enabled: true,
                weight: 1,
                ..Default::default()
            }],
            ..Default::default()
        };
        let saved = ManagedModelsRepository::upsert_many(
            db,
            &[ManagedModelV2Upsert {
                model: model.clone(),
                previous_slug: None,
            }],
        )
        .await
        .unwrap()
        .pop()
        .unwrap();
        assert!(!saved.id.is_empty());
        assert!(!saved.routes[0].id.is_empty());
        assert!(saved.user_edited);
        let mut invalid = model.clone();
        invalid.slug = format!("{slug}-invalid");
        invalid.routes[0].weight = 0;
        model.display_name = "must roll back".into();
        assert!(ManagedModelsRepository::upsert_many(
            db,
            &[
                ManagedModelV2Upsert {
                    model: model.clone(),
                    previous_slug: None
                },
                ManagedModelV2Upsert {
                    model: invalid.clone(),
                    previous_slug: None
                },
            ]
        )
        .await
        .is_err());
        assert_eq!(
            ManagedModelsRepository::get(db, &slug)
                .await
                .unwrap()
                .unwrap()
                .display_name,
            "Managed fixture"
        );
        assert!(ManagedModelsRepository::get(db, &invalid.slug)
            .await
            .unwrap()
            .is_none());
        model.slug = format!("{slug}-renamed");
        let renamed = ManagedModelsRepository::upsert_many(
            db,
            &[ManagedModelV2Upsert {
                model: model.clone(),
                previous_slug: Some(slug.clone()),
            }],
        )
        .await
        .unwrap()
        .pop()
        .unwrap();
        assert_eq!(renamed.id, saved.id);
        assert!(ManagedModelsRepository::get(db, &slug)
            .await
            .unwrap()
            .is_none());
        assert!(ManagedModelsRepository::update_states(
            db,
            &ManagedModelBatchStateV2Update {
                slugs: vec![renamed.slug.clone(), format!("missing-{stamp}")],
                enabled: false,
                visibility: "hide".into(),
            }
        )
        .await
        .is_err());
        assert!(
            ManagedModelsRepository::get(db, &renamed.slug)
                .await
                .unwrap()
                .unwrap()
                .enabled
        );
        ManagedModelsRepository::delete(db, &renamed.slug)
            .await
            .unwrap();
        assert!(ModelPricesRepository::get(db, &saved.id)
            .await
            .unwrap()
            .is_none());
        assert!(ModelRoutesRepository::get(db, &saved.routes[0].id)
            .await
            .unwrap()
            .is_none());

        let mut builtin = CatalogModelRecord::from(saved.clone());
        builtin.id = format!("builtin-fixture-{stamp}");
        builtin.slug = format!("builtin-fixture-{stamp}");
        builtin.origin = "builtin".into();
        builtin.builtin_revision = Some(1);
        ModelCatalogRepository::put(db, builtin.clone())
            .await
            .unwrap();
        ModelPricesRepository::put(
            db,
            CatalogPriceRecord {
                model_id: builtin.id.clone(),
                price: saved.price,
                created_at: 1,
                updated_at: 1,
            },
        )
        .await
        .unwrap();
        ManagedModelsRepository::delete(db, &builtin.slug)
            .await
            .unwrap();
        assert!(ManagedModelsRepository::get(db, &builtin.slug)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn managed_model_aggregate_is_atomic_and_preserves_identity() {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        exercise(storage.connection()).await;
    }

    #[cfg(feature = "mysql")]
    #[tokio::test]
    #[ignore = "requires isolated MySQL database"]
    async fn mysql_managed_model_aggregate() {
        let url = std::env::var("CODEXMANAGER_TEST_MYSQL_URL").unwrap();
        let storage = SeaOrmStorage::connect(StorageBackendKind::Mysql, &url)
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        exercise(storage.connection()).await;
    }

    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[ignore = "requires isolated PostgreSQL database"]
    async fn postgres_managed_model_aggregate() {
        let url = std::env::var("CODEXMANAGER_TEST_POSTGRES_URL").unwrap();
        let storage = SeaOrmStorage::connect(StorageBackendKind::Postgres, &url)
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        exercise(storage.connection()).await;
    }
}
