use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

static REQUEST_GATE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

fn gate_key(key_id: &str, path: &str, model: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        key_id.trim(),
        path.trim(),
        model.map(str::trim).filter(|v| !v.is_empty()).unwrap_or("-")
    )
}

pub(crate) fn request_gate_lock(key_id: &str, path: &str, model: Option<&str>) -> Arc<Mutex<()>> {
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut map) = lock.lock() else {
        return Arc::new(Mutex::new(()));
    };
    map.entry(gate_key(key_id, path, model))
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

#[cfg(test)]
fn clear_request_gate_locks_for_tests() {
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_scope_reuses_same_lock_instance() {
        clear_request_gate_locks_for_tests();
        let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn different_scope_uses_different_lock_instances() {
        clear_request_gate_locks_for_tests();
        let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
        let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex-high"));
        assert!(!Arc::ptr_eq(&first, &second));
    }
}
