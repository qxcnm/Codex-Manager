use super::route_quality::route_health_score;
use codexmanager_core::storage::{Account, Token};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ROUTE_STRATEGY_ENV: &str = "CODEXMANAGER_ROUTE_STRATEGY";
const ROUTE_MODE_ORDERED: u8 = 0;
const ROUTE_MODE_BALANCED_ROUND_ROBIN: u8 = 1;
const ROUTE_STRATEGY_ORDERED: &str = "ordered";
const ROUTE_STRATEGY_BALANCED: &str = "balanced";
const ROUTE_HEALTH_P2C_ENABLED_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_ENABLED";
const ROUTE_HEALTH_P2C_ORDERED_WINDOW_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_ORDERED_WINDOW";
const ROUTE_HEALTH_P2C_BALANCED_WINDOW_ENV: &str = "CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW";
const ROUTE_STATE_TTL_SECS_ENV: &str = "CODEXMANAGER_ROUTE_STATE_TTL_SECS";
const ROUTE_STATE_CAPACITY_ENV: &str = "CODEXMANAGER_ROUTE_STATE_CAPACITY";
const DEFAULT_ROUTE_HEALTH_P2C_ENABLED: bool = true;
const DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW: usize = 3;
// 中文注释：balanced 默认应严格轮询所有可用账号；仅在显式调大窗口时才启用健康度换头。
const DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW: usize = 1;
// 中文注释：Route 状态（按 key_id + model 维度）用于 round-robin 起点与 P2C nonce。
// 为贴近 Codex 默认行为，TTL 与容量默认关闭；只有显式配置时才启用回收限制。
const DEFAULT_ROUTE_STATE_TTL_SECS: u64 = 0;
const DEFAULT_ROUTE_STATE_CAPACITY: usize = 0;
const ROUTE_STATE_MAINTENANCE_EVERY: u64 = 64;

static ROUTE_MODE: AtomicU8 = AtomicU8::new(ROUTE_MODE_ORDERED);
static ROUTE_HEALTH_P2C_ENABLED: AtomicBool = AtomicBool::new(DEFAULT_ROUTE_HEALTH_P2C_ENABLED);
static ROUTE_HEALTH_P2C_ORDERED_WINDOW: AtomicUsize =
    AtomicUsize::new(DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW);
static ROUTE_HEALTH_P2C_BALANCED_WINDOW: AtomicUsize =
    AtomicUsize::new(DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW);
static ROUTE_STATE_TTL_SECS: AtomicU64 = AtomicU64::new(DEFAULT_ROUTE_STATE_TTL_SECS);
static ROUTE_STATE_CAPACITY: AtomicUsize = AtomicUsize::new(DEFAULT_ROUTE_STATE_CAPACITY);
static ROUTE_STATE: OnceLock<Mutex<RouteRoundRobinState>> = OnceLock::new();
static ROUTE_CONFIG_LOADED: OnceLock<()> = OnceLock::new();

#[derive(Clone)]
struct RouteStateEntry<T> {
    value: T,
    last_seen: Instant,
}

impl<T> RouteStateEntry<T> {
    /// 函数 `new`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - value: 参数 value
    /// - last_seen: 参数 last_seen
    ///
    /// # 返回
    /// 返回函数执行结果
    fn new(value: T, last_seen: Instant) -> Self {
        Self { value, last_seen }
    }
}

#[derive(Clone)]
struct AccountRoundRobinCursor {
    last_selected_account_id: String,
    next_account_id: String,
}

#[derive(Default)]
struct RouteRoundRobinState {
    next_start_by_key_model: HashMap<String, RouteStateEntry<usize>>,
    account_cursor_by_key_model: HashMap<String, RouteStateEntry<AccountRoundRobinCursor>>,
    p2c_nonce_by_key_model: HashMap<String, RouteStateEntry<u64>>,
    manual_preferred_account_id: Option<String>,
    maintenance_tick: u64,
}

/// 函数 `apply_route_strategy`
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
pub(crate) fn apply_route_strategy(
    candidates: &mut [(Account, Token)],
    key_id: &str,
    model: Option<&str>,
) {
    let _ = apply_route_strategy_with_source(candidates, key_id, model);
}

