use codexmanager_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use crate::usage_account_meta::{derive_account_meta, patch_account_meta_in_place};

static CANDIDATE_SNAPSHOT_CACHE: OnceLock<Mutex<Option<CandidateSnapshotCache>>> = OnceLock::new();
static SELECTION_CONFIG_LOADED: OnceLock<()> = OnceLock::new();
static CANDIDATE_CACHE_TTL_MS: AtomicU64 = AtomicU64::new(DEFAULT_CANDIDATE_CACHE_TTL_MS);
static QUOTA_GUARD_PRIMARY_MIN_REMAINING_BITS: AtomicU64 =
    AtomicU64::new(DEFAULT_QUOTA_GUARD_PRIMARY_MIN_REMAINING_PERCENT.to_bits());
static QUOTA_GUARD_SECONDARY_MIN_REMAINING_BITS: AtomicU64 =
    AtomicU64::new(DEFAULT_QUOTA_GUARD_SECONDARY_MIN_REMAINING_PERCENT.to_bits());
static QUOTA_GUARD_ENABLED: AtomicBool = AtomicBool::new(DEFAULT_QUOTA_GUARD_ENABLED);
static QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK: AtomicBool =
    AtomicBool::new(DEFAULT_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK);
static CURRENT_DB_PATH: OnceLock<RwLock<String>> = OnceLock::new();
const DEFAULT_CANDIDATE_CACHE_TTL_MS: u64 = 500;
const CANDIDATE_CACHE_TTL_ENV: &str = "CODEXMANAGER_CANDIDATE_CACHE_TTL_MS";
// OpenAI 在 used_percent 未到 100 时就会触发 usage limit（常见于 ChatGPT Plus OAuth
// 账号的 5 小时窗口）。将快要耗尽的账号移出正常候选，必要时按兜底开关使用低额度账号。
pub(crate) const LOW_QUOTA_THRESHOLD_ENV: &str = "CODEXMANAGER_LOW_QUOTA_THRESHOLD_PERCENT";
const DEFAULT_LOW_QUOTA_THRESHOLD_PERCENT: f64 = 95.0;
pub(crate) const QUOTA_GUARD_ENABLED_ENV: &str = "CODEXMANAGER_QUOTA_GUARD_ENABLED";
pub(crate) const QUOTA_GUARD_PRIMARY_MIN_REMAINING_ENV: &str =
    "CODEXMANAGER_QUOTA_GUARD_5H_MIN_REMAINING_PERCENT";
pub(crate) const QUOTA_GUARD_SECONDARY_MIN_REMAINING_ENV: &str =
    "CODEXMANAGER_QUOTA_GUARD_WEEKLY_MIN_REMAINING_PERCENT";
pub(crate) const QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK_ENV: &str =
    "CODEXMANAGER_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK";
const DEFAULT_QUOTA_GUARD_ENABLED: bool = false;
const DEFAULT_QUOTA_GUARD_PRIMARY_MIN_REMAINING_PERCENT: f64 = 5.0;
const DEFAULT_QUOTA_GUARD_SECONDARY_MIN_REMAINING_PERCENT: f64 = 10.0;
const DEFAULT_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK: bool = true;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaGuardConfig {
    pub enabled: bool,
    pub primary_min_remaining_percent: f64,
    pub secondary_min_remaining_percent: f64,
    pub allow_all_low_quota_fallback: bool,
}

impl Default for QuotaGuardConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_QUOTA_GUARD_ENABLED,
            primary_min_remaining_percent: DEFAULT_QUOTA_GUARD_PRIMARY_MIN_REMAINING_PERCENT,
            secondary_min_remaining_percent: DEFAULT_QUOTA_GUARD_SECONDARY_MIN_REMAINING_PERCENT,
            allow_all_low_quota_fallback: DEFAULT_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK,
        }
    }
}

impl QuotaGuardConfig {
    fn normalized(self) -> Self {
        Self {
            enabled: self.enabled,
            primary_min_remaining_percent: normalize_percent(self.primary_min_remaining_percent),
            secondary_min_remaining_percent: normalize_percent(
                self.secondary_min_remaining_percent,
            ),
            allow_all_low_quota_fallback: self.allow_all_low_quota_fallback,
        }
    }

    fn has_threshold(self) -> bool {
        self.primary_min_remaining_percent > 0.0 || self.secondary_min_remaining_percent > 0.0
    }
}

