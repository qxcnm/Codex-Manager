use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Duration, Local, LocalResult, TimeZone};
use codexmanager_core::rpc::types::{
    ApiKeySummary, DashboardAdminUsageSummaryResult, DashboardDailyUsagePoint,
    DashboardSourceUsageSummary, DashboardTokenUsageResult, DashboardUserUsageSummary,
    MemberDashboardAlert, MemberDashboardApiKeySummary, MemberDashboardKeyUsage,
    MemberDashboardModelUsage, MemberDashboardSummaryResult, MemberDashboardUsagePoint,
    MemberDashboardUsageToday, MemberDashboardWalletResult, ModelInfo, RequestLogListParams,
};
use codexmanager_core::storage::{
    DailyTokenUsageRollup, SourceTokenUsageRollup, TokenUsageRollup, UserTokenUsageRollup,
};
use serde_json::json;

use crate::{
    apikey_list, apikey_models, quota::model_pricing, requestlog_list, storage_helpers, RpcActor,
};

const TREND_DAYS: i64 = 7;
const MEMBER_TOP_KEY_LIMIT: usize = 8;
const MEMBER_TOP_MODEL_LIMIT: usize = 6;
const MEMBER_RECENT_LOG_LIMIT: i64 = 8;
const LOW_WALLET_CREDIT_MICROS: i64 = 1_000_000;
const DAY_SECONDS: i64 = 24 * 60 * 60;
const ADMIN_USAGE_RANGE_DAYS: i64 = 7;

pub(crate) fn read_admin_usage_summary(
    actor: &RpcActor,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
) -> Result<DashboardAdminUsageSummaryResult, String> {
    if !actor.is_admin() {
        return Err("permission_denied: admin dashboard usage requires admin session".to_string());
    }
    crate::initialize_storage_if_needed()?;
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let (today_start, today_end) = local_day_bounds_ts()?;
    let range_start = start_ts
        .filter(|value| *value > 0)
        .unwrap_or_else(|| today_start.saturating_sub((ADMIN_USAGE_RANGE_DAYS - 1) * DAY_SECONDS));
    let range_end = end_ts
        .filter(|value| *value > range_start)
        .unwrap_or(today_end);

    let today_usage = storage
        .summarize_request_token_stats_daily(today_start, today_end, DAY_SECONDS)
        .map_err(|err| format!("summarize today usage failed: {err}"))?
        .into_iter()
        .next()
        .map(|item| item.usage)
        .unwrap_or_default();
    let daily_usage = fill_daily_usage(
        range_start,
        range_end,
        DAY_SECONDS,
        storage
            .summarize_request_token_stats_daily(range_start, range_end, DAY_SECONDS)
            .map_err(|err| format!("summarize daily usage failed: {err}"))?,
    );
    let users = build_dashboard_user_summaries(
        &storage,
        storage
            .summarize_request_token_stats_by_user_between(today_start, today_end)
            .map_err(|err| format!("summarize today user usage failed: {err}"))?,
        storage
            .summarize_request_token_stats_by_user_between(range_start, range_end)
            .map_err(|err| format!("summarize range user usage failed: {err}"))?,
    )?;
    let openai_accounts = build_dashboard_source_summaries(
        "openai_account",
        account_source_metadata(&storage)?,
        storage
            .summarize_request_token_stats_by_source_between(
                "openai_account",
                today_start,
                today_end,
            )
            .map_err(|err| format!("summarize today account usage failed: {err}"))?,
        storage
            .summarize_request_token_stats_by_source_between(
                "openai_account",
                range_start,
                range_end,
            )
            .map_err(|err| format!("summarize range account usage failed: {err}"))?,
    );
    let aggregate_apis = build_dashboard_source_summaries(
        "aggregate_api",
        aggregate_source_metadata(&storage)?,
        storage
            .summarize_request_token_stats_by_source_between(
                "aggregate_api",
                today_start,
                today_end,
            )
            .map_err(|err| format!("summarize today aggregate API usage failed: {err}"))?,
        storage
            .summarize_request_token_stats_by_source_between(
                "aggregate_api",
                range_start,
                range_end,
            )
            .map_err(|err| format!("summarize range aggregate API usage failed: {err}"))?,
    );

    Ok(DashboardAdminUsageSummaryResult {
        range_start_ts: range_start,
        range_end_ts: range_end,
        today_start_ts: today_start,
        today_end_ts: today_end,
        today_usage: dashboard_usage(&today_usage),
        daily_usage,
        users,
        openai_accounts,
        aggregate_apis,
    })
}

