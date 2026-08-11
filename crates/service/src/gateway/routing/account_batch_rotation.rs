use codexmanager_core::storage::{
    now_ts, Account, AdapterCredentialProbeState, Storage, Token, UsageSnapshotRecord,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const DEFAULT_BATCH_SIZE: usize = 5;
const DEFAULT_FALLBACK_WINDOW_MINUTES: u64 = 300;
const DEFAULT_MAX_ATTEMPTS_PER_REQUEST: usize = 4;
const STATE_KEY_PREFIX: &str = "gateway.account_batch_state.codex.";

static ENABLED: AtomicBool = AtomicBool::new(false);
static BATCH_SIZE: AtomicUsize = AtomicUsize::new(DEFAULT_BATCH_SIZE);
static FALLBACK_WINDOW_MINUTES: AtomicU64 = AtomicU64::new(DEFAULT_FALLBACK_WINDOW_MINUTES);
static MAX_ATTEMPTS_PER_REQUEST: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_ATTEMPTS_PER_REQUEST);
static STATES: OnceLock<Mutex<HashMap<String, AccountBatchState>>> = OnceLock::new();
// Admission probes must never make a customer request wait.  A single
// background worker refreshes missing batch slots; live traffic uses only the
// last confirmed pool (and may immediately fall through to the configured
// aggregate fallback when that pool is empty).
static AUTO_REFILL_PROBE_RUNNING: AtomicBool = AtomicBool::new(false);

const AUTO_REFILL_PROBE_CONCURRENCY: usize = 4;
// A historical "available" flag is not admission. Active gateway successes
// keep this timestamp fresh; idle accounts must pass a new Responses probe before
// they can occupy a batch slot.
const GATEWAY_PROBE_FRESHNESS_SECS: i64 = 120;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountBatchRotationConfig {
    pub enabled: bool,
    pub batch_size: usize,
    pub fallback_window_minutes: u64,
    #[serde(default = "default_max_attempts_per_request")]
    pub max_attempts_per_request: usize,
}

const fn default_max_attempts_per_request() -> usize {
    DEFAULT_MAX_ATTEMPTS_PER_REQUEST
}

impl Default for AccountBatchRotationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            batch_size: DEFAULT_BATCH_SIZE,
            fallback_window_minutes: DEFAULT_FALLBACK_WINDOW_MINUTES,
            max_attempts_per_request: DEFAULT_MAX_ATTEMPTS_PER_REQUEST,
        }
    }
}

