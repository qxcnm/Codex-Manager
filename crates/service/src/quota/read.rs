#[path = "billing_seaorm.rs"]
mod billing_seaorm;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use codexmanager_core::rpc::types::{
    AccountQuotaCapacityOverrideResult, AccountQuotaCapacityTemplateResult, BillingRuleResult,
    QuotaAggregateApiOverviewResult, QuotaApiKeyOverviewResult, QuotaBillingRulesResult,
    QuotaCapacityConfigResult, QuotaModelPoolItem, QuotaModelPoolsResult, QuotaModelUsageItem,
    QuotaModelUsageResult, QuotaOpenAiAccountOverviewResult, QuotaOverviewResult,
    QuotaPoolSourceBreakdown, QuotaRefreshSourceResult, QuotaRefreshSourcesResult,
    QuotaSourceListResult, QuotaSourceModelAssignmentResult, QuotaSourceSummary,
    QuotaSystemPoolResult, QuotaTodayUsageResult,
};
use codexmanager_core::storage::{
    AccountQuotaCapacityOverride, AccountQuotaCapacityTemplate, AccountQuotaPoolSource,
    AccountQuotaSourceSummary, AccountSubscription, AccountTokenPlan,
    AggregateApiQuotaSourceSummary, BillingRule, Storage, UsageSnapshotQuotaSourceRow,
    UsageSnapshotRecord,
};
use rand::RngCore;
use serde_json::Value;

use super::api_key_usage::load_api_key_quota_context;
use super::model_pricing;
use crate::{
    refresh_aggregate_api_balance, storage_helpers::open_storage, time_bounds, usage_refresh,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct QuotaRefreshSourcesInput {
    pub(crate) kinds: Vec<String>,
    pub(crate) source_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BillingRuleUpsertInput {
    pub(crate) id: Option<String>,
    pub(crate) name: String,
    pub(crate) status: Option<String>,
    pub(crate) priority: Option<i64>,
    pub(crate) multiplier_millis: i64,
    pub(crate) model_pattern: Option<String>,
    pub(crate) service_tier: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) project_id: Option<String>,
    pub(crate) api_key_id: Option<String>,
    pub(crate) starts_at: Option<i64>,
    pub(crate) ends_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
struct BalanceSnapshot {
    remaining: Option<f64>,
    total: Option<f64>,
    used: Option<f64>,
    unit: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
struct AggregateBalanceUsdSummary {
    total_usd: f64,
    has_valid_snapshot: bool,
}

impl AggregateBalanceUsdSummary {
    fn total_usd(self) -> Option<f64> {
        self.has_valid_snapshot.then_some(self.total_usd.max(0.0))
    }
}

fn token_total(input: i64, cached: i64, output: i64) -> i64 {
    input.saturating_sub(cached).saturating_add(output).max(0)
}

fn parse_quota_source_balance_snapshot(api: &AggregateApiQuotaSourceSummary) -> BalanceSnapshot {
    parse_balance_snapshot_json(api.last_balance_json.as_deref())
}

fn parse_balance_snapshot_json(raw: Option<&str>) -> BalanceSnapshot {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return BalanceSnapshot::default();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return BalanceSnapshot::default();
    };
    BalanceSnapshot {
        remaining: value.get("remaining").and_then(Value::as_f64),
        total: value.get("total").and_then(Value::as_f64),
        used: value.get("used").and_then(Value::as_f64),
        unit: value
            .get("unit")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
    }
}

fn balance_json_usd(raw: &str) -> Option<f64> {
    parse_balance_snapshot_json(Some(raw))
        .remaining
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn summarize_aggregate_balance_usd<'a>(
    raw_balances: impl IntoIterator<Item = &'a String>,
) -> AggregateBalanceUsdSummary {
    let mut summary = AggregateBalanceUsdSummary::default();
    for balance in raw_balances
        .into_iter()
        .filter_map(|raw| balance_json_usd(raw))
    {
        summary.total_usd += balance;
        summary.has_valid_snapshot = true;
    }
    summary
}

fn load_aggregate_balance_usd_summary(
    storage: &Storage,
) -> Result<AggregateBalanceUsdSummary, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let balances = storage
        .list_aggregate_api_balance_jsons()
        .map_err(|err| format!("list aggregate API balance snapshots failed: {err}"))?;
    Ok(summarize_aggregate_balance_usd(&balances))
}

fn remaining_percent(used_percent: Option<f64>) -> Option<f64> {
    used_percent.map(|used| (100.0 - used.clamp(0.0, 100.0)).max(0.0))
}

fn account_source_is_available(account: &AccountQuotaSourceSummary) -> bool {
    matches!(
        account.status.as_str(),
        "active" | "available" | "force_enabled"
    )
}

fn aggregate_source_display_name(api: &AggregateApiQuotaSourceSummary) -> String {
    api.supplier_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(api.url.as_str())
        .to_string()
}

fn aggregate_source_balance_status(api: &AggregateApiQuotaSourceSummary) -> &'static str {
    match api.last_balance_status.as_deref() {
        Some("success") => "ok",
        Some("error" | "failed") => "error",
        _ if api.balance_query_enabled => "unknown",
        _ => "warning",
    }
}

fn balance_unit_or_usd(balance: &BalanceSnapshot) -> String {
    balance
        .unit
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("USD")
        .to_string()
}

fn account_quota_source_status(account: &AccountQuotaSourceSummary) -> String {
    if account_source_is_available(account) {
        "ok".to_string()
    } else {
        account.status.clone()
    }
}

