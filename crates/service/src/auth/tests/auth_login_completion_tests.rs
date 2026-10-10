use super::*;
use axum::{extract::State, routing::post, Json, Router};
use base64::Engine;
use std::future::IntoFuture;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

struct EnvGuard(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl EnvGuard {
    fn set(values: &[(&'static str, String)]) -> Self {
        Self(
            values
                .iter()
                .map(|(key, value)| {
                    let old = std::env::var_os(key);
                    std::env::set_var(key, value);
                    (*key, old)
                })
                .collect(),
        )
    }
}
impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, old) in self.0.drain(..) {
            if let Some(old) = old {
                std::env::set_var(key, old);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[derive(Clone, Default)]
struct ProviderState {
    requests: Arc<AtomicUsize>,
    waiting: Arc<Notify>,
}

async fn token_provider(
    State(state): State<ProviderState>,
    body: String,
) -> Json<serde_json::Value> {
    state.requests.fetch_add(1, Ordering::SeqCst);
    if body.contains("code=slow") {
        state.waiting.notify_one();
        std::future::pending::<()>().await;
    }
    if body.contains("grant_type=authorization_code") {
        let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::json!({"sub":"async-login-user","email":"fixture@example.invalid"})
                .to_string(),
        );
        Json(
            serde_json::json!({"id_token":format!("e30.{claims}.fixture"),"access_token":"fixture-access","refresh_token":"fixture-refresh"}),
        )
    } else {
        Json(serde_json::json!({"access_token":"fixture-api-key"}))
    }
}

async fn device_usercode_provider(State(state): State<ProviderState>) -> Json<serde_json::Value> {
    state.requests.fetch_add(1, Ordering::SeqCst);
    Json(serde_json::json!({
        "device_auth_id": "fixture-device", "user_code": "fixture-code", "interval": 1
    }))
}

async fn pending_device_provider() {
    std::future::pending::<()>().await;
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_cancels_device_poll_and_persists_terminal_session_before_drain() {
    use tokio::io::AsyncReadExt;
    let _lock = crate::test_env_guard();
    crate::clear_shutdown_flag();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let (ready, accepted) = tokio::sync::oneshot::channel();
    let provider = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 8192];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        ready.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), socket.read(&mut buffer))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let db = std::env::temp_dir().join(format!(
        "codexmanager-device-shutdown-{}-{}.sqlite",
        std::process::id(),
        now_ts()
    ));
    let _env = EnvGuard::set(&[
        ("CODEXMANAGER_DB_PATH", db.to_string_lossy().into_owned()),
        ("CODEXMANAGER_STORAGE_BACKEND", "sqlite".into()),
    ]);
    run_auth_storage(crate::storage_helpers::initialize_storage)
        .await
        .unwrap();
    seed_session("device-shutdown").await;
    let code = || DeviceCodeStartResult {
        verification_url: issuer.clone(),
        user_code: "fixture".into(),
        device_auth_id: "fixture".into(),
        interval: 1,
    };
    spawn_device_code_login_completion(issuer.clone(), "device-shutdown".into(), code()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), accepted)
        .await
        .unwrap()
        .unwrap();
    crate::request_shutdown("");
    assert!(spawn_device_code_login_completion(issuer.clone(), "rejected".into(), code()).is_err());
    tokio::time::timeout(Duration::from_secs(3), drain_device_login_tasks())
        .await
        .unwrap();
    drain_auth_completions().await;
    provider.await.unwrap();
    let session = run_auth_storage(|| {
        Ok(open_storage()
            .unwrap()
            .get_login_session("device-shutdown")
            .unwrap()
            .unwrap())
    })
    .await
    .unwrap();
    assert_eq!(session.status, "cancelled");
    assert!(lock_active_device_login_tasks().is_empty());
    crate::clear_shutdown_flag();
}

