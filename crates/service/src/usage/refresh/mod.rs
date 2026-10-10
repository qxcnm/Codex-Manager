use codexmanager_core::auth::{extract_token_exp, DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::storage::{
    now_ts, Account, AccountTokenRefreshIssuer, Storage, Token, UsageSnapshotRecord,
};
use codexmanager_core::usage::has_usable_luna_reserve;
#[cfg(test)]
use codexmanager_core::usage::parse_usage_snapshot;
#[cfg(test)]
use crossbeam_channel::{bounded, Receiver};
use crossbeam_channel::{Sender, TrySendError};
use futures_util::{FutureExt, StreamExt};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::account_status::mark_account_unavailable_for_auth_error;
use crate::storage_helpers::open_storage;
use crate::usage_account_meta::{
    clean_header_value, derive_account_meta, patch_account_meta, patch_account_meta_cached,
    resolve_workspace_id_for_account,
};
use crate::usage_http::{
    fetch_account_subscription_async, fetch_usage_snapshot_async, log_account_data_route,
};
use crate::usage_keepalive::{is_keepalive_error_ignorable, run_gateway_keepalive_once};
use crate::usage_scheduler::{
    parse_interval_secs, DEFAULT_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS,
    DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS, DEFAULT_GATEWAY_KEEPALIVE_JITTER_SECS,
    DEFAULT_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS, DEFAULT_USAGE_POLL_INTERVAL_SECS,
    DEFAULT_USAGE_POLL_JITTER_SECS, MIN_GATEWAY_KEEPALIVE_INTERVAL_SECS,
    MIN_USAGE_POLL_INTERVAL_SECS,
};
use crate::usage_snapshot_store::store_usage_snapshot;
use crate::usage_token_refresh::{
    refresh_and_persist_access_token_async, token_refresh_ahead_secs,
};

mod background;
mod batch;
pub(crate) use background::drain_usage_background_tasks;
mod errors;
mod queue;
mod reset_warmup;
mod runner;
mod settings;

static USAGE_POLLING_STARTED: AtomicBool = AtomicBool::new(false);
static GATEWAY_KEEPALIVE_STARTED: AtomicBool = AtomicBool::new(false);
static TOKEN_REFRESH_POLLING_STARTED: AtomicBool = AtomicBool::new(false);
static WARMUP_CRON_STARTED: AtomicBool = AtomicBool::new(false);
static RESET_WARMUP_STARTED: AtomicBool = AtomicBool::new(false);
static WARMUP_CRON_SIGNAL: OnceLock<(Mutex<u64>, Condvar)> = OnceLock::new();
static BACKGROUND_TASKS_CONFIG_LOADED: OnceLock<()> = OnceLock::new();
static USAGE_POLL_CURSOR: AtomicUsize = AtomicUsize::new(0);
static USAGE_POLLING_ENABLED: AtomicBool = AtomicBool::new(true);
static USAGE_POLL_INTERVAL_SECS: AtomicU64 = AtomicU64::new(DEFAULT_USAGE_POLL_INTERVAL_SECS);
static GATEWAY_KEEPALIVE_ENABLED: AtomicBool = AtomicBool::new(true);
static GATEWAY_KEEPALIVE_INTERVAL_SECS: AtomicU64 =
    AtomicU64::new(DEFAULT_GATEWAY_KEEPALIVE_INTERVAL_SECS);
static TOKEN_REFRESH_POLLING_ENABLED: AtomicBool = AtomicBool::new(true);
static TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC: AtomicU64 =
    AtomicU64::new(DEFAULT_TOKEN_REFRESH_POLL_INTERVAL_SECS);
static USAGE_REFRESH_WORKERS: AtomicUsize = AtomicUsize::new(DEFAULT_USAGE_REFRESH_WORKERS);
static HTTP_WORKER_FACTOR: AtomicUsize = AtomicUsize::new(DEFAULT_HTTP_WORKER_FACTOR);
static HTTP_WORKER_MIN: AtomicUsize = AtomicUsize::new(DEFAULT_HTTP_WORKER_MIN);
static HTTP_STREAM_WORKER_FACTOR: AtomicUsize = AtomicUsize::new(DEFAULT_HTTP_STREAM_WORKER_FACTOR);
static HTTP_STREAM_WORKER_MIN: AtomicUsize = AtomicUsize::new(DEFAULT_HTTP_STREAM_WORKER_MIN);
static WARMUP_CRON_ENABLED: AtomicBool = AtomicBool::new(false);
static WARMUP_CRON_EXPRESSION: OnceLock<Mutex<String>> = OnceLock::new();

const ENV_DISABLE_POLLING: &str = "CODEXMANAGER_DISABLE_POLLING";
const ENV_USAGE_POLLING_ENABLED: &str = "CODEXMANAGER_USAGE_POLLING_ENABLED";
const ENV_USAGE_POLL_INTERVAL_SECS: &str = "CODEXMANAGER_USAGE_POLL_INTERVAL_SECS";
const ENV_USAGE_POLL_BATCH_LIMIT: &str = "CODEXMANAGER_USAGE_POLL_BATCH_LIMIT";
const ENV_USAGE_POLL_CYCLE_BUDGET_SECS: &str = "CODEXMANAGER_USAGE_POLL_CYCLE_BUDGET_SECS";
const ENV_AUTO_REFRESH_AFTER_ACCOUNT_ADD: &str =
    "CODEXMANAGER_AUTO_USAGE_REFRESH_AFTER_ACCOUNT_ADD";
const ENV_GATEWAY_KEEPALIVE_ENABLED: &str = "CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED";
const ENV_GATEWAY_KEEPALIVE_INTERVAL_SECS: &str = "CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS";
const ENV_TOKEN_REFRESH_POLLING_ENABLED: &str = "CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED";
const ENV_TOKEN_REFRESH_POLL_INTERVAL_SECS: &str = "CODEXMANAGER_TOKEN_REFRESH_POLL_INTERVAL_SECS";
const ENV_WARMUP_CRON_ENABLED: &str = "CODEXMANAGER_WARMUP_CRON_ENABLED";
const ENV_WARMUP_CRON_EXPRESSION: &str = "CODEXMANAGER_WARMUP_CRON_EXPRESSION";
const ENV_TOKEN_REFRESH_BATCH_LIMIT: &str = "CODEXMANAGER_TOKEN_REFRESH_BATCH_LIMIT";
const COMMON_POLL_JITTER_ENV: &str = "CODEXMANAGER_POLL_JITTER_SECS";
const COMMON_POLL_FAILURE_BACKOFF_MAX_ENV: &str = "CODEXMANAGER_POLL_FAILURE_BACKOFF_MAX_SECS";
const USAGE_POLL_JITTER_ENV: &str = "CODEXMANAGER_USAGE_POLL_JITTER_SECS";
const USAGE_POLL_FAILURE_BACKOFF_MAX_ENV: &str = "CODEXMANAGER_USAGE_POLL_FAILURE_BACKOFF_MAX_SECS";
const USAGE_REFRESH_WORKERS_ENV: &str = "CODEXMANAGER_USAGE_REFRESH_WORKERS";
const DEFAULT_USAGE_POLL_BATCH_LIMIT: usize = 100;
const DEFAULT_USAGE_POLL_CYCLE_BUDGET_SECS: u64 = 30;
const DEFAULT_USAGE_REFRESH_WORKERS: usize = 4;
const DEFAULT_HTTP_WORKER_FACTOR: usize = 4;
const DEFAULT_HTTP_WORKER_MIN: usize = 8;
const DEFAULT_HTTP_STREAM_WORKER_FACTOR: usize = 1;
const DEFAULT_HTTP_STREAM_WORKER_MIN: usize = 2;
const ENV_HTTP_WORKER_FACTOR: &str = "CODEXMANAGER_HTTP_WORKER_FACTOR";
const ENV_HTTP_WORKER_MIN: &str = "CODEXMANAGER_HTTP_WORKER_MIN";
const ENV_HTTP_STREAM_WORKER_FACTOR: &str = "CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR";
const ENV_HTTP_STREAM_WORKER_MIN: &str = "CODEXMANAGER_HTTP_STREAM_WORKER_MIN";
const GATEWAY_KEEPALIVE_JITTER_ENV: &str = "CODEXMANAGER_GATEWAY_KEEPALIVE_JITTER_SECS";
const GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_ENV: &str =
    "CODEXMANAGER_GATEWAY_KEEPALIVE_FAILURE_BACKOFF_MAX_SECS";
const DEFAULT_TOKEN_REFRESH_POLL_INTERVAL_SECS: u64 = 60;
const MIN_TOKEN_REFRESH_POLL_INTERVAL_SECS: u64 = 10;
const TOKEN_REFRESH_FAILURE_BACKOFF_MAX_SECS: u64 = 300;
const TOKEN_REFRESH_LOOKAHEAD_BUFFER_SECS: u64 = 60;
const TOKEN_REFRESH_FALLBACK_AGE_SECS: i64 = 2700;
const DEFAULT_TOKEN_REFRESH_BATCH_LIMIT: usize = 2048;
const BACKGROUND_TASK_RESTART_REQUIRED_KEYS: [&str; 5] = [
    "usageRefreshWorkers",
    "httpWorkerFactor",
    "httpWorkerMin",
    "httpStreamWorkerFactor",
    "httpStreamWorkerMin",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageAvailabilityStatus {
    Available,
    AvailableLunaReserve,
    PrimaryWindowAvailableOnly,
    Unavailable,
    Unknown,
}

impl UsageAvailabilityStatus {
    /// 函数 `as_code`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn as_code(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::AvailableLunaReserve => "available_luna_reserve",
            Self::PrimaryWindowAvailableOnly => "primary_window_available_only",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct UsageRefreshResult {
    _status: UsageAvailabilityStatus,
}

#[derive(Debug, Clone)]
pub struct UsageRefreshCompletedEvent {
    pub source: &'static str,
    pub processed: usize,
    pub total: usize,
    pub completed_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageRefreshRunResult {
    pub ok: bool,
    pub source: &'static str,
    pub account_id: Option<String>,
    pub processed: usize,
    pub total: usize,
    pub message: Option<String>,
}

type UsageRefreshCompletedHandler = Arc<dyn Fn(UsageRefreshCompletedEvent) + Send + Sync>;

static USAGE_REFRESH_COMPLETED_HANDLER: OnceLock<Mutex<Option<UsageRefreshCompletedHandler>>> =
    OnceLock::new();
static USAGE_REFRESH_COMPLETED_SUBSCRIBERS: OnceLock<
    Mutex<Vec<Sender<UsageRefreshCompletedEvent>>>,
> = OnceLock::new();
static USAGE_REFRESH_COMPLETED_ASYNC: OnceLock<
    tokio::sync::broadcast::Sender<UsageRefreshCompletedEvent>,
> = OnceLock::new();

use self::batch::refresh_usage_and_aggregate_balances_for_polling_cycle;
#[cfg(test)]
use self::batch::{next_usage_poll_cursor, usage_poll_batch_indices};
pub(crate) use self::batch::{
    refresh_usage_for_all_accounts_result, refresh_usage_for_all_accounts_result_async,
};
use self::errors::{
    mark_usage_unreachable_if_needed, record_usage_refresh_failure, should_retry_with_refresh,
};
#[cfg(test)]
pub(crate) use self::queue::enqueue_usage_refresh_with_worker;
#[cfg(test)]
use self::queue::reset_usage_refresh_executor_for_tests;
use self::runner::{
    gateway_keepalive_loop, token_refresh_polling_loop, usage_polling_loop, warmup_cron_loop,
};
use self::settings::ensure_background_tasks_config_loaded;
pub(crate) use self::settings::{
    background_tasks_settings, reload_background_tasks_runtime_from_env,
    set_background_tasks_settings, validate_background_tasks_settings_patch,
    BackgroundTasksSettingsPatch,
};

pub fn set_usage_refresh_completed_handler<F>(handler: F)
where
    F: Fn(UsageRefreshCompletedEvent) + Send + Sync + 'static,
{
    let slot = USAGE_REFRESH_COMPLETED_HANDLER.get_or_init(|| Mutex::new(None));
    let mut guard = crate::lock_utils::lock_recover(slot, "usage_refresh_completed_handler");
    *guard = Some(Arc::new(handler));
}

#[cfg(test)]
pub(crate) fn subscribe_usage_refresh_completed() -> Receiver<UsageRefreshCompletedEvent> {
    let (sender, receiver) = bounded(32);
    let subscribers = USAGE_REFRESH_COMPLETED_SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard =
        crate::lock_utils::lock_recover(subscribers, "usage_refresh_completed_subscribers");
    guard.push(sender);
    receiver
}

pub(crate) fn subscribe_usage_refresh_completed_async(
) -> tokio::sync::broadcast::Receiver<UsageRefreshCompletedEvent> {
    USAGE_REFRESH_COMPLETED_ASYNC
        .get_or_init(|| tokio::sync::broadcast::channel(32).0)
        .subscribe()
}

#[cfg(test)]
pub(crate) fn usage_refresh_async_subscriber_count() -> usize {
    USAGE_REFRESH_COMPLETED_ASYNC
        .get()
        .map_or(0, tokio::sync::broadcast::Sender::receiver_count)
}

pub(crate) fn notify_usage_refresh_completed(source: &'static str, processed: usize, total: usize) {
    let event = UsageRefreshCompletedEvent {
        source,
        processed,
        total,
        completed_at: now_ts(),
    };
    let handler = USAGE_REFRESH_COMPLETED_HANDLER.get().and_then(|slot| {
        let guard = crate::lock_utils::lock_recover(slot, "usage_refresh_completed_handler");
        guard.clone()
    });

    if let Some(handler) = handler {
        handler(event.clone());
    }

    if let Some(subscribers) = USAGE_REFRESH_COMPLETED_SUBSCRIBERS.get() {
        let mut guard =
            crate::lock_utils::lock_recover(subscribers, "usage_refresh_completed_subscribers");
        guard.retain(|sender| match sender.try_send(event.clone()) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        });
    }
    if let Some(sender) = USAGE_REFRESH_COMPLETED_ASYNC.get() {
        let _ = sender.send(event);
    }
}

/// 函数 `ensure_usage_polling`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn ensure_usage_polling() {
    ensure_background_tasks_config_loaded();
    start_background_loop("usage-polling", &USAGE_POLLING_STARTED, usage_polling_loop);
}

/// 函数 `ensure_gateway_keepalive`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn ensure_gateway_keepalive() {
    ensure_background_tasks_config_loaded();
    start_background_loop(
        "gateway-keepalive",
        &GATEWAY_KEEPALIVE_STARTED,
        gateway_keepalive_loop,
    );
}

/// 函数 `ensure_token_refresh_polling`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn ensure_token_refresh_polling() {
    ensure_background_tasks_config_loaded();
    start_background_loop(
        "token-refresh-polling",
        &TOKEN_REFRESH_POLLING_STARTED,
        token_refresh_polling_loop,
    );
}

