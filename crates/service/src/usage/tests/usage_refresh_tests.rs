use super::{
    clear_pending_usage_refresh_tasks_for_tests, enqueue_usage_refresh_with_worker,
    load_token_refresh_issuers_for_tokens, next_usage_poll_cursor, notify_usage_refresh_completed,
    record_codex_admission_probe_failure, record_codex_model_probe_failure,
    record_codex_probe_outcome, record_codex_responses_verified, refresh_account_snapshot,
    reset_usage_poll_cursor_for_tests, resolve_token_refresh_issuer, run_token_refresh_task,
    set_usage_refresh_completed_handler, should_retry_usage_refresh_with_token,
    subscribe_usage_refresh_completed, token_refresh_access_exp_cutoff, token_refresh_due_cutoff,
    token_refresh_schedule, usage_poll_batch_indices, UsageAvailabilityStatus,
};
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode as TinyStatusCode};

#[test]
fn usage_snapshot_is_saved_before_accounts_check_cloudflare_failure() {
    let _guard = crate::test_env_guard();
    // The service initializes gateway clients before background refresh starts.
    // Mirror that lifecycle here so lazy construction of the blocking client
    // pool cannot happen from inside the test's async usage runtime.
    crate::gateway::reload_runtime_config_from_env();
    let _ = crate::gateway::current_codex_user_agent();
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: "acc-usage-before-check".to_string(),
            label: "usage-before-check".to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: Some("subscription-account".to_string()),
            workspace_id: None,
            group_name: None,
            sort: 0,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert account");

    let server = Server::http("127.0.0.1:0").expect("start usage mock server");
    let base_url = format!("http://{}", server.server_addr());
    let (paths_tx, paths_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        for index in 0..2 {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .expect("mock server receive timeout")
                .expect("receive request");
            paths_tx
                .send(request.url().to_string())
                .expect("record request path");
            let response = if index == 0 {
                Response::from_string(
                    r#"{"rate_limit":{"primary_window":{"used_percent":12.5,"limit_window_seconds":18000,"reset_at":4102444800}}}"#,
                )
                .with_status_code(TinyStatusCode(200))
                .with_header(
                    Header::from_bytes("Content-Type", "application/json")
                        .expect("content type header"),
                )
            } else {
                Response::from_string("<html><title>Just a moment...</title></html>")
                    .with_status_code(TinyStatusCode(403))
                    .with_header(
                        Header::from_bytes("Content-Type", "text/html; charset=utf-8")
                            .expect("content type header"),
                    )
                    .with_header(Header::from_bytes("cf-ray", "ray-test-NRT").expect("cf-ray"))
            };
            request.respond(response).expect("respond mock request");
        }
    });

    let result = refresh_account_snapshot(
        &storage,
        "acc-usage-before-check",
        &base_url,
        "access-token",
        None,
        Some("subscription-account"),
        true,
    );

    assert_eq!(
        result.expect("Cloudflare metadata failure is best effort"),
        UsageAvailabilityStatus::PrimaryWindowAvailableOnly
    );
    let paths = [
        paths_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("usage request path"),
        paths_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("accounts check request path"),
    ];
    assert_eq!(paths[0], "/api/codex/usage");
    assert_eq!(paths[1], "/accounts/check/v4-2023-04-27");
    let saved = storage
        .latest_usage_snapshot_for_account("acc-usage-before-check")
        .expect("load usage snapshot")
        .expect("usage snapshot saved");
    assert_eq!(saved.used_percent, Some(12.5));
    assert_eq!(saved.window_minutes, Some(300));
    handle.join().expect("join usage mock server");
}

#[test]
fn usage_refresh_completed_handler_receives_notification() {
    let _guard = crate::test_env_guard();
    let (tx, rx) = mpsc::channel();
    set_usage_refresh_completed_handler(move |event| {
        let _ = tx.send(event);
    });

    notify_usage_refresh_completed("test-notify", 2, 3);
    let event = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("usage refresh completed event");
    assert_eq!(event.source, "test-notify");
    assert_eq!(event.processed, 2);
    assert_eq!(event.total, 3);
    assert!(event.completed_at > 0);
}

#[test]
fn usage_refresh_completed_subscriber_receives_notification() {
    let _guard = crate::test_env_guard();
    let rx = subscribe_usage_refresh_completed();

    notify_usage_refresh_completed("test-subscribe", 1, 1);
    let event = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("usage refresh completed event");
    assert_eq!(event.source, "test-subscribe");
    assert_eq!(event.processed, 1);
    assert_eq!(event.total, 1);
    assert!(event.completed_at > 0);
}