#[derive(Clone)]
struct CandidateSnapshotCache {
    db_path: String,
    low_quota_mode: LowQuotaCandidateMode,
    expires_at: Instant,
    candidates: Vec<(Account, Token)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LowQuotaCandidateMode {
    NormalOnly,
    AppendFallback,
}

/// 函数 `collect_gateway_candidates`
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
pub(crate) fn collect_gateway_candidates(
    storage: &Storage,
) -> Result<Vec<(Account, Token)>, String> {
    collect_gateway_candidates_with_low_quota_mode(storage, LowQuotaCandidateMode::NormalOnly)
}

pub(crate) fn collect_gateway_candidates_with_low_quota_mode(
    storage: &Storage,
    low_quota_mode: LowQuotaCandidateMode,
) -> Result<Vec<(Account, Token)>, String> {
    if let Some(cached) = read_candidate_cache(low_quota_mode) {
        return Ok(cached);
    }

    let candidates = collect_gateway_candidates_uncached(storage, low_quota_mode, None)?;
    write_candidate_cache(low_quota_mode, candidates.clone());
    Ok(candidates)
}

pub(crate) fn collect_gateway_candidates_for_accounts_with_low_quota_mode(
    storage: &Storage,
    account_ids: &[String],
    low_quota_mode: LowQuotaCandidateMode,
) -> Result<Vec<(Account, Token)>, String> {
    if account_ids.is_empty() {
        return Ok(Vec::new());
    }
    collect_gateway_candidates_uncached(storage, low_quota_mode, Some(account_ids))
}

/// 函数 `collect_gateway_candidates_uncached`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
///
/// # 返回
/// 返回函数执行结果
fn collect_gateway_candidates_uncached(
    storage: &Storage,
    low_quota_mode: LowQuotaCandidateMode,
    account_ids: Option<&[String]>,
) -> Result<Vec<(Account, Token)>, String> {
    // 选择可用账号作为网关上游候选
    let candidates = match account_ids {
        Some(account_ids) => storage.list_gateway_candidates_for_accounts(account_ids),
        None => storage.list_gateway_candidates(),
    }
    .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(candidates.len());
    for (account, token) in candidates {
        let mut candidate_account = account.clone();
        let (chatgpt_account_id, workspace_id) = derive_account_meta(&token);
        if patch_account_meta_in_place(&mut candidate_account, chatgpt_account_id, workspace_id) {
            candidate_account.updated_at = now_ts();
            let _ = storage.update_account_workspace_identity(
                &candidate_account.id,
                candidate_account.chatgpt_account_id.as_deref(),
                candidate_account.workspace_id.as_deref(),
                candidate_account.updated_at,
            );
        }
        out.push((candidate_account, token));
    }
    apply_quota_guard(storage, &mut out, low_quota_mode);
    if out.is_empty() && account_ids.is_none() {
        log_no_candidates(storage);
    }
    Ok(out)
}

/// 将低于保留额度阈值的账号从正常候选中剔除；正常池为空时按兜底开关决定是否继续使用低额度账号。
fn apply_quota_guard(
    storage: &Storage,
    candidates: &mut Vec<(Account, Token)>,
    low_quota_mode: LowQuotaCandidateMode,
) {
    if candidates.is_empty() {
        return;
    }
    let config = current_quota_guard_config();
    if !config.enabled || !config.has_threshold() {
        return;
    }
    let account_ids = candidates
        .iter()
        .map(|(account, _)| account.id.clone())
        .collect::<Vec<_>>();
    let low_quota_ids = load_low_quota_account_ids(storage, &account_ids, config);
    if low_quota_ids.is_empty() {
        return;
    }
    let mut normal = Vec::with_capacity(candidates.len());
    let mut low_quota = Vec::new();
    for candidate in candidates.drain(..) {
        if low_quota_ids.contains(candidate.0.id.as_str()) {
            low_quota.push(candidate);
        } else {
            normal.push(candidate);
        }
    }
    if normal.is_empty() && config.allow_all_low_quota_fallback {
        *candidates = low_quota;
    } else if low_quota_mode == LowQuotaCandidateMode::AppendFallback {
        normal.extend(low_quota);
        *candidates = normal;
    } else {
        *candidates = normal;
    }
}

fn load_low_quota_account_ids(
    storage: &Storage,
    account_ids: &[String],
    config: QuotaGuardConfig,
) -> std::collections::HashSet<String> {
    storage
        .low_quota_account_ids_for_accounts(
            account_ids,
            config.primary_min_remaining_percent,
            config.secondary_min_remaining_percent,
        )
        .unwrap_or_default()
        .into_iter()
        .collect()
}

pub(crate) fn low_quota_threshold_percent() -> f64 {
    std::env::var(LOW_QUOTA_THRESHOLD_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .filter(|pct| pct.is_finite() && *pct > 0.0 && *pct <= 100.0)
        .unwrap_or(DEFAULT_LOW_QUOTA_THRESHOLD_PERCENT)
}

pub(crate) fn current_quota_guard_config() -> QuotaGuardConfig {
    ensure_selection_config_loaded();
    QuotaGuardConfig {
        enabled: QUOTA_GUARD_ENABLED.load(Ordering::Relaxed),
        primary_min_remaining_percent: f64::from_bits(
            QUOTA_GUARD_PRIMARY_MIN_REMAINING_BITS.load(Ordering::Relaxed),
        ),
        secondary_min_remaining_percent: f64::from_bits(
            QUOTA_GUARD_SECONDARY_MIN_REMAINING_BITS.load(Ordering::Relaxed),
        ),
        allow_all_low_quota_fallback: QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK.load(Ordering::Relaxed),
    }
    .normalized()
}

pub(crate) fn set_quota_guard_config(config: QuotaGuardConfig) -> QuotaGuardConfig {
    let normalized = config.normalized();
    QUOTA_GUARD_ENABLED.store(normalized.enabled, Ordering::Relaxed);
    QUOTA_GUARD_PRIMARY_MIN_REMAINING_BITS.store(
        normalized.primary_min_remaining_percent.to_bits(),
        Ordering::Relaxed,
    );
    QUOTA_GUARD_SECONDARY_MIN_REMAINING_BITS.store(
        normalized.secondary_min_remaining_percent.to_bits(),
        Ordering::Relaxed,
    );
    QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK
        .store(normalized.allow_all_low_quota_fallback, Ordering::Relaxed);
    clear_candidate_cache();
    normalized
}

pub(crate) fn is_low_quota_snapshot(snap: &UsageSnapshotRecord) -> bool {
    let config = current_quota_guard_config();
    config.enabled && config.has_threshold() && is_low_quota_snapshot_at(snap, config)
}

fn is_low_quota_snapshot_at(snap: &UsageSnapshotRecord, config: QuotaGuardConfig) -> bool {
    let primary_low = config.primary_min_remaining_percent > 0.0
        && snap
            .used_percent
            .is_some_and(|pct| remaining_percent(pct) <= config.primary_min_remaining_percent);
    let secondary_low = config.secondary_min_remaining_percent > 0.0
        && snap
            .secondary_used_percent
            .is_some_and(|pct| remaining_percent(pct) <= config.secondary_min_remaining_percent);
    primary_low || secondary_low
}

fn remaining_percent(used_percent: f64) -> f64 {
    (100.0 - used_percent).clamp(0.0, 100.0)
}

fn normalize_percent(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        0.0
    }
}

fn parse_bool_env(raw: &str, fallback: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => fallback,
    }
}

