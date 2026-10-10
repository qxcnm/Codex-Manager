use super::{
    build_warmup_headers, consume_warmup_stream, finish_warmup_attempt,
    resolve_reset_warmup_target, resolve_target_accounts, resolve_warmup_model_slug_with_ceiling,
    should_retry_warmup_with_refresh, summarize_warmup_error, WarmupAuthorization,
    DEFAULT_WARMUP_MODEL,
};
use codexmanager_core::storage::{
    now_ts, Account, ManagedModelV2, ManagedModelV2Upsert, ModelPriceV2, Storage, Token,
    UsageSnapshotRecord,
};
use std::io::Cursor;

#[test]
fn reset_warmup_does_not_resend_after_uncertain_transport_failure() {
    let mut messages = Vec::new();
    let result = super::warmup_request_with_message_fallback("hi", false, |message| {
        messages.push(message.to_string());
        Err("request timed out".to_string())
    });
    assert_eq!(messages, ["hi"]);
    assert_eq!(result.unwrap_err(), "request timed out");
}

#[test]
fn manual_warmup_retains_existing_message_fallback() {
    let mut messages = Vec::new();
    let result = super::warmup_request_with_message_fallback("hi", true, |message| {
        messages.push(message.to_string());
        if messages.len() == 1 {
            Err("initial message rejected".to_string())
        } else {
            Ok(())
        }
    });
    assert!(result.is_ok());
    assert_eq!(messages, ["hi", "你好"]);
}

fn reset_warmup_fixture() -> (Storage, Account) {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("initialize storage");
    let account = Account {
        id: "reset-account".to_string(),
        label: "Reset account".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    };
    storage.insert_account(&account).unwrap();
    storage
        .insert_token(&Token {
            account_id: account.id.clone(),
            id_token: String::new(),
            access_token: "test-access-token".to_string(),
            refresh_token: "test-refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        })
        .unwrap();
    (storage, account)
}

#[test]
fn reset_warmup_target_bypasses_stale_exhausted_quota_but_rechecks_status() {
    let (storage, mut account) = reset_warmup_fixture();
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account.id.clone(),
            used_percent: Some(100.0),
            window_minutes: Some(300),
            resets_at: Some(now_ts() - 10),
            secondary_used_percent: Some(25.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: Some(now_ts() + 86400),
            credits_json: None,
            captured_at: now_ts() - 60,
        })
        .unwrap();
    let target = resolve_reset_warmup_target(&storage, &account.id).unwrap();
    assert_eq!(target.account.id, account.id);
    for status in ["disabled", " unavailable ", "banned", "inactive"] {
        account.status = status.to_string();
        storage.insert_account(&account).unwrap();
        assert!(resolve_reset_warmup_target(&storage, &account.id).is_err());
    }
}

#[test]
fn successful_warmup_records_request_and_refreshes_only_target_usage() {
    let (storage, account) = reset_warmup_fixture();
    let account_id = account.id.clone();
    let mut refreshed = Vec::new();
    let result = finish_warmup_attempt(
        &storage,
        account,
        DEFAULT_WARMUP_MODEL,
        42,
        Ok("sent".to_string()),
        |id| {
            refreshed.push(id.to_string());
            true
        },
    );
    assert!(result.ok);
    assert_eq!(refreshed, [account_id.clone()]);
    let logs = storage.list_request_logs(None, 10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].account_id.as_deref(), Some(account_id.as_str()));
    assert_eq!(logs[0].status_code, Some(200));
    assert_eq!(logs[0].request_type.as_deref(), Some("account_warmup"));
}

#[test]
fn failed_warmup_records_failure_without_usage_refresh() {
    let _guard = crate::test_env_guard();
    let (storage, account) = reset_warmup_fixture();
    let result = finish_warmup_attempt(
        &storage,
        account,
        DEFAULT_WARMUP_MODEL,
        42,
        Err("status=429 exhausted".to_string()),
        |_| panic!("failed send must not report success by refreshing usage"),
    );
    assert!(!result.ok);
    let logs = storage.list_request_logs(None, 10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status_code, Some(429));
}

#[test]
fn summarize_warmup_error_redacts_invalid_agent_task_details() {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "x-openai-authorization-error",
        reqwest::header::HeaderValue::from_static("task-secret-from-header"),
    );
    let summary = summarize_warmup_error(
        401,
        &headers,
        r#"{"error":{"code":"task_expired","message":"task-secret-from-body"}}"#,
    );

    assert!(summary.contains("invalid_task_id"));
    assert!(crate::agent_identity::is_agent_identity_task_invalid_error(
        &summary
    ));
    assert!(!summary.contains("task-secret-from-body"));
    assert!(!summary.contains("task-secret-from-header"));
}

