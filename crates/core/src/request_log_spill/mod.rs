//! Building blocks of the request payload write queue that do not depend on
//! the service: memory budget, spill segment format and files, and the
//! memory/spill ordering state machine. Pure `std` + `crc32fast`.

pub mod budget;
pub mod order;
pub mod record;
pub mod segment;

/// `clear_epoch` value of a job captured while a log clear was in flight.
/// Such a job can never be validated by epoch.
pub const CLEAR_EPOCH_IN_FLIGHT: u64 = u64::MAX;

/// How the writer validates a job against log clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationCheck {
    /// Use the captured generation with the database `*_if_current` guard.
    Known(i64),
    /// The process mirror was not initialized at capture time and no clear
    /// started since: read the generation inside the write transaction.
    ResolveAtWrite,
    /// The job predates a clear (or comes from another process run without
    /// a generation) and must not be written.
    Reject,
}

/// Decide how to validate a job. `generation` is `None` (or negative) when
/// the mirror was unknown at capture time. Epochs only count clears of the
/// current process, so a job without generation from another run (another
/// `boot_id`) is always rejected.
pub fn classify_generation(
    generation: Option<i64>,
    clear_epoch: u64,
    boot_id: u64,
    current_boot_id: u64,
    current_clear_epoch: u64,
) -> GenerationCheck {
    match generation {
        Some(generation) if generation >= 0 => GenerationCheck::Known(generation),
        _ if clear_epoch != CLEAR_EPOCH_IN_FLIGHT
            && boot_id == current_boot_id
            && clear_epoch == current_clear_epoch =>
        {
            GenerationCheck::ResolveAtWrite
        }
        _ => GenerationCheck::Reject,
    }
}

#[cfg(test)]
#[path = "tests/record_tests.rs"]
mod record_tests;