#[derive(Debug, Clone, Default)]
struct SourceMetadata {
    name: Option<String>,
    status: Option<String>,
    provider: Option<String>,
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
        LocalResult::None => start + DAY_SECONDS,
    };
    Ok((start, end.max(start)))
}

fn dashboard_usage(usage: &TokenUsageRollup) -> DashboardTokenUsageResult {
    DashboardTokenUsageResult {
        input_tokens: usage.input_tokens.max(0),
        cached_input_tokens: usage.cached_input_tokens.max(0),
        output_tokens: usage.output_tokens.max(0),
        reasoning_output_tokens: usage.reasoning_output_tokens.max(0),
        total_tokens: usage.total_tokens.max(0),
        estimated_cost_usd: usage.estimated_cost_usd.max(0.0),
        request_count: usage.request_count.max(0),
        success_count: usage.success_count.max(0),
        error_count: usage.error_count.max(0),
    }
}

fn fill_daily_usage(
    start_ts: i64,
    end_ts: i64,
    bucket_seconds: i64,
    items: Vec<DailyTokenUsageRollup>,
) -> Vec<DashboardDailyUsagePoint> {
    let bucket_seconds = bucket_seconds.max(1);
    let mut by_start = items
        .into_iter()
        .map(|item| (item.day_start_ts, item))
        .collect::<BTreeMap<_, _>>();
    let mut cursor = start_ts;
    let mut result = Vec::new();
    while cursor < end_ts {
        let next = cursor.saturating_add(bucket_seconds).min(end_ts);
        if let Some(item) = by_start.remove(&cursor) {
            result.push(DashboardDailyUsagePoint {
                day_start_ts: item.day_start_ts,
                day_end_ts: item.day_end_ts,
                usage: dashboard_usage(&item.usage),
            });
        } else {
            result.push(DashboardDailyUsagePoint {
                day_start_ts: cursor,
                day_end_ts: next,
                usage: DashboardTokenUsageResult::default(),
            });
        }
        cursor = next;
    }
    result
}

fn build_dashboard_user_summaries(
    storage: &codexmanager_core::storage::Storage,
    today_items: Vec<UserTokenUsageRollup>,
    range_items: Vec<UserTokenUsageRollup>,
) -> Result<Vec<DashboardUserUsageSummary>, String> {
    let today_map = today_items
        .into_iter()
        .map(|item| (item.user_id, item.usage))
        .collect::<HashMap<_, _>>();
    let range_map = range_items
        .into_iter()
        .map(|item| (item.user_id, item.usage))
        .collect::<HashMap<_, _>>();
    let users = storage
        .list_app_users()
        .map_err(|err| format!("list app users failed: {err}"))?;
    let wallets = storage
        .list_wallets()
        .map_err(|err| format!("list app wallets failed: {err}"))?
        .into_iter()
        .filter(|wallet| wallet.owner_kind == "user")
        .map(|wallet| (wallet.owner_id.clone(), wallet))
        .collect::<HashMap<_, _>>();
    let mut user_ids = users
        .iter()
        .map(|user| user.id.clone())
        .collect::<HashSet<_>>();
    user_ids.extend(today_map.keys().cloned());
    user_ids.extend(range_map.keys().cloned());
    let user_map = users
        .into_iter()
        .map(|user| (user.id.clone(), user))
        .collect::<HashMap<_, _>>();

    let mut results = user_ids
        .into_iter()
        .map(|user_id| {
            let user = user_map.get(user_id.as_str());
            let wallet_available = wallets
                .get(user_id.as_str())
                .map(|wallet| wallet.balance_credit_micros - wallet.frozen_credit_micros);
            DashboardUserUsageSummary {
                user_id: user_id.clone(),
                username: user.map(|item| item.username.clone()),
                display_name: user.and_then(|item| item.display_name.clone()),
                role: user.map(|item| item.role.clone()),
                status: user.map(|item| item.status.clone()),
                wallet_available_credit_micros: wallet_available,
                today_usage: dashboard_usage(
                    today_map
                        .get(user_id.as_str())
                        .unwrap_or(&TokenUsageRollup::default()),
                ),
                range_usage: dashboard_usage(
                    range_map
                        .get(user_id.as_str())
                        .unwrap_or(&TokenUsageRollup::default()),
                ),
            }
        })
        .collect::<Vec<_>>();
    results.sort_by(|a, b| {
        b.today_usage
            .total_tokens
            .cmp(&a.today_usage.total_tokens)
            .then_with(|| b.range_usage.total_tokens.cmp(&a.range_usage.total_tokens))
            .then_with(|| a.user_id.cmp(&b.user_id))
    });
    Ok(results)
}