fn make_model(slug: &str, sort_order: i64, supported_in_api: bool) -> ManagedModelV2Upsert {
    ManagedModelV2Upsert {
        model: ManagedModelV2 {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            origin: "custom".to_string(),
            enabled: true,
            supported_in_api,
            visibility: "list".to_string(),
            sort_order,
            instructions_mode: "passthrough".to_string(),
            price: ModelPriceV2 {
                price_status: "missing".to_string(),
                ..Default::default()
            },
            ..ManagedModelV2::default()
        },
        ..ManagedModelV2Upsert::default()
    }
}

fn disable_seed_models(storage: &Storage) {
    for mut model in storage
        .list_managed_models_v2(true)
        .expect("list seeded models")
    {
        model.enabled = false;
        storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                model,
                ..Default::default()
            })
            .expect("disable seeded model");
    }
}

#[test]
fn resolve_warmup_model_slug_uses_first_supported_model_from_catalog_order() {
    let (storage, account) = reset_warmup_fixture();
    let target = resolve_reset_warmup_target(&storage, &account.id).expect("resolve target");
    disable_seed_models(&storage);
    let mut hidden = make_model("gpt-hidden", 0, true);
    hidden.model.visibility = "hide".to_string();
    let mut image = make_model("gpt-image-only", 0, true);
    image.model.capabilities = serde_json::json!({
        "supports_text_generation": false,
        "output_modalities": ["image"]
    });
    for model in [
        hidden,
        image,
        make_model("gpt-unsupported", 1, false),
        make_model("gpt-latest", 1, true),
        make_model("gpt-older", 2, true),
    ] {
        storage
            .upsert_managed_model_v2(&model)
            .expect("save model catalog V2 item");
    }

    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &target, "auto").unwrap(),
        "gpt-latest"
    );
}

#[test]
fn resolve_warmup_model_slug_falls_back_when_catalog_missing() {
    let (storage, account) = reset_warmup_fixture();
    let target = resolve_reset_warmup_target(&storage, &account.id).expect("resolve target");
    disable_seed_models(&storage);
    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &target, "auto").unwrap(),
        "gpt-6-luna"
    );
}

#[test]
fn resolve_warmup_model_slug_honors_free_account_model_ceiling() {
    let (storage, account) = reset_warmup_fixture();
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account.id.clone(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: Some(now_ts() + 3600),
            secondary_used_percent: Some(20.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: Some(now_ts() + 86400),
            credits_json: Some(r#"{"planType":"free"}"#.to_string()),
            captured_at: now_ts(),
        })
        .expect("insert Free usage snapshot");
    let target = resolve_reset_warmup_target(&storage, &account.id).expect("resolve target");
    disable_seed_models(&storage);
    for model in [
        make_model("gpt-newest", 1, true),
        make_model("gpt-free-boundary", 2, true),
        make_model("gpt-older", 3, true),
    ] {
        storage
            .upsert_managed_model_v2(&model)
            .expect("save model catalog V2 item");
    }

    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &target, "gpt-free-boundary").unwrap(),
        "gpt-free-boundary"
    );

    let paid_account = Account {
        id: "paid-account".to_string(),
        label: "Paid account".to_string(),
        ..account
    };
    storage
        .insert_account(&paid_account)
        .expect("insert paid account");
    storage
        .insert_token(&Token {
            account_id: paid_account.id.clone(),
            id_token: String::new(),
            access_token: "paid-access-token".to_string(),
            refresh_token: "paid-refresh-token".to_string(),
            api_key_access_token: None,
            last_refresh: now_ts(),
        })
        .expect("insert paid token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: paid_account.id.clone(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: Some(now_ts() + 3600),
            secondary_used_percent: Some(20.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: Some(now_ts() + 86400),
            credits_json: Some(r#"{"planType":"plus"}"#.to_string()),
            captured_at: now_ts(),
        })
        .expect("insert paid usage snapshot");
    let paid_target =
        resolve_reset_warmup_target(&storage, &paid_account.id).expect("resolve paid target");
    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &paid_target, "gpt-free-boundary")
            .unwrap(),
        "gpt-newest"
    );
}

