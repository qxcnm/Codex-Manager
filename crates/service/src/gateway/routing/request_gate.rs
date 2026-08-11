use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use codexmanager_core::storage::now_ts;

const REQUEST_GATE_LOCK_TTL_SECS: i64 = 30 * 60;
const REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS: i64 = 60;

struct RequestGateLockEntry {
    lock: Arc<RequestGateLock>,
    last_seen_at: i64,
}

#[derive(Default)]
struct RequestGateLockTable {
    entries: HashMap<String, RequestGateLockEntry>,
    last_cleanup_at: i64,
}

static REQUEST_GATE_LOCKS: OnceLock<Mutex<RequestGateLockTable>> = OnceLock::new();

#[derive(Debug)]
pub(crate) enum RequestGateAcquireError {
    Poisoned,
}

#[derive(Default)]
struct RequestGateState {
    held: bool,
}

pub(crate) struct RequestGateLock {
    state: Mutex<RequestGateState>,
    available: Condvar,
}

impl RequestGateLock {
    /// 函数 `new`
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
    fn new() -> Self {
        Self {
            state: Mutex::new(RequestGateState::default()),
            available: Condvar::new(),
        }
    }

    /// 函数 `try_acquire`
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
    pub(crate) fn try_acquire(
        self: &Arc<Self>,
    ) -> Result<Option<RequestGateGuard>, RequestGateAcquireError> {
        let mut state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        if state.held {
            return Ok(None);
        }
        state.held = true;
        drop(state);
        Ok(Some(RequestGateGuard {
            lock: Arc::clone(self),
        }))
    }

    pub(crate) fn acquire(self: &Arc<Self>) -> Result<RequestGateGuard, RequestGateAcquireError> {
        let state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        let Ok(mut state) = self.available.wait_while(state, |state| state.held) else {
            log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait_while");
            return Err(RequestGateAcquireError::Poisoned);
        };
        state.held = true;
        drop(state);
        Ok(RequestGateGuard {
            lock: Arc::clone(self),
        })
    }

    /// 函数 `acquire_with_timeout`
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
    pub(crate) fn acquire_with_timeout(
        self: &Arc<Self>,
        timeout: Duration,
    ) -> Result<Option<RequestGateGuard>, RequestGateAcquireError> {
        let state = match self.state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait");
                return Err(RequestGateAcquireError::Poisoned);
            }
        };
        let wait_result = self
            .available
            .wait_timeout_while(state, timeout, |state| state.held);
        let Ok((mut state, _)) = wait_result else {
            log::warn!("event=lock_poisoned lock=request_gate_state action=skip_wait_timeout");
            return Err(RequestGateAcquireError::Poisoned);
        };
        if state.held {
            return Ok(None);
        }
        state.held = true;
        drop(state);
        Ok(Some(RequestGateGuard {
            lock: Arc::clone(self),
        }))
    }
}

pub(crate) struct RequestGateGuard {
    lock: Arc<RequestGateLock>,
}

impl Drop for RequestGateGuard {
    /// 函数 `drop`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 无
    fn drop(&mut self) {
        let mut state = match self.lock.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                log::warn!("event=lock_poisoned lock=request_gate_state action=recover_release");
                poisoned.into_inner()
            }
        };
        state.held = false;
        self.lock.available.notify_one();
    }
}

/// 函数 `gate_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - key_id: 参数 key_id
/// - path: 参数 path
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
fn gate_key(key_id: &str, path: &str, model: Option<&str>, request_scope: &str) -> String {
    format!(
        "{}|{}|{}|{}",
        key_id.trim(),
        path.trim(),
        model
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("-"),
        request_scope.trim()
    )
}

/// 函数 `request_gate_lock`
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
pub(crate) fn request_gate_lock(
    key_id: &str,
    path: &str,
    model: Option<&str>,
    request_scope: &str,
) -> Arc<RequestGateLock> {
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = crate::lock_utils::lock_recover(lock, "request_gate_locks");
    let now = now_ts();
    maybe_cleanup_request_gate_locks(&mut table, now);
    let entry = table
        .entries
        .entry(gate_key(key_id, path, model, request_scope))
        .or_insert_with(|| RequestGateLockEntry {
            lock: Arc::new(RequestGateLock::new()),
            last_seen_at: now,
        });
    entry.last_seen_at = now;
    entry.lock.clone()
}

/// 函数 `maybe_cleanup_request_gate_locks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - table: 参数 table
/// - now: 参数 now
///
/// # 返回
/// 无
fn maybe_cleanup_request_gate_locks(table: &mut RequestGateLockTable, now: i64) {
    if table.last_cleanup_at != 0
        && now.saturating_sub(table.last_cleanup_at) < REQUEST_GATE_LOCK_CLEANUP_INTERVAL_SECS
    {
        return;
    }
    table.last_cleanup_at = now;
    table.entries.retain(|_, entry| {
        let stale = now.saturating_sub(entry.last_seen_at) > REQUEST_GATE_LOCK_TTL_SECS;
        !stale || Arc::strong_count(&entry.lock) > 1
    });
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
    let lock = REQUEST_GATE_LOCKS.get_or_init(|| Mutex::new(RequestGateLockTable::default()));
    let mut table = crate::lock_utils::lock_recover(lock, "request_gate_locks");
    table.entries.clear();
    table.last_cleanup_at = 0;
}

/// 函数 `clear_request_gate_locks_for_tests`
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
fn clear_request_gate_locks_for_tests() {
    clear_runtime_state();
}

#[cfg(test)]
#[path = "tests/request_gate_tests.rs"]
mod tests;