pub(crate) struct RouteStrategyApplication {
    pub(crate) strategy_label: &'static str,
    pub(crate) source: &'static str,
}

pub(crate) fn apply_route_strategy_with_source(
    candidates: &mut [(Account, Token)],
    key_id: &str,
    model: Option<&str>,
) -> RouteStrategyApplication {
    ensure_route_config_loaded();
    let mode = route_mode();
    let default_application = RouteStrategyApplication {
        strategy_label: route_mode_label(mode),
        source: "route_strategy",
    };
    if rotate_to_manual_preferred_account(candidates) {
        return RouteStrategyApplication {
            strategy_label: "manual_preferred_account",
            source: "manual_preferred_account",
        };
    }
    if candidates.len() <= 1 {
        // A single available account is still an actual balanced selection. Persist its
        // identity so a later pool expansion continues with that account's successor rather
        // than reviving a stale cursor from before the pool contracted.
        if candidates.len() == 1 && mode == ROUTE_MODE_BALANCED_ROUND_ROBIN {
            let _ = next_account_start_index(candidates, key_id, model);
        }
        return default_application;
    }

    if mode == ROUTE_MODE_BALANCED_ROUND_ROBIN {
        apply_balanced_account_round_robin(candidates, key_id, model);
    }

    apply_health_p2c(candidates, key_id, model, mode);
    default_application
}

/// 函数 `apply_balanced_round_robin`
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
pub(crate) fn apply_balanced_round_robin<T>(
    candidates: &mut [T],
    key_id: &str,
    model: Option<&str>,
) {
    ensure_route_config_loaded();
    if candidates.len() <= 1 {
        return;
    }
    let start = next_start_index(key_id, model, candidates.len());
    if start > 0 {
        candidates.rotate_left(start);
    }
}

fn apply_balanced_account_round_robin(
    candidates: &mut [(Account, Token)],
    key_id: &str,
    model: Option<&str>,
) {
    if candidates.len() <= 1 {
        return;
    }
    let start = next_account_start_index(candidates, key_id, model);
    if start > 0 {
        candidates.rotate_left(start);
    }
}

pub(crate) fn preview_balanced_round_robin<T>(
    candidates: &mut [T],
    key_id: &str,
    model: Option<&str>,
) {
    ensure_route_config_loaded();
    if candidates.len() <= 1 {
        return;
    }
    let start = current_start_index(key_id, model, candidates.len());
    if start > 0 {
        candidates.rotate_left(start);
    }
}

/// 函数 `rotate_to_manual_preferred_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
///
/// # 返回
/// 返回函数执行结果
fn rotate_to_manual_preferred_account(candidates: &mut [(Account, Token)]) -> bool {
    let Some(account_id) = get_manual_preferred_account() else {
        return false;
    };
    let Some(index) = candidates
        .iter()
        .position(|(account, _)| account.id == account_id)
    else {
        return false;
    };
    if index > 0 {
        candidates.rotate_left(index);
    }
    true
}

/// 函数 `route_mode`
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
fn route_mode() -> u8 {
    ROUTE_MODE.load(Ordering::Relaxed)
}

/// 函数 `route_mode_label`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - mode: 参数 mode
///
/// # 返回
/// 返回函数执行结果
fn route_mode_label(mode: u8) -> &'static str {
    if mode == ROUTE_MODE_BALANCED_ROUND_ROBIN {
        ROUTE_STRATEGY_BALANCED
    } else {
        ROUTE_STRATEGY_ORDERED
    }
}

/// 函数 `parse_route_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn parse_route_mode(raw: &str) -> Option<u8> {
    match raw.trim().to_ascii_lowercase().as_str() {
        ROUTE_STRATEGY_ORDERED | "order" | "priority" | "sequential" => Some(ROUTE_MODE_ORDERED),
        ROUTE_STRATEGY_BALANCED | "round_robin" | "round-robin" | "rr" => {
            Some(ROUTE_MODE_BALANCED_ROUND_ROBIN)
        }
        _ => None,
    }
}

/// 函数 `current_route_strategy`
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
pub(crate) fn current_route_strategy() -> &'static str {
    ensure_route_config_loaded();
    route_mode_label(route_mode())
}

