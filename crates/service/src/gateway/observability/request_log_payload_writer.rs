//! The single database writer of the request payload queue.
//!
//! * Drains the memory queue first, then (while spilling) reads spill
//!   segments in order, so the global capture order is preserved. Leftover
//!   segments of an earlier run are replayed before anything else.
//! * Preprocessing (redaction, truncation, splitting, hashing) runs on a
//!   small pool; results are written in queue order by this thread.
//! * Group commit: one transaction per batch, cut at
//!   [`BATCH_MAX_JOBS`] jobs, [`BATCH_MAX_BYTES`] bytes or after
//!   [`BATCH_MAX_HOLD`] of accumulated write time.
//! * Every panic is isolated with `catch_unwind`; the loop restarts with
//!   backoff and counts restarts. Accounting of in-flight jobs is released
//!   by a drop guard, so a panic never leaks budget.

use super::clear;
use super::persist::{
    prepare_request_log_payload, write_prepared_payload, ParentCache, PreparedPayload,
};
use super::pipeline::{
    lock_inner, JobHookFn, PipelineShared, QueuedJob, StorageRef, BATCH_MAX_BYTES, BATCH_MAX_HOLD,
    BATCH_MAX_JOBS,
};
use super::{OutboundAttemptCapture, RequestLogPayloadJob};
use bytes::Bytes;
use codexmanager_core::request_log_spill::order::DropReason;
use codexmanager_core::request_log_spill::record::DecodedRecord;
use codexmanager_core::request_log_spill::segment::{SpillPos, SpillReader};
use codexmanager_core::request_log_spill::{classify_generation, GenerationCheck};
use codexmanager_core::storage::{is_sqlite_busy_error, Storage};
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The writer refreshes the in-memory clear-generation mirror from SQLite
/// at least this often (and inside every group commit). This bounds the
/// window in which jobs captured right after a clear committed still
/// carry the old generation and get rejected.
pub(super) const GENERATION_REFRESH_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_WAIT: Duration = GENERATION_REFRESH_INTERVAL;
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(5);
const READ_ERROR_SKIP_AFTER: u32 = 5;
const STORAGE_RETRY_BACKOFF_MIN: Duration = Duration::from_millis(100);
const STORAGE_RETRY_BACKOFF_MAX: Duration = Duration::from_secs(2);
const BEGIN_ATTEMPTS: u32 = 3;
const BUSY_BACKOFF_MIN: Duration = Duration::from_millis(50);
const BUSY_BACKOFF_MAX: Duration = Duration::from_secs(2);

pub(super) fn spawn_writer(
    shared: Arc<PipelineShared>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("request-log-payload-writer".to_string())
        .spawn(move || supervise(shared))
}

fn supervise(shared: Arc<PipelineShared>) {
    let mut backoff = Duration::from_millis(100);
    loop {
        match catch_unwind(AssertUnwindSafe(|| writer_main(&shared))) {
            Ok(()) => return,
            Err(_) => {
                shared.stats.writer_restarts.fetch_add(1, Ordering::Relaxed);
                log::error!("event=request_log_payload_writer_panicked action=restart");
                skip_poisoned_disk_batch(&shared);
                if lock_inner(&shared).shutdown {
                    return;
                }
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(MAX_RESTART_BACKOFF);
            }
        }
    }
}

/// A disk batch that panicked the writer twice is skipped so a poisoned
/// record cannot stall the queue forever.
fn skip_poisoned_disk_batch(shared: &PipelineShared) {
    let mut inner = lock_inner(shared);
    let Some((start, next, failures)) = inner.disk_batch_in_flight else {
        return;
    };
    if failures + 1 >= 2 {
        inner.disk_batch_in_flight = None;
        inner.order.set_consumed(next);
        inner.release_upto = Some(next);
        inner.order.try_resume_memory();
        drop(inner);
        shared
            .stats
            .spill_corrupt_total
            .fetch_add(1, Ordering::Relaxed);
        shared.spill_cv.notify_all();
        log::error!(
            "event=request_log_payload_spill_batch_skipped segment={} offset={}",
            start.segment,
            start.offset
        );
    } else {
        inner.disk_batch_in_flight = Some((start, next, failures + 1));
    }
}

