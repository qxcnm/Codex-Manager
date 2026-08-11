use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{apikey_list, requestlog_list, storage_helpers, time_bounds, RpcActor};
use codexmanager_core::rpc::types::{
    ApiKeySummary, DashboardAdminUsageSummaryResult, DashboardDailyUsagePoint,
    DashboardSourceUsageSummary, DashboardTokenUsageResult, DashboardUserUsageSummary,
    MemberDashboardAlert, MemberDashboardApiKeySummary, MemberDashboardKeyUsage,
    MemberDashboardModelUsage, MemberDashboardSummaryResult, MemberDashboardUsagePoint,
    MemberDashboardUsageToday, MemberDashboardWalletResult, RequestLogListParams,
};
use codexmanager_core::storage::{
    DailyTokenUsageRollup, SourceTokenUsageRollup, TokenUsageRollup, UserTokenUsageRollup,
};

const TREND_DAYS: i64 = 7;
const MEMBER_TOP_KEY_LIMIT: usize = 8;
const MEMBER_TOP_MODEL_LIMIT: usize = 6;
const MEMBER_RECENT_LOG_LIMIT: i64 = 8;
const LOW_WALLET_CREDIT_MICROS: i64 = 1_000_000;
const ADMIN_USAGE_RANGE_DAYS: i64 = 7;
const ADMIN_TOP_USER_LIMIT: usize = 12;
const ADMIN_TOP_SOURCE_LIMIT: usize = 1_000;

