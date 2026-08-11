#[cfg(test)]
use codexmanager_core::storage::AccountTokenCandidate;
#[cfg(test)]
use codexmanager_core::storage::AccountUsageRefreshTarget;
use codexmanager_core::storage::{Storage, Token};
use crossbeam_channel::unbounded;
#[cfg(test)]
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use super::{
    notify_usage_refresh_completed, open_storage, record_usage_refresh_failure,
    record_usage_refresh_metrics, refresh_usage_for_token, DEFAULT_USAGE_POLL_BATCH_LIMIT,
    DEFAULT_USAGE_POLL_CYCLE_BUDGET_SECS, ENV_USAGE_POLL_BATCH_LIMIT,
    ENV_USAGE_POLL_CYCLE_BUDGET_SECS, USAGE_POLL_CURSOR, USAGE_REFRESH_WORKERS,
};

/// 函数 `refresh_usage_for_all_accounts`
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
pub(crate) fn refresh_usage_for_all_accounts() -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let tasks = load_refreshable_usage_refresh_tasks(&storage)?;
    if tasks.is_empty() {
        return Ok(());
    }
    let total = tasks.len();
    let processed = run_usage_refresh_tasks(tasks)?;
    notify_usage_refresh_completed("manual_all", processed, total);
    Ok(())
}

/// 函数 `refresh_usage_for_polling_batch`
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
pub(crate) fn refresh_usage_for_polling_batch() -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let tasks = load_refreshable_usage_refresh_tasks(&storage)?;
    if tasks.is_empty() {
        return Ok(());
    }

    let total = tasks.len();
    let start_cursor = USAGE_POLL_CURSOR.load(Ordering::Relaxed) % total;
    let batch_limit = usage_poll_batch_limit(total);
    let cycle_budget = usage_poll_cycle_budget();
    let cycle_started_at = Instant::now();
    let indices = usage_poll_batch_indices(total, start_cursor, batch_limit);
    let selected_tasks = indices
        .into_iter()
        .map(|index| tasks[index].clone())
        .collect::<Vec<_>>();
    let processed = run_usage_refresh_tasks(selected_tasks)?;

    if processed > 0 {
        USAGE_POLL_CURSOR.store(
            next_usage_poll_cursor(total, start_cursor, processed),
            Ordering::Relaxed,
        );
    }
    if cycle_budget.is_some_and(|budget| cycle_started_at.elapsed() >= budget) {
        log::info!(
            "usage polling batch exceeded budget: processed={} total={} workers={} elapsed_ms={} budget_secs={}",
            processed,
            total,
            usage_refresh_worker_count().min(processed.max(1)),
            cycle_started_at.elapsed().as_millis(),
            cycle_budget.map(|budget| budget.as_secs()).unwrap_or(0)
        );
    }
    if processed < total {
        log::info!(
            "usage polling batch truncated: processed={} total={} batch_limit={} workers={} budget_secs={}",
            processed,
            total,
            batch_limit,
            usage_refresh_worker_count().min(processed.max(1)),
            cycle_budget.map(|budget| budget.as_secs()).unwrap_or(0)
        );
    }
    notify_usage_refresh_completed("polling", processed, total);
    Ok(())
}

pub(crate) fn refresh_usage_and_aggregate_balances_for_polling_cycle() -> Result<(), String> {
    let usage_result = refresh_usage_for_polling_batch();
    let aggregate_result = refresh_aggregate_api_balances_for_polling_cycle();

    match (usage_result, aggregate_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), Ok(())) => Err(err),
        (Ok(()), Err(err)) => Err(err),
        (Err(usage_err), Err(aggregate_err)) => Err(format!(
            "{usage_err}; aggregate api balance polling failed: {aggregate_err}"
        )),
    }
}

