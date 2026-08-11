use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use chrono::DateTime;
use codexmanager_core::storage::now_ts;
use reqwest::header::HeaderMap;

const DEFAULT_ACCOUNT_COOLDOWN_SECS: i64 = 20;
const DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
const DEFAULT_ACCOUNT_COOLDOWN_429_SECS: i64 = 45;
const DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS: i64 = 30;
const DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
// A Cloudflare/security challenge is normally tied to the current egress path.
// Do not hammer it every few seconds or sweep the whole account pool through it.
// Match the production behavior used by the mature CPA gateway: a security
// challenge is a real egress warning, not a normal transient 4xx. Keep the
// account away from customer traffic for ten minutes and lengthen repeated
// challenges instead of retrying the same account every minute.
const DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS: i64 = 600;
const DEFAULT_ACCOUNT_COOLDOWN_ANTHROPIC_CHALLENGE_SECS: i64 = 1200;
const ACCOUNT_RATE_LIMIT_COOLDOWN_LADDER_SECS: [i64; 4] =
    [DEFAULT_ACCOUNT_COOLDOWN_429_SECS, 300, 1800, 7200];
// 中文注释：offense 只用于“短时间内持续 429”场景；超过该时间视为新一轮，避免长期记仇导致误伤。
const ACCOUNT_RATE_LIMIT_OFFENSE_FORGET_AFTER_SECS: i64 = 30 * 60;
const ACCOUNT_CHALLENGE_COOLDOWN_LADDER_SECS: [i64; 3] = [600, 1800, 7200];
const ACCOUNT_CHALLENGE_OFFENSE_FORGET_AFTER_SECS: i64 = 3 * 60 * 60;

const ACCOUNT_COOLDOWN_CLEANUP_INTERVAL_SECS: i64 = 30;
const MAX_UPSTREAM_RATE_LIMIT_WINDOW_SECS: i64 = 7 * 24 * 60 * 60;

#[derive(Default)]
struct AccountCooldownState {
    entries: HashMap<String, i64>,
    rate_limited_accounts: HashSet<String>,
    offense_counts: HashMap<String, u32>,
    offense_last_at: HashMap<String, i64>,
    challenge_counts: HashMap<String, u32>,
    challenge_last_at: HashMap<String, i64>,
    last_cleanup_at: i64,
}

static ACCOUNT_COOLDOWN_UNTIL: OnceLock<Mutex<AccountCooldownState>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CooldownReason {
    Default,
    Network,
    RateLimited,
    Upstream5xx,
    Upstream4xx,
    Challenge,
    AnthropicChallenge,
}

/// 函数 `cooldown_secs_for_reason`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - reason: 参数 reason
///
/// # 返回
/// 返回函数执行结果
fn cooldown_secs_for_reason(reason: CooldownReason) -> i64 {
    match reason {
        CooldownReason::Default => DEFAULT_ACCOUNT_COOLDOWN_SECS,
        CooldownReason::Network => DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS,
        CooldownReason::RateLimited => DEFAULT_ACCOUNT_COOLDOWN_429_SECS,
        CooldownReason::Upstream5xx => DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS,
        CooldownReason::Upstream4xx => DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS,
        CooldownReason::Challenge => DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS,
        CooldownReason::AnthropicChallenge => DEFAULT_ACCOUNT_COOLDOWN_ANTHROPIC_CHALLENGE_SECS,
    }
}

/// 函数 `rate_limit_cooldown_secs_for_offense`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - offense_count: 参数 offense_count
///
/// # 返回
/// 返回函数执行结果
fn rate_limit_cooldown_secs_for_offense(offense_count: u32) -> i64 {
    let idx = offense_count
        .saturating_sub(1)
        .min((ACCOUNT_RATE_LIMIT_COOLDOWN_LADDER_SECS.len() - 1) as u32) as usize;
    ACCOUNT_RATE_LIMIT_COOLDOWN_LADDER_SECS[idx]
}

/// 函数 `cooldown_secs_for_mark`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - offense_counts: 参数 offense_counts
/// - offense_last_at: 参数 offense_last_at
/// - account_id: 参数 account_id
/// - reason: 参数 reason
/// - now: 参数 now
///
/// # 返回
/// 返回函数执行结果
fn cooldown_secs_for_mark(
    offense_counts: &mut HashMap<String, u32>,
    offense_last_at: &mut HashMap<String, i64>,
    account_id: &str,
    reason: CooldownReason,
    now: i64,
) -> i64 {
    match reason {
        CooldownReason::RateLimited => {
            if let Some(last) = offense_last_at.get(account_id).copied() {
                if now.saturating_sub(last) > ACCOUNT_RATE_LIMIT_OFFENSE_FORGET_AFTER_SECS {
                    offense_counts.remove(account_id);
                }
            }
            let offense_count = offense_counts
                .entry(account_id.to_string())
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
            offense_last_at.insert(account_id.to_string(), now);
            rate_limit_cooldown_secs_for_offense(*offense_count)
        }
        _ => cooldown_secs_for_reason(reason),
    }
}