fn key_display_name(id: &str, name: Option<&str>) -> String {
    name.map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(id)
        .to_string()
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn normalize_billing_status(value: Option<String>) -> Result<String, String> {
    match normalize_optional_text(value)
        .unwrap_or_else(|| "active".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "active" => Ok("active".to_string()),
        "disabled" => Ok("disabled".to_string()),
        _ => Err("计费规则状态只能是 active 或 disabled".to_string()),
    }
}

fn generate_id(prefix: &str, bytes_len: usize) -> String {
    let mut bytes = vec![0u8; bytes_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("{prefix}_{hex}")
}

fn billing_rule_result(rule: BillingRule) -> BillingRuleResult {
    BillingRuleResult {
        id: rule.id,
        name: rule.name,
        status: rule.status,
        priority: rule.priority,
        multiplier_millis: rule.multiplier_millis,
        model_pattern: rule.model_pattern,
        service_tier: rule.service_tier,
        user_id: rule.user_id,
        project_id: rule.project_id,
        api_key_id: rule.api_key_id,
        starts_at: rule.starts_at,
        ends_at: rule.ends_at,
        created_at: rule.created_at,
        updated_at: rule.updated_at,
    }
}

pub(crate) fn read_quota_overview() -> Result<QuotaOverviewResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    read_quota_overview_with_storage(&storage)
}

fn read_quota_overview_with_storage(
    storage: &codexmanager_core::storage::Storage,
) -> Result<QuotaOverviewResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let api_key_stats = storage
        .api_key_quota_overview_stats()
        .map_err(|err| format!("summarize API key quota overview failed: {err}"))?;

    let aggregate_stats = storage
        .aggregate_api_overview_stats()
        .map_err(|err| format!("summarize aggregate API overview failed: {err}"))?;
    let aggregate_balance = load_aggregate_balance_usd_summary(storage)?;

    let account_stats = storage
        .account_quota_overview_stats()
        .map_err(|err| format!("summarize account quota overview failed: {err}"))?;

    let (day_start, day_end) = time_bounds::local_day_bounds_ts()?;
    let today = storage
        .summarize_request_token_stats_between(day_start, day_end)
        .map_err(|err| format!("summarize today token usage failed: {err}"))?;
    let today_input = today.input_tokens.max(0);
    let today_cached = today.cached_input_tokens.max(0);
    let today_output = today.output_tokens.max(0);

    Ok(QuotaOverviewResult {
        api_key: QuotaApiKeyOverviewResult {
            key_count: api_key_stats.key_count,
            limited_key_count: api_key_stats.limited_key_count,
            total_limit_tokens: (api_key_stats.total_limit_tokens > 0)
                .then_some(api_key_stats.total_limit_tokens),
            total_used_tokens: api_key_stats.total_used_tokens,
            total_remaining_tokens: (api_key_stats.total_limit_tokens > 0)
                .then_some(api_key_stats.total_remaining_tokens),
            estimated_cost_usd: api_key_stats.estimated_cost_usd.max(0.0),
        },
        aggregate_api: QuotaAggregateApiOverviewResult {
            source_count: aggregate_stats.source_count,
            enabled_balance_query_count: aggregate_stats.enabled_balance_query_count,
            ok_count: aggregate_stats.ok_count,
            error_count: aggregate_stats.error_count,
            total_balance_usd: aggregate_balance.total_usd(),
            last_refreshed_at: aggregate_stats.last_refreshed_at,
        },
        openai_account: QuotaOpenAiAccountOverviewResult {
            account_count: account_stats.account_count,
            available_count: account_stats.available_count,
            low_quota_count: account_stats.low_quota_count,
            primary_remain_percent: account_stats
                .primary_remain_percent_avg
                .map(|value| value.round() as i64),
            secondary_remain_percent: account_stats
                .secondary_remain_percent_avg
                .map(|value| value.round() as i64),
            last_refreshed_at: account_stats.last_refreshed_at,
        },
        today_usage: QuotaTodayUsageResult {
            input_tokens: today_input,
            cached_input_tokens: today_cached,
            output_tokens: today_output,
            reasoning_output_tokens: today.reasoning_output_tokens.max(0),
            total_tokens: token_total(today_input, today_cached, today_output),
            estimated_cost_usd: today.estimated_cost_usd.max(0.0),
        },
    })
}

pub(crate) fn read_billing_rules() -> Result<QuotaBillingRulesResult, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return crate::storage_helpers::seaorm_block_on(billing_seaorm::list);
    }

    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    read_billing_rules_with_storage(&storage)
}

fn read_billing_rules_with_storage(storage: &Storage) -> Result<QuotaBillingRulesResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let items = storage
        .list_billing_rules()
        .map_err(|err| format!("list billing rules failed: {err}"))?
        .into_iter()
        .map(billing_rule_result)
        .collect();
    Ok(QuotaBillingRulesResult { items })
}

pub(crate) fn upsert_billing_rule(
    input: BillingRuleUpsertInput,
) -> Result<QuotaBillingRulesResult, String> {
    if crate::storage_helpers::seaorm_enabled() {
        return crate::storage_helpers::seaorm_block_on(move |s| billing_seaorm::upsert(s, input));
    }

    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let name = input.name.trim();
    if name.is_empty() {
        return Err("计费规则名称不能为空".to_string());
    }
    let multiplier_millis = input.multiplier_millis;
    if !(0..=100_000).contains(&multiplier_millis) {
        return Err("计费倍率必须在 0 到 100 之间".to_string());
    }
    if let (Some(starts_at), Some(ends_at)) = (input.starts_at, input.ends_at) {
        if starts_at >= ends_at {
            return Err("计费规则结束时间必须晚于开始时间".to_string());
        }
    }
    let user_id = normalize_optional_text(input.user_id);
    if let Some(user_id) = user_id.as_deref() {
        if !storage
            .app_user_exists(user_id)
            .map_err(|err| format!("read app user failed: {err}"))?
        {
            return Err("计费规则用户不存在".to_string());
        }
    }
    let api_key_id = normalize_optional_text(input.api_key_id);
    if let Some(api_key_id) = api_key_id.as_deref() {
        if !storage
            .api_key_exists(api_key_id)
            .map_err(|err| format!("read api key failed: {err}"))?
        {
            return Err("计费规则 API Key 不存在".to_string());
        }
    }
    let project_id = normalize_optional_text(input.project_id);
    if project_id.is_some() {
        return Err("项目维度计费规则暂未开放".to_string());
    }
    let now = codexmanager_core::storage::now_ts();
    storage
        .upsert_billing_rule(&BillingRule {
            id: normalize_optional_text(input.id).unwrap_or_else(|| generate_id("br", 8)),
            name: name.to_string(),
            status: normalize_billing_status(input.status)?,
            priority: input.priority.unwrap_or(0),
            multiplier_millis,
            model_pattern: normalize_optional_text(input.model_pattern),
            service_tier: normalize_optional_text(input.service_tier),
            user_id,
            project_id: None,
            api_key_id,
            starts_at: input.starts_at.filter(|value| *value > 0),
            ends_at: input.ends_at.filter(|value| *value > 0),
            created_at: now,
            updated_at: now,
        })
        .map_err(|err| format!("save billing rule failed: {err}"))?;
    read_billing_rules_with_storage(&storage)
}

