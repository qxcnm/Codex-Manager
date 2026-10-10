//! In-process mirror of the request payload clear generation.
//!
//! Invariant: a job captured before a log clear is never written after that
//! clear (including after a restart or a spill replay).
//!
//! * The mirror only ever holds values read from SQLite, so it is never
//!   ahead of the database. A job carrying mirror value `G` is written only
//!   while the database is still at `G` (`*_if_current` in the same write
//!   transaction); every clear increments the database generation first.
//! * Until the mirror is initialized, jobs carry [`GENERATION_UNRESOLVED`]
//!   plus the number of clears started in this process (`clear_epoch`).
//!   The writer resolves the generation inside its write transaction and
//!   only if no clear started since capture and none was in flight at
//!   capture time. Spill records carry the process `boot_id`, so unresolved
//!   jobs replayed by another process run are always rejected.
//!
//! Every service path that increments the generation must run through
//! [`guard_request_log_payload_clear`] (or begin/finish).

use codexmanager_core::request_log_spill::CLEAR_EPOCH_IN_FLIGHT;
use codexmanager_core::storage::Storage;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Job generation placeholder used before the mirror is initialized.
pub(crate) const GENERATION_UNRESOLVED: i64 = -1;

/// Mirror plus clear counters. Production uses [`global_clear_state`];
/// tests use private instances so parallel tests cannot interfere.
pub(crate) struct ClearState {
    mirror: AtomicI64,
    started: AtomicU64,
    finished: AtomicU64,
    /// Serializes `begin_clear` with the (rare) lowering of the mirror.
    /// Never taken by the hot path.
    lower_lock: Mutex<()>,
}

static GLOBAL_CLEAR_STATE: ClearState = ClearState::new();
static BOOT_ID: OnceLock<u64> = OnceLock::new();

pub(crate) fn global_clear_state() -> &'static ClearState {
    &GLOBAL_CLEAR_STATE
}

