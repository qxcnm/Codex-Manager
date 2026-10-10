use super::*;

fn revision9_storage() -> Storage {
    let storage = Storage::open_in_memory().unwrap();
    storage.init().unwrap();
    storage
        .conn
        .execute("DELETE FROM models WHERE slug=?1", [GPT61_SOL_SLUG])
        .unwrap();
    storage
        .conn
        .execute(
            "DELETE FROM schema_migrations WHERE version=?1",
            [MODEL_CATALOG_REVISION10_MIGRATION_VERSION],
        )
        .unwrap();
    storage
        .conn
        .execute(
            "UPDATE model_catalog_v2_meta SET value='9' WHERE key='builtin_revision'",
            [],
        )
        .unwrap();
    storage
        .conn
        .execute(
            "UPDATE model_catalog_v2_meta SET value=?1 WHERE key='fixture_sha256'",
            [revision9_fixture().source_sha256],
        )
        .unwrap();
    storage.migration_cache().take();
    storage
}

#[test]
fn revision10_adds_sol61_without_changing_revision9_models() {
    let storage = revision9_storage();
    let before = serde_json::to_value(storage.list_managed_models_v2(true).unwrap()).unwrap();
    storage.init().unwrap();
    let model = storage
        .get_managed_model_v2(GPT61_SOL_SLUG)
        .unwrap()
        .unwrap();
    assert!(model.enabled && model.supported_in_api);
    assert_eq!(model.builtin_revision, Some(10));
    assert_eq!(
        model.capabilities["service_tiers"],
        serde_json::json!(["priority"])
    );
    assert_eq!(
        model.capabilities["tool_calling_endpoints"],
        serde_json::json!(["/v1/responses"])
    );
    assert_eq!(model.capabilities["api_context_window"], 1_050_000);
    assert_eq!(model.capabilities["max_output_tokens"], 128_000);
    assert_eq!(model.routes[0].source_kind, "account_pool");
    assert_eq!(model.routes[0].upstream_model, GPT61_SOL_SLUG);
    // Check the inclusive short-context boundary and the first long-context token.
    let (_, short) = storage
        .select_model_price_tier_v2(GPT61_SOL_SLUG, 272_000)
        .unwrap()
        .unwrap();
    let (_, long) = storage
        .select_model_price_tier_v2(GPT61_SOL_SLUG, 272_001)
        .unwrap()
        .unwrap();
    assert_eq!(short.cached_input_microusd_per_1m, 100_000);
    assert_eq!(long.cached_input_microusd_per_1m, 200_000);
    assert_eq!(short.output_microusd_per_1m, 10_000_000);
    assert_eq!(long.output_microusd_per_1m, 15_000_000);
    storage.init().unwrap();
    let after: Vec<_> = storage
        .list_managed_models_v2(true)
        .unwrap()
        .into_iter()
        .filter(|m| m.slug != GPT61_SOL_SLUG)
        .collect();
    assert_eq!(serde_json::to_value(after).unwrap(), before);
    let hash: String = storage
        .conn
        .query_row(
            "SELECT value FROM model_catalog_v2_meta WHERE key='fixture_sha256'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hash, fixture().source_sha256);
}

#[test]
fn revision10_preserves_existing_sol61_custom_edits_and_tombstones() {
    for custom in [false, true] {
        let storage = revision9_storage();
        seed_missing_with_fixture(&storage.conn, &fixture()).unwrap();
        let mut model = storage
            .get_managed_model_v2(GPT61_SOL_SLUG)
            .unwrap()
            .unwrap();
        model.user_edited = true;
        model.enabled = false;
        model.display_name = "My Sol".into();
        model.price.price_status = "custom".into();
        model.price.input_microusd_per_1m = Some(123_456);
        model.price_tiers.truncate(1);
        model.price_tiers[0].input_microusd_per_1m = 123_456;
        model.routes.clear();
        if custom {
            storage
                .conn
                .execute("DELETE FROM models WHERE slug=?1", [GPT61_SOL_SLUG])
                .unwrap();
            model.id.clear();
            model.origin = "custom".into();
            model.builtin_revision = None;
        }
        let saved = storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                model,
                ..Default::default()
            })
            .unwrap();
        storage.init().unwrap();
        assert_eq!(
            serde_json::to_value(
                storage
                    .get_managed_model_v2(GPT61_SOL_SLUG)
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }
    let storage = revision9_storage();
    seed_missing_with_fixture(&storage.conn, &fixture()).unwrap();
    storage.delete_managed_model_v2(GPT61_SOL_SLUG).unwrap();
    storage.init().unwrap();
    assert!(storage
        .get_managed_model_v2(GPT61_SOL_SLUG)
        .unwrap()
        .is_none());
}

#[test]
fn revision10_does_not_seed_into_future_catalog() {
    let storage = revision9_storage();
    storage
        .conn
        .execute(
            "UPDATE models SET builtin_revision=11 WHERE slug='gpt-6-astra'",
            [],
        )
        .unwrap();
    storage.init().unwrap();
    assert!(storage
        .get_managed_model_v2(GPT61_SOL_SLUG)
        .unwrap()
        .is_none());
    let revision: String = storage
        .conn
        .query_row(
            "SELECT value FROM model_catalog_v2_meta WHERE key='builtin_revision'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(revision, "11");
}

#[test]
fn revision10_failed_insert_rolls_back_migration_markers() {
    let storage = revision9_storage();
    storage.conn.execute("CREATE TRIGGER reject_sol61 BEFORE INSERT ON models WHEN NEW.slug='gpt-6.1-sol' BEGIN SELECT RAISE(ABORT, 'test insert failure'); END;", []).unwrap();
    assert!(storage.apply_model_catalog_revision10_migration().is_err());
    assert!(!storage
        .has_migration(MODEL_CATALOG_REVISION10_MIGRATION_VERSION)
        .unwrap());
    let revision: String = storage
        .conn
        .query_row(
            "SELECT value FROM model_catalog_v2_meta WHERE key='builtin_revision'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(revision, "9");
    assert!(storage
        .get_managed_model_v2(GPT61_SOL_SLUG)
        .unwrap()
        .is_none());
}

#[test]
fn revision10_fixture_has_verified_hash_and_no_prompts() {
    let raw = include_str!("../../seeds/model_catalog_v2_2026_09_30.json");
    let value: Value = serde_json::from_str(raw).unwrap();
    assert_eq!(
        value["source_sha256"],
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&value["models"]).unwrap())
        )
    );
    for forbidden in [
        "base_instructions",
        "instructions_template",
        "instructions_text",
    ] {
        assert!(!raw.contains(forbidden));
    }
}
