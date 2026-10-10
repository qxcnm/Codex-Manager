use super::*;
use base64::Engine;
use codexmanager_core::storage::Account;
use std::ffi::OsString;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode as TinyStatusCode};

fn jwt_with_json(payload_json: &str) -> String {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json);
    format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig")
}

struct EnvVarRestore {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvVarRestore {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn remove(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvVarRestore {
    fn drop(&mut self) {
        match self.original.as_ref() {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn token_with_refresh(account_id: &str, refresh_token: &str) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: refresh_token.to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    }
}

fn insert_account(storage: &Storage, account_id: &str) {
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: account_id.to_string(),
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
}

fn start_region_blocked_refresh_server() -> (
    String,
    std::sync::mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let server = Server::http("127.0.0.1:0").expect("start refresh mock server");
    let url = format!("http://{}/oauth/token", server.server_addr());
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(5))
            .expect("refresh mock timeout")
            .expect("receive refresh request");
        let mut body = String::new();
        request
            .as_reader()
            .read_to_string(&mut body)
            .expect("read refresh request body");
        tx.send(body).expect("send refresh request body");
        let response = Response::from_string("")
            .with_status_code(TinyStatusCode(403))
            .with_header(
                Header::from_bytes(
                    "x-openai-authorization-error",
                    "unsupported_country_region_territory",
                )
                .expect("auth error header"),
            )
            .with_header(Header::from_bytes("cf-ray", "ray-hkg").expect("cf-ray header"));
        request.respond(response).expect("respond refresh request");
    });
    (url, rx, handle)
}

#[test]
fn recover_refresh_race_uses_latest_token_when_refresh_token_changed() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_account(&storage, "acc-race");
    let mut token = token_with_refresh("acc-race", "refresh-old");
    storage.insert_token(&token).expect("insert old token");
    storage
        .insert_token(&token_with_refresh("acc-race", "refresh-new"))
        .expect("insert new token");

    let recovered = recover_refresh_race_from_latest_token(
        &storage,
        &mut token,
        "refresh-old",
        "refresh token failed with status 400 Bad Request: invalid_grant",
    )
    .expect("recover");

    assert!(recovered);
    assert_eq!(token.refresh_token, "refresh-new");
}

#[test]
fn recover_refresh_race_keeps_error_when_refresh_token_unchanged() {
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_account(&storage, "acc-no-race");
    let mut token = token_with_refresh("acc-no-race", "refresh-old");
    storage.insert_token(&token).expect("insert token");

    let recovered = recover_refresh_race_from_latest_token(
        &storage,
        &mut token,
        "refresh-old",
        "refresh token failed with status 400 Bad Request: invalid_grant",
    )
    .expect("recover");

    assert!(!recovered);
    assert_eq!(token.refresh_token, "refresh-old");
}

#[test]
fn refresh_and_persist_region_blocked_mock_suspends_account() {
    let _guard = crate::test_env_guard();
    let _ = crate::usage_http::usage_http_client();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    insert_account(&storage, "acc-region-blocked");
    let mut token = token_with_refresh("acc-region-blocked", "refresh-old");
    storage.insert_token(&token).expect("insert token");
    let (url, rx, handle) = start_region_blocked_refresh_server();
    let _restore = EnvVarRestore::set("CODEX_REFRESH_TOKEN_URL_OVERRIDE", &url);

    let err = refresh_and_persist_access_token(
        &storage,
        &mut token,
        "https://auth.openai.com",
        "client-id",
        token_refresh_ahead_secs(),
    )
    .expect_err("region blocked refresh should fail");
    let body = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("receive refresh request body");
    handle.join().expect("join refresh mock server");

    assert!(body.contains("refresh_token=refresh-old"));
    assert!(err.contains("auth_error=unsupported_country_region_territory"));
    assert!(
        crate::account_status::mark_account_unavailable_for_auth_error(
            &storage,
            "acc-region-blocked",
            &err,
        )
    );
    let account = storage
        .find_account_by_id("acc-region-blocked")
        .expect("find account")
        .expect("account exists");
    assert_eq!(account.status, "unavailable");
    let reasons = storage
        .latest_account_status_reasons(&["acc-region-blocked".to_string()])
        .expect("load reasons");
    assert_eq!(
        reasons.get("acc-region-blocked").map(String::as_str),
        Some(crate::account_status::REFRESH_TOKEN_REGION_BLOCKED_REASON)
    );
    let stored = storage
        .find_token_by_account_id("acc-region-blocked")
        .expect("load token")
        .expect("token exists");
    assert_eq!(stored.refresh_token, "refresh-old");
}