/// Random id of this process run, stored in every spill record.
pub(crate) fn boot_id() -> u64 {
    *BOOT_ID.get_or_init(|| {
        let random: u64 = rand::random();
        random.max(1)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureSnapshot {
    pub generation: i64,
    pub clear_epoch: u64,
}

impl ClearState {
    pub(crate) const fn new() -> Self {
        Self {
            mirror: AtomicI64::new(GENERATION_UNRESOLVED),
            started: AtomicU64::new(0),
            finished: AtomicU64::new(0),
            lower_lock: Mutex::new(()),
        }
    }

    /// Lock-free snapshot taken by the hot path (atomics only).
    pub(crate) fn capture_snapshot(&self) -> CaptureSnapshot {
        let finished = self.finished.load(Ordering::SeqCst);
        let started = self.started.load(Ordering::SeqCst);
        let generation = self.mirror.load(Ordering::SeqCst);
        CaptureSnapshot {
            generation,
            clear_epoch: if started == finished {
                started
            } else {
                CLEAR_EPOCH_IN_FLIGHT
            },
        }
    }

    /// Number of clears started in this process.
    pub(crate) fn current_clear_epoch(&self) -> u64 {
        self.started.load(Ordering::SeqCst)
    }

    /// Current mirror value ([`GENERATION_UNRESOLVED`] before init).
    pub(crate) fn mirror_value(&self) -> i64 {
        self.mirror.load(Ordering::SeqCst)
    }

    /// Clear epoch to pass to [`Self::observe_generation`]: take it
    /// *before* reading the generation from SQLite. `None` while a clear
    /// is in flight.
    pub(crate) fn read_epoch(&self) -> Option<u64> {
        let finished = self.finished.load(Ordering::SeqCst);
        let started = self.started.load(Ordering::SeqCst);
        (started == finished).then_some(started)
    }

    /// Publish a generation just read from SQLite.
    ///
    /// The mirror only moves up (`fetch_max`), so a slow reader that read
    /// `G` before a clear committed `G + 1` can never move it back. It is
    /// lowered only when the database itself went back (file replaced or
    /// restored) and no clear started or was running since the read began
    /// (`read_epoch`); that check and [`Self::begin_clear`] share a lock.
    /// Any value read from SQLite keeps the mirror at or behind the
    /// database, so stale jobs are never accepted.
    pub(crate) fn observe_generation(&self, generation: i64, read_epoch: Option<u64>) {
        if generation < 0 {
            return;
        }
        let previous = self.mirror.fetch_max(generation, Ordering::SeqCst);
        if generation >= previous {
            return;
        }
        let Some(read_epoch) = read_epoch else {
            return;
        };
        let _guard = self
            .lower_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.read_epoch() == Some(read_epoch) {
            self.mirror.store(generation, Ordering::SeqCst);
        }
    }

    pub(crate) fn begin_clear(&self) {
        let _guard = self
            .lower_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.started.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn finish_clear(&self) {
        self.finished.fetch_add(1, Ordering::SeqCst);
    }
}

/// Synchronous startup initialization (before the gateway accepts traffic).
pub(crate) fn initialize_generation_from_storage(storage: &Storage) -> Result<i64, String> {
    let state = global_clear_state();
    let epoch = state.read_epoch();
    let generation = storage
        .request_log_payload_generation()
        .map_err(|err| err.to_string())?;
    state.observe_generation(generation, epoch);
    Ok(generation)
}

/// Marker returned by [`begin_request_log_payload_clear`].
#[must_use = "finish_request_log_payload_clear must be called"]
pub(crate) struct RequestLogPayloadClearToken {
    seq: Option<u64>,
}

/// Call before a clear touches the database.
pub(crate) fn begin_request_log_payload_clear() -> RequestLogPayloadClearToken {
    global_clear_state().begin_clear();
    RequestLogPayloadClearToken {
        seq: super::pipeline::begin_clear(),
    }
}

/// Call after the clear returned. On success queued jobs captured before
/// the clear are discarded, the segments spilled before it are scheduled
/// for deletion and the mirror is refreshed once more.
///
/// The writer thread already refreshes the mirror from SQLite at every
/// group commit and at least every 100 ms, so jobs captured while a long
/// (batched) clear is still deleting old rows carry the new generation.
pub(crate) fn finish_request_log_payload_clear(
    token: RequestLogPayloadClearToken,
    succeeded: bool,
) {
    if succeeded {
        let generation = crate::storage_helpers::open_storage()
            .and_then(|storage| storage.request_log_payload_generation().ok());
        super::pipeline::purge_after_clear(token.seq, generation);
    } else {
        super::pipeline::abandon_clear(token.seq);
    }
    global_clear_state().finish_clear();
}

/// Wrap a call that clears request logs (and therefore increments the
/// payload clear generation).
pub(crate) fn guard_request_log_payload_clear<T, E>(
    clear: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let token = begin_request_log_payload_clear();
    let result = clear();
    finish_request_log_payload_clear(token, result.is_ok());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_log_payload_clear_mirror_never_moves_back_across_a_clear() {
        let state = ClearState::new();
        let epoch = state.read_epoch();
        state.observe_generation(4, epoch);
        // Writer reads G=4 and stalls; a clear commits G=5 meanwhile.
        let stale_epoch = state.read_epoch();
        state.begin_clear();
        state.observe_generation(5, None);
        state.finish_clear();
        state.observe_generation(4, stale_epoch);
        assert_eq!(state.mirror_value(), 5);
        // Even without a clear, an older read never lowers it ...
        state.observe_generation(4, None);
        assert_eq!(state.mirror_value(), 5);
        // ... except when the database itself went back (restored file)
        // and no clear ran during the read.
        let epoch = state.read_epoch();
        state.observe_generation(2, epoch);
        assert_eq!(state.mirror_value(), 2);
        // A read that overlapped an in-flight clear never lowers it.
        state.observe_generation(6, None);
        state.begin_clear();
        assert_eq!(state.read_epoch(), None);
        state.observe_generation(1, state.read_epoch());
        state.finish_clear();
        assert_eq!(state.mirror_value(), 6);
        assert_eq!(state.capture_snapshot().generation, 6);
    }
}