impl AccountBatchRotationConfig {
    pub(crate) fn normalized(self) -> Self {
        Self {
            enabled: self.enabled,
            batch_size: self.batch_size.clamp(1, 10_000),
            fallback_window_minutes: self.fallback_window_minutes.clamp(1, 43_200),
            max_attempts_per_request: self.max_attempts_per_request.clamp(1, 64),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AccountBatchState {
    #[serde(default)]
    ordered_account_ids: Vec<String>,
    #[serde(default)]
    current_batch: usize,
    #[serde(default)]
    effective_batch_size: usize,
    #[serde(default)]
    cycle: u64,
    #[serde(default)]
    exhausted_at: Option<i64>,
    #[serde(default)]
    earliest_reset_at: Option<i64>,
    #[serde(default)]
    current_batch_available: usize,
    #[serde(default)]
    blocked_until_by_batch: HashMap<usize, i64>,
    #[serde(default)]
    updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountBatchRotationStatus {
    pub enabled: bool,
    pub batch_size: usize,
    pub fallback_window_minutes: u64,
    pub max_attempts_per_request: usize,
    pub scope: Option<String>,
    pub current_batch: usize,
    pub total_batches: usize,
    pub cycle: u64,
    pub current_batch_accounts: usize,
    pub current_batch_available: usize,
    pub earliest_reset_at: Option<i64>,
}

pub(crate) fn account_batch_rotation_config() -> AccountBatchRotationConfig {
    AccountBatchRotationConfig {
        enabled: ENABLED.load(Ordering::Relaxed),
        batch_size: BATCH_SIZE.load(Ordering::Relaxed).max(1),
        fallback_window_minutes: FALLBACK_WINDOW_MINUTES.load(Ordering::Relaxed).max(1),
        max_attempts_per_request: MAX_ATTEMPTS_PER_REQUEST.load(Ordering::Relaxed).max(1),
    }
}

pub(crate) fn set_account_batch_rotation_config(
    config: AccountBatchRotationConfig,
) -> AccountBatchRotationConfig {
    let config = config.normalized();
    ENABLED.store(config.enabled, Ordering::Relaxed);
    BATCH_SIZE.store(config.batch_size, Ordering::Relaxed);
    FALLBACK_WINDOW_MINUTES.store(config.fallback_window_minutes, Ordering::Relaxed);
    MAX_ATTEMPTS_PER_REQUEST.store(config.max_attempts_per_request, Ordering::Relaxed);
    config
}

fn state_key(scope: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in scope.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{STATE_KEY_PREFIX}{hash:016x}")
}

fn reconcile_available_pool(
    state: &mut AccountBatchState,
    available_pool: &[String],
    batch_size: usize,
) {
    let size_changed = state.effective_batch_size != batch_size;
    if state.ordered_account_ids == available_pool && !size_changed {
        return;
    }

    // Preserve the current position when possible, but never preserve an
    // unavailable account as a batch placeholder.
    let current_start = state.current_batch.saturating_mul(batch_size);
    let anchor = state
        .ordered_account_ids
        .get(current_start..(current_start + batch_size).min(state.ordered_account_ids.len()))
        .and_then(|ids| ids.iter().find(|id| available_pool.contains(id)))
        .cloned();

    state.ordered_account_ids = available_pool.to_vec();
    state.effective_batch_size = batch_size;
    state.blocked_until_by_batch.clear();
    let total_batches = available_pool.len().div_ceil(batch_size);
    state.current_batch = if total_batches == 0 {
        0
    } else if size_changed {
        0
    } else if let Some(anchor) = anchor {
        available_pool
            .iter()
            .position(|id| id == &anchor)
            .map(|index| index / batch_size)
            .unwrap_or(0)
    } else {
        state.current_batch.min(total_batches - 1)
    };
}

fn account_quota_reset_at(snapshot: Option<&UsageSnapshotRecord>, fallback_at: i64) -> Option<i64> {
    let snapshot = snapshot?;
    let mut resets = Vec::new();
    if snapshot.used_percent.is_some_and(|used| used >= 100.0) {
        resets.push(snapshot.resets_at.unwrap_or(fallback_at));
    }
    if snapshot
        .secondary_used_percent
        .is_some_and(|used| used >= 100.0)
    {
        resets.push(snapshot.secondary_resets_at.unwrap_or(fallback_at));
    }
    // The account is reusable only after every exhausted window has reset.
    resets.into_iter().max()
}

fn update_batch_blocks(
    state: &mut AccountBatchState,
    globally_available_ids: &HashSet<String>,
    snapshots: &HashMap<String, UsageSnapshotRecord>,
    batch_size: usize,
    fallback_window_minutes: u64,
    now: i64,
) {
    let fallback_at = now.saturating_add((fallback_window_minutes.saturating_mul(60)) as i64);
    for (batch_index, batch_ids) in state.ordered_account_ids.chunks(batch_size).enumerate() {
        // 可用性必须优先于持久化的阻塞时间。账号可能在阻塞窗口内完成
        // Token 修复、解除冷却或重新探测为 active；旧状态不能继续把它
        // 挡在池外，否则会出现“有活号但 503 无可用账号”。
        if batch_ids
            .iter()
            .any(|id| globally_available_ids.contains(id))
        {
            state.blocked_until_by_batch.remove(&batch_index);
            continue;
        }
        if state
            .blocked_until_by_batch
            .get(&batch_index)
            .is_some_and(|until| *until > now)
        {
            continue;
        }
        let earliest = batch_ids
            .iter()
            .map(|id| {
                if let Some(reset_at) = account_quota_reset_at(snapshots.get(id), fallback_at) {
                    return reset_at.max(now);
                }
                if let Some(cooldown) = crate::gateway::account_cooldown_info(id) {
                    return if cooldown.rate_limited {
                        fallback_at
                    } else {
                        cooldown.until
                    };
                }
                // limited/unavailable without a reliable reset timestamp uses the configured window.
                fallback_at
            })
            .min()
            .unwrap_or(fallback_at);
        state.blocked_until_by_batch.insert(batch_index, earliest);
    }
}

fn select_batch(
    state: &mut AccountBatchState,
    full_pool: &[String],
    available_ids: &HashSet<String>,
    batch_size: usize,
    now: i64,
) -> Vec<String> {
    if state.ordered_account_ids.is_empty() {
        state.ordered_account_ids = full_pool.to_vec();
    }
    let mut total_batches = state.ordered_account_ids.len().div_ceil(batch_size);
    if total_batches == 0 {
        return Vec::new();
    }
    state.current_batch %= total_batches;
    for offset in 0..total_batches {
        let index = (state.current_batch + offset) % total_batches;
        if state
            .blocked_until_by_batch
            .get(&index)
            .is_some_and(|until| *until > now)
        {
            continue;
        }
        let start = index * batch_size;
        let ids = &state.ordered_account_ids
            [start..(start + batch_size).min(state.ordered_account_ids.len())];
        let mut selected = ids
            .iter()
            .filter(|id| available_ids.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !selected.is_empty() {
            // 批次大小表示实际参与调用的可用账号数量，而不是固定槽位数量。
            // 当前批次有账号限额/冷却时，从后续顺位补满，避免“配置 2 个、实际只打 1 个”。
            if selected.len() < batch_size {
                for offset in 0..state.ordered_account_ids.len() {
                    let candidate_index = (start + offset) % state.ordered_account_ids.len();
                    let candidate_batch = candidate_index / batch_size;
                    if state
                        .blocked_until_by_batch
                        .get(&candidate_batch)
                        .is_some_and(|until| *until > now)
                    {
                        continue;
                    }
                    let candidate_id = &state.ordered_account_ids[candidate_index];
                    if available_ids.contains(candidate_id.as_str())
                        && !selected.iter().any(|id| id == candidate_id)
                    {
                        selected.push(candidate_id.clone());
                        if selected.len() >= batch_size {
                            break;
                        }
                    }
                }
            }
            if index < state.current_batch || state.current_batch + offset >= total_batches {
                state.cycle = state.cycle.saturating_add(1);
                // New accounts and sort changes become effective only at a forward wrap.
                state.ordered_account_ids = full_pool.to_vec();
                total_batches = state.ordered_account_ids.len().div_ceil(batch_size).max(1);
                state.current_batch = index.min(total_batches - 1);
            } else {
                state.current_batch = index;
            }
            state.exhausted_at = None;
            return selected;
        }
    }
    if state.ordered_account_ids != full_pool {
        state.ordered_account_ids = full_pool.to_vec();
        state.current_batch = 0;
        state.cycle = state.cycle.saturating_add(1);
        return select_batch(state, full_pool, available_ids, batch_size, now);
    }
    Vec::new()
}

#[derive(Debug)]
struct ConfirmedGatewayPool {
    ordered_ids: Vec<String>,
    request_available_ids: HashSet<String>,
    refill_probe_ids: Vec<String>,
}

fn build_confirmed_gateway_pool(
    gateway_candidates: &[(Account, Token)],
    request_candidate_ids: &HashSet<String>,
    probe_states: &[AdapterCredentialProbeState],
    now: i64,
    is_in_cooldown: impl Fn(&str) -> bool,
) -> ConfirmedGatewayPool {
    let states = probe_states
        .iter()
        .map(|state| (state.credential_id.as_str(), state))
        .collect::<HashMap<_, _>>();
    let mut ordered_ids = Vec::new();
    let mut request_available_ids = HashSet::new();
    let mut unprobed_ids = Vec::new();
    let mut retryable_failed_ids = Vec::new();

    for (account, _) in gateway_candidates {
        let in_cooldown = is_in_cooldown(&account.id);
        let state = states.get(account.id.as_str()).copied();
        if state.is_some_and(|state| {
            state.status == "available"
                && state.error_code.as_deref()
                    == Some(crate::usage_refresh::CODEX_RESPONSES_VERIFIED)
                && now.saturating_sub(state.checked_at) <= GATEWAY_PROBE_FRESHNESS_SECS
        }) {
            // A short network cooldown is a routing preference, not proof that
            // a Responses-verified account is dead. Keep it in the confirmed
            // pool so the executor can skip it when another account exists,
            // but still use it as the last live account instead of returning
            // a burst of 503s during the cooldown window.
            ordered_ids.push(account.id.clone());
            if request_candidate_ids.contains(&account.id) {
                request_available_ids.insert(account.id.clone());
            }
            continue;
        }
        if in_cooldown {
            // Never spend admission probes on an unverified account while it
            // is cooling down. Only previously Responses-verified accounts get
            // the soft-cooldown fallback above.
            continue;
        }
        if !request_candidate_ids.contains(&account.id) {
            continue;
        }
        match state {
            None => unprobed_ids.push(account.id.clone()),
            Some(state) if state.status == "unprobed" => unprobed_ids.push(account.id.clone()),
            Some(state) if state.status == "available" => unprobed_ids.push(account.id.clone()),
            Some(state)
                if state.status == "failed"
                    && state
                        .retry_after
                        .is_none_or(|retry_after| retry_after <= now) =>
            {
                retryable_failed_ids.push(account.id.clone())
            }
            _ => {}
        }
    }

    // Fresh imports are checked before retrying accounts that just failed.
    // This prevents a large dead pool from starving newly imported live data.
    unprobed_ids.extend(retryable_failed_ids);
    ConfirmedGatewayPool {
        ordered_ids,
        request_available_ids,
        refill_probe_ids: unprobed_ids,
    }
}

fn load_confirmed_gateway_pool(
    storage: &Storage,
    gateway_candidates: &[(Account, Token)],
    request_candidate_ids: &HashSet<String>,
) -> Result<ConfirmedGatewayPool, String> {
    let ids = gateway_candidates
        .iter()
        .map(|(account, _)| account.id.clone())
        .collect::<Vec<_>>();
    let states = storage
        .list_adapter_credential_probe_states("codex", &ids)
        .map_err(|error| format!("list Codex probe states failed: {error}"))?;
    Ok(build_confirmed_gateway_pool(
        gateway_candidates,
        request_candidate_ids,
        &states,
        now_ts(),
        crate::gateway::is_account_in_cooldown,
    ))
}

fn probe_refill_candidates(
    credential_ids: Vec<String>,
    required_successes: usize,
    request_model: Option<&str>,
) {
    if credential_ids.is_empty() || required_successes == 0 {
        return;
    }
    // Scan concurrently until the missing slots are proven live or every
    // candidate is exhausted. Customer traffic never doubles as a probe.
    let queue = Arc::new(Mutex::new(VecDeque::from(credential_ids)));
    let successes = Arc::new(AtomicUsize::new(0));
    let request_model = Arc::new(request_model.map(str::to_string));
    let worker_count = AUTO_REFILL_PROBE_CONCURRENCY.min(
        queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len(),
    );
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = queue.clone();
            let successes = successes.clone();
            let request_model = request_model.clone();
            scope.spawn(move || loop {
                if successes.load(Ordering::Acquire) >= required_successes {
                    break;
                }
                let credential_id = queue
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .pop_front();
                let Some(credential_id) = credential_id else {
                    break;
                };
                if crate::usage_refresh::probe_codex_account_batch_admission(
                    &credential_id,
                    request_model.as_ref().as_deref(),
                )
                .is_ok()
                {
                    successes.fetch_add(1, Ordering::AcqRel);
                }
            });
        }
    });
}

fn schedule_confirmed_gateway_pool_refill_if_needed(
    storage: &Storage,
    request_candidate_ids: &HashSet<String>,
    batch_size: usize,
    request_model: Option<&str>,
    gateway_candidates: &[(Account, Token)],
) -> Result<ConfirmedGatewayPool, String> {
    let pool = load_confirmed_gateway_pool(storage, gateway_candidates, request_candidate_ids)?;
    let shortage = batch_size.saturating_sub(pool.request_available_ids.len());
    if shortage == 0 || pool.refill_probe_ids.is_empty() {
        return Ok(pool);
    }

    // When the current batch has no confirmed account at all, admission must
    // complete before routing the customer request.  Falling through to an
    // aggregate provider here can send traffic to a Cloudflare-blocked
    // fallback even though a freshly imported account is recoverable.  Probe
    // synchronously, then rebuild the pool from storage so the verified
    // account is immediately eligible for this request.
    if pool.request_available_ids.is_empty() {
        let credential_ids = pool.refill_probe_ids.clone();
        let request_model = request_model.map(str::to_owned);
        probe_refill_candidates(credential_ids, shortage, request_model.as_deref());
        return load_confirmed_gateway_pool(storage, gateway_candidates, request_candidate_ids);
    }

    // Do not turn a downstream request into an account probe.  In particular,
    // after a Cloudflare challenge the client should promptly try another
    // confirmed account / aggregate fallback while this worker rechecks the
    // affected pool in the background.
    if AUTO_REFILL_PROBE_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let credential_ids = pool.refill_probe_ids.clone();
        let request_model = request_model.map(str::to_owned);
        std::thread::spawn(move || {
            probe_refill_candidates(credential_ids, shortage, request_model.as_deref());
            AUTO_REFILL_PROBE_RUNNING.store(false, Ordering::Release);
        });
    }
    Ok(pool)
}

pub(crate) fn apply_account_batch_rotation(
    storage: &Storage,
    request_model: Option<&str>,
    _model_account_ids: Option<&HashSet<String>>,
    candidates: &mut Vec<(Account, Token)>,
) -> Result<(), String> {
    let config = account_batch_rotation_config();
    if !config.enabled {
        return Ok(());
    }
    // The cursor is provider-wide. Model, plan and key policy only intersect the chosen batch and
    // therefore cannot move the shared cursor or multiply the configured active account count.
    let scope = "codex";
    // The persistent batch pool is built from gateway-eligible accounts only.
    // unavailable/limited/disabled/banned accounts never consume batch slots;
    // once repaired or reset they are returned by this query and rejoin naturally.
    let gateway_candidates = storage
        .list_gateway_candidates()
        .map_err(|err| format!("list available account batch pool failed: {err}"))?;
    let request_candidate_ids = candidates
        .iter()
        .map(|(account, _)| account.id.clone())
        .collect::<HashSet<_>>();
    let confirmed_pool = schedule_confirmed_gateway_pool_refill_if_needed(
        storage,
        &request_candidate_ids,
        config.batch_size,
        request_model,
        &gateway_candidates,
    )?;
    let full_pool = confirmed_pool.ordered_ids;
    let globally_available_ids = confirmed_pool.request_available_ids;
    let snapshots = storage
        .latest_usage_snapshots_for_accounts(&full_pool)
        .map_err(|err| format!("list batch usage snapshots failed: {err}"))?
        .into_iter()
        .map(|snapshot| (snapshot.account_id.clone(), snapshot))
        .collect::<HashMap<_, _>>();

    let key = state_key(scope);
    let lock = STATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut states = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !states.contains_key(&key) {
        let loaded = storage
            .get_app_setting(&key)
            .map_err(|err| format!("load account batch state failed: {err}"))?
            .map(|raw| {
                serde_json::from_str::<AccountBatchState>(&raw)
                    .map_err(|err| format!("parse account batch state failed: {err}"))
            })
            .transpose()?
            .unwrap_or_default();
        states.insert(key.clone(), loaded);
    }
    let state = states.get_mut(&key).expect("batch state inserted");
    reconcile_available_pool(state, &full_pool, config.batch_size);
    let now = now_ts();
    update_batch_blocks(
        state,
        &globally_available_ids,
        &snapshots,
        config.batch_size,
        config.fallback_window_minutes,
        now,
    );
    let selected = select_batch(
        state,
        &full_pool,
        &globally_available_ids,
        config.batch_size,
        now,
    )
    .into_iter()
    .collect::<HashSet<_>>();
    candidates.retain(|(account, _)| selected.contains(&account.id));

    let batch_start = state.current_batch.saturating_mul(config.batch_size);
    let batch_end = (batch_start + config.batch_size).min(state.ordered_account_ids.len());
    let batch_ids = state
        .ordered_account_ids
        .get(batch_start..batch_end)
        .unwrap_or(&[]);
    state.earliest_reset_at = state
        .blocked_until_by_batch
        .get(&state.current_batch)
        .copied()
        .or_else(|| {
            batch_ids
                .iter()
                .filter_map(|id| account_quota_reset_at(snapshots.get(id), now))
                .min()
        });
    if selected.is_empty() {
        state.exhausted_at.get_or_insert(now);
    } else {
        state.exhausted_at = None;
    }
    state.current_batch_available = selected.len();
    state.updated_at = now;
    let raw = serde_json::to_string(state)
        .map_err(|err| format!("serialize account batch state failed: {err}"))?;
    storage
        .set_app_setting(&key, &raw, state.updated_at)
        .map_err(|err| format!("persist account batch state failed: {err}"))?;
    Ok(())
}

pub(crate) fn account_batch_rotation_status() -> AccountBatchRotationStatus {
    let config = account_batch_rotation_config();
    let states = STATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let latest = states.values().max_by_key(|state| state.updated_at);
    AccountBatchRotationStatus {
        enabled: config.enabled,
        batch_size: config.batch_size,
        fallback_window_minutes: config.fallback_window_minutes,
        max_attempts_per_request: config.max_attempts_per_request,
        scope: latest.map(|_| "codex".to_string()),
        current_batch: latest.map(|state| state.current_batch + 1).unwrap_or(0),
        total_batches: latest
            .map(|state| state.ordered_account_ids.len().div_ceil(config.batch_size))
            .unwrap_or(0),
        cycle: latest.map(|state| state.cycle).unwrap_or(0),
        current_batch_accounts: latest
            .map(|state| {
                let start = state.current_batch.saturating_mul(config.batch_size);
                state
                    .ordered_account_ids
                    .len()
                    .saturating_sub(start)
                    .min(config.batch_size)
            })
            .unwrap_or(0),
        current_batch_available: latest
            .map(|state| state.current_batch_available)
            .unwrap_or(0),
        earliest_reset_at: latest.and_then(|state| state.earliest_reset_at),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_confirmed_gateway_pool, reconcile_available_pool, select_batch, update_batch_blocks,
        AccountBatchRotationConfig, AccountBatchState, GATEWAY_PROBE_FRESHNESS_SECS,
    };
    use codexmanager_core::storage::{Account, AdapterCredentialProbeState, Token};
    use std::collections::{HashMap, HashSet};

    fn ids(range: std::ops::RangeInclusive<usize>) -> Vec<String> {
        range.map(|value| value.to_string()).collect()
    }

    fn candidate(id: &str) -> (Account, Token) {
        (
            Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "openai".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: 0,
                updated_at: 0,
            },
            Token {
                account_id: id.to_string(),
                id_token: String::new(),
                access_token: "access".to_string(),
                refresh_token: "refresh".to_string(),
                api_key_access_token: None,
                last_refresh: 0,
            },
        )
    }

