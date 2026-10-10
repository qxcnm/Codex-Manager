use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static ACCOUNT_INFLIGHT: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
static GATEWAY_REQUEST_LABELS: OnceLock<Mutex<HashMap<GatewayRequestLabelKey, usize>>> =
    OnceLock::new();
static GATEWAY_TOTAL_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_ACTIVE_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_FAILOVER_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_CANDIDATE_SKIPS_TOTAL: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_CANDIDATE_SKIP_COOLDOWN_TOTAL: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_CANDIDATE_SKIP_INFLIGHT_TOTAL: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_COOLDOWN_MARKS: AtomicUsize = AtomicUsize::new(0);
static RPC_TOTAL_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static RPC_FAILED_REQUESTS: AtomicUsize = AtomicUsize::new(0);
static RPC_REQUEST_DURATION_MS_TOTAL: AtomicU64 = AtomicU64::new(0);
static USAGE_REFRESH_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static USAGE_REFRESH_SUCCESSES: AtomicUsize = AtomicUsize::new(0);
static USAGE_REFRESH_FAILURES: AtomicUsize = AtomicUsize::new(0);
static USAGE_REFRESH_DURATION_MS_TOTAL: AtomicU64 = AtomicU64::new(0);
static DB_ERRORS_TOTAL: AtomicUsize = AtomicUsize::new(0);
static DB_BUSY_TOTAL: AtomicUsize = AtomicUsize::new(0);
static HTTP_QUEUE_CAPACITY: AtomicUsize = AtomicUsize::new(0);
static HTTP_QUEUE_DEPTH: AtomicUsize = AtomicUsize::new(0);
static HTTP_STREAM_QUEUE_CAPACITY: AtomicUsize = AtomicUsize::new(0);
static HTTP_STREAM_QUEUE_DEPTH: AtomicUsize = AtomicUsize::new(0);
static HTTP_QUEUE_ENQUEUE_FAILURES: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_UPSTREAM_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_UPSTREAM_ATTEMPT_ERRORS: AtomicUsize = AtomicUsize::new(0);
static GATEWAY_UPSTREAM_ATTEMPT_DURATION_MS_TOTAL: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GatewayRequestLabelKey {
    route: &'static str,
    status_class: &'static str,
    protocol: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GatewayCandidateSkipReason {
    Cooldown,
    Inflight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GatewayMetricsSnapshot {
    pub total_requests: usize,
    pub active_requests: usize,
    pub account_inflight_total: usize,
    pub failover_attempts: usize,
    pub candidate_skips_total: usize,
    pub candidate_skip_cooldown_total: usize,
    pub candidate_skip_inflight_total: usize,
    pub cooldown_marks: usize,
    pub rpc_total_requests: usize,
    pub rpc_failed_requests: usize,
    pub rpc_request_duration_ms_total: u64,
    pub usage_refresh_attempts: usize,
    pub usage_refresh_successes: usize,
    pub usage_refresh_failures: usize,
    pub usage_refresh_duration_ms_total: u64,
    pub db_errors_total: usize,
    pub db_busy_total: usize,
    pub http_queue_capacity: usize,
    pub http_queue_depth: usize,
    pub http_stream_queue_capacity: usize,
    pub http_stream_queue_depth: usize,
    pub http_queue_enqueue_failures: usize,
    pub gateway_upstream_attempt_duration_ms_total: u64,
    pub gateway_upstream_attempts: usize,
    pub gateway_upstream_attempt_errors: usize,
}

pub(crate) struct GatewayRequestGuard;
pub(crate) struct RpcRequestGuard {
    started_at: Instant,
    failed: bool,
}

impl Drop for GatewayRequestGuard {
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
        GATEWAY_ACTIVE_REQUESTS.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Drop for RpcRequestGuard {
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
        let duration_ms = duration_to_millis(self.started_at.elapsed());
        RPC_REQUEST_DURATION_MS_TOTAL.fetch_add(duration_ms, Ordering::Relaxed);
        if self.failed {
            RPC_FAILED_REQUESTS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl RpcRequestGuard {
    /// 函数 `mark_success`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - crate: 参数 crate
    ///
    /// # 返回
    /// 无
    pub(crate) fn mark_success(&mut self) {
        self.failed = false;
    }
}

/// 函数 `begin_gateway_request`
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
pub(crate) fn begin_gateway_request() -> GatewayRequestGuard {
    GATEWAY_TOTAL_REQUESTS.fetch_add(1, Ordering::Relaxed);
    GATEWAY_ACTIVE_REQUESTS.fetch_add(1, Ordering::Relaxed);
    GatewayRequestGuard
}

/// 函数 `begin_rpc_request`
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
pub(crate) fn begin_rpc_request() -> RpcRequestGuard {
    RPC_TOTAL_REQUESTS.fetch_add(1, Ordering::Relaxed);
    RpcRequestGuard {
        started_at: Instant::now(),
        failed: true,
    }
}

/// 函数 `record_gateway_failover_attempt`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_gateway_failover_attempt() {
    GATEWAY_FAILOVER_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
}

/// 函数 `record_gateway_candidate_skip`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-13
///
/// # 参数
/// - reason: 参数 reason
///
/// # 返回
/// 无
pub(crate) fn record_gateway_candidate_skip(reason: GatewayCandidateSkipReason) {
    GATEWAY_CANDIDATE_SKIPS_TOTAL.fetch_add(1, Ordering::Relaxed);
    match reason {
        GatewayCandidateSkipReason::Cooldown => {
            GATEWAY_CANDIDATE_SKIP_COOLDOWN_TOTAL.fetch_add(1, Ordering::Relaxed);
        }
        GatewayCandidateSkipReason::Inflight => {
            GATEWAY_CANDIDATE_SKIP_INFLIGHT_TOTAL.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// 函数 `record_gateway_cooldown_mark`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_gateway_cooldown_mark() {
    GATEWAY_COOLDOWN_MARKS.fetch_add(1, Ordering::Relaxed);
}

/// 函数 `record_usage_refresh_outcome`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_usage_refresh_outcome(success: bool, duration_ms: u64) {
    USAGE_REFRESH_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    if success {
        USAGE_REFRESH_SUCCESSES.fetch_add(1, Ordering::Relaxed);
    } else {
        USAGE_REFRESH_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    USAGE_REFRESH_DURATION_MS_TOTAL.fetch_add(duration_ms, Ordering::Relaxed);
}

/// 函数 `record_db_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_db_error(err: &str) {
    DB_ERRORS_TOTAL.fetch_add(1, Ordering::Relaxed);
    if is_db_busy_error(err) {
        DB_BUSY_TOTAL.fetch_add(1, Ordering::Relaxed);
    }
}

/// 函数 `record_http_queue_capacity`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
#[cfg(test)]
pub(crate) fn record_http_queue_capacity(normal_capacity: usize, stream_capacity: usize) {
    HTTP_QUEUE_CAPACITY.store(normal_capacity, Ordering::Relaxed);
    HTTP_STREAM_QUEUE_CAPACITY.store(stream_capacity, Ordering::Relaxed);
}

/// 函数 `record_http_queue_enqueue`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
#[cfg(test)]
pub(crate) fn record_http_queue_enqueue(is_stream_queue: bool) {
    if is_stream_queue {
        HTTP_STREAM_QUEUE_DEPTH.fetch_add(1, Ordering::Relaxed);
    } else {
        HTTP_QUEUE_DEPTH.fetch_add(1, Ordering::Relaxed);
    }
}

/// 函数 `record_http_queue_dequeue`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
#[cfg(test)]
pub(crate) fn record_http_queue_dequeue(is_stream_queue: bool) {
    if is_stream_queue {
        atomic_dec_saturating(&HTTP_STREAM_QUEUE_DEPTH);
    } else {
        atomic_dec_saturating(&HTTP_QUEUE_DEPTH);
    }
}

/// 函数 `record_http_queue_enqueue_failure`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
#[cfg(test)]
pub(crate) fn record_http_queue_enqueue_failure() {
    HTTP_QUEUE_ENQUEUE_FAILURES.fetch_add(1, Ordering::Relaxed);
}

/// 函数 `record_gateway_upstream_attempt`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_gateway_upstream_attempt(duration_ms: u64, failed: bool) {
    GATEWAY_UPSTREAM_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
    GATEWAY_UPSTREAM_ATTEMPT_DURATION_MS_TOTAL.fetch_add(duration_ms, Ordering::Relaxed);
    if failed {
        GATEWAY_UPSTREAM_ATTEMPT_ERRORS.fetch_add(1, Ordering::Relaxed);
    }
}

/// 函数 `record_gateway_request_outcome`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn record_gateway_request_outcome(
    path: &str,
    status_code: u16,
    protocol_type: Option<&str>,
) {
    let key = GatewayRequestLabelKey {
        route: classify_gateway_route(path),
        status_class: classify_status_class(status_code),
        protocol: classify_protocol(protocol_type),
    };
    let lock = GATEWAY_REQUEST_LABELS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = crate::lock_utils::lock_recover(lock, "gateway_request_labels");
    let entry = map.entry(key).or_insert(0);
    *entry += 1;
}

/// 函数 `duration_to_millis`
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
pub(crate) fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

/// 函数 `account_inflight_total`
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
fn account_inflight_total() -> usize {
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    let map = crate::lock_utils::lock_recover(lock, "account_inflight");
    map.values().copied().sum()
}

/// 函数 `gateway_metrics_snapshot`
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
pub(crate) fn gateway_metrics_snapshot() -> GatewayMetricsSnapshot {
    GatewayMetricsSnapshot {
        total_requests: GATEWAY_TOTAL_REQUESTS.load(Ordering::Relaxed),
        active_requests: GATEWAY_ACTIVE_REQUESTS.load(Ordering::Relaxed),
        account_inflight_total: account_inflight_total(),
        failover_attempts: GATEWAY_FAILOVER_ATTEMPTS.load(Ordering::Relaxed),
        candidate_skips_total: GATEWAY_CANDIDATE_SKIPS_TOTAL.load(Ordering::Relaxed),
        candidate_skip_cooldown_total: GATEWAY_CANDIDATE_SKIP_COOLDOWN_TOTAL
            .load(Ordering::Relaxed),
        candidate_skip_inflight_total: GATEWAY_CANDIDATE_SKIP_INFLIGHT_TOTAL
            .load(Ordering::Relaxed),
        cooldown_marks: GATEWAY_COOLDOWN_MARKS.load(Ordering::Relaxed),
        rpc_total_requests: RPC_TOTAL_REQUESTS.load(Ordering::Relaxed),
        rpc_failed_requests: RPC_FAILED_REQUESTS.load(Ordering::Relaxed),
        rpc_request_duration_ms_total: RPC_REQUEST_DURATION_MS_TOTAL.load(Ordering::Relaxed),
        usage_refresh_attempts: USAGE_REFRESH_ATTEMPTS.load(Ordering::Relaxed),
        usage_refresh_successes: USAGE_REFRESH_SUCCESSES.load(Ordering::Relaxed),
        usage_refresh_failures: USAGE_REFRESH_FAILURES.load(Ordering::Relaxed),
        usage_refresh_duration_ms_total: USAGE_REFRESH_DURATION_MS_TOTAL.load(Ordering::Relaxed),
        db_errors_total: DB_ERRORS_TOTAL.load(Ordering::Relaxed),
        db_busy_total: DB_BUSY_TOTAL.load(Ordering::Relaxed),
        http_queue_capacity: HTTP_QUEUE_CAPACITY.load(Ordering::Relaxed),
        http_queue_depth: HTTP_QUEUE_DEPTH.load(Ordering::Relaxed),
        http_stream_queue_capacity: HTTP_STREAM_QUEUE_CAPACITY.load(Ordering::Relaxed),
        http_stream_queue_depth: HTTP_STREAM_QUEUE_DEPTH.load(Ordering::Relaxed),
        http_queue_enqueue_failures: HTTP_QUEUE_ENQUEUE_FAILURES.load(Ordering::Relaxed),
        gateway_upstream_attempt_duration_ms_total: GATEWAY_UPSTREAM_ATTEMPT_DURATION_MS_TOTAL
            .load(Ordering::Relaxed),
        gateway_upstream_attempts: GATEWAY_UPSTREAM_ATTEMPTS.load(Ordering::Relaxed),
        gateway_upstream_attempt_errors: GATEWAY_UPSTREAM_ATTEMPT_ERRORS.load(Ordering::Relaxed),
    }
}

/// 函数 `gateway_metrics_prometheus`
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
pub(crate) fn gateway_metrics_prometheus() -> String {
    let m = gateway_metrics_snapshot();
    let labeled = gateway_labeled_metrics_prometheus();
    format!(
        "codexmanager_gateway_requests_total {}\n\
codexmanager_gateway_requests_active {}\n\
codexmanager_gateway_account_inflight_total {}\n\
codexmanager_gateway_failover_attempts_total {}\n\
codexmanager_gateway_candidate_skips_total {}\n\
codexmanager_gateway_candidate_skips_by_reason_total{{reason=\"cooldown\"}} {}\n\
codexmanager_gateway_candidate_skips_by_reason_total{{reason=\"inflight\"}} {}\n\
codexmanager_gateway_cooldown_marks_total {}\n\
codexmanager_rpc_requests_total {}\n\
codexmanager_rpc_requests_failed_total {}\n\
codexmanager_rpc_request_duration_milliseconds_total {}\n\
codexmanager_rpc_request_duration_milliseconds_count {}\n\
codexmanager_usage_refresh_attempts_total {}\n\
codexmanager_usage_refresh_success_total {}\n\
codexmanager_usage_refresh_failures_total {}\n\
codexmanager_usage_refresh_duration_milliseconds_total {}\n\
codexmanager_usage_refresh_duration_milliseconds_count {}\n\
codexmanager_db_errors_total {}\n\
codexmanager_db_busy_total {}\n\
codexmanager_http_queue_capacity {}\n\
codexmanager_http_queue_depth {}\n\
codexmanager_http_stream_queue_capacity {}\n\
codexmanager_http_stream_queue_depth {}\n\
codexmanager_http_queue_enqueue_failures_total {}\n\
codexmanager_gateway_upstream_attempt_duration_milliseconds_total {}\n\
codexmanager_gateway_upstream_attempt_duration_milliseconds_count {}\n\
codexmanager_gateway_upstream_attempt_errors_total {}\n\
{}",
        m.total_requests,
        m.active_requests,
        m.account_inflight_total,
        m.failover_attempts,
        m.candidate_skips_total,
        m.candidate_skip_cooldown_total,
        m.candidate_skip_inflight_total,
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
        m.db_errors_total,
        m.db_busy_total,
        m.http_queue_capacity,
        m.http_queue_depth,
        m.http_stream_queue_capacity,
        m.http_stream_queue_depth,
        m.http_queue_enqueue_failures,
        m.gateway_upstream_attempt_duration_ms_total,
        m.gateway_upstream_attempts,
        m.gateway_upstream_attempt_errors,
        labeled,
    )
}

/// 函数 `gateway_labeled_metrics_prometheus`
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
fn gateway_labeled_metrics_prometheus() -> String {
    let lock = GATEWAY_REQUEST_LABELS.get_or_init(|| Mutex::new(HashMap::new()));
    let map = crate::lock_utils::lock_recover(lock, "gateway_request_labels");
    let mut entries = map.iter().map(|(k, v)| (*k, *v)).collect::<Vec<_>>();
    entries.sort_by_key(|(k, _)| (k.route, k.status_class, k.protocol));
    let mut text = String::new();
    for (key, value) in entries {
        let line = format!(
            "codexmanager_gateway_requests_labeled_total{{route=\"{}\",status_class=\"{}\",protocol=\"{}\"}} {}\n",
            key.route, key.status_class, key.protocol, value
        );
        text.push_str(&line);
    }
    text
}

/// 函数 `classify_gateway_route`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn classify_gateway_route(path: &str) -> &'static str {
    let path = path.split('?').next().unwrap_or(path);
    if path.starts_with("/v1/responses") {
        return "responses";
    }
    if path.starts_with("/v1/chat/completions") {
        return "chat_completions";
    }
    if path.starts_with("/v1/messages/count_tokens") {
        return "messages_count_tokens";
    }
    if path.starts_with("/v1/messages") {
        return "messages";
    }
    if path.starts_with("/v1/models") {
        return "models";
    }
    if path.starts_with("/v1/embeddings") {
        return "embeddings";
    }
    if path == "/health" {
        return "health";
    }
    "other"
}

/// 函数 `classify_status_class`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status_code: 参数 status_code
///
/// # 返回
/// 返回函数执行结果
fn classify_status_class(status_code: u16) -> &'static str {
    match status_code {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
}

/// 函数 `classify_protocol`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - protocol_type: 参数 protocol_type
///
/// # 返回
/// 返回函数执行结果
fn classify_protocol(protocol_type: Option<&str>) -> &'static str {
    let Some(protocol_type) = protocol_type.map(str::trim).filter(|v| !v.is_empty()) else {
        return "unknown";
    };
    if protocol_type.eq_ignore_ascii_case("openai_compat")
        || protocol_type.eq_ignore_ascii_case("openai")
    {
        return "openai_compat";
    }
    if protocol_type.eq_ignore_ascii_case("anthropic_native")
        || protocol_type.eq_ignore_ascii_case("anthropic")
    {
        return "anthropic_native";
    }
    if protocol_type.eq_ignore_ascii_case("gemini_native")
        || protocol_type.eq_ignore_ascii_case("gemini")
    {
        return "gemini_native";
    }
    "other"
}

/// 函数 `account_inflight_count`
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
pub(crate) fn account_inflight_count(account_id: &str) -> usize {
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    let map = crate::lock_utils::lock_recover(lock, "account_inflight");
    map.get(account_id).copied().unwrap_or(0)
}

pub(crate) struct AccountInFlightGuard {
    account_id: String,
}

impl Drop for AccountInFlightGuard {
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
        let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
        let mut map = crate::lock_utils::lock_recover(lock, "account_inflight");
        if let Some(value) = map.get_mut(&self.account_id) {
            if *value > 1 {
                *value -= 1;
            } else {
                map.remove(&self.account_id);
            }
        }
    }
}

/// 函数 `acquire_account_inflight`
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
pub(crate) fn acquire_account_inflight(account_id: &str) -> AccountInFlightGuard {
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = crate::lock_utils::lock_recover(lock, "account_inflight");
    let entry = map.entry(account_id.to_string()).or_insert(0);
    *entry += 1;
    AccountInFlightGuard {
        account_id: account_id.to_string(),
    }
}

/// 函数 `atomic_dec_saturating`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 无
#[cfg(test)]
fn atomic_dec_saturating(value: &AtomicUsize) {
    let mut current = value.load(Ordering::Relaxed);
    loop {
        if current == 0 {
            break;
        }
        match value.compare_exchange_weak(
            current,
            current - 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(next) => current = next,
        }
    }
}

/// 函数 `is_db_busy_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
fn is_db_busy_error(err: &str) -> bool {
    let normalized = err.trim().to_ascii_lowercase();
    normalized.contains("database is locked")
        || normalized.contains("sqlite_busy")
        || normalized.contains("busy timeout")
}
