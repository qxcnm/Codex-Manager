use super::*;
use axum::{extract::State, http::Uri, routing::post, Json, Router};
use base64::Engine;
use codexmanager_core::rpc::types::LoginStartResult;
use codexmanager_core::storage::{LoginSession, ProxyProfileCreateInput};
use std::future::IntoFuture;

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
            match old {
                Some(old) => std::env::set_var(key, old),
                None => std::env::remove_var(key),
            }
        }
    }
}

type ProxyRequests = Arc<Mutex<Vec<String>>>;

async fn mock_auth_proxy(
    State(requests): State<ProxyRequests>,
    uri: Uri,
    body: String,
) -> Json<serde_json::Value> {
    requests.lock().unwrap().push(uri.to_string());
    let value = match uri.path() {
        "/api/accounts/deviceauth/usercode" => serde_json::json!({
            "device_auth_id": "fixture-device", "user_code": "FIXTURE", "interval": 1,
        }),
        "/api/accounts/deviceauth/token" => serde_json::json!({
            "authorization_code": "device-proxy", "code_verifier": "fixture-verifier",
            "code_challenge": "fixture-challenge",
        }),
        "/oauth/token" if body.contains("grant_type=authorization_code") => {
            let subject = if body.contains("code=device-proxy") {
                "proxy-device"
            } else {
                "proxy-browser"
            };
            let claims = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                serde_json::json!({"sub":subject,"email":format!("{subject}@example.invalid")})
                    .to_string(),
            );
            serde_json::json!({
                "id_token":format!("e30.{claims}.fixture"),
                "access_token":"fixture-access", "refresh_token":"fixture-refresh",
            })
        }
        "/oauth/token" => serde_json::json!({"access_token":"fixture-api-key"}),
        _ => panic!("unexpected proxy request: {uri}"),
    };
    Json(value)
}

