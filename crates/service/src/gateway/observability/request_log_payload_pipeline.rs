//! Request payload write queue: memory budget, spill hand-off and stats.
//!
//! Hot path ([`PipelineShared::submit`]): atomics, one short IO-free mutex
//! section (route + push), `Bytes` reference-count clones and condvar
//! notifications. No database access, no file IO, no storage pool, no
//! logging, no thread spawning, no error returned to the caller, and panics
//! are caught by the caller in `request_log_payload.rs`. Every thread is
//! started at service startup; a writer that failed to spawn is recreated by
//! the supervisor thread, never by the hot path.

use super::clear;
use super::RequestLogPayloadJob;
use codexmanager_core::request_log_spill::budget::{
    parse_cgroup_memory_limit, resolve_queue_budget, spill_handoff_budget, QueueBudget,
    QueueBudgetSource, MIB,
};
use codexmanager_core::request_log_spill::order::{DropReason, RouteDecision, SpillOrderState};
use codexmanager_core::request_log_spill::segment::{
    SpillOpenError, SpillPos, SpillStore, SEGMENT_MAX_BYTES, SPILL_DIR_NAME,
};
use codexmanager_core::storage::Storage;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

/// Group commit limits: jobs, bytes and accumulated write-lock time.
pub(super) const BATCH_MAX_JOBS: usize = 64;
pub(super) const BATCH_MAX_BYTES: u64 = 8 * MIB;
pub(super) const BATCH_MAX_HOLD: Duration = Duration::from_millis(50);
const RECENT_DROPS_CAPACITY: usize = 10_000;
/// Fixed per-job overhead added to the body bytes (strings, structs).
const JOB_OVERHEAD_BYTES: u64 = 256;
pub(crate) const ENV_QUEUE_MAX_BYTES: &str = "CODEXMANAGER_REQUEST_LOG_PAYLOAD_QUEUE_MAX_BYTES";
const WRITER_RESPAWN_INTERVAL: Duration = Duration::from_secs(10);
const SUPERVISOR_TICK: Duration = Duration::from_secs(1);
/// Captures dropped because the queue was never started (seaorm, or a
/// process that serves traffic without `start_server`).
static UNSTARTED_DROPS: AtomicU64 = AtomicU64::new(0);

pub(super) type StorageRef = Box<dyn Deref<Target = Storage>>;
pub(super) type OpenStorageFn = Arc<dyn Fn() -> Option<StorageRef> + Send + Sync>;
pub(super) type HookFn = Arc<dyn Fn() + Send + Sync>;
pub(super) type JobHookFn = Arc<dyn Fn(&RequestLogPayloadJob) + Send + Sync>;

#[derive(Clone)]
pub(super) struct PipelineHooks {
    pub open: OpenStorageFn,
    /// Called by the writer before each batch (tests pause the writer).
    pub before_write: Option<HookFn>,
    /// Called by a preprocessing worker before each job (tests inject panics).
    pub before_preprocess: Option<JobHookFn>,
    /// Called by the spill thread before appending a job (tests inject panics).
    pub before_spill: Option<JobHookFn>,
}

impl PipelineHooks {
    pub(super) fn production() -> Self {
        Self {
            open: Arc::new(|| {
                crate::storage_helpers::open_storage()
                    .map(|storage| Box::new(storage) as StorageRef)
            }),
            before_write: None,
            before_preprocess: None,
            before_spill: None,
        }
    }
}

pub(super) enum SpillSetup {
    /// Spilling is impossible; over-budget jobs are dropped with the reason.
    Disabled(DropReason),
    /// Already opened (synchronous service startup).
    Opened(SpillStore),
}

pub(super) struct PipelineOptions {
    /// Clear-generation mirror (tests use a private instance).
    pub clear: &'static clear::ClearState,
    pub budget: QueueBudget,
    pub spill: SpillSetup,
    /// Preprocessing threads; 0 detects the count on the writer thread.
    pub workers: usize,
    pub hooks: PipelineHooks,
}

pub(super) struct QueuedJob {
    pub job: RequestLogPayloadJob,
    pub bytes: u64,
}