struct WriterState {
    read_cursor: SpillPos,
    purge_seq: u64,
    read_failures: u32,
    cache: ParentCache,
    pool: PreprocessPool,
    reported_drops: u64,
    last_report: Instant,
    last_generation_refresh: Option<Instant>,
}

enum Work {
    Memory(Vec<QueuedJob>),
    Disk {
        jobs: Vec<RequestLogPayloadJob>,
        next: SpillPos,
    },
    Idle,
    Shutdown,
}

fn writer_main(shared: &Arc<PipelineShared>) {
    let (read_cursor, purge_seq) = {
        let inner = lock_inner(shared);
        (
            inner.read_start.max(inner.order.consumed()),
            inner.purge_seq,
        )
    };
    let mut state = WriterState {
        read_cursor,
        purge_seq,
        read_failures: 0,
        cache: ParentCache::default(),
        pool: PreprocessPool::new(if shared.workers == 0 {
            super::pipeline::preprocess_worker_count()
        } else {
            shared.workers
        }),
        reported_drops: 0,
        last_report: Instant::now(),
        last_generation_refresh: None,
    };
    loop {
        report_drops(shared, &mut state);
        if state
            .last_generation_refresh
            .is_none_or(|last| last.elapsed() >= GENERATION_REFRESH_INTERVAL)
        {
            refresh_generation_mirror(shared);
            state.last_generation_refresh = Some(Instant::now());
        }
        match gather(shared, &mut state) {
            Work::Shutdown => return,
            Work::Idle => {}
            Work::Memory(jobs) => {
                let guard = InFlightGuard::new(shared, &jobs);
                let jobs: Vec<RequestLogPayloadJob> =
                    jobs.into_iter().map(|queued| queued.job).collect();
                write_jobs(shared, &mut state, jobs);
                guard.complete();
            }
            Work::Disk { jobs, next } => {
                let count = jobs.len() as u64;
                write_jobs(shared, &mut state, jobs);
                shared
                    .stats
                    .replayed_total
                    .fetch_add(count, Ordering::Relaxed);
                mark_consumed(shared, next);
            }
        }
    }
}

/// Writer thread only (never the hot path): read the clear generation and
/// publish it to the mirror. Any value read from SQLite is safe to publish.
fn refresh_generation_mirror(shared: &PipelineShared) {
    let read_epoch = shared.clear.read_epoch();
    let Some(storage) = (shared.hooks.open)() else {
        return;
    };
    if let Ok(generation) = storage.request_log_payload_generation() {
        shared.clear.observe_generation(generation, read_epoch);
    }
}

fn mark_consumed(shared: &PipelineShared, next: SpillPos) {
    let mut inner = lock_inner(shared);
    inner.disk_batch_in_flight = None;
    inner.order.set_consumed(next);
    // A clear purge may have moved `consumed` past `next` meanwhile.
    inner.release_upto = Some(inner.order.consumed());
    inner.order.try_resume_memory();
    drop(inner);
    shared.spill_cv.notify_all();
}