/// 函数 `set_route_strategy`
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
pub(crate) fn set_route_strategy(strategy: &str) -> Result<&'static str, String> {
    ensure_route_config_loaded();
    let Some(mode) = parse_route_mode(strategy) else {
        return Err(
            "invalid strategy; use ordered or balanced (aliases: round_robin/round-robin/rr)"
                .to_string(),
        );
    };
    ROUTE_MODE.store(mode, Ordering::Relaxed);
    if let Some(lock) = ROUTE_STATE.get() {
        let mut state = crate::lock_utils::lock_recover(lock, "route_state");
        state.next_start_by_key_model.clear();
        state.account_cursor_by_key_model.clear();
        state.p2c_nonce_by_key_model.clear();
        state.maintenance_tick = 0;
    }
    Ok(route_mode_label(mode))
}

/// 函数 `get_manual_preferred_account`
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
pub(crate) fn get_manual_preferred_account() -> Option<String> {
    crate::storage_helpers::open_storage()
        .and_then(|storage| {
            crate::account::remote_storage::AccountStorage::new(&storage)
                .preferred_account_id()
                .ok()
        })
        .flatten()
}

/// 函数 `set_manual_preferred_account`
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
pub(crate) fn set_manual_preferred_account(account_id: &str) -> Result<(), String> {
    let id = account_id.trim();
    if id.is_empty() {
        return Err("accountId is required".to_string());
    }
    let storage = crate::storage_helpers::open_storage()
        .ok_or_else(|| "storage not initialized".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .set_preferred_account(Some(id))
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// 函数 `clear_manual_preferred_account`
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
pub(crate) fn clear_manual_preferred_account() {
    if let Some(storage) = crate::storage_helpers::open_storage() {
        let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
        let _ = storage.set_preferred_account(None);
    }
}

/// 函数 `next_start_index`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - key_id: 参数 key_id
/// - model: 参数 model
/// - candidate_count: 参数 candidate_count
///
/// # 返回
/// 返回函数执行结果
fn next_start_index(key_id: &str, model: Option<&str>, candidate_count: usize) -> usize {
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let mut state_guard = crate::lock_utils::lock_recover(lock, "route_state");
    let state = &mut *state_guard;
    let now = Instant::now();
    state.maybe_maintain(now);

    let ttl = route_state_ttl();
    let capacity = route_state_capacity();
    let key = key_model_key(key_id, model);
    remove_entry_if_expired(&mut state.next_start_by_key_model, key.as_str(), now, ttl);
    let start = {
        let entry = state
            .next_start_by_key_model
            .entry(key.clone())
            .or_insert(RouteStateEntry::new(0, now));
        entry.last_seen = now;
        let start = entry.value % candidate_count;
        entry.value = (start + 1) % candidate_count;
        start
    };
    enforce_capacity_pair(
        &mut state.next_start_by_key_model,
        &mut state.p2c_nonce_by_key_model,
        capacity,
    );
    start
}

fn next_account_start_index(
    candidates: &[(Account, Token)],
    key_id: &str,
    model: Option<&str>,
) -> usize {
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let mut state_guard = crate::lock_utils::lock_recover(lock, "route_state");
    let state = &mut *state_guard;
    let now = Instant::now();
    state.maybe_maintain(now);

    let ttl = route_state_ttl();
    let capacity = route_state_capacity();
    let key = key_model_key(key_id, model);
    remove_entry_if_expired(
        &mut state.account_cursor_by_key_model,
        key.as_str(),
        now,
        ttl,
    );

    let previous_cursor = state
        .account_cursor_by_key_model
        .get(key.as_str())
        .map(|entry| entry.value.clone());
    let start = previous_cursor
        .as_ref()
        .and_then(|cursor| {
            candidates
                .iter()
                .position(|(account, _)| account.id == cursor.last_selected_account_id)
                .map(|index| (index + 1) % candidates.len())
                .or_else(|| {
                    candidates
                        .iter()
                        .position(|(account, _)| account.id == cursor.next_account_id)
                })
        })
        .unwrap_or(0);
    let next = (start + 1) % candidates.len();
    state.account_cursor_by_key_model.insert(
        key,
        RouteStateEntry::new(
            AccountRoundRobinCursor {
                last_selected_account_id: candidates[start].0.id.clone(),
                next_account_id: candidates[next].0.id.clone(),
            },
            now,
        ),
    );
    enforce_capacity(&mut state.account_cursor_by_key_model, capacity);
    start
}

fn current_start_index(key_id: &str, model: Option<&str>, candidate_count: usize) -> usize {
    if candidate_count == 0 {
        return 0;
    }
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let state_guard = crate::lock_utils::lock_recover(lock, "route_state");
    let state = &*state_guard;
    let key = key_model_key(key_id, model);
    let Some(entry) = state.next_start_by_key_model.get(key.as_str()) else {
        return 0;
    };
    let ttl = route_state_ttl();
    if !ttl.is_zero() && is_entry_expired(entry.last_seen, Instant::now(), ttl) {
        return 0;
    }
    entry.value % candidate_count
}

/// 函数 `apply_health_p2c`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
/// - key_id: 参数 key_id
/// - model: 参数 model
/// - mode: 参数 mode
///
/// # 返回
/// 无
fn apply_health_p2c(
    candidates: &mut [(Account, Token)],
    key_id: &str,
    model: Option<&str>,
    mode: u8,
) {
    if !route_health_p2c_enabled() {
        return;
    }
    let window = route_health_window(mode).min(candidates.len());
    if window <= 1 {
        return;
    }
    let Some(challenger_idx) = p2c_challenger_index(key_id, model, window) else {
        return;
    };
    let current_score = route_health_score(candidates[0].0.id.as_str());
    let challenger_score = route_health_score(candidates[challenger_idx].0.id.as_str());
    if challenger_score > current_score {
        // 中文注释：只交换头部候选，避免“整段 rotate”过度扰动既有顺序与轮询语义。
        candidates.swap(0, challenger_idx);
    }
}

/// 函数 `p2c_challenger_index`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - key_id: 参数 key_id
/// - model: 参数 model
/// - candidate_count: 参数 candidate_count
///
/// # 返回
/// 返回函数执行结果
fn p2c_challenger_index(
    key_id: &str,
    model: Option<&str>,
    candidate_count: usize,
) -> Option<usize> {
    if candidate_count < 2 {
        return None;
    }
    let lock = ROUTE_STATE.get_or_init(|| Mutex::new(RouteRoundRobinState::default()));
    let mut state_guard = crate::lock_utils::lock_recover(lock, "route_state");
    let state = &mut *state_guard;
    let now = Instant::now();
    state.maybe_maintain(now);

    let ttl = route_state_ttl();
    let capacity = route_state_capacity();
    let key = key_model_key(key_id, model);
    remove_entry_if_expired(&mut state.p2c_nonce_by_key_model, key.as_str(), now, ttl);
    let nonce = {
        let entry = state
            .p2c_nonce_by_key_model
            .entry(key.clone())
            .or_insert(RouteStateEntry::new(0, now));
        entry.last_seen = now;
        let nonce = entry.value;
        entry.value = nonce.wrapping_add(1);
        nonce
    };
    enforce_capacity_pair(
        &mut state.p2c_nonce_by_key_model,
        &mut state.next_start_by_key_model,
        capacity,
    );
    let seed = stable_hash_u64(format!("{key}|{nonce}").as_bytes());
    // 中文注释：当前候选列表已有顺序（ordered / round-robin 后），P2C 只从前 window 内挑一个挑战者
    // 与“当前头部候选”对比，避免完全打乱轮询/排序语义。
    let offset = (seed as usize) % (candidate_count - 1);
    Some(offset + 1)
}

/// 函数 `stable_hash_u64`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - input: 参数 input
///
/// # 返回
/// 返回函数执行结果
fn stable_hash_u64(input: &[u8]) -> u64 {
    let mut hash = 14695981039346656037_u64;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211_u64);
    }
    hash
}

