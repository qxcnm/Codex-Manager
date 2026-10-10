use super::*;

#[test]
fn budget_is_memory_divided_by_32_and_clamped() {
    assert_eq!(queue_budget_for_memory(0), QUEUE_BUDGET_MIN_BYTES);
    assert_eq!(queue_budget_for_memory(512 * MIB), QUEUE_BUDGET_MIN_BYTES);
    // 3.6 GiB machine -> ~115 MiB.
    let small = queue_budget_for_memory(3_865_470_566);
    assert!(small > 110 * MIB && small < 120 * MIB, "{small}");
    assert_eq!(queue_budget_for_memory(16 * GIB), 512 * MIB);
    assert_eq!(queue_budget_for_memory(64 * GIB), QUEUE_BUDGET_MAX_BYTES);
    assert_eq!(queue_budget_for_memory(u64::MAX), QUEUE_BUDGET_MAX_BYTES);
}

#[test]
fn cgroup_limit_caps_physical_memory() {
    let budget = resolve_queue_budget(None, Some(16 * GIB), Some(2 * GIB));
    assert_eq!(budget.source, QueueBudgetSource::CgroupLimit);
    assert_eq!(budget.effective_memory, Some(2 * GIB));
    assert_eq!(budget.bytes, 64 * MIB);

    let budget = resolve_queue_budget(None, Some(8 * GIB), Some(32 * GIB));
    assert_eq!(budget.source, QueueBudgetSource::PhysicalMemory);
    assert_eq!(budget.bytes, 256 * MIB);

    let budget = resolve_queue_budget(None, None, None);
    assert_eq!(budget.source, QueueBudgetSource::Fallback);
    assert_eq!(budget.bytes, QUEUE_BUDGET_MIN_BYTES);
}

#[test]
fn override_wins_and_invalid_override_is_reported() {
    let budget = resolve_queue_budget(Some("200MiB"), Some(16 * GIB), None);
    assert_eq!(budget.source, QueueBudgetSource::Override);
    assert_eq!(budget.bytes, 200 * MIB);

    let budget = resolve_queue_budget(Some("4096"), Some(16 * GIB), None);
    assert_eq!(budget.bytes, QUEUE_BUDGET_OVERRIDE_MIN_BYTES);

    let budget = resolve_queue_budget(Some("lots"), Some(16 * GIB), None);
    assert!(budget.invalid_override);
    assert_eq!(budget.source, QueueBudgetSource::PhysicalMemory);
    assert_eq!(budget.bytes, 512 * MIB);

    let budget = resolve_queue_budget(Some("  "), Some(16 * GIB), None);
    assert!(!budget.invalid_override);
}

#[test]
fn byte_sizes_parse_with_binary_units() {
    assert_eq!(parse_byte_size("134217728"), Some(134_217_728));
    assert_eq!(parse_byte_size("128MiB"), Some(128 * MIB));
    assert_eq!(parse_byte_size("128 mb"), Some(128 * MIB));
    assert_eq!(parse_byte_size("1g"), Some(GIB));
    assert_eq!(parse_byte_size("512k"), Some(512 * 1024));
    assert_eq!(parse_byte_size("MiB"), None);
    assert_eq!(parse_byte_size("12XB"), None);
    assert_eq!(parse_byte_size("99999999999999999999"), None);
}

#[test]
fn cgroup_text_parsing_handles_unlimited_values() {
    assert_eq!(parse_cgroup_memory_limit("max\n"), None);
    assert_eq!(parse_cgroup_memory_limit(""), None);
    assert_eq!(parse_cgroup_memory_limit("9223372036854771712\n"), None);
    assert_eq!(parse_cgroup_memory_limit("0"), None);
    assert_eq!(
        parse_cgroup_memory_limit("2147483648\n"),
        Some(2_147_483_648)
    );
    assert_eq!(parse_cgroup_memory_limit("garbage"), None);
}

#[test]
fn handoff_and_disk_limits() {
    assert_eq!(spill_handoff_budget(115 * MIB), 115 * MIB / 2);
    assert_eq!(spill_handoff_budget(GIB), SPILL_HANDOFF_MAX_BYTES);
    assert_eq!(spill_disk_reserve(10 * GIB), GIB);
    assert_eq!(spill_disk_reserve(100 * GIB), 5 * GIB);
    assert_eq!(spill_dir_cap(20 * GIB), 2 * GIB);
    assert_eq!(spill_dir_cap(500 * GIB), SPILL_DIR_MAX_BYTES);

    assert_eq!(check_spill_space(None, u64::MAX / 2, 1), Ok(()));
    assert_eq!(
        check_spill_space(Some((100 * GIB, 50 * GIB)), 0, MIB),
        Ok(())
    );
    assert_eq!(
        check_spill_space(Some((100 * GIB, 5 * GIB)), 0, MIB),
        Err(SpillSpaceIssue::DiskReserve)
    );
    assert_eq!(
        check_spill_space(Some((100 * GIB, 20 * GIB)), 2 * GIB, MIB),
        Err(SpillSpaceIssue::DirectoryCap)
    );
}
