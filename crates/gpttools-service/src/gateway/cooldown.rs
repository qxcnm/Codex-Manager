use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gpttools_core::storage::now_ts;

const DEFAULT_ACCOUNT_COOLDOWN_SECS: i64 = 20;
const DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
const DEFAULT_ACCOUNT_COOLDOWN_429_SECS: i64 = 45;
const DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS: i64 = 30;
const DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS: i64 = DEFAULT_ACCOUNT_COOLDOWN_SECS;
const DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS: i64 = 6;

static ACCOUNT_COOLDOWN_UNTIL: OnceLock<Mutex<HashMap<String, i64>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CooldownReason {
    Default,
    Network,
    RateLimited,
    Upstream5xx,
    Upstream4xx,
    Challenge,
}

fn cooldown_secs_for_reason(reason: CooldownReason) -> i64 {
    match reason {
        CooldownReason::Default => DEFAULT_ACCOUNT_COOLDOWN_SECS,
        CooldownReason::Network => DEFAULT_ACCOUNT_COOLDOWN_NETWORK_SECS,
        CooldownReason::RateLimited => DEFAULT_ACCOUNT_COOLDOWN_429_SECS,
        CooldownReason::Upstream5xx => DEFAULT_ACCOUNT_COOLDOWN_5XX_SECS,
        CooldownReason::Upstream4xx => DEFAULT_ACCOUNT_COOLDOWN_4XX_SECS,
        CooldownReason::Challenge => DEFAULT_ACCOUNT_COOLDOWN_CHALLENGE_SECS,
    }
}

pub(super) fn cooldown_reason_for_status(status: u16) -> CooldownReason {
    match status {
        429 => CooldownReason::RateLimited,
        500..=599 => CooldownReason::Upstream5xx,
        401 | 403 => CooldownReason::Challenge,
        400..=499 => CooldownReason::Upstream4xx,
        _ => CooldownReason::Default,
    }
}

pub(super) fn is_account_in_cooldown(account_id: &str) -> bool {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut map) = lock.lock() else {
        return false;
    };
    let now = now_ts();
    map.retain(|_, until| *until > now);
    map.get(account_id).copied().unwrap_or(0) > now
}

pub(super) fn mark_account_cooldown(account_id: &str, reason: CooldownReason) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        super::record_gateway_cooldown_mark();
        let cooldown_until = now_ts() + cooldown_secs_for_reason(reason);
        // 中文注释：同账号短时间内可能触发不同失败类型；保留更晚的 until 可避免被较短冷却覆盖。
        match map.get_mut(account_id) {
            Some(until) => {
                if cooldown_until > *until {
                    *until = cooldown_until;
                }
            }
            None => {
                map.insert(account_id.to_string(), cooldown_until);
            }
        }
    }
}

pub(super) fn mark_account_cooldown_for_status(account_id: &str, status: u16) {
    mark_account_cooldown(account_id, cooldown_reason_for_status(status));
}

pub(super) fn clear_account_cooldown(account_id: &str) {
    let lock = ACCOUNT_COOLDOWN_UNTIL.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        map.remove(account_id);
    }
}
