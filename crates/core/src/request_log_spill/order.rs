//! Ordering state machine between the in-memory queue and the spill files.
//!
//! Jobs go to memory while the memory budget allows it. The first job that
//! does not fit switches the queue into spilling mode: from then on every
//! job is appended to the spill files (or dropped), even if memory frees up,
//! until the writer has drained the memory queue, every hand-off job is on
//! disk and the reader has consumed everything committed. Only then does the
//! queue switch back to memory. This keeps the global capture order intact,
//! which parent-request detection and prefix sharing rely on.

use super::segment::SpillPos;

/// Why a job was not recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DropReason {
    /// Free space or spill directory cap exhausted.
    DiskFull,
    /// The spill hand-off area is full: the disk cannot keep up.
    DiskSlow,
    /// Encoding, file or database write failed.
    IoError,
    /// The writer is not running and could not be restarted.
    WriterUnavailable,
    /// Another process owns the spill directory.
    SpillLocked,
}

impl DropReason {
    pub const ALL: [DropReason; 5] = [
        DropReason::DiskFull,
        DropReason::DiskSlow,
        DropReason::IoError,
        DropReason::WriterUnavailable,
        DropReason::SpillLocked,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DiskFull => "disk_full",
            Self::DiskSlow => "disk_slow",
            Self::IoError => "io_error",
            Self::WriterUnavailable => "writer_unavailable",
            Self::SpillLocked => "spill_locked",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::DiskFull => 0,
            Self::DiskSlow => 1,
            Self::IoError => 2,
            Self::WriterUnavailable => 3,
            Self::SpillLocked => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Memory,
    Spill,
    Drop(DropReason),
}

/// Byte accounting and mode of the queue. All mutation happens under the
/// caller's (short, IO-free) lock.
#[derive(Debug, Clone, Default)]
pub struct SpillOrderState {
    spilling: bool,
    memory_bytes: u64,
    handoff_bytes: u64,
    handoff_jobs: u64,
    /// Set while spilling cannot accept jobs (locked, no directory, disk
    /// full, write errors).
    spill_blocked: Option<DropReason>,
    committed: SpillPos,
    consumed: SpillPos,
}

impl SpillOrderState {
    /// `start` is the replay start (segments left by an earlier run) and
    /// `committed` the end of the existing data. Leftover data puts the
    /// queue into spilling mode so new jobs are ordered after it.
    pub fn new(start: SpillPos, committed: SpillPos) -> Self {
        Self {
            spilling: start < committed,
            committed,
            consumed: start,
            ..Default::default()
        }
    }

    /// Attach a spill store opened after jobs were already accepted (lazy
    /// start). Accounting is kept; leftover data switches to spilling mode
    /// and spilling is unblocked.
    pub fn attach_spill(&mut self, start: SpillPos, committed: SpillPos) {
        self.committed = committed;
        self.consumed = start;
        if start < committed {
            self.spilling = true;
        }
        self.spill_blocked = None;
    }

    pub fn spilling(&self) -> bool {
        self.spilling
    }

    pub fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }

    pub fn handoff_bytes(&self) -> u64 {
        self.handoff_bytes
    }

    pub fn handoff_jobs(&self) -> u64 {
        self.handoff_jobs
    }

    pub fn committed(&self) -> SpillPos {
        self.committed
    }

    pub fn consumed(&self) -> SpillPos {
        self.consumed
    }

    pub fn spill_blocked(&self) -> Option<DropReason> {
        self.spill_blocked
    }

    pub fn set_spill_blocked(&mut self, reason: Option<DropReason>) {
        self.spill_blocked = reason;
    }

    /// Decide where a job of `size` bytes goes and account for it.
    pub fn route(&mut self, size: u64, memory_budget: u64, handoff_budget: u64) -> RouteDecision {
        if !self.spilling {
            if self.memory_bytes.saturating_add(size) <= memory_budget {
                self.memory_bytes += size;
                return RouteDecision::Memory;
            }
            if let Some(reason) = self.spill_blocked {
                return RouteDecision::Drop(reason);
            }
            self.spilling = true;
        } else if let Some(reason) = self.spill_blocked {
            return RouteDecision::Drop(reason);
        }
        // A single oversized job is accepted when the hand-off area is empty.
        if self.handoff_jobs > 0 && self.handoff_bytes.saturating_add(size) > handoff_budget {
            return RouteDecision::Drop(DropReason::DiskSlow);
        }
        self.handoff_bytes += size;
        self.handoff_jobs += 1;
        RouteDecision::Spill
    }

    /// A memory job was persisted, rejected or discarded.
    pub fn release_memory(&mut self, size: u64) {
        self.memory_bytes = self.memory_bytes.saturating_sub(size);
    }

    /// A hand-off job left memory: appended (`committed` is the new
    /// readable end when known) or dropped.
    pub fn finish_handoff(&mut self, size: u64, committed: Option<SpillPos>) {
        self.handoff_bytes = self.handoff_bytes.saturating_sub(size);
        self.handoff_jobs = self.handoff_jobs.saturating_sub(1);
        if let Some(committed) = committed {
            self.set_committed(committed);
        }
    }

    pub fn set_committed(&mut self, committed: SpillPos) {
        if committed > self.committed {
            self.committed = committed;
        }
    }

    /// The reader consumed everything before `position`.
    pub fn set_consumed(&mut self, position: SpillPos) {
        if position > self.consumed {
            self.consumed = position;
        }
    }

    /// Unread spilled data exists.
    pub fn has_disk_backlog(&self) -> bool {
        self.consumed < self.committed
    }

    /// Switch back to memory once nothing is pending on the spill side.
    /// The caller must only call this after the memory queue was drained
    /// up to the first spilled job (which is always true while spilling,
    /// because spilling jobs never enter the memory queue).
    pub fn try_resume_memory(&mut self) -> bool {
        if self.spilling && self.handoff_jobs == 0 && !self.has_disk_backlog() {
            self.spilling = false;
            return true;
        }
        false
    }

    /// Log clear: the spill files were deleted and the stream restarts at
    /// `position`. Discarded queued jobs are released individually through
    /// [`Self::release_memory`] / [`Self::finish_handoff`].
    pub fn skip_stream_to(&mut self, position: SpillPos) {
        self.set_committed(position);
        self.set_consumed(position);
        self.try_resume_memory();
    }
}

#[cfg(test)]
#[path = "tests/order_tests.rs"]
mod tests;