/// 函数 `route_health_p2c_enabled`
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
fn route_health_p2c_enabled() -> bool {
    ROUTE_HEALTH_P2C_ENABLED.load(Ordering::Relaxed)
}

/// 函数 `route_health_window`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - mode: 参数 mode
///
/// # 返回
/// 返回函数执行结果
fn route_health_window(mode: u8) -> usize {
    if mode == ROUTE_MODE_BALANCED_ROUND_ROBIN {
        ROUTE_HEALTH_P2C_BALANCED_WINDOW.load(Ordering::Relaxed)
    } else {
        ROUTE_HEALTH_P2C_ORDERED_WINDOW.load(Ordering::Relaxed)
    }
}

/// 函数 `route_state_ttl`
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
fn route_state_ttl() -> Duration {
    Duration::from_secs(ROUTE_STATE_TTL_SECS.load(Ordering::Relaxed))
}

/// 函数 `route_state_capacity`
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
fn route_state_capacity() -> usize {
    ROUTE_STATE_CAPACITY.load(Ordering::Relaxed)
}

/// 函数 `is_entry_expired`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - last_seen: 参数 last_seen
/// - now: 参数 now
/// - ttl: 参数 ttl
///
/// # 返回
/// 返回函数执行结果
fn is_entry_expired(last_seen: Instant, now: Instant, ttl: Duration) -> bool {
    if ttl.is_zero() {
        return false;
    }
    now.checked_duration_since(last_seen)
        .is_some_and(|age| age > ttl)
}

