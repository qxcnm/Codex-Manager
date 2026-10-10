use super::{
    models, CatalogModelRecord, CatalogPriceRecord, CatalogPriceTierRecord, CatalogRouteRecord,
    ManagedModelsRepository, ModelCatalogRepository, ModelPriceTiersRepository,
    ModelPricesRepository, ModelRoutesRepository,
};
use crate::desktop_history::{api_key_profiles, model_catalog_v2_meta as meta};
use codexmanager_core::storage::{
    now_ts, ManagedModelV2, ModelFastPolicyV2, ModelPriceTierV2, ModelPriceV2, ModelRouteV2,
};
use sea_orm::sea_query::OnConflict;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QuerySelect,
    Set, TransactionTrait,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const LATEST_REVISION: i64 = 9;
const RETIRED_REVISION9_SLUGS: &[&str] = &["gpt-5.4", "gpt-5.4-mini", "gpt-5.2"];
const DELETED_BUILTIN_META_PREFIX: &str = "deleted_builtin_model:";

#[derive(Debug, Clone, Deserialize)]
struct BuiltinCatalogFixture {
    revision: i64,
    source_sha256: String,
    models: Vec<BuiltinModelSeed>,
}

#[derive(Debug, Clone, Deserialize)]
struct BuiltinModelSeed {
    slug: String,
    display_name: String,
    description: String,
    visibility: String,
    priority: i64,
    default_reasoning_effort: Option<String>,
    context_window: Option<i64>,
    max_context_window: Option<i64>,
    capabilities: Value,
    price_status: String,
    price_source: Option<String>,
    price_tiers: Vec<ModelPriceTierV2>,
}

pub(crate) async fn reconcile_builtin_catalog(db: &DatabaseConnection) -> Result<(), DbErr> {
    reconcile_revision9_catalog(db).await?;
    // Revision 10 only adds GPT-6.1 Sol. Existing rows own their metadata,
    // prices and routes; insert_missing_seed also respects custom slugs and tombstones.
    let latest: BuiltinCatalogFixture = serde_json::from_str(include_str!(
        "../../../core/seeds/model_catalog_v2_2026_09_30.json"
    ))
    .expect("revision 10 model catalog fixture must be valid");
    let tx = db.begin().await?;
    crate::UsersRepository::lock(&tx, "model_groups").await?;
    let stored_revision = meta_value(&tx, "builtin_revision")
        .await?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default();
    let max_revision = models::Entity::find()
        .select_only()
        .column_as(models::Column::BuiltinRevision.max(), "max_revision")
        .filter(models::Column::Origin.eq("builtin"))
        .filter(models::Column::UserEdited.eq(false))
        .into_tuple::<Option<i64>>()
        .one(&tx)
        .await?
        .flatten()
        .unwrap_or_default();
    let effective_revision = stored_revision.max(max_revision);
    if effective_revision <= latest.revision {
        for seed in &latest.models {
            insert_missing_seed(&tx, &latest, seed, now_ts()).await?;
        }
        set_meta(&tx, "builtin_revision", &latest.revision.to_string()).await?;
        set_meta(&tx, "fixture_sha256", &latest.source_sha256).await?;
    } else if stored_revision < effective_revision {
        set_meta(&tx, "builtin_revision", &effective_revision.to_string()).await?;
    }
    tx.commit().await
}

