use super::{
    enqueue_usage_refresh_with_worker, load_token_refresh_issuers_for_tokens,
    next_usage_poll_cursor, notify_usage_refresh_completed, refresh_usage_for_account_result,
    reset_usage_poll_cursor_for_tests, reset_usage_refresh_executor_for_tests,
    resolve_token_refresh_issuer, run_token_refresh_task, set_usage_refresh_completed_handler,
    should_retry_usage_refresh_with_token, subscribe_usage_refresh_completed,
    subscribe_usage_refresh_completed_async, token_refresh_access_exp_cutoff,
    token_refresh_due_cutoff, token_refresh_schedule, usage_poll_batch_indices,
};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use codexmanager_core::storage::{now_ts, Account, AccountAgentIdentity, Storage, Token};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use std::collections::HashSet;
use std::ffi::OsString;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;
use tiny_http::{Header, Response, Server, StatusCode as TinyStatusCode};

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        if key == "CODEXMANAGER_UPSTREAM_PROXY_URL" {
            crate::gateway::reload_runtime_config_from_env();
            crate::usage_http::reload_usage_http_client_from_env();
        }
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
        if self.key == "CODEXMANAGER_UPSTREAM_PROXY_URL" {
            crate::gateway::reload_runtime_config_from_env();
            crate::usage_http::reload_usage_http_client_from_env();
        }
    }
}