pub(crate) fn read_admin_usage_summary(
    actor: &RpcActor,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    include_breakdowns: bool,
) -> Result<DashboardAdminUsageSummaryResult, String> {
    if !actor.is_admin() {
        return Err("permission_denied: admin dashboard usage requires admin session".to_string());
    }
    crate::initialize_storage_if_needed()?;
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let (today_start, today_end) = time_bounds::local_day_bounds_ts()?;
    let range_start = start_ts.filter(|value| *value > 0).unwrap_or_else(|| {
        today_start.saturating_sub((ADMIN_USAGE_RANGE_DAYS - 1) * time_bounds::DAY_SECONDS)
    });
    let range_end = end_ts
        .filter(|value| *value > range_start)
        .unwrap_or(today_end);

    let raw_daily_usage = storage
        .summarize_request_token_stats_daily(range_start, range_end, time_bounds::DAY_SECONDS)
        .map_err(|err| format!("summarize daily usage failed: {err}"))?;
    let today_usage = match daily_usage_bucket(&raw_daily_usage, today_start, today_end) {
        Some(usage) => usage,
        None => storage
            .summarize_request_token_stats_daily(today_start, today_end, time_bounds::DAY_SECONDS)
            .map_err(|err| format!("summarize today usage failed: {err}"))?
            .into_iter()
            .next()
            .map(|item| item.usage)
            .unwrap_or_default(),
    };
    let daily_usage = fill_daily_usage(
        range_start,
        range_end,
        time_bounds::DAY_SECONDS,
        raw_daily_usage,
    );
    let (users, openai_accounts, kiro_credentials, grok_credentials, aggregate_apis) =
        if include_breakdowns {
            let users = build_dashboard_user_summaries(
                &storage,
                storage
                    .summarize_request_token_stats_by_user_between_limited(
                        today_start,
                        today_end,
                        Some(ADMIN_TOP_USER_LIMIT),
                    )
                    .map_err(|err| format!("summarize today user usage failed: {err}"))?,
                storage
                    .summarize_request_token_stats_by_user_between_limited(
                        range_start,
                        range_end,
                        Some(ADMIN_TOP_USER_LIMIT),
                    )
                    .map_err(|err| format!("summarize range user usage failed: {err}"))?,
            )?;
            let today_source_usage = storage
                .summarize_request_token_stats_by_sources_between_limited(
                    &["openai_account", "kiro", "grok", "aggregate_api"],
                    today_start,
                    today_end,
                    Some(ADMIN_TOP_SOURCE_LIMIT),
                )
                .map_err(|err| format!("summarize today source usage failed: {err}"))?;
            let range_source_usage = storage
                .summarize_request_token_stats_by_sources_between_limited(
                    &["openai_account", "kiro", "grok", "aggregate_api"],
                    range_start,
                    range_end,
                    Some(ADMIN_TOP_SOURCE_LIMIT),
                )
                .map_err(|err| format!("summarize range source usage failed: {err}"))?;
            let today_account_usage = filter_source_usage(&today_source_usage, "openai_account");
            let range_account_usage = filter_source_usage(&range_source_usage, "openai_account");
            let openai_accounts = build_dashboard_source_summaries(
                "openai_account",
                account_source_metadata(
                    &storage,
                    &dashboard_source_ids(&today_account_usage, &range_account_usage),
                )?,
                today_account_usage,
                range_account_usage,
            );
            let today_kiro_usage = filter_source_usage(&today_source_usage, "kiro");
            let range_kiro_usage = filter_source_usage(&range_source_usage, "kiro");
            let kiro_credentials = build_dashboard_source_summaries(
                "kiro",
                kiro_source_metadata(
                    &storage,
                    &dashboard_source_ids(&today_kiro_usage, &range_kiro_usage),
                )?,
                today_kiro_usage,
                range_kiro_usage,
            );
            let today_grok_usage = filter_source_usage(&today_source_usage, "grok");
            let range_grok_usage = filter_source_usage(&range_source_usage, "grok");
            let grok_credentials = build_dashboard_source_summaries(
                "grok",
                grok_source_metadata(
                    &storage,
                    &dashboard_source_ids(&today_grok_usage, &range_grok_usage),
                )?,
                today_grok_usage,
                range_grok_usage,
            );
            let today_aggregate_usage = filter_source_usage(&today_source_usage, "aggregate_api");
            let range_aggregate_usage = filter_source_usage(&range_source_usage, "aggregate_api");
            let aggregate_apis = build_dashboard_source_summaries(
                "aggregate_api",
                aggregate_source_metadata(
                    &storage,
                    &dashboard_source_ids(&today_aggregate_usage, &range_aggregate_usage),
                )?,
                today_aggregate_usage,
                range_aggregate_usage,
            );

            (
                users,
                openai_accounts,
                kiro_credentials,
                grok_credentials,
                aggregate_apis,
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };

    Ok(DashboardAdminUsageSummaryResult {
        range_start_ts: range_start,
        range_end_ts: range_end,
        today_start_ts: today_start,
        today_end_ts: today_end,
        today_usage: dashboard_usage(&today_usage),
        daily_usage,
        users,
        openai_accounts,
        kiro_credentials,
        grok_credentials,
        aggregate_apis,
    })
}

#[derive(Debug, Clone, Default)]
struct SourceMetadata {
    name: Option<String>,
    status: Option<String>,
    provider: Option<String>,
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

fn daily_usage_bucket(
    items: &[DailyTokenUsageRollup],
    today_start: i64,
    today_end: i64,
) -> Option<TokenUsageRollup> {
    items
        .iter()
        .find(|item| item.day_start_ts == today_start && item.day_end_ts == today_end)
        .map(|item| item.usage.clone())
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
    let mut user_ids = today_map.keys().cloned().collect::<HashSet<_>>();
    user_ids.extend(range_map.keys().cloned());
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }
    let user_id_list = user_ids.iter().cloned().collect::<Vec<_>>();
    let users = storage
        .list_dashboard_app_user_summaries_for_ids(&user_id_list)
        .map_err(|err| format!("list dashboard app users failed: {err}"))?;
    let user_map = users
        .into_iter()
        .map(|user| (user.id.clone(), user))
        .collect::<HashMap<_, _>>();

    let mut results = user_ids
        .into_iter()
        .map(|user_id| {
            let user = user_map.get(user_id.as_str());
            DashboardUserUsageSummary {
                user_id: user_id.clone(),
                username: user.map(|item| item.username.clone()),
                display_name: user.and_then(|item| item.display_name.clone()),
                role: user.map(|item| item.role.clone()),
                status: user.map(|item| item.status.clone()),
                wallet_available_credit_micros: user
                    .and_then(|item| item.wallet_available_credit_micros),
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
    results.truncate(ADMIN_TOP_USER_LIMIT);
    Ok(results)
}

fn account_source_metadata(
    storage: &codexmanager_core::storage::Storage,
    source_ids: &[String],
) -> Result<HashMap<String, SourceMetadata>, String> {
    if source_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(storage
        .list_account_dashboard_source_metadata_for_ids(source_ids)
        .map_err(|err| format!("list account dashboard metadata failed: {err}"))?
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

fn kiro_source_metadata(
    storage: &codexmanager_core::storage::Storage,
    source_ids: &[String],
) -> Result<HashMap<String, SourceMetadata>, String> {
    let wanted = source_ids.iter().collect::<HashSet<_>>();
    Ok(storage
        .list_kiro_credentials()
        .map_err(|err| format!("list Kiro dashboard metadata failed: {err}"))?
        .into_iter()
        .filter(|item| wanted.contains(&item.id))
        .map(|item| {
            (
                item.id,
                SourceMetadata {
                    name: item.email,
                    status: Some(item.status),
                    provider: Some("kiro".to_string()),
                },
            )
        })
        .collect())
}

fn grok_source_metadata(
    storage: &codexmanager_core::storage::Storage,
    source_ids: &[String],
) -> Result<HashMap<String, SourceMetadata>, String> {
    let wanted = source_ids.iter().collect::<HashSet<_>>();
    Ok(storage
        .list_grok_credentials()
        .map_err(|err| format!("list Grok dashboard metadata failed: {err}"))?
        .into_iter()
        .filter(|item| wanted.contains(&item.id))
        .map(|item| {
            (
                item.id,
                SourceMetadata {
                    name: Some(item.account_masked),
                    status: Some(item.status),
                    provider: Some("grok".to_string()),
                },
            )
        })
        .collect())
}

fn aggregate_source_metadata(
    storage: &codexmanager_core::storage::Storage,
    source_ids: &[String],
) -> Result<HashMap<String, SourceMetadata>, String> {
    if source_ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(storage
        .list_aggregate_api_dashboard_source_metadata_for_ids(source_ids)
        .map_err(|err| format!("list aggregate API dashboard metadata failed: {err}"))?
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

fn dashboard_source_ids(
    today_items: &[SourceTokenUsageRollup],
    range_items: &[SourceTokenUsageRollup],
) -> Vec<String> {
    let mut source_ids = today_items
        .iter()
        .chain(range_items.iter())
        .map(|item| item.source_id.trim())
        .filter(|source_id| !source_id.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    source_ids.sort();
    source_ids.dedup();
    source_ids
}

fn filter_source_usage(
    items: &[SourceTokenUsageRollup],
    source_kind: &str,
) -> Vec<SourceTokenUsageRollup> {
    items
        .iter()
        .filter(|item| item.source_kind == source_kind)
        .cloned()
        .collect()
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
    results.truncate(ADMIN_TOP_SOURCE_LIMIT);
    results
}

pub(crate) fn read_member_dashboard_summary(
    actor: &RpcActor,
    requested_user_id: Option<String>,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
    include_details: bool,
) -> Result<MemberDashboardSummaryResult, String> {
    crate::initialize_storage_if_needed()?;
    let target_user_id = resolve_target_user_id(actor, requested_user_id)?;
    let (day_start, day_end) = resolve_day_bounds(day_start_ts, day_end_ts);
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let distribution_enabled = crate::distribution_enabled_for_storage(&storage);

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

    let key_ids = storage
        .list_api_key_ids_for_user(&user_id)
        .map_err(|err| format!("list api key ids for user failed: {err}"))?;
    let api_keys = apikey_list::read_api_keys_for_ids_with_storage(&storage, &key_ids)?;
    let api_key_summary = build_api_key_summary(&api_keys);
    let wallet = read_member_wallet(&storage, &user_id)?;

    let usage_trend = read_usage_trend_7d(&storage, &user_id, day_start, day_end)?;
    let today_usage_rollup = usage_trend.today_usage;
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

    let (top_keys, top_models) =
        read_member_usage_breakdown(&storage, &api_keys, &key_ids, day_start, day_end)?;
    let available_model_count = if include_details {
        Some(read_available_model_count(&storage)?)
    } else {
        None
    };
    let recent_logs = if include_details {
        requestlog_list::read_request_log_page_for_key_ids_with_storage(
            &storage,
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
        .items
    } else {
        Vec::new()
    };
    let alerts = build_alerts(
        distribution_enabled,
        wallet.as_ref(),
        &api_key_summary,
        &usage_today,
        available_model_count,
    );

    Ok(MemberDashboardSummaryResult {
        user_id: Some(user_id),
        distribution_enabled,
        wallet,
        api_key_summary,
        usage_today,
        usage_trend_7d: usage_trend.points,
        top_keys,
        top_models,
        available_models: Vec::new(),
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
    time_bounds::resolve_optional_utc_day_bounds_ts(
        day_start_ts,
        day_end_ts,
        codexmanager_core::storage::now_ts(),
    )
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

fn read_member_wallet(
    storage: &codexmanager_core::storage::Storage,
    user_id: &str,
) -> Result<Option<MemberDashboardWalletResult>, String> {
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

fn read_available_model_count(
    storage: &codexmanager_core::storage::Storage,
) -> Result<usize, String> {
    storage
        .count_available_model_catalog_models("default")
        .map(|count| count.max(0) as usize)
        .map_err(|err| format!("count model catalog failed: {err}"))
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

#[derive(Debug, Clone, Default)]
struct MemberUsageTrend {
    today_usage: TokenUsageRollup,
    points: Vec<MemberDashboardUsagePoint>,
}

fn read_usage_trend_7d(
    storage: &codexmanager_core::storage::Storage,
    user_id: &str,
    day_start: i64,
    day_end: i64,
) -> Result<MemberUsageTrend, String> {
    let day_span = (day_end - day_start).max(1);
    let range_start = day_start.saturating_sub((TREND_DAYS - 1) * day_span);
    let items = storage
        .summarize_request_token_stats_daily_for_user(user_id, range_start, day_end, day_span)
        .map_err(|err| format!("summarize member token trend failed: {err}"))?;
    let mut by_start = items
        .iter()
        .cloned()
        .map(|item| (item.day_start_ts, item.usage))
        .collect::<BTreeMap<_, _>>();
    let today_usage = daily_usage_bucket(&items, day_start, day_end).unwrap_or_default();
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
    Ok(MemberUsageTrend {
        today_usage,
        points,
    })
}

fn read_member_usage_breakdown(
    storage: &codexmanager_core::storage::Storage,
    api_keys: &[ApiKeySummary],
    key_ids: &[String],
    day_start: i64,
    day_end: i64,
) -> Result<(Vec<MemberDashboardKeyUsage>, Vec<MemberDashboardModelUsage>), String> {
    if key_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let snapshot = storage
        .load_member_dashboard_usage_breakdown_snapshot(
            key_ids,
            day_start,
            day_end,
            TREND_DAYS,
            MEMBER_TOP_MODEL_LIMIT,
        )
        .map_err(|err| format!("load member usage breakdown failed: {err}"))?;

    let mut today_by_key: HashMap<String, (i64, f64)> = HashMap::new();
    for item in snapshot.today_key_model_usage.into_iter() {
        let entry = today_by_key.entry(item.key_id).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(item.total_tokens.max(0));
        entry.1 += item.estimated_cost_usd.max(0.0);
    }

    let total_by_key = snapshot
        .total_key_usage
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

    let top_models = snapshot
        .top_model_usage
        .into_iter()
        .map(|item| MemberDashboardModelUsage {
            model: item.model,
            total_tokens: item.total_tokens.max(0),
            estimated_cost_usd: item.estimated_cost_usd.max(0.0),
        })
        .collect::<Vec<_>>();

    Ok((top_keys, top_models))
}

fn build_alerts(
    distribution_enabled: bool,
    wallet: Option<&MemberDashboardWalletResult>,
    api_key_summary: &MemberDashboardApiKeySummary,
    usage_today: &MemberDashboardUsageToday,
    available_model_count: Option<usize>,
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

    if available_model_count == Some(0) {
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

#[cfg(test)]
#[path = "dashboard_tests.rs"]
mod tests;