#[test]
fn token_refresh_ahead_secs_defaults_to_one_hour() {
    let _guard = crate::test_env_guard();
    let _restore = EnvVarRestore::remove(ENV_TOKEN_REFRESH_AHEAD_SECS);

    assert_eq!(token_refresh_ahead_secs(), DEFAULT_TOKEN_REFRESH_AHEAD_SECS);
}

#[test]
fn token_refresh_ahead_secs_reads_positive_env() {
    let _guard = crate::test_env_guard();
    let _restore = EnvVarRestore::set(ENV_TOKEN_REFRESH_AHEAD_SECS, "1800");

    assert_eq!(token_refresh_ahead_secs(), 1800);
}

#[test]
fn token_refresh_ahead_secs_ignores_invalid_env() {
    let _guard = crate::test_env_guard();
    let _restore = EnvVarRestore::set(ENV_TOKEN_REFRESH_AHEAD_SECS, "0");

    assert_eq!(token_refresh_ahead_secs(), DEFAULT_TOKEN_REFRESH_AHEAD_SECS);
}

#[test]
fn token_refresh_client_id_prefers_access_token_claim() {
    let token = Token {
        account_id: "acc-client-id".to_string(),
        id_token: jwt_with_json(r#"{"sub":"user-1","client_id":"client-from-id"}"#),
        access_token: jwt_with_json(r#"{"sub":"user-1","client_id":"client-from-access"}"#),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };

    assert_eq!(
        token_refresh_client_id(&token, "client-from-env"),
        "client-from-access"
    );
}

#[test]
fn token_refresh_client_id_falls_back_to_id_token_then_env() {
    let token = Token {
        account_id: "acc-client-id-fallback".to_string(),
        id_token: jwt_with_json(r#"{"sub":"user-1","client_id":"client-from-id"}"#),
        access_token: jwt_with_json(r#"{"sub":"user-1"}"#),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };
    assert_eq!(
        token_refresh_client_id(&token, "client-from-env"),
        "client-from-id"
    );

    let token_without_claim = Token {
        id_token: jwt_with_json(r#"{"sub":"user-1"}"#),
        ..token
    };
    assert_eq!(
        token_refresh_client_id(&token_without_claim, "client-from-env"),
        "client-from-env"
    );
}

#[test]
fn async_refresh_is_shared_per_account_and_independent_accounts_progress() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _env = crate::test_env_guard();
    let _ = crate::usage_http::usage_http_client();
    let storage = Arc::new(Storage::open_in_memory().unwrap());
    storage.init().unwrap();
    for id in ["async-refresh-a", "async-refresh-b"] {
        insert_account(&storage, id);
        storage.insert_token(&token_with_refresh(id, id)).unwrap();
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/oauth/token", listener.local_addr().unwrap());
    let _url = EnvVarRestore::set("CODEX_REFRESH_TOKEN_URL_OVERRIDE", &endpoint);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let requests = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Semaphore::new(0));
        let app = axum::Router::new().fallback({
            let requests = requests.clone();
            let started = started.clone();
            let release = release.clone();
            move |body: String| {
                let requests = requests.clone();
                let started = started.clone();
                let release = release.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    started.notify_one();
                    let permit = release.acquire().await.unwrap();
                    permit.forget();
                    let suffix = if body.contains("refresh_token=async-refresh-a") {
                        "a"
                    } else {
                        "b"
                    };
                    axum::Json(serde_json::json!({
                        "access_token": format!("access-new-{suffix}"),
                        "refresh_token": format!("refresh-new-{suffix}"),
                    }))
                }
            }
        });
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let refresh = |id: &'static str| {
            let storage = storage.clone();
            async move {
                let mut token = token_with_refresh(id, id);
                refresh_and_persist_access_token_async(
                    &storage,
                    &mut token,
                    "https://auth.openai.com",
                    "client",
                    60,
                )
                .await?;
                Ok::<_, String>(token)
            }
        };
        let first = tokio::spawn(refresh("async-refresh-a"));
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        let (cancel, cancelled) = tokio::sync::watch::channel(false);
        let cancelled_waiter =
            tokio::spawn(crate::http::gateway_request::scope_response_cancellation(
                cancelled,
                refresh("async-refresh-a"),
            ));
        tokio::task::yield_now().await;
        cancel.send_replace(true);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), cancelled_waiter)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err(),
            "token refresh cancelled",
        );
        let duplicate = tokio::spawn(refresh("async-refresh-a"));
        let independent = tokio::spawn(refresh("async-refresh-b"));
        tokio::time::timeout(Duration::from_secs(2), async {
            while requests.load(Ordering::SeqCst) < 2 {
                started.notified().await;
            }
        })
        .await
        .expect("the other account refresh runs while the first account waits");
        // Simulate another service instance winning the database update while
        // this instance's refresh HTTP request is still in flight.
        let external_winner = Token {
            access_token: "external-access-a".into(),
            refresh_token: "external-refresh-a".into(),
            ..token_with_refresh("async-refresh-a", "async-refresh-a")
        };
        storage.insert_token(&external_winner).unwrap();
        release.add_permits(2);
        let first = first.await.unwrap().unwrap();
        let duplicate = duplicate.await.unwrap().unwrap();
        let independent = independent.await.unwrap().unwrap();
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert_eq!(first.refresh_token, "external-refresh-a");
        assert_eq!(duplicate.refresh_token, "external-refresh-a");
        assert_eq!(independent.refresh_token, "refresh-new-b");
        assert_eq!(
            storage
                .find_token_by_account_id("async-refresh-a")
                .unwrap()
                .unwrap()
                .refresh_token,
            "external-refresh-a"
        );
        assert_eq!(
            storage
                .find_token_by_account_id("async-refresh-b")
                .unwrap()
                .unwrap()
                .refresh_token,
            "refresh-new-b"
        );
        server.abort();
        let _ = server.await;
    });
}

