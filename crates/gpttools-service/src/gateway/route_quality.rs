use gpttools_core::storage::now_ts;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Default)]
struct RouteQualityRecord {
    success_2xx: u32,
    challenge_403: u32,
    throttle_429: u32,
    updated_at: i64,
}

static ROUTE_QUALITY: OnceLock<Mutex<HashMap<String, RouteQualityRecord>>> = OnceLock::new();
const ROUTE_QUALITY_TTL_SECS: i64 = 24 * 60 * 60;

fn with_map_mut<F>(mutator: F)
where
    F: FnOnce(&mut HashMap<String, RouteQualityRecord>),
{
    let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut map) = lock.lock() else {
        return;
    };
    let now = now_ts();
    map.retain(|_, value| value.updated_at + ROUTE_QUALITY_TTL_SECS > now);
    mutator(&mut map);
}

pub(crate) fn record_route_quality(account_id: &str, status_code: u16) {
    with_map_mut(|map| {
        let now = now_ts();
        let record = map.entry(account_id.to_string()).or_default();
        record.updated_at = now;
        if (200..300).contains(&status_code) {
            record.success_2xx = record.success_2xx.saturating_add(1);
            return;
        }
        if status_code == 403 {
            record.challenge_403 = record.challenge_403.saturating_add(1);
            return;
        }
        if status_code == 429 {
            record.throttle_429 = record.throttle_429.saturating_add(1);
        }
    });
}

pub(crate) fn route_quality_penalty(account_id: &str) -> i64 {
    let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(map) = lock.lock() else {
        return 0;
    };
    let Some(record) = map.get(account_id) else {
        return 0;
    };
    i64::from(record.challenge_403) * 6 + i64::from(record.throttle_429) * 3
        - i64::from(record.success_2xx) * 2
}

#[cfg(test)]
pub(crate) fn clear_route_quality_for_tests() {
    let lock = ROUTE_QUALITY.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_quality_penalty_prefers_successful_accounts() {
        clear_route_quality_for_tests();
        record_route_quality("acc_a", 403);
        record_route_quality("acc_a", 403);
        record_route_quality("acc_b", 200);
        assert!(route_quality_penalty("acc_a") > route_quality_penalty("acc_b"));
    }
}