#[derive(Default)]
pub(super) struct QueueInner {
    pub order: SpillOrderState,
    pub memory: VecDeque<QueuedJob>,
    pub handoff: VecDeque<QueuedJob>,
    /// Next id handed out by [`PipelineShared::begin_clear`].
    pub next_clear_seq: u64,
    /// Clears that started: the spill thread must roll the active segment
    /// and record the boundary before appending anything else.
    pub roll_requests: Vec<u64>,
    /// Boundary recorded per started clear (everything appended before
    /// the clear started lies before it).
    pub clear_boundaries: BTreeMap<u64, SpillPos>,
    /// Finished clears whose pre-boundary segments must be deleted. While
    /// non-empty the writer does not start new disk reads.
    pub purges: Vec<u64>,
    /// Clears that started while no spill store was installed; the store
    /// records their boundary when it is installed (after its leftovers).
    pub clears_without_boundary: Vec<u64>,
    /// Running clears and the mirror generation when each started. While
    /// any clear runs, hand-off jobs captured at or below the highest of
    /// these values are held in memory instead of being written to disk:
    /// they are discarded when the clear succeeds and requeued if it fails.
    pub clear_thresholds: BTreeMap<u64, i64>,
    /// Hand-off jobs held back by a running clear (still accounted as
    /// hand-off bytes).
    pub held: VecDeque<QueuedJob>,
    /// Incremented per clear; in-flight disk reads of an older value are
    /// discarded.
    pub purge_seq: u64,
    /// The writer is reading spill files (no purge meanwhile).
    pub reader_busy: bool,
    /// Consumed position the spill thread may release (delete files).
    pub release_upto: Option<SpillPos>,
    /// First position the reader must read (startup replay / after purge).
    pub read_start: SpillPos,
    /// Directory of the installed spill store (read by the writer).
    pub spill_dir: Option<PathBuf>,
    /// Disk batch being written: `(start, next, panics)`.
    pub disk_batch_in_flight: Option<(SpillPos, SpillPos, u32)>,
    pub writer_alive: bool,
    pub spill_running: bool,
    pub shutdown: bool,
}

#[derive(Default)]
pub(super) struct PipelineStats {
    pub dropped: [AtomicU64; 5],
    pub spilled_total: AtomicU64,
    pub replayed_total: AtomicU64,
    pub written_total: AtomicU64,
    pub stale_rejected_total: AtomicU64,
    pub spill_torn_total: AtomicU64,
    pub spill_corrupt_total: AtomicU64,
    pub writer_restarts: AtomicU64,
    pub spill_restarts: AtomicU64,
    pub preprocess_panics: AtomicU64,
    pub spill_disk_bytes: AtomicU64,
    pub discarded_by_clear_total: AtomicU64,
    pub busy_retries: AtomicU64,
    pub storage_unavailable_retries: AtomicU64,
}

/// Why a recent trace has no stored content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TraceDropReason {
    Dropped(DropReason),
    /// Rejected at write time: captured before a log clear, or older than
    /// the retention cutoff. Not counted in `dropped_total`.
    Stale,
}

impl TraceDropReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dropped(reason) => reason.as_str(),
            Self::Stale => "stale_generation",
        }
    }
}

#[derive(Default)]
struct RecentDrops {
    reasons: HashMap<String, TraceDropReason>,
    order: VecDeque<String>,
}

impl RecentDrops {
    fn insert(&mut self, trace_id: &str, reason: TraceDropReason) {
        if self.reasons.insert(trace_id.to_string(), reason).is_none() {
            self.order.push_back(trace_id.to_string());
            while self.order.len() > RECENT_DROPS_CAPACITY {
                if let Some(expired) = self.order.pop_front() {
                    self.reasons.remove(&expired);
                }
            }
        }
    }
}

pub(crate) struct PipelineShared {
    pub(super) inner: Mutex<QueueInner>,
    pub(super) writer_cv: Condvar,
    pub(super) spill_cv: Condvar,
    pub(super) budget: AtomicU64,
    pub(super) handoff_budget: AtomicU64,
    budget_info: Mutex<Option<QueueBudget>>,
    pub(super) stats: PipelineStats,
    recent_drops: Mutex<RecentDrops>,
    pub(super) hooks: PipelineHooks,
    pub(super) workers: usize,
    pub(super) boot_id: u64,
    pub(super) clear: &'static clear::ClearState,
    supervisor_cv: Condvar,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

pub(super) fn lock_inner(shared: &PipelineShared) -> MutexGuard<'_, QueueInner> {
    shared
        .inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Bytes kept alive by a queued job. The wire body only counts when it is
/// a different allocation than the logical body.
pub(super) fn job_accounted_bytes(job: &RequestLogPayloadJob) -> u64 {
    let mut bytes = job.body.len() as u64 + JOB_OVERHEAD_BYTES;
    if let Some(attempt) = job.attempt.as_ref() {
        let shared = attempt.wire_body.as_ptr() == job.body.as_ptr()
            && attempt.wire_body.len() == job.body.len();
        if !shared {
            bytes += attempt.wire_body.len() as u64;
        }
        bytes += (attempt.url.len() + attempt.method.len() + attempt.transport.len()) as u64;
    }
    bytes
        + (job.trace_id.len()
            + job.stage.len()
            + job.conversation_key.as_ref().map_or(0, String::len)) as u64
}

pub(super) fn preprocess_worker_count() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1);
    if cores <= 2 {
        1
    } else {
        (cores / 2).clamp(1, 4)
    }
}