pub(crate) fn delete_billing_rule(id: &str) -> Result<QuotaBillingRulesResult, String> {
    if crate::storage_helpers::seaorm_enabled() {
        let id = id.to_owned();
        return crate::storage_helpers::seaorm_block_on(move |s| billing_seaorm::delete(s, id));
    }

    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let id = id.trim();
    if id.is_empty() {
        return Err("计费规则 ID 不能为空".to_string());
    }
    storage
        .delete_billing_rule(id)
        .map_err(|err| format!("delete billing rule failed: {err}"))?;
    read_billing_rules_with_storage(&storage)
}

pub(crate) fn read_quota_model_usage(
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> Result<QuotaModelUsageResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    read_quota_model_usage_with_storage(&storage, start_ts, end_ts)
}

fn read_quota_model_usage_with_storage(
    storage: &Storage,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> Result<QuotaModelUsageResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let price_rules = model_pricing::load_catalog_prices(storage)?;
    let usage = storage
        .summarize_request_token_stats_by_model(start_ts, end_ts)
        .map_err(|err| format!("summarize token usage by model failed: {err}"))?;
    let api_key_remaining_tokens = storage
        .api_key_remaining_quota_tokens()
        .map_err(|err| format!("summarize api key remaining quota failed: {err}"))?;

    let aggregate_balance_usd = load_aggregate_balance_usd_summary(storage)?.total_usd();

    let account_stats = storage
        .account_quota_overview_stats()
        .map_err(|err| format!("summarize account quota overview failed: {err}"))?;
    let openai_available_account_count = account_stats.available_count;
    let openai_primary_remain_percent = account_stats
        .primary_remain_percent_avg
        .map(|value| value.round() as i64);
    let openai_secondary_remain_percent = account_stats
        .secondary_remain_percent_avg
        .map(|value| value.round() as i64);

    Ok(QuotaModelUsageResult {
        items: usage
            .into_iter()
            .map(|item| {
                let price = model_pricing::resolve_model_price_from_catalog(
                    &price_rules,
                    item.model.as_str(),
                    item.input_tokens,
                );
                let aggregate_estimated_remaining_tokens =
                    aggregate_balance_usd.and_then(|balance| {
                        model_pricing::estimate_remaining_tokens_from_usd_with_catalog(
                            &price_rules,
                            &item.model,
                            balance,
                        )
                    });
                QuotaModelUsageItem {
                    model: item.model,
                    provider: price.as_ref().map(|price| price.provider.clone()),
                    input_tokens: item.input_tokens,
                    cached_input_tokens: item.cached_input_tokens,
                    output_tokens: item.output_tokens,
                    reasoning_output_tokens: item.reasoning_output_tokens,
                    total_tokens: item.total_tokens,
                    estimated_cost_usd: (price.is_some() || item.estimated_cost_usd > 0.0)
                        .then_some(item.estimated_cost_usd.max(0.0)),
                    price_status: if price.is_some() { "ok" } else { "missing" }.to_string(),
                    api_key_remaining_tokens: (api_key_remaining_tokens > 0)
                        .then_some(api_key_remaining_tokens),
                    aggregate_estimated_remaining_tokens,
                    aggregate_balance_usd,
                    openai_available_account_count,
                    openai_primary_remain_percent,
                    openai_secondary_remain_percent,
                    openai_estimated_remaining_tokens: None,
                    openai_estimate_enabled: false,
                }
            })
            .collect(),
    })
}

#[derive(Debug, Clone, Default)]
struct PoolAccumulator {
    provider: Option<String>,
    total_remaining_tokens: i64,
    has_total_remaining_tokens: bool,
    aggregate_remaining_tokens: i64,
    has_aggregate_remaining_tokens: bool,
    account_primary_remaining_tokens: i64,
    has_account_primary_remaining_tokens: bool,
    account_secondary_remaining_tokens: i64,
    has_account_secondary_remaining_tokens: bool,
    account_estimated_remaining_tokens: i64,
    has_account_estimated_remaining_tokens: bool,
    source_count: i64,
    sources: Vec<QuotaPoolSourceBreakdown>,
    price_status: String,
}

#[derive(Debug, Clone, Default)]
struct AccountCapacity {
    primary_window_tokens: Option<i64>,
    secondary_window_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default)]
struct AccountCapacityConfig {
    templates: Vec<AccountQuotaCapacityTemplate>,
}

#[derive(Debug, Default)]
struct AccountPoolContext {
    accounts: Vec<AccountQuotaPoolSource>,
    usage_by_account: HashMap<String, UsageSnapshotQuotaSourceRow>,
    tokens_by_account: HashMap<String, AccountTokenPlan>,
    subscriptions_by_account: HashMap<String, AccountSubscription>,
    usage_plan_fallback_by_account: HashMap<String, UsageSnapshotRecord>,
    overrides_by_account: HashMap<String, AccountQuotaCapacityOverride>,
}

impl AccountCapacityConfig {
    fn template_map(&self) -> HashMap<String, AccountQuotaCapacityTemplate> {
        self.templates
            .iter()
            .cloned()
            .map(|item| (item.plan_type.to_ascii_lowercase(), item))
            .collect()
    }

    fn template_results(&self) -> Vec<AccountQuotaCapacityTemplateResult> {
        capacity_template_results_with_slots(self.templates.clone())
    }
}

const ACCOUNT_CAPACITY_TEMPLATE_SLOTS: &[&str] = &["free", "plus", "pro", "team", "enterprise"];

