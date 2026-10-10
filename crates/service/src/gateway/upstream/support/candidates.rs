use codexmanager_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};
use codexmanager_core::usage::{has_usable_luna_reserve, is_luna_reserve_model};
use std::collections::HashMap;

use crate::usage_account_meta::{derive_account_meta, patch_account_meta_in_place};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum CandidateSkipReason {
    Cooldown,
    Inflight,
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
    account_group_filter: Option<&str>,
    account_plan_filter: Option<&str>,
    low_quota_mode: super::super::super::LowQuotaCandidateMode,
) -> Result<Vec<(Account, Token)>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    let normalized_group_filter = account_group_filter
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let normalized_plan_filter = account_plan_filter
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"));
    let exclude_free_accounts = request_exceeds_free_account_model_ceiling(storage, request_model)?;
    let reserve_model = is_luna_reserve_model(request_model);

    // 中文注释：未受限的 Key 继续复用全局缓存；受限 Key 必须先形成 group + plan
    // 的授权交集，再在交集内执行额度保护，避免组外账号影响组内低额度兜底。
    if normalized_group_filter.is_none()
        && normalized_plan_filter.is_none()
        && !exclude_free_accounts
        && !reserve_model
    {
        return super::super::super::collect_gateway_candidates_with_low_quota_mode(
            storage,
            low_quota_mode,
        );
    }

    let mut authorized_candidates = if reserve_model {
        storage.list_gateway_candidates_unfiltered()
    } else {
        storage.list_gateway_candidates()
    }
    .map_err(|err| format!("list gateway candidates failed: {err}"))?;
    if let Some(group_filter) = normalized_group_filter {
        authorized_candidates.retain(|(account, _)| {
            crate::account_group::account_matches_group_filter(account, Some(group_filter))
        });
    }

    let mut snapshots = HashMap::new();
    if reserve_model || normalized_plan_filter.is_some() || exclude_free_accounts {
        let account_ids = authorized_candidates
            .iter()
            .map(|(account, _)| account.id.clone())
            .collect::<Vec<_>>();
        snapshots = storage
            .latest_usage_snapshots_for_accounts(&account_ids)
            .map_err(|err| format!("list account usage snapshots failed: {err}"))?
            .into_iter()
            .map(|snapshot| (snapshot.account_id.clone(), snapshot))
            .collect::<HashMap<_, _>>();
        if let Some(plan_filter) = normalized_plan_filter {
            authorized_candidates.retain(|(account, token)| {
                crate::account_plan::account_matches_plan_filter_with_snapshot(
                    token,
                    snapshots.get(account.id.as_str()),
                    Some(plan_filter),
                )
            });
        }

        if exclude_free_accounts {
            let subscriptions = storage
                .list_account_subscriptions_for_accounts(&account_ids)
                .map_err(|err| format!("list account subscriptions failed: {err}"))?
                .into_iter()
                .map(|subscription| (subscription.account_id.clone(), subscription))
                .collect::<HashMap<_, _>>();
            authorized_candidates.retain(|(account, token)| {
                let token_plan = crate::account_plan::token_plan_from_token(token);
                !crate::account_plan::resolve_effective_account_plan(
                    Some(&token_plan),
                    snapshots.get(account.id.as_str()),
                    subscriptions.get(account.id.as_str()),
                )
                .is_some_and(|plan| plan.normalized == "free")
            });
        }
    }

    let authorized_account_ids = authorized_candidates
        .into_iter()
        .map(|(account, _)| account.id)
        .collect::<Vec<_>>();
    // 中文注释：保持账号原始顺序（按账户排序字段）作为候选顺序，失败时再依次切下一个。
    if reserve_model {
        return collect_luna_reserve_candidates(storage, &authorized_account_ids, &snapshots);
    }
    super::super::super::collect_gateway_candidates_for_account_ids_with_low_quota_mode(
        storage,
        &authorized_account_ids,
        low_quota_mode,
    )
}

fn collect_luna_reserve_candidates(
    storage: &Storage,
    account_ids: &[String],
    snapshots: &HashMap<String, UsageSnapshotRecord>,
) -> Result<Vec<(Account, Token)>, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(storage);
    if account_ids.is_empty() {
        return Ok(Vec::new());
    }

    let candidates = storage
        .list_gateway_candidates_unfiltered_for_accounts(account_ids)
        .map_err(|err| format!("list Luna Reserve candidates failed: {err}"))?;
    let mut out = Vec::with_capacity(candidates.len());
    for (account, token) in candidates {
        let force_enabled = account.status.trim().eq_ignore_ascii_case("force_enabled");
        let reserve_available = snapshots
            .get(account.id.as_str())
            .and_then(|snapshot| snapshot.credits_json.as_deref())
            .map(|credits| has_usable_luna_reserve(Some(credits)))
            .unwrap_or(false);
        if !force_enabled && !reserve_available {
            continue;
        }

        let mut candidate_account = account;
        let (chatgpt_account_id, workspace_id) = derive_account_meta(&token);
        if patch_account_meta_in_place(&mut candidate_account, chatgpt_account_id, workspace_id) {
            candidate_account.updated_at = now_ts();
            let _ = storage.update_account_workspace_identity(
                &candidate_account.id,
                candidate_account.chatgpt_account_id.as_deref(),
                candidate_account.workspace_id.as_deref(),
                candidate_account.updated_at,
            );
        }
        out.push((candidate_account, token));
    }
    Ok(out)
}

fn request_exceeds_free_account_model_ceiling(
    storage: &Storage,
    request_model: Option<&str>,
) -> Result<bool, String> {
    let configured = super::super::super::current_free_account_max_model();
    crate::models_v2::request_exceeds_model_ceiling(storage, request_model, &configured)
        .map_err(|err| format!("read free account model ceiling failed: {err}"))
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