impl PipelineShared {
    /// Create the shared state and start the spill, writer and supervisor
    /// threads. Startup only (spawns threads, may log).
    pub(super) fn start(options: PipelineOptions) -> Arc<Self> {
        let budget_bytes = options.budget.bytes;
        let shared = Arc::new(Self {
            inner: Mutex::new(QueueInner::default()),
            writer_cv: Condvar::new(),
            spill_cv: Condvar::new(),
            budget: AtomicU64::new(budget_bytes),
            handoff_budget: AtomicU64::new(spill_handoff_budget(budget_bytes)),
            budget_info: Mutex::new(Some(options.budget)),
            stats: PipelineStats::default(),
            recent_drops: Mutex::new(RecentDrops::default()),
            hooks: options.hooks,
            workers: options.workers,
            boot_id: clear::boot_id(),
            clear: options.clear,
            supervisor_cv: Condvar::new(),
            threads: Mutex::new(Vec::new()),
        });
        match options.spill {
            SpillSetup::Disabled(reason) => {
                lock_inner(&shared).order.set_spill_blocked(Some(reason));
            }
            SpillSetup::Opened(store) => super::spill::install_store(&shared, store),
        }
        shared.spawn_writer();
        let supervisor = shared.clone();
        match std::thread::Builder::new()
            .name("request-log-payload-supervisor".to_string())
            .spawn(move || supervise_writer(supervisor))
        {
            Ok(handle) => shared.register_thread(handle),
            Err(err) => log::warn!("event=request_log_payload_supervisor_spawn_failed err={err}"),
        }
        shared
    }

    /// Startup or supervisor thread only (spawns, may log).
    fn spawn_writer(self: &Arc<Self>) {
        let alive = match super::writer::spawn_writer(self.clone()) {
            Ok(handle) => {
                lock_recover(&self.threads).push(handle);
                true
            }
            Err(err) => {
                log::warn!("event=request_log_payload_writer_spawn_failed err={err}");
                false
            }
        };
        lock_inner(self).writer_alive = alive;
    }

    pub(super) fn register_thread(&self, handle: std::thread::JoinHandle<()>) {
        lock_recover(&self.threads).push(handle);
    }

    /// Hot path. Never blocks on IO, never spawns, never logs and never
    /// fails the caller.
    pub(super) fn submit(self: &Arc<Self>, mut job: RequestLogPayloadJob) {
        let bytes = job_accounted_bytes(&job);
        let budget = self.budget.load(Ordering::Relaxed);
        let handoff_budget = self.handoff_budget.load(Ordering::Relaxed);
        let reason = {
            let mut inner = lock_inner(self);
            let snapshot = self.clear.capture_snapshot();
            job.generation = snapshot.generation;
            job.clear_epoch = snapshot.clear_epoch;
            job.boot_id = self.boot_id;
            if inner.shutdown || !inner.writer_alive {
                DropReason::WriterUnavailable
            } else {
                match inner.order.route(bytes, budget, handoff_budget) {
                    RouteDecision::Memory => {
                        inner.memory.push_back(QueuedJob { job, bytes });
                        drop(inner);
                        self.writer_cv.notify_one();
                        return;
                    }
                    RouteDecision::Spill => {
                        inner.handoff.push_back(QueuedJob { job, bytes });
                        drop(inner);
                        self.spill_cv.notify_one();
                        return;
                    }
                    RouteDecision::Drop(reason) => reason,
                }
            }
        };
        self.record_drop(&job.trace_id, reason);
    }

