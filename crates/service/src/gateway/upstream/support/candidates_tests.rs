use super::{
    allow_openai_fallback_for_account_with_snapshot, candidate_skip_reason_for_proxy,
    CandidateSkipReason,
};
use codexmanager_core::storage::{
    now_ts, Account, ModelSourceMapping, ModelSourceModel, Storage, Token, UsageSnapshotRecord,
};

fn insert_active_account_with_token(storage: &Storage, account_id: &str, sort: i64) {
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: account_id.to_string(),
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
            account_id: account_id.to_string(),
            id_token: "header.payload.sig".to_string(),
            access_token: "header.payload.sig".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    crate::gateway::invalidate_candidate_cache();
}

fn set_account_group(storage: &Storage, account_id: &str, group_name: &str) {
    storage
        .update_account_group_name(account_id, Some(group_name))
        .expect("set account group");
    crate::gateway::invalidate_candidate_cache();
}

fn insert_usage_snapshot(
    storage: &Storage,
    account_id: &str,
    used_percent: f64,
    plan_type: Option<&str>,
) {
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(used_percent),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: None,
            secondary_window_minutes: None,
            secondary_resets_at: None,
            credits_json: plan_type.map(|plan| format!(r#"{{"planType":"{plan}"}}"#)),
            captured_at: now_ts(),
        })
        .expect("insert usage snapshot");
    crate::gateway::invalidate_candidate_cache();
}

fn insert_saturated_usage_snapshot(
    storage: &Storage,
    account_id: &str,
    credits_json: Option<&str>,
) {
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(100.0),
            window_minutes: Some(300),
            resets_at: None,
            secondary_used_percent: Some(100.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: None,
            credits_json: credits_json.map(str::to_string),
            captured_at: now_ts(),
        })
        .expect("insert saturated usage snapshot");
    crate::gateway::invalidate_candidate_cache();
}

struct QuotaGuardReset(crate::gateway::QuotaGuardConfig);

impl Drop for QuotaGuardReset {
    fn drop(&mut self) {
        crate::gateway::set_quota_guard_config(self.0);
    }
}

struct FreeAccountMaxModelReset(String);

impl Drop for FreeAccountMaxModelReset {
    fn drop(&mut self) {
        let _ = crate::gateway::set_free_account_max_model(self.0.as_str());
    }
}

#[test]
fn account_group_filter_limits_initial_and_failover_candidate_pool() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    for (id, sort, group) in [
        ("acc-team-a-first", 0, "team-a"),
        ("acc-team-a-second", 1, " team-a "),
        ("acc-team-b", 2, "team-b"),
    ] {
        insert_active_account_with_token(&storage, id, sort);
        set_account_group(&storage, id, group);
    }

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-5.4"),
        Some("team-a"),
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare restricted candidates");
    assert_eq!(
        candidates
            .iter()
            .map(|(account, _)| account.id.as_str())
            .collect::<Vec<_>>(),
        vec!["acc-team-a-first", "acc-team-a-second"]
    );

    let case_mismatch = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-5.4"),
        Some("TEAM-A"),
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare case-sensitive candidates");
    assert!(case_mismatch.is_empty());
}

#[test]
fn account_group_and_plan_filters_form_an_intersection() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    for (id, sort, group, plan) in [
        ("acc-a-plus", 0, "team-a", "plus"),
        ("acc-a-free", 1, "team-a", "free"),
        ("acc-b-plus", 2, "team-b", "plus"),
    ] {
        insert_active_account_with_token(&storage, id, sort);
        set_account_group(&storage, id, group);
        insert_usage_snapshot(&storage, id, 10.0, Some(plan));
    }

    let candidates = super::prepare_gateway_candidates(
        &storage,
        None,
        Some("team-a"),
        Some("plus"),
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare intersected candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-a-plus");
}

#[test]
fn quota_fallback_is_evaluated_inside_restricted_group() {
    let _guard = crate::test_env_guard();
    let previous = crate::gateway::current_quota_guard_config();
    let _reset = QuotaGuardReset(previous);
    crate::gateway::set_quota_guard_config(crate::gateway::QuotaGuardConfig {
        enabled: true,
        primary_min_remaining_percent: 5.0,
        secondary_min_remaining_percent: 0.0,
        allow_all_low_quota_fallback: true,
    });
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_active_account_with_token(&storage, "acc-team-a-low", 0);
    set_account_group(&storage, "acc-team-a-low", "team-a");
    insert_usage_snapshot(&storage, "acc-team-a-low", 99.0, None);
    insert_active_account_with_token(&storage, "acc-team-b-healthy", 1);
    set_account_group(&storage, "acc-team-b-healthy", "team-b");
    insert_usage_snapshot(&storage, "acc-team-b-healthy", 10.0, None);

    let candidates = super::prepare_gateway_candidates(
        &storage,
        None,
        Some("team-a"),
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare restricted low quota candidates");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-team-a-low");
}