pub(crate) fn ensure_warmup_cron() {
    ensure_background_tasks_config_loaded();
    start_background_loop(
        "account-warmup-cron",
        &WARMUP_CRON_STARTED,
        warmup_cron_loop,
    );
}

pub(crate) fn ensure_reset_warmup() {
    start_background_loop(
        "account-reset-warmup",
        &RESET_WARMUP_STARTED,
        reset_warmup::reset_warmup_loop,
    );
}

/// 函数 `spawn_background_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - worker: 参数 worker
///
/// # 返回
/// 无
fn start_background_loop<F, Fut>(name: &'static str, started: &'static AtomicBool, worker: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    use std::sync::atomic::Ordering;
    if started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    struct StartedGuard(&'static AtomicBool);
    impl Drop for StartedGuard {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Release);
        }
    }
    let started_guard = StartedGuard(started);
    if let Err(error) = background::spawn(async move {
        let _started = started_guard;
        loop {
            if std::panic::AssertUnwindSafe(worker())
                .catch_unwind()
                .await
                .is_ok()
            {
                break;
            }
            log::error!("background task panicked and will restart: task={name}");
            if crate::shutdown_requested() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }) {
        log::error!("background task unavailable: task={name} error={error}");
    }
}

fn run_usage_future<F, T>(future: F) -> Result<T, String>
where
    F: std::future::Future<Output = Result<T, String>> + Send,
    T: Send,
{
    crate::aggregate_api::run_aggregate_future(future)
}

