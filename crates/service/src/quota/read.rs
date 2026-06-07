use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{Duration, Local, LocalResult, TimeZone};
use codexmanager_core::rpc::types::{
    AccountQuotaCapacityOverrideResult, AccountQuotaCapacityTemplateResult, BillingRuleResult,
    ModelPriceRuleEntry, ModelPriceRuleListResult, ModelPriceRuleUpsertInput,
    QuotaAggregateApiOverviewResult, QuotaApiKeyModelUsageItem, QuotaApiKeyOverviewResult,
    QuotaApiKeyUsageItem, QuotaApiKeyUsageResult, QuotaBillingRulesResult,
    QuotaCapacityConfigResult, QuotaModelPoolItem, QuotaModelPoolsResult, QuotaModelUsageItem,
    QuotaModelUsageResult, QuotaOpenAiAccountOverviewResult, QuotaOverviewResult,
    QuotaPoolSourceBreakdown, QuotaRefreshSourceResult, QuotaRefreshSourcesResult,
    QuotaSourceListResult, QuotaSourceModelAssignmentResult, QuotaSourceSummary,
    QuotaSystemPoolResult, QuotaTodayUsageResult,
};
use codexmanager_core::storage::{
    now_ts, Account, AccountQuotaCapacityOverride, AccountQuotaCapacityTemplate, AccountSubscription,
    AggregateApi, ApiKey, BillingRule, ModelPriceRule, QuotaSourceModelAssignment, Token,
    UsageSnapshotRecord,
};
use rand::RngCore;
use serde_json::Value;

use super::model_pricing;
use crate::account_plan::resolve_effective_account_plan;
use crate::{refresh_aggregate_api_balance, storage_helpers::open_storage, usage_refresh};

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

fn local_day_bounds_ts() -> Result<(i64, i64), String> {
    let now = Local::now();
    let today = now.date_naive();
    let start_naive = today
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "build local start-of-day failed".to_string())?;
    let tomorrow_naive = (today + Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "build local end-of-day failed".to_string())?;
    let start = match Local.from_local_datetime(&start_naive) {
        LocalResult::Single(value) => value.timestamp(),
        LocalResult::Ambiguous(a, b) => a.timestamp().min(b.timestamp()),
        LocalResult::None => now.timestamp(),
    };
    let end = match Local.from_local_datetime(&tomorrow_naive) {
        LocalResult::Single(value) => value.timestamp(),
        LocalResult::Ambiguous(a, b) => a.timestamp().max(b.timestamp()),
        LocalResult::None => start + 86_400,
    };
    Ok((start, end.max(start)))
}

fn token_total(input: i64, cached: i64, output: i64) -> i64 {
    input.saturating_sub(cached).saturating_add(output).max(0)
}