    fn probe_state(
        id: &str,
        status: &str,
        retry_after: Option<i64>,
    ) -> AdapterCredentialProbeState {
        AdapterCredentialProbeState {
            pool_id: "codex".to_string(),
            credential_id: id.to_string(),
            status: status.to_string(),
            error_code: (status == "available")
                .then(|| crate::usage_refresh::CODEX_RESPONSES_VERIFIED.to_string()),
            checked_at: 90,
            retry_after,
        }
    }

    #[test]
    fn only_confirmed_available_accounts_enter_the_batch_pool() {
        let candidates = [
            "available",
            "unprobed",
            "failed-ready",
            "failed-wait",
            "quota",
        ]
        .into_iter()
        .map(candidate)
        .collect::<Vec<_>>();
        let request_ids = candidates
            .iter()
            .map(|(account, _)| account.id.clone())
            .collect::<HashSet<_>>();
        let states = vec![
            probe_state("available", "available", None),
            probe_state("unprobed", "unprobed", None),
            probe_state("failed-ready", "failed", Some(99)),
            probe_state("failed-wait", "failed", Some(101)),
            probe_state("quota", "unavailable", None),
        ];

        let pool = build_confirmed_gateway_pool(&candidates, &request_ids, &states, 100, |_| false);

        assert_eq!(pool.ordered_ids, vec!["available"]);
        assert_eq!(
            pool.request_available_ids,
            HashSet::from(["available".to_string()])
        );
        assert_eq!(pool.refill_probe_ids, vec!["unprobed", "failed-ready"]);
    }

