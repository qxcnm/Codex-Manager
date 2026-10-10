use super::*;
use codexmanager_core::storage::{
    ManagedModelPriceV2Update, ModelFastPolicyV2, ModelPriceTierV2, ModelPriceV2, ModelRouteV2,
    StorageBackendKind,
};
use sea_orm::{DatabaseConnection, TransactionTrait};

fn estimated_price_update(slug: &str, input: i64) -> ManagedModelPriceV2Update {
    ManagedModelPriceV2Update {
        slug: slug.to_string(),
        price: ModelPriceV2 {
            price_status: "estimated".into(),
            price_source: Some("fixture".into()),
            input_microusd_per_1m: Some(input),
            cached_input_microusd_per_1m: Some(input),
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: Some(input * 2),
        },
        price_tiers: vec![ModelPriceTierV2 {
            min_input_tokens: 0,
            input_microusd_per_1m: input,
            cached_input_microusd_per_1m: input,
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: input * 2,
        }],
    }
}

fn missing_price_update(slug: &str) -> ManagedModelPriceV2Update {
    ManagedModelPriceV2Update {
        slug: slug.to_string(),
        price: ModelPriceV2 {
            price_status: "missing".into(),
            ..Default::default()
        },
        price_tiers: Vec::new(),
    }
}

