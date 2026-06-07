use codexmanager_core::rpc::types::{StartupSnapshotResult, UsageAggregateSummaryResult};

use crate::{
    account_list, apikey_list, apikey_models, gateway, requestlog_list, requestlog_today_summary,
    storage_helpers, usage_aggregate, RpcActor,
};

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
) -> Result<StartupSnapshotResult, String> {
    let storage =
        storage_helpers::open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let accounts = storage
        .list_accounts()
        .map_err(|err| format!("list accounts failed: {err}"))?;
    let db_path = std::env::var("CODEXMANAGER_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    log::info!(
        "startup/snapshot read: db_path={} account_count={}",
        db_path,
        accounts.len()
    );
    let account_context = account_list::build_account_summary_context(&storage, &accounts)?;
    let usage_aggregate_summary = usage_aggregate::compute_usage_aggregate_summary(
        &accounts,
        &account_context.usage_snapshots,
    );
    let usage_snapshots = account_context
        .usage_snapshots
        .into_iter()
        .map(crate::usage_read::usage_snapshot_result_from_record)
        .collect();
    let api_keys = apikey_list::read_api_keys()?;
    let api_models = apikey_models::read_model_options(false)?;
    let manual_preferred_account_id = gateway::manual_preferred_account();
    let request_log_today_summary =
        requestlog_today_summary::read_requestlog_today_summary(day_start_ts, day_end_ts)?;
    let request_logs = requestlog_list::read_request_logs(None, request_log_limit)?;

    Ok(StartupSnapshotResult {
        accounts: account_context.items,
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
) -> Result<StartupSnapshotResult, String> {
    if actor.is_admin() {
        return read_startup_snapshot(request_log_limit, day_start_ts, day_end_ts);
    }
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: startup requires user session".to_string())?;
    let key_ids = crate::list_api_key_ids_for_user(user_id)?;
    let api_keys = apikey_list::read_api_keys_for_ids(&key_ids)?;
    let api_models = apikey_models::read_model_options(false)?;
    let request_log_today_summary =
        requestlog_today_summary::read_requestlog_today_summary_for_key_ids(
            day_start_ts,
            day_end_ts,
            &key_ids,
        )?;
    let request_logs =
        requestlog_list::read_request_logs_for_key_ids(None, request_log_limit, &key_ids)?;

    Ok(StartupSnapshotResult {
        accounts: Vec::new(),
        usage_snapshots: Vec::new(),
        usage_aggregate_summary: UsageAggregateSummaryResult::default(),
        api_keys,
        api_models,
        manual_preferred_account_id: None,
        request_log_today_summary,
        request_logs,
    })
}
