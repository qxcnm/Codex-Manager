//! Spill thread: appends over-budget jobs to `request-log-spill/` segments.
//!
//! The hot path never touches files: it only moves jobs into the bounded
//! hand-off queue. This thread owns the [`SpillStore`] (exclusive directory
//! lock), checks disk space every few seconds, flushes so the writer can
//! read, `sync_data`s about once per second and on segment roll, deletes
//! consumed segments and, after a log clear, every segment written before
//! the clear started (raw, unredacted bodies must not linger on disk).
//! Panics are isolated by a per-job guard plus a restarting loop.

use super::pipeline::{lock_inner, PipelineShared, QueuedJob};
use super::RequestLogPayloadJob;
use codexmanager_core::request_log_spill::budget::check_spill_space;
use codexmanager_core::request_log_spill::order::DropReason;
use codexmanager_core::request_log_spill::record::{
    SpillAttemptMeta, SpillRecordMeta, SpillRecordRef,
};
use codexmanager_core::request_log_spill::segment::{SpillPos, SpillStore};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

const IDLE_WAIT: Duration = Duration::from_secs(1);
const READER_WAIT: Duration = Duration::from_millis(20);
const DISK_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const DELETE_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const FLUSH_THRESHOLD_BYTES: u64 = 4 * 1024 * 1024;

/// Install an opened store and start the spill thread.
pub(super) fn install_store(shared: &Arc<PipelineShared>, mut store: SpillStore) {
    // Clears that started before the store existed: everything already in
    // the directory (leftovers of an earlier run) predates them, anything
    // spilled from now on may be newer. Their boundary is the end of the
    // leftovers, never "the time the clear finished".
    let start = store.replay_start();
    let committed = store.committed();
    let dir = store.dir().to_path_buf();
    shared
        .stats
        .spill_disk_bytes
        .store(store.live_bytes(), Ordering::Relaxed);
    if start < committed {
        log::info!(
            "event=request_log_payload_spill_replay segments={} bytes={}",
            store.segment_ids().len(),
            store.live_bytes()
        );
    }
    {
        let mut inner = lock_inner(shared);
        // Same critical section as `spill_running = true`, so a concurrent
        // `begin_clear` either lands here or requests a roll. A freshly
        // opened store has no active segment: `roll` does no file IO.
        if !inner.clears_without_boundary.is_empty() {
            let boundary = store.roll();
            for seq in std::mem::take(&mut inner.clears_without_boundary) {
                inner.clear_boundaries.insert(seq, boundary);
            }
        }
        inner.order.attach_spill(start, committed);
        inner.read_start = start;
        inner.spill_dir = Some(dir);
        inner.spill_running = true;
    }
    let thread_shared = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("request-log-payload-spill".to_string())
        .spawn(move || spill_main(thread_shared, store));
    match spawned {
        Ok(handle) => shared.register_thread(handle),
        Err(err) => {
            log::warn!("event=request_log_payload_spill_spawn_failed err={err}");
            let mut inner = lock_inner(shared);
            inner.spill_running = false;
            inner.order.set_spill_blocked(Some(DropReason::IoError));
        }
    }
    shared.writer_cv.notify_all();
}

/// Cached `(total, available)` of the disk holding the spill directory.
struct DiskSpace {
    dir: PathBuf,
    refreshed: Option<Instant>,
    info: Option<(u64, u64)>,
}

impl DiskSpace {
    fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            refreshed: None,
            info: None,
        }
    }

    fn current(&mut self) -> Option<(u64, u64)> {
        if self
            .refreshed
            .is_none_or(|refreshed| refreshed.elapsed() >= DISK_REFRESH_INTERVAL)
        {
            self.info = query_disk_space(&self.dir);
            self.refreshed = Some(Instant::now());
        }
        self.info
    }
}

/// Windows `canonicalize` returns `\\?\C:\...` while mount points are
/// `C:\`; strip the verbatim prefix so prefix matching works.
fn normalize_for_mount_match(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            if stripped.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(stripped.to_string());
            }
        }
    }
    path
}

