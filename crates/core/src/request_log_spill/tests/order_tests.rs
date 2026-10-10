use super::*;

const BUDGET: u64 = 100;
const HANDOFF: u64 = 50;

#[test]
fn jobs_stay_in_memory_until_budget_is_exhausted() {
    let mut state = SpillOrderState::new(SpillPos::new(1, 0), SpillPos::new(1, 0));
    assert!(!state.spilling());
    assert_eq!(state.route(60, BUDGET, HANDOFF), RouteDecision::Memory);
    assert_eq!(state.route(40, BUDGET, HANDOFF), RouteDecision::Memory);
    assert_eq!(state.memory_bytes(), 100);
    assert_eq!(state.route(1, BUDGET, HANDOFF), RouteDecision::Spill);
    assert!(state.spilling());
    // Memory frees up, but later jobs must still follow the spilled one.
    state.release_memory(100);
    assert_eq!(state.route(10, BUDGET, HANDOFF), RouteDecision::Spill);
    assert_eq!(state.handoff_jobs(), 2);
}

#[test]
fn oversized_job_spills_and_full_handoff_drops_as_disk_slow() {
    let mut state = SpillOrderState::new(SpillPos::new(1, 0), SpillPos::new(1, 0));
    assert_eq!(state.route(500, BUDGET, HANDOFF), RouteDecision::Spill);
    assert_eq!(
        state.route(10, BUDGET, HANDOFF),
        RouteDecision::Drop(DropReason::DiskSlow)
    );
    state.finish_handoff(500, Some(SpillPos::new(1, 520)));
    assert_eq!(state.route(10, BUDGET, HANDOFF), RouteDecision::Spill);
}

#[test]
fn queue_returns_to_memory_only_after_spill_backlog_is_consumed() {
    let mut state = SpillOrderState::new(SpillPos::new(1, 0), SpillPos::new(1, 0));
    state.route(100, BUDGET, HANDOFF);
    assert_eq!(state.route(30, BUDGET, HANDOFF), RouteDecision::Spill);
    state.release_memory(100);
    assert!(!state.try_resume_memory(), "hand-off still pending");
    state.finish_handoff(30, Some(SpillPos::new(1, 50)));
    assert!(state.has_disk_backlog());
    assert!(!state.try_resume_memory(), "disk backlog not consumed");
    state.set_consumed(SpillPos::new(1, 20));
    assert!(!state.try_resume_memory());
    state.set_consumed(SpillPos::new(1, 50));
    assert!(state.try_resume_memory());
    assert_eq!(state.route(30, BUDGET, HANDOFF), RouteDecision::Memory);
}

#[test]
fn leftover_segments_start_in_spilling_mode() {
    let mut state = SpillOrderState::new(SpillPos::new(3, 0), SpillPos::new(5, 900));
    assert!(state.spilling());
    assert_eq!(state.route(1, BUDGET, HANDOFF), RouteDecision::Spill);
    state.finish_handoff(1, Some(SpillPos::new(6, 40)));
    state.set_consumed(SpillPos::new(5, 900));
    assert!(!state.try_resume_memory());
    state.set_consumed(SpillPos::new(6, 40));
    assert!(state.try_resume_memory());
}

#[test]
fn blocked_spill_drops_with_reason_and_keeps_order() {
    let mut state = SpillOrderState::new(SpillPos::new(1, 0), SpillPos::new(1, 0));
    state.set_spill_blocked(Some(DropReason::SpillLocked));
    assert_eq!(state.route(50, BUDGET, HANDOFF), RouteDecision::Memory);
    assert_eq!(
        state.route(60, BUDGET, HANDOFF),
        RouteDecision::Drop(DropReason::SpillLocked)
    );
    assert!(!state.spilling(), "locked spill never enters spilling mode");
    assert_eq!(state.route(50, BUDGET, HANDOFF), RouteDecision::Memory);

    let mut state = SpillOrderState::new(SpillPos::new(1, 0), SpillPos::new(1, 0));
    state.route(100, BUDGET, HANDOFF);
    assert_eq!(state.route(10, BUDGET, HANDOFF), RouteDecision::Spill);
    state.set_spill_blocked(Some(DropReason::DiskFull));
    state.release_memory(100);
    // Still spilling: a memory job now would overtake the spilled one.
    assert_eq!(
        state.route(10, BUDGET, HANDOFF),
        RouteDecision::Drop(DropReason::DiskFull)
    );
}

#[test]
fn purge_skips_the_stream_and_resumes_memory() {
    let mut state = SpillOrderState::new(SpillPos::new(1, 0), SpillPos::new(4, 10));
    assert!(state.spilling());
    state.skip_stream_to(SpillPos::new(5, 0));
    assert!(!state.spilling());
    assert!(!state.has_disk_backlog());
    assert_eq!(state.route(10, BUDGET, HANDOFF), RouteDecision::Memory);
}

#[test]
fn drop_reason_names_are_stable() {
    let names: Vec<&str> = DropReason::ALL
        .iter()
        .map(|reason| reason.as_str())
        .collect();
    assert_eq!(
        names,
        vec![
            "disk_full",
            "disk_slow",
            "io_error",
            "writer_unavailable",
            "spill_locked"
        ]
    );
    for (index, reason) in DropReason::ALL.iter().enumerate() {
        assert_eq!(reason.index(), index);
    }
}

#[test]
fn attaching_a_store_keeps_accounting_and_unblocks_spilling() {
    let mut state = SpillOrderState::default();
    state.set_spill_blocked(Some(DropReason::IoError));
    assert_eq!(state.route(80, BUDGET, HANDOFF), RouteDecision::Memory);
    assert_eq!(
        state.route(80, BUDGET, HANDOFF),
        RouteDecision::Drop(DropReason::IoError)
    );
    state.attach_spill(SpillPos::new(2, 0), SpillPos::new(3, 64));
    assert_eq!(state.memory_bytes(), 80);
    assert!(state.spilling(), "leftover segments are replayed first");
    assert_eq!(state.route(5, BUDGET, HANDOFF), RouteDecision::Spill);
}

#[test]
fn releasing_a_failed_handoff_job_lets_the_queue_return_to_memory() {
    let mut state = SpillOrderState::default();
    assert_eq!(state.route(BUDGET, BUDGET, HANDOFF), RouteDecision::Memory);
    assert_eq!(state.route(10, BUDGET, HANDOFF), RouteDecision::Spill);
    state.release_memory(BUDGET);
    assert!(
        !state.try_resume_memory(),
        "the hand-off job is still pending"
    );
    // The spill thread failed (or panicked) on that job: its guard
    // releases the accounting without appending anything.
    state.finish_handoff(10, None);
    assert_eq!(state.handoff_jobs(), 0);
    assert!(state.try_resume_memory());
    assert_eq!(state.route(10, BUDGET, HANDOFF), RouteDecision::Memory);
}