/// 函数 `enqueue_usage_refresh_for_account`
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
pub(crate) fn enqueue_usage_refresh_for_account(account_id: &str) -> bool {
    queue::enqueue_usage_refresh_async(account_id, |id| async move {
        if let Err(err) = refresh_usage_for_account_async(&id).await {
            let status = classify_usage_status_from_error(&err);
            log::warn!(
                "async usage refresh failed: account_id={} status={} err={}",
                id,
                status.as_code(),
                err
            );
        }
    })
}

pub(crate) fn enqueue_usage_refresh_after_account_add(account_id: &str) -> bool {
    if !auto_refresh_after_account_add_enabled() {
        return false;
    }
    let queued = enqueue_usage_refresh_for_account(account_id);
    if queued {
        log::info!("queued usage refresh after account add: account_id={account_id}");
    }
    queued
}

fn auto_refresh_after_account_add_enabled() -> bool {
    env_bool_or(ENV_AUTO_REFRESH_AFTER_ACCOUNT_ADD, true)
}

fn env_bool_or(name: &str, default: bool) -> bool {
    let Some(raw) = std::env::var(name).ok() else {
        return default;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

/// 函数 `reset_usage_poll_cursor_for_tests`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[cfg(test)]
fn reset_usage_poll_cursor_for_tests() {
    USAGE_POLL_CURSOR.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// 函数 `refresh_tokens_before_expiry_for_all_accounts`
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
pub(crate) async fn refresh_tokens_before_expiry_for_all_accounts() -> Result<(), String> {
    let storage = open_storage()
        .map(|storage| storage.shared_handle())
        .ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let now = now_ts();
    let due_cutoff = token_refresh_due_cutoff(
        now,
        TOKEN_REFRESH_POLL_INTERVAL_SECS_ATOMIC.load(std::sync::atomic::Ordering::Relaxed),
    );
    let refresh_ahead_secs = token_refresh_ahead_secs();
    let access_exp_cutoff = token_refresh_access_exp_cutoff(due_cutoff, refresh_ahead_secs);
    let mut tokens = storage
        .list_tokens_due_for_refresh(due_cutoff, access_exp_cutoff, token_refresh_batch_limit())
        .map_err(|e| e.to_string())?;
    if tokens.is_empty() {
        return Ok(());
    }
    let issuers = load_token_refresh_issuers_for_tokens(&storage, &tokens)?;
    let issuer_map = issuers
        .iter()
        .map(|issuer| (issuer.id.clone(), issuer.issuer.clone()))
        .collect::<HashMap<_, _>>();

    let default_issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let mut refreshed = 0usize;
    let mut skipped = 0usize;

    let mut due_tokens = Vec::with_capacity(tokens.len());
    for token in tokens.iter_mut() {
        let _ = storage.touch_token_refresh_attempt(&token.account_id, now);
        let (exp_opt, scheduled_at) = token_refresh_schedule(
            token,
            now,
            refresh_ahead_secs,
            TOKEN_REFRESH_FALLBACK_AGE_SECS,
        );
        let _ =
            storage.update_token_refresh_schedule(&token.account_id, exp_opt, Some(scheduled_at));
        if scheduled_at > due_cutoff {
            skipped = skipped.saturating_add(1);
            continue;
        }
        due_tokens.push(TokenRefreshTask {
            issuer: resolve_token_refresh_issuer(
                issuer_map.get(&token.account_id).map(String::as_str),
                &default_issuer,
            ),
            client_id: client_id.clone(),
            token: token.clone(),
        });
    }

    refreshed = refreshed.saturating_add(run_token_refresh_tasks(due_tokens).await?);
    let _ = (refreshed, skipped);
    Ok(())
}

fn load_token_refresh_issuers_for_tokens(
    storage: &Storage,
    tokens: &[Token],
) -> Result<Vec<AccountTokenRefreshIssuer>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let account_ids = tokens
        .iter()
        .map(|token| token.account_id.clone())
        .collect::<Vec<_>>();
    storage
        .list_account_token_refresh_issuers_for_ids(&account_ids)
        .map_err(|e| e.to_string())
}

/// 函数 `refresh_usage_for_account`
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
pub(crate) async fn refresh_usage_for_account_result_async(
    account_id: &str,
) -> Result<UsageRefreshRunResult, String> {
    // 刷新单个账号用量
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("account_id is required".to_string());
    }
    let storage = open_storage()
        .map(|storage| storage.shared_handle())
        .ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let token = match storage
        .find_token_by_account_id(account_id)
        .map_err(|e| e.to_string())?
    {
        Some(token) => token,
        None => {
            return Ok(UsageRefreshRunResult {
                ok: false,
                source: "single",
                account_id: Some(account_id.to_string()),
                processed: 0,
                total: 0,
                message: Some("account token not found".to_string()),
            });
        }
    };

    let workspace_id = resolve_workspace_id_for_account(&storage, account_id);

    let started_at = Instant::now();
    match refresh_usage_for_token(&storage, &token, workspace_id.as_deref(), None).await {
        Ok(_) => {}
        Err(err) => {
            record_usage_refresh_metrics(false, started_at);
            record_usage_refresh_failure(&storage, &token.account_id, &err);
            return Err(err);
        }
    }
    record_usage_refresh_metrics(true, started_at);
    notify_usage_refresh_completed("single", 1, 1);
    Ok(UsageRefreshRunResult {
        ok: true,
        source: "single",
        account_id: Some(account_id.to_string()),
        processed: 1,
        total: 1,
        message: None,
    })
}

pub(crate) fn refresh_usage_for_account_result(
    account_id: &str,
) -> Result<UsageRefreshRunResult, String> {
    run_usage_future(refresh_usage_for_account_result_async(account_id))
}

pub(crate) fn refresh_usage_for_account(account_id: &str) -> Result<(), String> {
    run_usage_future(refresh_usage_for_account_async(account_id))
}

pub(crate) async fn refresh_usage_for_account_async(account_id: &str) -> Result<(), String> {
    let result = refresh_usage_for_account_result_async(account_id).await?;
    if result.ok || result.total == 0 {
        return Ok(());
    }
    Err(result
        .message
        .unwrap_or_else(|| "usage refresh did not run".to_string()))
}

/// 函数 `record_usage_refresh_metrics`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - success: 参数 success
/// - started_at: 参数 started_at
///
/// # 返回
/// 无
fn record_usage_refresh_metrics(success: bool, started_at: Instant) {
    crate::gateway::record_usage_refresh_outcome(
        success,
        crate::gateway::duration_to_millis(started_at.elapsed()),
    );
}

/// 函数 `refresh_usage_for_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - token: 参数 token
/// - workspace_id: 参数 workspace_id
/// - account_cache: 参数 account_cache
///
/// # 返回
/// 返回函数执行结果
async fn refresh_usage_for_token(
    storage: &Storage,
    token: &Token,
    workspace_id: Option<&str>,
    account_cache: Option<&mut HashMap<String, Account>>,
) -> Result<UsageRefreshResult, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    // 读取用量接口所需的基础配置
    let issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let base_url = std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string());

    let mut current = token.clone();
    let account = storage
        .find_account_by_id(&current.account_id)
        .map_err(|err| format!("load account for usage refresh failed: {err}"))?
        .ok_or_else(|| "account disappeared before usage refresh".to_string())?;
    let mut resolved_workspace_id = workspace_id.map(|v| v.to_string());
    let agent_identity = storage
        .find_account_agent_identity(&current.account_id)
        .map_err(|err| format!("load agent identity failed: {err}"))?;
    if resolved_workspace_id.is_none() {
        resolved_workspace_id = agent_identity
            .as_ref()
            .and_then(|identity| identity.workspace_id.clone());
    }
    let (derived_chatgpt_id, derived_workspace_id) = derive_account_meta(&current);

    if resolved_workspace_id.is_none() {
        resolved_workspace_id = derived_workspace_id
            .clone()
            .or_else(|| derived_chatgpt_id.clone());
    }

    if let Some(accounts) = account_cache {
        patch_account_meta_cached(
            storage,
            accounts,
            &current.account_id,
            derived_chatgpt_id.clone(),
            derived_workspace_id.clone(),
        );
    } else {
        patch_account_meta(
            storage,
            &current.account_id,
            derived_chatgpt_id.clone(),
            derived_workspace_id.clone(),
        );
    }

    let resolved_workspace_id = clean_header_value(resolved_workspace_id);
    let resolved_subscription_account_id =
        clean_header_value(derived_chatgpt_id.or_else(|| resolved_workspace_id.clone()));

    if agent_identity.is_some() {
        let registration_client =
            crate::gateway::async_upstream_client_for_account(&current.account_id)
                .map_err(|err| format!("build agent task registration client failed: {err}"))?;
        let authorization =
            crate::agent_identity::resolve_or_bootstrap_account_agent_identity_authorization_async(
                storage,
                &registration_client,
                &account,
                &current,
            )
            .await?
            .ok_or_else(|| "agent identity disappeared before usage refresh".to_string())?;
        let failed_task_id = authorization.task_id.clone();
        let authorization_scope_id = authorization
            .account_scope_id
            .as_deref()
            .or(resolved_workspace_id.as_deref());
        let first = refresh_account_snapshot(
            storage,
            &current.account_id,
            &base_url,
            &authorization.value,
            authorization_scope_id,
            None,
            authorization.is_fedramp,
        )
        .await;
        let outcome = match first {
            Err(err) if crate::agent_identity::is_agent_identity_task_invalid_error(&err) => {
                let recovered =
                    crate::agent_identity::recover_account_agent_identity_authorization_async(
                        storage,
                        &registration_client,
                        &account,
                        &current,
                        &failed_task_id,
                    )
                    .await?
                    .ok_or_else(|| {
                        "agent identity disappeared during usage task recovery".to_string()
                    })?;
                refresh_account_snapshot(
                    storage,
                    &current.account_id,
                    &base_url,
                    &recovered.value,
                    recovered
                        .account_scope_id
                        .as_deref()
                        .or(resolved_workspace_id.as_deref()),
                    None,
                    recovered.is_fedramp,
                )
                .await
            }
            other => other,
        };
        return match outcome {
            Ok(status) => Ok(UsageRefreshResult { _status: status }),
            Err(err) => {
                mark_usage_unreachable_if_needed(storage, &current.account_id, &err);
                Err(err)
            }
        };
    }

    let bearer = current.access_token.clone();

    match refresh_account_snapshot(
        storage,
        &current.account_id,
        &base_url,
        &bearer,
        resolved_workspace_id.as_deref(),
        resolved_subscription_account_id.as_deref(),
        false,
    )
    .await
    {
        Ok(status) => Ok(UsageRefreshResult { _status: status }),
        Err(err) if should_retry_usage_refresh_with_token(&current, &err) => {
            if current.refresh_token.trim().is_empty() {
                log::debug!(
                    "skip usage refresh token retry for account without refresh token: account_id={}",
                    current.account_id
                );
                mark_usage_unreachable_if_needed(storage, &current.account_id, &err);
                return Err(err);
            }
            // 中文注释：token 刷新与持久化独立封装，避免轮询流程继续膨胀；
            // 不下沉会让后续 async 迁移时刷新链路与业务编排强耦合，回归范围扩大。
            if let Err(refresh_err) = refresh_and_persist_access_token_async(
                storage,
                &mut current,
                &issuer,
                &client_id,
                token_refresh_ahead_secs(),
            )
            .await
            {
                mark_usage_unreachable_if_needed(storage, &current.account_id, &refresh_err);
                return Err(refresh_err);
            }
            let (refreshed_chatgpt_id, refreshed_workspace_id) = derive_account_meta(&current);
            patch_account_meta(
                storage,
                &current.account_id,
                refreshed_chatgpt_id.clone(),
                refreshed_workspace_id.clone(),
            );
            let refreshed_workspace_id =
                clean_header_value(refreshed_workspace_id.or_else(|| refreshed_chatgpt_id.clone()));
            let refreshed_subscription_account_id =
                clean_header_value(refreshed_chatgpt_id.or_else(|| refreshed_workspace_id.clone()));
            let bearer = current.access_token.clone();
            match refresh_account_snapshot(
                storage,
                &current.account_id,
                &base_url,
                &bearer,
                refreshed_workspace_id.as_deref(),
                refreshed_subscription_account_id.as_deref(),
                false,
            )
            .await
            {
                Ok(status) => Ok(UsageRefreshResult { _status: status }),
                Err(err) => {
                    mark_usage_unreachable_if_needed(storage, &current.account_id, &err);
                    Err(err)
                }
            }
        }
        Err(err) => {
            mark_usage_unreachable_if_needed(storage, &current.account_id, &err);
            Err(err)
        }
    }
}