pub(crate) fn read_quota_capacity_config() -> Result<QuotaCapacityConfigResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    read_quota_capacity_config_with_storage(&storage)
}

fn read_quota_capacity_config_with_storage(
    storage: &Storage,
) -> Result<QuotaCapacityConfigResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    Ok(QuotaCapacityConfigResult {
        source_assignments: build_source_assignment_results(
            route_assignment_map(storage, None)
                .map_err(|err| format!("list model catalog V2 routes failed: {err}"))?,
        ),
        templates: capacity_template_results_with_slots(
            storage
                .list_account_quota_capacity_templates()
                .map_err(|err| format!("list account quota capacity templates failed: {err}"))?,
        ),
        account_overrides: storage
            .list_account_quota_capacity_overrides()
            .map_err(|err| format!("list account quota capacity overrides failed: {err}"))?
            .into_iter()
            .map(override_result)
            .collect(),
    })
}

pub(crate) fn update_account_quota_capacity_template(
    plan_type: &str,
    primary_window_tokens: Option<i64>,
    secondary_window_tokens: Option<i64>,
) -> Result<QuotaCapacityConfigResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .upsert_account_quota_capacity_template(
            plan_type,
            primary_window_tokens,
            secondary_window_tokens,
        )
        .map_err(|err| format!("update account quota capacity template failed: {err}"))?;
    read_quota_capacity_config_with_storage(&storage)
}

pub(crate) fn update_account_quota_capacity_override(
    account_id: &str,
    primary_window_tokens: Option<i64>,
    secondary_window_tokens: Option<i64>,
) -> Result<QuotaCapacityConfigResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .upsert_account_quota_capacity_override(
            account_id,
            primary_window_tokens,
            secondary_window_tokens,
        )
        .map_err(|err| format!("update account quota capacity override failed: {err}"))?;
    read_quota_capacity_config_with_storage(&storage)
}

pub(crate) fn read_quota_model_pools() -> Result<QuotaModelPoolsResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let price_rules = model_pricing::load_catalog_prices(&storage)?;
    let api_models = api_available_model_slugs(&storage)?;
    let assignments = route_assignment_map(&storage, None)
        .map_err(|err| format!("list model catalog V2 routes failed: {err}"))?;
    let capacity_config = load_account_capacity_config(&storage)?;
    let pools = build_model_pool_accumulators(
        &storage,
        &price_rules,
        &api_models,
        &assignments,
        &capacity_config,
    )?;
    let mut items = pools
        .into_iter()
        .map(|(model, pool)| pool_to_model_item(model, pool))
        .collect::<Vec<_>>();
    let model_order = api_models
        .iter()
        .enumerate()
        .map(|(index, model)| (model.as_str(), index))
        .collect::<HashMap<_, _>>();
    items.sort_by(|a, b| {
        let a_order = model_order
            .get(a.model.as_str())
            .copied()
            .unwrap_or(usize::MAX);
        let b_order = model_order
            .get(b.model.as_str())
            .copied()
            .unwrap_or(usize::MAX);
        a_order.cmp(&b_order).then_with(|| a.model.cmp(&b.model))
    });
    Ok(QuotaModelPoolsResult {
        items,
        templates: capacity_config.template_results(),
        account_overrides: storage
            .list_account_quota_capacity_overrides()
            .map_err(|err| format!("list account quota capacity overrides failed: {err}"))?
            .into_iter()
            .map(override_result)
            .collect(),
    })
}

pub(crate) fn read_quota_system_pool(
    reference_model: Option<String>,
) -> Result<QuotaSystemPoolResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let price_rules = model_pricing::load_catalog_prices(&storage)?;
    let api_models = api_available_model_slugs(&storage)?;
    let reference_model = reference_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| api_models.first().cloned())
        .unwrap_or_else(|| "unknown".to_string());
    let assignments = route_assignment_map(&storage, Some(reference_model.as_str()))
        .map_err(|err| format!("list model catalog V2 routes failed: {err}"))?;
    let target_models = HashSet::from([reference_model.clone()]);
    let capacity_config = load_account_capacity_config(&storage)?;
    let mut pools = build_model_pool_accumulators_for_models(
        &storage,
        &price_rules,
        &api_models,
        &assignments,
        Some(&target_models),
        &capacity_config,
    )?;
    let pool = pools.remove(reference_model.as_str()).unwrap_or_else(|| {
        let mut pool = PoolAccumulator {
            price_status: "missing".to_string(),
            ..PoolAccumulator::default()
        };
        if let Some(price) = model_pricing::resolve_model_price_from_catalog(
            &price_rules,
            reference_model.as_str(),
            0,
        ) {
            pool.provider = Some(price.provider);
            pool.price_status = "ok".to_string();
        }
        pool
    });
    let aggregate_source_count = pool
        .sources
        .iter()
        .filter(|source| source.source_kind == "aggregate_api" && source.remaining_tokens.is_some())
        .count() as i64;
    let account_source_count = pool
        .sources
        .iter()
        .filter(|source| {
            source.source_kind == "openai_account" && source.remaining_tokens.is_some()
        })
        .count() as i64;
    let unknown_source_count = pool
        .sources
        .iter()
        .filter(|source| source.remaining_tokens.is_none())
        .count() as i64;
    Ok(QuotaSystemPoolResult {
        reference_model,
        provider: pool.provider,
        total_remaining_tokens: pool
            .has_total_remaining_tokens
            .then_some(pool.total_remaining_tokens),
        aggregate_remaining_tokens: pool
            .has_aggregate_remaining_tokens
            .then_some(pool.aggregate_remaining_tokens),
        account_primary_remaining_tokens: pool
            .has_account_primary_remaining_tokens
            .then_some(pool.account_primary_remaining_tokens),
        account_secondary_remaining_tokens: pool
            .has_account_secondary_remaining_tokens
            .then_some(pool.account_secondary_remaining_tokens),
        account_estimated_remaining_tokens: pool
            .has_account_estimated_remaining_tokens
            .then_some(pool.account_estimated_remaining_tokens),
        aggregate_source_count,
        account_source_count,
        unknown_source_count,
        price_status: pool.price_status,
        sources: pool.sources,
    })
}