/// 函数 `remove_entry_if_expired`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - map: 参数 map
/// - key: 参数 key
/// - now: 参数 now
/// - ttl: 参数 ttl
///
/// # 返回
/// 无
fn remove_entry_if_expired<T>(
    map: &mut HashMap<String, RouteStateEntry<T>>,
    key: &str,
    now: Instant,
    ttl: Duration,
) {
    if ttl.is_zero() {
        return;
    }
    let expired = map
        .get(key)
        .is_some_and(|entry| is_entry_expired(entry.last_seen, now, ttl));
    if expired {
        map.remove(key);
    }
}

/// 函数 `prune_expired_entries`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - map: 参数 map
/// - now: 参数 now
/// - ttl: 参数 ttl
///
/// # 返回
/// 无
fn prune_expired_entries<T>(
    map: &mut HashMap<String, RouteStateEntry<T>>,
    now: Instant,
    ttl: Duration,
) {
    if ttl.is_zero() {
        return;
    }
    map.retain(|_, entry| !is_entry_expired(entry.last_seen, now, ttl));
}

/// 函数 `enforce_capacity_pair`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - map: 参数 map
/// - other: 参数 other
/// - capacity: 参数 capacity
///
/// # 返回
/// 无
fn enforce_capacity_pair<T, U>(
    map: &mut HashMap<String, RouteStateEntry<T>>,
    other: &mut HashMap<String, RouteStateEntry<U>>,
    capacity: usize,
) {
    if capacity == 0 {
        return;
    }
    while map.len() > capacity {
        let Some(oldest_key) = find_oldest_key(map) else {
            break;
        };
        map.remove(oldest_key.as_str());
        other.remove(oldest_key.as_str());
    }
}

fn enforce_capacity<T>(map: &mut HashMap<String, RouteStateEntry<T>>, capacity: usize) {
    if capacity == 0 {
        return;
    }
    while map.len() > capacity {
        let Some(oldest_key) = find_oldest_key(map) else {
            break;
        };
        map.remove(oldest_key.as_str());
    }
}

/// 函数 `find_oldest_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - map: 参数 map
///
/// # 返回
/// 返回函数执行结果
fn find_oldest_key<T>(map: &HashMap<String, RouteStateEntry<T>>) -> Option<String> {
    map.iter()
        .min_by(|(ka, ea), (kb, eb)| ea.last_seen.cmp(&eb.last_seen).then_with(|| ka.cmp(kb)))
        .map(|(key, _)| key.clone())
}