#[test]
fn resolve_warmup_model_slug_treats_missing_plan_metadata_as_free_until_refreshed() {
    let (storage, account) = reset_warmup_fixture();
    let target = resolve_reset_warmup_target(&storage, &account.id).expect("resolve target");
    disable_seed_models(&storage);
    for model in [
        make_model("gpt-newest", 1, true),
        make_model("gpt-free-boundary", 2, true),
        make_model("gpt-older", 3, true),
    ] {
        storage
            .upsert_managed_model_v2(&model)
            .expect("save model catalog V2 item");
    }

    // The fixture has no usable plan in either the token or usage snapshot.
    // Until a refresh resolves it, warmup must stay within the Free ceiling.
    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &target, "gpt-free-boundary")
            .expect("resolve conservative warmup model"),
        "gpt-free-boundary"
    );
}

#[test]
fn resolve_warmup_model_slug_treats_unknown_plan_as_free_until_refreshed() {
    let (storage, account) = reset_warmup_fixture();
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account.id.clone(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: Some(now_ts() + 3_600),
            secondary_used_percent: Some(20.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: Some(now_ts() + 86_400),
            credits_json: Some(r#"{"planType":"unknown"}"#.to_string()),
            captured_at: now_ts(),
        })
        .expect("insert unknown-plan usage snapshot");
    let target = resolve_reset_warmup_target(&storage, &account.id).expect("resolve target");
    disable_seed_models(&storage);
    for model in [
        make_model("gpt-newest", 1, true),
        make_model("gpt-free-boundary", 2, true),
        make_model("gpt-older", 3, true),
    ] {
        storage
            .upsert_managed_model_v2(&model)
            .expect("save model catalog V2 item");
    }

    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &target, "gpt-free-boundary")
            .expect("resolve conservative warmup model"),
        "gpt-free-boundary"
    );
}

#[test]
fn resolve_warmup_model_slug_treats_inactive_or_expired_paid_subscription_as_free() {
    let (storage, account) = reset_warmup_fixture();
    let now = now_ts();
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account.id.clone(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: Some(now + 3_600),
            secondary_used_percent: Some(20.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: Some(now + 86_400),
            credits_json: Some(r#"{"planType":"plus"}"#.to_string()),
            captured_at: now,
        })
        .expect("insert stale paid usage snapshot");
    disable_seed_models(&storage);
    for model in [
        make_model("gpt-newest", 1, true),
        make_model("gpt-free-boundary", 2, true),
        make_model("gpt-older", 3, true),
    ] {
        storage
            .upsert_managed_model_v2(&model)
            .expect("save model catalog V2 item");
    }
    let target = resolve_reset_warmup_target(&storage, &account.id).expect("resolve target");

    storage
        .upsert_account_subscription(
            &account.id,
            false,
            Some("pro"),
            Some("plus"),
            Some(now + 3_600),
            None,
        )
        .expect("store inactive paid subscription");
    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &target, "gpt-free-boundary").unwrap(),
        "gpt-free-boundary"
    );

    storage
        .upsert_account_subscription(
            &account.id,
            true,
            Some("pro"),
            Some("plus"),
            Some(now - 1),
            None,
        )
        .expect("store expired paid subscription");
    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(&storage, &target, "gpt-free-boundary").unwrap(),
        "gpt-free-boundary"
    );
}

#[test]
fn resolve_warmup_model_slug_selects_seeded_luna_for_free_account() {
    let (storage, account) = reset_warmup_fixture();
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account.id.clone(),
            used_percent: Some(10.0),
            window_minutes: Some(300),
            resets_at: Some(now_ts() + 3600),
            secondary_used_percent: Some(20.0),
            secondary_window_minutes: Some(10080),
            secondary_resets_at: Some(now_ts() + 86400),
            credits_json: Some(r#"{"planType":"free"}"#.to_string()),
            captured_at: now_ts(),
        })
        .expect("insert Free usage snapshot");
    let target = resolve_reset_warmup_target(&storage, &account.id).expect("resolve target");

    assert_eq!(
        resolve_warmup_model_slug_with_ceiling(
            &storage,
            &target,
            codexmanager_core::usage::LUNA_MODEL_SLUG,
        )
        .unwrap(),
        codexmanager_core::usage::LUNA_MODEL_SLUG
    );
}