#[test]
fn free_account_model_ceiling_filters_free_accounts_before_quota_selection() {
    let _guard = crate::test_env_guard();
    let _free_model_reset =
        FreeAccountMaxModelReset(crate::gateway::current_free_account_max_model());
    crate::gateway::set_free_account_max_model("gpt-5.6-luna").expect("set free model ceiling");
    let previous_quota_guard = crate::gateway::current_quota_guard_config();
    let _quota_guard_reset = QuotaGuardReset(previous_quota_guard);
    crate::gateway::set_quota_guard_config(crate::gateway::QuotaGuardConfig {
        enabled: true,
        primary_min_remaining_percent: 20.0,
        secondary_min_remaining_percent: 0.0,
        allow_all_low_quota_fallback: true,
    });

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_active_account_with_token(&storage, "acc-free-healthy", 0);
    insert_usage_snapshot(&storage, "acc-free-healthy", 10.0, Some("free"));
    insert_active_account_with_token(&storage, "acc-plus-low", 1);
    insert_usage_snapshot(&storage, "acc-plus-low", 99.0, Some("plus"));

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-6-sol"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates above free model ceiling");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-plus-low");
}

#[test]
fn free_account_model_ceiling_keeps_boundary_model_and_paid_accounts() {
    let _guard = crate::test_env_guard();
    let _free_model_reset =
        FreeAccountMaxModelReset(crate::gateway::current_free_account_max_model());
    crate::gateway::set_free_account_max_model("gpt-5.6-luna").expect("set free model ceiling");
    let previous_quota_guard = crate::gateway::current_quota_guard_config();
    let _quota_guard_reset = QuotaGuardReset(previous_quota_guard);
    crate::gateway::set_quota_guard_config(crate::gateway::QuotaGuardConfig {
        enabled: false,
        primary_min_remaining_percent: 0.0,
        secondary_min_remaining_percent: 0.0,
        allow_all_low_quota_fallback: true,
    });

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_active_account_with_token(&storage, "acc-free-boundary", 0);
    insert_usage_snapshot(&storage, "acc-free-boundary", 10.0, Some("free"));
    insert_active_account_with_token(&storage, "acc-pro-boundary", 1);
    insert_usage_snapshot(&storage, "acc-pro-boundary", 10.0, Some("pro"));

    let boundary_candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-5.6-luna"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates at free model ceiling");
    assert_eq!(boundary_candidates.len(), 2);

    let above_ceiling_candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-6-sol"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates above free model ceiling");
    assert_eq!(above_ceiling_candidates.len(), 1);
    assert_eq!(above_ceiling_candidates[0].0.id, "acc-pro-boundary");
}

#[test]
fn free_account_model_ceiling_treats_reserve_alias_as_luna_boundary() {
    let _guard = crate::test_env_guard();
    let _free_model_reset =
        FreeAccountMaxModelReset(crate::gateway::current_free_account_max_model());
    crate::gateway::set_free_account_max_model(codexmanager_core::usage::LUNA_MODEL_SLUG)
        .expect("set Luna free model ceiling");
    let previous_quota_guard = crate::gateway::current_quota_guard_config();
    let _quota_guard_reset = QuotaGuardReset(previous_quota_guard);
    crate::gateway::set_quota_guard_config(crate::gateway::QuotaGuardConfig {
        enabled: false,
        primary_min_remaining_percent: 0.0,
        secondary_min_remaining_percent: 0.0,
        allow_all_low_quota_fallback: true,
    });

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_active_account_with_token(&storage, "acc-free-reserve", 0);
    insert_usage_snapshot(&storage, "acc-free-reserve", 10.0, Some("free"));
    storage
        .update_account_status("acc-free-reserve", "force_enabled")
        .expect("mark Reserve candidate force enabled");

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some(codexmanager_core::usage::LUNA_RESERVE_MODEL_SLUG),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare Reserve candidates at Luna free model ceiling");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-free-reserve");
}