/// 函数 `enqueue_usage_refresh_for_same_account_is_deduplicated_until_finish`
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
fn enqueue_usage_refresh_for_same_account_is_deduplicated_until_finish() {
    let _guard = crate::test_env_guard();
    clear_pending_usage_refresh_tasks_for_tests();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();

    let first = enqueue_usage_refresh_with_worker("acc-dedup", move |_| {
        let _ = started_tx.send(());
        let _ = release_rx.recv();
    });
    assert!(first);
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("worker started");

    let second = enqueue_usage_refresh_with_worker("acc-dedup", |_| {});
    assert!(!second);

    let _ = release_tx.send(());
    std::thread::sleep(Duration::from_millis(20));

    let third = enqueue_usage_refresh_with_worker("acc-dedup", |_| {});
    assert!(third);
    std::thread::sleep(Duration::from_millis(20));
    clear_pending_usage_refresh_tasks_for_tests();
}

/// 函数 `enqueue_usage_refresh_for_different_accounts_keeps_queue_progress`
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
fn enqueue_usage_refresh_for_different_accounts_keeps_queue_progress() {
    let _guard = crate::test_env_guard();
    clear_pending_usage_refresh_tasks_for_tests();
    let (started_tx, started_rx) = mpsc::channel::<String>();
    let (release_tx, release_rx) = mpsc::channel();
    let started_tx_first = started_tx.clone();

    let first = enqueue_usage_refresh_with_worker("acc-a", move |_| {
        let _ = started_tx_first.send("acc-a".to_string());
        let _ = release_rx.recv_timeout(Duration::from_secs(1));
    });
    assert!(first);

    let started_tx = started_tx.clone();
    let second = enqueue_usage_refresh_with_worker("acc-b", move |_| {
        let _ = started_tx.send("acc-b".to_string());
    });
    assert!(second);

    let first_started = started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first task should start");
    let _ = release_tx.send(());
    let second_started = started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("second task should start");

    let seen: HashSet<String> = [first_started, second_started].into_iter().collect();
    assert_eq!(seen.len(), 2);
    assert!(seen.contains("acc-a"));
    assert!(seen.contains("acc-b"));

    std::thread::sleep(Duration::from_millis(20));
    clear_pending_usage_refresh_tasks_for_tests();
}

/// 函数 `schedule_prefers_exp_minus_ahead`
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
fn schedule_prefers_exp_minus_ahead() {
    let now = now_ts();
    let token = Token {
        account_id: "acc-1".to_string(),
        id_token: "id".to_string(),
        access_token: "a.eyJleHAiOjQxMDI0NDQ4MDB9.s".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now - 10,
    };
    let (exp, scheduled_at) = token_refresh_schedule(&token, now, 3600, 2700);
    assert_eq!(exp, Some(4_102_444_800));
    assert_eq!(scheduled_at, 4_102_441_200);
}

#[test]
fn schedule_prefers_refresh_token_exp_when_it_expires_first() {
    let now = now_ts();
    let token = Token {
        account_id: "acc-refresh-exp-first".to_string(),
        id_token: "id".to_string(),
        access_token: "a.eyJleHAiOjQxMDI0NDQ4MDB9.s".to_string(),
        refresh_token: "r.eyJleHAiOjQxMDI0NDMwMDB9.s".to_string(),
        api_key_access_token: None,
        last_refresh: now - 10,
    };
    let (exp, scheduled_at) = token_refresh_schedule(&token, now, 3600, 2700);
    assert_eq!(exp, Some(4_102_444_800));
    assert_eq!(scheduled_at, 4_102_439_400);
}

/// 函数 `schedule_falls_back_to_last_refresh_when_exp_missing`
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
fn schedule_falls_back_to_last_refresh_when_exp_missing() {
    let now = now_ts();
    let token = Token {
        account_id: "acc-2".to_string(),
        id_token: "id".to_string(),
        access_token: "no-jwt".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now - 5000,
    };
    let (exp, scheduled_at) = token_refresh_schedule(&token, now, 300, 2700);
    assert_eq!(exp, None);
    assert_eq!(scheduled_at, now);
}

/// 函数 `schedule_skips_when_refresh_token_is_empty`
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
fn schedule_skips_when_refresh_token_is_empty() {
    let now = now_ts();
    let token = Token {
        account_id: "acc-empty-refresh".to_string(),
        id_token: "id".to_string(),
        access_token: "a.eyJleHAiOjQxMDI0NDQ4MDB9.s".to_string(),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now - 10,
    };
    let (exp, scheduled_at) = token_refresh_schedule(&token, now, 600, 2700);
    assert_eq!(exp, None);
    assert_eq!(scheduled_at, i64::MAX);
}

/// 函数 `usage_refresh_retry_skips_when_refresh_token_is_empty`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn usage_refresh_retry_skips_when_refresh_token_is_empty() {
    let token = Token {
        account_id: "acc-empty-refresh".to_string(),
        id_token: "id".to_string(),
        access_token: "access".to_string(),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert!(!should_retry_usage_refresh_with_token(
        &token,
        "usage endpoint status 401 Unauthorized"
    ));
    assert!(!should_retry_usage_refresh_with_token(
        &token,
        "usage endpoint status 403 Forbidden"
    ));
}