async fn reconcile_revision9_catalog(db: &DatabaseConnection) -> Result<(), DbErr> {
    let latest = latest_fixture();
    let previous = previous_fixture();
    let tx = db.begin().await?;
    crate::UsersRepository::lock(&tx, "model_groups").await?;

    let stored_revision = meta_value(&tx, "builtin_revision")
        .await?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default();
    let max_unedited_revision = models::Entity::find()
        .select_only()
        .column_as(models::Column::BuiltinRevision.max(), "max_revision")
        .filter(models::Column::Origin.eq("builtin"))
        .filter(models::Column::UserEdited.eq(false))
        .into_tuple::<Option<i64>>()
        .one(&tx)
        .await?
        .flatten()
        .unwrap_or_default();
    let effective_revision = stored_revision.max(max_unedited_revision);

    if effective_revision > latest.revision {
        if stored_revision < effective_revision {
            set_meta(&tx, "builtin_revision", &effective_revision.to_string()).await?;
        }
        tx.commit().await?;
        return Ok(());
    }

    let now = now_ts();
    if effective_revision >= previous.revision && effective_revision < latest.revision {
        for seed in latest.models.iter().filter(|seed| {
            previous
                .models
                .iter()
                .any(|previous_seed| previous_seed.slug.eq_ignore_ascii_case(&seed.slug))
        }) {
            let tombstone_key = deleted_builtin_meta_key(&seed.slug);
            if meta_value(&tx, &tombstone_key).await?.is_none()
                && ModelCatalogRepository::find_by_slug(&tx, &seed.slug)
                    .await?
                    .is_none()
            {
                mark_builtin_deleted(&tx, &seed.slug, now).await?;
            }
        }
    }
    for seed in &latest.models {
        insert_missing_seed(&tx, &latest, seed, now).await?;
    }

    if effective_revision < latest.revision {
        for seed in &latest.models {
            let previous_seed = previous
                .models
                .iter()
                .find(|candidate| candidate.slug.eq_ignore_ascii_case(&seed.slug));
            reconcile_existing_seed(&tx, previous_seed, seed, now).await?;
        }

        for slug in RETIRED_REVISION9_SLUGS {
            let previous_seed = previous
                .models
                .iter()
                .find(|seed| seed.slug.eq_ignore_ascii_case(slug))
                .ok_or_else(|| {
                    DbErr::Custom(format!(
                        "revision 8 fixture is missing retired model {slug}"
                    ))
                })?;
            retire_seed(&tx, previous_seed, now).await?;
        }
    }

    models::Entity::update_many()
        .col_expr(
            models::Column::BuiltinRevision,
            sea_orm::sea_query::Expr::value(latest.revision),
        )
        .filter(models::Column::Origin.eq("builtin"))
        .filter(models::Column::UserEdited.eq(false))
        .filter(
            sea_orm::Condition::any()
                .add(models::Column::BuiltinRevision.is_null())
                .add(models::Column::BuiltinRevision.lt(latest.revision)),
        )
        .exec(&tx)
        .await?;
    set_meta(&tx, "builtin_revision", &latest.revision.to_string()).await?;
    set_meta(&tx, "fixture_sha256", &latest.source_sha256).await?;
    tx.commit().await
}

pub(super) async fn mark_builtin_deleted(
    db: &impl ConnectionTrait,
    slug: &str,
    now: i64,
) -> Result<(), DbErr> {
    set_meta(db, &deleted_builtin_meta_key(slug), &now.to_string()).await
}

async fn insert_missing_seed(
    db: &impl ConnectionTrait,
    fixture: &BuiltinCatalogFixture,
    seed: &BuiltinModelSeed,
    now: i64,
) -> Result<(), DbErr> {
    if meta_value(db, &deleted_builtin_meta_key(&seed.slug))
        .await?
        .is_some()
    {
        return Ok(());
    }
    if ModelCatalogRepository::find_by_slug(db, &seed.slug)
        .await?
        .is_some()
    {
        return Ok(());
    }

    let id = builtin_id(&seed.slug);
    if let Some(owner) = ModelCatalogRepository::get(db, &id).await? {
        return Err(DbErr::Custom(format!(
            "builtin model id {id} is already owned by {}",
            owner.slug
        )));
    }
    ModelCatalogRepository::put(
        db,
        CatalogModelRecord {
            id: id.clone(),
            slug: seed.slug.clone(),
            display_name: seed.display_name.clone(),
            description: Some(seed.description.clone()),
            provider: None,
            family: None,
            category: None,
            origin: "builtin".into(),
            enabled: true,
            supported_in_api: true,
            visibility: seed.visibility.clone(),
            sort_order: seed.priority,
            context_window: seed.context_window,
            max_context_window: seed.max_context_window,
            default_reasoning_effort: seed.default_reasoning_effort.clone(),
            instructions_mode: "passthrough".into(),
            instructions_text: None,
            builtin_revision: Some(fixture.revision),
            user_edited: false,
            created_at: now,
            updated_at: now,
            tags: Vec::new(),
            capabilities: seed.capabilities.clone(),
            fast_policy: ModelFastPolicyV2::Passthrough,
        },
    )
    .await?;
    replace_seed_pricing(db, &id, seed, now, now).await?;
    let route = default_route(&id, &seed.slug);
    ModelRoutesRepository::put(
        db,
        CatalogRouteRecord {
            model_id: id,
            route,
            created_at: now,
            updated_at: now,
        },
    )
    .await
}