    /// Count a job that will not be recorded and remember its trace.
    pub(super) fn record_drop(&self, trace_id: &str, reason: DropReason) {
        self.stats.dropped[reason.index()].fetch_add(1, Ordering::Relaxed);
        lock_recover(&self.recent_drops).insert(trace_id, TraceDropReason::Dropped(reason));
    }

    /// A job rejected at write time (captured before a clear or older than
    /// the retention cutoff): queryable per trace, not counted as dropped.
    pub(super) fn record_stale(&self, trace_id: &str) {
        self.stats
            .stale_rejected_total
            .fetch_add(1, Ordering::Relaxed);
        lock_recover(&self.recent_drops).insert(trace_id, TraceDropReason::Stale);
    }

    pub(super) fn drop_reason_for_trace(&self, trace_id: &str) -> Option<TraceDropReason> {
        lock_recover(&self.recent_drops)
            .reasons
            .get(trace_id)
            .copied()
    }

    /// A log clear is about to touch the database. The spill thread rolls
    /// to a new segment first, so the later purge deletes only what was
    /// spilled before this point, and hand-off jobs captured before the
    /// clear are held in memory meanwhile. Returns the clear id.
    pub(super) fn begin_clear(&self) -> u64 {
        let seq = {
            let mut inner = lock_inner(self);
            inner.next_clear_seq += 1;
            let seq = inner.next_clear_seq;
            let threshold = self.clear.mirror_value();
            inner.clear_thresholds.insert(seq, threshold);
            if inner.spill_running {
                inner.roll_requests.push(seq);
            } else {
                inner.clears_without_boundary.push(seq);
            }
            seq
        };
        self.spill_cv.notify_all();
        seq
    }

    /// The clear failed: nothing is purged for it and held jobs go back to
    /// the front of the hand-off queue once no clear is running.
    pub(super) fn abandon_clear(&self, seq: u64) {
        {
            let mut inner = lock_inner(self);
            inner.roll_requests.retain(|pending| *pending != seq);
            inner
                .clears_without_boundary
                .retain(|pending| *pending != seq);
            inner.clear_boundaries.remove(&seq);
            inner.clear_thresholds.remove(&seq);
            if inner.clear_thresholds.is_empty() {
                while let Some(queued) = inner.held.pop_back() {
                    inner.handoff.push_front(queued);
                }
            }
        }
        self.spill_cv.notify_all();
    }

    /// Log clear `seq` succeeded and the database is at `generation`.
    /// Discards queued jobs captured before the clear (older generation or
    /// unresolved), keeps jobs captured after the clear committed (they
    /// carry the new generation thanks to the writer's mirror refresh),
    /// discards the jobs the spill thread held back during the clear and
    /// schedules deletion of the segments spilled before the clear started.
    /// Atomic with respect to [`Self::submit`]. The database still rejects
    /// every stale job in its write transaction; this purge only bounds how
    /// long stale plaintext stays in memory or on disk.
    ///
    /// `seq == None` (the clear started before the queue existed): only the
    /// queues are filtered and no segment is deleted, so nothing written
    /// while the clear ran can be lost.
    pub(super) fn purge_after_clear(&self, seq: Option<u64>, generation: Option<i64>) {
        let discarded = {
            let mut inner = lock_inner(self);
            if let Some(generation) = generation {
                self.clear.observe_generation(generation, None);
            }
            let threshold = generation.unwrap_or_else(|| self.clear.mirror_value());
            let is_fresh = |queued: &QueuedJob| {
                queued.job.generation >= 0 && queued.job.generation >= threshold
            };
            let (memory_keep, memory_drop): (VecDeque<_>, VecDeque<_>) =
                std::mem::take(&mut inner.memory)
                    .into_iter()
                    .partition(|queued| is_fresh(queued));
            let (handoff_keep, mut handoff_drop): (VecDeque<_>, VecDeque<_>) =
                std::mem::take(&mut inner.handoff)
                    .into_iter()
                    .partition(|queued| is_fresh(queued));
            inner.memory = memory_keep;
            inner.handoff = handoff_keep;
            if let Some(seq) = seq {
                inner.clear_thresholds.remove(&seq);
                inner.roll_requests.retain(|pending| *pending != seq);
                inner
                    .clears_without_boundary
                    .retain(|pending| *pending != seq);
                if inner.spill_running {
                    inner.purges.push(seq);
                } else {
                    inner.clear_boundaries.remove(&seq);
                }
            }
            if inner.clear_thresholds.is_empty() {
                handoff_drop.extend(std::mem::take(&mut inner.held));
            }
            for queued in &memory_drop {
                inner.order.release_memory(queued.bytes);
            }
            for queued in &handoff_drop {
                inner.order.finish_handoff(queued.bytes, None);
            }
            inner.purge_seq += 1;
            inner.order.try_resume_memory();
            let mut discarded: Vec<QueuedJob> = memory_drop.into_iter().collect();
            discarded.extend(handoff_drop);
            discarded
        };
        self.stats
            .discarded_by_clear_total
            .fetch_add(discarded.len() as u64, Ordering::Relaxed);
        self.writer_cv.notify_all();
        self.spill_cv.notify_all();
        {
            // Queryable per trace (e.g. a job captured in the few ms after
            // the clear committed but before the mirror refresh).
            let mut recent = lock_recover(&self.recent_drops);
            for queued in &discarded {
                recent.insert(&queued.job.trace_id, TraceDropReason::Stale);
            }
        }
        drop(discarded);
    }