fn refresh_aggregate_api_balances_for_polling_cycle() -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let api_ids = storage
        .list_active_balance_query_aggregate_api_ids()
        .map_err(|err| format!("list aggregate API balance query IDs failed: {err}"))?;
    drop(storage);

    if api_ids.is_empty() {
        return Ok(());
    }

    let total = api_ids.len();
    let mut success_count = 0usize;
    let mut failed_count = 0usize;

    for api_id in api_ids {
        match crate::refresh_aggregate_api_balance(api_id.as_str()) {
            Ok(result) if result.ok => {
                success_count = success_count.saturating_add(1);
            }
            Ok(result) => {
                failed_count = failed_count.saturating_add(1);
                log::warn!(
                    "aggregate api balance polling failed: api_id={} message={}",
                    result.id,
                    result
                        .message
                        .unwrap_or_else(|| "balance query returned unsuccessful result".to_string())
                );
            }
            Err(err) => {
                failed_count = failed_count.saturating_add(1);
                log::warn!(
                    "aggregate api balance polling errored: api_id={} err={}",
                    api_id,
                    err
                );
            }
        }
    }

    log::info!(
        "aggregate api balance polling completed: total={} success={} failed={}",
        total,
        success_count,
        failed_count
    );

    Ok(())
}

#[cfg(test)]
pub(crate) fn load_refreshable_accounts(
    storage: &Storage,
) -> Result<Vec<AccountUsageRefreshTarget>, String> {
    load_refreshable_accounts_impl(storage)
}

#[cfg(test)]
fn load_refreshable_accounts_impl(
    storage: &Storage,
) -> Result<Vec<AccountUsageRefreshTarget>, String> {
    storage
        .list_account_usage_refresh_targets_by_statuses(&refreshable_account_statuses())
        .map_err(|err| format!("list refreshable accounts failed: {err}"))
}

fn load_refreshable_usage_refresh_tasks(
    storage: &Storage,
) -> Result<Vec<UsageRefreshBatchTask>, String> {
    storage
        .list_account_usage_refresh_token_targets_by_statuses(&refreshable_account_statuses())
        .map_err(|err| format!("list refreshable token accounts failed: {err}"))
        .map(|targets| {
            targets
                .into_iter()
                .map(|target| UsageRefreshBatchTask {
                    account_id: target.account_id,
                    token: target.token,
                    workspace_id: target.workspace_id,
                })
                .collect()
        })
}

fn refreshable_account_statuses() -> Vec<String> {
    ["active", "inactive", "limited", "unavailable", "unknown"]
        .into_iter()
        .map(String::from)
        .collect()
}

#[derive(Clone)]
struct UsageRefreshBatchTask {
    account_id: String,
    token: Token,
    workspace_id: Option<String>,
}

#[cfg(test)]
#[derive(Clone)]
struct UsageRefreshTaskPlan {
    account_id: String,
    workspace_id: Option<String>,
}

/// 函数 `build_usage_refresh_tasks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - tokens: 参数 tokens
/// - accounts: 参数 accounts
/// - banned_ids: 参数 banned_ids
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn build_usage_refresh_tasks(
    tokens: Vec<Token>,
    accounts: &[AccountUsageRefreshTarget],
    banned_ids: &HashSet<String>,
) -> Vec<UsageRefreshBatchTask> {
    let plans = build_usage_refresh_task_plans(
        tokens
            .iter()
            .map(|token| AccountTokenCandidate {
                account_id: token.account_id.clone(),
                has_access_token: !token.access_token.trim().is_empty(),
                has_refresh_token: !token.refresh_token.trim().is_empty(),
                last_refresh: token.last_refresh,
            })
            .collect(),
        accounts,
        banned_ids,
    );
    hydrate_usage_refresh_tasks(plans, tokens)
}

#[cfg(test)]
fn build_usage_refresh_task_plans(
    tokens: Vec<AccountTokenCandidate>,
    accounts: &[AccountUsageRefreshTarget],
    banned_ids: &HashSet<String>,
) -> Vec<UsageRefreshTaskPlan> {
    let account_ids = accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<HashSet<_>>();
    let mut skipped_ids = accounts
        .iter()
        .filter(|account| is_account_refresh_skipped(account))
        .map(|account| account.id.clone())
        .collect::<HashSet<_>>();
    skipped_ids.extend(banned_ids.iter().cloned());
    let workspace_map = build_workspace_map_from_refresh_targets(accounts);

    tokens
        .into_iter()
        .filter(|token| account_ids.contains(&token.account_id))
        .filter(|token| token.has_access_token && token.has_refresh_token)
        .filter(|token| !skipped_ids.contains(&token.account_id))
        .map(|token| {
            let account_id = token.account_id.clone();
            UsageRefreshTaskPlan {
                workspace_id: workspace_map.get(&account_id).cloned().unwrap_or(None),
                account_id,
            }
        })
        .collect()
}