fn unique_temp_db_path(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir()
        .join(format!(
            "codexmanager-{name}-{}-{nanos}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .to_string()
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

#[test]
fn usage_refresh_async_and_legacy_subscribers_receive_same_notification() {
    let _guard = crate::test_env_guard();
    let legacy = subscribe_usage_refresh_completed();
    let mut receiver = subscribe_usage_refresh_completed_async();

    notify_usage_refresh_completed("async-and-legacy", 3, 4);
    let asynchronous = receiver.try_recv().expect("async usage event");
    let synchronous = legacy.try_recv().expect("legacy usage event");
    assert_eq!(asynchronous.source, synchronous.source);
    assert_eq!(asynchronous.processed, 3);
    assert_eq!(asynchronous.total, 4);
    assert_eq!(asynchronous.completed_at, synchronous.completed_at);
}

#[test]
fn refresh_usage_for_account_result_reports_missing_token() {
    let _guard = crate::test_env_guard();
    let db_path = unique_temp_db_path("usage-refresh-missing-token");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", &db_path);
    let _ = std::fs::remove_file(&db_path);
    crate::storage_helpers::initialize_storage().expect("init storage");

    let now = now_ts();
    {
        let storage = crate::storage_helpers::open_storage().expect("open storage");
        storage
            .insert_account(&Account {
                id: "acc-no-token".to_string(),
                label: "No Token".to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
    }

    let result = refresh_usage_for_account_result("acc-no-token").expect("refresh result");

    assert!(!result.ok);
    assert_eq!(result.source, "single");
    assert_eq!(result.account_id.as_deref(), Some("acc-no-token"));
    assert_eq!(result.processed, 0);
    assert_eq!(result.total, 0);
    assert_eq!(result.message.as_deref(), Some("account token not found"));

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test(flavor = "current_thread")]
async fn refresh_usage_for_agent_identity_uses_assertion_and_skips_subscription_check() {
    let _guard = crate::test_env_guard();
    let db_path = unique_temp_db_path("usage-refresh-agent-identity");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", &db_path);
    let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _ = std::fs::remove_file(&db_path);
    crate::storage_helpers::initialize_storage().expect("init storage");

    let server = Server::http("127.0.0.1:0").expect("start usage server");
    let base_url = format!("http://{}", server.server_addr());
    let _base_url_guard = EnvGuard::set("CODEXMANAGER_USAGE_BASE_URL", &base_url);
    crate::usage_http::reload_usage_http_client_from_env();
    let (request_tx, request_rx) = mpsc::channel();
    let (request_started_tx, request_started_rx) = tokio::sync::oneshot::channel();
    let (release_response_tx, release_response_rx) = mpsc::channel();
    let server_handle = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .expect("usage server timeout")
            .expect("receive usage request");
        let authorization = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("authorization"))
            .map(|header| header.value.as_str().to_string());
        let workspace = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("chatgpt-account-id"))
            .map(|header| header.value.as_str().to_string());
        request_tx
            .send((request.url().to_string(), authorization, workspace))
            .expect("record usage request");
        request_started_tx.send(()).expect("signal request started");
        release_response_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("async executor remains available during HTTP");
        request
            .respond(
                Response::from_string(r#"{"gpt4":{"usedPercent":8.0,"windowMinutes":180}}"#)
                    .with_status_code(TinyStatusCode(200))
                    .with_header(
                        Header::from_bytes("Content-Type", "application/json")
                            .expect("content-type header"),
                    ),
            )
            .expect("respond usage request");
    });

    let now = now_ts();
    let account_id = "acc-agent-identity-usage";
    let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
    let private_key = signing_key.to_pkcs8_der().expect("encode private key");
    {
        let storage = crate::storage_helpers::open_storage().expect("open storage");
        storage
            .insert_account(&Account {
                id: account_id.to_string(),
                label: "Agent Identity".to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some("workspace-agent".to_string()),
                workspace_id: Some("workspace-agent".to_string()),
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: account_id.to_string(),
                id_token: String::new(),
                access_token: String::new(),
                refresh_token: String::new(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert empty token");
        storage
            .upsert_account_agent_identity(&AccountAgentIdentity {
                account_id: account_id.to_string(),
                agent_runtime_id: "agent-runtime-usage".to_string(),
                agent_private_key: BASE64_STANDARD.encode(private_key.as_bytes()),
                task_id: Some("task-usage".to_string()),
                chatgpt_user_id: "user-usage".to_string(),
                chatgpt_account_is_fedramp: false,
                auth_mode: "agentIdentity".to_string(),
                workspace_id: Some("workspace-agent".to_string()),
                created_at: now,
                updated_at: now,
            })
            .expect("insert agent identity");
    }

    let (result, ()) = tokio::join!(
        super::refresh_usage_for_account_result_async(account_id),
        async move {
            request_started_rx.await.expect("request started");
            tokio::time::sleep(Duration::from_millis(1)).await;
            release_response_tx
                .send(())
                .expect("release provider response");
        }
    );
    let result = result.expect("refresh agent usage");
    let (path, authorization, workspace) = request_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("receive recorded usage request");
    server_handle.join().expect("join usage server");

    assert!(result.ok);
    assert_eq!(result.processed, 1);
    assert_eq!(path, "/api/codex/usage");
    assert!(authorization
        .as_deref()
        .is_some_and(|value| value.starts_with("AgentAssertion ")));
    assert_eq!(workspace.as_deref(), Some("workspace-agent"));
    let storage = crate::storage_helpers::open_storage().expect("open storage after refresh");
    assert!(storage
        .latest_usage_snapshot_for_account(account_id)
        .expect("find usage snapshot")
        .is_some());
    drop(storage);

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test(flavor = "current_thread")]
async fn refresh_usage_without_chatgpt_account_identity_omits_header_and_succeeds() {
    let _guard = crate::test_env_guard();
    let db_path = unique_temp_db_path("usage-refresh-missing-chatgpt-account-id");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", &db_path);
    let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    let _ = std::fs::remove_file(&db_path);
    crate::storage_helpers::initialize_storage().expect("init storage");

    let server = Server::http("127.0.0.1:0").expect("start usage server");
    let base_url = format!("http://{}", server.server_addr());
    let _base_url_guard = EnvGuard::set("CODEXMANAGER_USAGE_BASE_URL", &base_url);
    crate::usage_http::reload_usage_http_client_from_env();
    let (request_tx, request_rx) = mpsc::channel();
    let server_handle = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .expect("usage server timeout")
            .expect("receive usage request");
        let account_header = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("chatgpt-account-id"))
            .map(|header| header.value.as_str().to_string());
        request_tx
            .send((request.url().to_string(), account_header))
            .expect("record usage request");
        request
            .respond(
                Response::from_string(r#"{"gpt4":{"usedPercent":8.0,"windowMinutes":180}}"#)
                    .with_status_code(TinyStatusCode(200))
                    .with_header(
                        Header::from_bytes("Content-Type", "application/json")
                            .expect("content-type header"),
                    ),
            )
            .expect("respond usage request");
    });

    let now = now_ts();
    let account_id = "acc-missing-chatgpt-account-id";
    {
        let storage = crate::storage_helpers::open_storage().expect("open storage");
        storage
            .insert_account(&Account {
                id: account_id.to_string(),
                label: "Phone login account".to_string(),
                issuer: "https://auth.openai.com".to_string(),
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
                account_id: account_id.to_string(),
                id_token: "phone-id-token-without-account-identity".to_string(),
                access_token: "phone-access-token-without-account-identity".to_string(),
                refresh_token: String::new(),
                api_key_access_token: None,
                last_refresh: now,
            })
            .expect("insert token");
    }

    let result = super::refresh_usage_for_account_result_async(account_id)
        .await
        .expect("refresh usage without account identity");
    let (path, account_header) = request_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("receive recorded usage request");
    server_handle.join().expect("join usage server");

    assert!(result.ok);
    assert_eq!(result.processed, 1);
    assert_eq!(path, "/api/codex/usage");
    assert_eq!(account_header, None);
    let storage = crate::storage_helpers::open_storage().expect("open storage after refresh");
    let account = storage
        .find_account_by_id(account_id)
        .expect("find account")
        .expect("stored account");
    assert_eq!(account.chatgpt_account_id, None);
    assert_eq!(account.workspace_id, None);
    assert!(storage
        .latest_usage_snapshot_for_account(account_id)
        .expect("find usage snapshot")
        .is_some());
    drop(storage);

    let _ = std::fs::remove_file(&db_path);
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
    reset_usage_refresh_executor_for_tests();
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
    reset_usage_refresh_executor_for_tests();

    let third = enqueue_usage_refresh_with_worker("acc-dedup", |_| {});
    assert!(third);
    reset_usage_refresh_executor_for_tests();
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
    reset_usage_refresh_executor_for_tests();
    let (started_tx, started_rx) = mpsc::channel::<String>();
    let (release_tx, release_rx) = mpsc::channel();
    let started_tx_first = started_tx.clone();

    let first = enqueue_usage_refresh_with_worker("acc-a", move |_| {
        let _ = started_tx_first.send("acc-a".to_string());
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
    });
    assert!(first);

    let started_tx = started_tx.clone();
    let second = enqueue_usage_refresh_with_worker("acc-b", move |_| {
        let _ = started_tx.send("acc-b".to_string());
    });
    assert!(second);

    let first_started = started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("first task should start");
    let _ = release_tx.send(());
    let second_started = started_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("second task should start");

    let seen: HashSet<String> = [first_started, second_started].into_iter().collect();
    assert_eq!(seen.len(), 2);
    assert!(seen.contains("acc-a"));
    assert!(seen.contains("acc-b"));

    reset_usage_refresh_executor_for_tests();
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
fn usage_background_loops_and_queue_restart_after_shutdown() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    let _env = crate::test_env_guard();
    struct ClearShutdown;
    impl Drop for ClearShutdown {
        fn drop(&mut self) {
            crate::clear_shutdown_flag();
        }
    }
    let _clear = ClearShutdown;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        crate::clear_shutdown_flag();
        for cycle in 0..2 {
            let (loop_started_tx, mut loop_started_rx) = tokio::sync::mpsc::unbounded_channel();
            super::start_background_loop("usage-restart-fixture", &STARTED, move || {
                let sender = loop_started_tx.clone();
                async move {
                    sender.send(()).unwrap();
                    super::runner::wait_for_shutdown().await;
                }
            });
            tokio::time::timeout(Duration::from_secs(2), loop_started_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(STARTED.load(Ordering::Acquire));
            let (queue_started_tx, queue_started_rx) = tokio::sync::oneshot::channel();
            let dropped = std::sync::Arc::new(AtomicBool::new(false));
            struct DropFlag(std::sync::Arc<AtomicBool>);
            impl Drop for DropFlag {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Release);
                }
            }
            let drop_flag = DropFlag(dropped.clone());
            assert!(
                super::queue::enqueue_usage_refresh_async(
                    "restart-fixture-account",
                    move |_| async move {
                        let _flag = drop_flag;
                        queue_started_tx.send(()).unwrap();
                        std::future::pending::<()>().await;
                    }
                ),
                "queue accepts after restart, cycle={cycle}"
            );
            tokio::time::timeout(Duration::from_secs(2), queue_started_rx)
                .await
                .unwrap()
                .unwrap();
            crate::request_shutdown("");
            assert!(!super::queue::enqueue_usage_refresh_async(
                "shutdown-rejected",
                |_| async {}
            ));
            tokio::time::timeout(
                Duration::from_secs(3),
                super::drain_usage_background_tasks(),
            )
            .await
            .expect("background tasks drain on shutdown");
            assert!(!STARTED.load(Ordering::Acquire), "loop start marker resets");
            assert!(
                dropped.load(Ordering::Acquire),
                "active queue future is cancelled before shutdown returns"
            );
            crate::clear_shutdown_flag();
        }
    });
}