    pub(super) fn snapshot_stats(&self, trace_id: Option<&str>) -> RequestLogPayloadQueueStats {
        let (queued_bytes, queued_jobs, pending_bytes, pending_jobs, spilling, blocked) = {
            let inner = lock_inner(self);
            (
                inner.order.memory_bytes(),
                inner.memory.len() as u64,
                inner.order.handoff_bytes(),
                inner.order.handoff_jobs(),
                inner.order.spilling(),
                inner.order.spill_blocked(),
            )
        };
        let budget = *lock_recover(&self.budget_info);
        let unstarted = UNSTARTED_DROPS.load(Ordering::Relaxed);
        let dropped_by_reason: BTreeMap<String, u64> = DropReason::ALL
            .iter()
            .map(|reason| {
                let mut count = self.stats.dropped[reason.index()].load(Ordering::Relaxed);
                if *reason == DropReason::WriterUnavailable {
                    count += unstarted;
                }
                (reason.as_str().to_string(), count)
            })
            .collect();
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        RequestLogPayloadQueueStats {
            budget_bytes: self.budget.load(Ordering::Relaxed),
            budget_source: budget
                .map(|budget| budget.source.as_str().to_string())
                .unwrap_or_else(|| "pending".to_string()),
            effective_memory_bytes: budget.and_then(|budget| budget.effective_memory),
            handoff_budget_bytes: self.handoff_budget.load(Ordering::Relaxed),
            queued_bytes,
            queued_jobs,
            spill_pending_bytes: pending_bytes,
            spill_pending_jobs: pending_jobs,
            spill_disk_bytes: load(&self.stats.spill_disk_bytes),
            spilling,
            spill_available: blocked.is_none(),
            spill_blocked_reason: blocked.map(|reason| reason.as_str().to_string()),
            spilled_total: load(&self.stats.spilled_total),
            replayed_total: load(&self.stats.replayed_total),
            written_total: load(&self.stats.written_total),
            stale_rejected_total: load(&self.stats.stale_rejected_total),
            dropped_total: dropped_by_reason.values().sum(),
            dropped_by_reason,
            spill_torn_total: load(&self.stats.spill_torn_total),
            spill_corrupt_total: load(&self.stats.spill_corrupt_total),
            writer_restarts: load(&self.stats.writer_restarts),
            spill_restarts: load(&self.stats.spill_restarts),
            preprocess_panics: load(&self.stats.preprocess_panics),
            discarded_by_clear_total: load(&self.stats.discarded_by_clear_total),
            db_busy_retries: load(&self.stats.busy_retries),
            storage_unavailable_retries: load(&self.stats.storage_unavailable_retries),
            trace_drop_reason: trace_id
                .map(str::trim)
                .filter(|trace_id| !trace_id.is_empty())
                .and_then(|trace_id| self.drop_reason_for_trace(trace_id))
                .map(|reason| reason.as_str().to_string()),
        }
    }

    /// Stop every thread (tests).
    #[cfg(test)]
    pub(super) fn shutdown(&self) {
        lock_inner(self).shutdown = true;
        self.writer_cv.notify_all();
        self.spill_cv.notify_all();
        self.supervisor_cv.notify_all();
        let handles = std::mem::take(&mut *lock_recover(&self.threads));
        for handle in handles {
            let _ = handle.join();
        }
    }
}

