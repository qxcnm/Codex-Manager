use codexmanager_core::auth::{extract_token_exp, DEFAULT_CLIENT_ID, DEFAULT_ISSUER};
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use codexmanager_core::usage::parse_usage_snapshot;
use crossbeam_channel::{bounded, unbounded, Receiver, Sender, TrySendError};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::account_status::mark_account_unavailable_for_auth_error;
use crate::storage_helpers::open_storage;
use crate::usage_account_meta::{
    build_workspace_map_from_accounts, clean_header_value, derive_account_meta, patch_account_meta,
    patch_account_meta_cached, workspace_header_for_account,
};
use crate::usage_http::{
    fetch_account_subscription, fetch_account_subscription_with_explicit_proxy,
    fetch_usage_snapshot, fetch_usage_snapshot_with_explicit_proxy, log_account_data_route,
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
use crate::usage_token_refresh::{refresh_and_persist_access_token, token_refresh_ahead_secs};

mod batch;
mod errors;
mod queue;
mod runner;
mod settings;

static USAGE_POLLING_STARTED: OnceLock<()> = OnceLock::new();
static GATEWAY_KEEPALIVE_STARTED: OnceLock<()> = OnceLock::new();
static TOKEN_REFRESH_POLLING_STARTED: OnceLock<()> = OnceLock::new();
static WARMUP_CRON_STARTED: OnceLock<()> = OnceLock::new();
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

type UsageRefreshCompletedHandler = Arc<dyn Fn(UsageRefreshCompletedEvent) + Send + Sync>;

static USAGE_REFRESH_COMPLETED_HANDLER: OnceLock<Mutex<Option<UsageRefreshCompletedHandler>>> =
    OnceLock::new();
static USAGE_REFRESH_COMPLETED_SUBSCRIBERS: OnceLock<
    Mutex<Vec<Sender<UsageRefreshCompletedEvent>>>,
> = OnceLock::new();

use self::batch::refresh_usage_and_aggregate_balances_for_polling_cycle;
pub(crate) use self::batch::refresh_usage_for_all_accounts;
#[cfg(test)]
use self::batch::{next_usage_poll_cursor, usage_poll_batch_indices};
use self::errors::{
    mark_usage_unreachable_if_needed, record_usage_refresh_failure, should_retry_with_refresh,
};
#[cfg(test)]
use self::queue::clear_pending_usage_refresh_tasks_for_tests;
pub(crate) use self::queue::enqueue_usage_refresh_with_worker;
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

pub(crate) fn subscribe_usage_refresh_completed() -> Receiver<UsageRefreshCompletedEvent> {
    let (sender, receiver) = bounded(32);
    let subscribers = USAGE_REFRESH_COMPLETED_SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard =
        crate::lock_utils::lock_recover(subscribers, "usage_refresh_completed_subscribers");
    guard.push(sender);
    receiver
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
    USAGE_POLLING_STARTED.get_or_init(|| {
        spawn_background_loop("usage-polling", usage_polling_loop);
    });
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
    GATEWAY_KEEPALIVE_STARTED.get_or_init(|| {
        spawn_background_loop("gateway-keepalive", gateway_keepalive_loop);
    });
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
    TOKEN_REFRESH_POLLING_STARTED.get_or_init(|| {
        spawn_background_loop("token-refresh-polling", token_refresh_polling_loop);
    });
}

pub(crate) fn ensure_warmup_cron() {
    ensure_background_tasks_config_loaded();
    WARMUP_CRON_STARTED.get_or_init(|| {
        spawn_background_loop("account-warmup-cron", warmup_cron_loop);
    });
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
fn spawn_background_loop(name: &str, worker: fn()) {
    let thread_name = name.to_string();
    let _ = thread::Builder::new()
        .name(thread_name.clone())
        .spawn(move || loop {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(worker));
            if result.is_ok() {
                break;
            }
            log::error!(
                "background task panicked and will restart: task={}",
                thread_name
            );
            thread::sleep(Duration::from_secs(1));
        });
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
    enqueue_usage_refresh_with_worker(account_id, |id| {
        if let Err(err) = refresh_usage_for_account(&id) {
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
pub(crate) fn refresh_tokens_before_expiry_for_all_accounts() -> Result<(), String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
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
    let accounts = storage.list_accounts().map_err(|e| e.to_string())?;
    let account_map = accounts
        .iter()
        .map(|account| (account.id.clone(), account.clone()))
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
                account_map.get(&token.account_id),
                &default_issuer,
            ),
            client_id: client_id.clone(),
            token: token.clone(),
        });
    }

    refreshed = refreshed.saturating_add(run_token_refresh_tasks(due_tokens)?);
    let _ = (refreshed, skipped);
    Ok(())
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
pub(crate) fn refresh_usage_for_account(account_id: &str) -> Result<(), String> {
    // 刷新单个账号用量
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let token = match storage
        .find_token_by_account_id(account_id)
        .map_err(|e| e.to_string())?
    {
        Some(token) => token,
        None => return Ok(()),
    };

    let account = storage
        .find_account_by_id(account_id)
        .map_err(|e| e.to_string())?;
    let workspace_id = account.as_ref().and_then(workspace_header_for_account);
    let mut account_map = account
        .map(|value| {
            let mut map = HashMap::new();
            map.insert(value.id.clone(), value);
            map
        })
        .unwrap_or_default();

    let started_at = Instant::now();
    let account_cache = if account_map.is_empty() {
        None
    } else {
        Some(&mut account_map)
    };
    match refresh_usage_for_token(&storage, &token, workspace_id.as_deref(), account_cache) {
        Ok(_) => {}
        Err(err) => {
            record_usage_refresh_metrics(false, started_at);
            record_usage_refresh_failure(&storage, &token.account_id, &err);
            return Err(err);
        }
    }
    record_usage_refresh_metrics(true, started_at);
    notify_usage_refresh_completed("single", 1, 1);
    Ok(())
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
fn refresh_usage_for_token(
    storage: &Storage,
    token: &Token,
    workspace_id: Option<&str>,
    account_cache: Option<&mut HashMap<String, Account>>,
) -> Result<UsageRefreshResult, String> {
    // 读取用量接口所需的基础配置
    let issuer =
        std::env::var("CODEXMANAGER_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_string());
    let client_id =
        std::env::var("CODEXMANAGER_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
    let base_url = std::env::var("CODEXMANAGER_USAGE_BASE_URL")
        .unwrap_or_else(|_| "https://chatgpt.com".to_string());

    let mut current = token.clone();
    let mut resolved_workspace_id = workspace_id.map(|v| v.to_string());
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
    let bearer = current.access_token.clone();

    match refresh_account_snapshot(
        storage,
        &current.account_id,
        &base_url,
        &bearer,
        resolved_workspace_id.as_deref(),
        resolved_subscription_account_id.as_deref(),
    ) {
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
            if let Err(refresh_err) = refresh_and_persist_access_token(
                storage,
                &mut current,
                &issuer,
                &client_id,
                token_refresh_ahead_secs(),
            ) {
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
            ) {
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

fn refresh_account_snapshot(
    storage: &Storage,
    account_id: &str,
    base_url: &str,
    bearer: &str,
    workspace_id: Option<&str>,
    subscription_account_id: Option<&str>,
) -> Result<UsageAvailabilityStatus, String> {
    let proxy_mode = crate::account_proxy::resolve_account_proxy_mode(account_id);
    if let Some(subscription_account_id) = subscription_account_id {
        log_account_data_route(
            "subscription",
            account_id,
            &proxy_mode,
            "accounts_check",
            false,
        );
        let subscription = match &proxy_mode {
            crate::account_proxy::AccountProxyMode::Disabled => {
                fetch_account_subscription(base_url, bearer, subscription_account_id, workspace_id)?
            }
            crate::account_proxy::AccountProxyMode::Explicit { proxy_url } => {
                fetch_account_subscription_with_explicit_proxy(
                    base_url,
                    bearer,
                    subscription_account_id,
                    workspace_id,
                    proxy_url,
                )?
            }
            crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
                return Err(error.clone());
            }
        };
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

    log_account_data_route("usage", account_id, &proxy_mode, "usage", true);
    let value = match &proxy_mode {
        crate::account_proxy::AccountProxyMode::Disabled => {
            fetch_usage_snapshot(base_url, bearer, workspace_id)?
        }
        crate::account_proxy::AccountProxyMode::Explicit { proxy_url } => {
            fetch_usage_snapshot_with_explicit_proxy(base_url, bearer, workspace_id, proxy_url)?
        }
        crate::account_proxy::AccountProxyMode::Invalid { error, .. } => {
            return Err(error.clone());
        }
    };
    let status = classify_usage_status_from_snapshot_value(&value);
    store_usage_snapshot(storage, account_id, value)?;
    Ok(status)
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
fn classify_usage_status_from_snapshot_value(value: &serde_json::Value) -> UsageAvailabilityStatus {
    let parsed = parse_usage_snapshot(value);

    let primary_present = parsed.used_percent.is_some() && parsed.window_minutes.is_some();
    if !primary_present {
        return UsageAvailabilityStatus::Unknown;
    }

    if parsed.used_percent.map(|v| v >= 100.0).unwrap_or(false) {
        return UsageAvailabilityStatus::Unavailable;
    }

    let secondary_used = parsed.secondary_used_percent;
    let secondary_window = parsed.secondary_window_minutes;
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
        return UsageAvailabilityStatus::Unavailable;
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
fn run_token_refresh_tasks(tasks: Vec<TokenRefreshTask>) -> Result<usize, String> {
    let total = tasks.len();
    if total == 0 {
        return Ok(0);
    }

    let worker_count = token_refresh_worker_count(total);
    if worker_count <= 1 {
        let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
        let mut refreshed = 0usize;
        for task in tasks {
            let mut token = task.token;
            if run_token_refresh_task(&storage, &mut token, &task.issuer, &task.client_id) {
                refreshed = refreshed.saturating_add(1);
            }
        }
        return Ok(refreshed);
    }

    let (sender, receiver) = unbounded::<TokenRefreshTask>();
    for task in tasks {
        sender
            .send(task)
            .map_err(|_| "enqueue token refresh task failed".to_string())?;
    }
    drop(sender);

    let refreshed = std::sync::atomic::AtomicUsize::new(0);
    thread::scope(|scope| -> Result<(), String> {
        let mut handles = Vec::with_capacity(worker_count);
        for worker_index in 0..worker_count {
            let receiver = receiver.clone();
            let refreshed = &refreshed;
            handles.push(scope.spawn(move || {
                let storage = open_storage().ok_or_else(|| {
                    format!("token refresh worker {worker_index} storage unavailable")
                })?;
                while let Ok(task) = receiver.recv() {
                    let mut token = task.token;
                    if run_token_refresh_task(&storage, &mut token, &task.issuer, &task.client_id) {
                        refreshed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                Ok::<(), String>(())
            }));
        }

        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(err)) => return Err(err),
                Err(_) => return Err("token refresh worker panicked".to_string()),
            }
        }
        Ok(())
    })?;

    Ok(refreshed.load(std::sync::atomic::Ordering::Relaxed))
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
fn run_token_refresh_task(
    storage: &Storage,
    token: &mut Token,
    issuer: &str,
    client_id: &str,
) -> bool {
    if token.refresh_token.trim().is_empty() {
        log::debug!(
            "skip token refresh polling for account without refresh token: account_id={}",
            token.account_id
        );
        return false;
    }
    match refresh_and_persist_access_token(
        storage,
        token,
        issuer,
        client_id,
        token_refresh_ahead_secs(),
    ) {
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

fn resolve_token_refresh_issuer(account: Option<&Account>, default_issuer: &str) -> String {
    account
        .map(|account| account.issuer.trim())
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
