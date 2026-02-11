use gpttools_core::storage::{Account, Storage, Token};

pub(crate) fn prepare_gateway_candidates(
    storage: &Storage,
) -> Result<Vec<(Account, Token)>, String> {
    let mut candidates = super::super::collect_gateway_candidates(storage)?;
    // 中文注释：先避开冷却中的账号，再按并发负载排序，减少并发时反复命中不稳定账号。
    candidates.sort_by_key(|(account, _)| {
        (
            super::super::is_account_in_cooldown(&account.id),
            super::super::account_inflight_count(&account.id),
        )
    });
    super::super::rotate_candidates_for_fairness(&mut candidates);
    Ok(candidates)
}
pub(crate) fn should_skip_candidate_for_proxy(
    account_id: &str,
    idx: usize,
    candidate_count: usize,
    account_max_inflight: usize,
) -> bool {
    let has_more_candidates = idx + 1 < candidate_count;
    if super::super::is_account_in_cooldown(account_id) && has_more_candidates {
        super::super::record_gateway_failover_attempt();
        return true;
    }

    if account_max_inflight > 0
        && super::super::account_inflight_count(account_id) >= account_max_inflight
        && has_more_candidates
    {
        // 中文注释：并发上限是软约束，最后一个候选仍要尝试，避免把可恢复抖动直接放大成全局不可用。
        super::super::record_gateway_failover_attempt();
        return true;
    }

    false
}