/// Recreates the writer thread when spawning it failed. Runs on its own
/// thread so the hot path never spawns threads or logs.
fn supervise_writer(shared: Arc<PipelineShared>) {
    let mut last_attempt = std::time::Instant::now();
    let mut inner = lock_inner(&shared);
    loop {
        if inner.shutdown {
            return;
        }
        if !inner.writer_alive && last_attempt.elapsed() >= WRITER_RESPAWN_INTERVAL {
            drop(inner);
            last_attempt = std::time::Instant::now();
            shared.stats.writer_restarts.fetch_add(1, Ordering::Relaxed);
            shared.spawn_writer();
            inner = lock_inner(&shared);
            continue;
        }
        let (guard, _) = shared
            .supervisor_cv
            .wait_timeout(inner, SUPERVISOR_TICK)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner = guard;
    }
}

/// Counters exposed to administrators through `requestlog/payload_queue_stats`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestLogPayloadQueueStats {
    pub budget_bytes: u64,
    pub budget_source: String,
    pub effective_memory_bytes: Option<u64>,
    pub handoff_budget_bytes: u64,
    pub queued_bytes: u64,
    pub queued_jobs: u64,
    pub spill_pending_bytes: u64,
    pub spill_pending_jobs: u64,
    pub spill_disk_bytes: u64,
    pub spilling: bool,
    pub spill_available: bool,
    pub spill_blocked_reason: Option<String>,
    pub spilled_total: u64,
    pub replayed_total: u64,
    pub written_total: u64,
    pub stale_rejected_total: u64,
    pub dropped_total: u64,
    pub dropped_by_reason: BTreeMap<String, u64>,
    pub spill_torn_total: u64,
    pub spill_corrupt_total: u64,
    pub writer_restarts: u64,
    pub spill_restarts: u64,
    pub preprocess_panics: u64,
    pub discarded_by_clear_total: u64,
    pub db_busy_retries: u64,
    pub storage_unavailable_retries: u64,
    pub trace_drop_reason: Option<String>,
}

/// Detect `min(physical memory, cgroup limit)` and resolve the budget.
/// Reads `/proc` / cgroup files: never call it on the hot path.
pub(super) fn detect_queue_budget() -> QueueBudget {
    let override_value = std::env::var(ENV_QUEUE_MAX_BYTES).ok();
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let physical = Some(system.total_memory()).filter(|value| *value > 0);
    let cgroup = system
        .cgroup_limits()
        .map(|limits| limits.total_memory)
        .filter(|value| *value > 0)
        .or_else(read_cgroup_limit_file);
    let budget = resolve_queue_budget(override_value.as_deref(), physical, cgroup);
    log::info!(
        "event=request_log_payload_queue_budget bytes={} source={} effective_memory={} physical_memory={} cgroup_limit={} handoff_bytes={} invalid_override={}",
        budget.bytes,
        budget.source.as_str(),
        budget.effective_memory.map_or_else(|| "-".to_string(), |value| value.to_string()),
        physical.map_or_else(|| "-".to_string(), |value| value.to_string()),
        cgroup.map_or_else(|| "-".to_string(), |value| value.to_string()),
        spill_handoff_budget(budget.bytes),
        budget.invalid_override,
    );
    if budget.source == QueueBudgetSource::Fallback {
        log::warn!("event=request_log_payload_queue_budget_fallback reason=memory_unknown");
    }
    budget
}

#[cfg(target_os = "linux")]
fn read_cgroup_limit_file() -> Option<u64> {
    [
        "/sys/fs/cgroup/memory.max",
        "/sys/fs/cgroup/memory/memory.limit_in_bytes",
    ]
    .iter()
    .find_map(|path| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| parse_cgroup_memory_limit(&text))
    })
}

#[cfg(not(target_os = "linux"))]
fn read_cgroup_limit_file() -> Option<u64> {
    let _ = parse_cgroup_memory_limit;
    None
}

/// `request-log-spill/` next to the SQLite database.
pub(super) fn spill_dir_from_env() -> Option<PathBuf> {
    let db_path = std::env::var_os("CODEXMANAGER_DB_PATH")?;
    let db_path = PathBuf::from(db_path);
    let parent = db_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Some(parent.join(SPILL_DIR_NAME))
}