async fn reconcile_existing_seed(
    db: &impl ConnectionTrait,
    previous_seed: Option<&BuiltinModelSeed>,
    seed: &BuiltinModelSeed,
    now: i64,
) -> Result<(), DbErr> {
    let Some(mut current) = ManagedModelsRepository::get(db, &seed.slug).await? else {
        return Ok(());
    };
    if current.origin != "builtin"
        || current.user_edited
        || current.builtin_revision.unwrap_or_default() >= LATEST_REVISION
    {
        return Ok(());
    }

    let replace_pricing =
        previous_seed.is_some_and(|previous| seed_pricing_matches(&current, previous));
    current.display_name = seed.display_name.clone();
    current.description = Some(seed.description.clone());
    current.visibility = seed.visibility.clone();
    current.sort_order = seed.priority;
    current.context_window = seed.context_window;
    current.max_context_window = seed.max_context_window;
    current.default_reasoning_effort = seed.default_reasoning_effort.clone();
    current.capabilities = seed.capabilities.clone();
    current.builtin_revision = Some(LATEST_REVISION);
    current.updated_at = now;
    ModelCatalogRepository::put(db, CatalogModelRecord::from(current.clone())).await?;
    if replace_pricing {
        replace_seed_pricing(db, &current.id, seed, current.created_at, now).await?;
    }
    Ok(())
}

async fn retire_seed(
    db: &impl ConnectionTrait,
    previous_seed: &BuiltinModelSeed,
    now: i64,
) -> Result<(), DbErr> {
    mark_builtin_deleted(db, &previous_seed.slug, now).await?;
    let Some(mut current) = ManagedModelsRepository::get(db, &previous_seed.slug).await? else {
        return Ok(());
    };
    if current.origin == "custom" {
        return Ok(());
    }
    if current.origin != "builtin" {
        return Err(DbErr::Custom(format!(
            "retired model {} has unsupported origin {}",
            current.slug, current.origin
        )));
    }

    let group_referenced = crate::model_groups::group_models_v2::Entity::find()
        .filter(crate::model_groups::group_models_v2::Column::ModelId.eq(&current.id))
        .one(db)
        .await?
        .is_some();
    let api_key_referenced = crate::api_keys::Entity::find()
        .select_only()
        .column(crate::api_keys::Column::ModelSlug)
        .filter(crate::api_keys::Column::ModelSlug.is_not_null())
        .into_tuple::<Option<String>>()
        .all(db)
        .await?
        .into_iter()
        .flatten()
        .any(|slug| slug.trim().eq_ignore_ascii_case(&current.slug));
    let profile_referenced = api_key_profiles::Entity::find()
        .select_only()
        .column(api_key_profiles::Column::DefaultModel)
        .filter(api_key_profiles::Column::DefaultModel.is_not_null())
        .into_tuple::<Option<String>>()
        .all(db)
        .await?
        .into_iter()
        .flatten()
        .any(|slug| slug.trim().eq_ignore_ascii_case(&current.slug));
    let pristine = !current.user_edited
        && seed_metadata_matches(&current, previous_seed)
        && seed_pricing_matches(&current, previous_seed)
        && has_only_default_route(&current);

    if pristine && !group_referenced && !api_key_referenced && !profile_referenced {
        ModelCatalogRepository::delete(db, &current.id).await?;
        return Ok(());
    }

    current.origin = "custom".into();
    current.builtin_revision = None;
    current.user_edited = true;
    current.updated_at = now;
    ModelCatalogRepository::put(db, CatalogModelRecord::from(current.clone())).await?;
    for route in current.routes.iter().filter(|route| {
        route.source_kind == "account_pool"
            && route.source_id == "default"
            && route.upstream_model.eq_ignore_ascii_case(&current.slug)
            && route.enabled
            && route.priority == 0
            && route.weight == 1
    }) {
        ModelRoutesRepository::delete(db, &route.id).await?;
    }
    Ok(())
}

fn seed_metadata_matches(model: &ManagedModelV2, seed: &BuiltinModelSeed) -> bool {
    model.display_name == seed.display_name
        && model.description.as_deref() == Some(seed.description.as_str())
        && model.provider.is_none()
        && model.family.is_none()
        && model.category.is_none()
        && model.tags.is_empty()
        && model.enabled
        && model.supported_in_api
        && model.visibility == seed.visibility
        && model.sort_order == seed.priority
        && model.context_window == seed.context_window
        && model.max_context_window == seed.max_context_window
        && model.default_reasoning_effort == seed.default_reasoning_effort
        && model.capabilities == seed.capabilities
        && model.instructions_mode == "passthrough"
        && model
            .instructions_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
        && model.fast_policy == ModelFastPolicyV2::Passthrough
}

fn seed_pricing_matches(model: &ManagedModelV2, seed: &BuiltinModelSeed) -> bool {
    seed_price(seed)
        .is_ok_and(|expected| model.price == expected && model.price_tiers == seed.price_tiers)
}

fn has_only_default_route(model: &ManagedModelV2) -> bool {
    matches!(
        model.routes.as_slice(),
        [route]
            if route.source_kind == "account_pool"
                && route.source_id == "default"
                && route.upstream_model.eq_ignore_ascii_case(&model.slug)
                && route.enabled
                && route.priority == 0
                && route.weight == 1
    )
}

