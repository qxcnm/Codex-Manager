use gpttools_core::storage::now_ts;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const ROUTE_HINT_TTL_SECS: i64 = 30 * 60;

#[derive(Debug, Clone)]
struct RouteHintRecord {
    account_id: String,
    expires_at: i64,
}

static ROUTE_HINTS: OnceLock<Mutex<HashMap<String, RouteHintRecord>>> = OnceLock::new();

fn hint_key(key_id: &str, path: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        key_id.trim(),
        path.trim(),
        model.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("-")
    )
}

pub(crate) fn preferred_route_account(
    key_id: &str,
    path: &str,
    model: Option<&str>,
) -> Option<String> {
    let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut map) = lock.lock() else {
        return None;
    };
    let now = now_ts();
    map.retain(|_, value| value.expires_at > now);
    let key = hint_key(key_id, path, model);
    map.get(&key).map(|value| value.account_id.clone())
}

pub(crate) fn remember_success_route_account(
    key_id: &str,
    path: &str,
    model: Option<&str>,
    account_id: &str,
) {
    let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut map) = lock.lock() else {
        return;
    };
    let key = hint_key(key_id, path, model);
    map.insert(
        key,
        RouteHintRecord {
            account_id: account_id.to_string(),
            expires_at: now_ts() + ROUTE_HINT_TTL_SECS,
        },
    );
}

#[cfg(test)]
pub(crate) fn clear_route_hints_for_tests() {
    let lock = ROUTE_HINTS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_route_account_returns_last_successful_account() {
        clear_route_hints_for_tests();
        assert_eq!(
            preferred_route_account("gk_1", "/v1/responses", Some("gpt-5.3-codex")),
            None
        );
        remember_success_route_account(
            "gk_1",
            "/v1/responses",
            Some("gpt-5.3-codex"),
            "acc_2",
        );
        assert_eq!(
            preferred_route_account("gk_1", "/v1/responses", Some("gpt-5.3-codex"))
                .as_deref(),
            Some("acc_2")
        );
    }
}