async fn refresh_account_snapshot(
    storage: &Storage,
    account_id: &str,
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
    subscription_account_id: Option<&str>,
    is_fedramp: bool,
) -> Result<UsageAvailabilityStatus, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let proxy_mode = crate::account_proxy::resolve_account_proxy_mode(account_id);
    let explicit_proxy = match &proxy_mode {
        crate::account_proxy::AccountProxyMode::Disabled => None,
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url, .. } => {
            Some(proxy_url.as_str())
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => return Err(error.clone()),
    };
    if let Some(subscription_account_id) = subscription_account_id {
        log_account_data_route(
            "subscription",
            account_id,
            &proxy_mode,
            "accounts_check",
            false,
        );
        let subscription = fetch_account_subscription_async(
            base_url,
            bearer,
            subscription_account_id,
            workspace_id,
            explicit_proxy,
        )
        .await?;
        storage
            .upsert_account_subscription(
                account_id,
                subscription.has_subscription,
                subscription.account_plan_type.as_deref(),
                subscription.plan_type.as_deref(),
                subscription.expires_at,
                subscription.renews_at,
            )
            .map_err(|err| format!("store account subscription failed: {err}"))?;
    }
    let usage_account_id = resolve_usage_account_id(subscription_account_id, workspace_id);
    log_account_data_route("usage", account_id, &proxy_mode, "usage", true);
    let value = fetch_usage_snapshot_async(
        base_url,
        bearer,
        usage_account_id,
        is_fedramp,
        explicit_proxy,
    )
    .await?;
    let stored = store_usage_snapshot(storage, account_id, value)?;
    Ok(classify_usage_status_from_snapshot_record(&stored))
}

