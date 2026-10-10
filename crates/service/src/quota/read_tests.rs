use super::*;
use codexmanager_core::storage::{
    now_ts, Account, AggregateApi, ApiKey, AppUser, ManagedModelV2Upsert, ModelRouteV2,
    RequestTokenStat, Storage, UsageSnapshotRecord,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{quota::api_key_usage, test_env_guard};

static QUOTA_READ_TEST_DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

fn new_test_dir(prefix: &str) -> PathBuf {
    let seq = QUOTA_READ_TEST_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("{prefix}-{}-{seq}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn account(id: &str, status: &str, now: i64) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: status.to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn usage(account_id: &str, used_percent: f64, now: i64) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: Some(used_percent),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: None,
        secondary_window_minutes: None,
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now,
    }
}

fn aggregate_api(id: &str, balance_json: Option<&str>, now: i64) -> AggregateApi {
    AggregateApi {
        id: id.to_string(),
        provider_type: "openai-compatible".to_string(),
        supplier_name: Some(id.to_string()),
        sort: 0,
        url: format!("https://{id}.example.test/v1"),
        auth_type: "bearer".to_string(),
        auth_params_json: None,
        action: None,
        model_override: None,
        user_agent: None,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_test_at: None,
        last_test_status: None,
        last_test_error: None,
        balance_query_enabled: true,
        balance_query_template: None,
        balance_query_base_url: None,
        balance_query_user_id: None,
        balance_query_config_json: None,
        last_balance_at: Some(now),
        last_balance_status: Some("success".to_string()),
        last_balance_error: None,
        last_balance_json: balance_json.map(str::to_string),
    }
}

fn test_app_user(id: &str, username: &str, now: i64) -> AppUser {
    AppUser {
        id: id.to_string(),
        username: username.to_string(),
        display_name: None,
        password_hash: "hash".to_string(),
        role: "member".to_string(),
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_login_at: None,
    }
}

fn test_api_key(id: &str, now: i64) -> ApiKey {
    ApiKey {
        id: id.to_string(),
        name: Some(id.to_string()),
        model_slug: Some("gpt-5-mini".to_string()),
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: "account_rotation".to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: "codex".to_string(),
        protocol_type: "openai_compat".to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("hash-{id}"),
        status: "active".to_string(),
        created_at: now,
        last_used_at: None,
    }
}

#[test]
fn api_available_model_slugs_preserves_catalog_sort_order() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let models = api_available_model_slugs(&storage).expect("available models");

    assert_eq!(models.len(), 10);
    assert_eq!(
        &models[..4],
        ["gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol"]
    );
    assert!(!models.iter().any(|model| model == "codex-auto-review"));
    for slug in [
        "gpt-image-2",
        "gpt-image-2.5-sunburst",
        "gpt-image-2.5-flare",
    ] {
        assert!(models.iter().any(|model| model == slug));
    }
}

#[test]
fn api_available_model_slugs_does_not_seed_legacy_price_rules() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let models = api_available_model_slugs(&storage).expect("available models");

    assert_eq!(models.len(), 10);
    assert_eq!(
        storage
            .list_enabled_model_price_rules()
            .expect("list legacy price rows")
            .len(),
        0
    );
}

#[test]
fn billing_rule_upsert_validates_user_and_api_key_with_exists_helpers() {
    let _lock = test_env_guard();
    let dir = new_test_dir("billing-rule-upsert");
    let db_path = dir.join("codexmanager.db");
    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let now = now_ts();
    storage
        .insert_app_user(&test_app_user("billing-user", "billing-user", now))
        .expect("insert app user");
    storage
        .insert_api_key(&test_api_key("billing-key", now))
        .expect("insert api key");

    let result = upsert_billing_rule(BillingRuleUpsertInput {
        id: Some("billing-rule".to_string()),
        name: "Billing Rule".to_string(),
        status: Some("active".to_string()),
        priority: Some(1),
        multiplier_millis: 1_500,
        model_pattern: Some("gpt-5-mini".to_string()),
        user_id: Some("billing-user".to_string()),
        api_key_id: Some("billing-key".to_string()),
        ..BillingRuleUpsertInput::default()
    })
    .expect("upsert billing rule");
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].id, "billing-rule");

    let missing_user = upsert_billing_rule(BillingRuleUpsertInput {
        name: "Missing User".to_string(),
        multiplier_millis: 1_000,
        user_id: Some("missing-user".to_string()),
        ..BillingRuleUpsertInput::default()
    })
    .expect_err("missing user should fail");
    assert!(missing_user.contains("计费规则用户不存在"));

    let missing_key = upsert_billing_rule(BillingRuleUpsertInput {
        name: "Missing Key".to_string(),
        multiplier_millis: 1_000,
        api_key_id: Some("missing-key".to_string()),
        ..BillingRuleUpsertInput::default()
    })
    .expect_err("missing api key should fail");
    assert!(missing_key.contains("计费规则 API Key 不存在"));
}