#[tokio::test(flavor = "current_thread")]
async fn native_auth_rpc_preserves_permissions_and_awaits_device_and_code_provider() {
    let _lock = crate::test_env_guard();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let state = ProviderState::default();
    let provider = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/rpc", post(crate::http::rpc_endpoint::handle_rpc_http))
                .route("/oauth/token", post(token_provider))
                .route(
                    "/api/accounts/deviceauth/usercode",
                    post(device_usercode_provider),
                )
                .route(
                    "/api/accounts/deviceauth/token",
                    post(pending_device_provider),
                )
                .with_state(state.clone()),
        )
        .into_future(),
    );
    let db = std::env::temp_dir().join(format!(
        "codexmanager-async-auth-rpc-{}-{}.sqlite",
        std::process::id(),
        now_ts(),
    ));
    let _env = EnvGuard::set(&[
        ("CODEXMANAGER_DB_PATH", db.to_string_lossy().into_owned()),
        ("CODEXMANAGER_STORAGE_BACKEND", "sqlite".into()),
        ("CODEXMANAGER_ISSUER", issuer.clone()),
        (
            "CODEXMANAGER_REDIRECT_URI",
            format!("{issuer}/auth/callback"),
        ),
        (
            "CODEXMANAGER_AUTO_USAGE_REFRESH_AFTER_ACCOUNT_ADD",
            "0".into(),
        ),
    ]);
    run_auth_storage(crate::storage_helpers::initialize_storage)
        .await
        .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = |role: &str, method: &str, params: serde_json::Value| {
        client
            .post(format!("{issuer}/rpc"))
            .header("X-CodexManager-Rpc-Token", crate::rpc_auth_token())
            .header("X-CodexManager-Rpc-Actor-Role", role)
            .json(&serde_json::json!({"jsonrpc":"2.0","id":71,"method":method,"params":params}))
    };
    for method in ["account/login/start", "account/login/complete"] {
        let denied: serde_json::Value =
            request("member", method, serde_json::json!({"type":"device"}))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert!(denied["result"]["error"].is_string(), "{denied}");
    }
    assert_eq!(
        state.requests.load(Ordering::SeqCst),
        0,
        "authorization precedes network"
    );
    let started: serde_json::Value = tokio::time::timeout(Duration::from_secs(5), async {
        request(
            "system_admin",
            "account/login/start",
            serde_json::json!({"type":"device","openBrowser":false}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
    })
    .await
    .expect("device RPC awaits a provider on the caller runtime");
    let login_id = started["result"]["loginId"]
        .as_str()
        .expect("device login id")
        .to_owned();
    assert_eq!(started["result"]["userCode"], "fixture-code");
    let cancelled_id = login_id.clone();
    run_auth_storage(move || crate::auth_login::login_cancel(&cancelled_id).map(|_| ()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while ACTIVE_DEVICE_LOGIN_TASKS
            .get()
            .is_some_and(|tasks| tasks.lock().unwrap().contains_key(&login_id))
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("device task stops before fixture shutdown");
    seed_session("rpc-complete").await;
    let completed: serde_json::Value = tokio::time::timeout(Duration::from_secs(5), async {
        request(
            "system_admin",
            "account/login/complete",
            serde_json::json!({"state":"rpc-complete","code":"ok"}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
    })
    .await
    .expect("completion RPC awaits the shared runtime provider");
    assert_eq!(completed["result"]["ok"], true, "{completed}");
    assert_eq!(state.requests.load(Ordering::SeqCst), 3);
    run_auth_storage(|| {
        let storage = open_storage().unwrap();
        assert_eq!(
            storage
                .get_login_session("rpc-complete")
                .unwrap()
                .unwrap()
                .status,
            "success"
        );
        Ok(())
    })
    .await
    .unwrap();
    provider.abort();
}

async fn seed_session(id: &'static str) {
    run_auth_storage(move || {
        let storage = open_storage().ok_or_else(|| "storage unavailable".to_owned())?;
        storage
            .insert_login_session(&LoginSession {
                login_id: id.into(),
                state: id.into(),
                code_verifier: "fixture-verifier".into(),
                status: "pending".into(),
                error: None,
                workspace_id: None,
                note: None,
                tags: None,
                group_name: None,
                created_at: now_ts(),
                updated_at: now_ts(),
            })
            .map_err(|e| e.to_string())
    })
    .await
    .unwrap();
}

// One runtime thread also serves the mock token endpoint. Blocking network
// compatibility inside the callback would prevent this test from completing.
#[tokio::test(flavor = "current_thread")]
async fn async_callback_awaits_network_and_persists_login() {
    let _lock = crate::test_env_guard();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let state = ProviderState::default();
    let provider = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/oauth/token", post(token_provider))
                .with_state(state.clone()),
        )
        .into_future(),
    );
    let db = std::env::temp_dir().join(format!(
        "codexmanager-async-login-{}-{}.sqlite",
        std::process::id(),
        now_ts()
    ));
    let _env = EnvGuard::set(&[
        ("CODEXMANAGER_DB_PATH", db.to_string_lossy().into_owned()),
        ("CODEXMANAGER_STORAGE_BACKEND", "sqlite".into()),
        ("CODEXMANAGER_ISSUER", issuer.clone()),
        (
            "CODEXMANAGER_REDIRECT_URI",
            format!("{issuer}/auth/callback"),
        ),
        (
            "CODEXMANAGER_AUTO_USAGE_REFRESH_AFTER_ACCOUNT_ADD",
            "0".into(),
        ),
    ]);
    run_auth_storage(crate::storage_helpers::initialize_storage)
        .await
        .unwrap();
    seed_session("async-success").await;
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        crate::http::callback_endpoint::handle_callback_http(
            "/auth/callback?state=async-success&code=ok"
                .parse()
                .unwrap(),
        ),
    )
    .await
    .expect("callback yields to token provider");
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(state.requests.load(Ordering::SeqCst), 2);
    run_auth_storage(|| {
        let storage = open_storage().unwrap();
        let session = storage.get_login_session("async-success").unwrap().unwrap();
        assert_eq!(session.status, "success");
        assert!(session.code_verifier.is_empty());
        let accounts = storage.list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        let token = storage
            .find_token_by_account_id(&accounts[0].id)
            .unwrap()
            .unwrap();
        assert_eq!(token.access_token, "fixture-access");
        assert_eq!(
            token.api_key_access_token.as_deref(),
            Some("fixture-api-key")
        );
        Ok(())
    })
    .await
    .unwrap();

    seed_session("async-cancel").await;
    let callback = tokio::spawn(crate::http::callback_endpoint::handle_callback_http(
        "/auth/callback?state=async-cancel&code=slow"
            .parse()
            .unwrap(),
    ));
    tokio::time::timeout(Duration::from_secs(5), state.waiting.notified())
        .await
        .unwrap();
    assert_eq!(
        AUTH_STORAGE_WORKERS.available_permits(),
        8,
        "network wait releases all persistence workers"
    );
    callback.abort();
    assert!(callback.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let finished = run_auth_storage(|| {
                let storage = open_storage().unwrap();
                let session = storage.get_login_session("async-cancel").unwrap().unwrap();
                assert_eq!(storage.list_accounts().unwrap().len(), 1);
                Ok(session.status == "failed" && session.code_verifier.is_empty())
            })
            .await
            .unwrap();
            if finished {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("cancelled callback releases completing state");

    seed_session("async-replaced").await;
    run_auth_storage(|| {
        let storage = open_storage().unwrap();
        let expected = storage
            .get_login_session("async-replaced")
            .unwrap()
            .unwrap();
        assert!(storage
            .claim_login_session_for_completion("async-replaced")
            .unwrap());
        storage
            .update_login_session_code_verifier("async-replaced", "replacement-verifier")
            .unwrap();
        finish_failed(&expected, "stale completion cancelled")?;
        let current = storage
            .get_login_session("async-replaced")
            .unwrap()
            .unwrap();
        assert_eq!(current.status, "completing");
        assert_eq!(current.code_verifier, "replacement-verifier");
        assert!(storage
            .finish_login_session("async-replaced", "success", None)
            .unwrap());
        finish_failed(&current, "late completion cancelled")?;
        assert_eq!(
            storage
                .get_login_session("async-replaced")
                .unwrap()
                .unwrap()
                .status,
            "success"
        );
        Ok(())
    })
    .await
    .unwrap();
    provider.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn device_poll_cancels_while_upstream_is_silent() {
    let _lock = crate::test_env_guard();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let accepted = Arc::new(Notify::new());
    let started = accepted.clone();
    let provider = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        started.notify_one();
        std::future::pending::<()>().await;
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let trigger = cancel.clone();
    let signal = tokio::spawn(async move {
        accepted.notified().await;
        trigger.store(true, Ordering::SeqCst);
    });
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        poll_device_auth_token_async_with_timeout(
            &issuer,
            "device",
            "code",
            1,
            Duration::from_secs(60),
            cancel,
        ),
    )
    .await
    .expect("cancellation interrupts connect/header wait");
    assert!(matches!(result, Err(DeviceLoginError::Cancelled)));
    signal.await.unwrap();
    provider.abort();
}