fn parse_balance_snapshot(api: &AggregateApi) -> BalanceSnapshot {
    let Some(raw) = api
        .last_balance_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
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

fn balance_usd(api: &AggregateApi) -> Option<f64> {
    parse_balance_snapshot(api)
        .remaining
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn remaining_percent(used_percent: Option<f64>) -> Option<f64> {
    used_percent.map(|used| (100.0 - used.clamp(0.0, 100.0)).max(0.0))
}

fn average_percent(values: impl Iterator<Item = Option<f64>>) -> Option<i64> {
    let mut count = 0_i64;
    let mut total = 0.0_f64;
    for value in values.flatten() {
        total += value;
        count += 1;
    }
    (count > 0).then(|| (total / count as f64).round() as i64)
}

fn is_low_quota(usage: Option<&UsageSnapshotRecord>) -> bool {
    let Some(usage) = usage else {
        return false;
    };
    for remain in [
        remaining_percent(usage.used_percent),
        remaining_percent(usage.secondary_used_percent),
    ]
    .into_iter()
    .flatten()
    {
        if remain > 0.0 && remain <= 20.0 {
            return true;
        }
    }
    false
}

fn account_is_available(account: &Account) -> bool {
    matches!(account.status.as_str(), "active" | "available")
}

fn api_display_name(api: &AggregateApi) -> String {
    api.supplier_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(api.url.as_str())
        .to_string()
}

fn key_display_name(key: &ApiKey) -> String {
    key.name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(key.id.as_str())
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
    let _price_rules = model_pricing::load_enabled_price_rules(&storage)?;

    let api_keys = storage
        .list_api_keys()
        .map_err(|err| format!("list api keys failed: {err}"))?;
    let quota_limits = storage
        .list_api_key_quota_limits()
        .map_err(|err| format!("list api key quota limits failed: {err}"))?;
    let usage_by_key = storage
        .summarize_request_token_stats_by_key()
        .map_err(|err| format!("summarize api key usage failed: {err}"))?;
    let usage_map = usage_by_key
        .iter()
        .map(|item| (item.key_id.as_str(), item))
        .collect::<HashMap<_, _>>();

    let mut total_limit_tokens = 0_i64;
    let mut total_used_tokens = 0_i64;
    let mut total_remaining_tokens = 0_i64;
    let mut estimated_cost_usd = 0.0_f64;
    for key in &api_keys {
        let used = usage_map
            .get(key.id.as_str())
            .map(|item| item.total_tokens.max(0))
            .unwrap_or(0);
        total_used_tokens = total_used_tokens.saturating_add(used);
        estimated_cost_usd += usage_map
            .get(key.id.as_str())
            .map(|item| item.estimated_cost_usd.max(0.0))
            .unwrap_or(0.0);
        if let Some(limit) = quota_limits
            .get(key.id.as_str())
            .copied()
            .filter(|value| *value > 0)
        {
            total_limit_tokens = total_limit_tokens.saturating_add(limit);
            total_remaining_tokens =
                total_remaining_tokens.saturating_add(limit.saturating_sub(used));
        }
    }

    let aggregate_apis = storage
        .list_aggregate_apis()
        .map_err(|err| format!("list aggregate APIs failed: {err}"))?;
    let mut aggregate_ok_count = 0_i64;
    let mut aggregate_error_count = 0_i64;
    let mut total_balance_usd = 0.0_f64;
    let mut has_balance = false;
    let mut last_refreshed_at: Option<i64> = None;
    for api in &aggregate_apis {
        match api.last_balance_status.as_deref() {
            Some("success") => aggregate_ok_count += 1,
            Some("error" | "failed") => aggregate_error_count += 1,
            _ => {}
        }
        if let Some(balance) = balance_usd(api) {
            total_balance_usd += balance;
            has_balance = true;
        }
        if let Some(ts) = api.last_balance_at {
            last_refreshed_at = Some(last_refreshed_at.map_or(ts, |current| current.max(ts)));
        }
    }

    let accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let usage_items = storage
        .latest_usage_snapshots_by_account()
        .map_err(|err| format!("list usage snapshots failed: {err}"))?;
    let usage_map = usage_items
        .iter()
        .map(|item| (item.account_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let last_usage_at = usage_items.iter().map(|item| item.captured_at).max();
    let available_count = accounts
        .iter()
        .filter(|account| account_is_available(account))
        .count() as i64;
    let low_quota_count = accounts
        .iter()
        .filter(|account| is_low_quota(usage_map.get(account.id.as_str()).copied()))
        .count() as i64;

    let (day_start, day_end) = local_day_bounds_ts()?;
    let today = storage
        .summarize_request_token_stats_between(day_start, day_end)
        .map_err(|err| format!("summarize today token usage failed: {err}"))?;
    let today_input = today.input_tokens.max(0);
    let today_cached = today.cached_input_tokens.max(0);
    let today_output = today.output_tokens.max(0);

    Ok(QuotaOverviewResult {
        api_key: QuotaApiKeyOverviewResult {
            key_count: api_keys.len() as i64,
            limited_key_count: quota_limits.len() as i64,
            total_limit_tokens: (total_limit_tokens > 0).then_some(total_limit_tokens),
            total_used_tokens,
            total_remaining_tokens: (total_limit_tokens > 0).then_some(total_remaining_tokens),
            estimated_cost_usd: estimated_cost_usd.max(0.0),
        },
        aggregate_api: QuotaAggregateApiOverviewResult {
            source_count: aggregate_apis.len() as i64,
            enabled_balance_query_count: aggregate_apis
                .iter()
                .filter(|api| api.balance_query_enabled)
                .count() as i64,
            ok_count: aggregate_ok_count,
            error_count: aggregate_error_count,
            total_balance_usd: has_balance.then_some(total_balance_usd.max(0.0)),
            last_refreshed_at,
        },
        openai_account: QuotaOpenAiAccountOverviewResult {
            account_count: accounts.len() as i64,
            available_count,
            low_quota_count,
            primary_remain_percent: average_percent(
                usage_items
                    .iter()
                    .map(|item| remaining_percent(item.used_percent)),
            ),
            secondary_remain_percent: average_percent(
                usage_items
                    .iter()
                    .map(|item| remaining_percent(item.secondary_used_percent)),
            ),
            last_refreshed_at: last_usage_at,
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
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
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
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
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
        if storage
            .find_app_user_by_id(user_id)
            .map_err(|err| format!("read app user failed: {err}"))?
            .is_none()
        {
            return Err("计费规则用户不存在".to_string());
        }
    }
    let api_key_id = normalize_optional_text(input.api_key_id);
    if let Some(api_key_id) = api_key_id.as_deref() {
        if storage
            .find_api_key_by_id(api_key_id)
            .map_err(|err| format!("read api key failed: {err}"))?
            .is_none()
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
    read_billing_rules()
}

pub(crate) fn delete_billing_rule(id: &str) -> Result<QuotaBillingRulesResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let id = id.trim();
    if id.is_empty() {
        return Err("计费规则 ID 不能为空".to_string());
    }
    storage
        .delete_billing_rule(id)
        .map_err(|err| format!("delete billing rule failed: {err}"))?;
    read_billing_rules()
}

pub(crate) fn read_quota_model_usage(
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> Result<QuotaModelUsageResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let price_rules = model_pricing::load_enabled_price_rules(&storage)?;
    let usage = storage
        .summarize_request_token_stats_by_model(start_ts, end_ts)
        .map_err(|err| format!("summarize token usage by model failed: {err}"))?;
    let quota_limits = storage
        .list_api_key_quota_limits()
        .map_err(|err| format!("list api key quota limits failed: {err}"))?;
    let key_usage = storage
        .summarize_request_token_stats_by_key()
        .map_err(|err| format!("summarize key usage failed: {err}"))?;
    let api_key_remaining_tokens = quota_limits
        .iter()
        .map(|(key_id, limit)| {
            let used = key_usage
                .iter()
                .find(|item| item.key_id == *key_id)
                .map(|item| item.total_tokens.max(0))
                .unwrap_or(0);
            limit.saturating_sub(used)
        })
        .sum::<i64>();

    let aggregate_balance_usd = storage
        .list_aggregate_apis()
        .map_err(|err| format!("list aggregate APIs failed: {err}"))?
        .iter()
        .filter_map(balance_usd)
        .sum::<f64>();
    let aggregate_balance_usd = (aggregate_balance_usd > 0.0).then_some(aggregate_balance_usd);

    let accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let usage_items = storage
        .latest_usage_snapshots_by_account()
        .map_err(|err| format!("list usage snapshots failed: {err}"))?;

    let openai_available_account_count = accounts
        .iter()
        .filter(|account| account_is_available(account))
        .count() as i64;
    let openai_primary_remain_percent = average_percent(
        usage_items
            .iter()
            .map(|item| remaining_percent(item.used_percent)),
    );
    let openai_secondary_remain_percent = average_percent(
        usage_items
            .iter()
            .map(|item| remaining_percent(item.secondary_used_percent)),
    );

    Ok(QuotaModelUsageResult {
        items: usage
            .into_iter()
            .map(|item| {
                let cost = model_pricing::estimate_cost_with_rules(
                    &price_rules,
                    Some(item.model.as_str()),
                    item.input_tokens,
                    item.cached_input_tokens,
                    item.output_tokens,
                );
                let aggregate_estimated_remaining_tokens =
                    aggregate_balance_usd.and_then(|balance| {
                        model_pricing::estimate_remaining_tokens_from_usd_with_rules(
                            &price_rules,
                            &item.model,
                            balance,
                        )
                    });
                QuotaModelUsageItem {
                    model: item.model,
                    provider: cost.provider,
                    input_tokens: item.input_tokens,
                    cached_input_tokens: item.cached_input_tokens,
                    output_tokens: item.output_tokens,
                    reasoning_output_tokens: item.reasoning_output_tokens,
                    total_tokens: item.total_tokens,
                    estimated_cost_usd: cost.cost_usd,
                    price_status: cost.price_status.to_string(),
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

const ACCOUNT_CAPACITY_TEMPLATE_SLOTS: &[&str] = &["free", "plus", "pro", "team", "enterprise"];

pub(crate) fn read_quota_capacity_config() -> Result<QuotaCapacityConfigResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    Ok(QuotaCapacityConfigResult {
        source_assignments: build_source_assignment_results(
            storage
                .list_quota_source_model_assignments()
                .map_err(|err| format!("list quota source assignments failed: {err}"))?,
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

pub(crate) fn set_quota_source_models(
    source_kind: &str,
    source_id: &str,
    model_slugs: Vec<String>,
) -> Result<QuotaCapacityConfigResult, String> {
    let mut storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    storage
        .set_quota_source_model_assignments(source_kind, source_id, model_slugs.as_slice())
        .map_err(|err| format!("set quota source model assignments failed: {err}"))?;
    drop(storage);
    read_quota_capacity_config()
}

pub(crate) fn update_account_quota_capacity_template(
    plan_type: &str,
    primary_window_tokens: Option<i64>,
    secondary_window_tokens: Option<i64>,
) -> Result<QuotaCapacityConfigResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    storage
        .upsert_account_quota_capacity_template(
            plan_type,
            primary_window_tokens,
            secondary_window_tokens,
        )
        .map_err(|err| format!("update account quota capacity template failed: {err}"))?;
    drop(storage);
    read_quota_capacity_config()
}

pub(crate) fn update_account_quota_capacity_override(
    account_id: &str,
    primary_window_tokens: Option<i64>,
    secondary_window_tokens: Option<i64>,
) -> Result<QuotaCapacityConfigResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    storage
        .upsert_account_quota_capacity_override(
            account_id,
            primary_window_tokens,
            secondary_window_tokens,
        )
        .map_err(|err| format!("update account quota capacity override failed: {err}"))?;
    drop(storage);
    read_quota_capacity_config()
}

pub(crate) fn read_quota_model_pools() -> Result<QuotaModelPoolsResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let price_rules = model_pricing::load_enabled_price_rules(&storage)?;
    let api_models = api_available_model_slugs(&storage, &price_rules)?;
    let assignments = assignment_map(
        storage
            .list_quota_source_model_assignments()
            .map_err(|err| format!("list quota source assignments failed: {err}"))?,
    );
    let pools = build_model_pool_accumulators(&storage, &price_rules, &api_models, &assignments)?;
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

pub(crate) fn read_quota_system_pool(
    reference_model: Option<String>,
) -> Result<QuotaSystemPoolResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let price_rules = model_pricing::load_enabled_price_rules(&storage)?;
    let api_models = api_available_model_slugs(&storage, &price_rules)?;
    let reference_model = reference_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| api_models.first().cloned())
        .unwrap_or_else(|| "unknown".to_string());
    let assignments = assignment_map(
        storage
            .list_quota_source_model_assignments()
            .map_err(|err| format!("list quota source assignments failed: {err}"))?,
    );
    let mut pools =
        build_model_pool_accumulators(&storage, &price_rules, &api_models, &assignments)?;
    let pool = pools.remove(reference_model.as_str()).unwrap_or_else(|| {
        let mut pool = PoolAccumulator {
            price_status: "missing".to_string(),
            ..PoolAccumulator::default()
        };
        if let Some(price) =
            model_pricing::resolve_model_price_from_rules(&price_rules, reference_model.as_str(), 0)
                .or_else(|| model_pricing::resolve_model_price(reference_model.as_str(), 0))
        {
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
    price_rules: &[ModelPriceRule],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
) -> Result<BTreeMap<String, PoolAccumulator>, String> {
    let mut pools = BTreeMap::<String, PoolAccumulator>::new();
    seed_model_pools(&mut pools, price_rules, api_models);
    add_aggregate_api_pools(storage, price_rules, api_models, assignments, &mut pools)?;
    add_account_pools(storage, price_rules, api_models, assignments, &mut pools)?;
    Ok(pools)
}

fn seed_model_pools(
    pools: &mut BTreeMap<String, PoolAccumulator>,
    price_rules: &[ModelPriceRule],
    api_models: &[String],
) {
    for model in api_models {
        let price = model_pricing::resolve_model_price_from_rules(price_rules, model, 0)
            .or_else(|| model_pricing::resolve_model_price(model, 0));
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
    price_rules: &[ModelPriceRule],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
    pools: &mut BTreeMap<String, PoolAccumulator>,
) -> Result<(), String> {
    let aggregate_apis = storage
        .list_aggregate_apis()
        .map_err(|err| format!("list aggregate APIs failed: {err}"))?;
    for api in aggregate_apis {
        if api.status == "disabled" {
            continue;
        }
        let balance = parse_balance_snapshot(&api);
        let models = source_models("aggregate_api", &api.id, assignments, api_models);
        let balance_unit = balance
            .unit
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("USD")
            .to_string();
        let status = match api.last_balance_status.as_deref() {
            Some("success") => "ok",
            Some("error" | "failed") => "error",
            _ if api.balance_query_enabled => "unknown",
            _ => "warning",
        };
        for model in models {
            let model = model.trim().to_string();
            if model.is_empty() {
                continue;
            }
            let provider = model_pricing::resolve_model_price_from_rules(price_rules, &model, 0)
                .or_else(|| model_pricing::resolve_model_price(&model, 0))
                .map(|price| price.provider);
            let remaining_tokens = balance.remaining.and_then(|remaining| {
                if is_usd_unit(balance_unit.as_str()) {
                    model_pricing::estimate_remaining_tokens_from_usd_with_rules(
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
                name: api_display_name(&api),
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
    price_rules: &[ModelPriceRule],
    api_models: &[String],
    assignments: &HashMap<(String, String), Vec<String>>,
    pools: &mut BTreeMap<String, PoolAccumulator>,
) -> Result<(), String> {
    let accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let usage_map = storage
        .latest_usage_snapshots_by_account()
        .map_err(|err| format!("list usage snapshots failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, UsageSnapshotRecord>>();
    let token_map = storage
        .list_tokens()
        .map_err(|err| format!("list account tokens failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, Token>>();
    let subscription_map = storage
        .list_account_subscriptions()
        .map_err(|err| format!("list account subscriptions failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountSubscription>>();
    let template_map = storage
        .list_account_quota_capacity_templates()
        .map_err(|err| format!("list account quota capacity templates failed: {err}"))?
        .into_iter()
        .map(|item| (item.plan_type.to_ascii_lowercase(), item))
        .collect::<HashMap<String, AccountQuotaCapacityTemplate>>();
    let override_map = storage
        .list_account_quota_capacity_overrides()
        .map_err(|err| format!("list account quota capacity overrides failed: {err}"))?
        .into_iter()
        .map(|item| (item.account_id.clone(), item))
        .collect::<HashMap<String, AccountQuotaCapacityOverride>>();

    for account in accounts {
        if !account_is_available(&account) {
            continue;
        }
        let account_id = account.id.clone();
        let usage = usage_map.get(account_id.as_str());
        let plan_type =
            resolve_account_plan_type(&account_id, &token_map, &usage_map, &subscription_map);
        let capacity = resolve_account_capacity(
            &account_id,
            plan_type.as_deref(),
            &template_map,
            &override_map,
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
            if model.is_empty() {
                continue;
            }
            let provider = model_pricing::resolve_model_price_from_rules(price_rules, &model, 0)
                .or_else(|| model_pricing::resolve_model_price(&model, 0))
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
    assignments: Vec<QuotaSourceModelAssignment>,
) -> Vec<QuotaSourceModelAssignmentResult> {
    let mut grouped = BTreeMap::<(String, String), Vec<String>>::new();
    for assignment in assignments {
        grouped
            .entry((assignment.source_kind, assignment.source_id))
            .or_default()
            .push(assignment.model_slug);
    }
    grouped
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
        .collect()
}

fn assignment_map(
    assignments: Vec<QuotaSourceModelAssignment>,
) -> HashMap<(String, String), Vec<String>> {
    let mut grouped = HashMap::<(String, String), BTreeSet<String>>::new();
    for assignment in assignments {
        grouped
            .entry((assignment.source_kind, assignment.source_id))
            .or_default()
            .insert(assignment.model_slug);
    }
    grouped
        .into_iter()
        .map(|(key, values)| (key, values.into_iter().collect()))
        .collect()
}

fn source_models(
    source_kind: &str,
    source_id: &str,
    assignments: &HashMap<(String, String), Vec<String>>,
    api_models: &[String],
) -> Vec<String> {
    assignments
        .get(&(source_kind.to_string(), source_id.to_string()))
        .filter(|models| !models.is_empty())
        .cloned()
        .unwrap_or_else(|| api_models.to_vec())
}

fn api_available_model_slugs(
    storage: &codexmanager_core::storage::Storage,
    price_rules: &[ModelPriceRule],
) -> Result<Vec<String>, String> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    for slug in storage
        .list_model_catalog_models("default")
        .map_err(|err| format!("list model catalog failed: {err}"))?
        .into_iter()
        .filter(|item| item.supported_in_api.unwrap_or(true))
        .filter(|item| {
            !matches!(
                item.visibility.as_deref(),
                Some("hidden" | "disabled" | "unavailable")
            )
        })
        .map(|item| item.slug)
    {
        let normalized = slug.trim();
        if normalized.is_empty() || !seen.insert(normalized.to_string()) {
            continue;
        }
        models.push(normalized.to_string());
    }

    if models.is_empty() {
        for rule in price_rules {
            let normalized = rule.model_pattern.trim();
            if normalized.is_empty()
                || normalized.contains('*')
                || !seen.insert(normalized.to_string())
            {
                continue;
            }
            models.push(normalized.to_string());
        }
    }

    Ok(models)
}

fn resolve_account_plan_type(
    account_id: &str,
    tokens: &HashMap<String, Token>,
    usages: &HashMap<String, UsageSnapshotRecord>,
    subscriptions: &HashMap<String, AccountSubscription>,
) -> Option<String> {
    resolve_effective_account_plan(
        tokens.get(account_id),
        usages.get(account_id),
        subscriptions.get(account_id),
    )
    .map(|plan| plan.normalized)
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

pub(crate) fn read_quota_api_key_usage() -> Result<QuotaApiKeyUsageResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let price_rules = model_pricing::load_enabled_price_rules(&storage)?;
    let api_keys = storage
        .list_api_keys()
        .map_err(|err| format!("list api keys failed: {err}"))?;
    let quota_limits = storage
        .list_api_key_quota_limits()
        .map_err(|err| format!("list api key quota limits failed: {err}"))?;
    let usage_by_key = storage
        .summarize_request_token_stats_by_key()
        .map_err(|err| format!("summarize api key usage failed: {err}"))?;
    let usage_map = usage_by_key
        .iter()
        .map(|item| (item.key_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let model_usage = storage
        .summarize_request_token_stats_by_key_and_model(None, None)
        .map_err(|err| format!("summarize api key model usage failed: {err}"))?;
    let mut models_by_key: BTreeMap<String, Vec<QuotaApiKeyModelUsageItem>> = BTreeMap::new();
    for item in model_usage {
        let cost = model_pricing::estimate_cost_with_rules(
            &price_rules,
            Some(item.model.as_str()),
            item.input_tokens,
            item.cached_input_tokens,
            item.output_tokens,
        );
        models_by_key
            .entry(item.key_id)
            .or_default()
            .push(QuotaApiKeyModelUsageItem {
                model: item.model,
                input_tokens: item.input_tokens,
                cached_input_tokens: item.cached_input_tokens,
                output_tokens: item.output_tokens,
                reasoning_output_tokens: item.reasoning_output_tokens,
                total_tokens: item.total_tokens,
                estimated_cost_usd: cost.cost_usd,
                price_status: cost.price_status.to_string(),
            });
    }

    Ok(QuotaApiKeyUsageResult {
        items: api_keys
            .into_iter()
            .map(|key| {
                let used = usage_map
                    .get(key.id.as_str())
                    .map(|item| item.total_tokens.max(0))
                    .unwrap_or(0);
                let limit = quota_limits.get(key.id.as_str()).copied();
                QuotaApiKeyUsageItem {
                    key_id: key.id.clone(),
                    name: key.name,
                    model_slug: key.model_slug,
                    quota_limit_tokens: limit,
                    used_tokens: used,
                    remaining_tokens: limit.map(|value| value.saturating_sub(used)),
                    estimated_cost_usd: usage_map
                        .get(key.id.as_str())
                        .map(|item| item.estimated_cost_usd.max(0.0))
                        .unwrap_or(0.0),
                    models: models_by_key.remove(key.id.as_str()).unwrap_or_default(),
                }
            })
            .collect(),
    })
}

pub(crate) fn read_quota_source_list() -> Result<QuotaSourceListResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let price_rules = model_pricing::load_enabled_price_rules(&storage)?;
    let api_models = api_available_model_slugs(&storage, &price_rules)?;
    let assignments = assignment_map(
        storage
            .list_quota_source_model_assignments()
            .map_err(|err| format!("list quota source assignments failed: {err}"))?,
    );
    let mut items = Vec::new();

    let api_keys = storage
        .list_api_keys()
        .map_err(|err| format!("list api keys failed: {err}"))?;
    let quota_limits = storage
        .list_api_key_quota_limits()
        .map_err(|err| format!("list api key quota limits failed: {err}"))?;
    let usage_by_key = storage
        .summarize_request_token_stats_by_key()
        .map_err(|err| format!("summarize api key usage failed: {err}"))?;
    let usage_map = usage_by_key
        .iter()
        .map(|item| (item.key_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    for key in api_keys {
        let used = usage_map
            .get(key.id.as_str())
            .map(|item| item.total_tokens.max(0))
            .unwrap_or(0);
        let limit = quota_limits.get(key.id.as_str()).copied();
        items.push(QuotaSourceSummary {
            id: key.id.clone(),
            kind: "api_key".to_string(),
            name: key_display_name(&key),
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

    for api in storage
        .list_aggregate_apis()
        .map_err(|err| format!("list aggregate APIs failed: {err}"))?
    {
        let balance = parse_balance_snapshot(&api);
        let status = match api.last_balance_status.as_deref() {
            Some("success") => "ok",
            Some("error" | "failed") => "error",
            _ if api.balance_query_enabled => "unknown",
            _ => "warning",
        };
        items.push(QuotaSourceSummary {
            id: api.id.clone(),
            kind: "aggregate_api".to_string(),
            name: api_display_name(&api),
            status: status.to_string(),
            metric_kind: "money_balance".to_string(),
            remaining: balance.remaining,
            total: balance.total,
            used: balance.used,
            unit: Some(balance.unit.unwrap_or_else(|| "USD".to_string())),
            models: source_models("aggregate_api", &api.id, &assignments, &api_models),
            provider: Some(api.provider_type),
            captured_at: api.last_balance_at,
            error: api.last_balance_error,
        });
    }

    for account in storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?
    {
        let usage = storage
            .latest_usage_snapshot_for_account(&account.id)
            .map_err(|err| format!("read account usage failed: {err}"))?;
        let remaining = usage
            .as_ref()
            .and_then(|item| remaining_percent(item.used_percent));
        let used = usage.as_ref().and_then(|item| item.used_percent);
        items.push(QuotaSourceSummary {
            id: account.id.clone(),
            kind: "openai_account".to_string(),
            name: account.label.clone(),
            status: if account_is_available(&account) {
                "ok".to_string()
            } else {
                account.status
            },
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
    let kinds = if input.kinds.is_empty() {
        HashSet::from(["aggregate_api".to_string(), "openai_account".to_string()])
    } else {
        input.kinds.into_iter().collect::<HashSet<_>>()
    };
    let source_ids = input.source_ids.into_iter().collect::<HashSet<_>>();
    let mut items = Vec::new();

    if kinds.contains("aggregate_api") {
        let aggregate_apis = storage
            .list_aggregate_apis()
            .map_err(|err| format!("list aggregate APIs failed: {err}"))?;
        for api in aggregate_apis {
            if !source_ids.is_empty() && !source_ids.contains(api.id.as_str()) {
                continue;
            }
            if !api.balance_query_enabled {
                continue;
            }
            let result = refresh_aggregate_api_balance(api.id.as_str());
            items.push(QuotaRefreshSourceResult {
                id: api.id,
                kind: "aggregate_api".to_string(),
                ok: result.is_ok(),
                error: result.err(),
            });
        }
    }

    if kinds.contains("openai_account") {
        let accounts = storage
            .list_accounts()
            .map_err(|err| format!("list accounts failed: {err}"))?;
        for account in accounts {
            if !source_ids.is_empty() && !source_ids.contains(account.id.as_str()) {
                continue;
            }
            let result = usage_refresh::refresh_usage_for_account(account.id.as_str());
            items.push(QuotaRefreshSourceResult {
                id: account.id,
                kind: "openai_account".to_string(),
                ok: result.is_ok(),
                error: result.err(),
            });
        }
    }

    Ok(QuotaRefreshSourcesResult { items })
}

pub(crate) fn list_model_price_rules() -> Result<ModelPriceRuleListResult, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let rules = storage
        .list_enabled_model_price_rules()
        .map_err(|err| format!("list model price rules failed: {err}"))?;
    Ok(ModelPriceRuleListResult {
        items: rules.into_iter().map(price_rule_entry).collect(),
    })
}

pub(crate) fn read_model_price_rule(
    model_pattern: &str,
) -> Result<Option<ModelPriceRuleEntry>, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let rules = storage
        .list_enabled_model_price_rules()
        .map_err(|err| format!("list model price rules failed: {err}"))?;
    let normalized = model_pattern.trim().to_ascii_lowercase();
    Ok(rules
        .into_iter()
        .find(|rule| {
            rule.source == "custom"
                && rule.match_type.eq_ignore_ascii_case("exact")
                && rule.model_pattern.to_ascii_lowercase() == normalized
        })
        .map(price_rule_entry))
}

pub(crate) fn upsert_model_price_rule(
    input: ModelPriceRuleUpsertInput,
) -> Result<ModelPriceRuleEntry, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let now = now_ts();
    let model_pattern = input.model_pattern.trim().to_string();
    if model_pattern.is_empty() {
        return Err("model_pattern 不能为空".to_string());
    }
    let id = input
        .id
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| format!("user-{}", model_pattern));
    let rule = ModelPriceRule {
        id,
        provider: input
            .provider
            .unwrap_or_else(|| crate::quota::model_pricing::infer_provider(&model_pattern).to_string()),
        model_pattern,
        match_type: input
            .match_type
            .unwrap_or_else(|| "exact".to_string()),
        billing_mode: "standard".to_string(),
        currency: "USD".to_string(),
        unit: "per_1m_tokens".to_string(),
        input_price_per_1m: input.input_price_per_1m,
        cached_input_price_per_1m: input.cached_input_price_per_1m,
        output_price_per_1m: input.output_price_per_1m,
        reasoning_output_price_per_1m: None,
        cache_write_5m_price_per_1m: None,
        cache_write_1h_price_per_1m: None,
        cache_hit_price_per_1m: None,
        long_context_threshold_tokens: None,
        long_context_input_price_per_1m: None,
        long_context_cached_input_price_per_1m: None,
        long_context_output_price_per_1m: None,
        source: "custom".to_string(),
        source_url: None,
        seed_version: None,
        enabled: input.enabled.unwrap_or(true),
        priority: input.priority.unwrap_or(20000),
        created_at: now,
        updated_at: now,
    };
    if let Some(v) = rule.input_price_per_1m {
        if !v.is_finite() || v < 0.0 {
            return Err("input_price_per_1m 必须为非负有效数字".to_string());
        }
    }
    if let Some(v) = rule.cached_input_price_per_1m {
        if !v.is_finite() || v < 0.0 {
            return Err("cached_input_price_per_1m 必须为非负有效数字".to_string());
        }
    }
    if let Some(v) = rule.output_price_per_1m {
        if !v.is_finite() || v < 0.0 {
            return Err("output_price_per_1m 必须为非负有效数字".to_string());
        }
    }
    storage
        .upsert_model_price_rule(&rule)
        .map_err(|err| format!("upsert model price rule failed: {err}"))?;
    Ok(price_rule_entry(rule))
}

fn price_rule_entry(rule: ModelPriceRule) -> ModelPriceRuleEntry {
    ModelPriceRuleEntry {
        id: rule.id,
        provider: rule.provider,
        model_pattern: rule.model_pattern,
        match_type: rule.match_type,
        input_price_per_1m: rule.input_price_per_1m,
        cached_input_price_per_1m: rule.cached_input_price_per_1m,
        output_price_per_1m: rule.output_price_per_1m,
        enabled: rule.enabled,
        priority: rule.priority,
        source: rule.source,
        created_at: rule.created_at,
        updated_at: rule.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{ModelCatalogModelRecord, Storage};

    fn model_record(slug: &str, sort_index: i64) -> ModelCatalogModelRecord {
        ModelCatalogModelRecord {
            scope: "default".to_string(),
            slug: slug.to_string(),
            display_name: slug.to_string(),
            source_kind: "remote".to_string(),
            supported_in_api: Some(true),
            extra_json: "{}".to_string(),
            sort_index,
            updated_at: 1,
            ..ModelCatalogModelRecord::default()
        }
    }

    #[test]
    fn api_available_model_slugs_preserves_catalog_sort_order() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        storage
            .upsert_model_catalog_models(&[
                model_record("z-model", 0),
                model_record("a-model", 1),
                ModelCatalogModelRecord {
                    visibility: Some("hidden".to_string()),
                    ..model_record("hidden-model", 2)
                },
                ModelCatalogModelRecord {
                    supported_in_api: Some(false),
                    ..model_record("disabled-model", 3)
                },
            ])
            .expect("upsert models");

        let models = api_available_model_slugs(&storage, &[]).expect("available models");

        assert_eq!(models, vec!["z-model", "a-model"]);
    }
}