#[test]
fn free_account_model_ceiling_auto_does_not_filter_unknown_models() {
    let _guard = crate::test_env_guard();
    let _free_model_reset =
        FreeAccountMaxModelReset(crate::gateway::current_free_account_max_model());
    crate::gateway::set_free_account_max_model("auto").expect("disable free model ceiling");
    let previous_quota_guard = crate::gateway::current_quota_guard_config();
    let _quota_guard_reset = QuotaGuardReset(previous_quota_guard);
    crate::gateway::set_quota_guard_config(crate::gateway::QuotaGuardConfig {
        enabled: false,
        primary_min_remaining_percent: 0.0,
        secondary_min_remaining_percent: 0.0,
        allow_all_low_quota_fallback: true,
    });

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_active_account_with_token(&storage, "acc-free-auto", 0);
    insert_usage_snapshot(&storage, "acc-free-auto", 10.0, Some("free"));

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-custom-preview"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates without free model ceiling");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-free-auto");
}

#[test]
fn free_account_model_ceiling_treats_unknown_request_models_as_above_ceiling() {
    let _guard = crate::test_env_guard();
    let _free_model_reset =
        FreeAccountMaxModelReset(crate::gateway::current_free_account_max_model());
    crate::gateway::set_free_account_max_model("gpt-5.6-luna").expect("set free model ceiling");
    let previous_quota_guard = crate::gateway::current_quota_guard_config();
    let _quota_guard_reset = QuotaGuardReset(previous_quota_guard);
    crate::gateway::set_quota_guard_config(crate::gateway::QuotaGuardConfig {
        enabled: false,
        primary_min_remaining_percent: 0.0,
        secondary_min_remaining_percent: 0.0,
        allow_all_low_quota_fallback: true,
    });

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_active_account_with_token(&storage, "acc-free-unknown", 0);
    insert_usage_snapshot(&storage, "acc-free-unknown", 10.0, Some("free"));
    insert_active_account_with_token(&storage, "acc-plus-unknown", 1);
    insert_usage_snapshot(&storage, "acc-plus-unknown", 10.0, Some("plus"));

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-custom-preview"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates for unknown model");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-plus-unknown");
}

#[test]
fn reserve_alias_uses_only_reserve_or_force_enabled_accounts() {
    let _guard = crate::test_env_guard();
    let _free_model_reset =
        FreeAccountMaxModelReset(crate::gateway::current_free_account_max_model());
    crate::gateway::set_free_account_max_model("auto").expect("disable free model ceiling");
    let previous_quota_guard = crate::gateway::current_quota_guard_config();
    let _quota_guard_reset = QuotaGuardReset(previous_quota_guard);
    crate::gateway::set_quota_guard_config(crate::gateway::QuotaGuardConfig {
        enabled: false,
        primary_min_remaining_percent: 0.0,
        secondary_min_remaining_percent: 0.0,
        allow_all_low_quota_fallback: false,
    });

    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_active_account_with_token(&storage, "acc-luna-reserve", 0);
    insert_active_account_with_token(&storage, "acc-force-enabled", 1);
    insert_active_account_with_token(&storage, "acc-exhausted", 2);
    insert_active_account_with_token(&storage, "acc-standard-healthy", 3);
    insert_active_account_with_token(&storage, "acc-expired-reserve", 4);
    storage
        .update_account_status("acc-force-enabled", "force_enabled")
        .expect("mark force enabled");
    insert_saturated_usage_snapshot(
        &storage,
        "acc-luna-reserve",
        Some(
            r#"{"_codexmanager_extra_rate_limits":[{"limit_name":"Luna Reserve","metered_feature":"base_model_inference","primary_window":{"used_percent":10.0}}]}"#,
        ),
    );
    insert_saturated_usage_snapshot(&storage, "acc-force-enabled", None);
    insert_saturated_usage_snapshot(&storage, "acc-exhausted", None);
    insert_usage_snapshot(&storage, "acc-standard-healthy", 10.0, Some("plus"));
    insert_saturated_usage_snapshot(
        &storage,
        "acc-expired-reserve",
        Some(
            r#"{"_codexmanager_extra_rate_limits":[{"limit_name":"Luna Reserve","primary_window":{"remaining_percent":100.0,"reset_at":1}}]}"#,
        ),
    );

    let reserve_candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-reserve"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare Luna Reserve candidates");
    assert_eq!(
        reserve_candidates
            .iter()
            .map(|(account, _)| account.id.as_str())
            .collect::<Vec<_>>(),
        vec!["acc-luna-reserve", "acc-force-enabled"]
    );

    let luna_candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-5.6-luna"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare standard Luna candidates");
    assert_eq!(
        luna_candidates
            .iter()
            .map(|(account, _)| account.id.as_str())
            .collect::<Vec<_>>(),
        vec!["acc-force-enabled", "acc-standard-healthy"]
    );

    let standard_candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-5.4"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare standard candidates");
    assert_eq!(
        standard_candidates
            .iter()
            .map(|(account, _)| account.id.as_str())
            .collect::<Vec<_>>(),
        vec!["acc-force-enabled", "acc-standard-healthy"]
    );
}