fn build_model_pool_accumulators(
    storage: &codexmanager_core::storage::Storage,
    price_rules: &[model_pricing::CatalogModelPrice],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
    capacity_config: &AccountCapacityConfig,
) -> Result<BTreeMap<String, PoolAccumulator>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    build_model_pool_accumulators_for_models(
        storage,
        price_rules,
        api_models,
        assignments,
        None,
        capacity_config,
    )
}

#[cfg(test)]
fn build_model_pool_accumulators_from_storage(
    storage: &codexmanager_core::storage::Storage,
    price_rules: &[model_pricing::CatalogModelPrice],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
) -> Result<BTreeMap<String, PoolAccumulator>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let capacity_config = load_account_capacity_config(storage)?;
    build_model_pool_accumulators(
        storage,
        price_rules,
        api_models,
        assignments,
        &capacity_config,
    )
}

fn build_model_pool_accumulators_for_models(
    storage: &codexmanager_core::storage::Storage,
    price_rules: &[model_pricing::CatalogModelPrice],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
    target_models: Option<&HashSet<String>>,
    capacity_config: &AccountCapacityConfig,
) -> Result<BTreeMap<String, PoolAccumulator>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let mut pools = BTreeMap::<String, PoolAccumulator>::new();
    seed_model_pools(&mut pools, price_rules, api_models, target_models);
    add_aggregate_api_pools(
        storage,
        price_rules,
        api_models,
        assignments,
        target_models,
        &mut pools,
    )?;
    add_account_pools(
        storage,
        price_rules,
        api_models,
        assignments,
        target_models,
        capacity_config,
        &mut pools,
    )?;
    Ok(pools)
}

fn seed_model_pools(
    pools: &mut BTreeMap<String, PoolAccumulator>,
    price_rules: &[model_pricing::CatalogModelPrice],
    api_models: &[String],
    target_models: Option<&HashSet<String>>,
) {
    for model in api_models {
        if !model_matches_target(model, target_models) {
            continue;
        }
        let price = model_pricing::resolve_model_price_from_catalog(price_rules, model, 0);
        let entry = pools
            .entry(model.clone())
            .or_insert_with(|| PoolAccumulator {
                price_status: if price.is_some() {
                    "ok".to_string()
                } else {
                    "missing".to_string()
                },
                ..PoolAccumulator::default()
            });
        if let Some(price) = price {
            entry.provider = Some(price.provider);
            entry.price_status = "ok".to_string();
        }
    }
}

fn add_aggregate_api_pools(
    storage: &codexmanager_core::storage::Storage,
    price_rules: &[model_pricing::CatalogModelPrice],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
    target_models: Option<&HashSet<String>>,
    pools: &mut BTreeMap<String, PoolAccumulator>,
) -> Result<(), String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let aggregate_apis = storage
        .list_aggregate_api_quota_source_summaries()
        .map_err(|err| format!("list aggregate APIs failed: {err}"))?;
    for api in aggregate_apis {
        if api.status == "disabled" {
            continue;
        }
        let balance = parse_quota_source_balance_snapshot(&api);
        let models = source_models("aggregate_api", &api.id, assignments, api_models);
        let balance_unit = balance_unit_or_usd(&balance);
        let status = aggregate_source_balance_status(&api);
        for model in models {
            let model = model.trim().to_string();
            if model.is_empty() || !model_matches_target(&model, target_models) {
                continue;
            }
            let provider = model_pricing::resolve_model_price_from_catalog(price_rules, &model, 0)
                .map(|price| price.provider);
            let remaining_tokens = balance.remaining.and_then(|remaining| {
                if is_usd_unit(balance_unit.as_str()) {
                    model_pricing::estimate_remaining_tokens_from_usd_with_catalog(
                        price_rules,
                        &model,
                        remaining,
                    )
                } else {
                    None
                }
            });
            let price_status = match (balance.remaining, is_usd_unit(balance_unit.as_str())) {
                (None, _) => "missing_balance",
                (Some(_), false) => "unsupported_unit",
                (Some(_), true) if remaining_tokens.is_none() => "missing",
                _ => "ok",
            };
            let source = QuotaPoolSourceBreakdown {
                source_kind: "aggregate_api".to_string(),
                source_id: api.id.clone(),
                name: aggregate_source_display_name(&api),
                status: status.to_string(),
                remaining_tokens,
                raw_remaining: balance.remaining,
                raw_unit: Some(balance_unit.clone()),
                models: vec![model.clone()],
                captured_at: api.last_balance_at,
                price_status: price_status.to_string(),
            };
            let entry = pools
                .entry(model.clone())
                .or_insert_with(|| PoolAccumulator {
                    price_status: if price_status == "ok" {
                        "ok".to_string()
                    } else {
                        price_status.to_string()
                    },
                    ..PoolAccumulator::default()
                });
            if provider.is_some() {
                entry.provider = provider;
            }
            if price_status == "ok" {
                entry.price_status = "ok".to_string();
            }
            if let Some(tokens) = remaining_tokens {
                entry.total_remaining_tokens = entry.total_remaining_tokens.saturating_add(tokens);
                entry.has_total_remaining_tokens = true;
                entry.aggregate_remaining_tokens =
                    entry.aggregate_remaining_tokens.saturating_add(tokens);
                entry.has_aggregate_remaining_tokens = true;
                entry.source_count += 1;
            }
            entry.sources.push(source);
        }
    }
    Ok(())
}