fn account_source_metadata(
    storage: &codexmanager_core::storage::Storage,
) -> Result<HashMap<String, SourceMetadata>, String> {
    Ok(storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?
        .into_iter()
        .map(|account| {
            (
                account.id,
                SourceMetadata {
                    name: Some(account.label),
                    status: Some(account.status),
                    provider: Some("openai".to_string()),
                },
            )
        })
        .collect())
}

fn aggregate_source_metadata(
    storage: &codexmanager_core::storage::Storage,
) -> Result<HashMap<String, SourceMetadata>, String> {
    Ok(storage
        .list_aggregate_apis()
        .map_err(|err| format!("list aggregate APIs failed: {err}"))?
        .into_iter()
        .map(|api| {
            let name = api
                .supplier_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(api.url.as_str())
                .to_string();
            (
                api.id,
                SourceMetadata {
                    name: Some(name),
                    status: Some(api.status),
                    provider: Some(api.provider_type),
                },
            )
        })
        .collect())
}

fn build_dashboard_source_summaries(
    source_kind: &str,
    metadata: HashMap<String, SourceMetadata>,
    today_items: Vec<SourceTokenUsageRollup>,
    range_items: Vec<SourceTokenUsageRollup>,
) -> Vec<DashboardSourceUsageSummary> {
    let today_map = today_items
        .into_iter()
        .map(|item| (item.source_id, item.usage))
        .collect::<HashMap<_, _>>();
    let range_map = range_items
        .into_iter()
        .map(|item| (item.source_id, item.usage))
        .collect::<HashMap<_, _>>();
    let mut ids = metadata.keys().cloned().collect::<HashSet<_>>();
    ids.extend(today_map.keys().cloned());
    ids.extend(range_map.keys().cloned());
    let mut results = ids
        .into_iter()
        .map(|source_id| {
            let meta = metadata
                .get(source_id.as_str())
                .cloned()
                .unwrap_or_default();
            DashboardSourceUsageSummary {
                source_kind: source_kind.to_string(),
                source_id: source_id.clone(),
                name: meta.name,
                status: meta.status,
                provider: meta.provider,
                today_usage: dashboard_usage(
                    today_map
                        .get(source_id.as_str())
                        .unwrap_or(&TokenUsageRollup::default()),
                ),
                range_usage: dashboard_usage(
                    range_map
                        .get(source_id.as_str())
                        .unwrap_or(&TokenUsageRollup::default()),
                ),
            }
        })
        .collect::<Vec<_>>();
    results.sort_by(|a, b| {
        b.today_usage
            .total_tokens
            .cmp(&a.today_usage.total_tokens)
            .then_with(|| b.range_usage.total_tokens.cmp(&a.range_usage.total_tokens))
            .then_with(|| a.source_id.cmp(&b.source_id))
    });
    results
}