fn gather(shared: &Arc<PipelineShared>, state: &mut WriterState) -> Work {
    let mut inner = lock_inner(shared);
    loop {
        if inner.shutdown {
            return Work::Shutdown;
        }
        if inner.purge_seq != state.purge_seq && inner.purges.is_empty() {
            state.purge_seq = inner.purge_seq;
            state.read_cursor = inner.read_start.max(inner.order.consumed());
        }
        if !inner.memory.is_empty() {
            let mut jobs = Vec::new();
            let mut bytes = 0_u64;
            while let Some(front) = inner.memory.front() {
                if !jobs.is_empty()
                    && (jobs.len() >= BATCH_MAX_JOBS || bytes + front.bytes > BATCH_MAX_BYTES)
                {
                    break;
                }
                let Some(queued) = inner.memory.pop_front() else {
                    break;
                };
                bytes += queued.bytes;
                jobs.push(queued);
            }
            return Work::Memory(jobs);
        }
        let committed = inner.order.committed();
        let spill_dir = inner.spill_dir.clone();
        if inner.purges.is_empty() && state.read_cursor < committed {
            if let Some(dir) = spill_dir {
                inner.reader_busy = true;
                let seq = inner.purge_seq;
                let from = state.read_cursor;
                drop(inner);
                let result =
                    SpillReader::new(&dir).read(from, committed, BATCH_MAX_JOBS, BATCH_MAX_BYTES);
                inner = lock_inner(shared);
                inner.reader_busy = false;
                shared.spill_cv.notify_all();
                if inner.purge_seq != seq {
                    continue;
                }
                match result {
                    Ok(batch) => {
                        state.read_failures = 0;
                        shared
                            .stats
                            .spill_torn_total
                            .fetch_add(batch.torn, Ordering::Relaxed);
                        shared
                            .stats
                            .spill_corrupt_total
                            .fetch_add(batch.corrupt, Ordering::Relaxed);
                        state.read_cursor = batch.next;
                        if batch.records.is_empty() {
                            inner.order.set_consumed(batch.next);
                            inner.release_upto = Some(batch.next);
                            inner.order.try_resume_memory();
                            shared.spill_cv.notify_all();
                            continue;
                        }
                        inner.disk_batch_in_flight = match inner.disk_batch_in_flight {
                            Some((start, _, failures)) if start == from => {
                                Some((from, batch.next, failures))
                            }
                            _ => Some((from, batch.next, 0)),
                        };
                        drop(inner);
                        let jobs = batch.records.into_iter().map(job_from_record).collect();
                        return Work::Disk {
                            jobs,
                            next: batch.next,
                        };
                    }
                    Err(err) => {
                        state.read_failures += 1;
                        // Never log while holding the queue lock.
                        drop(inner);
                        log::warn!(
                            "event=request_log_payload_spill_read_failed segment={} offset={} failures={} err={err}",
                            from.segment,
                            from.offset,
                            state.read_failures
                        );
                        inner = lock_inner(shared);
                        if state.read_failures >= READ_ERROR_SKIP_AFTER {
                            state.read_failures = 0;
                            state.read_cursor = committed;
                            inner.order.set_consumed(committed);
                            inner.release_upto = Some(committed);
                            inner.order.try_resume_memory();
                            shared.stats.dropped[DropReason::IoError.index()]
                                .fetch_add(1, Ordering::Relaxed);
                            shared.spill_cv.notify_all();
                            continue;
                        }
                    }
                }
            }
        }
        if inner.order.spilling() && inner.order.try_resume_memory() {
            shared.spill_cv.notify_all();
        }
        let (guard, timeout) = shared
            .writer_cv
            .wait_timeout(inner, IDLE_WAIT)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner = guard;
        if timeout.timed_out() {
            return Work::Idle;
        }
    }
}

