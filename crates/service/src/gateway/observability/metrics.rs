use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

static ACCOUNT_INFLIGHT: OnceLock<Mutex<AccountInflightState>> = OnceLock::new();
const MAX_ACCOUNT_INFLIGHT_WAITERS: usize = 4096;

#[derive(Default)]
struct AccountInflightState {
    counts: HashMap<String, usize>,
    waiters: VecDeque<Arc<AccountInflightWaiter>>,
    next_ticket: u64,
}

struct AccountInflightWaiter {
    ticket: u64,
    account_ids: Vec<String>,
    limit: usize,
    grant: Mutex<Option<String>>,
    available: Condvar,
}

fn first_available_account(
    state: &AccountInflightState,
    account_ids: &[String],
    limit: usize,
) -> Option<String> {
    account_ids
        .iter()
        .find(|account_id| {
            state
                .counts
                .get(account_id.as_str())
                .copied()
                .unwrap_or_default()
                < limit
        })
        .cloned()
}

/// Reserves available slots for the oldest satisfiable waiters. A waiter whose
/// candidates are all busy does not block an older-independent account from
/// serving the next waiter, so this is FIFO without cross-account HOL stalls.
fn dispatch_account_waiters_locked(
    state: &mut AccountInflightState,
) -> Vec<(Arc<AccountInflightWaiter>, String)> {
    let mut grants = Vec::new();
    let mut index = 0;
    while index < state.waiters.len() {
        let waiter = state.waiters[index].clone();
        let Some(account_id) =
            first_available_account(state, waiter.account_ids.as_slice(), waiter.limit)
        else {
            index += 1;
            continue;
        };
        *state.counts.entry(account_id.clone()).or_insert(0) += 1;
        state.waiters.remove(index);
        grants.push((waiter, account_id));
    }
    grants
}

fn deliver_account_waiter_grants(grants: Vec<(Arc<AccountInflightWaiter>, String)>) {
    for (waiter, account_id) in grants {
        let mut grant = crate::lock_utils::lock_recover(&waiter.grant, "account_inflight_grant");
        *grant = Some(account_id);
        drop(grant);
        waiter.available.notify_one();
    }
}
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
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(AccountInflightState::default()));
    let state = crate::lock_utils::lock_recover(lock, "account_inflight");
    state.counts.values().copied().sum()
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
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(AccountInflightState::default()));
    let state = crate::lock_utils::lock_recover(lock, "account_inflight");
    state.counts.get(account_id).copied().unwrap_or(0)
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
        let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(AccountInflightState::default()));
        let mut state = crate::lock_utils::lock_recover(lock, "account_inflight");
        if let Some(value) = state.counts.get_mut(&self.account_id) {
            if *value > 1 {
                *value -= 1;
            } else {
                state.counts.remove(&self.account_id);
            }
        }
        let grants = dispatch_account_waiters_locked(&mut state);
        drop(state);
        deliver_account_waiter_grants(grants);
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
    try_acquire_account_inflight(account_id, 0)
        .expect("unlimited account inflight acquisition must succeed")
}

/// Atomically reserves an in-flight slot for one upstream account.
///
/// A zero limit preserves the historical unlimited behavior. Keeping the
/// check and increment under the same mutex prevents concurrent platform keys
/// from racing past the per-account limit.
pub(crate) fn try_acquire_account_inflight(
    account_id: &str,
    limit: usize,
) -> Option<AccountInFlightGuard> {
    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(AccountInflightState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "account_inflight");
    let entry = state.counts.entry(account_id.to_string()).or_insert(0);
    if limit > 0 && *entry >= limit {
        return None;
    }
    *entry += 1;
    Some(AccountInFlightGuard {
        account_id: account_id.to_string(),
    })
}