fn add_account_pools(
    storage: &codexmanager_core::storage::Storage,
    price_rules: &[model_pricing::CatalogModelPrice],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
    target_models: Option<&HashSet<String>>,
    capacity_config: &AccountCapacityConfig,
    pools: &mut BTreeMap<String, PoolAccumulator>,
) -> Result<(), String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let context = load_account_pool_context(storage)?;
    let template_map = capacity_config.template_map();

    for account in context.accounts {
        let account_id = account.id.clone();
        let usage = context.usage_by_account.get(account_id.as_str());
        let plan_type = resolve_account_plan_type_from_sources(
            &account_id,
            &context.tokens_by_account,
            context
                .usage_plan_fallback_by_account
                .get(account_id.as_str()),
            &context.subscriptions_by_account,
        );
        let capacity = resolve_account_capacity(
            &account_id,
            plan_type.as_deref(),
            &template_map,
            &context.overrides_by_account,
        );
        let primary_remaining = capacity.as_ref().and_then(|capacity| {
            estimate_window_remaining_tokens(
                capacity.primary_window_tokens,
                usage.and_then(|item| item.used_percent),
            )
        });
        let secondary_remaining = capacity.as_ref().and_then(|capacity| {
            estimate_window_remaining_tokens(
                capacity.secondary_window_tokens,
                usage.and_then(|item| item.secondary_used_percent),
            )
        });
        let estimated_remaining = [primary_remaining, secondary_remaining]
            .into_iter()
            .flatten()
            .max();
        let price_status = if capacity.is_none() {
            "unconfigured"
        } else if usage.is_none() {
            "unknown_usage"
        } else if estimated_remaining.is_none() {
            "missing_window"
        } else {
            "ok"
        };
        let status = if price_status == "ok" {
            "ok"
        } else {
            price_status
        };
        let raw_remaining = usage.and_then(|item| remaining_percent(item.used_percent));
        let models = source_models("openai_account", &account.id, assignments, api_models);
        for model in models {
            let model = model.trim().to_string();
            if model.is_empty() || !model_matches_target(&model, target_models) {
                continue;
            }
            let provider = model_pricing::resolve_model_price_from_catalog(price_rules, &model, 0)
                .map(|price| price.provider)
                .or_else(|| Some("openai".to_string()));
            let source = QuotaPoolSourceBreakdown {
                source_kind: "openai_account".to_string(),
                source_id: account.id.clone(),
                name: account.label.clone(),
                status: status.to_string(),
                remaining_tokens: estimated_remaining,
                raw_remaining,
                raw_unit: Some("percent".to_string()),
                models: vec![model.clone()],
                captured_at: usage.map(|item| item.captured_at),
                price_status: price_status.to_string(),
            };
            let entry = pools
                .entry(model.clone())
                .or_insert_with(|| PoolAccumulator {
                    provider: provider.clone(),
                    price_status: "ok".to_string(),
                    ..PoolAccumulator::default()
                });
            if provider.is_some() {
                entry.provider = provider;
            }
            if let Some(tokens) = estimated_remaining {
                entry.total_remaining_tokens = entry.total_remaining_tokens.saturating_add(tokens);
                entry.has_total_remaining_tokens = true;
                entry.account_estimated_remaining_tokens = entry
                    .account_estimated_remaining_tokens
                    .saturating_add(tokens);
                entry.has_account_estimated_remaining_tokens = true;
                entry.source_count += 1;
            }
            if let Some(tokens) = primary_remaining {
                entry.account_primary_remaining_tokens = entry
                    .account_primary_remaining_tokens
                    .saturating_add(tokens);
                entry.has_account_primary_remaining_tokens = true;
            }
            if let Some(tokens) = secondary_remaining {
                entry.account_secondary_remaining_tokens = entry
                    .account_secondary_remaining_tokens
                    .saturating_add(tokens);
                entry.has_account_secondary_remaining_tokens = true;
            }
            entry.sources.push(source);
        }
    }
    Ok(())
}