#[cfg(test)]
fn build_usage_refresh_task_plans_from_targets(
    accounts: &[AccountUsageRefreshTarget],
    banned_ids: &HashSet<String>,
) -> Vec<UsageRefreshTaskPlan> {
    accounts
        .iter()
        .filter(|account| !is_account_refresh_skipped(account))
        .filter(|account| !banned_ids.contains(&account.id))
        .map(|account| UsageRefreshTaskPlan {
            account_id: account.id.clone(),
            workspace_id: account.workspace_id.clone(),
        })
        .collect()
}

#[cfg(test)]
fn hydrate_usage_refresh_tasks(
    plans: Vec<UsageRefreshTaskPlan>,
    tokens: Vec<Token>,
) -> Vec<UsageRefreshBatchTask> {
    let token_map = tokens
        .into_iter()
        .map(|token| (token.account_id.clone(), token))
        .collect::<std::collections::HashMap<_, _>>();
    plans
        .into_iter()
        .filter_map(|plan| {
            let token = token_map.get(plan.account_id.as_str())?.clone();
            Some(UsageRefreshBatchTask {
                account_id: plan.account_id,
                token,
                workspace_id: plan.workspace_id,
            })
        })
        .collect()
}

/// 函数 `run_usage_refresh_tasks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - tasks: 参数 tasks
///
/// # 返回
/// 返回函数执行结果
fn run_usage_refresh_tasks(tasks: Vec<UsageRefreshBatchTask>) -> Result<usize, String> {
    let total = tasks.len();
    if total == 0 {
        return Ok(0);
    }

    let worker_count = usage_refresh_worker_count().min(total);
    if worker_count <= 1 {
        let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
        let mut succeeded = 0usize;
        let mut first_error: Option<String> = None;
        for task in tasks {
            match run_usage_refresh_task(&storage, task) {
                Ok(()) => succeeded = succeeded.saturating_add(1),
                Err(err) => {
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }
        if succeeded == 0 {
            return Err(format_all_usage_refresh_failed(total, first_error));
        }
        return Ok(total);
    }

    let (sender, receiver) = unbounded::<UsageRefreshBatchTask>();
    for task in tasks {
        sender
            .send(task)
            .map_err(|_| "enqueue usage refresh task failed".to_string())?;
    }
    drop(sender);

    let succeeded = AtomicUsize::new(0);
    let first_error = Mutex::new(None::<String>);
    thread::scope(|scope| -> Result<(), String> {
        let mut handles = Vec::with_capacity(worker_count);
        for worker_index in 0..worker_count {
            let receiver = receiver.clone();
            let succeeded = &succeeded;
            let first_error = &first_error;
            handles.push(scope.spawn(move || {
                let storage = open_storage().ok_or_else(|| {
                    format!("usage refresh worker {worker_index} storage unavailable")
                })?;
                while let Ok(task) = receiver.recv() {
                    match run_usage_refresh_task(&storage, task) {
                        Ok(()) => {
                            succeeded.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(err) => {
                            if let Ok(mut guard) = first_error.lock() {
                                if guard.is_none() {
                                    *guard = Some(err);
                                }
                            }
                        }
                    }
                }
                Ok::<(), String>(())
            }));
        }

        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(err)) => return Err(err),
                Err(_) => return Err("usage refresh worker panicked".to_string()),
            }
        }
        Ok(())
    })?;

    if succeeded.load(Ordering::Relaxed) == 0 {
        let first_error = first_error.lock().ok().and_then(|guard| guard.clone());
        return Err(format_all_usage_refresh_failed(total, first_error));
    }
    Ok(total)
}

fn format_all_usage_refresh_failed(total: usize, first_error: Option<String>) -> String {
    match first_error {
        Some(error) if !error.trim().is_empty() => {
            format!("usage refresh failed for all {total} account(s): {error}")
        }
        _ => format!("usage refresh failed for all {total} account(s)"),
    }
}