#[test]
fn quota_api_key_usage_limits_model_usage_to_existing_keys() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_api_key(&test_api_key("visible-key", now))
        .expect("insert api key");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("visible-key".to_string()),
            model: Some("gpt-visible".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            estimated_cost_usd: Some(0.01),
            created_at: now,
            ..RequestTokenStat::default()
        })
        .expect("insert visible token stat");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 2,
            key_id: Some("orphan-key".to_string()),
            model: Some("gpt-orphan".to_string()),
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
            estimated_cost_usd: Some(1.0),
            created_at: now,
            ..RequestTokenStat::default()
        })
        .expect("insert orphan token stat");

    let result =
        api_key_usage::read_quota_api_key_usage_with_storage(&storage).expect("read api key usage");

    assert_eq!(result.items.len(), 1);
    let item = &result.items[0];
    assert_eq!(item.key_id, "visible-key");
    assert_eq!(item.used_tokens, 15);
    assert_eq!(item.models.len(), 1);
    assert_eq!(item.models[0].model, "gpt-visible");

    let overview = read_quota_overview_with_storage(&storage).expect("read quota overview");
    assert_eq!(overview.api_key.key_count, 1);
    assert_eq!(overview.api_key.total_used_tokens, 15);
    assert_eq!(overview.api_key.estimated_cost_usd, 0.01);
}

#[test]
fn quota_api_key_usage_empty_keys_returns_empty() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");

    let result =
        api_key_usage::read_quota_api_key_usage_with_storage(&storage).expect("read api key usage");

    assert!(result.items.is_empty());
}

#[test]
fn quota_capacity_updates_return_config_from_same_storage_handle() {
    let _lock = test_env_guard();
    let dir = new_test_dir("quota-capacity-update");
    let db_path = dir.join("codexmanager.db");
    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    let _guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let now = now_ts();
    storage
        .insert_account(&account("acc-capacity", "active", now))
        .expect("insert account");

    let templated = update_account_quota_capacity_template("pro", Some(10_000), Some(20_000))
        .expect("update capacity template");
    let pro_template = templated
        .templates
        .iter()
        .find(|item| item.plan_type == "pro")
        .expect("pro template returned");
    assert_eq!(pro_template.primary_window_tokens, Some(10_000));
    assert_eq!(pro_template.secondary_window_tokens, Some(20_000));

    let overridden =
        update_account_quota_capacity_override("acc-capacity", Some(30_000), Some(40_000))
            .expect("update capacity override");
    let override_item = overridden
        .account_overrides
        .iter()
        .find(|item| item.account_id == "acc-capacity")
        .expect("account override returned");
    assert_eq!(override_item.primary_window_tokens, Some(30_000));
    assert_eq!(override_item.secondary_window_tokens, Some(40_000));
}

#[test]
fn quota_overview_low_quota_counts_only_available_accounts() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_account(&account("acc-active-low", "active", now))
        .expect("insert active account");
    storage
        .insert_account(&account("acc-disabled-low", "disabled", now))
        .expect("insert disabled account");
    storage
        .insert_usage_snapshot(&usage("acc-active-low", 90.0, now))
        .expect("insert active usage");
    storage
        .insert_usage_snapshot(&usage("acc-disabled-low", 10.0, now))
        .expect("insert disabled usage");

    let result = read_quota_overview_with_storage(&storage).expect("read quota overview");

    assert_eq!(result.openai_account.account_count, 2);
    assert_eq!(result.openai_account.available_count, 1);
    assert_eq!(result.openai_account.low_quota_count, 1);
    assert_eq!(result.openai_account.primary_remain_percent, Some(10));
}

#[test]
fn quota_model_usage_counts_only_available_accounts() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_account(&account("acc-active-model", "active", now))
        .expect("insert active account");
    storage
        .insert_account(&account("acc-disabled-model", "disabled", now))
        .expect("insert disabled account");
    storage
        .insert_usage_snapshot(&usage("acc-active-model", 25.0, now))
        .expect("insert active usage");
    storage
        .insert_usage_snapshot(&usage("acc-disabled-model", 1.0, now))
        .expect("insert disabled usage");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-model".to_string()),
            account_id: Some("acc-active-model".to_string()),
            model: Some("gpt-test".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            created_at: now,
            ..RequestTokenStat::default()
        })
        .expect("insert token stat");

    let result =
        read_quota_model_usage_with_storage(&storage, None, None).expect("read model usage");

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].model, "gpt-test");
    assert_eq!(result.items[0].openai_available_account_count, 1);
    assert_eq!(result.items[0].openai_primary_remain_percent, Some(75));
}