#[test]
fn cancelled_refresh_persists_rotated_grant_and_preserves_concurrent_credentials() {
    let _env = crate::test_env_guard();
    let _override = EnvVarRestore::remove("CODEX_REFRESH_TOKEN_URL_OVERRIDE");
    let _proxy = EnvVarRestore::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
    crate::clear_shutdown_flag();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for phase in 0..3 {
            let storage = Arc::new(Storage::open_in_memory().unwrap());
            storage.init().unwrap();
            let account_id = format!("cancel-rotation-{phase}");
            insert_account(&storage, &account_id);
            let initial = token_with_refresh(&account_id, "old-grant");
            storage.insert_token(&initial).unwrap();
            let (stage_tx, mut stage_rx) = tokio::sync::mpsc::unbounded_channel();
            let release_grant = Arc::new(tokio::sync::Semaphore::new(0));
            let release_exchange = Arc::new(tokio::sync::Semaphore::new(0));
            let app = axum::Router::new().fallback({
                let release_grant = release_grant.clone();
                let release_exchange = release_exchange.clone();
                move |body: String| {
                    let stage_tx = stage_tx.clone();
                    let release_grant = release_grant.clone();
                    let release_exchange = release_exchange.clone();
                    async move {
                        if body.contains("refresh_token=") {
                            stage_tx.send(0).unwrap();
                            release_grant.acquire().await.unwrap().forget();
                            axum::Json(serde_json::json!({"access_token":"rotated-access", "refresh_token":"rotated-grant", "id_token":"new-id"}))
                        } else {
                            stage_tx.send(1).unwrap();
                            release_exchange.acquire().await.unwrap().forget();
                            axum::Json(serde_json::json!({"access_token":"exchanged-api-key"}))
                        }
                    }
                }
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let issuer = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
            let waiter = tokio::spawn({
                let storage = storage.clone();
                async move {
                    let mut token = initial;
                    refresh_and_persist_access_token_async(&storage, &mut token, &issuer, "client", 60).await
                }
            });
            assert_eq!(tokio::time::timeout(Duration::from_secs(3), stage_rx.recv()).await.unwrap(), Some(0));
            if phase != 0 {
                release_grant.add_permits(1);
                assert_eq!(tokio::time::timeout(Duration::from_secs(3), stage_rx.recv()).await.unwrap(), Some(1));
                assert_eq!(storage.find_token_by_account_id(&account_id).unwrap().unwrap().refresh_token, "rotated-grant");
                if phase == 1 {
                    let winner = Token { access_token: "imported-access".into(), refresh_token: "imported-grant".into(), ..token_with_refresh(&account_id, "ignored") };
                    storage.insert_token(&winner).unwrap();
                } else {
                    storage.delete_account(&account_id).unwrap();
                }
            }
            waiter.abort();
            assert!(waiter.await.unwrap_err().is_cancelled());
            release_grant.add_permits(1);
            release_exchange.add_permits(1);
            tokio::time::timeout(Duration::from_secs(5), drain_token_refresh_tasks()).await.expect("detached credential completion drains");
            let stored = storage.find_token_by_account_id(&account_id).unwrap();
            match phase {
                0 => {
                    let stored = stored.unwrap();
                    assert_eq!(stored.refresh_token, "rotated-grant");
                    assert_eq!(stored.api_key_access_token.as_deref(), Some("exchanged-api-key"));
                }
                1 => {
                    let stored = stored.unwrap();
                    assert_eq!(stored.refresh_token, "imported-grant");
                    assert_eq!(stored.api_key_access_token, None);
                }
                _ => assert!(stored.is_none(), "completion must not resurrect deleted credentials"),
            }
            server.abort();
            let _ = server.await;
        }
    });
}