fn resolve_usage_account_id<'a>(
    chatgpt_account_id: Option<&'a str>,
    workspace_id: Option<&'a str>,
) -> Option<&'a str> {
    chatgpt_account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            workspace_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
}

#[cfg(test)]
#[path = "../../../tests/usage/usage_refresh_status_tests.rs"]
mod status_tests;

#[cfg(test)]
#[path = "../tests/usage_refresh_tests.rs"]
mod tests;

/// 函数 `classify_usage_status_from_snapshot_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
#[cfg(test)]
fn classify_usage_status_from_snapshot_value(value: &serde_json::Value) -> UsageAvailabilityStatus {
    let parsed = parse_usage_snapshot(value);

    classify_usage_status(
        parsed.used_percent,
        parsed.window_minutes,
        parsed.secondary_used_percent,
        parsed.secondary_window_minutes,
        parsed.credits_json.as_deref(),
    )
}

fn classify_usage_status_from_snapshot_record(
    snapshot: &UsageSnapshotRecord,
) -> UsageAvailabilityStatus {
    classify_usage_status(
        snapshot.used_percent,
        snapshot.window_minutes,
        snapshot.secondary_used_percent,
        snapshot.secondary_window_minutes,
        snapshot.credits_json.as_deref(),
    )
}

fn classify_usage_status(
    used_percent: Option<f64>,
    window_minutes: Option<i64>,
    secondary_used_percent: Option<f64>,
    secondary_window_minutes: Option<i64>,
    credits_json: Option<&str>,
) -> UsageAvailabilityStatus {
    let primary_present = used_percent.is_some() && window_minutes.is_some();
    if !primary_present {
        return UsageAvailabilityStatus::Unknown;
    }

    if used_percent.map(|v| v >= 100.0).unwrap_or(false) {
        return if has_usable_luna_reserve(credits_json) {
            UsageAvailabilityStatus::AvailableLunaReserve
        } else {
            UsageAvailabilityStatus::Unavailable
        };
    }

    let secondary_used = secondary_used_percent;
    let secondary_window = secondary_window_minutes;
    let secondary_present = secondary_used.is_some() || secondary_window.is_some();
    let secondary_complete = secondary_used.is_some() && secondary_window.is_some();

    if !secondary_present {
        return UsageAvailabilityStatus::PrimaryWindowAvailableOnly;
    }
    if !secondary_complete {
        // 中文注释：secondary 半缺失时不再视为未知或不可用，
        // 只要主窗口有额度，就先保留可继续尝试的状态。
        return UsageAvailabilityStatus::PrimaryWindowAvailableOnly;
    }
    if secondary_used.map(|v| v >= 100.0).unwrap_or(false) {
        return if has_usable_luna_reserve(credits_json) {
            UsageAvailabilityStatus::AvailableLunaReserve
        } else {
            UsageAvailabilityStatus::Unavailable
        };
    }
    UsageAvailabilityStatus::Available
}

