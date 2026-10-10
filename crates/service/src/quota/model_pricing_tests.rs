use super::*;
use codexmanager_core::storage::{ManagedModelV2Upsert, Storage};

fn assert_close(actual: f64, expected: f64) {
    let delta = (actual - expected).abs();
    assert!(
        delta < 0.000_000_1,
        "expected {expected}, got {actual}, delta {delta}"
    );
}

fn prices() -> (Storage, Vec<CatalogModelPrice>) {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let prices = load_catalog_prices(&storage).expect("load V2 prices");
    (storage, prices)
}

#[test]
fn catalog_prices_are_exact_and_missing_prices_do_not_fallback() {
    let (_storage, prices) = prices();
    assert_eq!(prices.len(), 11);
    let astra = resolve_model_price_from_catalog(&prices, "gpt-6-astra", 0).expect("astra");
    assert_close(astra.input_price_per_1m, 10.0);
    assert_close(astra.cached_input_price_per_1m, 1.0);
    assert_close(astra.cache_write_price_per_1m, 12.5);
    assert_close(astra.output_price_per_1m, 50.0);
    let astra_long =
        resolve_model_price_from_catalog(&prices, "gpt-6-astra", 272_001).expect("astra long");
    assert_close(astra_long.input_price_per_1m, 20.0);
    assert_close(astra_long.cached_input_price_per_1m, 2.0);
    assert_close(astra_long.cache_write_price_per_1m, 25.0);
    assert_close(astra_long.output_price_per_1m, 75.0);
    let sol6 = resolve_model_price_from_catalog(&prices, "gpt-6-sol", 0).expect("GPT-6 Sol");
    assert_close(sol6.input_price_per_1m, 2.0);
    assert_close(sol6.cached_input_price_per_1m, 0.2);
    assert_close(sol6.cache_write_price_per_1m, 2.5);
    assert_close(sol6.output_price_per_1m, 10.0);
    let sol6_long =
        resolve_model_price_from_catalog(&prices, "gpt-6-sol", 272_001).expect("GPT-6 Sol long");
    assert_close(sol6_long.input_price_per_1m, 4.0);
    assert_close(sol6_long.cached_input_price_per_1m, 0.4);
    assert_close(sol6_long.cache_write_price_per_1m, 5.0);
    assert_close(sol6_long.output_price_per_1m, 15.0);
    let luna6 = resolve_model_price_from_catalog(&prices, "gpt-6-luna", 0).expect("GPT-6 Luna");
    assert_close(luna6.input_price_per_1m, 0.1);
    assert_close(luna6.cached_input_price_per_1m, 0.01);
    assert_close(luna6.cache_write_price_per_1m, 0.125);
    assert_close(luna6.output_price_per_1m, 0.5);
    let luna6_long =
        resolve_model_price_from_catalog(&prices, "gpt-6-luna", 272_001).expect("GPT-6 Luna long");
    assert_close(luna6_long.input_price_per_1m, 0.2);
    assert_close(luna6_long.cached_input_price_per_1m, 0.02);
    assert_close(luna6_long.cache_write_price_per_1m, 0.25);
    assert_close(luna6_long.output_price_per_1m, 0.75);
    for retired in ["gpt-5.4", "gpt-5.4-mini", "gpt-5.2"] {
        assert!(resolve_model_price_from_catalog(&prices, retired, 0).is_none());
    }
    let sol = resolve_model_price_from_catalog(&prices, "gpt-5.6-sol", 0).expect("sol");
    assert_close(sol.input_price_per_1m, 5.0);
    assert_close(sol.cached_input_price_per_1m, 0.5);
    assert_close(sol.cache_write_price_per_1m, 6.25);
    assert_close(sol.output_price_per_1m, 30.0);
    let terra = resolve_model_price_from_catalog(&prices, "gpt-5.6-terra", 0).expect("terra");
    assert_close(terra.input_price_per_1m, 2.0);
    assert_close(terra.cached_input_price_per_1m, 0.2);
    assert_close(terra.cache_write_price_per_1m, 2.5);
    assert_close(terra.output_price_per_1m, 12.0);
    let legacy_luna =
        resolve_model_price_from_catalog(&prices, "gpt-5.6-luna", 0).expect("legacy luna");
    assert_close(legacy_luna.input_price_per_1m, 0.2);
    assert_close(legacy_luna.cached_input_price_per_1m, 0.02);
    assert_close(legacy_luna.cache_write_price_per_1m, 0.25);
    assert_close(legacy_luna.output_price_per_1m, 1.2);
    let reserve = resolve_model_price_from_catalog(&prices, "gpt-reserve", 0).expect("reserve");
    assert_close(reserve.input_price_per_1m, luna6.input_price_per_1m);
    assert_close(
        reserve.cached_input_price_per_1m,
        luna6.cached_input_price_per_1m,
    );
    assert_close(
        reserve.cache_write_price_per_1m,
        luna6.cache_write_price_per_1m,
    );
    assert_close(reserve.output_price_per_1m, luna6.output_price_per_1m);
    let image = resolve_model_price_from_catalog(&prices, "gpt-image-2", 0).expect("image");
    assert_close(image.input_price_per_1m, 8.0);
    assert_close(image.cached_input_price_per_1m, 2.0);
    assert_close(image.output_price_per_1m, 30.0);
    for slug in ["gpt-image-2.5-sunburst", "gpt-image-2.5-flare"] {
        assert!(
            resolve_model_price_from_catalog(&prices, slug, 0).is_none(),
            "{slug} must not expose a unified price for mixed text/image inputs"
        );
    }
    assert!(resolve_model_price_from_catalog(&prices, "codex-auto-review", 0).is_none());
    assert!(resolve_model_price_from_catalog(&prices, "unknown-provider-model", 0).is_none());
}