async fn replace_seed_pricing(
    db: &impl ConnectionTrait,
    model_id: &str,
    seed: &BuiltinModelSeed,
    created_at: i64,
    updated_at: i64,
) -> Result<(), DbErr> {
    ModelPricesRepository::put(
        db,
        CatalogPriceRecord {
            model_id: model_id.into(),
            price: seed_price(seed)?,
            created_at,
            updated_at,
        },
    )
    .await?;
    let tiers = seed
        .price_tiers
        .iter()
        .cloned()
        .map(|tier| CatalogPriceTierRecord {
            model_id: model_id.into(),
            tier,
            created_at,
            updated_at,
        })
        .collect::<Vec<_>>();
    ModelPriceTiersRepository::replace_for_model(db, model_id, &tiers).await
}

fn seed_price(seed: &BuiltinModelSeed) -> Result<ModelPriceV2, DbErr> {
    if seed.price_status == "missing" {
        if !seed.price_tiers.is_empty() {
            return Err(DbErr::Custom(format!(
                "missing-price built-in {} has price tiers",
                seed.slug
            )));
        }
        return Ok(ModelPriceV2 {
            price_status: "missing".into(),
            price_source: seed.price_source.clone(),
            ..Default::default()
        });
    }
    let base = seed
        .price_tiers
        .iter()
        .find(|tier| tier.min_input_tokens == 0)
        .ok_or_else(|| {
            DbErr::Custom(format!(
                "priced built-in {} requires a min_input_tokens=0 tier",
                seed.slug
            ))
        })?;
    Ok(ModelPriceV2 {
        price_status: seed.price_status.clone(),
        price_source: seed.price_source.clone(),
        input_microusd_per_1m: Some(base.input_microusd_per_1m),
        cached_input_microusd_per_1m: Some(base.cached_input_microusd_per_1m),
        cache_write_microusd_per_1m: base.cache_write_microusd_per_1m,
        output_microusd_per_1m: Some(base.output_microusd_per_1m),
    })
}

async fn meta_value(db: &impl ConnectionTrait, key: &str) -> Result<Option<String>, DbErr> {
    Ok(meta::Entity::find_by_id(key)
        .one(db)
        .await?
        .map(|row| row.value))
}

async fn set_meta(db: &impl ConnectionTrait, key: &str, value: &str) -> Result<(), DbErr> {
    meta::Entity::insert(meta::ActiveModel {
        key: Set(key.into()),
        value: Set(value.into()),
    })
    .on_conflict(
        OnConflict::column(meta::Column::Key)
            .update_column(meta::Column::Value)
            .to_owned(),
    )
    .exec(db)
    .await
    .map(|_| ())
}

fn latest_fixture() -> BuiltinCatalogFixture {
    let fixture: BuiltinCatalogFixture = serde_json::from_str(include_str!(
        "../../../core/seeds/model_catalog_v2_2026_09_24.json"
    ))
    .expect("revision 9 model catalog fixture must be valid");
    assert_eq!(fixture.revision, LATEST_REVISION);
    fixture
}

fn previous_fixture() -> BuiltinCatalogFixture {
    serde_json::from_str(include_str!(
        "../../../core/seeds/model_catalog_v2_2026_07_10.json"
    ))
    .expect("revision 8 model catalog fixture must be valid")
}

fn builtin_id(slug: &str) -> String {
    format!("builtin:{}", slug.trim().to_ascii_lowercase())
}

fn default_route(model_id: &str, slug: &str) -> ModelRouteV2 {
    let mut route = ModelRouteV2 {
        source_kind: "account_pool".into(),
        source_id: "default".into(),
        upstream_model: slug.into(),
        enabled: true,
        priority: 0,
        weight: 1,
        ..Default::default()
    };
    route.id = route_id(model_id, &route);
    route
}

fn route_id(model_id: &str, route: &ModelRouteV2) -> String {
    let identity = format!(
        "{model_id}\0{}\0{}\0{}",
        route.source_kind, route.source_id, route.upstream_model
    );
    format!("route:{}", stable_hash(&identity))
}

