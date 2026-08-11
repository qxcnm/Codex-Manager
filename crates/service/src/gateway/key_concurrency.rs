use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn inflight() -> &'static Mutex<HashMap<String, usize>> {
    static INFLIGHT: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) struct ApiKeyInFlightGuard {
    key_id: String,
}

impl Drop for ApiKeyInFlightGuard {
    fn drop(&mut self) {
        let mut counts = crate::lock_utils::lock_recover(inflight(), "api_key_inflight");
        if let Some(count) = counts.get_mut(&self.key_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&self.key_id);
            }
        }
    }
}

pub(crate) fn try_acquire_api_key_inflight(
    key_id: &str,
    limit: Option<i64>,
) -> Result<ApiKeyInFlightGuard, usize> {
    let mut counts = crate::lock_utils::lock_recover(inflight(), "api_key_inflight");
    let current = counts.get(key_id).copied().unwrap_or(0);
    if limit.is_some_and(|limit| limit > 0 && current >= limit as usize) {
        return Err(current);
    }
    counts.insert(key_id.to_string(), current.saturating_add(1));
    Ok(ApiKeyInFlightGuard {
        key_id: key_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_key_limit_is_atomic_and_released_on_drop() {
        let first = try_acquire_api_key_inflight("limited-key", Some(1)).expect("first");
        assert_eq!(
            try_acquire_api_key_inflight("limited-key", Some(1)).err(),
            Some(1)
        );
        assert!(try_acquire_api_key_inflight("other-key", Some(1)).is_ok());
        drop(first);
        assert!(try_acquire_api_key_inflight("limited-key", Some(1)).is_ok());
    }
}
