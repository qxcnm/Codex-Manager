use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn test_rule(
    id: &str,
    model_pattern: &str,
    match_type: &str,
    priority: i64,
    input: f64,
    cached: Option<f64>,
    output: f64,
) -> ModelPriceRule {
    ModelPriceRule {
        id: id.to_string(),
        provider: "test".to_string(),
        model_pattern: model_pattern.to_string(),
        match_type: match_type.to_string(),
        billing_mode: "standard".to_string(),
        currency: "USD".to_string(),
        unit: "per_1m_tokens".to_string(),
        input_price_per_1m: Some(input),
        cached_input_price_per_1m: cached,
        output_price_per_1m: Some(output),
        reasoning_output_price_per_1m: None,
        cache_write_5m_price_per_1m: None,
        cache_write_1h_price_per_1m: None,
        cache_hit_price_per_1m: None,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source: "test".to_string(),
        source_url: None,
        seed_version: None,
        enabled: true,
        priority,
        created_at: 0,
        updated_at: 0,
    }
}

fn assert_close(actual: f64, expected: f64) {
    let delta = (actual - expected).abs();
    assert!(
        delta < 0.000_000_1,
        "expected {expected}, got {actual}, delta {delta}"
    );
}

fn isolated_test_db_path(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut path = std::env::temp_dir();
    path.push(format!(
        "codexmanager-{name}-{}-{nanos}.sqlite",
        std::process::id()
    ));
    path.to_string_lossy().into_owned()
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.previous.as_deref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
        invalidate_price_rule_cache();
    }
}

#[test]
fn resolves_exact_and_wildcard_database_rules() {
    let rules = vec![
        test_rule("wild", "vendor-*-mini", "wildcard", 10, 1.0, Some(0.1), 2.0),
        test_rule(
            "exact",
            "vendor-model-mini",
            "exact",
            100,
            3.0,
            Some(0.3),
            4.0,
        ),
    ];
    let exact = resolve_model_price_from_rules(&rules, "vendor-model-mini", 0).expect("exact rule");
    assert_close(exact.input_price_per_1m, 3.0);
    assert_close(exact.cached_input_price_per_1m, 0.3);
    assert_close(exact.output_price_per_1m, 4.0);

    let wildcard =
        resolve_model_price_from_rules(&rules, "vendor-other-mini", 0).expect("wildcard rule");
    assert_close(wildcard.input_price_per_1m, 1.0);
    assert_close(wildcard.output_price_per_1m, 2.0);
}

#[test]
fn resolves_exact_and_snapshot_models() {
    let exact = resolve_model_price("gpt-5.4-mini", 0).expect("exact price");
    assert_eq!(exact.provider, "openai");
    assert_close(exact.input_price_per_1m, 0.75);
    assert_close(exact.cached_input_price_per_1m, 0.075);
    assert_close(exact.output_price_per_1m, 4.5);

    let snapshot = resolve_model_price("gpt-5.4-mini-2026-03-17", 0).expect("snapshot price");
    assert_close(snapshot.input_price_per_1m, 0.75);
    assert_close(snapshot.output_price_per_1m, 4.5);
}

#[test]
fn prefers_more_specific_prefix_for_latest_claude_opus() {
    let latest = resolve_model_price("claude-opus-4.7-20260219", 0).expect("latest opus price");
    assert_eq!(latest.provider, "anthropic");
    assert_close(latest.input_price_per_1m, 5.0);
    assert_close(latest.cached_input_price_per_1m, 0.5);
    assert_close(latest.output_price_per_1m, 25.0);

    let legacy = resolve_model_price("claude-opus-4-20250514", 0).expect("opus 4 price");
    assert_close(legacy.input_price_per_1m, 15.0);
    assert_close(legacy.output_price_per_1m, 75.0);
}

#[test]
fn runtime_provider_namespaces_reuse_official_upstream_prices() {
    let direct = resolve_model_price("claude-sonnet-4.5", 0).expect("direct Claude price");
    let kiro = resolve_model_price("kiro/claude-sonnet-4.5", 0).expect("Kiro Claude price");
    assert_eq!(kiro.provider, direct.provider);
    assert_close(kiro.input_price_per_1m, direct.input_price_per_1m);
    assert_close(kiro.output_price_per_1m, direct.output_price_per_1m);
}

#[test]
fn returns_missing_for_unknown_models() {
    assert!(resolve_model_price("unknown-provider-model", 0).is_none());
    let cost = estimate_cost(Some("unknown-provider-model"), 100, 0, 100);
    assert_eq!(cost.price_status, "missing");
    assert!(cost.cost_usd.is_none());
    assert!(cost.provider.is_none());
}

#[test]
fn zero_usd_balance_is_known_zero_tokens() {
    let tokens = estimate_remaining_tokens_from_usd_with_rules(&[], "gpt-5.4-mini", 0.0);
    assert_eq!(tokens, Some(0));
}

#[test]
fn estimates_cost_with_cached_input_discount() {
    let cost = estimate_cost(Some("gpt-5.4"), 1_000, 400, 100);
    assert_eq!(cost.price_status, "ok");
    assert_eq!(cost.provider.as_deref(), Some("openai"));
    assert_close(cost.cost_usd.expect("cost"), 0.0031);
}

#[test]
fn falls_back_cached_input_to_input_price_when_no_discount_exists() {
    let cost = estimate_cost(Some("gpt-5.5-pro"), 1_000, 200, 100);
    assert_eq!(cost.price_status, "ok");
    assert_close(cost.cost_usd.expect("cost"), 0.048);
}

#[test]
fn applies_openai_long_context_pricing_at_threshold() {
    let standard = resolve_model_price("gpt-5.4", 271_999).expect("standard price");
    assert_close(standard.input_price_per_1m, 2.5);
    assert_close(standard.output_price_per_1m, 15.0);

    let long_context = resolve_model_price("gpt-5.4", 272_000).expect("long context price");
    assert_close(long_context.input_price_per_1m, 5.0);
    assert_close(long_context.cached_input_price_per_1m, 0.5);
    assert_close(long_context.output_price_per_1m, 22.5);
}

#[test]
fn estimate_cost_usd_for_log_reuses_cached_enabled_price_rules_until_invalidated() {
    let _lock = crate::test_env_guard();
    invalidate_price_rule_cache();
    let _guard = EnvGuard::set(
        "CODEXMANAGER_DB_PATH",
        &isolated_test_db_path("price-cache-test"),
    );
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let mut rule = test_rule(
        "cached-rule",
        "cache-model",
        "exact",
        50_000,
        1.0,
        Some(1.0),
        1.0,
    );
    rule.created_at = now;
    rule.updated_at = now;
    storage
        .upsert_model_price_rule(&rule)
        .expect("insert first price rule");

    let first =
        estimate_cost_usd_for_log(&storage, Some("cache-model"), Some(1_000_000), None, None);
    assert_close(first, 1.0);

    rule.input_price_per_1m = Some(2.0);
    rule.cached_input_price_per_1m = Some(2.0);
    rule.updated_at = now + 1;
    storage
        .upsert_model_price_rule(&rule)
        .expect("update price rule");

    let cached =
        estimate_cost_usd_for_log(&storage, Some("cache-model"), Some(1_000_000), None, None);
    assert_close(cached, 1.0);

    invalidate_price_rule_cache();
    let refreshed =
        estimate_cost_usd_for_log(&storage, Some("cache-model"), Some(1_000_000), None, None);
    assert_close(refreshed, 2.0);
}
