use super::*;
use std::thread;
use std::time::{Duration, Instant};

/// 函数 `same_scope_reuses_same_lock_instance`
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
#[test]
fn same_scope_reuses_same_lock_instance() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    assert!(Arc::ptr_eq(&first, &second));
}

/// 函数 `different_scope_uses_different_lock_instances`
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
#[test]
fn different_scope_uses_different_lock_instances() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    let second = request_gate_lock(
        "gk_1",
        "/v1/responses",
        Some("gpt-5.3-codex-high"),
        "conv-1",
    );
    assert!(!Arc::ptr_eq(&first, &second));

    let third = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-2");
    assert!(!Arc::ptr_eq(&first, &third));
}

/// 函数 `stale_unshared_lock_entry_is_reclaimed`
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
#[test]
fn stale_unshared_lock_entry_is_reclaimed() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let key = gate_key("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    let weak = Arc::downgrade(&first);
    drop(first);

    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = lock.lock().expect("request gate table lock");
    let now = now_ts();
    table
        .entries
        .get_mut(&key)
        .expect("request gate entry")
        .last_seen_at = now - REQUEST_GATE_LOCK_TTL_SECS - 1;
    table.last_cleanup_at = now - REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS - 1;
    drop(table);

    let _second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    assert!(weak.upgrade().is_none());
}

/// 函数 `stale_shared_lock_entry_is_not_reclaimed`
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
#[test]
fn stale_shared_lock_entry_is_not_reclaimed() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let key = gate_key("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");

    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = lock.lock().expect("request gate table lock");
    let now = now_ts();
    table
        .entries
        .get_mut(&key)
        .expect("request gate entry")
        .last_seen_at = now - REQUEST_GATE_LOCK_TTL_SECS - 1;
    table.last_cleanup_at = now - REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS - 1;
    drop(table);

    let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"), "conv-1");
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn acquire_waits_until_previous_guard_released() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let lock = request_gate_lock(
        "gk_wait",
        "/v1/responses",
        Some("gpt-5.3-codex"),
        "conv-wait",
    );
    let first_guard = lock
        .try_acquire()
        .expect("lock should not be poisoned")
        .expect("first guard");
    let waiter = lock.clone();

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        let guard = waiter.acquire().expect("waiter acquires after release");
        let waited = started_at.elapsed();
        drop(guard);
        waited
    });

    thread::sleep(Duration::from_millis(60));
    drop(first_guard);

    let waited = handle.join().expect("join waiter thread");
    assert!(
        waited >= Duration::from_millis(40),
        "expected waiter to block, actual wait: {waited:?}"
    );
}