pub(crate) fn read_member_dashboard_summary(
    actor: &RpcActor,
    requested_user_id: Option<String>,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
) -> Result<MemberDashboardSummaryResult, String> {
    crate::initialize_storage_if_needed()?;
    let distribution_enabled = crate::distribution_enabled();
    let target_user_id = resolve_target_user_id(actor, requested_user_id)?;
    let (day_start, day_end) = resolve_day_bounds(day_start_ts, day_end_ts);

    let Some(user_id) = target_user_id else {
        return Ok(empty_summary(
            None,
            distribution_enabled,
            vec![MemberDashboardAlert {
                kind: "no_user".to_string(),
                severity: "info".to_string(),
                title: "未选择成员".to_string(),
                message: "管理员调试普通用户仪表盘时需要指定成员。".to_string(),
                action_label: Some("账号管理".to_string()),
                action_href: Some("/account-manager/".to_string()),
            }],
        ));
    };

    let key_ids = crate::list_api_key_ids_for_user(&user_id)?;
    let api_keys = apikey_list::read_api_keys_for_ids(&key_ids)?;
    let api_key_summary = build_api_key_summary(&api_keys);
    let wallet = read_member_wallet(&user_id)?;

    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let today_usage_rollup = storage
        .summarize_request_token_stats_for_user_between(&user_id, day_start, day_end)
        .map_err(|err| format!("summarize member token usage failed: {err}"))?;
    let usage_today = MemberDashboardUsageToday {
        input_tokens: today_usage_rollup.input_tokens,
        cached_input_tokens: today_usage_rollup.cached_input_tokens,
        output_tokens: today_usage_rollup.output_tokens,
        reasoning_output_tokens: today_usage_rollup.reasoning_output_tokens,
        total_tokens: today_usage_rollup.total_tokens,
        estimated_cost_usd: today_usage_rollup.estimated_cost_usd,
        total_count: today_usage_rollup.request_count,
        success_count: today_usage_rollup.success_count,
        error_count: today_usage_rollup.error_count,
        success_rate: (today_usage_rollup.request_count > 0).then(|| {
            today_usage_rollup.success_count as f64 / today_usage_rollup.request_count as f64
        }),
    };

    let usage_trend_7d = read_usage_trend_7d(&user_id, day_start, day_end)?;
    let (top_keys, top_models) =
        read_member_usage_breakdown(&api_keys, &key_ids, day_start, day_end)?;
    let available_models = read_available_models_with_price_summary()?;
    let recent_logs = requestlog_list::read_request_log_page_for_key_ids(
        RequestLogListParams {
            page: 1,
            page_size: MEMBER_RECENT_LOG_LIMIT,
            query: None,
            status_filter: Some("all".to_string()),
            start_ts: None,
            end_ts: None,
        },
        &key_ids,
    )?
    .items;
    let alerts = build_alerts(
        distribution_enabled,
        wallet.as_ref(),
        &api_key_summary,
        &usage_today,
        available_models.len(),
    );

    Ok(MemberDashboardSummaryResult {
        user_id: Some(user_id),
        distribution_enabled,
        wallet,
        api_key_summary,
        usage_today,
        usage_trend_7d,
        top_keys,
        top_models,
        available_models,
        recent_logs,
        alerts,
    })
}

fn resolve_target_user_id(
    actor: &RpcActor,
    requested_user_id: Option<String>,
) -> Result<Option<String>, String> {
    if actor.is_admin() {
        return Ok(requested_user_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| actor.user_id.clone()));
    }
    actor
        .user_id
        .as_ref()
        .map(|value| Some(value.clone()))
        .ok_or_else(|| "permission_denied: dashboard requires user session".to_string())
}

fn resolve_day_bounds(day_start_ts: Option<i64>, day_end_ts: Option<i64>) -> (i64, i64) {
    match (
        day_start_ts.filter(|value| *value > 0),
        day_end_ts.filter(|value| *value > 0),
    ) {
        (Some(start), Some(end)) if end > start => (start, end),
        _ => {
            let now = codexmanager_core::storage::now_ts();
            let start = now - now.rem_euclid(24 * 60 * 60);
            (start, start + 24 * 60 * 60)
        }
    }
}

fn empty_summary(
    user_id: Option<String>,
    distribution_enabled: bool,
    alerts: Vec<MemberDashboardAlert>,
) -> MemberDashboardSummaryResult {
    MemberDashboardSummaryResult {
        user_id,
        distribution_enabled,
        alerts,
        ..MemberDashboardSummaryResult::default()
    }
}

fn read_member_wallet(user_id: &str) -> Result<Option<MemberDashboardWalletResult>, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let wallet = storage
        .find_wallet_by_owner("user", user_id)
        .map_err(|err| format!("read app wallet failed: {err}"))?;
    Ok(wallet.map(|wallet| MemberDashboardWalletResult {
        id: wallet.id,
        balance_credit_micros: wallet.balance_credit_micros,
        frozen_credit_micros: wallet.frozen_credit_micros,
        available_credit_micros: wallet
            .balance_credit_micros
            .saturating_sub(wallet.frozen_credit_micros),
        status: wallet.status,
        updated_at: wallet.updated_at,
    }))
}

fn read_available_models_with_price_summary() -> Result<Vec<ModelInfo>, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let price_rules = model_pricing::load_enabled_price_rules(&storage)?;
    Ok(apikey_models::read_model_options(false)?
        .models
        .into_iter()
        .filter(|model| model.supported_in_api && model.visibility.as_deref() != Some("hide"))
        .map(|mut model| {
            if let Some(price) =
                model_pricing::resolve_model_price_from_rules(&price_rules, &model.slug, 0)
                    .or_else(|| model_pricing::resolve_model_price(&model.slug, 0))
            {
                model.extra.insert(
                    "priceSummary".to_string(),
                    json!({
                        "provider": price.provider,
                        "inputUsdPer1M": price.input_price_per_1m,
                        "cachedInputUsdPer1M": price.cached_input_price_per_1m,
                        "outputUsdPer1M": price.output_price_per_1m,
                    }),
                );
            }
            model
        })
        .collect())
}