fn upsert_account_source_model(storage: &Storage, account_id: &str, upstream_model: &str) {
    let now = now_ts();
    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "openai_account".to_string(),
            source_id: account_id.to_string(),
            upstream_model: upstream_model.to_string(),
            display_name: Some(upstream_model.to_string()),
            status: "available".to_string(),
            discovery_kind: "manual".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account source model");
}

#[test]
fn prepare_gateway_candidates_accepts_direct_upstream_model_without_platform_mapping() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-direct-upstream".to_string(),
            label: "acc-direct-upstream".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");
    storage
        .insert_token(&Token {
            account_id: "acc-direct-upstream".to_string(),
            id_token: "header.payload.sig".to_string(),
            access_token: "header.payload.sig".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "openai_account".to_string(),
            source_id: "acc-direct-upstream".to_string(),
            upstream_model: "gpt-5.4-mini".to_string(),
            display_name: Some("gpt-5.4-mini".to_string()),
            status: "available".to_string(),
            discovery_kind: "manual".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert source model");
    crate::gateway::invalidate_candidate_cache();

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-5.4-mini"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-direct-upstream");
}

#[test]
fn prepare_gateway_candidates_uses_account_pool_despite_legacy_aggregate_mapping() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    insert_active_account_with_token(&storage, "acc-aggregate-owned", 0);
    storage
        .upsert_model_source_model(&ModelSourceModel {
            source_kind: "openai_account".to_string(),
            source_id: "acc-aggregate-owned".to_string(),
            upstream_model: "gpt-aggregate-owned".to_string(),
            display_name: Some("gpt-aggregate-owned".to_string()),
            status: "available".to_string(),
            discovery_kind: "manual".to_string(),
            last_synced_at: Some(now),
            extra_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account source model");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-aggregate-owned".to_string(),
            platform_model_slug: "gpt-aggregate-owned".to_string(),
            source_kind: "aggregate_api".to_string(),
            source_id: "agg-owned".to_string(),
            upstream_model: "gpt-aggregate-owned".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert aggregate mapping");

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-aggregate-owned"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-aggregate-owned");
}

#[test]
fn prepare_gateway_candidates_ignores_legacy_per_account_mapping() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    insert_active_account_with_token(&storage, "acc-explicit-route", 0);
    insert_active_account_with_token(&storage, "acc-other-route", 1);
    upsert_account_source_model(&storage, "acc-explicit-route", "gpt-hybrid-route");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-explicit-account".to_string(),
            platform_model_slug: "gpt-hybrid-route".to_string(),
            source_kind: "openai_account".to_string(),
            source_id: "acc-explicit-route".to_string(),
            upstream_model: "gpt-hybrid-route".to_string(),
            enabled: true,
            priority: 2,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account mapping");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-hybrid-aggregate".to_string(),
            platform_model_slug: "gpt-hybrid-route".to_string(),
            source_kind: "aggregate_api".to_string(),
            source_id: "agg-hybrid".to_string(),
            upstream_model: "gpt-hybrid-route".to_string(),
            enabled: true,
            priority: 1,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert aggregate mapping");

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-hybrid-route"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates");

    assert_eq!(
        candidates
            .into_iter()
            .map(|(account, _token)| account.id)
            .collect::<Vec<_>>(),
        vec![
            "acc-explicit-route".to_string(),
            "acc-other-route".to_string()
        ]
    );
}

