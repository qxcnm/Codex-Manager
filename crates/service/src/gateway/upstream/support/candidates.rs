use codexmanager_core::storage::{Account, Storage, Token};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum CandidateSkipReason {
    Cooldown,
    Inflight,
}

pub(crate) fn prepare_gateway_candidates(
    storage: &Storage,
    request_model: Option<&str>,
) -> Result<Vec<(Account, Token)>, String> {
    // 中文注释：保持账号原始顺序（按账户排序字段）作为候选顺序，失败时再依次切下一个。
    let candidates = super::super::super::collect_gateway_candidates(storage)?;
    Ok(filter_free_account_candidates(
        storage,
        candidates,
        request_model,
        super::super::super::current_free_account_max_model().as_str(),
    ))
}

pub(in super::super) fn candidate_skip_reason_for_proxy(
    account_id: &str,
    idx: usize,
    candidate_count: usize,
    account_max_inflight: usize,
) -> Option<CandidateSkipReason> {
    // 中文注释：当用户手动“切到当前”后，首候选应持续优先命中；
    // 仅在真实请求失败时由上游流程自动清除手动锁定，再回退常规轮转。
    let is_manual_preferred_head = idx == 0
        && super::super::super::manual_preferred_account()
            .as_deref()
            .is_some_and(|manual_id| manual_id == account_id);
    if is_manual_preferred_head {
        return None;
    }

    let has_more_candidates = idx + 1 < candidate_count;
    if super::super::super::is_account_in_cooldown(account_id) && has_more_candidates {
        super::super::super::record_gateway_failover_attempt();
        return Some(CandidateSkipReason::Cooldown);
    }

    if account_max_inflight > 0
        && super::super::super::account_inflight_count(account_id) >= account_max_inflight
        && has_more_candidates
    {
        // 中文注释：并发上限是软约束，最后一个候选仍要尝试，避免把可恢复抖动直接放大成全局不可用。
        super::super::super::record_gateway_failover_attempt();
        return Some(CandidateSkipReason::Inflight);
    }

    None
}

fn filter_free_account_candidates(
    storage: &Storage,
    candidates: Vec<(Account, Token)>,
    request_model: Option<&str>,
    free_account_max_model: &str,
) -> Vec<(Account, Token)> {
    let Some(request_model) = request_model
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return candidates;
    };

    candidates
        .into_iter()
        .filter(|(account, token)| {
            !is_free_account_candidate(storage, account.id.as_str(), token)
                || request_model_supported_by_free_limit(request_model, free_account_max_model)
        })
        .collect()
}

fn is_free_account_candidate(storage: &Storage, account_id: &str, token: &Token) -> bool {
    if crate::account_plan::is_free_plan_type(
        crate::account_plan::extract_plan_type_from_id_token(&token.id_token).as_deref(),
    ) || crate::account_plan::is_free_plan_type(
        crate::account_plan::extract_plan_type_from_id_token(&token.access_token).as_deref(),
    ) {
        return true;
    }

    storage
        .latest_usage_snapshot_for_account(account_id)
        .ok()
        .flatten()
        .map(|snapshot| {
            crate::account_plan::is_free_plan_from_credits_json(snapshot.credits_json.as_deref())
        })
        .unwrap_or(false)
}

fn request_model_supported_by_free_limit(request_model: &str, max_model: &str) -> bool {
    if request_model.trim().eq_ignore_ascii_case(max_model.trim()) {
        return true;
    }

    match (
        parse_model_version(request_model),
        parse_model_version(max_model),
    ) {
        (Some(request_version), Some(max_version)) => {
            compare_model_versions(request_version.as_slice(), max_version.as_slice())
                != Ordering::Greater
        }
        _ => false,
    }
}

fn parse_model_version(model: &str) -> Option<Vec<u32>> {
    let lower = model.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("gpt-")?;
    let version_text = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();
    if version_text.is_empty() {
        return None;
    }
    let parts = version_text
        .split('.')
        .map(str::trim)
        .map(|part| part.parse::<u32>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() {
        return None;
    }
    Some(parts)
}

fn compare_model_versions(left: &[u32], right: &[u32]) -> Ordering {
    let width = left.len().max(right.len());
    for idx in 0..width {
        let left_part = *left.get(idx).unwrap_or(&0);
        let right_part = *right.get(idx).unwrap_or(&0);
        match left_part.cmp(&right_part) {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::{filter_free_account_candidates, request_model_supported_by_free_limit};
    use codexmanager_core::storage::{now_ts, Account, Storage, Token, UsageSnapshotRecord};

    #[test]
    fn free_account_model_limit_accepts_same_or_lower_versions() {
        assert!(request_model_supported_by_free_limit("gpt-5.2", "gpt-5.2"));
        assert!(request_model_supported_by_free_limit(
            "gpt-5.2-codex",
            "gpt-5.2"
        ));
        assert!(request_model_supported_by_free_limit("gpt-4.1", "gpt-5.2"));
        assert!(!request_model_supported_by_free_limit(
            "gpt-5.3-codex",
            "gpt-5.2"
        ));
        assert!(!request_model_supported_by_free_limit("o3", "gpt-5.2"));
    }

    #[test]
    fn filter_free_account_candidates_excludes_free_accounts_above_limit() {
        let storage = Storage::open_in_memory().expect("open");
        storage.init().expect("init");
        let now = now_ts();
        for (id, sort, credits_json) in [
            ("acc-pro", 0_i64, None),
            ("acc-free", 1_i64, Some(r#"{"planType":"free"}"#)),
        ] {
            storage
                .insert_account(&Account {
                    id: id.to_string(),
                    label: id.to_string(),
                    issuer: "issuer".to_string(),
                    chatgpt_account_id: None,
                    workspace_id: None,
                    group_name: None,
                    sort,
                    status: "active".to_string(),
                    created_at: now,
                    updated_at: now,
                })
                .expect("insert account");
            storage
                .insert_token(&Token {
                    account_id: id.to_string(),
                    id_token: "header.payload.sig".to_string(),
                    access_token: "header.payload.sig".to_string(),
                    refresh_token: "refresh".to_string(),
                    api_key_access_token: None,
                    last_refresh: now,
                })
                .expect("insert token");
            storage
                .insert_usage_snapshot(&UsageSnapshotRecord {
                    account_id: id.to_string(),
                    used_percent: Some(10.0),
                    window_minutes: Some(300),
                    resets_at: None,
                    secondary_used_percent: Some(20.0),
                    secondary_window_minutes: Some(10_080),
                    secondary_resets_at: None,
                    credits_json: credits_json.map(str::to_string),
                    captured_at: now,
                })
                .expect("insert usage");
        }

        let candidates = storage.list_gateway_candidates().expect("list candidates");
        let filtered =
            filter_free_account_candidates(&storage, candidates, Some("gpt-5.4"), "gpt-5.2");
        let ids = filtered
            .into_iter()
            .map(|(account, _)| account.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["acc-pro"]);
    }
}