/// 函数 `read_candidate_cache`
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
fn read_candidate_cache(low_quota_mode: LowQuotaCandidateMode) -> Option<Vec<(Account, Token)>> {
    let ttl = candidate_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let db_path = cache_identity()?;
    let now = Instant::now();
    let mutex = CANDIDATE_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("candidate snapshot cache lock poisoned; dropping cache and continuing");
            let mut guard = poisoned.into_inner();
            *guard = None;
            guard
        }
    };
    let cached = guard.as_ref()?;
    if cached.db_path != db_path
        || cached.low_quota_mode != low_quota_mode
        || cached.expires_at <= now
    {
        *guard = None;
        return None;
    }
    Some(cached.candidates.clone())
}

/// 函数 `write_candidate_cache`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
///
/// # 返回
/// 无
fn write_candidate_cache(low_quota_mode: LowQuotaCandidateMode, candidates: Vec<(Account, Token)>) {
    let ttl = candidate_cache_ttl();
    if ttl.is_zero() {
        return;
    }
    let Some(db_path) = cache_identity() else {
        return;
    };
    let expires_at = Instant::now() + ttl;
    let mutex = CANDIDATE_SNAPSHOT_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("candidate snapshot cache lock poisoned; recovering");
            poisoned.into_inner()
        }
    };
    *guard = Some(CandidateSnapshotCache {
        db_path,
        low_quota_mode,
        expires_at,
        candidates,
    });
}

/// 函数 `cache_identity`
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
fn cache_identity() -> Option<String> {
    let db_path = current_db_path();
    if db_path.trim().is_empty() || db_path == "<unset>" {
        return None;
    }
    Some(db_path)
}

/// 函数 `candidate_cache_ttl`
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
fn candidate_cache_ttl() -> Duration {
    ensure_selection_config_loaded();
    let ttl_ms = CANDIDATE_CACHE_TTL_MS.load(Ordering::Relaxed);
    Duration::from_millis(ttl_ms)
}

