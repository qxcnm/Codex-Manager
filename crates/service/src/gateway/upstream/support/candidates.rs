use codexmanager_core::storage::{Account, Storage, Token, UsageSnapshotRecord};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum CandidateSkipReason {
    Cooldown,
    Inflight,
}

fn account_source_ids_for_model(storage: &Storage, model: &str) -> Result<HashSet<String>, String> {
    storage
        .list_available_source_model_ids_by_upstream_model("openai_account", model)
        .map(|ids| ids.into_iter().collect())
        .map_err(|err| format!("list source models by upstream model failed: {err}"))
}

/// 函数 `prepare_gateway_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn prepare_gateway_candidates(
    storage: &Storage,
    request_model: Option<&str>,
    account_plan_filter: Option<&str>,
    low_quota_mode: super::super::super::LowQuotaCandidateMode,
) -> Result<Vec<(Account, Token)>, String> {
    let normalized_model = request_model
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let account_source_ids = if let Some(model) = normalized_model {
        let _ = crate::apikey_models::bootstrap_account_pool_model_routes(storage, false);
        let source_ids = account_source_ids_for_model(storage, model)?;
        Some(source_ids.into_iter().collect::<Vec<_>>())
    } else {
        None
    };
    // 中文注释：保持账号原始顺序（按账户排序字段）作为候选顺序，失败时再依次切下一个。
    let mut candidates = if let Some(account_source_ids) = account_source_ids.as_deref() {
        super::super::super::collect_gateway_candidates_for_accounts_with_low_quota_mode(
            storage,
            account_source_ids,
            low_quota_mode,
        )?
    } else {
        super::super::super::collect_gateway_candidates_with_low_quota_mode(
            storage,
            low_quota_mode,
        )?
    };
    // A profile-bound account must never leak to the global proxy or direct
    // connection when its configured exit is disabled or unhealthy.
    candidates.retain(|(account, _)| !crate::proxy_profiles::account_is_proxy_blocked(&account.id));
    let normalized_filter = account_plan_filter
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"));
    if let Some(plan_filter) = normalized_filter {
        let account_ids = candidates
            .iter()
            .map(|(account, _)| account.id.clone())
            .collect::<Vec<_>>();
        let snapshots = storage
            .latest_usage_snapshots_for_accounts(&account_ids)
            .map_err(|err| format!("list account usage snapshots failed: {err}"))?
            .into_iter()
            .map(|snapshot| (snapshot.account_id.clone(), snapshot))
            .collect::<HashMap<_, _>>();
        candidates.retain(|(account, token)| {
            crate::account_plan::account_matches_plan_filter_with_snapshot(
                token,
                snapshots.get(account.id.as_str()),
                Some(plan_filter),
            )
        });
    }
    let model_account_ids = account_source_ids
        .as_ref()
        .map(|ids| ids.iter().cloned().collect::<HashSet<_>>());
    super::super::super::account_batch_rotation::apply_account_batch_rotation(
        storage,
        normalized_model,
        model_account_ids.as_ref(),
        &mut candidates,
    )?;
    Ok(candidates)
}

/// 函数 `allow_openai_fallback_for_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-03
///
/// # 参数
/// - storage: 参数 storage
/// - account: 参数 account
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn allow_openai_fallback_for_account_with_snapshot(
    token: &Token,
    snapshot: Option<&UsageSnapshotRecord>,
) -> bool {
    if let Some(plan) = crate::account_plan::resolve_token_account_plan(token) {
        return matches!(plan.normalized.as_str(), "free" | "go" | "plus" | "pro");
    }

    let token_plan = crate::account_plan::token_plan_from_token(token);
    let Some(plan) = crate::account_plan::resolve_account_plan(Some(&token_plan), snapshot) else {
        return false;
    };
    matches!(plan.normalized.as_str(), "free" | "go" | "plus" | "pro")
}

/// 函数 `candidate_skip_reason_for_proxy`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn candidate_skip_reason_for_proxy(
    account_id: &str,
    idx: usize,
    candidate_count: usize,
    account_max_inflight: usize,
    skip_last_cooldown: bool,
) -> Option<CandidateSkipReason> {
    let has_more_candidates = idx + 1 < candidate_count;
    if super::super::super::is_account_in_cooldown(account_id)
        && (has_more_candidates || skip_last_cooldown)
    {
        super::super::super::record_gateway_candidate_skip(
            super::super::super::GatewayCandidateSkipReason::Cooldown,
        );
        return Some(CandidateSkipReason::Cooldown);
    }

    if account_max_inflight > 0
        && super::super::super::account_inflight_count(account_id) >= account_max_inflight
        && has_more_candidates
    {
        // 中文注释：并发上限是软约束，最后一个候选仍要尝试，避免把可恢复抖动直接放大成全局不可用。
        super::super::super::record_gateway_candidate_skip(
            super::super::super::GatewayCandidateSkipReason::Inflight,
        );
        return Some(CandidateSkipReason::Inflight);
    }

    None
}

#[cfg(test)]
#[path = "candidates_tests.rs"]
mod tests;