fn load_account_pool_context(
    storage: &codexmanager_core::storage::Storage,
) -> Result<AccountPoolContext, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let accounts = storage
        .list_available_account_quota_pool_sources()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    if account_ids.is_empty() {
        return Ok(AccountPoolContext {
            accounts,
            ..AccountPoolContext::default()
        });
    }

    let usage_by_account = storage
        .latest_usage_quota_source_rows_for_accounts(&account_ids)
        .map_err(|err| format!("list usage snapshots failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, UsageSnapshotQuotaSourceRow>>();
    let tokens_by_account = storage
        .list_account_token_plans_for_accounts(&account_ids)
        .map_err(|err| format!("list account tokens failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountTokenPlan>>();
    let subscriptions_by_account = storage
        .list_account_subscriptions_for_accounts(&account_ids)
        .map_err(|err| format!("list account subscriptions failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountSubscription>>();
    let usage_plan_fallback_by_account = load_usage_plan_fallback_snapshots(
        storage,
        &account_ids,
        &tokens_by_account,
        &subscriptions_by_account,
    )?;
    let overrides_by_account = storage
        .list_account_quota_capacity_overrides_for_accounts(&account_ids)
        .map_err(|err| format!("list account quota capacity overrides failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountQuotaCapacityOverride>>();

    Ok(AccountPoolContext {
        accounts,
        usage_by_account,
        tokens_by_account,
        subscriptions_by_account,
        usage_plan_fallback_by_account,
        overrides_by_account,
    })
}

fn model_matches_target(model: &str, target_models: Option<&HashSet<String>>) -> bool {
    target_models.is_none_or(|targets| targets.contains(model.trim()))
}

fn pool_to_model_item(model: String, pool: PoolAccumulator) -> QuotaModelPoolItem {
    QuotaModelPoolItem {
        model,
        provider: pool.provider,
        total_remaining_tokens: pool
            .has_total_remaining_tokens
            .then_some(pool.total_remaining_tokens),
        aggregate_remaining_tokens: pool
            .has_aggregate_remaining_tokens
            .then_some(pool.aggregate_remaining_tokens),
        account_primary_remaining_tokens: pool
            .has_account_primary_remaining_tokens
            .then_some(pool.account_primary_remaining_tokens),
        account_secondary_remaining_tokens: pool
            .has_account_secondary_remaining_tokens
            .then_some(pool.account_secondary_remaining_tokens),
        account_estimated_remaining_tokens: pool
            .has_account_estimated_remaining_tokens
            .then_some(pool.account_estimated_remaining_tokens),
        source_count: pool.source_count,
        sources: pool.sources,
        price_status: pool.price_status,
    }
}

fn build_source_assignment_results(
    assignments: HashMap<(String, String), Vec<String>>,
) -> Vec<QuotaSourceModelAssignmentResult> {
    let mut items = assignments
        .into_iter()
        .map(|((source_kind, source_id), mut model_slugs)| {
            model_slugs.sort();
            model_slugs.dedup();
            QuotaSourceModelAssignmentResult {
                source_kind,
                source_id,
                model_slugs,
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        left.source_kind
            .cmp(&right.source_kind)
            .then_with(|| left.source_id.cmp(&right.source_id))
    });
    items
}

fn route_assignment_map(
    storage: &Storage,
    target_model: Option<&str>,
) -> rusqlite::Result<HashMap<(String, String), Vec<String>>> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let mut grouped = HashMap::<(String, String), BTreeSet<String>>::new();
    for model in storage.list_api_models_v2()? {
        if target_model.is_some_and(|target| !model.slug.eq_ignore_ascii_case(target)) {
            continue;
        }
        for route in model.routes.into_iter().filter(|route| route.enabled) {
            let source_kind = match route.source_kind.as_str() {
                "account_pool" => "openai_account",
                "aggregate_api" => "aggregate_api",
                _ => continue,
            };
            grouped
                .entry((source_kind.to_string(), route.source_id))
                .or_default()
                .insert(model.slug.clone());
        }
    }
    Ok(grouped
        .into_iter()
        .map(|(key, values)| (key, values.into_iter().collect()))
        .collect())
}

fn load_account_capacity_config(storage: &Storage) -> Result<AccountCapacityConfig, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    Ok(AccountCapacityConfig {
        templates: storage
            .list_account_quota_capacity_templates()
            .map_err(|err| format!("list account quota capacity templates failed: {err}"))?,
    })
}

fn source_models(
    source_kind: &str,
    source_id: &str,
    assignments: &HashMap<(String, String), Vec<String>>,
    _api_models: &[String],
) -> Vec<String> {
    assignments
        .get(&(source_kind.to_string(), source_id.to_string()))
        .or_else(|| {
            if source_kind == "openai_account" {
                assignments.get(&("openai_account".to_string(), "default".to_string()))
            } else {
                None
            }
        })
        .cloned()
        .unwrap_or_default()
}

fn api_available_model_slugs(
    storage: &codexmanager_core::storage::Storage,
) -> Result<Vec<String>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .list_api_models_v2()
        .map_err(|err| format!("list model catalog V2 failed: {err}"))
        .map(|models| models.into_iter().map(|model| model.slug).collect())
}

fn resolve_account_plan_type_from_sources(
    account_id: &str,
    tokens: &HashMap<String, AccountTokenPlan>,
    usage: Option<&UsageSnapshotRecord>,
    subscriptions: &HashMap<String, AccountSubscription>,
) -> Option<String> {
    crate::account_plan::resolve_effective_account_plan(
        tokens.get(account_id),
        usage,
        subscriptions.get(account_id),
    )
    .map(|plan| plan.normalized)
}

fn load_usage_plan_fallback_snapshots(
    storage: &codexmanager_core::storage::Storage,
    account_ids: &[String],
    tokens: &HashMap<String, AccountTokenPlan>,
    subscriptions: &HashMap<String, AccountSubscription>,
) -> Result<HashMap<String, UsageSnapshotRecord>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let fallback_ids = account_ids
        .iter()
        .filter(|account_id| {
            resolve_account_plan_type_from_sources(account_id, tokens, None, subscriptions)
                .is_none()
        })
        .cloned()
        .collect::<Vec<_>>();
    if fallback_ids.is_empty() {
        return Ok(HashMap::new());
    }

    storage
        .latest_usage_snapshots_for_accounts(&fallback_ids)
        .map_err(|err| format!("list usage plan fallback snapshots failed: {err}"))
        .map(|items| {
            items
                .into_iter()
                .map(|item| (item.account_id.clone(), item))
                .collect()
        })
}

fn resolve_account_capacity(
    account_id: &str,
    plan_type: Option<&str>,
    templates: &HashMap<String, AccountQuotaCapacityTemplate>,
    overrides: &HashMap<String, AccountQuotaCapacityOverride>,
) -> Option<AccountCapacity> {
    if let Some(override_capacity) = overrides.get(account_id) {
        return Some(AccountCapacity {
            primary_window_tokens: override_capacity.primary_window_tokens,
            secondary_window_tokens: override_capacity.secondary_window_tokens,
        });
    }
    let plan_type = plan_type?.trim().to_ascii_lowercase();
    let template = templates.get(plan_type.as_str())?;
    if template.primary_window_tokens.is_none() && template.secondary_window_tokens.is_none() {
        return None;
    }
    Some(AccountCapacity {
        primary_window_tokens: template.primary_window_tokens,
        secondary_window_tokens: template.secondary_window_tokens,
    })
}

fn estimate_window_remaining_tokens(
    capacity_tokens: Option<i64>,
    used_percent: Option<f64>,
) -> Option<i64> {
    let capacity_tokens = capacity_tokens.filter(|value| *value > 0)?;
    let remaining = remaining_percent(used_percent)?;
    Some(((capacity_tokens as f64) * remaining / 100.0).round() as i64)
}

fn is_usd_unit(unit: &str) -> bool {
    matches!(
        unit.trim().to_ascii_lowercase().as_str(),
        "usd" | "$" | "dollar" | "dollars" | "us_dollar"
    )
}

fn template_result(item: AccountQuotaCapacityTemplate) -> AccountQuotaCapacityTemplateResult {
    AccountQuotaCapacityTemplateResult {
        plan_type: item.plan_type,
        primary_window_tokens: item.primary_window_tokens,
        secondary_window_tokens: item.secondary_window_tokens,
    }
}

fn capacity_template_results_with_slots(
    items: Vec<AccountQuotaCapacityTemplate>,
) -> Vec<AccountQuotaCapacityTemplateResult> {
    let mut map = items
        .into_iter()
        .map(|item| (item.plan_type.to_ascii_lowercase(), item))
        .collect::<BTreeMap<_, _>>();
    let mut results = Vec::new();
    for slot in ACCOUNT_CAPACITY_TEMPLATE_SLOTS {
        if let Some(item) = map.remove(*slot) {
            results.push(template_result(item));
        } else {
            results.push(AccountQuotaCapacityTemplateResult {
                plan_type: (*slot).to_string(),
                primary_window_tokens: None,
                secondary_window_tokens: None,
            });
        }
    }
    results.extend(map.into_values().map(template_result));
    results
}

fn override_result(item: AccountQuotaCapacityOverride) -> AccountQuotaCapacityOverrideResult {
    AccountQuotaCapacityOverrideResult {
        account_id: item.account_id,
        primary_window_tokens: item.primary_window_tokens,
        secondary_window_tokens: item.secondary_window_tokens,
    }
}

pub(crate) fn read_quota_source_list() -> Result<QuotaSourceListResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    read_quota_source_list_with_storage(&storage)
}

fn read_quota_source_list_with_storage(storage: &Storage) -> Result<QuotaSourceListResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let api_models = api_available_model_slugs(storage)?;
    let mut items = Vec::new();

    let api_key_context = load_api_key_quota_context(storage)?;
    let usage_map = api_key_context
        .usage_by_key
        .iter()
        .map(|item| (item.key_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    for key in api_key_context.api_keys {
        let used = usage_map
            .get(key.id.as_str())
            .map(|item| item.total_tokens.max(0))
            .unwrap_or(0);
        let limit = key.quota_limit_tokens;
        items.push(QuotaSourceSummary {
            id: key.id.clone(),
            kind: "api_key".to_string(),
            name: key_display_name(&key.id, key.name.as_deref()),
            status: key.status,
            metric_kind: "token_limit".to_string(),
            remaining: limit.map(|value| value.saturating_sub(used) as f64),
            total: limit.map(|value| value as f64),
            used: Some(used as f64),
            unit: Some("token".to_string()),
            models: key.model_slug.into_iter().collect(),
            provider: None,
            captured_at: key.last_used_at,
            error: None,
        });
    }

    let aggregate_apis = storage
        .list_aggregate_api_quota_source_summaries()
        .map_err(|err| format!("list aggregate APIs failed: {err}"))?;
    let accounts = storage
        .list_account_quota_source_summaries()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let assignments = route_assignment_map(storage, None)
        .map_err(|err| format!("list model catalog V2 routes failed: {err}"))?;

    for api in aggregate_apis {
        let balance = parse_quota_source_balance_snapshot(&api);
        let status = aggregate_source_balance_status(&api);
        items.push(QuotaSourceSummary {
            id: api.id.clone(),
            kind: "aggregate_api".to_string(),
            name: aggregate_source_display_name(&api),
            status: status.to_string(),
            metric_kind: "money_balance".to_string(),
            remaining: balance.remaining,
            total: balance.total,
            used: balance.used,
            unit: Some(balance_unit_or_usd(&balance)),
            models: source_models("aggregate_api", &api.id, &assignments, &api_models),
            provider: Some(api.provider_type),
            captured_at: api.last_balance_at,
            error: api.last_balance_error,
        });
    }

    let usage_by_account = storage
        .latest_usage_quota_source_rows_for_accounts(&account_ids)
        .map_err(|err| format!("read account usage failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<_, _>>();

    for account in accounts {
        let usage: Option<&UsageSnapshotQuotaSourceRow> = usage_by_account.get(account.id.as_str());
        let remaining = usage.and_then(|item| remaining_percent(item.used_percent));
        let used = usage.and_then(|item| item.used_percent);
        items.push(QuotaSourceSummary {
            id: account.id.clone(),
            kind: "openai_account".to_string(),
            name: account.label.clone(),
            status: account_quota_source_status(&account),
            metric_kind: "window_percent".to_string(),
            remaining,
            total: Some(100.0),
            used,
            unit: Some("percent".to_string()),
            models: source_models("openai_account", &account.id, &assignments, &api_models),
            provider: Some("openai".to_string()),
            captured_at: usage.map(|item| item.captured_at),
            error: None,
        });
    }

    Ok(QuotaSourceListResult { items })
}

pub(crate) fn refresh_quota_sources(
    input: QuotaRefreshSourcesInput,
) -> Result<QuotaRefreshSourcesResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let kinds = if input.kinds.is_empty() {
        HashSet::from(["aggregate_api".to_string(), "openai_account".to_string()])
    } else {
        input.kinds.into_iter().collect::<HashSet<_>>()
    };
    let source_ids = normalize_source_ids(input.source_ids);
    let mut items = Vec::new();

    if kinds.contains("aggregate_api") {
        let aggregate_api_ids = if source_ids.is_empty() {
            storage
                .list_balance_query_aggregate_api_ids()
                .map_err(|err| format!("list aggregate APIs failed: {err}"))?
        } else {
            storage
                .list_balance_query_aggregate_api_ids_for_ids(&source_ids)
                .map_err(|err| format!("list aggregate APIs failed: {err}"))?
        };
        for id in aggregate_api_ids {
            let result = refresh_aggregate_api_balance(id.as_str());
            items.push(QuotaRefreshSourceResult {
                id,
                kind: "aggregate_api".to_string(),
                ok: result.is_ok(),
                error: result.err(),
            });
        }
    }

    if kinds.contains("openai_account") {
        let account_ids = if source_ids.is_empty() {
            storage
                .list_account_ids()
                .map_err(|err| format!("list accounts failed: {err}"))?
        } else {
            storage
                .list_account_ids_for_ids(&source_ids)
                .map_err(|err| format!("list accounts failed: {err}"))?
        };
        for id in account_ids {
            let result = usage_refresh::refresh_usage_for_account(id.as_str());
            items.push(QuotaRefreshSourceResult {
                id,
                kind: "openai_account".to_string(),
                ok: result.is_ok(),
                error: result.err(),
            });
        }
    }

    Ok(QuotaRefreshSourcesResult { items })
}

fn normalize_source_ids(source_ids: Vec<String>) -> Vec<String> {
    let mut source_ids = source_ids
        .into_iter()
        .map(|source_id| source_id.trim().to_string())
        .filter(|source_id| !source_id.is_empty())
        .collect::<Vec<_>>();
    source_ids.sort();
    source_ids.dedup();
    source_ids
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