#[test]
fn quota_model_usage_reads_aggregate_balance_from_balance_snapshots_only() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_aggregate_api(&aggregate_api(
            "agg-valid-a",
            Some(r#"{"remaining":2.5,"unit":"USD"}"#),
            now,
        ))
        .expect("insert valid aggregate api a");
    storage
        .insert_aggregate_api(&aggregate_api(
            "agg-valid-b",
            Some(r#"{"remaining":1.5,"unit":"USD"}"#),
            now,
        ))
        .expect("insert valid aggregate api b");
    storage
        .insert_aggregate_api(&aggregate_api("agg-invalid", Some("not-json"), now))
        .expect("insert invalid aggregate api");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-model".to_string()),
            account_id: None,
            model: Some("gpt-6-luna".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            created_at: now,
            ..RequestTokenStat::default()
        })
        .expect("insert token stat");

    let result =
        read_quota_model_usage_with_storage(&storage, None, None).expect("read model usage");

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].aggregate_balance_usd, Some(4.0));
    assert!(result.items[0]
        .aggregate_estimated_remaining_tokens
        .is_some());
}

#[test]
fn quota_model_usage_preserves_known_zero_aggregate_balance() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_aggregate_api(&aggregate_api(
            "agg-zero",
            Some(r#"{"remaining":0.0,"unit":"USD"}"#),
            now,
        ))
        .expect("insert zero balance aggregate api");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 1,
            key_id: Some("key-model".to_string()),
            account_id: None,
            model: Some("gpt-5-mini".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_tokens: Some(15),
            created_at: now,
            ..RequestTokenStat::default()
        })
        .expect("insert token stat");

    let overview = read_quota_overview_with_storage(&storage).expect("read quota overview");
    let model_usage =
        read_quota_model_usage_with_storage(&storage, None, None).expect("read model usage");

    assert_eq!(overview.aggregate_api.total_balance_usd, Some(0.0));
    assert_eq!(model_usage.items.len(), 1);
    assert_eq!(model_usage.items[0].aggregate_balance_usd, Some(0.0));
    assert_eq!(
        model_usage.items[0].aggregate_estimated_remaining_tokens,
        Some(0)
    );
}

#[test]
fn quota_source_list_uses_account_summaries_and_v2_routes() {
    let mut storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let mut account = account("acc-source", "active", now);
    account.label = "Source Account".to_string();
    account.issuer = "ignored-issuer".to_string();
    account.workspace_id = Some("ignored-workspace".to_string());
    storage
        .insert_account(&account)
        .expect("insert account source");
    storage
        .insert_usage_snapshot(&usage("acc-source", 42.0, now))
        .expect("insert account usage");
    storage
        .set_quota_source_model_assignments(
            "openai_account",
            "acc-source",
            &["gpt-source".to_string()],
        )
        .expect("assign account source model");
    storage
        .set_quota_source_model_assignments(
            "openai_account",
            "acc-unlisted",
            &["gpt-should-not-load".to_string()],
        )
        .expect("assign unlisted account source model");

    let result = read_quota_source_list_with_storage(&storage).expect("read quota source list");
    let item = result
        .items
        .iter()
        .find(|item| item.kind == "openai_account" && item.id == "acc-source")
        .expect("account source item exists");

    assert_eq!(item.name, "Source Account");
    assert_eq!(item.status, "ok");
    assert_eq!(item.metric_kind, "window_percent");
    assert_eq!(item.remaining, Some(58.0));
    assert_eq!(item.used, Some(42.0));
    assert!(item.models.contains(&"gpt-6-sol".to_string()));
    assert!(!item.models.contains(&"gpt-5.4".to_string()));
    assert!(!item.models.contains(&"gpt-source".to_string()));
    assert!(!result
        .items
        .iter()
        .any(|item| item.models.contains(&"gpt-should-not-load".to_string())));
}

#[test]
fn model_pool_accounts_skip_unavailable_sources_before_batch_loading() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_account(&account("acc-active", "active", now))
        .expect("insert active account");
    storage
        .insert_account(&account("acc-unavailable", "unavailable", now))
        .expect("insert unavailable account");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            secondary_used_percent: Some(10.0),
            ..usage("acc-active", 25.0, now)
        })
        .expect("insert active usage");
    storage
        .insert_usage_snapshot(&usage("acc-unavailable", 1.0, now))
        .expect("insert unavailable usage");
    storage
        .upsert_account_quota_capacity_override("acc-active", Some(100), Some(200))
        .expect("insert active capacity");
    storage
        .upsert_account_quota_capacity_override("acc-unavailable", Some(100), None)
        .expect("insert unavailable capacity");

    let assignments = HashMap::from([
        (
            ("openai_account".to_string(), "acc-active".to_string()),
            vec!["gpt-test".to_string()],
        ),
        (
            ("openai_account".to_string(), "acc-unavailable".to_string()),
            vec!["gpt-test".to_string()],
        ),
    ]);
    let pools = build_model_pool_accumulators_from_storage(
        &storage,
        &[],
        &["gpt-test".to_string()],
        &assignments,
    )
    .expect("build pools");
    let pool = pools.get("gpt-test").expect("pool exists");

    let source_ids = pool
        .sources
        .iter()
        .filter(|source| source.source_kind == "openai_account")
        .map(|source| source.source_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(source_ids, vec!["acc-active"]);
    assert_eq!(pool.account_primary_remaining_tokens, 75);
    assert_eq!(pool.account_secondary_remaining_tokens, 180);
    assert_eq!(pool.account_estimated_remaining_tokens, 180);
}