/// 函数 `key_model_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - key_id: 参数 key_id
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
fn key_model_key(key_id: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}",
        key_id.trim(),
        model
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("-")
    )
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
    let raw = std::env::var(ROUTE_STRATEGY_ENV).unwrap_or_default();
    let mode = parse_route_mode(raw.as_str()).unwrap_or(ROUTE_MODE_ORDERED);
    ROUTE_MODE.store(mode, Ordering::Relaxed);
    ROUTE_HEALTH_P2C_ENABLED.store(
        env_bool_or(
            ROUTE_HEALTH_P2C_ENABLED_ENV,
            DEFAULT_ROUTE_HEALTH_P2C_ENABLED,
        ),
        Ordering::Relaxed,
    );
    ROUTE_HEALTH_P2C_ORDERED_WINDOW.store(
        env_usize_or(
            ROUTE_HEALTH_P2C_ORDERED_WINDOW_ENV,
            DEFAULT_ROUTE_HEALTH_P2C_ORDERED_WINDOW,
        ),
        Ordering::Relaxed,
    );
    ROUTE_HEALTH_P2C_BALANCED_WINDOW.store(
        env_usize_or(
            ROUTE_HEALTH_P2C_BALANCED_WINDOW_ENV,
            DEFAULT_ROUTE_HEALTH_P2C_BALANCED_WINDOW,
        ),
        Ordering::Relaxed,
    );
    ROUTE_STATE_TTL_SECS.store(
        env_u64_or(ROUTE_STATE_TTL_SECS_ENV, DEFAULT_ROUTE_STATE_TTL_SECS),
        Ordering::Relaxed,
    );
    ROUTE_STATE_CAPACITY.store(
        env_usize_or(ROUTE_STATE_CAPACITY_ENV, DEFAULT_ROUTE_STATE_CAPACITY),
        Ordering::Relaxed,
    );

    if let Some(lock) = ROUTE_STATE.get() {
        let mut state = crate::lock_utils::lock_recover(lock, "route_state");
        state.next_start_by_key_model.clear();
        state.account_cursor_by_key_model.clear();
        state.p2c_nonce_by_key_model.clear();
        state.manual_preferred_account_id = None;
        state.maintenance_tick = 0;
    }
}

/// 函数 `ensure_route_config_loaded`
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
fn ensure_route_config_loaded() {
    let _ = ROUTE_CONFIG_LOADED.get_or_init(|| reload_from_env());
}

/// 函数 `env_bool_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn env_bool_or(name: &str, default: bool) -> bool {
    let Ok(raw) = std::env::var(name) else {
        return default;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

/// 函数 `env_usize_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn env_usize_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

/// 函数 `env_u64_or`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
/// - default: 参数 default
///
/// # 返回
/// 返回函数执行结果
fn env_u64_or(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

impl RouteRoundRobinState {
    /// 函数 `maybe_maintain`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - now: 参数 now
    ///
    /// # 返回
    /// 无
    fn maybe_maintain(&mut self, now: Instant) {
        self.maintenance_tick = self.maintenance_tick.wrapping_add(1);
        if self.maintenance_tick % ROUTE_STATE_MAINTENANCE_EVERY != 0 {
            return;
        }
        let ttl = route_state_ttl();
        let capacity = route_state_capacity();
        prune_expired_entries(&mut self.next_start_by_key_model, now, ttl);
        prune_expired_entries(&mut self.account_cursor_by_key_model, now, ttl);
        prune_expired_entries(&mut self.p2c_nonce_by_key_model, now, ttl);
        enforce_capacity(&mut self.account_cursor_by_key_model, capacity);
        enforce_capacity_pair(
            &mut self.next_start_by_key_model,
            &mut self.p2c_nonce_by_key_model,
            capacity,
        );
        enforce_capacity_pair(
            &mut self.p2c_nonce_by_key_model,
            &mut self.next_start_by_key_model,
            capacity,
        );
    }
}

/// 函数 `clear_route_state_for_tests`
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
fn clear_route_state_for_tests() {
    super::route_quality::clear_route_quality_for_tests();
    if let Some(lock) = ROUTE_STATE.get() {
        let mut state = crate::lock_utils::lock_recover(lock, "route_state");
        state.next_start_by_key_model.clear();
        state.account_cursor_by_key_model.clear();
        state.p2c_nonce_by_key_model.clear();
        state.manual_preferred_account_id = None;
        state.maintenance_tick = 0;
    }
}

#[cfg(test)]
#[path = "tests/route_hint_tests.rs"]
mod tests;