/// Rate-limited drop log (the hot path itself never logs).
fn report_drops(shared: &PipelineShared, state: &mut WriterState) {
    if state.last_report.elapsed() < Duration::from_secs(10) {
        return;
    }
    state.last_report = Instant::now();
    let total: u64 = shared
        .stats
        .dropped
        .iter()
        .map(|value| value.load(Ordering::Relaxed))
        .sum();
    if total != state.reported_drops {
        let by_reason = DropReason::ALL
            .iter()
            .map(|reason| {
                format!(
                    "{}={}",
                    reason.as_str(),
                    shared.stats.dropped[reason.index()].load(Ordering::Relaxed)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        log::warn!("event=request_log_payload_dropped total={total} by_reason={by_reason}");
        state.reported_drops = total;
    }
}

/// Releases the budget of in-flight memory jobs. Dropped without
/// [`Self::complete`] (writer panic), it also counts them as dropped.
struct InFlightGuard<'a> {
    shared: &'a PipelineShared,
    bytes: Vec<u64>,
    traces: Vec<String>,
    completed: bool,
}

impl<'a> InFlightGuard<'a> {
    fn new(shared: &'a PipelineShared, jobs: &[QueuedJob]) -> Self {
        Self {
            shared,
            bytes: jobs.iter().map(|queued| queued.bytes).collect(),
            traces: jobs
                .iter()
                .map(|queued| queued.job.trace_id.clone())
                .collect(),
            completed: false,
        }
    }

    fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        {
            let mut inner = lock_inner(self.shared);
            for bytes in &self.bytes {
                inner.order.release_memory(*bytes);
            }
        }
        if !self.completed {
            for trace_id in &self.traces {
                self.shared
                    .record_drop(trace_id, DropReason::WriterUnavailable);
            }
        }
    }
}

/// Rebuild a job from a spill record without copying the body.
fn job_from_record(record: DecodedRecord) -> RequestLogPayloadJob {
    let DecodedRecord {
        meta,
        payload,
        body_range,
        wire_range,
    } = record;
    let payload = Bytes::from(payload);
    let body = payload.slice(body_range);
    let attempt = meta.attempt.map(|attempt| OutboundAttemptCapture {
        method: attempt.method,
        url: attempt.url,
        transport: attempt.transport,
        content_encoding: attempt.content_encoding,
        wire_body: wire_range
            .clone()
            .map(|range| payload.slice(range))
            .unwrap_or_else(|| body.clone()),
    });
    RequestLogPayloadJob {
        trace_id: meta.trace_id,
        stage: meta.stage,
        body,
        conversation_key: meta.conversation_key,
        redact: meta.redact,
        preview: meta.preview,
        created_at: meta.created_at,
        generation: meta.generation.unwrap_or(clear::GENERATION_UNRESOLVED),
        clear_epoch: meta.clear_epoch,
        boot_id: meta.boot_id,
        attempt,
    }
}

/// Acquire storage for one batch. A failure (pool exhausted, acquire
/// timeout, database temporarily unavailable) is transient: the batch is
/// kept and retried with backoff and without a limit, so the memory budget
/// and the disk spill absorb the backlog. Every attempt acquires a fresh
/// handle and a failed attempt holds nothing, so the writer never pins a
/// pool connection while waiting. Returns `None` only on shutdown.
fn open_storage_until_available(shared: &PipelineShared) -> Option<StorageRef> {
    let mut backoff = STORAGE_RETRY_BACKOFF_MIN;
    let mut failures = 0_u64;
    loop {
        if let Some(storage) = (shared.hooks.open)() {
            if failures > 0 {
                log::info!("event=request_log_payload_storage_recovered failures={failures}");
            }
            return Some(storage);
        }
        failures += 1;
        shared
            .stats
            .storage_unavailable_retries
            .fetch_add(1, Ordering::Relaxed);
        if failures == 1 || failures % 50 == 0 {
            log::warn!(
                "event=request_log_payload_storage_unavailable failures={failures} action=retry"
            );
        }
        if lock_inner(shared).shutdown {
            return None;
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(STORAGE_RETRY_BACKOFF_MAX);
    }
}

fn write_jobs(shared: &PipelineShared, state: &mut WriterState, jobs: Vec<RequestLogPayloadJob>) {
    if jobs.is_empty() {
        return;
    }
    if let Some(before_write) = shared.hooks.before_write.as_ref() {
        before_write();
    }
    let traces: Vec<String> = jobs.iter().map(|job| job.trace_id.clone()).collect();
    let results = state.pool.run(jobs, shared.hooks.before_preprocess.clone());
    let mut prepared = Vec::with_capacity(results.len());
    for (trace_id, result) in traces.iter().zip(results) {
        match result {
            Ok(item) => prepared.push(item),
            Err(()) => {
                shared
                    .stats
                    .preprocess_panics
                    .fetch_add(1, Ordering::Relaxed);
                shared.record_drop(trace_id, DropReason::WriterUnavailable);
            }
        }
    }
    if prepared.is_empty() {
        return;
    }
    let Some(storage) = open_storage_until_available(shared) else {
        // Shutdown while the storage was unavailable.
        for item in &prepared {
            shared.record_drop(&item.trace_id, DropReason::WriterUnavailable);
        }
        return;
    };
    let wait = || {
        refresh_generation_mirror(shared);
        lock_inner(shared).shutdown
    };
    let report = write_prepared_batch(
        &storage,
        prepared,
        &mut state.cache,
        shared.boot_id,
        shared.clear,
        &wait,
    );
    state.last_generation_refresh = Some(Instant::now());
    shared
        .stats
        .written_total
        .fetch_add(report.written, Ordering::Relaxed);
    shared
        .stats
        .busy_retries
        .fetch_add(report.busy_retries, Ordering::Relaxed);
    for trace_id in &report.stale {
        shared.record_stale(trace_id);
    }
    for trace_id in &report.failed {
        shared.record_drop(trace_id, DropReason::IoError);
    }
}

#[derive(Debug, Default)]
pub(super) struct BatchReport {
    pub written: u64,
    /// Already stored (duplicate) or skipped as identical to the client body.
    pub not_inserted: u64,
    /// Rejected: captured before a log clear or older than the retention
    /// cutoff (checked in the write transaction).
    pub stale: Vec<String>,
    pub failed: Vec<String>,
    /// Write-lock waits caused by `SQLITE_BUSY` / `SQLITE_LOCKED`.
    pub busy_retries: u64,
}

enum Outcome {
    Written,
    NotInserted,
    Stale,
    Failed,
}

fn panic_error() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some("request payload write panicked".to_string()))
}