#[test]
fn catalog_price_switches_at_272k_boundary() {
    let (_storage, prices) = prices();
    let standard = resolve_model_price_from_catalog(&prices, "gpt-5.5", 271_999).expect("standard");
    assert_close(standard.input_price_per_1m, 5.0);
    assert_close(standard.output_price_per_1m, 30.0);
    let long = resolve_model_price_from_catalog(&prices, "gpt-5.5", 272_000).expect("long");
    assert_close(long.input_price_per_1m, 10.0);
    assert_close(long.cached_input_price_per_1m, 1.0);
    assert_close(long.output_price_per_1m, 45.0);
}

#[test]
fn gpt56_catalog_prices_switch_to_official_long_context_rates() {
    let (_storage, prices) = prices();
    for (slug, base, long_rates) in [
        ("gpt-5.6-sol", (5.0, 0.5, 30.0), (10.0, 1.0, 45.0)),
        ("gpt-5.6-terra", (2.0, 0.2, 12.0), (4.0, 0.4, 18.0)),
        ("gpt-5.6-luna", (0.2, 0.02, 1.2), (0.4, 0.04, 1.8)),
    ] {
        let standard =
            resolve_model_price_from_catalog(&prices, slug, 272_000).expect("standard tier");
        assert_close(standard.input_price_per_1m, base.0);
        assert_close(standard.cached_input_price_per_1m, base.1);
        assert_close(standard.cache_write_price_per_1m, base.0 * 1.25);
        assert_close(standard.output_price_per_1m, base.2);

        let long = resolve_model_price_from_catalog(&prices, slug, 272_001).expect("long tier");
        assert_close(long.input_price_per_1m, long_rates.0);
        assert_close(long.cached_input_price_per_1m, long_rates.1);
        assert_close(long.cache_write_price_per_1m, long_rates.0 * 1.25);
        assert_close(long.output_price_per_1m, long_rates.2);
    }
}

#[test]
fn catalog_cost_uses_cached_subset_once() {
    let (_storage, prices) = prices();
    let cost = estimate_cost_with_catalog(&prices, Some("gpt-6-sol"), 1_000, 400, 100);
    assert_eq!(cost.price_status, "ok");
    assert_eq!(cost.provider.as_deref(), Some("openai"));
    assert_close(cost.cost_usd.expect("cost"), 0.00228);

    let gpt56 = estimate_cost_with_catalog(&prices, Some("gpt-5.6-sol"), 1_000, 400, 100);
    assert_eq!(gpt56.price_status, "ok");
    assert_close(gpt56.cost_usd.expect("GPT-5.6 cost"), 0.0062);
}

#[test]
fn zero_balance_is_known_and_positive_balance_uses_gpt56_price() {
    let (_storage, prices) = prices();
    assert_eq!(
        estimate_remaining_tokens_from_usd_with_catalog(&prices, "gpt-5.6-sol", 0.0),
        Some(0)
    );
    assert_eq!(
        estimate_remaining_tokens_from_usd_with_catalog(&prices, "gpt-5.6-sol", 1.0),
        Some(80_000)
    );
}

#[test]
fn price_edits_are_read_from_db_without_runtime_cache() {
    let (storage, prices) = prices();
    let before = resolve_model_price_from_catalog(&prices, "gpt-6-luna", 0).expect("before");
    assert_close(before.input_price_per_1m, 0.1);

    let mut model = storage
        .get_managed_model_v2("gpt-6-luna")
        .expect("read model")
        .expect("model");
    model.price.price_status = "custom".to_string();
    model.price.input_microusd_per_1m = Some(2_000_000);
    model.price.cached_input_microusd_per_1m = Some(2_000_000);
    model.price_tiers[0].input_microusd_per_1m = 2_000_000;
    model.price_tiers[0].cached_input_microusd_per_1m = 2_000_000;
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some(model.slug.clone()),
            model,
        })
        .expect("update model price");

    let refreshed = load_catalog_prices(&storage).expect("reload V2 prices");
    let after = resolve_model_price_from_catalog(&refreshed, "gpt-6-luna", 0).expect("after");
    assert_close(after.input_price_per_1m, 2.0);
}

#[test]
fn wildcard_match_remains_available_for_billing_multipliers() {
    assert!(wildcard_matches("gpt-*-mini", "gpt-5.4-mini"));
    assert!(!wildcard_matches("gpt-*-mini", "gpt-5.4"));
}