fn query_disk_space(dir: &Path) -> Option<(u64, u64)> {
    let canonical = normalize_for_mount_match(std::fs::canonicalize(dir).ok()?);
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| (disk.total_space(), disk.available_space()))
}

fn same_allocation(left: &bytes::Bytes, right: &bytes::Bytes) -> bool {
    left.as_ptr() == right.as_ptr() && left.len() == right.len()
}

fn append_job(store: &mut SpillStore, job: &RequestLogPayloadJob) -> std::io::Result<u64> {
    let meta = SpillRecordMeta {
        trace_id: job.trace_id.clone(),
        stage: job.stage.clone(),
        generation: (job.generation >= 0).then_some(job.generation),
        clear_epoch: job.clear_epoch,
        boot_id: job.boot_id,
        redact: job.redact,
        preview: job.preview,
        conversation_key: job.conversation_key.clone(),
        attempt: job.attempt.as_ref().map(|attempt| SpillAttemptMeta {
            method: attempt.method.clone(),
            url: attempt.url.clone(),
            transport: attempt.transport.clone(),
            content_encoding: attempt.content_encoding.clone(),
        }),
        created_at: job.created_at,
    };
    let wire_body = job
        .attempt
        .as_ref()
        .filter(|attempt| !same_allocation(&attempt.wire_body, &job.body))
        .map(|attempt| &attempt.wire_body[..]);
    store.append(&SpillRecordRef {
        meta: &meta,
        body: &job.body,
        wire_body,
    })
}

/// A job appended to the active segment but not yet flushed. It counts as
/// spilled (and its hand-off accounting is released) only once the stream
/// is known to contain it completely.
pub(super) struct Unflushed {
    bytes: u64,
    trace_id: String,
    end: SpillPos,
}

/// Owns the hand-off accounting of the job the spill thread is working on.
/// Dropped without [`Self::appended`] (failure or panic), it releases the
/// accounting and records the drop, so the queue can always return to
/// memory mode.
struct HandoffGuard<'a> {
    shared: &'a PipelineShared,
    bytes: u64,
    trace_id: String,
    reason: DropReason,
    armed: bool,
}

impl<'a> HandoffGuard<'a> {
    fn new(shared: &'a PipelineShared, queued: &QueuedJob) -> Self {
        Self {
            shared,
            bytes: queued.bytes,
            trace_id: queued.job.trace_id.clone(),
            // A panic means the spill thread itself failed.
            reason: DropReason::WriterUnavailable,
            armed: true,
        }
    }

    fn fail(mut self, reason: DropReason) {
        self.reason = reason;
    }

    fn appended(mut self, end: SpillPos) -> Unflushed {
        self.armed = false;
        Unflushed {
            bytes: self.bytes,
            trace_id: std::mem::take(&mut self.trace_id),
            end,
        }
    }
}

impl Drop for HandoffGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        lock_inner(self.shared)
            .order
            .finish_handoff(self.bytes, None);
        self.shared.record_drop(&self.trace_id, self.reason);
        self.shared.writer_cv.notify_all();
    }
}