/// Group commit of prepared jobs in queue order. Each transaction holds the
/// SQLite write lock for at most about [`BATCH_MAX_HOLD`]; the remainder
/// continues in a new transaction.
///
/// When SQLite reports the database as busy/locked (another connection
/// holds the write lock: a clear's batched deletes, a maintenance task),
/// the batch is retried with backoff and without a retry limit. Nothing is
/// dropped and the queue accounting is unchanged, so the memory budget and
/// the disk spill absorb the backlog meanwhile. `wait` runs between busy
/// retries (mirror refresh) and returns `true` to give up (shutdown).
/// Only real (non-busy) errors count as failures.
pub(super) fn write_prepared_batch(
    storage: &Storage,
    prepared: Vec<PreparedPayload>,
    cache: &mut ParentCache,
    boot_id: u64,
    clear_state: &clear::ClearState,
    wait: &dyn Fn() -> bool,
) -> BatchReport {
    let mut report = BatchReport::default();
    let mut pending: VecDeque<PreparedPayload> = prepared.into();
    let mut begin_failures = 0_u32;
    let mut busy_backoff = BUSY_BACKOFF_MIN;
    while !pending.is_empty() {
        let started = Instant::now();
        let mut done: Vec<Outcome> = Vec::new();
        let read_epoch = clear_state.read_epoch();
        let result = storage.write_request_log_payload_batch(|batch| {
            let db_generation = batch.current_generation().ok();
            if let Some(generation) = db_generation {
                clear_state.observe_generation(generation, read_epoch);
            }
            let clear_epoch = clear_state.current_clear_epoch();
            for item in pending.iter() {
                let check = classify_generation(
                    Some(item.generation).filter(|generation| *generation >= 0),
                    item.clear_epoch,
                    item.boot_id,
                    boot_id,
                    clear_epoch,
                );
                let generation = match check {
                    GenerationCheck::Known(generation) => Some(generation),
                    GenerationCheck::ResolveAtWrite => db_generation,
                    GenerationCheck::Reject => None,
                };
                let outcome = match generation {
                    None => Outcome::Stale,
                    Some(generation) => {
                        if matches!(batch.job_is_current(generation, item.created_at), Ok(false)) {
                            Outcome::Stale
                        } else {
                            match batch.entry(|batch| {
                                catch_unwind(AssertUnwindSafe(|| {
                                    write_prepared_payload(batch, item, generation, cache)
                                }))
                                .unwrap_or_else(|_| Err(panic_error()))
                            }) {
                                Ok(write) if write.inserted => Outcome::Written,
                                Ok(_) => Outcome::NotInserted,
                                Err(err) => {
                                    log::warn!(
                                        "event=request_log_payload_insert_failed trace_id={} err={err}",
                                        item.trace_id
                                    );
                                    Outcome::Failed
                                }
                            }
                        }
                    }
                };
                done.push(outcome);
                if started.elapsed() >= BATCH_MAX_HOLD {
                    break;
                }
            }
        });
        match result {
            Ok(()) => {
                begin_failures = 0;
                busy_backoff = BUSY_BACKOFF_MIN;
                for outcome in done {
                    let Some(item) = pending.pop_front() else {
                        break;
                    };
                    match outcome {
                        Outcome::Written => report.written += 1,
                        Outcome::NotInserted => report.not_inserted += 1,
                        Outcome::Stale => report.stale.push(item.trace_id),
                        Outcome::Failed => report.failed.push(item.trace_id),
                    }
                }
            }
            Err(err) if is_sqlite_busy_error(&err) => {
                // Begin or commit hit the write lock of another connection;
                // the transaction was rolled back, every job stays pending.
                report.busy_retries += 1;
                if report.busy_retries == 1 || report.busy_retries % 50 == 0 {
                    log::info!(
                        "event=request_log_payload_batch_busy retries={} jobs={} err={err}",
                        report.busy_retries,
                        pending.len()
                    );
                }
                std::thread::sleep(busy_backoff);
                busy_backoff = (busy_backoff * 2).min(BUSY_BACKOFF_MAX);
                if wait() {
                    report
                        .failed
                        .extend(pending.drain(..).map(|item| item.trace_id));
                    break;
                }
            }
            Err(err) if done.is_empty() => {
                begin_failures += 1;
                log::warn!(
                    "event=request_log_payload_batch_begin_failed attempt={begin_failures} err={err}"
                );
                if begin_failures >= BEGIN_ATTEMPTS {
                    report
                        .failed
                        .extend(pending.drain(..).map(|item| item.trace_id));
                    break;
                }
                std::thread::sleep(Duration::from_millis(100 * begin_failures as u64));
            }
            Err(err) => {
                log::warn!(
                    "event=request_log_payload_batch_commit_failed jobs={} err={err}",
                    done.len()
                );
                for _ in 0..done.len() {
                    if let Some(item) = pending.pop_front() {
                        report.failed.push(item.trace_id);
                    }
                }
            }
        }
    }
    report
}

