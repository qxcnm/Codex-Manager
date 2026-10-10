use codexmanager_core::{
    rpc::types::{
        QuotaOpenAiAccountOverviewResult, StartupSnapshotResult, UsageAggregateSummaryResult,
    },
    storage::{AccountQuotaOverviewStats, AccountSummaryStorageSnapshotOptions},
};

use crate::{
    account_list, apikey_list, gateway, requestlog_list, requestlog_today_summary, storage_helpers,
    usage_aggregate, RpcActor,
};

const STARTUP_REQUEST_LOG_DEFAULT_LIMIT: i64 = 24;
const STARTUP_REQUEST_LOG_MAX_LIMIT: i64 = 24;

fn normalize_startup_request_log_limit(limit: Option<i64>) -> i64 {
    match limit {
        Some(value) if value <= 0 => 0,
        Some(value) => value.min(STARTUP_REQUEST_LOG_MAX_LIMIT),
        None => STARTUP_REQUEST_LOG_DEFAULT_LIMIT,
    }
}

/// 函数 `read_startup_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn read_startup_snapshot(
    request_log_limit: Option<i64>,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
    include_api_models: bool,
    include_api_keys: bool,
    include_accounts: bool,
    include_usage_snapshots: bool,
    include_account_runtime: bool,
    include_account_details: bool,
) -> Result<StartupSnapshotResult, String> {
    let request_log_limit = normalize_startup_request_log_limit(request_log_limit);
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let db_path = std::env::var("CODEXMANAGER_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    let account_summary = storage
        .account_quota_overview_stats()
        .map(startup_account_summary)
        .map_err(|err| format!("read startup account summary failed: {err}"))?;
    let should_read_account_context = include_accounts || include_usage_snapshots;
    let (accounts, usage_snapshots, usage_aggregate_summary, account_count_for_log) =
        if should_read_account_context {
            let accounts = storage
                .list_account_summary_rows()
                .map_err(|err| format!("list accounts failed: {err}"))?;
            let account_ids = accounts
                .iter()
                .map(|account| account.id.clone())
                .collect::<Vec<_>>();
            let account_count_for_log = accounts.len();
            let account_context =
                account_list::build_account_summary_context_from_rows_with_options(
                    &storage,
                    accounts,
                    if include_account_runtime {
                        AccountSummaryStorageSnapshotOptions {
                            include_details: include_account_details,
                            ..AccountSummaryStorageSnapshotOptions::default()
                        }
                    } else {
                        AccountSummaryStorageSnapshotOptions::dashboard_light()
                    },
                )?;
            let usage_aggregate_summary =
                usage_aggregate::compute_usage_aggregate_summary_for_account_ids_list(
                    &account_ids,
                    &account_context.usage_snapshots,
                );
            let accounts = if include_accounts {
                account_context.items
            } else {
                Vec::new()
            };
            let usage_snapshots = if include_usage_snapshots {
                account_context
                    .usage_snapshots
                    .into_iter()
                    .map(crate::usage_read::usage_snapshot_result_from_record)
                    .collect()
            } else {
                Vec::new()
            };
            (
                accounts,
                usage_snapshots,
                usage_aggregate_summary,
                account_count_for_log,
            )
        } else {
            (
                Vec::new(),
                Vec::new(),
                usage_aggregate::read_usage_aggregate_summary_with_storage(&storage)?,
                account_summary.account_count.max(0) as usize,
            )
        };
    log::info!(
        "startup/snapshot read: db_path={} account_count={} include_accounts={} include_usage_snapshots={}",
        db_path,
        account_count_for_log,
        include_accounts,
        include_usage_snapshots
    );
    let api_keys = if include_api_keys {
        apikey_list::read_api_keys_with_storage(&storage)?
    } else {
        Vec::new()
    };
    let api_models = if include_api_models {
        crate::models_v2::models_response_with_storage(&storage)?
    } else {
        Default::default()
    };
    let manual_preferred_account_id = gateway::manual_preferred_account();
    let request_log_today_summary =
        requestlog_today_summary::read_requestlog_today_summary_with_storage(
            &storage,
            day_start_ts,
            day_end_ts,
        )?;
    let request_logs =
        requestlog_list::read_request_logs_with_storage(&storage, None, Some(request_log_limit))?;

    Ok(StartupSnapshotResult {
        accounts,
        account_summary,
        usage_snapshots,
        usage_aggregate_summary,
        api_keys,
        api_models,
        manual_preferred_account_id,
        request_log_today_summary,
        request_logs,
    })
}

pub(crate) fn read_startup_snapshot_for_actor(
    actor: &RpcActor,
    request_log_limit: Option<i64>,
    day_start_ts: Option<i64>,
    day_end_ts: Option<i64>,
    include_api_models: bool,
    include_api_keys: bool,
    include_accounts: bool,
    include_usage_snapshots: bool,
    include_account_runtime: bool,
    include_account_details: bool,
) -> Result<StartupSnapshotResult, String> {
    if actor.is_admin() {
        return read_startup_snapshot(
            request_log_limit,
            day_start_ts,
            day_end_ts,
            include_api_models,
            include_api_keys,
            include_accounts,
            include_usage_snapshots,
            include_account_runtime,
            include_account_details,
        );
    }
    let request_log_limit = normalize_startup_request_log_limit(request_log_limit);
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: startup requires user session".to_string())?;
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let key_ids = storage
        .list_api_key_ids_for_user(user_id)
        .map_err(|err| format!("list api key ids for user failed: {err}"))?;
    let api_keys = if include_api_keys {
        apikey_list::read_api_keys_for_ids_with_storage(&storage, &key_ids)?
    } else {
        Vec::new()
    };
    let api_models = if include_api_models {
        crate::models_v2::models_response_with_storage(&storage)?
    } else {
        Default::default()
    };
    let request_log_today_summary =
        requestlog_today_summary::read_requestlog_today_summary_for_key_ids_with_storage(
            &storage,
            day_start_ts,
            day_end_ts,
            &key_ids,
        )?;
    let request_logs = requestlog_list::read_request_logs_for_key_ids_with_storage(
        &storage,
        None,
        Some(request_log_limit),
        &key_ids,
    )?;

    Ok(StartupSnapshotResult {
        accounts: Vec::new(),
        account_summary: QuotaOpenAiAccountOverviewResult::default(),
        usage_snapshots: Vec::new(),
        usage_aggregate_summary: UsageAggregateSummaryResult::default(),
        api_keys,
        api_models,
        manual_preferred_account_id: None,
        request_log_today_summary,
        request_logs,
    })
}

fn startup_account_summary(stats: AccountQuotaOverviewStats) -> QuotaOpenAiAccountOverviewResult {
    QuotaOpenAiAccountOverviewResult {
        account_count: stats.account_count,
        available_count: stats.available_count,
        low_quota_count: stats.low_quota_count,
        primary_remain_percent: stats
            .primary_remain_percent_avg
            .map(|value| value.round() as i64),
        secondary_remain_percent: stats
            .secondary_remain_percent_avg
            .map(|value| value.round() as i64),
        last_refreshed_at: stats.last_refreshed_at,
    }
}

#[cfg(test)]
#[path = "startup_snapshot_tests.rs"]
mod tests;
