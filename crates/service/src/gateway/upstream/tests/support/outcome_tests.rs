use super::*;
use codexmanager_core::storage::{now_ts, UsageSnapshotRecord};
use reqwest::header::HeaderValue;

fn exhausted_usage_snapshot(account_id: &str) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: Some(100.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(100.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    }
}

fn low_quota_usage_snapshot(account_id: &str) -> UsageSnapshotRecord {
    UsageSnapshotRecord {
        account_id: account_id.to_string(),
        used_percent: Some(98.0),
        window_minutes: Some(300),
        resets_at: None,
        secondary_used_percent: Some(20.0),
        secondary_window_minutes: Some(10080),
        secondary_resets_at: None,
        credits_json: None,
        captured_at: now_ts(),
    }
}

/// 函数 `official_status_404_with_more_candidates_triggers_failover`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn official_status_404_with_more_candidates_triggers_failover() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-404",
        reqwest::StatusCode::NOT_FOUND,
        None,
        "https://chatgpt.com/backend-api/codex/chat/completions",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::Failover));
}

/// 函数 `official_status_404_on_last_candidate_keeps_upstream_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-07-01
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn official_status_404_on_last_candidate_keeps_upstream_response() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-404-last",
        reqwest::StatusCode::NOT_FOUND,
        None,
        "https://chatgpt.com/backend-api/codex/responses",
        false,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}

/// 函数 `custom_status_404_with_more_candidates_triggers_failover`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn custom_status_404_with_more_candidates_triggers_failover() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-404",
        reqwest::StatusCode::NOT_FOUND,
        None,
        "https://example.com/custom-openai/chat/completions",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::Failover));
}

/// 函数 `official_status_429_with_more_candidates_triggers_failover`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-03
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn official_status_429_with_more_candidates_triggers_failover() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-429",
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        None,
        "https://api.openai.com/v1/responses",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::Failover));
}

/// 函数 `status_429_on_last_candidate_keeps_upstream_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn status_429_on_last_candidate_keeps_upstream_response() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-429",
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        None,
        "https://api.openai.com/v1/responses",
        false,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}

/// 函数 `official_status_401_with_more_candidates_keeps_upstream_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn official_status_401_with_more_candidates_keeps_upstream_response() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-401",
        reqwest::StatusCode::UNAUTHORIZED,
        None,
        "https://chatgpt.com/backend-api/codex/responses",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}

/// 函数 `challenge_with_more_candidates_triggers_failover`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn challenge_with_more_candidates_triggers_failover() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let content_type = HeaderValue::from_static("text/html; charset=utf-8");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-challenge",
        reqwest::StatusCode::FORBIDDEN,
        Some(&content_type),
        "https://chatgpt.com/backend-api/codex/responses",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::Failover));
}

/// 函数 `challenge_on_last_candidate_keeps_upstream_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn challenge_on_last_candidate_keeps_upstream_response() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let content_type = HeaderValue::from_static("text/html; charset=utf-8");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-challenge",
        reqwest::StatusCode::FORBIDDEN,
        Some(&content_type),
        "https://chatgpt.com/backend-api/codex/responses",
        false,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}

/// 函数 `official_status_500_with_more_candidates_keeps_upstream_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn official_status_500_with_more_candidates_keeps_upstream_response() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-500",
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        None,
        "https://chatgpt.com/backend-api/codex/responses",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}

#[test]
fn official_compact_502_with_low_quota_snapshot_triggers_failover() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_usage_snapshot(&low_quota_usage_snapshot("acc-compact-low"))
        .expect("insert usage");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-compact-low",
        reqwest::StatusCode::BAD_GATEWAY,
        None,
        "https://chatgpt.com/backend-api/codex/responses/compact",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::Failover));
}

#[test]
fn official_compact_502_without_low_quota_snapshot_keeps_upstream_response() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-compact-healthy",
        reqwest::StatusCode::BAD_GATEWAY,
        None,
        "https://chatgpt.com/backend-api/codex/responses/compact",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}

/// 函数 `status_500_on_last_candidate_keeps_upstream_response`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn status_500_on_last_candidate_keeps_upstream_response() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-500",
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        None,
        "https://chatgpt.com/backend-api/codex/responses",
        false,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}

/// 函数 `official_usage_exhausted_with_more_candidates_triggers_failover`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn official_usage_exhausted_with_more_candidates_triggers_failover() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_usage_snapshot(&exhausted_usage_snapshot("acc-usage"))
        .expect("insert usage");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-usage",
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        None,
        "https://chatgpt.com/backend-api/codex/responses",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::Failover));
}

/// 函数 `official_usage_exhausted_does_not_override_401`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn official_usage_exhausted_does_not_override_401() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    storage
        .insert_usage_snapshot(&exhausted_usage_snapshot("acc-usage-401"))
        .expect("insert usage");
    let decision = decide_upstream_outcome(
        &storage,
        "acc-usage-401",
        reqwest::StatusCode::UNAUTHORIZED,
        None,
        "https://chatgpt.com/backend-api/codex/responses",
        true,
        |_, _, _| {},
    );
    assert!(matches!(decision, UpstreamOutcomeDecision::RespondUpstream));
}