pub(super) fn open_spill_setup(dir: Option<PathBuf>, segment_max: u64) -> SpillSetup {
    let Some(dir) = dir else {
        return SpillSetup::Disabled(DropReason::IoError);
    };
    match SpillStore::open(&dir, segment_max) {
        Ok(store) => SpillSetup::Opened(store),
        Err(SpillOpenError::Locked) => {
            log::warn!(
                "event=request_log_payload_spill_locked dir={} action=disable_spill",
                dir.display()
            );
            SpillSetup::Disabled(DropReason::SpillLocked)
        }
        Err(SpillOpenError::Io(err)) => {
            log::warn!(
                "event=request_log_payload_spill_unavailable dir={} err={err}",
                dir.display()
            );
            SpillSetup::Disabled(DropReason::IoError)
        }
    }
}

static GLOBAL_PIPELINE: OnceLock<Arc<PipelineShared>> = OnceLock::new();

/// Hot path accessor: the queue is started at service startup only.
#[cfg_attr(test, allow(dead_code))]
pub(super) fn global_pipeline() -> Option<&'static Arc<PipelineShared>> {
    GLOBAL_PIPELINE.get()
}

/// Hot path: the queue was never started, count the capture as dropped.
#[cfg_attr(test, allow(dead_code))]
pub(super) fn count_unstarted_drop() {
    UNSTARTED_DROPS.fetch_add(1, Ordering::Relaxed);
}

/// Synchronous service startup, before the gateway accepts traffic:
/// initialize the generation mirror, detect the budget, open the spill
/// directory and start the writer (which replays leftover segments first).
pub(crate) fn initialize_request_log_payload_pipeline() {
    if crate::storage_helpers::seaorm_enabled() {
        return;
    }
    match crate::storage_helpers::open_storage() {
        Some(storage) => match clear::initialize_generation_from_storage(&storage) {
            Ok(generation) => {
                log::info!(
                    "event=request_log_payload_generation_initialized generation={generation}"
                )
            }
            Err(err) => log::warn!("event=request_log_payload_generation_init_failed err={err}"),
        },
        None => {
            log::warn!("event=request_log_payload_generation_init_failed err=storage unavailable")
        }
    }
    GLOBAL_PIPELINE.get_or_init(|| {
        let budget = detect_queue_budget();
        PipelineShared::start(PipelineOptions {
            clear: clear::global_clear_state(),
            budget,
            spill: open_spill_setup(spill_dir_from_env(), SEGMENT_MAX_BYTES),
            workers: preprocess_worker_count(),
            hooks: PipelineHooks::production(),
        })
    });
}

/// Called by the clear guard before the database clear.
pub(super) fn begin_clear() -> Option<u64> {
    GLOBAL_PIPELINE.get().map(|pipeline| pipeline.begin_clear())
}

/// Called by the clear guard when the database clear failed.
pub(super) fn abandon_clear(seq: Option<u64>) {
    if let (Some(pipeline), Some(seq)) = (GLOBAL_PIPELINE.get(), seq) {
        pipeline.abandon_clear(seq);
    }
}

pub(super) fn purge_after_clear(seq: Option<u64>, generation: Option<i64>) {
    match GLOBAL_PIPELINE.get() {
        Some(pipeline) => pipeline.purge_after_clear(seq, generation),
        None => {
            if let Some(generation) = generation {
                clear::global_clear_state().observe_generation(generation, None);
            }
        }
    }
}

/// Stats for the admin RPC. Zeros when the queue never started.
pub(crate) fn request_log_payload_queue_stats(
    trace_id: Option<&str>,
) -> RequestLogPayloadQueueStats {
    match GLOBAL_PIPELINE.get() {
        Some(pipeline) => pipeline.snapshot_stats(trace_id),
        None => RequestLogPayloadQueueStats {
            budget_source: "not_started".to_string(),
            dropped_by_reason: DropReason::ALL
                .iter()
                .map(|reason| {
                    let count = if *reason == DropReason::WriterUnavailable {
                        UNSTARTED_DROPS.load(Ordering::Relaxed)
                    } else {
                        0
                    };
                    (reason.as_str().to_string(), count)
                })
                .collect(),
            dropped_total: UNSTARTED_DROPS.load(Ordering::Relaxed),
            spill_available: true,
            ..Default::default()
        },
    }
}

#[cfg(test)]
#[path = "tests/request_log_payload_pipeline_tests.rs"]
mod tests;