type PoolOutcome = Result<PreparedPayload, ()>;

struct PoolTask {
    index: usize,
    job: RequestLogPayloadJob,
    hook: Option<JobHookFn>,
    reply: mpsc::Sender<(usize, PoolOutcome)>,
}

fn preprocess(job: &RequestLogPayloadJob, hook: Option<&JobHookFn>) -> PoolOutcome {
    catch_unwind(AssertUnwindSafe(|| {
        if let Some(hook) = hook {
            hook(job);
        }
        prepare_request_log_payload(job)
    }))
    .map_err(|_| ())
}

/// Small CPU pool (1 worker on <= 2 cores, else min(cores / 2, 4)).
struct PreprocessPool {
    sender: Option<mpsc::Sender<PoolTask>>,
}

impl PreprocessPool {
    fn new(workers: usize) -> Self {
        let (sender, receiver) = mpsc::channel::<PoolTask>();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut spawned = 0;
        for index in 0..workers.max(1) {
            let receiver = receiver.clone();
            let result = std::thread::Builder::new()
                .name(format!("request-log-payload-prep-{index}"))
                .spawn(move || loop {
                    let task = {
                        let receiver = receiver
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        receiver.recv()
                    };
                    let Ok(task) = task else {
                        return;
                    };
                    let outcome = preprocess(&task.job, task.hook.as_ref());
                    drop(task.job);
                    let _ = task.reply.send((task.index, outcome));
                });
            match result {
                Ok(_) => spawned += 1,
                Err(err) => log::warn!("event=request_log_payload_prep_spawn_failed err={err}"),
            }
        }
        Self {
            sender: (spawned > 0).then_some(sender),
        }
    }

    /// Preprocess `jobs`, returning results in input order. Falls back to
    /// inline processing when the pool is unavailable.
    fn run(
        &mut self,
        jobs: Vec<RequestLogPayloadJob>,
        hook: Option<JobHookFn>,
    ) -> Vec<PoolOutcome> {
        let count = jobs.len();
        let mut results: Vec<Option<PoolOutcome>> = (0..count).map(|_| None).collect();
        let (reply, replies) = mpsc::channel();
        let mut inline = Vec::new();
        for (index, job) in jobs.into_iter().enumerate() {
            let task = PoolTask {
                index,
                job,
                hook: hook.clone(),
                reply: reply.clone(),
            };
            match self.sender.as_ref() {
                Some(sender) => {
                    if let Err(mpsc::SendError(task)) = sender.send(task) {
                        self.sender = None;
                        inline.push(task);
                    }
                }
                None => inline.push(task),
            }
        }
        drop(reply);
        for task in inline {
            let outcome = preprocess(&task.job, task.hook.as_ref());
            results[task.index] = Some(outcome);
        }
        for (index, outcome) in replies.iter() {
            if let Some(slot) = results.get_mut(index) {
                *slot = Some(outcome);
            }
        }
        results
            .into_iter()
            .map(|outcome| outcome.unwrap_or(Err(())))
            .collect()
    }
}