pub(crate) async fn exercise(db: &DatabaseConnection) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let id = format!("catalog-fixture-{stamp}");
    let mut model = CatalogModelRecord {
        id: id.clone(),
        slug: format!("Catalog-Fixture-{stamp}"),
        display_name: "跨数据库模型".into(),
        description: Some("long description ".repeat(60)),
        provider: Some("fixture".into()),
        family: Some("reasoning".into()),
        category: None,
        origin: "custom".into(),
        enabled: true,
        supported_in_api: true,
        visibility: "list".into(),
        sort_order: 3,
        context_window: Some(272000),
        max_context_window: Some(1_000_000),
        default_reasoning_effort: Some("high".into()),
        instructions_mode: "fallback".into(),
        instructions_text: Some("preserved instructions".into()),
        builtin_revision: None,
        user_edited: true,
        created_at: 10,
        updated_at: 20,
        tags: vec!["多语言".into(), "tools".into()],
        capabilities: serde_json::json!({"tools":true,"vision":false,"notes":"x".repeat(700)}),
        fast_policy: ModelFastPolicyV2::Filter,
    };
    ModelCatalogRepository::put(db, model.clone())
        .await
        .expect("insert model");
    assert_eq!(
        ModelCatalogRepository::get(db, &id)
            .await
            .expect("read model"),
        Some(model.clone())
    );
    assert_eq!(
        ModelCatalogRepository::find_by_slug(db, &format!(" {} ", model.slug.to_ascii_uppercase()))
            .await
            .expect("case-insensitive slug"),
        Some(model.clone())
    );
    let mut conflict = model.clone();
    conflict.id = format!("{id}-conflict");
    conflict.slug = model.slug.to_ascii_lowercase();
    conflict.display_name = "must not overwrite".into();
    assert!(ModelCatalogRepository::put(db, conflict.clone())
        .await
        .is_err());
    assert!(ModelCatalogRepository::get(db, &conflict.id)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        ModelCatalogRepository::get(db, &id).await.unwrap(),
        Some(model.clone())
    );
    model.enabled = false;
    model.fast_policy = ModelFastPolicyV2::Force;
    model.updated_at = 21;
    ModelCatalogRepository::put(db, model.clone())
        .await
        .expect("update model");
    assert_eq!(
        ModelCatalogRepository::get(db, &id).await.unwrap(),
        Some(model.clone())
    );
    assert!(ModelCatalogRepository::list(db, true, 0)
        .await
        .unwrap()
        .is_empty());

    let mut route = CatalogRouteRecord {
        model_id: id.clone(),
        route: ModelRouteV2 {
            id: format!("{id}-route"),
            source_kind: "aggregate_api".into(),
            source_id: "fixture-source".into(),
            upstream_model: "upstream-model".into(),
            enabled: true,
            priority: 2,
            weight: 3,
        },
        created_at: 10,
        updated_at: 20,
    };
    ModelRoutesRepository::put(db, route.clone())
        .await
        .expect("insert route");
    assert_eq!(
        ModelRoutesRepository::get(db, &route.route.id)
            .await
            .unwrap(),
        Some(route.clone())
    );
    let mut route_conflict = route.clone();
    route_conflict.route.id = format!("{id}-route-conflict");
    assert!(ModelRoutesRepository::put(db, route_conflict)
        .await
        .is_err());
    route.route.priority = 99;
    route.route.enabled = false;
    ModelRoutesRepository::put(db, route.clone())
        .await
        .expect("update route");
    assert_eq!(
        ModelRoutesRepository::list_for_model(db, &id, false, 10)
            .await
            .unwrap(),
        vec![route.clone()]
    );
    assert!(ModelRoutesRepository::list_for_model(db, &id, true, 10)
        .await
        .unwrap()
        .is_empty());

    let price = CatalogPriceRecord {
        model_id: id.clone(),
        price: ModelPriceV2 {
            price_status: "custom".into(),
            price_source: None,
            input_microusd_per_1m: Some(9_007_199_254_740_993),
            cached_input_microusd_per_1m: Some(1),
            cache_write_microusd_per_1m: None,
            output_microusd_per_1m: Some(9_007_199_254_740_995),
        },
        created_at: 10,
        updated_at: 20,
    };
    ModelPricesRepository::put(db, price.clone())
        .await
        .expect("insert integer prices");
    assert_eq!(
        ModelPricesRepository::get(db, &id).await.unwrap(),
        Some(price.clone())
    );
    let tiers = vec![
        CatalogPriceTierRecord {
            model_id: id.clone(),
            tier: codexmanager_core::storage::ModelPriceTierV2 {
                min_input_tokens: 0,
                input_microusd_per_1m: 9_007_199_254_740_993,
                cached_input_microusd_per_1m: 1,
                cache_write_microusd_per_1m: None,
                output_microusd_per_1m: 9_007_199_254_740_995,
            },
            created_at: 10,
            updated_at: 20,
        },
        CatalogPriceTierRecord {
            model_id: id.clone(),
            tier: codexmanager_core::storage::ModelPriceTierV2 {
                min_input_tokens: 272_001,
                input_microusd_per_1m: 8,
                cached_input_microusd_per_1m: 1,
                cache_write_microusd_per_1m: Some(2),
                output_microusd_per_1m: 7,
            },
            created_at: 10,
            updated_at: 20,
        },
    ];
    ModelPriceTiersRepository::replace_for_model(db, &id, &tiers)
        .await
        .expect("replace price tiers");
    assert_eq!(
        ModelPriceTiersRepository::list_for_model(db, &id)
            .await
            .unwrap(),
        tiers
    );
    let mut invalid_tiers = tiers.clone();
    invalid_tiers.push(invalid_tiers[0].clone());
    assert!(
        ModelPriceTiersRepository::replace_for_model(db, &id, &invalid_tiers)
            .await
            .is_err()
    );
    let mut invalid = price.clone();
    invalid.price.price_status = "missing".into();
    assert!(ModelPricesRepository::put(db, invalid).await.is_err());
    assert_eq!(
        ModelPricesRepository::get(db, &id).await.unwrap(),
        Some(price.clone())
    );

    let ignored =
        ManagedModelsRepository::update_prices(db, &[estimated_price_update(&model.slug, 101)])
            .await
            .expect("preserve custom price");
    assert!(ignored.is_empty());
    assert_eq!(
        ModelPricesRepository::get(db, &id).await.unwrap(),
        Some(price.clone())
    );
    assert_eq!(
        ModelPriceTiersRepository::list_for_model(db, &id)
            .await
            .unwrap(),
        tiers
    );

    let forced_update = estimated_price_update(&model.slug, 151);
    assert_eq!(
        ManagedModelsRepository::update_prices_with_custom_override(
            db,
            std::slice::from_ref(&forced_update),
            true,
        )
        .await
        .expect("explicitly selected custom price can be overwritten"),
        vec![model.slug.clone()]
    );
    let forced = ManagedModelsRepository::get(db, &model.slug)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(forced.price, forced_update.price);
    assert_eq!(forced.price_tiers, forced_update.price_tiers);

    let mut estimated_seed = price.clone();
    estimated_seed.price.price_status = "estimated".into();
    estimated_seed.price.price_source = Some("old-fixture".into());
    ModelPricesRepository::put(db, estimated_seed)
        .await
        .expect("prepare externally managed price");
    let synced_update = estimated_price_update(&model.slug, 202);
    assert_eq!(
        ManagedModelsRepository::update_prices(db, std::slice::from_ref(&synced_update))
            .await
            .expect("sync price"),
        vec![model.slug.clone()]
    );
    let synced = ManagedModelsRepository::get(db, &model.slug)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(synced.price, synced_update.price);
    assert_eq!(synced.price_tiers, synced_update.price_tiers);

    let error = ManagedModelsRepository::update_prices(
        db,
        &[
            estimated_price_update(&model.slug, 303),
            estimated_price_update("missing-model", 404),
        ],
    )
    .await
    .expect_err("late missing model must roll back all price changes");
    assert!(error.to_string().contains("model_not_found"));
    let after_failed = ManagedModelsRepository::get(db, &model.slug)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_failed.price, synced.price);
    assert_eq!(after_failed.price_tiers, synced.price_tiers);

    model.enabled = true;
    model.updated_at = 30;
    ModelCatalogRepository::put(db, model.clone())
        .await
        .expect("enable model for explicit billing group");
    let group_id = format!("price-sync-group-{stamp}");
    crate::ModelGroupsRepository::upsert(
        db,
        crate::ModelGroupRecord {
            id: group_id.clone(),
            name: "Price sync group".into(),
            description: None,
            status: "active".into(),
            sort: 10,
            is_default: false,
            rate_multiplier_millis: 1_000,
            created_at: 30,
            updated_at: 30,
        },
    )
    .await
    .expect("create explicit billing group");
    crate::ModelGroupsRepository::replace_models_v2(
        db,
        &group_id,
        &[crate::ModelGroupModelRecord {
            group_id: group_id.clone(),
            platform_model_slug: model.slug.clone(),
            enabled: true,
            rate_multiplier_millis: None,
            billing_model_slug: None,
            note: None,
            created_at: 30,
            updated_at: 30,
        }],
    )
    .await
    .expect("assign explicit billing group");
    let grouped = ManagedModelsRepository::get(db, &model.slug)
        .await
        .unwrap()
        .unwrap();
    assert!(grouped.permission_group_ids.contains(&group_id));

    let error = ManagedModelsRepository::update_prices(
        db,
        &[
            missing_price_update(&model.slug),
            estimated_price_update("missing-model", 505),
        ],
    )
    .await
    .expect_err("late failure must roll back price and group invalidation");
    assert!(error.to_string().contains("model_not_found"));
    let after_invalidation_failure = ManagedModelsRepository::get(db, &model.slug)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after_invalidation_failure.price, synced.price);
    assert_eq!(after_invalidation_failure.price_tiers, synced.price_tiers);
    assert!(after_invalidation_failure
        .permission_group_ids
        .contains(&group_id));

    assert_eq!(
        ManagedModelsRepository::update_prices(db, &[missing_price_update(&model.slug)])
            .await
            .expect("invalidate withdrawn external price"),
        vec![model.slug.clone()]
    );
    let invalidated = ManagedModelsRepository::get(db, &model.slug)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(invalidated.price.price_status, "missing");
    assert!(invalidated.price.price_source.is_none());
    assert!(invalidated.price.input_microusd_per_1m.is_none());
    assert!(invalidated.price.cached_input_microusd_per_1m.is_none());
    assert!(invalidated.price.cache_write_microusd_per_1m.is_none());
    assert!(invalidated.price.output_microusd_per_1m.is_none());
    assert!(invalidated.price_tiers.is_empty());
    assert!(!invalidated.permission_group_ids.contains(&group_id));

    let rollback_id = format!("{id}-rollback");
    model.id = rollback_id.clone();
    model.slug = format!("{}-rollback", model.slug);
    route.model_id = rollback_id.clone();
    route.route.id = format!("{rollback_id}-route");
    let rollback_route_id = route.route.id.clone();
    let mut rollback_price = price;
    rollback_price.model_id = rollback_id.clone();
    let tx = db.begin().await.expect("begin catalog transaction");
    ModelCatalogRepository::put(&tx, model)
        .await
        .expect("transaction model");
    ModelRoutesRepository::put(&tx, route)
        .await
        .expect("transaction route");
    ModelPricesRepository::put(&tx, rollback_price)
        .await
        .expect("transaction price");
    tx.rollback().await.expect("rollback catalog");
    assert!(ModelCatalogRepository::get(db, &rollback_id)
        .await
        .unwrap()
        .is_none());
    assert!(ModelRoutesRepository::get(db, &rollback_route_id)
        .await
        .unwrap()
        .is_none());
    assert!(ModelPricesRepository::get(db, &rollback_id)
        .await
        .unwrap()
        .is_none());
    assert!(ModelCatalogRepository::delete(db, &id)
        .await
        .expect("remove isolated model"));
    assert!(ModelRoutesRepository::list_for_model(db, &id, false, 10)
        .await
        .unwrap()
        .is_empty());
    assert!(ModelPricesRepository::get(db, &id).await.unwrap().is_none());
    assert!(ModelPriceTiersRepository::list_for_model(db, &id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn model_catalog_preserves_metadata_routes_integer_prices_and_transactions() {
    let storage = crate::SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
        .await
        .unwrap();
    storage.migrate().await.unwrap();
    storage.migrate().await.unwrap();
    exercise(storage.connection()).await;
}