/// 函数 `current_db_path`
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
fn current_db_path() -> String {
    ensure_selection_config_loaded();
    crate::lock_utils::read_recover(current_db_path_cell(), "current_db_path").clone()
}

/// 函数 `log_no_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
///
/// # 返回
/// 无
fn log_no_candidates(storage: &Storage) {
    let account_count = storage.account_count().unwrap_or_default();
    let token_account_count = storage.token_account_count().unwrap_or_default();
    let snapshot_count = storage.usage_snapshot_count().unwrap_or_default();
    let status_counts = storage
        .account_status_counts()
        .unwrap_or_default()
        .into_iter()
        .map(|item| format!("{}={}", item.status, item.count))
        .collect::<Vec<_>>()
        .join(",");
    let db_path = current_db_path();
    log::warn!(
        "gateway no candidates: db_path={}, accounts={}, token_accounts={}, snapshots={}, statuses={}",
        db_path,
        account_count,
        token_account_count,
        snapshot_count,
        status_counts
    );
}

/// 函数 `reload_from_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 无
pub(super) fn reload_from_env() {
    let ttl_ms = std::env::var(CANDIDATE_CACHE_TTL_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_CANDIDATE_CACHE_TTL_MS);
    CANDIDATE_CACHE_TTL_MS.store(ttl_ms, Ordering::Relaxed);

    let legacy_min_remaining =
        || Some(100.0 - low_quota_threshold_percent()).map(|value| value.clamp(0.0, 100.0));
    let primary_min_remaining = std::env::var(QUOTA_GUARD_PRIMARY_MIN_REMAINING_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .map(normalize_percent)
        .or_else(|| {
            std::env::var(LOW_QUOTA_THRESHOLD_ENV)
                .ok()
                .and_then(|_| legacy_min_remaining())
        })
        .unwrap_or(DEFAULT_QUOTA_GUARD_PRIMARY_MIN_REMAINING_PERCENT);
    let secondary_min_remaining = std::env::var(QUOTA_GUARD_SECONDARY_MIN_REMAINING_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .map(normalize_percent)
        .or_else(|| {
            std::env::var(LOW_QUOTA_THRESHOLD_ENV)
                .ok()
                .and_then(|_| legacy_min_remaining())
        })
        .unwrap_or(DEFAULT_QUOTA_GUARD_SECONDARY_MIN_REMAINING_PERCENT);
    let enabled = std::env::var(QUOTA_GUARD_ENABLED_ENV)
        .ok()
        .map(|raw| parse_bool_env(&raw, DEFAULT_QUOTA_GUARD_ENABLED))
        .unwrap_or(DEFAULT_QUOTA_GUARD_ENABLED);
    let allow_fallback = std::env::var(QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK_ENV)
        .ok()
        .map(|raw| parse_bool_env(&raw, DEFAULT_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK))
        .unwrap_or(DEFAULT_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK);
    let _ = set_quota_guard_config(QuotaGuardConfig {
        enabled,
        primary_min_remaining_percent: primary_min_remaining,
        secondary_min_remaining_percent: secondary_min_remaining,
        allow_all_low_quota_fallback: allow_fallback,
    });

    let db_path = std::env::var("CODEXMANAGER_DB_PATH").unwrap_or_else(|_| "<unset>".to_string());
    let mut cached = crate::lock_utils::write_recover(current_db_path_cell(), "current_db_path");
    *cached = db_path;
    clear_candidate_cache();
}

pub(crate) fn invalidate_candidate_cache() {
    clear_candidate_cache();
}

/// 函数 `ensure_selection_config_loaded`
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
fn ensure_selection_config_loaded() {
    let _ = SELECTION_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

/// 函数 `current_db_path_cell`
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
fn current_db_path_cell() -> &'static RwLock<String> {
    CURRENT_DB_PATH.get_or_init(|| RwLock::new("<unset>".to_string()))
}

/// 函数 `clear_candidate_cache`
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
fn clear_candidate_cache() {
    if let Some(mutex) = CANDIDATE_SNAPSHOT_CACHE.get() {
        let mut guard = match mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("candidate snapshot cache lock poisoned; recovering for tests");
                poisoned.into_inner()
            }
        };
        *guard = None;
    }
}

/// 函数 `clear_candidate_cache_for_tests`
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
fn clear_candidate_cache_for_tests() {
    clear_candidate_cache();
}

#[cfg(test)]
#[path = "tests/selection_tests.rs"]
mod tests;
