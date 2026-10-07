//! Memory budget of the request payload write queue.
//!
//! The budget bounds the total body bytes of every job that was accepted by
//! the gateway hot path but is not yet persisted. It is derived from the
//! effective memory of the process (physical memory, capped by a container
//! cgroup limit) and can be overridden by an environment variable.

pub const MIB: u64 = 1024 * 1024;
pub const GIB: u64 = 1024 * MIB;

/// Lower bound of the computed budget.
pub const QUEUE_BUDGET_MIN_BYTES: u64 = 32 * MIB;
/// Upper bound of the computed budget.
pub const QUEUE_BUDGET_MAX_BYTES: u64 = GIB;
/// `budget = effective_memory / divisor` before clamping.
pub const QUEUE_BUDGET_MEMORY_DIVISOR: u64 = 32;
/// Smallest accepted explicit override.
pub const QUEUE_BUDGET_OVERRIDE_MIN_BYTES: u64 = MIB;
/// Upper bound of the spill hand-off area (jobs waiting for the spill thread).
pub const SPILL_HANDOFF_MAX_BYTES: u64 = 256 * MIB;
/// Minimum free space that must remain on the data disk.
pub const SPILL_DISK_RESERVE_MIN_BYTES: u64 = GIB;
/// Maximum size of the spill directory.
pub const SPILL_DIR_MAX_BYTES: u64 = 8 * GIB;

/// Where the final budget came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueBudgetSource {
    /// Explicit environment override.
    Override,
    /// Physical memory was the smallest known limit.
    PhysicalMemory,
    /// A container cgroup limit was smaller than physical memory.
    CgroupLimit,
    /// No memory information was available; the minimum budget is used.
    Fallback,
}

impl QueueBudgetSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::PhysicalMemory => "physical_memory",
            Self::CgroupLimit => "cgroup_limit",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueBudget {
    pub bytes: u64,
    pub effective_memory: Option<u64>,
    pub source: QueueBudgetSource,
    /// True when an override was present but could not be parsed.
    pub invalid_override: bool,
}

/// `min(physical, cgroup)` over the known, non-zero values.
pub fn effective_memory(
    physical: Option<u64>,
    cgroup: Option<u64>,
) -> Option<(u64, QueueBudgetSource)> {
    let physical = physical.filter(|value| *value > 0);
    let cgroup = cgroup.filter(|value| *value > 0);
    match (physical, cgroup) {
        (Some(physical), Some(cgroup)) if cgroup < physical => {
            Some((cgroup, QueueBudgetSource::CgroupLimit))
        }
        (Some(physical), _) => Some((physical, QueueBudgetSource::PhysicalMemory)),
        (None, Some(cgroup)) => Some((cgroup, QueueBudgetSource::CgroupLimit)),
        (None, None) => None,
    }
}

/// `clamp(effective_memory / 32, 32 MiB, 1 GiB)`.
pub fn queue_budget_for_memory(effective_memory: u64) -> u64 {
    (effective_memory / QUEUE_BUDGET_MEMORY_DIVISOR)
        .clamp(QUEUE_BUDGET_MIN_BYTES, QUEUE_BUDGET_MAX_BYTES)
}

/// Resolve the queue budget. A valid override always wins (it is only
/// raised to [`QUEUE_BUDGET_OVERRIDE_MIN_BYTES`]); an invalid or empty
/// override is ignored and reported through `invalid_override`.
pub fn resolve_queue_budget(
    override_value: Option<&str>,
    physical: Option<u64>,
    cgroup: Option<u64>,
) -> QueueBudget {
    let effective = effective_memory(physical, cgroup);
    let mut invalid_override = false;
    if let Some(raw) = override_value.map(str::trim).filter(|raw| !raw.is_empty()) {
        match parse_byte_size(raw) {
            Some(bytes) if bytes > 0 => {
                return QueueBudget {
                    bytes: bytes.max(QUEUE_BUDGET_OVERRIDE_MIN_BYTES),
                    effective_memory: effective.map(|(value, _)| value),
                    source: QueueBudgetSource::Override,
                    invalid_override: false,
                };
            }
            _ => invalid_override = true,
        }
    }
    match effective {
        Some((memory, source)) => QueueBudget {
            bytes: queue_budget_for_memory(memory),
            effective_memory: Some(memory),
            source,
            invalid_override,
        },
        None => QueueBudget {
            bytes: QUEUE_BUDGET_MIN_BYTES,
            effective_memory: None,
            source: QueueBudgetSource::Fallback,
            invalid_override,
        },
    }
}

/// Byte cap of the spill hand-off area: `min(budget / 2, 256 MiB)`.
pub fn spill_handoff_budget(queue_budget: u64) -> u64 {
    (queue_budget / 2).clamp(1, SPILL_HANDOFF_MAX_BYTES)
}

/// Free space that must stay available on the data disk:
/// `max(1 GiB, 5% of the disk)`.
pub fn spill_disk_reserve(total_space: u64) -> u64 {
    SPILL_DISK_RESERVE_MIN_BYTES.max(total_space / 20)
}

/// Maximum spill directory size: `min(10% of the free space, 8 GiB)`.
pub fn spill_dir_cap(available_space: u64) -> u64 {
    (available_space / 10).min(SPILL_DIR_MAX_BYTES)
}

/// Why a spill write must not happen right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpillSpaceIssue {
    /// Free space would drop below the reserve.
    DiskReserve,
    /// The spill directory would exceed its cap.
    DirectoryCap,
}

/// Check the disk space rules for one more record of `incoming` bytes.
/// `None` disk information means "unknown" and is allowed (the write
/// itself still fails cleanly when the disk is really full).
pub fn check_spill_space(
    disk: Option<(u64, u64)>,
    dir_bytes: u64,
    incoming: u64,
) -> Result<(), SpillSpaceIssue> {
    let Some((total_space, available_space)) = disk else {
        return Ok(());
    };
    let reserve = spill_disk_reserve(total_space);
    if available_space < reserve.saturating_add(incoming) {
        return Err(SpillSpaceIssue::DiskReserve);
    }
    if dir_bytes.saturating_add(incoming) > spill_dir_cap(available_space) {
        return Err(SpillSpaceIssue::DirectoryCap);
    }
    Ok(())
}

/// Parse `134217728`, `128MiB`, `128MB`, `128M`, `1GiB`, `512k`, ...
/// Units are binary (`MB` == `MiB`).
pub fn parse_byte_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    if digits.is_empty() {
        return None;
    }
    let value = digits.parse::<u64>().ok()?;
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => MIB,
        "g" | "gb" | "gib" => GIB,
        _ => return None,
    };
    value.checked_mul(multiplier)
}

/// Parse a cgroup memory limit file (`memory.max` of cgroup v2 or
/// `memory.limit_in_bytes` of cgroup v1). `max`, empty text and the v1
/// "unlimited" sentinel (close to `i64::MAX`) mean no limit.
pub fn parse_cgroup_memory_limit(text: &str) -> Option<u64> {
    let value = text.trim();
    if value.is_empty() || value == "max" {
        return None;
    }
    let limit = value.parse::<u64>().ok()?;
    if limit == 0 || limit >= (1_u64 << 60) {
        return None;
    }
    Some(limit)
}

#[cfg(test)]
#[path = "tests/budget_tests.rs"]
mod tests;