fn challenge_cooldown_secs_for_mark(
    counts: &mut HashMap<String, u32>,
    last_at: &mut HashMap<String, i64>,
    account_id: &str,
    now: i64,
) -> i64 {
    if last_at
        .get(account_id)
        .is_some_and(|last| now.saturating_sub(*last) > ACCOUNT_CHALLENGE_OFFENSE_FORGET_AFTER_SECS)
    {
        counts.remove(account_id);
    }
    let count = counts
        .entry(account_id.to_string())
        .and_modify(|value| *value = value.saturating_add(1))
        .or_insert(1);
    last_at.insert(account_id.to_string(), now);
    let index = count
        .saturating_sub(1)
        .min((ACCOUNT_CHALLENGE_COOLDOWN_LADDER_SECS.len() - 1) as u32) as usize;
    ACCOUNT_CHALLENGE_COOLDOWN_LADDER_SECS[index]
}

/// 函数 `decay_offense_count_for_success`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - offense_counts: 参数 offense_counts
/// - offense_last_at: 参数 offense_last_at
/// - account_id: 参数 account_id
///
/// # 返回
/// 无
fn decay_offense_count_for_success(
    offense_counts: &mut HashMap<String, u32>,
    offense_last_at: &mut HashMap<String, i64>,
    account_id: &str,
) {
    let mut should_remove = false;
    if let Some(count) = offense_counts.get_mut(account_id) {
        if *count <= 1 {
            should_remove = true;
        } else {
            *count -= 1;
        }
    }
    if should_remove {
        offense_counts.remove(account_id);
        offense_last_at.remove(account_id);
    }
}

/// 函数 `cooldown_reason_for_status`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn cooldown_reason_for_status(status: u16) -> CooldownReason {
    match status {
        429 => CooldownReason::RateLimited,
        500..=599 => CooldownReason::Upstream5xx,
        401 | 403 => CooldownReason::Challenge,
        400..=499 => CooldownReason::Upstream4xx,
        _ => CooldownReason::Default,
    }
}

/// 函数 `is_account_in_cooldown`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn is_account_in_cooldown(account_id: &str) -> bool {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "account_cooldown_until");
    let now = now_ts();
    match state.entries.get(account_id).copied() {
        Some(until) if until > now => true,
        Some(_) => {
            state.entries.remove(account_id);
            state.rate_limited_accounts.remove(account_id);
            false
        }
        None => false,
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AccountCooldownInfo {
    pub until: i64,
    pub rate_limited: bool,
}

pub(crate) fn account_cooldown_info(account_id: &str) -> Option<AccountCooldownInfo> {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "account_cooldown_until");
    let now = now_ts();
    let until = state.entries.get(account_id).copied()?;
    if until <= now {
        state.entries.remove(account_id);
        state.rate_limited_accounts.remove(account_id);
        return None;
    }
    Some(AccountCooldownInfo {
        until,
        rate_limited: state.rate_limited_accounts.contains(account_id),
    })
}

/// 函数 `mark_account_cooldown`
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
pub(super) fn mark_account_cooldown(account_id: &str, reason: CooldownReason) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut guard = crate::lock_utils::lock_recover(lock, "account_cooldown_until");
    let state = &mut *guard;
    super::record_gateway_cooldown_mark();
    let now = now_ts();
    maybe_cleanup_expired_cooldowns(state, now);
    let cooldown_until = now
        + if reason == CooldownReason::Challenge {
            challenge_cooldown_secs_for_mark(
                &mut state.challenge_counts,
                &mut state.challenge_last_at,
                account_id,
                now,
            )
        } else {
            cooldown_secs_for_mark(
                &mut state.offense_counts,
                &mut state.offense_last_at,
                account_id,
                reason,
                now,
            )
        };
    if reason == CooldownReason::RateLimited {
        state.rate_limited_accounts.insert(account_id.to_string());
    }
    // 中文注释：同账号短时间内可能触发不同失败类型；保留更晚的 until 可避免被较短冷却覆盖。
    match state.entries.get_mut(account_id) {
        Some(until) => {
            if cooldown_until > *until {
                *until = cooldown_until;
            }
        }
        None => {
            state.entries.insert(account_id.to_string(), cooldown_until);
        }
    }
}

fn parse_duration_seconds(value: &str) -> Option<i64> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    if let Ok(seconds) = value.parse::<f64>() {
        return Some(seconds.ceil().max(0.0) as i64);
    }

    let bytes = value.as_bytes();
    let mut index = 0;
    let mut total = 0_f64;
    let mut parsed_any = false;
    while index < bytes.len() {
        let number_start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        if number_start == index {
            return None;
        }
        let number = value[number_start..index].parse::<f64>().ok()?;
        let unit_start = index;
        while index < bytes.len() && bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let multiplier = match &value[unit_start..index] {
            "ms" => 0.001,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 60.0 * 60.0,
            "d" => 24.0 * 60.0 * 60.0,
            _ => return None,
        };
        total += number * multiplier;
        parsed_any = true;
    }
    parsed_any.then_some(total.ceil().max(0.0) as i64)
}