#[test]
fn route_assignment_map_reads_only_model_catalog_v2_routes() {
    let mut storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let mut model = storage
        .get_managed_model_v2("gpt-6-sol")
        .expect("read model")
        .expect("seeded model");
    model.routes.push(ModelRouteV2 {
        source_kind: "aggregate_api".to_string(),
        source_id: "agg-source".to_string(),
        upstream_model: "provider-gpt-6-sol".to_string(),
        enabled: true,
        weight: 1,
        ..Default::default()
    });
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            model,
            ..Default::default()
        })
        .expect("save aggregate route");
    storage
        .set_quota_source_model_assignments(
            "future_source",
            "future-source",
            &["gpt-future".to_string()],
        )
        .expect("seed ignored legacy assignment");

    let assignments = route_assignment_map(&storage, None).expect("list V2 routes");

    assert!(assignments.contains_key(&("aggregate_api".to_string(), "agg-source".to_string())));
    assert!(assignments.contains_key(&("openai_account".to_string(), "default".to_string())));
    assert!(!assignments.contains_key(&("future_source".to_string(), "future-source".to_string())));
}

#[test]
fn model_pool_builder_can_limit_work_to_reference_model() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    storage
        .insert_account(&account("acc-reference", "active", now))
        .expect("insert account");
    storage
        .insert_usage_snapshot(&usage("acc-reference", 10.0, now))
        .expect("insert usage");
    storage
        .upsert_account_quota_capacity_override("acc-reference", Some(100), None)
        .expect("insert capacity");

    let assignments = HashMap::from([(
        ("openai_account".to_string(), "acc-reference".to_string()),
        vec!["gpt-reference".to_string()],
    )]);
    let target_models = HashSet::from(["gpt-reference".to_string()]);
    let capacity_config = load_account_capacity_config(&storage).expect("load capacity");
    let pools = build_model_pool_accumulators_for_models(
        &storage,
        &[],
        &["gpt-reference".to_string(), "gpt-other".to_string()],
        &assignments,
        Some(&target_models),
        &capacity_config,
    )
    .expect("build targeted pools");

    assert_eq!(
        pools.keys().cloned().collect::<Vec<_>>(),
        vec!["gpt-reference"]
    );
    let pool = pools.get("gpt-reference").expect("reference pool exists");
    assert_eq!(pool.account_estimated_remaining_tokens, 90);
    assert_eq!(pool.sources.len(), 1);
    assert_eq!(pool.sources[0].source_id, "acc-reference");
}

#[test]
fn target_model_assignment_map_expands_v2_default_account_pool_route() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();

    for (account_id, used_percent) in [
        ("acc-target", 10.0),
        ("acc-other-model", 30.0),
        ("acc-implicit", 20.0),
    ] {
        storage
            .insert_account(&account(account_id, "active", now))
            .expect("insert account");
        storage
            .insert_usage_snapshot(&usage(account_id, used_percent, now))
            .expect("insert usage");
        storage
            .upsert_account_quota_capacity_override(account_id, Some(100), None)
            .expect("insert capacity");
    }
    let assignments =
        route_assignment_map(&storage, Some("gpt-6-sol")).expect("build V2 route map");
    let target_models = HashSet::from(["gpt-6-sol".to_string()]);
    let capacity_config = load_account_capacity_config(&storage).expect("load capacity");
    let pools = build_model_pool_accumulators_for_models(
        &storage,
        &[],
        &["gpt-6-sol".to_string(), "gpt-5.6-sol".to_string()],
        &assignments,
        Some(&target_models),
        &capacity_config,
    )
    .expect("build targeted pools");

    let pool = pools.get("gpt-6-sol").expect("target pool exists");
    let source_ids = pool
        .sources
        .iter()
        .filter(|source| source.source_kind == "openai_account")
        .map(|source| source.source_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        source_ids,
        vec!["acc-implicit", "acc-other-model", "acc-target"]
    );
    assert_eq!(pool.account_estimated_remaining_tokens, 240);
}
