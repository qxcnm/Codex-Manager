use super::*;
use codexmanager_core::storage::{ModelPriceTierV2, Storage};

fn api() -> AggregateApi {
    AggregateApi {
        id: "discovery-source".into(),
        provider_type: "compatible".into(),
        supplier_name: Some("Discovery fixture".into()),
        sort: 0,
        url: "https://provider.invalid/v1".into(),
        auth_type: "apikey".into(),
        auth_params_json: None,
        action: None,
        model_override: None,
        user_agent: None,
        status: "active".into(),
        created_at: 0,
        updated_at: 0,
        last_test_at: None,
        last_test_status: None,
        last_test_error: None,
        balance_query_enabled: false,
        balance_query_template: None,
        balance_query_base_url: None,
        balance_query_user_id: None,
        balance_query_config_json: None,
        last_balance_at: None,
        last_balance_status: None,
        last_balance_error: None,
        last_balance_json: None,
    }
}

fn custom_model(slug: &str) -> ManagedModelV2 {
    ManagedModelV2 {
        slug: slug.into(),
        display_name: "Original name".into(),
        origin: "custom".into(),
        enabled: true,
        supported_in_api: true,
        visibility: "list".into(),
        instructions_mode: "passthrough".into(),
        price: ModelPriceV2 {
            price_status: "missing".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn save(storage: &Storage, model: ManagedModelV2) {
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            model,
            ..Default::default()
        })
        .expect("save fixture model");
}

#[test]
fn discovery_does_not_overwrite_edits_made_after_catalog_read() {
    let storage = Storage::open_in_memory().unwrap();
    storage.init().unwrap();
    let slug = "discovery-existing-model";
    save(&storage, custom_model(slug));
    let snapshot = storage.list_managed_models_v2(true).unwrap();
    let plan = prepare(&api(), &snapshot, vec![slug.into()], &BTreeMap::new()).unwrap();

    let mut edited = storage.get_managed_model_v2(slug).unwrap().unwrap();
    edited.display_name = "Manually edited".into();
    edited.enabled = false;
    edited.sort_order = 900;
    edited.price.price_status = "custom".into();
    edited.price.input_microusd_per_1m = Some(123456);
    edited.price.cached_input_microusd_per_1m = Some(12345);
    edited.price.output_microusd_per_1m = Some(234567);
    edited.price_tiers = vec![ModelPriceTierV2 {
        min_input_tokens: 0,
        input_microusd_per_1m: 123456,
        cached_input_microusd_per_1m: 12345,
        cache_write_microusd_per_1m: None,
        output_microusd_per_1m: 234567,
    }];
    edited.routes.push(ModelRouteV2 {
        source_kind: "aggregate_api".into(),
        source_id: "another-source".into(),
        upstream_model: "manual-upstream".into(),
        enabled: false,
        weight: 3,
        priority: 11,
        ..Default::default()
    });
    save(&storage, edited.clone());

    let result = storage
        .upsert_missing_managed_models_and_ensure_routes_v2(&plan.candidates, &plan.routes)
        .unwrap();
    assert!(result.created_models.is_empty());
    assert_eq!(result.added_routes, [slug]);
    let mut current = storage.get_managed_model_v2(slug).unwrap().unwrap();
    assert_eq!(current.display_name, edited.display_name);
    assert_eq!(current.enabled, edited.enabled);
    assert_eq!(current.sort_order, edited.sort_order);
    assert_eq!(current.price, edited.price);
    assert_eq!(current.price_tiers, edited.price_tiers);
    assert_eq!(current.routes.len(), 2);
    assert!(current
        .routes
        .iter()
        .any(|route| route.source_id == "another-source" && !route.enabled));

    let route = current
        .routes
        .iter_mut()
        .find(|route| route.source_id == api().id)
        .unwrap();
    route.enabled = false;
    route.priority = 17;
    route.weight = 4;
    save(&storage, current);
    let repeated = storage
        .upsert_missing_managed_models_and_ensure_routes_v2(&plan.candidates, &plan.routes)
        .unwrap();
    assert!(repeated.created_models.is_empty());
    assert!(repeated.added_routes.is_empty());
    assert_eq!(repeated.unchanged_routes, [slug]);
    let current = storage.get_managed_model_v2(slug).unwrap().unwrap();
    let route = current
        .routes
        .iter()
        .find(|route| route.source_id == api().id)
        .unwrap();
    assert!(!route.enabled);
    assert_eq!((route.priority, route.weight), (17, 4));
}

#[test]
fn discovery_preserves_a_model_created_concurrently_and_deduplicates_ids() {
    let storage = Storage::open_in_memory().unwrap();
    storage.init().unwrap();
    let slug = "discovery-raced-model";
    let plan = prepare(
        &api(),
        &[],
        vec![slug.into(), slug.to_uppercase()],
        &BTreeMap::from([(slug.into(), "Discovered name".into())]),
    )
    .unwrap();
    assert_eq!(plan.candidates.len(), 1);
    assert_eq!(plan.routes.len(), 1);

    let mut manual = custom_model(slug);
    manual.enabled = false;
    manual.display_name = "Created manually meanwhile".into();
    save(&storage, manual.clone());
    let result = storage
        .upsert_missing_managed_models_and_ensure_routes_v2(&plan.candidates, &plan.routes)
        .unwrap();
    assert!(result.created_models.is_empty());
    assert_eq!(result.added_routes, [slug]);
    let current = storage.get_managed_model_v2(slug).unwrap().unwrap();
    assert_eq!(current.display_name, manual.display_name);
    assert!(!current.enabled);
}

#[test]
fn discovery_normalizes_gemini_names_and_validates_entire_batch() {
    let mut api = api();
    api.provider_type = "gemini".into();
    let plan = prepare(
        &api,
        &[],
        vec!["models/gemini-test".into()],
        &BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(plan.candidates[0].model.slug, "gemini-test");
    assert_eq!(plan.routes[0].route.upstream_model, "gemini-test");
    assert!(prepare(
        &api,
        &[],
        vec!["valid".into(), "bad model".into()],
        &BTreeMap::new()
    )
    .is_err());
}