    #[test]
    fn stale_available_account_must_be_reprobed_before_batch_admission() {
        let candidates = ["fresh", "stale"]
            .into_iter()
            .map(candidate)
            .collect::<Vec<_>>();
        let request_ids = HashSet::from(["fresh".to_string(), "stale".to_string()]);
        let mut fresh = probe_state("fresh", "available", None);
        fresh.checked_at = 1_000;
        let mut stale = probe_state("stale", "available", None);
        stale.checked_at = 1_000 - GATEWAY_PROBE_FRESHNESS_SECS - 1;

        let pool =
            build_confirmed_gateway_pool(&candidates, &request_ids, &[fresh, stale], 1_000, |_| {
                false
            });

        assert_eq!(pool.ordered_ids, vec!["fresh"]);
        assert_eq!(pool.refill_probe_ids, vec!["stale"]);
    }

    #[test]
    fn import_or_model_probe_success_still_requires_responses_admission() {
        let candidates = [candidate("model-only")];
        let request_ids = HashSet::from(["model-only".to_string()]);
        let mut state = probe_state("model-only", "available", None);
        state.error_code = Some("codex_models_verified".to_string());

        let pool =
            build_confirmed_gateway_pool(&candidates, &request_ids, &[state], 100, |_| false);

        assert!(pool.ordered_ids.is_empty());
        assert!(pool.request_available_ids.is_empty());
        assert_eq!(pool.refill_probe_ids, vec!["model-only"]);
    }