/// 函数 `classify_usage_status_from_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
fn classify_usage_status_from_error(err: &str) -> UsageAvailabilityStatus {
    if err.starts_with("usage endpoint status ")
        || err.starts_with("usage endpoint failed: status=")
        || err.starts_with("subscription endpoint status ")
        || err.starts_with("subscription endpoint failed: status=")
    {
        return UsageAvailabilityStatus::Unavailable;
    }
    UsageAvailabilityStatus::Unknown
}

/// 函数 `token_refresh_batch_limit`
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
fn token_refresh_batch_limit() -> usize {
    std::env::var(ENV_TOKEN_REFRESH_BATCH_LIMIT)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_TOKEN_REFRESH_BATCH_LIMIT)
        .max(1)
}

/// 函数 `token_refresh_worker_count`
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
fn token_refresh_worker_count(total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    USAGE_REFRESH_WORKERS
        .load(std::sync::atomic::Ordering::Relaxed)
        .max(1)
        .min(total)
}

/// 函数 `run_token_refresh_tasks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - tokens: 参数 tokens
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
///
/// # 返回
/// 返回函数执行结果
#[derive(Clone)]
struct TokenRefreshTask {
    token: Token,
    issuer: String,
    client_id: String,
}

/// 函数 `run_token_refresh_tasks`
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
async fn run_token_refresh_tasks(tasks: Vec<TokenRefreshTask>) -> Result<usize, String> {
    let total = tasks.len();
    if total == 0 {
        return Ok(0);
    }
    let mut work = futures_util::stream::iter(tasks.into_iter().map(|task| async move {
        let storage = open_storage()
            .map(|storage| storage.shared_handle())
            .ok_or_else(|| "token refresh storage unavailable".to_string())?;
        let mut token = task.token;
        Ok::<bool, String>(
            run_token_refresh_task_async(&storage, &mut token, &task.issuer, &task.client_id).await,
        )
    }))
    .buffer_unordered(token_refresh_worker_count(total));
    let mut refreshed = 0;
    while let Some(result) = work.next().await {
        if result? {
            refreshed += 1;
        }
    }
    Ok(refreshed)
}