#[test]
fn should_retry_warmup_with_refresh_only_for_auth_errors_with_refresh_token() {
    let mut token = Token {
        account_id: "account-1".to_string(),
        id_token: String::new(),
        access_token: String::new(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: 0,
    };

    assert!(should_retry_warmup_with_refresh(
        &token,
        "status=401 body=Unauthorized"
    ));
    assert!(!should_retry_warmup_with_refresh(
        &token,
        "status=500 body=server error"
    ));

    token.refresh_token.clear();
    assert!(!should_retry_warmup_with_refresh(
        &token,
        "status=401 body=Unauthorized"
    ));
}

#[test]
fn resolve_target_accounts_only_returns_gateway_available_accounts() {
    let storage = Storage::open_in_memory().expect("open in-memory storage");
    storage.init().expect("init in-memory storage");
    let now = now_ts();

    for (id, status) in [
        ("acc-active", "active"),
        ("acc-unavailable", "unavailable"),
        ("acc-disabled", "disabled"),
        ("acc-banned", "banned"),
        ("acc-inactive", "inactive"),
    ] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "issuer".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: status.to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: id.to_string(),
                id_token: "id-token".to_string(),
                access_token: "access-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
    }

    let all_targets = resolve_target_accounts(&storage, &[]).expect("resolve all targets");
    assert_eq!(all_targets.len(), 1);
    assert_eq!(all_targets[0].account.id, "acc-active");
    assert_eq!(all_targets[0].token.account_id, "acc-active");

    let selected_targets = resolve_target_accounts(
        &storage,
        &[
            "acc-unavailable".to_string(),
            "acc-active".to_string(),
            "acc-disabled".to_string(),
        ],
    )
    .expect("resolve selected targets");
    assert_eq!(selected_targets.len(), 1);
    assert_eq!(selected_targets[0].account.id, "acc-active");
    assert_eq!(selected_targets[0].token.account_id, "acc-active");
}

#[test]
fn build_warmup_headers_omits_non_codex_headers() {
    let account = Account {
        id: "acc-1".to_string(),
        label: "acc-1".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: None,
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: 0,
        updated_at: 0,
    };

    let authorization = WarmupAuthorization {
        value: "bearer-token".to_string(),
        task_id: None,
        is_fedramp: false,
        uses_agent_identity: false,
        account_scope_id: None,
    };
    let headers = build_warmup_headers(&account, &authorization).expect("build warmup headers");

    assert!(headers.get("version").is_none());
    assert!(headers.get("openai-organization").is_none());
    assert!(headers.get("openai-project").is_none());
    assert!(headers.get("client_version").is_none());
    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer bearer-token")
    );
    assert!(headers.get("x-openai-fedramp").is_none());
}

#[test]
fn build_warmup_headers_preserves_agent_assertion_and_fedramp_context() {
    let account = Account {
        id: "acc-agent".to_string(),
        label: "acc-agent".to_string(),
        issuer: "issuer".to_string(),
        chatgpt_account_id: Some("workspace-agent".to_string()),
        workspace_id: Some("workspace-agent".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: 0,
        updated_at: 0,
    };

    let authorization = WarmupAuthorization {
        value: "AgentAssertion encoded".to_string(),
        task_id: Some("task-agent".to_string()),
        is_fedramp: true,
        uses_agent_identity: true,
        account_scope_id: Some("agent-bound-scope".to_string()),
    };
    let headers =
        build_warmup_headers(&account, &authorization).expect("build agent warmup headers");

    assert_eq!(
        headers
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("AgentAssertion encoded")
    );
    assert_eq!(
        headers
            .get("chatgpt-account-id")
            .and_then(|value| value.to_str().ok()),
        Some("agent-bound-scope")
    );
    assert_eq!(
        headers
            .get("x-openai-fedramp")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
}

#[test]
fn consume_warmup_stream_waits_for_response_completed() {
    let stream = Cursor::new(
        "event: response.created\n\
         data: {\"type\":\"response.created\"}\n\n\
         event: response.completed\n\
         data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\"}}\n\n",
    );

    assert!(consume_warmup_stream(stream).is_ok());
}

#[test]
fn consume_warmup_stream_rejects_incomplete_stream() {
    let stream = Cursor::new(
        "event: response.created\n\
         data: {\"type\":\"response.created\"}\n\n",
    );

    let err = consume_warmup_stream(stream).expect_err("stream should be incomplete");
    assert!(err.contains("before response.completed"));
}

#[test]
fn consume_warmup_stream_reports_error_event() {
    let stream = Cursor::new(
        "event: response.failed\n\
         data: {\"type\":\"response.failed\",\"error\":{\"message\":\"quota exceeded\"}}\n\n",
    );

    let err = consume_warmup_stream(stream).expect_err("stream should fail");
    assert!(err.contains("quota exceeded"));
}