#[tokio::test(flavor = "current_thread")]
async fn login_proxy_routes_browser_and_device_auth_and_persists_account_binding() {
    let _lock = crate::test_env_guard();
    crate::clear_shutdown_flag();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", listener.local_addr().unwrap());
    let requests = ProxyRequests::default();
    let proxy = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/oauth/token", post(mock_auth_proxy))
                .route("/api/accounts/deviceauth/usercode", post(mock_auth_proxy))
                .route("/api/accounts/deviceauth/token", post(mock_auth_proxy))
                .with_state(requests.clone()),
        )
        .into_future(),
    );
    let db = std::env::temp_dir().join(format!(
        "codexmanager-login-proxy-{}-{}.sqlite",
        std::process::id(),
        now_ts()
    ));
    let _env = EnvGuard::set(&[
        ("CODEXMANAGER_DB_PATH", db.to_string_lossy().into_owned()),
        ("CODEXMANAGER_STORAGE_BACKEND", "sqlite".into()),
        // This name cannot resolve. Every successful HTTP request must use our local proxy.
        ("CODEXMANAGER_ISSUER", "http://login.example.invalid".into()),
        ("CODEXMANAGER_LOGIN_ADDR", "127.0.0.1:0".into()),
        (
            "CODEXMANAGER_REDIRECT_URI",
            "http://localhost:1455/auth/callback".into(),
        ),
        (
            "CODEXMANAGER_AUTO_USAGE_REFRESH_AFTER_ACCOUNT_ADD",
            "0".into(),
        ),
    ]);
    run_auth_storage(crate::storage_helpers::initialize_storage)
        .await
        .unwrap();
    run_auth_storage(move || {
        let storage = open_storage().unwrap();
        for (id, enabled) in [
            ("login-proxy", true),
            ("disabled-proxy", false),
            ("removed-proxy", true),
        ] {
            storage
                .create_proxy_profile(&ProxyProfileCreateInput {
                    id: id.into(),
                    name: id.into(),
                    proxy_url: proxy_url.clone(),
                    enabled,
                    tags_json: None,
                    notes: None,
                })
                .unwrap();
        }
        Ok(())
    })
    .await
    .unwrap();

    // Invalid selections are rejected before device-code creation or any outbound request.
    for id in ["unknown-proxy", "disabled-proxy"] {
        assert!(crate::auth_login::login_start_async(
            "chatgptDeviceCode",
            false,
            None,
            None,
            None,
            None,
            Some(id.into()),
        )
        .await
        .is_err());
    }
    assert!(requests.lock().unwrap().is_empty());

    let browser = crate::auth_login::login_start_async(
        "chatgpt",
        false,
        None,
        None,
        None,
        None,
        Some("login-proxy".into()),
    )
    .await
    .unwrap();
    let LoginStartResult::Chatgpt { login_id, auth_url } = browser else {
        panic!("browser login");
    };
    assert!(url::Url::parse(&auth_url)
        .unwrap()
        .query_pairs()
        .all(|(key, _)| { key != "proxyProfileId" && key != "proxyUrl" }));
    let browser_id = login_id.clone();
    run_auth_storage(move || {
        let session = open_storage()
            .unwrap()
            .get_login_session(&browser_id)
            .unwrap()
            .unwrap();
        assert_eq!(session.proxy_profile_id.as_deref(), Some("login-proxy"));
        Ok(())
    })
    .await
    .unwrap();
    // Both the automatic callback and manual completion share this implementation.
    complete_login_with_redirect_async(&login_id, "browser-proxy", None)
        .await
        .unwrap();
    assert_eq!(requests.lock().unwrap().len(), 2);

    let device = crate::auth_login::login_start_async(
        "chatgptDeviceCode",
        false,
        None,
        None,
        None,
        None,
        Some("login-proxy".into()),
    )
    .await
    .unwrap();
    let LoginStartResult::ChatgptDeviceCode { login_id, .. } = device else {
        panic!("device login");
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let id = login_id.clone();
            let status = run_auth_storage(move || {
                Ok(open_storage()
                    .unwrap()
                    .get_login_session(&id)
                    .unwrap()
                    .unwrap()
                    .status)
            })
            .await
            .unwrap();
            if status == "success" {
                break;
            }
            assert_eq!(
                status == "pending" || status == "completing",
                true,
                "{status}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    run_auth_storage(|| {
        let storage = open_storage().unwrap();
        let accounts = storage.list_accounts().unwrap();
        assert_eq!(accounts.len(), 2);
        for account in accounts {
            let settings = storage
                .find_account_proxy_settings(&account.id)
                .unwrap()
                .unwrap();
            assert!(settings.enabled);
            assert_eq!(settings.proxy_source.as_deref(), Some("profile"));
            assert_eq!(settings.proxy_profile_id.as_deref(), Some("login-proxy"));
            assert!(settings.proxy_url.is_none());
            crate::auth::login_proxy::persist_login_proxy(&storage, &account.id, None)?;
            assert_eq!(
                storage
                    .find_account_proxy_settings(&account.id)
                    .unwrap()
                    .unwrap()
                    .proxy_profile_id,
                settings.proxy_profile_id
            );
        }
        // A profile removed while waiting for browser authorization fails the session immediately.
        storage
            .insert_login_session(&LoginSession {
                login_id: "removed-profile".into(),
                state: "removed-profile".into(),
                code_verifier: "fixture".into(),
                status: "pending".into(),
                error: None,
                workspace_id: None,
                note: None,
                tags: None,
                group_name: None,
                proxy_profile_id: Some("removed-proxy".into()),
                created_at: now_ts(),
                updated_at: now_ts(),
            })
            .unwrap();
        storage.delete_proxy_profile("removed-proxy").unwrap();
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        complete_login_with_redirect_async("removed-profile", "unused", None)
            .await
            .is_err()
    );
    run_auth_storage(|| {
        let storage = open_storage().unwrap();
        assert_eq!(
            storage
                .get_login_session("removed-profile")
                .unwrap()
                .unwrap()
                .status,
            "failed"
        );
        assert_eq!(storage.list_accounts().unwrap().len(), 2);
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(requests.lock().unwrap().len(), 6);
    assert!(requests
        .lock()
        .unwrap()
        .iter()
        .all(|uri| uri.starts_with("http://login.example.invalid/")));
    drain_device_login_tasks().await;
    drain_auth_completions().await;
    proxy.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn login_proxy_connection_failure_never_retries_directly() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let unavailable = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", unavailable.local_addr().unwrap());
    drop(unavailable);
    assert!(
        request_device_code_async(&issuer, "fixture", Some(&proxy_url))
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    assert!(auth_http_client_with_proxy(&issuer, Some("unsupported://fixture")).is_err());
}