/// 函数 `run_token_refresh_task`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - token: 参数 token
/// - issuer: 参数 issuer
/// - client_id: 参数 client_id
///
/// # 返回
/// 返回函数执行结果
async fn run_token_refresh_task_async(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
) -> bool {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    if token.refresh_token.trim().is_empty() {
        log::debug!(
            "skip token refresh polling for account without refresh token: account_id={}",
            token.account_id
        );
        return false;
    }
    match refresh_and_persist_access_token_async(
        storage,
        token,
        issuer,
        client_id,
        token_refresh_ahead_secs(),
    )
    .await
    {
        Ok(_) => true,
        Err(err) => {
            let _ = mark_account_unavailable_for_auth_error(storage, &token.account_id, &err);
            log::warn!(
                "token refresh polling failed: account_id={} err={}",
                token.account_id,
                err
            );
            false
        }
    }
}

#[cfg(test)]
fn run_token_refresh_task(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
) -> bool {
    crate::runtime::service_runtime::run_sync(run_token_refresh_task_async(
        storage, token, issuer, client_id,
    ))
    .expect("test token refresh compatibility capacity")
}

fn resolve_token_refresh_issuer(account_issuer: Option<&str>, default_issuer: &str) -> String {
    account_issuer
        .map(str::trim)
        .filter(|issuer| !issuer.is_empty())
        .unwrap_or(default_issuer)
        .to_string()
}