#[test]
fn prepare_gateway_candidates_reuses_complete_account_pool_cache() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    insert_active_account_with_token(&storage, "acc-cached-other", 0);
    insert_active_account_with_token(&storage, "acc-mapped-only", 1);
    upsert_account_source_model(&storage, "acc-mapped-only", "gpt-scoped-route");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-scoped-account".to_string(),
            platform_model_slug: "gpt-scoped-route".to_string(),
            source_kind: "openai_account".to_string(),
            source_id: "acc-mapped-only".to_string(),
            upstream_model: "gpt-scoped-route".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account mapping");

    let all_candidates =
        super::super::super::super::collect_gateway_candidates_with_low_quota_mode(
            &storage,
            crate::gateway::LowQuotaCandidateMode::NormalOnly,
        )
        .expect("warm global cache");
    assert_eq!(all_candidates.len(), 2);

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-scoped-route"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare scoped candidates");

    assert_eq!(
        candidates
            .into_iter()
            .map(|(account, _token)| account.id)
            .collect::<Vec<_>>(),
        vec![
            "acc-cached-other".to_string(),
            "acc-mapped-only".to_string()
        ]
    );
}

#[test]
fn prepare_gateway_candidates_does_not_filter_account_pool_by_legacy_mapping() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    insert_active_account_with_token(&storage, "acc-review-only", 0);
    upsert_account_source_model(&storage, "acc-review-only", "codex-auto-review");
    storage
        .upsert_model_source_mapping(&ModelSourceMapping {
            id: "map-review-only".to_string(),
            platform_model_slug: "gpt-5.5".to_string(),
            source_kind: "openai_account".to_string(),
            source_id: "acc-review-only".to_string(),
            upstream_model: "codex-auto-review".to_string(),
            enabled: true,
            priority: 0,
            weight: 1,
            billing_model_slug: None,
            created_at: now,
            updated_at: now,
        })
        .expect("upsert account mapping");

    let candidates = super::prepare_gateway_candidates(
        &storage,
        Some("gpt-5.5"),
        None,
        None,
        crate::gateway::LowQuotaCandidateMode::NormalOnly,
    )
    .expect("prepare candidates");

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0.id, "acc-review-only");
}

/// 函数 `allow_openai_fallback_for_account_accepts_individual_plan_tiers`
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
fn allow_openai_fallback_for_account_accepts_individual_plan_tiers() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let account = Account {
        id: "acc-pro".to_string(),
        label: "acc-pro".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: Some("org-pro".to_string()),
        workspace_id: Some("org-pro".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    };
    storage.insert_account(&account).expect("insert account");
    let token = Token {
        account_id: "acc-pro".to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: {
            let header = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0";
            let payload = "eyJzdWIiOiJhY2MtcHJvIiwiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfcGxhbl90eXBlIjoicHJvIn19";
            format!("{header}.{payload}.sig")
        },
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now,
    };

    assert!(allow_openai_fallback_for_account_with_snapshot(
        &token, None
    ));
}

/// 函数 `allow_openai_fallback_for_account_rejects_workspace_plans`
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
fn allow_openai_fallback_for_account_rejects_workspace_plans() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let now = now_ts();
    let account = Account {
        id: "acc-team".to_string(),
        label: "acc-team".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: Some("org-team".to_string()),
        workspace_id: Some("org-team".to_string()),
        group_name: Some("team".to_string()),
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    };
    storage.insert_account(&account).expect("insert account");
    let token = Token {
        account_id: "acc-team".to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: {
            let header = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0";
            let payload = "eyJzdWIiOiJhY2MtdGVhbSIsImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6eyJjaGF0Z3B0X3BsYW5fdHlwZSI6InRlYW0ifX0";
            format!("{header}.{payload}.sig")
        },
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now,
    };

    assert!(!allow_openai_fallback_for_account_with_snapshot(
        &token, None
    ));
}

#[test]
fn candidate_skip_reason_for_proxy_allows_failover_when_head_account_is_inflight_limited() {
    let _guard = crate::gateway::acquire_account_inflight("acc-preferred");
    let actual = candidate_skip_reason_for_proxy("acc-preferred", 0, 2, 1, false);
    assert_eq!(actual, Some(CandidateSkipReason::Inflight));
}

#[test]
fn candidate_skip_reason_for_proxy_can_skip_last_cooldown_candidate() {
    let account_id = "acc-cooldown-last-skip-test";
    crate::gateway::gateway_mark_account_cooldown_for_status(account_id, 403);

    let default_last = candidate_skip_reason_for_proxy(account_id, 0, 1, 0, false);
    let strict_last = candidate_skip_reason_for_proxy(account_id, 0, 1, 0, true);

    assert_eq!(default_last, None);
    assert_eq!(strict_last, Some(CandidateSkipReason::Cooldown));
}