#[test]
fn usage_refresh_retry_skips_region_blocked_errors() {
    let token = Token {
        account_id: "acc-region-blocked-retry".to_string(),
        id_token: "id".to_string(),
        access_token: "access".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert!(!should_retry_usage_refresh_with_token(
        &token,
        "usage endpoint failed: status=403 Forbidden body=code=unsupported_country_region_territory cf_ray=ray-HKG",
    ));
    assert!(should_retry_usage_refresh_with_token(
        &token,
        "usage endpoint status 403 Forbidden"
    ));
}

/// 函数 `due_cutoff_includes_next_poll_window_and_buffer`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-06
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn due_cutoff_includes_next_poll_window_and_buffer() {
    let now = now_ts();
    assert_eq!(token_refresh_due_cutoff(now, 600), now + 660);
}

/// 函数 `access_exp_cutoff_includes_refresh_ahead_window`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-26
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn access_exp_cutoff_includes_refresh_ahead_window() {
    assert_eq!(token_refresh_access_exp_cutoff(1_000, 3600), 4_600);
}

/// 函数 `due_cutoff_covers_boundary_when_poll_interval_matches_refresh_ahead`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-06
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn due_cutoff_covers_boundary_when_poll_interval_matches_refresh_ahead() {
    let exp = 4_102_444_800;
    let now = exp - 7_260;
    let token = Token {
        account_id: "acc-boundary".to_string(),
        id_token: "id".to_string(),
        access_token: "a.eyJleHAiOjQxMDI0NDQ4MDB9.s".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now - 10,
    };
    let (_, scheduled_at) = token_refresh_schedule(&token, now, 3600, 2700);

    assert_eq!(scheduled_at, exp - 3600);
    assert!(scheduled_at > now);
    assert!(scheduled_at <= token_refresh_due_cutoff(now, 3600));
}

/// 函数 `token_refresh_issuer_uses_account_issuer`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-26
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn token_refresh_issuer_uses_account_issuer() {
    assert_eq!(
        resolve_token_refresh_issuer(
            Some("https://custom-issuer.example"),
            "https://auth.openai.com"
        ),
        "https://custom-issuer.example"
    );
}

/// 函数 `token_refresh_issuer_falls_back_to_default`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-26
///
/// # 参数
/// 无
///
/// # 返回
/// 无
#[test]
fn token_refresh_issuer_falls_back_to_default() {
    assert_eq!(
        resolve_token_refresh_issuer(Some("  "), "https://auth.openai.com"),
        "https://auth.openai.com"
    );
    assert_eq!(
        resolve_token_refresh_issuer(None, "https://auth.openai.com"),
        "https://auth.openai.com"
    );
}

#[test]
fn load_token_refresh_issuers_for_tokens_reads_only_due_token_issuers() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let now = now_ts();

    for id in ["acc-due-b", "acc-ignored", "acc-due-a"] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: format!("https://{id}.example"),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: if id == "acc-due-a" { 0 } else { 1 },
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
    }

    let tokens = vec![
        Token {
            account_id: "acc-due-b".to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        },
        Token {
            account_id: "acc-missing".to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        },
        Token {
            account_id: "acc-due-a".to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        },
    ];

    let issuers =
        load_token_refresh_issuers_for_tokens(&storage, &tokens).expect("load account issuers");

    assert_eq!(
        issuers
            .into_iter()
            .map(|issuer| (issuer.id, issuer.issuer))
            .collect::<Vec<_>>(),
        vec![
            (
                "acc-due-a".to_string(),
                "https://acc-due-a.example".to_string()
            ),
            (
                "acc-due-b".to_string(),
                "https://acc-due-b.example".to_string()
            ),
        ]
    );
}

/// 函数 `run_token_refresh_task_skips_empty_refresh_token`
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
fn run_token_refresh_task_skips_empty_refresh_token() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    let now = now_ts();
    let mut token = Token {
        account_id: "acc-empty-refresh".to_string(),
        id_token: "id".to_string(),
        access_token: "access".to_string(),
        refresh_token: String::new(),
        api_key_access_token: None,
        last_refresh: now,
    };

    let refreshed =
        run_token_refresh_task(&storage, &mut token, "https://auth.openai.com", "codex-cli");
    assert!(!refreshed);
}

/// 函数 `usage_poll_batch_indices_rotate_from_cursor`
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
fn usage_poll_batch_indices_rotate_from_cursor() {
    reset_usage_poll_cursor_for_tests();
    assert_eq!(usage_poll_batch_indices(5, 4, 3), vec![4, 0, 1]);
    assert_eq!(usage_poll_batch_indices(3, 1, 10), vec![1, 2, 0]);
}