    #[test]
    fn fresh_responses_verified_account_remains_last_resort_during_cooldown() {
        let candidates = [candidate("verified")];
        let request_ids = HashSet::from(["verified".to_string()]);
        let state = probe_state("verified", "available", None);

        let pool = build_confirmed_gateway_pool(&candidates, &request_ids, &[state], 100, |_| true);

        assert_eq!(pool.ordered_ids, vec!["verified"]);
        assert_eq!(
            pool.request_available_ids,
            HashSet::from(["verified".to_string()])
        );
        assert!(pool.refill_probe_ids.is_empty());
    }

    #[test]
    fn unverified_account_in_cooldown_is_never_probed_or_admitted() {
        let candidates = [candidate("unverified")];
        let request_ids = HashSet::from(["unverified".to_string()]);
        let state = probe_state("unverified", "unprobed", None);

        let pool = build_confirmed_gateway_pool(&candidates, &request_ids, &[state], 100, |_| true);

        assert!(pool.ordered_ids.is_empty());
        assert!(pool.request_available_ids.is_empty());
        assert!(pool.refill_probe_ids.is_empty());
    }

    #[test]
    fn a_successful_second_probe_immediately_becomes_a_fill_candidate() {
        let candidates = ["first", "second"]
            .into_iter()
            .map(candidate)
            .collect::<Vec<_>>();
        let request_ids = HashSet::from(["first".to_string(), "second".to_string()]);
        let before = vec![
            probe_state("first", "available", None),
            probe_state("second", "unprobed", None),
        ];
        let before_pool =
            build_confirmed_gateway_pool(&candidates, &request_ids, &before, 100, |_| false);
        assert_eq!(before_pool.ordered_ids, vec!["first"]);
        assert_eq!(before_pool.refill_probe_ids, vec!["second"]);

        let after = vec![
            probe_state("first", "available", None),
            probe_state("second", "available", None),
        ];
        let after_pool =
            build_confirmed_gateway_pool(&candidates, &request_ids, &after, 101, |_| false);
        assert_eq!(after_pool.ordered_ids, vec!["first", "second"]);
        assert_eq!(after_pool.request_available_ids.len(), 2);
    }