/// 函数 `token_refresh_schedule`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - token: 参数 token
/// - now_ts_secs: 参数 now_ts_secs
/// - ahead_secs: 参数 ahead_secs
/// - fallback_age_secs: 参数 fallback_age_secs
///
/// # 返回
/// 返回函数执行结果
fn token_refresh_schedule(
    token: &Token,
    now_ts_secs: i64,
    ahead_secs: i64,
    fallback_age_secs: i64,
) -> (Option<i64>, i64) {
    if token.refresh_token.trim().is_empty() {
        return (None, i64::MAX);
    }
    let access_exp = extract_token_exp(&token.access_token);
    let refresh_exp = extract_token_exp(&token.refresh_token);
    let access_refresh_at = access_exp.map(|exp| exp.saturating_sub(ahead_secs));
    let refresh_refresh_at = refresh_exp.map(|exp| exp.saturating_sub(ahead_secs));
    let scheduled_at = match (access_refresh_at, refresh_refresh_at) {
        (Some(access_at), Some(refresh_at)) => Some(access_at.min(refresh_at)),
        (Some(access_at), None) => Some(access_at),
        (None, Some(refresh_at)) => Some(refresh_at),
        (None, None) => None,
    };
    if let Some(scheduled_at) = scheduled_at {
        return (access_exp, scheduled_at);
    }
    (
        access_exp,
        token
            .last_refresh
            .saturating_add(fallback_age_secs)
            .max(now_ts_secs),
    )
}

fn token_refresh_due_cutoff(now_ts_secs: i64, poll_interval_secs: u64) -> i64 {
    let lookahead_secs = poll_interval_secs.saturating_add(TOKEN_REFRESH_LOOKAHEAD_BUFFER_SECS);
    now_ts_secs.saturating_add(i64::try_from(lookahead_secs).unwrap_or(i64::MAX))
}

fn token_refresh_access_exp_cutoff(refresh_due_cutoff_ts: i64, ahead_secs: i64) -> i64 {
    refresh_due_cutoff_ts.saturating_add(ahead_secs)
}

/// 函数 `should_retry_usage_refresh_with_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// - token: 参数 token
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
pub(super) fn should_retry_usage_refresh_with_token(token: &Token, err: &str) -> bool {
    should_retry_with_refresh(err) && !token.refresh_token.trim().is_empty()
}