fn stable_hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn deleted_builtin_meta_key(slug: &str) -> String {
    format!(
        "{DELETED_BUILTIN_META_PREFIX}{}",
        slug.trim().to_ascii_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ApiKeyRecord, ApiKeysRepository, ModelGroupRecord, ModelGroupsRepository, SeaOrmStorage,
    };
    use codexmanager_core::storage::StorageBackendKind;
    use std::collections::HashSet;

    async fn schema_only() -> SeaOrmStorage {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .expect("connect test database");
        crate::migration::migrate(storage.connection())
            .await
            .expect("create schema without catalog reconciliation");
        storage
    }

    async fn seed_previous_catalog(db: &DatabaseConnection) {
        let fixture = previous_fixture();
        let tx = db.begin().await.expect("begin revision 8 seed");
        for seed in &fixture.models {
            insert_missing_seed(&tx, &fixture, seed, 8)
                .await
                .expect("insert revision 8 seed");
        }
        set_meta(&tx, "builtin_revision", &fixture.revision.to_string())
            .await
            .expect("store revision 8 marker");
        set_meta(&tx, "fixture_sha256", &fixture.source_sha256)
            .await
            .expect("store revision 8 fixture hash");
        tx.commit().await.expect("commit revision 8 seed");
    }

    async fn model(db: &DatabaseConnection, slug: &str) -> ManagedModelV2 {
        ManagedModelsRepository::get(db, slug)
            .await
            .expect("read managed model")
            .unwrap_or_else(|| panic!("missing model {slug}"))
    }

    #[tokio::test]
    async fn revision10_adds_sol61_and_preserves_revision9_catalog_and_new_model_edits() {
        let storage = schema_only().await;
        reconcile_revision9_catalog(storage.connection())
            .await
            .unwrap();
        let before = ManagedModelsRepository::list(storage.connection(), true)
            .await
            .unwrap();
        reconcile_builtin_catalog(storage.connection())
            .await
            .unwrap();
        let sol = model(storage.connection(), "gpt-6.1-sol").await;
        assert_eq!(sol.builtin_revision, Some(10));
        assert_eq!(sol.default_reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(sol.price.cached_input_microusd_per_1m, Some(100_000));
        assert_eq!(sol.price.cache_write_microusd_per_1m, Some(2_500_000));
        assert_eq!(sol.price_tiers[1].min_input_tokens, 272_001);
        assert_eq!(sol.price_tiers[1].cached_input_microusd_per_1m, 200_000);
        assert_eq!(sol.price_tiers[1].output_microusd_per_1m, 15_000_000);
        assert_eq!(sol.routes[0].upstream_model, "gpt-6.1-sol");
        let after: Vec<_> = ManagedModelsRepository::list(storage.connection(), true)
            .await
            .unwrap()
            .into_iter()
            .filter(|m| m.slug != "gpt-6.1-sol")
            .collect();
        assert_eq!(
            serde_json::to_value(after).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        for custom in [false, true] {
            let mut edited = model(storage.connection(), "gpt-6.1-sol").await;
            edited.display_name = "My Sol".into();
            edited.user_edited = true;
            edited.enabled = false;
            if custom {
                edited.origin = "custom".into();
                edited.builtin_revision = None;
            }
            ModelCatalogRepository::put(
                storage.connection(),
                CatalogModelRecord::from(edited.clone()),
            )
            .await
            .unwrap();
            set_meta(storage.connection(), "builtin_revision", "9")
                .await
                .unwrap();
            reconcile_builtin_catalog(storage.connection())
                .await
                .unwrap();
            assert_eq!(
                serde_json::to_value(model(storage.connection(), "gpt-6.1-sol").await).unwrap(),
                serde_json::to_value(edited).unwrap()
            );
        }
    }

    #[tokio::test]
    async fn fresh_catalog_seeds_all_models_and_respects_delete_tombstones() {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .expect("connect");
        storage.migrate().await.expect("initial migration");
        storage
            .reconcile_builtin_model_catalog()
            .await
            .expect("initial catalog reconciliation");
        storage.migrate().await.expect("idempotent migration");
        storage
            .reconcile_builtin_model_catalog()
            .await
            .expect("idempotent catalog reconciliation");

        let models = ManagedModelsRepository::list(storage.connection(), true)
            .await
            .expect("list seeded models");
        assert_eq!(models.len(), 12);
        let slugs = models
            .iter()
            .map(|model| model.slug.as_str())
            .collect::<HashSet<_>>();
        for slug in [
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-image-2.5-sunburst",
            "gpt-image-2.5-flare",
            "codex-auto-review",
        ] {
            assert!(slugs.contains(slug), "missing seeded model {slug}");
        }
        assert_eq!(
            model(storage.connection(), "codex-auto-review")
                .await
                .visibility,
            "hide"
        );
        for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
            let image = model(storage.connection(), slug).await;
            assert_eq!(image.price.price_status, "missing");
            assert!(image.price_tiers.is_empty());
            assert_eq!(image.routes.len(), 1);
            assert_eq!(image.routes[0].upstream_model, slug);
        }
        assert!(models.iter().all(|model| {
            model.origin == "builtin"
                && model.builtin_revision
                    == Some(if model.slug == "gpt-6.1-sol" {
                        10
                    } else {
                        LATEST_REVISION
                    })
        }));

        ManagedModelsRepository::delete(storage.connection(), "gpt-6-luna")
            .await
            .expect("delete builtin model");
        storage.migrate().await.expect("repeat after deletion");
        storage
            .reconcile_builtin_model_catalog()
            .await
            .expect("repeat catalog reconciliation after deletion");
        assert!(
            ManagedModelsRepository::get(storage.connection(), "gpt-6-luna")
                .await
                .expect("read deleted model")
                .is_none()
        );
        assert!(meta_value(
            storage.connection(),
            &deleted_builtin_meta_key("gpt-6-luna")
        )
        .await
        .expect("read deletion tombstone")
        .is_some());
    }

    #[tokio::test]
    async fn revision8_upgrade_adds_latest_and_removes_exact_legacy_builtins() {
        let storage = schema_only().await;
        seed_previous_catalog(storage.connection()).await;

        let mut older_revision =
            ModelCatalogRepository::find_by_slug(storage.connection(), "gpt-5.2")
                .await
                .expect("read legacy model")
                .expect("legacy model exists");
        older_revision.builtin_revision = Some(7);
        ModelCatalogRepository::put(storage.connection(), older_revision)
            .await
            .expect("simulate older exact seed revision");

        reconcile_builtin_catalog(storage.connection())
            .await
            .expect("upgrade catalog");
        reconcile_builtin_catalog(storage.connection())
            .await
            .expect("replay catalog upgrade");

        for slug in RETIRED_REVISION9_SLUGS {
            assert!(ManagedModelsRepository::get(storage.connection(), slug)
                .await
                .expect("read retired model")
                .is_none());
        }
        let models = ManagedModelsRepository::list(storage.connection(), true)
            .await
            .expect("list upgraded models");
        assert_eq!(models.len(), 12);
        assert!(models.iter().all(|model| {
            model.origin != "builtin"
                || model.builtin_revision
                    == Some(if model.slug == "gpt-6.1-sol" {
                        10
                    } else {
                        LATEST_REVISION
                    })
        }));
        assert_eq!(
            meta_value(storage.connection(), "builtin_revision")
                .await
                .expect("read catalog marker")
                .as_deref(),
            Some("10")
        );
    }

    #[tokio::test]
    async fn revision8_upgrade_preserves_legacy_deletion_without_tombstone() {
        let storage = schema_only().await;
        seed_previous_catalog(storage.connection()).await;

        let deleted = ModelCatalogRepository::find_by_slug(storage.connection(), "gpt-5.6-terra")
            .await
            .expect("read legacy built-in")
            .expect("legacy built-in exists");
        ModelCatalogRepository::delete(storage.connection(), &deleted.id)
            .await
            .expect("simulate legacy deletion without tombstone");
        assert!(meta_value(
            storage.connection(),
            &deleted_builtin_meta_key("gpt-5.6-terra")
        )
        .await
        .expect("read pre-upgrade deletion tombstone")
        .is_none());

        reconcile_builtin_catalog(storage.connection())
            .await
            .expect("upgrade catalog");

        assert!(
            ManagedModelsRepository::get(storage.connection(), "gpt-5.6-terra")
                .await
                .expect("read legacy-deleted model")
                .is_none()
        );
        assert!(meta_value(
            storage.connection(),
            &deleted_builtin_meta_key("gpt-5.6-terra")
        )
        .await
        .expect("read migrated deletion tombstone")
        .is_some());
        for slug in [
            "gpt-6-sol",
            "gpt-6-luna",
            "gpt-image-2.5-sunburst",
            "gpt-image-2.5-flare",
        ] {
            assert!(
                ManagedModelsRepository::get(storage.connection(), slug)
                    .await
                    .unwrap_or_else(|error| panic!("read new model {slug}: {error}"))
                    .is_some(),
                "missing new model {slug}"
            );
        }
    }

    #[tokio::test]
    async fn revision8_upgrade_preserves_changed_metadata_pricing_tiers_and_routes() {
        let storage = schema_only().await;
        seed_previous_catalog(storage.connection()).await;

        let mut metadata = ModelCatalogRepository::find_by_slug(storage.connection(), "gpt-5.4")
            .await
            .expect("read metadata model")
            .expect("metadata model exists");
        metadata.display_name = "Locally edited GPT-5.4".into();
        ModelCatalogRepository::put(storage.connection(), metadata)
            .await
            .expect("edit retired metadata");

        let price_model = model(storage.connection(), "gpt-5.4-mini").await;
        let mut price = ModelPricesRepository::get(storage.connection(), &price_model.id)
            .await
            .expect("read price")
            .expect("price exists");
        price.price.input_microusd_per_1m =
            price.price.input_microusd_per_1m.map(|value| value + 1);
        ModelPricesRepository::put(storage.connection(), price)
            .await
            .expect("edit retired price");
        let mut tiers =
            ModelPriceTiersRepository::list_for_model(storage.connection(), &price_model.id)
                .await
                .expect("read tiers");
        tiers[0].tier.input_microusd_per_1m += 1;
        ModelPriceTiersRepository::replace_for_model(storage.connection(), &price_model.id, &tiers)
            .await
            .expect("edit retired tiers");

        let routed = model(storage.connection(), "gpt-5.2").await;
        ModelRoutesRepository::put(
            storage.connection(),
            CatalogRouteRecord {
                model_id: routed.id.clone(),
                route: ModelRouteV2 {
                    id: "route:retired-custom".into(),
                    source_kind: "aggregate_api".into(),
                    source_id: "retired-provider".into(),
                    upstream_model: "vendor-gpt-5.2".into(),
                    enabled: true,
                    priority: 7,
                    weight: 2,
                },
                created_at: 8,
                updated_at: 8,
            },
        )
        .await
        .expect("add custom retired route");

        reconcile_builtin_catalog(storage.connection())
            .await
            .expect("upgrade changed catalog");

        let metadata = model(storage.connection(), "gpt-5.4").await;
        assert_eq!(metadata.display_name, "Locally edited GPT-5.4");
        let priced = model(storage.connection(), "gpt-5.4-mini").await;
        assert_eq!(
            priced.price.input_microusd_per_1m,
            price_model
                .price
                .input_microusd_per_1m
                .map(|value| value + 1)
        );
        assert_eq!(
            priced.price_tiers[0].input_microusd_per_1m,
            tiers[0].tier.input_microusd_per_1m
        );
        let routed = model(storage.connection(), "gpt-5.2").await;
        assert!(routed
            .routes
            .iter()
            .any(|route| route.id == "route:retired-custom"));
        for preserved in [metadata, priced, routed] {
            assert_eq!(preserved.origin, "custom");
            assert!(preserved.user_edited);
            assert_eq!(preserved.builtin_revision, None);
            assert!(!preserved.routes.iter().any(|route| {
                route.source_kind == "account_pool"
                    && route.source_id == "default"
                    && route.upstream_model.eq_ignore_ascii_case(&preserved.slug)
            }));
        }
    }

    #[tokio::test]
    async fn revision8_upgrade_preserves_disabled_group_and_api_key_references() {
        let storage = schema_only().await;
        seed_previous_catalog(storage.connection()).await;

        ModelGroupsRepository::upsert(
            storage.connection(),
            ModelGroupRecord {
                id: "retired-disabled-group".into(),
                name: "Retired access".into(),
                description: None,
                status: "active".into(),
                sort: 0,
                is_default: false,
                rate_multiplier_millis: 1_000,
                created_at: 8,
                updated_at: 8,
            },
        )
        .await
        .expect("create model group");
        let grouped = model(storage.connection(), "gpt-5.4").await;
        crate::model_groups::group_models_v2::Entity::insert(
            crate::model_groups::group_models_v2::ActiveModel {
                group_id: Set("retired-disabled-group".into()),
                model_id: Set(grouped.id),
                enabled: Set(false),
                rate_multiplier_millis: Set(None),
                created_at: Set(8),
                updated_at: Set(8),
            },
        )
        .exec(storage.connection())
        .await
        .expect("add disabled group reference");

        ApiKeysRepository::upsert(
            storage.connection(),
            ApiKeyRecord {
                id: "retired-model-key".into(),
                name: Some("Retired key".into()),
                model_slug: Some(" GPT-5.4-MINI ".into()),
                reasoning_effort: None,
                service_tier: None,
                rotation_strategy: "account_rotation".into(),
                aggregate_api_id: None,
                account_plan_filter: None,
                account_group_filter: None,
                client_type: "codex".into(),
                protocol_type: "openai_compat".into(),
                auth_scheme: "authorization_bearer".into(),
                upstream_base_url: None,
                static_headers_json: None,
                key_hash: "retired-model-key-hash".into(),
                status: "disabled".into(),
                created_at: 8,
                last_used_at: None,
            },
        )
        .await
        .expect("add API key model reference");
        api_key_profiles::Entity::insert(api_key_profiles::ActiveModel {
            key_id: Set("retired-profile-key".into()),
            client_type: Set("codex".into()),
            protocol_type: Set("openai_compat".into()),
            auth_scheme: Set("authorization_bearer".into()),
            upstream_base_url: Set(None),
            static_headers_json: Set(None),
            default_model: Set(Some(" GPT-5.2 ".into())),
            reasoning_effort: Set(None),
            service_tier: Set(None),
            created_at: Set(8),
            updated_at: Set(8),
        })
        .exec(storage.connection())
        .await
        .expect("add API key profile reference");

        reconcile_builtin_catalog(storage.connection())
            .await
            .expect("upgrade referenced catalog");
        for slug in RETIRED_REVISION9_SLUGS {
            let preserved = model(storage.connection(), slug).await;
            assert_eq!(preserved.origin, "custom");
            assert!(preserved.user_edited);
            assert_eq!(preserved.builtin_revision, None);
        }
    }

    #[tokio::test]
    async fn custom_slug_collision_and_future_revision_are_not_overwritten() {
        let storage = schema_only().await;
        let seed = latest_fixture()
            .models
            .into_iter()
            .find(|seed| seed.slug == "gpt-6-sol")
            .expect("latest Sol seed");
        let now = 10;
        let id = "custom:gpt-6-sol";
        ModelCatalogRepository::put(
            storage.connection(),
            CatalogModelRecord {
                id: id.into(),
                slug: seed.slug.clone(),
                display_name: "Private GPT-6-Sol".into(),
                description: Some("Custom route owner".into()),
                provider: Some("private".into()),
                family: None,
                category: None,
                origin: "custom".into(),
                enabled: true,
                supported_in_api: true,
                visibility: "list".into(),
                sort_order: 1,
                context_window: Some(1_000),
                max_context_window: Some(1_000),
                default_reasoning_effort: Some("low".into()),
                instructions_mode: "passthrough".into(),
                instructions_text: None,
                builtin_revision: None,
                user_edited: true,
                created_at: now,
                updated_at: now,
                tags: vec!["private".into()],
                capabilities: serde_json::json!({"private": true}),
                fast_policy: ModelFastPolicyV2::Passthrough,
            },
        )
        .await
        .expect("insert custom collision");
        replace_seed_pricing(storage.connection(), id, &seed, now, now)
            .await
            .expect("insert custom collision price");

        reconcile_builtin_catalog(storage.connection())
            .await
            .expect("reconcile custom collision");
        let custom = model(storage.connection(), "gpt-6-sol").await;
        assert_eq!(custom.origin, "custom");
        assert_eq!(custom.display_name, "Private GPT-6-Sol");
        assert_eq!(custom.capabilities, serde_json::json!({"private": true}));

        let mut future = ModelCatalogRepository::find_by_slug(storage.connection(), "gpt-6-astra")
            .await
            .expect("read future model")
            .expect("future model exists");
        future.display_name = "Future Astra".into();
        future.builtin_revision = Some(11);
        ModelCatalogRepository::put(storage.connection(), future)
            .await
            .expect("simulate future catalog");
        set_meta(storage.connection(), "builtin_revision", "11")
            .await
            .expect("store future marker");
        reconcile_builtin_catalog(storage.connection())
            .await
            .expect("skip future catalog");
        assert_eq!(
            model(storage.connection(), "gpt-6-astra")
                .await
                .display_name,
            "Future Astra"
        );
        assert_eq!(
            meta_value(storage.connection(), "builtin_revision")
                .await
                .expect("read future marker")
                .as_deref(),
            Some("11")
        );
    }

    #[tokio::test]
    async fn failed_reconciliation_rolls_back_models_and_marker() {
        let storage = schema_only().await;
        ModelCatalogRepository::put(
            storage.connection(),
            CatalogModelRecord {
                id: "builtin:gpt-6-luna".into(),
                slug: "id-collision".into(),
                display_name: "ID collision".into(),
                description: None,
                provider: None,
                family: None,
                category: None,
                origin: "custom".into(),
                enabled: true,
                supported_in_api: true,
                visibility: "list".into(),
                sort_order: 1,
                context_window: None,
                max_context_window: None,
                default_reasoning_effort: None,
                instructions_mode: "passthrough".into(),
                instructions_text: None,
                builtin_revision: None,
                user_edited: true,
                created_at: 1,
                updated_at: 1,
                tags: Vec::new(),
                capabilities: serde_json::json!({}),
                fast_policy: ModelFastPolicyV2::Passthrough,
            },
        )
        .await
        .expect("insert collision owner");

        let error = reconcile_builtin_catalog(storage.connection())
            .await
            .expect_err("ID collision must fail reconciliation");
        assert!(error.to_string().contains("already owned"));
        assert!(
            ManagedModelsRepository::get(storage.connection(), "gpt-6-sol")
                .await
                .expect("read rolled back model")
                .is_none()
        );
        assert!(meta_value(storage.connection(), "builtin_revision")
            .await
            .expect("read rolled back marker")
            .is_none());
        assert!(
            ModelCatalogRepository::find_by_slug(storage.connection(), "id-collision")
                .await
                .expect("read collision owner")
                .is_some()
        );
    }
}