/// Waits until one of the ordered candidate accounts has an available slot and
/// atomically reserves it. This keeps a burst of client sessions queued locally
/// instead of forwarding unlimited concurrent long streams through one account.
pub(crate) fn wait_acquire_candidate_account_inflight(
    account_ids: &[String],
    limit: usize,
    timeout: Option<Duration>,
) -> Option<(String, AccountInFlightGuard)> {
    if account_ids.is_empty() {
        return None;
    }
    if limit == 0 {
        let account_id = account_ids[0].clone();
        return try_acquire_account_inflight(account_id.as_str(), 0)
            .map(|guard| (account_id, guard));
    }

    let lock = ACCOUNT_INFLIGHT.get_or_init(|| Mutex::new(AccountInflightState::default()));
    let mut state = crate::lock_utils::lock_recover(lock, "account_inflight");
    if state.waiters.is_empty() {
        if let Some(account_id) = first_available_account(&state, account_ids, limit) {
            *state.counts.entry(account_id.clone()).or_insert(0) += 1;
            return Some((account_id.clone(), AccountInFlightGuard { account_id }));
        }
    }
    if state.waiters.len() >= MAX_ACCOUNT_INFLIGHT_WAITERS {
        // Bound local memory under an upstream outage. Callers treat this as no
        // slot available and return through the existing deadline/error path.
        return None;
    }

    let ticket = state.next_ticket;
    state.next_ticket = state.next_ticket.wrapping_add(1);
    let waiter = Arc::new(AccountInflightWaiter {
        ticket,
        account_ids: account_ids.to_vec(),
        limit,
        grant: Mutex::new(None),
        available: Condvar::new(),
    });
    state.waiters.push_back(waiter.clone());
    let grants = dispatch_account_waiters_locked(&mut state);
    drop(state);
    deliver_account_waiter_grants(grants);

    let started_at = Instant::now();
    let mut grant = crate::lock_utils::lock_recover(&waiter.grant, "account_inflight_grant");
    loop {
        if let Some(account_id) = grant.take() {
            return Some((account_id.clone(), AccountInFlightGuard { account_id }));
        }
        grant = match timeout {
            Some(timeout) => {
                let Some(remaining) = timeout.checked_sub(started_at.elapsed()) else {
                    break;
                };
                if remaining.is_zero() {
                    break;
                }
                match waiter.available.wait_timeout(grant, remaining) {
                    Ok((next, result)) => {
                        if result.timed_out() {
                            grant = next;
                            break;
                        }
                        next
                    }
                    Err(poisoned) => poisoned.into_inner().0,
                }
            }
            None => match waiter.available.wait(grant) {
                Ok(next) => next,
                Err(poisoned) => poisoned.into_inner(),
            },
        };
    }
    drop(grant);

    let mut state = crate::lock_utils::lock_recover(lock, "account_inflight");
    if let Some(index) = state
        .waiters
        .iter()
        .position(|queued| queued.ticket == ticket)
    {
        state.waiters.remove(index);
        return None;
    }
    drop(state);

    // Dispatcher has already reserved the slot and removed this ticket. Wait
    // for its handoff rather than leaking the reservation at the timeout race.
    let mut grant = crate::lock_utils::lock_recover(&waiter.grant, "account_inflight_grant");
    loop {
        if let Some(account_id) = grant.take() {
            return Some((account_id.clone(), AccountInFlightGuard { account_id }));
        }
        grant = match waiter.available.wait(grant) {
            Ok(next) => next,
            Err(poisoned) => poisoned.into_inner(),
        };
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

#[cfg(test)]
mod account_inflight_limit_tests {
    use super::{
        account_inflight_count, try_acquire_account_inflight,
        wait_acquire_candidate_account_inflight,
    };
    use std::time::Duration;
    use std::{sync::mpsc, thread};

    fn queued_waiters_for(account_id: &str) -> usize {
        let lock = super::ACCOUNT_INFLIGHT
            .get_or_init(|| std::sync::Mutex::new(super::AccountInflightState::default()));
        let state = crate::lock_utils::lock_recover(lock, "account_inflight_test");
        state
            .waiters
            .iter()
            .filter(|waiter| waiter.account_ids.iter().any(|id| id == account_id))
            .count()
    }

    #[test]
    fn account_limit_is_reserved_atomically_and_released_by_guard() {
        let account_id = "account-inflight-strict-limit-test";
        let first = try_acquire_account_inflight(account_id, 1).expect("first slot");
        assert_eq!(account_inflight_count(account_id), 1);
        assert!(try_acquire_account_inflight(account_id, 1).is_none());
        drop(first);
        assert_eq!(account_inflight_count(account_id), 0);
        assert!(try_acquire_account_inflight(account_id, 1).is_some());
    }

    #[test]
    fn zero_account_limit_preserves_unlimited_behavior() {
        let account_id = "account-inflight-unlimited-test";
        let first = try_acquire_account_inflight(account_id, 0).expect("first slot");
        let second = try_acquire_account_inflight(account_id, 0).expect("second slot");
        assert_eq!(account_inflight_count(account_id), 2);
        drop((first, second));
        assert_eq!(account_inflight_count(account_id), 0);
    }

    #[test]
    fn candidate_slot_wait_prefers_the_first_idle_account() {
        let busy_id = "account-slot-wait-busy";
        let idle_id = "account-slot-wait-idle";
        let busy = try_acquire_account_inflight(busy_id, 1).expect("busy slot");

        let (selected, selected_guard) = wait_acquire_candidate_account_inflight(
            &[busy_id.to_string(), idle_id.to_string()],
            1,
            Some(Duration::from_millis(10)),
        )
        .expect("idle slot");

        assert_eq!(selected, idle_id);
        drop((busy, selected_guard));
    }

    #[test]
    fn candidate_slot_wait_times_out_when_every_account_is_busy() {
        let account_id = "account-slot-wait-timeout";
        let busy = try_acquire_account_inflight(account_id, 1).expect("busy slot");

        let actual = wait_acquire_candidate_account_inflight(
            &[account_id.to_string()],
            1,
            Some(Duration::from_millis(10)),
        );

        assert!(actual.is_none());
        assert_eq!(queued_waiters_for(account_id), 0);
        drop(busy);
    }

    #[test]
    fn blocked_account_does_not_head_of_line_block_independent_account() {
        let blocked_id = "account-slot-no-hol-blocked";
        let idle_id = "account-slot-no-hol-idle";
        let busy = try_acquire_account_inflight(blocked_id, 1).expect("busy slot");
        let (ready_tx, ready_rx) = mpsc::channel();

        let old_waiter = thread::spawn(move || {
            ready_tx.send(()).expect("announce waiter");
            let (_, guard) = wait_acquire_candidate_account_inflight(
                &[blocked_id.to_string()],
                1,
                Some(Duration::from_secs(2)),
            )
            .expect("blocked waiter eventually gets slot");
            drop(guard);
        });
        ready_rx.recv().expect("waiter started");
        for _ in 0..100 {
            if queued_waiters_for(blocked_id) == 1 {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(queued_waiters_for(blocked_id), 1);

        let started = std::time::Instant::now();
        let (selected, idle_guard) = wait_acquire_candidate_account_inflight(
            &[idle_id.to_string()],
            1,
            Some(Duration::from_millis(200)),
        )
        .expect("independent idle account must not wait behind blocked account");
        assert_eq!(selected, idle_id);
        assert!(started.elapsed() < Duration::from_millis(100));

        drop(idle_guard);
        drop(busy);
        old_waiter.join().expect("join blocked waiter");
        assert_eq!(account_inflight_count(blocked_id), 0);
        assert_eq!(account_inflight_count(idle_id), 0);
    }

    #[test]
    fn candidate_slot_wait_hands_off_in_fifo_order_without_waking_every_waiter() {
        let account_id = "account-slot-wait-fifo";
        let busy = try_acquire_account_inflight(account_id, 1).expect("busy slot");
        let (tx, rx) = mpsc::channel();

        let first_tx = tx.clone();
        let first = thread::spawn(move || {
            let (_, guard) = wait_acquire_candidate_account_inflight(
                &[account_id.to_string()],
                1,
                Some(Duration::from_secs(2)),
            )
            .expect("first waiter slot");
            first_tx.send((1, guard)).expect("send first grant");
        });
        thread::sleep(Duration::from_millis(25));
        let second = thread::spawn(move || {
            let (_, guard) = wait_acquire_candidate_account_inflight(
                &[account_id.to_string()],
                1,
                Some(Duration::from_secs(2)),
            )
            .expect("second waiter slot");
            tx.send((2, guard)).expect("send second grant");
        });
        thread::sleep(Duration::from_millis(25));

        drop(busy);
        let (first_order, first_guard) = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first grant arrives");
        assert_eq!(first_order, 1);
        assert!(
            rx.recv_timeout(Duration::from_millis(40)).is_err(),
            "second waiter must remain asleep while the first owns the slot"
        );

        drop(first_guard);
        let (second_order, second_guard) = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("second grant arrives after release");
        assert_eq!(second_order, 2);
        drop(second_guard);
        first.join().expect("join first waiter");
        second.join().expect("join second waiter");
        assert_eq!(account_inflight_count(account_id), 0);
    }
}