fn build_api_key_summary(api_keys: &[ApiKeySummary]) -> MemberDashboardApiKeySummary {
    let enabled_count = api_keys
        .iter()
        .filter(|key| {
            let status = key.status.trim().to_ascii_lowercase();
            status == "enabled" || status == "active"
        })
        .count() as i64;
    MemberDashboardApiKeySummary {
        total_count: api_keys.len() as i64,
        enabled_count,
        disabled_count: api_keys.len() as i64 - enabled_count,
        last_used_at: api_keys.iter().filter_map(|key| key.last_used_at).max(),
    }
}

fn read_usage_trend_7d(
    user_id: &str,
    day_start: i64,
    day_end: i64,
) -> Result<Vec<MemberDashboardUsagePoint>, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let day_span = (day_end - day_start).max(1);
    let range_start = day_start.saturating_sub((TREND_DAYS - 1) * day_span);
    let items = storage
        .summarize_request_token_stats_daily_for_user(user_id, range_start, day_end, day_span)
        .map_err(|err| format!("summarize member token trend failed: {err}"))?;
    let mut by_start = items
        .into_iter()
        .map(|item| (item.day_start_ts, item.usage))
        .collect::<BTreeMap<_, _>>();
    let mut points = Vec::new();
    for offset in (0..TREND_DAYS).rev() {
        let start = day_start.saturating_sub(offset * day_span);
        let end = start.saturating_add(day_span);
        let usage = by_start.remove(&start).unwrap_or_default();
        points.push(MemberDashboardUsagePoint {
            day_start_ts: start,
            day_end_ts: end,
            total_tokens: usage.total_tokens.max(0),
            estimated_cost_usd: usage.estimated_cost_usd.max(0.0),
        });
    }
    Ok(points)
}

fn read_member_usage_breakdown(
    api_keys: &[ApiKeySummary],
    key_ids: &[String],
    day_start: i64,
    day_end: i64,
) -> Result<(Vec<MemberDashboardKeyUsage>, Vec<MemberDashboardModelUsage>), String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let today_usage = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(
            Some(day_start),
            Some(day_end),
            key_ids,
        )
        .map_err(|err| format!("summarize today key usage failed: {err}"))?;
    let total_usage = storage
        .summarize_request_token_stats_by_key_for_keys(key_ids)
        .map_err(|err| format!("summarize key usage failed: {err}"))?;
    let seven_day_usage = storage
        .summarize_request_token_stats_by_key_and_model_for_keys(
            Some(day_start.saturating_sub((TREND_DAYS - 1) * (day_end - day_start).max(1))),
            Some(day_end),
            key_ids,
        )
        .map_err(|err| format!("summarize model usage failed: {err}"))?;

    let mut today_by_key: HashMap<String, (i64, f64)> = HashMap::new();
    for item in today_usage.into_iter() {
        let entry = today_by_key.entry(item.key_id).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(item.total_tokens.max(0));
        entry.1 += item.estimated_cost_usd.max(0.0);
    }

    let total_by_key = total_usage
        .into_iter()
        .map(|item| {
            (
                item.key_id,
                (item.total_tokens.max(0), item.estimated_cost_usd.max(0.0)),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut top_keys = api_keys
        .iter()
        .map(|key| {
            let (today_tokens, today_cost_usd) =
                today_by_key.get(&key.id).copied().unwrap_or((0, 0.0));
            let (total_tokens, total_cost_usd) =
                total_by_key.get(&key.id).copied().unwrap_or((0, 0.0));
            MemberDashboardKeyUsage {
                key_id: key.id.clone(),
                name: key.name.clone(),
                model_slug: key.model_slug.clone(),
                status: key.status.clone(),
                today_tokens,
                today_cost_usd,
                total_tokens,
                total_cost_usd,
                last_used_at: key.last_used_at,
            }
        })
        .collect::<Vec<_>>();
    top_keys.sort_by(|a, b| {
        b.today_tokens
            .cmp(&a.today_tokens)
            .then_with(|| b.last_used_at.cmp(&a.last_used_at))
            .then_with(|| a.key_id.cmp(&b.key_id))
    });
    top_keys.truncate(MEMBER_TOP_KEY_LIMIT);

    let mut model_usage = BTreeMap::<String, (i64, f64)>::new();
    for item in seven_day_usage.into_iter() {
        let entry = model_usage.entry(item.model).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(item.total_tokens.max(0));
        entry.1 += item.estimated_cost_usd.max(0.0);
    }
    let mut top_models = model_usage
        .into_iter()
        .map(
            |(model, (total_tokens, estimated_cost_usd))| MemberDashboardModelUsage {
                model,
                total_tokens,
                estimated_cost_usd,
            },
        )
        .collect::<Vec<_>>();
    top_models.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.model.cmp(&b.model))
    });
    top_models.truncate(MEMBER_TOP_MODEL_LIMIT);

    Ok((top_keys, top_models))
}

