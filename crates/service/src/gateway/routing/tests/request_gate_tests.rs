use super::*;
use std::thread;
use std::time::{Duration, Instant};

#[tokio::test(flavor = "current_thread")]
async fn async_waiters_release_and_cancel_without_blocking_runtime() {
    let lock = Arc::new(RequestGateLock::new());
    let first = lock.try_acquire().unwrap().unwrap();
    let cancelled_lock = Arc::clone(&lock);
    let cancelled = tokio::spawn(async move { cancelled_lock.acquire_async().await });
    tokio::task::yield_now().await;
    cancelled.abort();
    assert!(matches!(cancelled.await, Err(error) if error.is_cancelled()));
    let waiting_lock = Arc::clone(&lock);
    let waiting = tokio::spawn(async move { waiting_lock.acquire_async().await });
    tokio::task::yield_now().await;
    assert!(!waiting.is_finished());
    drop(first);
    let next = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .expect("waiter woken")
        .unwrap()
        .unwrap();
    assert!(lock.try_acquire().unwrap().is_none());
    drop(next);
    assert!(lock.try_acquire().unwrap().is_some());
}

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
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
    let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
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
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
    let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex-high"));
    assert!(!Arc::ptr_eq(&first, &second));
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
    let key = gate_key("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
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

    let _second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
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
    let key = gate_key("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
    let first = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));

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

    let second = request_gate_lock("gk_1", "/v1/responses", Some("gpt-5.3-codex"));
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn acquire_waits_until_previous_guard_released() {
    let _guard = crate::test_env_guard();
    clear_request_gate_locks_for_tests();
    let lock = request_gate_lock("gk_wait", "/v1/responses", Some("gpt-5.3-codex"));
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
