use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static ACCOUNT_INFLIGHT: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
static GATEWAY_TOTAL_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_ACTIVE_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_FAILOVER_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_COOLDOWN_MARKS: AtomicUsize = AtomicUsize::new(0);
static RPC_TOTAL_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static RPC_FAILED_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static RPC_REQUEST_DURATION_MS_TOTAL: AtomicU64 = AtomicU64::new(0);
static USAGE_REFRESH_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static USAGE_REFRESH_SUCCESSES: AtomicUsize = AtomicUsize::new(0);
static USAGE_REFRESH_FAILURES: AtomicUsize = AtomicUsize::new(0);
static USAGE_REFRESH_DURATION_MS_TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatewayMetricsSnapshot {
    pub total_requests: usize,
    pub active_requests: usize,
    pub account_inflight_total: usize,
    pub failover_attempts: usize,
    pub cooldown_marks: usize,
    pub rpc_total_requests: usize,
    pub rpc_failed_requests: usize,
    pub rpc_request_duration_ms_total: u64,
    pub usage_refresh_attempts: usize,
    pub usage_refresh_successes: usize,
    pub usage_refresh_failures: usize,
    pub usage_refresh_duration_ms_total: u64,
}

pub(crate) struct GatewayRequestGuard;
pub(crate) struct RpcRequestGuard {
    started_at: Instant,
    failed: bool,
}

impl Drop for GatewayRequestGuard {
    fn drop(&mut self) {
        GATEWAY_ACTIVE_REQUESTS.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Drop for RpcRequestGuard {
    fn drop(&mut self) {
        let duration_ms = duration_to_millis(self.started_at.elapsed());
        RPC_REQUEST_DURATION_MS_TOTAL.fetch_add(duration_ms, Ordering::Relaxed);
        if self.failed {
            RPC_FAILED_REQUESTS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl RpcRequestGuard {
    pub(crate) fn mark_success(&mut self) {
        self.failed = false;
    }
}

pub(crate) fn begin_gateway_request() -> GatewayRequestGuard {
    GATEWAY_TOTAL_REQUESTS.fetch_add(1, Ordering::Relaxed);
    GATEWAY_ACTIVE_REQUESTS.fetch_add(1, Ordering::Relaxed);
    GatewayRequestGuard
}

pub(crate) fn begin_rpc_request() -> RpcRequestGuard {
    RPC_TOTAL_REQUESTS.fetch_add(1, Ordering::Relaxed);
    RpcRequestGuard {
        started_at: Instant::now(),
        failed: true,
    }
}

pub(crate) fn record_gateway_failover_attempt() {
    GATEWAY_FAILOVER_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_gateway_cooldown_mark() {
    GATEWAY_COOLDOWN_MARKS.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn record_usage_refresh_outcome(success: bool, duration_ms: u64) {
    USAGE_REFRESH_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    if success {
        USAGE_REFRESH_SUCCESSES.fetch_add(1, Ordering::Relaxed);
    } else {
        USAGE_REFRESH_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    USAGE_REFRESH_DURATION_MS_TOTAL.fetch_add(duration_ms, Ordering::Relaxed);
}

pub(crate) fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn account_inflight_total() -> usize {
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(map) = lock.lock() else {
        return 0;
    };
    map.values().copied().sum()
}

pub(crate) fn gateway_metrics_snapshot() -> GatewayMetricsSnapshot {
    GatewayMetricsSnapshot {
        total_requests: GATEWAY_TOTAL_REQUESTS.load(Ordering::Relaxed),
        active_requests: GATEWAY_ACTIVE_REQUESTS.load(Ordering::Relaxed),
        account_inflight_total: account_inflight_total(),
        failover_attempts: GATEWAY_FAILOVER_ATTEMPTS.load(Ordering::Relaxed),
        cooldown_marks: GATEWAY_COOLDOWN_MARKS.load(Ordering::Relaxed),
        rpc_total_requests: RPC_TOTAL_REQUESTS.load(Ordering::Relaxed),
        rpc_failed_requests: RPC_FAILED_REQUESTS.load(Ordering::Relaxed),
        rpc_request_duration_ms_total: RPC_REQUEST_DURATION_MS_TOTAL.load(Ordering::Relaxed),
        usage_refresh_attempts: USAGE_REFRESH_ATTEMPTS.load(Ordering::Relaxed),
        usage_refresh_successes: USAGE_REFRESH_SUCCESSES.load(Ordering::Relaxed),
        usage_refresh_failures: USAGE_REFRESH_FAILURES.load(Ordering::Relaxed),
        usage_refresh_duration_ms_total: USAGE_REFRESH_DURATION_MS_TOTAL.load(Ordering::Relaxed),
    }
}

pub(crate) fn gateway_metrics_prometheus() -> String {
    let m = gateway_metrics_snapshot();
    format!(
        "gpttools_gateway_requests_total {}\n\
gpttools_gateway_requests_active {}\n\
gpttools_gateway_account_inflight_total {}\n\
gpttools_gateway_failover_attempts_total {}\n\
gpttools_gateway_cooldown_marks_total {}\n\
gpttools_rpc_requests_total {}\n\
gpttools_rpc_requests_failed_total {}\n\
gpttools_rpc_request_duration_milliseconds_total {}\n\
gpttools_rpc_request_duration_milliseconds_count {}\n\
gpttools_usage_refresh_attempts_total {}\n\
gpttools_usage_refresh_success_total {}\n\
gpttools_usage_refresh_failures_total {}\n\
gpttools_usage_refresh_duration_milliseconds_total {}\n\
gpttools_usage_refresh_duration_milliseconds_count {}\n",
        m.total_requests,
        m.active_requests,
        m.account_inflight_total,
        m.failover_attempts,
        m.cooldown_marks,
        m.rpc_total_requests,
        m.rpc_failed_requests,
        m.rpc_request_duration_ms_total,
        m.rpc_total_requests,
        m.usage_refresh_attempts,
        m.usage_refresh_successes,
        m.usage_refresh_failures,
        m.usage_refresh_duration_ms_total,
        m.usage_refresh_attempts,
    )
}

pub(crate) fn account_inflight_count(account_id: &str) -> usize {
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(map) = lock.lock() else {
        return 0;
    };
    map.get(account_id).copied().unwrap_or(0)
}

pub(crate) struct AccountInFlightGuard {
    account_id: String,
}

impl Drop for AccountInFlightGuard {
    fn drop(&mut self) {
        let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
        let Ok(mut map) = lock.lock() else {
            return;
        };
        if let Some(value) = map.get_mut(&self.account_id) {
            if *value > 1 {
                *value -= 1;
            } else {
                map.remove(&self.account_id);
            }
        }
    }
}

pub(crate) fn acquire_account_inflight(account_id: &str) -> AccountInFlightGuard {
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut map) = lock.lock() {
        let entry = map.entry(account_id.to_string()).or_insert(0);
        *entry += 1;
    }
    AccountInFlightGuard {
        account_id: account_id.to_string(),
    }
}