/// 函数 `usage_poll_cursor_advances_by_processed_count`
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
fn usage_poll_cursor_advances_by_processed_count() {
    reset_usage_poll_cursor_for_tests();
    assert_eq!(next_usage_poll_cursor(5, 4, 2), 1);
    assert_eq!(next_usage_poll_cursor(5, 1, 5), 1);
    assert_eq!(next_usage_poll_cursor(0, 7, 3), 0);
}

#[test]
fn codex_models_404_probe_still_requires_independent_responses_admission() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    record_codex_model_probe_failure(
        &storage,
        "acc-models-404",
        "models upstream failed: status=404 body=Not Found",
    );
    let state = storage
        .list_adapter_credential_probe_states("codex", &["acc-models-404".to_string()])
        .expect("read probe state");
    assert_eq!(state[0].status, "failed");
    assert_eq!(
        state[0].error_code.as_deref(),
        Some("codex_models_not_found")
    );
    assert!(state[0]
        .retry_after
        .is_some_and(|retry_after| retry_after <= now_ts()));
}

#[test]
fn transient_codex_models_probe_failure_remains_retryable() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    record_codex_model_probe_failure(
        &storage,
        "acc-models-timeout",
        "models upstream request failed: timeout",
    );
    let state = storage
        .list_adapter_credential_probe_states("codex", &["acc-models-timeout".to_string()])
        .expect("read probe state");
    assert_eq!(state[0].status, "failed");
    assert_eq!(
        state[0].error_code.as_deref(),
        Some("codex_models_probe_failed")
    );
    assert!(state[0]
        .retry_after
        .is_some_and(|retry_after| retry_after <= now_ts()));
}

#[test]
fn usage_success_does_not_resurrect_runtime_not_found_account() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");
    record_codex_admission_probe_failure(
        &storage,
        "acc-runtime-dead",
        "responses upstream failed: status=404 body=Not Found",
    );
    record_codex_probe_outcome(
        &storage,
        "acc-runtime-dead",
        UsageAvailabilityStatus::Available,
    );
    let state = storage
        .list_adapter_credential_probe_states("codex", &["acc-runtime-dead".to_string()])
        .expect("read probe state");
    assert_eq!(state[0].status, "unavailable");
    assert_eq!(
        state[0].error_code.as_deref(),
        Some("codex_responses_not_found")
    );

    record_codex_responses_verified(&storage, "acc-runtime-dead");
    let recovered = storage
        .list_adapter_credential_probe_states("codex", &["acc-runtime-dead".to_string()])
        .expect("read recovered state");
    assert_eq!(recovered[0].status, "available");
    assert_eq!(
        recovered[0].error_code.as_deref(),
        Some("codex_responses_verified")
    );
}

#[test]
fn responses_admission_failure_classifies_permanent_auth_and_transient_errors() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init");

    for (id, error, status, code, retryable) in [
        (
            "not-found",
            "status=404 body=Not Found",
            "unavailable",
            "codex_responses_not_found",
            false,
        ),
        (
            "unauthorized",
            "status=401 body=Unauthorized",
            "failed",
            "codex_responses_unauthorized",
            true,
        ),
        (
            "network",
            "warmup request failed: timed out",
            "failed",
            "codex_responses_probe_failed",
            true,
        ),
        (
            "rate-limited",
            "status=429 body=usage_limit_reached",
            "failed",
            "codex_responses_rate_limited",
            true,
        ),
        (
            "cloudflare",
            "status=403 body=Cloudflare 安全验证页 kind=cloudflare_challenge",
            "failed",
            "codex_responses_cloudflare_challenge",
            true,
        ),
    ] {
        record_codex_admission_probe_failure(&storage, id, error);
        let states = storage
            .list_adapter_credential_probe_states("codex", &[id.to_string()])
            .expect("read state");
        assert_eq!(states[0].status, status);
        assert_eq!(states[0].error_code.as_deref(), Some(code));
        assert_eq!(states[0].retry_after.is_some(), retryable);
    }

    let states = storage
        .list_adapter_credential_probe_states(
            "codex",
            &["rate-limited".to_string(), "cloudflare".to_string()],
        )
        .expect("read cooldown states");
    let now = now_ts();
    let rate_limited = states
        .iter()
        .find(|state| state.credential_id == "rate-limited")
        .expect("rate limited state");
    assert!(rate_limited
        .retry_after
        .is_some_and(|retry_after| retry_after >= now + 5 * 60 * 60 - 2));
    let cloudflare = states
        .iter()
        .find(|state| state.credential_id == "cloudflare")
        .expect("cloudflare state");
    assert!(cloudflare
        .retry_after
        .is_some_and(|retry_after| retry_after >= now + 10 * 60 - 2));
}