/// 函数 `run_usage_refresh_task`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - task: 参数 task
///
/// # 返回
/// 无
fn run_usage_refresh_task(storage: &Storage, task: UsageRefreshBatchTask) -> Result<(), String> {
    let started_at = Instant::now();
    // Background polling only needs the quota endpoint. accounts/check is a
    // browser-facing metadata endpoint and polling it for every account at
    // once is both unnecessary and much more likely to trigger Cloudflare.
    match refresh_usage_for_token(
        storage,
        &task.token,
        task.workspace_id.as_deref(),
        None,
        false,
    ) {
        Ok(result) => {
            super::record_codex_probe_outcome(storage, &task.account_id, result.status);
            record_usage_refresh_metrics(true, started_at);
            Ok(())
        }
        Err(err) => {
            record_usage_refresh_metrics(false, started_at);
            record_usage_refresh_failure(storage, &task.account_id, &err);
            Err(err)
        }
    }
}

/// 函数 `usage_refresh_worker_count`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn usage_refresh_worker_count() -> usize {
    USAGE_REFRESH_WORKERS.load(Ordering::Relaxed).max(1)
}

/// 函数 `is_account_refresh_skipped`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account: 参数 account
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn is_account_refresh_skipped(account: &AccountUsageRefreshTarget) -> bool {
    let normalized = account.status.trim().to_ascii_lowercase();
    normalized == "disabled" || normalized == "banned"
}

#[cfg(test)]
fn build_workspace_map_from_refresh_targets(
    accounts: &[AccountUsageRefreshTarget],
) -> HashMap<String, Option<String>> {
    accounts
        .iter()
        .map(|account| (account.id.clone(), account.workspace_id.clone()))
        .collect()
}

/// 函数 `usage_poll_batch_limit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - total: 参数 total
///
/// # 返回
/// 返回函数执行结果
fn usage_poll_batch_limit(total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    let configured = std::env::var(ENV_USAGE_POLL_BATCH_LIMIT)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_USAGE_POLL_BATCH_LIMIT);
    if configured == 0 {
        total
    } else {
        configured.max(1).min(total)
    }
}

/// 函数 `usage_poll_cycle_budget`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn usage_poll_cycle_budget() -> Option<Duration> {
    let configured = std::env::var(ENV_USAGE_POLL_CYCLE_BUDGET_SECS)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_USAGE_POLL_CYCLE_BUDGET_SECS);
    if configured == 0 {
        None
    } else {
        Some(Duration::from_secs(configured.max(1)))
    }
}

/// 函数 `usage_poll_batch_indices`
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
#[cfg(test)]
pub(crate) fn usage_poll_batch_indices(
    total: usize,
    cursor: usize,
    batch_limit: usize,
) -> Vec<usize> {
    if total == 0 || batch_limit == 0 {
        return Vec::new();
    }
    let start = cursor % total;
    (0..batch_limit.min(total))
        .map(|offset| (start + offset) % total)
        .collect()
}

/// 函数 `next_usage_poll_cursor`
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
#[cfg(test)]
pub(crate) fn next_usage_poll_cursor(total: usize, cursor: usize, processed: usize) -> usize {
    if total == 0 {
        return 0;
    }
    (cursor % total + processed.min(total)) % total
}

/// 函数 `usage_poll_batch_indices`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - total: 参数 total
/// - cursor: 参数 cursor
/// - batch_limit: 参数 batch_limit
///
/// # 返回
/// 返回函数执行结果
#[cfg(not(test))]
fn usage_poll_batch_indices(total: usize, cursor: usize, batch_limit: usize) -> Vec<usize> {
    if total == 0 || batch_limit == 0 {
        return Vec::new();
    }
    let start = cursor % total;
    (0..batch_limit.min(total))
        .map(|offset| (start + offset) % total)
        .collect()
}

/// 函数 `next_usage_poll_cursor`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - total: 参数 total
/// - cursor: 参数 cursor
/// - processed: 参数 processed
///
/// # 返回
/// 返回函数执行结果
#[cfg(not(test))]
fn next_usage_poll_cursor(total: usize, cursor: usize, processed: usize) -> usize {
    if total == 0 {
        return 0;
    }
    (cursor % total + processed.min(total)) % total
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