    #[test]
    fn persisted_user_batch_size_is_not_replaced_by_the_installation_default() {
        let normalized = AccountBatchRotationConfig {
            enabled: true,
            batch_size: 2,
            fallback_window_minutes: 60,
            max_attempts_per_request: 4,
        }
        .normalized();
        assert_eq!(normalized.batch_size, 2);
    }

    #[test]
    fn legacy_rotation_config_gets_safe_request_attempt_budget() {
        let config: AccountBatchRotationConfig =
            serde_json::from_str(r#"{"enabled":true,"batchSize":2,"fallbackWindowMinutes":60}"#)
                .expect("deserialize legacy rotation config");
        assert_eq!(config.max_attempts_per_request, 4);
    }

    #[test]
    fn request_attempt_budget_is_user_configurable_and_bounded() {
        let normalized = AccountBatchRotationConfig {
            enabled: false,
            batch_size: 5,
            fallback_window_minutes: 300,
            max_attempts_per_request: 10_000,
        }
        .normalized();
        assert_eq!(normalized.max_attempts_per_request, 64);
    }

    #[test]
    fn backfills_unavailable_batch_slots_without_advancing_the_batch() {
        let pool = ids(1..=10);
        let mut state = AccountBatchState {
            ordered_account_ids: pool.clone(),
            effective_batch_size: 5,
            ..Default::default()
        };
        let available = ["2", "6", "7"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        assert_eq!(
            select_batch(&mut state, &pool, &available, 5, 100),
            vec!["2", "6", "7"]
        );
        assert_eq!(state.current_batch, 0);
    }

    #[test]
    fn backfills_to_the_configured_batch_size() {
        let pool = ids(1..=6);
        let mut state = AccountBatchState {
            ordered_account_ids: pool.clone(),
            effective_batch_size: 2,
            ..Default::default()
        };
        let available = ["1", "3", "4"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        assert_eq!(
            select_batch(&mut state, &pool, &available, 2, 100),
            vec!["1", "3"]
        );
        assert_eq!(state.current_batch, 0);
    }

    #[test]
    fn advances_forward_and_wraps() {
        let pool = ids(1..=12);
        let mut state = AccountBatchState {
            ordered_account_ids: pool.clone(),
            effective_batch_size: 5,
            ..Default::default()
        };
        let available = ["7"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        assert_eq!(
            select_batch(&mut state, &pool, &available, 5, 100),
            vec!["7"]
        );
        assert_eq!(state.current_batch, 1);
        let available = ["1"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        assert_eq!(
            select_batch(&mut state, &pool, &available, 5, 100),
            vec!["1"]
        );
        assert_eq!(state.current_batch, 0);
        assert_eq!(state.cycle, 1);
    }

    #[test]
    fn blocked_batch_is_skipped_until_reset() {
        let pool = ids(1..=10);
        let mut state = AccountBatchState {
            ordered_account_ids: pool.clone(),
            effective_batch_size: 5,
            blocked_until_by_batch: HashMap::from([(0, 500)]),
            ..Default::default()
        };
        let available = ["1", "6"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        assert_eq!(
            select_batch(&mut state, &pool, &available, 5, 100),
            vec!["6"]
        );
    }

    #[test]
    fn recovered_account_clears_stale_batch_block_immediately() {
        let pool = ids(1..=4);
        let mut state = AccountBatchState {
            ordered_account_ids: pool,
            effective_batch_size: 2,
            blocked_until_by_batch: HashMap::from([(1, 10_000)]),
            ..Default::default()
        };
        let available = ["3"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();

        update_batch_blocks(&mut state, &available, &HashMap::new(), 2, 300, 100);

        assert!(!state.blocked_until_by_batch.contains_key(&1));
        assert_eq!(
            select_batch(&mut state, &ids(1..=4), &available, 2, 100),
            vec!["3"]
        );
    }

    #[test]
    fn unavailable_accounts_are_removed_instead_of_consuming_batch_slots() {
        let mut state = AccountBatchState {
            ordered_account_ids: ids(1..=6),
            current_batch: 1,
            effective_batch_size: 2,
            blocked_until_by_batch: HashMap::from([(0, 500), (1, 500), (2, 500)]),
            ..Default::default()
        };

        reconcile_available_pool(&mut state, &["3".to_string(), "6".to_string()], 2);

        assert_eq!(state.ordered_account_ids, vec!["3", "6"]);
        assert_eq!(state.current_batch, 0);
        assert!(state.blocked_until_by_batch.is_empty());
        let available = ["3", "6"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        assert_eq!(
            select_batch(
                &mut state,
                &["3".to_string(), "6".to_string()],
                &available,
                2,
                100,
            ),
            vec!["3", "6"]
        );
    }

    #[test]
    fn batch_larger_than_available_pool_uses_every_available_account() {
        let pool = vec!["a".to_string(), "b".to_string()];
        let mut state = AccountBatchState::default();
        reconcile_available_pool(&mut state, &pool, 5);
        let available = pool.iter().cloned().collect::<HashSet<_>>();

        assert_eq!(select_batch(&mut state, &pool, &available, 5, 100), pool);
    }

    #[test]
    fn does_not_insert_new_accounts_mid_cycle() {
        let old_pool = ids(1..=10);
        let new_pool = ids(1..=11);
        let mut state = AccountBatchState {
            ordered_account_ids: old_pool,
            effective_batch_size: 5,
            ..Default::default()
        };
        let available = ["2", "11"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        assert_eq!(
            select_batch(&mut state, &new_pool, &available, 5, 100),
            vec!["2"]
        );
        assert_eq!(state.ordered_account_ids.len(), 10);
    }
}