fn build_alerts(
    distribution_enabled: bool,
    wallet: Option<&MemberDashboardWalletResult>,
    api_key_summary: &MemberDashboardApiKeySummary,
    usage_today: &MemberDashboardUsageToday,
    available_model_count: usize,
) -> Vec<MemberDashboardAlert> {
    let mut alerts = Vec::new();
    if api_key_summary.total_count == 0 {
        alerts.push(MemberDashboardAlert {
            kind: "no_api_key".to_string(),
            severity: "warning".to_string(),
            title: "还没有平台 Key".to_string(),
            message: "创建一个平台 Key 后就可以通过网关调用可用模型。".to_string(),
            action_label: Some("创建 Key".to_string()),
            action_href: Some("/apikeys/".to_string()),
        });
    } else if api_key_summary.enabled_count == 0 {
        alerts.push(MemberDashboardAlert {
            kind: "no_enabled_key".to_string(),
            severity: "warning".to_string(),
            title: "平台 Key 均已停用".to_string(),
            message: "至少启用一个平台 Key 才能继续发起请求。".to_string(),
            action_label: Some("平台密钥".to_string()),
            action_href: Some("/apikeys/".to_string()),
        });
    }

    if distribution_enabled {
        match wallet {
            Some(wallet) if wallet.available_credit_micros <= 0 => {
                alerts.push(MemberDashboardAlert {
                    kind: "wallet_empty".to_string(),
                    severity: "critical".to_string(),
                    title: "钱包余额不足".to_string(),
                    message: "当前余额已不可用，请联系管理员充值。".to_string(),
                    action_label: Some("账号设置".to_string()),
                    action_href: Some("/settings/".to_string()),
                })
            }
            Some(wallet) if wallet.available_credit_micros < LOW_WALLET_CREDIT_MICROS => {
                alerts.push(MemberDashboardAlert {
                    kind: "wallet_low".to_string(),
                    severity: "warning".to_string(),
                    title: "钱包余额偏低".to_string(),
                    message: "余额低于 $1，额度较快耗尽时请求可能被拦截。".to_string(),
                    action_label: Some("账号设置".to_string()),
                    action_href: Some("/settings/".to_string()),
                });
            }
            None => alerts.push(MemberDashboardAlert {
                kind: "wallet_missing".to_string(),
                severity: "warning".to_string(),
                title: "钱包未初始化".to_string(),
                message: "当前账号还没有可用钱包，请联系管理员检查账号配置。".to_string(),
                action_label: Some("账号设置".to_string()),
                action_href: Some("/settings/".to_string()),
            }),
            _ => {}
        }
    }

    if available_model_count == 0 {
        alerts.push(MemberDashboardAlert {
            kind: "no_available_model".to_string(),
            severity: "critical".to_string(),
            title: "暂无可用模型".to_string(),
            message: "当前没有对 API 开放的模型，请联系管理员检查模型目录。".to_string(),
            action_label: Some("模型管理".to_string()),
            action_href: Some("/models/".to_string()),
        });
    }

    if usage_today.total_count >= 5
        && usage_today.error_count.saturating_mul(100) >= usage_today.total_count * 20
    {
        alerts.push(MemberDashboardAlert {
            kind: "high_failure_rate".to_string(),
            severity: "warning".to_string(),
            title: "今日失败率偏高".to_string(),
            message: "最近请求出现较多失败，可以到请求日志查看错误原因。".to_string(),
            action_label: Some("请求日志".to_string()),
            action_href: Some("/logs/".to_string()),
        });
    }

    alerts
}