fn parse_retry_after_value(value: &str, now: i64) -> Option<i64> {
    if let Some(seconds) = parse_duration_seconds(value) {
        return Some(now.saturating_add(seconds));
    }
    DateTime::parse_from_rfc2822(value.trim())
        .ok()
        .map(|date| date.timestamp())
}

fn parse_reset_value(value: &str, now: i64) -> Option<i64> {
    let value = value.trim();
    if let Ok(raw) = value.parse::<f64>() {
        let rounded = raw.ceil().max(0.0) as i64;
        return Some(if rounded >= 1_000_000_000 {
            rounded
        } else {
            now.saturating_add(rounded)
        });
    }
    parse_duration_seconds(value).map(|seconds| now.saturating_add(seconds))
}

pub(super) fn rate_limit_reset_at_from_headers(headers: &HeaderMap, now: i64) -> Option<i64> {
    let mut candidates = Vec::new();
    if let Some(value) = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse_retry_after_value(value, now))
    {
        candidates.push(value);
    }
    for name in [
        "x-ratelimit-reset-requests",
        "x-ratelimit-reset-tokens",
        "x-ratelimit-reset",
        "x-ratelimit-reset-after",
    ] {
        if let Some(value) = headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| parse_reset_value(value, now))
        {
            candidates.push(value);
        }
    }
    candidates
        .into_iter()
        .filter(|until| *until > now)
        .map(|until| until.min(now.saturating_add(MAX_UPSTREAM_RATE_LIMIT_WINDOW_SECS)))
        .max()
}

pub(super) fn mark_account_rate_limited_until(account_id: &str, until: i64) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "account_cooldown_until");
    let now = now_ts();
    if until <= now {
        return;
    }
    maybe_cleanup_expired_cooldowns(&mut state, now);
    super::record_gateway_cooldown_mark();
    state.rate_limited_accounts.insert(account_id.to_string());
    state
        .entries
        .entry(account_id.to_string())
        .and_modify(|current| *current = (*current).max(until))
        .or_insert(until);
}

/// 函数 `mark_account_cooldown_for_status`
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
pub(super) fn mark_account_cooldown_for_status(account_id: &str, status: u16) {
    mark_account_cooldown(account_id, cooldown_reason_for_status(status));
}

/// 函数 `clear_account_cooldown`
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
pub(super) fn clear_account_cooldown(account_id: &str) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut guard = crate::lock_utils::lock_recover(lock, "account_cooldown_until");
    let state = &mut *guard;
    state.entries.remove(account_id);
    state.rate_limited_accounts.remove(account_id);
    decay_offense_count_for_success(
        &mut state.offense_counts,
        &mut state.offense_last_at,
        account_id,
    );
    decay_offense_count_for_success(
        &mut state.challenge_counts,
        &mut state.challenge_last_at,
        account_id,
    );
}

/// 函数 `maybe_cleanup_expired_cooldowns`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - state: 参数 state
/// - now: 参数 now
///
/// # 返回
/// 无
fn maybe_cleanup_expired_cooldowns(state: &mut AccountCooldownState, now: i64) {
    if state.last_cleanup_at != 0
        && now.saturating_sub(state.last_cleanup_at) < ACCOUNT_COOLDOWN_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    state.last_cleanup_at = now;
    state.entries.retain(|_, until| *until > now);
    state
        .rate_limited_accounts
        .retain(|account_id| state.entries.contains_key(account_id));
    let mut stale_offenses = Vec::new();
    for (account_id, last) in state.offense_last_at.iter() {
        if now.saturating_sub(*last) > ACCOUNT_RATE_LIMIT_OFFENSE_FORGET_AFTER_SECS {
            stale_offenses.push(account_id.clone());
        }
    }
    for account_id in stale_offenses {
        state.offense_last_at.remove(&account_id);
        state.offense_counts.remove(&account_id);
    }
    let mut stale_challenges = Vec::new();
    for (account_id, last) in state.challenge_last_at.iter() {
        if now.saturating_sub(*last) > ACCOUNT_CHALLENGE_OFFENSE_FORGET_AFTER_SECS {
            stale_challenges.push(account_id.clone());
        }
    }
    for account_id in stale_challenges {
        state.challenge_last_at.remove(&account_id);
        state.challenge_counts.remove(&account_id);
    }
}

/// 函数 `clear_runtime_state`
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
pub(super) fn clear_runtime_state() {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(AccountCooldownState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "account_cooldown_until");
    state.entries.clear();
    state.offense_counts.clear();
    state.offense_last_at.clear();
    state.challenge_counts.clear();
    state.challenge_last_at.clear();
    state.last_cleanup_at = 0;
}

/// 函数 `clear_account_cooldown_for_tests`
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
fn clear_account_cooldown_for_tests() {
    clear_runtime_state();
}

#[cfg(test)]
#[path = "tests/cooldown_tests.rs"]
mod tests;