fn spill_main(shared: Arc<PipelineShared>, mut store: SpillStore) {
    let mut disk = DiskSpace::new(store.dir());
    let mut unflushed: Vec<Unflushed> = Vec::new();
    loop {
        let result = catch_unwind(AssertUnwindSafe(|| {
            spill_loop(&shared, &mut store, &mut disk, &mut unflushed)
        }));
        match result {
            Ok(()) => break,
            Err(_) => {
                shared.stats.spill_restarts.fetch_add(1, Ordering::Relaxed);
                log::error!("event=request_log_payload_spill_panicked action=restart");
                let _ = store.close_active();
                finish_unflushed(&shared, &store, &mut unflushed);
                if lock_inner(&shared).shutdown {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    let _ = store.close_active();
    finish_unflushed(&shared, &store, &mut unflushed);
}

/// Settle appended jobs after a flush, a roll or a failure: jobs that are
/// complete in the stream count as spilled; jobs lost with a failed write
/// or flush (past the on-disk end of an abandoned segment) are recorded as
/// `io_error` drops. Either way their hand-off accounting is released and
/// the readable end of the stream is published.
fn finish_unflushed(shared: &PipelineShared, store: &SpillStore, unflushed: &mut Vec<Unflushed>) {
    let committed = store.committed();
    let mut lost = Vec::new();
    {
        let mut inner = lock_inner(shared);
        for item in unflushed.drain(..) {
            inner.order.finish_handoff(item.bytes, None);
            if store.record_survives(item.end.segment, item.end.offset) {
                shared.stats.spilled_total.fetch_add(1, Ordering::Relaxed);
            } else {
                lost.push(item.trace_id);
            }
        }
        inner.order.set_committed(committed);
    }
    if !lost.is_empty() {
        log::warn!(
            "event=request_log_payload_spill_records_lost count={}",
            lost.len()
        );
    }
    for trace_id in lost {
        shared.record_drop(&trace_id, DropReason::IoError);
    }
    shared.writer_cv.notify_all();
}

enum Step {
    Roll(Vec<u64>),
    Purge(Vec<u64>),
    Release(SpillPos),
    Job(QueuedJob),
    Flush,
    Tick,
    Shutdown,
}

fn next_step(shared: &PipelineShared, has_unflushed: bool) -> Step {
    let mut inner = lock_inner(shared);
    loop {
        if inner.shutdown {
            return Step::Shutdown;
        }
        // Rolls first: a job handed off after a clear started must land in
        // a segment after that clear's boundary.
        if !inner.roll_requests.is_empty() {
            return Step::Roll(std::mem::take(&mut inner.roll_requests));
        }
        if !inner.purges.is_empty() {
            if !inner.reader_busy {
                return Step::Purge(std::mem::take(&mut inner.purges));
            }
            let (guard, _) = shared
                .spill_cv
                .wait_timeout(inner, READER_WAIT)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            inner = guard;
            continue;
        }
        if let Some(position) = inner.release_upto.take() {
            return Step::Release(position);
        }
        // While a clear runs, jobs captured before it (generation at or
        // below the mirror value when it started, or unresolved) are held
        // in memory instead of being written as plaintext after the clear
        // boundary. They are discarded when the clear succeeds.
        let hold_at_or_below = inner.clear_thresholds.values().copied().max();
        while let Some(job) = inner.handoff.pop_front() {
            if hold_at_or_below.is_some_and(|threshold| job.job.generation <= threshold) {
                inner.held.push_back(job);
                continue;
            }
            return Step::Job(job);
        }
        if has_unflushed {
            return Step::Flush;
        }
        let (guard, timeout) = shared
            .spill_cv
            .wait_timeout(inner, IDLE_WAIT)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner = guard;
        if timeout.timed_out() {
            return Step::Tick;
        }
    }
}

fn spill_loop(
    shared: &PipelineShared,
    store: &mut SpillStore,
    disk: &mut DiskSpace,
    unflushed: &mut Vec<Unflushed>,
) {
    let mut unflushed_bytes = 0_u64;
    let mut last_delete_retry = Instant::now();
    loop {
        match next_step(shared, !unflushed.is_empty()) {
            Step::Shutdown => return,
            Step::Roll(seqs) => {
                let boundary = store.roll();
                finish_unflushed(shared, store, unflushed);
                unflushed_bytes = 0;
                let mut inner = lock_inner(shared);
                for seq in seqs {
                    inner.clear_boundaries.insert(seq, boundary);
                }
            }
            Step::Purge(seqs) => {
                // Only segments written before the clear started are
                // deleted; jobs spilled while it ran survive (stale ones
                // among them are still rejected by the database).
                let recorded = {
                    let mut inner = lock_inner(shared);
                    seqs.iter()
                        .filter_map(|seq| inner.clear_boundaries.remove(seq))
                        .max()
                };
                let Some(boundary) = recorded else {
                    // Never guess a boundary: deleting up to "now" could
                    // remove jobs captured after the clear committed.
                    log::warn!("event=request_log_payload_spill_purge_skipped reason=no_boundary");
                    continue;
                };
                let deleted = store.purge_before(boundary);
                {
                    let mut inner = lock_inner(shared);
                    inner.order.skip_stream_to(boundary);
                    inner.read_start = inner.read_start.max(boundary);
                }
                log::info!(
                    "event=request_log_payload_spill_purged segments={deleted} pending_deletes={}",
                    store.pending_delete_count()
                );
                shared.writer_cv.notify_all();
            }
            Step::Release(position) => {
                store.release_consumed(position);
            }
            Step::Job(queued) => {
                let guard = HandoffGuard::new(shared, &queued);
                if let Some(hook) = shared.hooks.before_spill.as_ref() {
                    hook(&queued.job);
                }
                let space = check_spill_space(disk.current(), store.live_bytes(), queued.bytes);
                if space.is_err() {
                    lock_inner(shared)
                        .order
                        .set_spill_blocked(Some(DropReason::DiskFull));
                    guard.fail(DropReason::DiskFull);
                    log::warn!(
                        "event=request_log_payload_spill_disk_full dir_bytes={} action=drop",
                        store.live_bytes()
                    );
                } else {
                    match append_job(store, &queued.job) {
                        Ok(frame_len) => match store.write_position() {
                            Some(end) => {
                                unflushed.push(guard.appended(end));
                                unflushed_bytes += frame_len;
                            }
                            None => guard.fail(DropReason::IoError),
                        },
                        Err(err) => {
                            log::warn!(
                                "event=request_log_payload_spill_write_failed trace_id={} err={err}",
                                queued.job.trace_id
                            );
                            guard.fail(DropReason::IoError);
                            // The segment was abandoned: settle the jobs
                            // buffered before this one against what
                            // actually reached the disk.
                            finish_unflushed(shared, store, unflushed);
                            unflushed_bytes = 0;
                        }
                    }
                }
                drop(queued);
                if unflushed_bytes >= FLUSH_THRESHOLD_BYTES {
                    flush(shared, store, unflushed);
                    unflushed_bytes = 0;
                }
            }
            Step::Flush => {
                flush(shared, store, unflushed);
                unflushed_bytes = 0;
            }
            Step::Tick => {}
        }
        maintenance(shared, store, disk, &mut last_delete_retry);
    }
}

fn flush(shared: &PipelineShared, store: &mut SpillStore, unflushed: &mut Vec<Unflushed>) {
    if let Err(err) = store.flush() {
        log::warn!("event=request_log_payload_spill_flush_failed err={err}");
    }
    finish_unflushed(shared, store, unflushed);
}

fn maintenance(
    shared: &PipelineShared,
    store: &mut SpillStore,
    disk: &mut DiskSpace,
    last_delete_retry: &mut Instant,
) {
    if let Err(err) = store.maybe_sync(Instant::now()) {
        log::warn!("event=request_log_payload_spill_sync_failed err={err}");
    }
    if last_delete_retry.elapsed() >= DELETE_RETRY_INTERVAL {
        *last_delete_retry = Instant::now();
        if store.retry_pending_deletes() > 0 {
            log::warn!(
                "event=request_log_payload_spill_delete_pending count={}",
                store.pending_delete_count()
            );
        }
    }
    shared
        .stats
        .spill_disk_bytes
        .store(store.live_bytes(), Ordering::Relaxed);
    let blocked_by_disk = lock_inner(shared).order.spill_blocked() == Some(DropReason::DiskFull);
    if blocked_by_disk && check_spill_space(disk.current(), store.live_bytes(), 0).is_ok() {
        lock_inner(shared).order.set_spill_blocked(None);
        log::info!("event=request_log_payload_spill_disk_recovered");
    }
}
