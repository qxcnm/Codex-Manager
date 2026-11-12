use super::*;

fn actor_auth_test_state() -> AppState {
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);
    AppState {
        client: reqwest::Client::new(),
        service_rpc_url: "http://127.0.0.1:1/rpc".to_string(),
        service_addr: "127.0.0.1:1".to_string(),
        rpc_token: "auth-test-token".to_string(),
        web_auth_session_key: "auth-test-session".to_string(),
        shutdown_tx,
        spawned_service: Arc::new(tokio::sync::Mutex::new(false)),
        missing_ui_html: Arc::new(String::new()),
    }
}

#[test]
fn auth_mode_snapshot_keeps_accounts_failures_from_becoming_admin() {
    let state = actor_auth_test_state();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-codexmanager-rpc-actor-role",
        HeaderValue::from_static("system_admin"),
    );
    // A missing or revoked account session must fail, even if a password hash
    // from an adjacent settings transition or a forged role header is present.
    assert!(rpc_actor_for_auth_mode("accounts", &headers, &state, Some("hash")).is_none());

    let actor = rpc_actor_for_auth_mode("none", &headers, &state, None).unwrap();
    assert!(actor.is_admin());
    assert!(actor.user_id.is_none());
}

#[test]
fn password_auth_creates_an_admin_actor_only_after_cookie_validation() {
    let state = actor_auth_test_state();
    let mut headers = HeaderMap::new();
    assert!(rpc_actor_for_auth_mode("password", &headers, &state, Some("hash")).is_none());
    let cookie = build_web_auth_cookie_value("hash", &state.rpc_token, &state.web_auth_session_key);
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{WEB_AUTH_COOKIE_NAME}={cookie}")).unwrap(),
    );
    let actor = rpc_actor_for_auth_mode("password", &headers, &state, Some("hash")).unwrap();
    assert!(actor.is_admin());
    assert!(actor.user_id.is_none());
    assert!(rpc_actor_for_auth_mode("password", &headers, &state, None).is_none());
}

struct MiddlewareAuthDatabase {
    directory: std::path::PathBuf,
    original_environment: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl MiddlewareAuthDatabase {
    fn new() -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "codexmanager-web-actor-{}-{timestamp}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).expect("reserve private auth database directory");
        let original_environment = [
            "CODEXMANAGER_STORAGE_BACKEND",
            "CODEXMANAGER_DATABASE_URL",
            "CODEXMANAGER_DB_PATH",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        let fixture = Self {
            directory,
            original_environment,
        };
        std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
        std::env::remove_var("CODEXMANAGER_DATABASE_URL");
        std::env::set_var(
            "CODEXMANAGER_DB_PATH",
            fixture.directory.join("auth.sqlite"),
        );
        codexmanager_service::initialize_storage_if_needed().expect("initialize auth fixture");
        fixture
    }
}

impl Drop for MiddlewareAuthDatabase {
    fn drop(&mut self) {
        for (key, value) in self.original_environment.drain(..) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        // Delete only this fixture's named files; a retained SQLite pool may
        // still hold a Windows handle until the test process exits.
        for suffix in ["", "-wal", "-shm", ".model-catalog-v2.lock"] {
            let _ = std::fs::remove_file(self.directory.join(format!("auth.sqlite{suffix}")));
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[tokio::test]
async fn middleware_carries_the_authenticated_actor_through_session_revocation() {
    const CHILD_TEST_ENV: &str = "CODEXMANAGER_WEB_ACTOR_MIDDLEWARE_CHILD_TEST";
    let current_thread = std::thread::current();
    let test_name = current_thread.name().expect("named middleware test thread");
    if std::env::var(CHILD_TEST_ENV).as_deref() != Ok(test_name) {
        // Other Web tests can read settings without this fixture's env lock.
        // Keep its temporary DB authority private to a child that runs only
        // this test, including the first migration and later mode changes.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture"])
            .env(CHILD_TEST_ENV, test_name)
            .output()
            .expect("start isolated middleware test process");
        assert!(
            output.status.success(),
            "isolated middleware test failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }
    let _lock = test_env_guard();
    let _database = MiddlewareAuthDatabase::new();
    let state = Arc::new(actor_auth_test_state());
    let app = axum::Router::new()
        .route(
            "/api/actor",
            axum::routing::get(
                |axum::Extension(actor): axum::Extension<codexmanager_service::RpcActor>,
                 headers: HeaderMap| async move {
                    if actor.is_member() {
                        let token = parse_cookie_value(&headers, WEB_AUTH_COOKIE_NAME).unwrap();
                        codexmanager_service::logout_app_user_session(&token).unwrap();
                        assert!(codexmanager_service::resolve_app_user_session(&token)
                            .unwrap()
                            .is_none());
                    }
                    axum::Json(actor)
                },
            ),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            web_auth_middleware,
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind middleware test route");
    let url = format!("http://{}/api/actor", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    codexmanager_service::set_web_auth_mode("none").unwrap();
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let actor: serde_json::Value = response.json().await.unwrap();
    assert_eq!(actor["role"], "system_admin");
    assert!(actor["userId"].is_null());

    codexmanager_service::set_web_access_password(Some("middleware-test-password")).unwrap();
    codexmanager_service::set_web_auth_mode("password").unwrap();
    assert_eq!(
        client.get(&url).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = build_web_auth_cookie_value(
        &current_web_access_password_hash().unwrap(),
        &state.rpc_token,
        &state.web_auth_session_key,
    );
    let response = client
        .get(&url)
        .header(header::COOKIE, format!("{WEB_AUTH_COOKIE_NAME}={cookie}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let actor: serde_json::Value = response.json().await.unwrap();
    assert_eq!(actor["role"], "system_admin");
    assert!(actor["userId"].is_null());

    codexmanager_service::set_web_auth_mode("accounts").unwrap();
    let admin = codexmanager_service::bootstrap_app_admin(
        "middleware-admin",
        "middleware-admin-password",
        None,
    )
    .unwrap();
    let member = codexmanager_service::create_app_user(codexmanager_service::AppUserCreateInput {
        username: "middleware-member".to_string(),
        password: "middleware-member-password".to_string(),
        role: Some("member".to_string()),
        ..Default::default()
    })
    .unwrap();
    let session =
        codexmanager_service::login_app_user("middleware-member", "middleware-member-password")
            .unwrap();
    let response = client
        .get(&url)
        .header(
            header::COOKIE,
            format!("{WEB_AUTH_COOKIE_NAME}={}", admin.token),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let actor: serde_json::Value = response.json().await.unwrap();
    assert_eq!(actor["role"], "admin");
    assert_eq!(actor["userId"], admin.user.id);

    let member_cookie = format!("{WEB_AUTH_COOKIE_NAME}={}", session.token);
    let response = client
        .get(&url)
        .header(header::COOKIE, &member_cookie)
        .header("x-codexmanager-rpc-actor-role", "system_admin")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let actor: serde_json::Value = response.json().await.unwrap();
    assert_eq!(actor["role"], "member");
    assert_eq!(actor["userId"], member.id);
    for cookie in [None, Some(member_cookie)] {
        let mut request = client.get(&url);
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    server.abort();
    let _ = server.await;
}

#[test]
fn password_auth_rejects_a_hash_removed_after_mode_was_read() {
    let mut headers = HeaderMap::new();
    assert!(!password_request_is_authenticated(
        &headers,
        None,
        "rpc-token",
        "session-key",
    ));

    let old_cookie = build_web_auth_cookie_value("old-hash", "rpc-token", "session-key");
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{WEB_AUTH_COOKIE_NAME}={old_cookie}")).unwrap(),
    );
    assert!(!password_request_is_authenticated(
        &headers,
        None,
        "rpc-token",
        "session-key",
    ));
}

#[test]
fn password_auth_accepts_only_a_cookie_for_the_current_hash_and_session() {
    let mut headers = HeaderMap::new();
    assert!(!password_request_is_authenticated(
        &headers,
        Some("current-hash"),
        "rpc-token",
        "session-key",
    ));
    let cookie = build_web_auth_cookie_value("current-hash", "rpc-token", "session-key");
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!("{WEB_AUTH_COOKIE_NAME}={cookie}")).unwrap(),
    );
    assert!(password_request_is_authenticated(
        &headers,
        Some("current-hash"),
        "rpc-token",
        "session-key",
    ));
    assert!(!password_request_is_authenticated(
        &headers,
        Some("new-hash"),
        "rpc-token",
        "session-key",
    ));
    assert!(!password_request_is_authenticated(
        &headers,
        Some("current-hash"),
        "rpc-token",
        "new-session-key",
    ));
}

/// 函数 `login_force_requested_accepts_truthy_flags`
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
fn login_force_requested_accepts_truthy_flags() {
    for value in ["1", "true", "TRUE", "yes", "on"] {
        let query = LoginQuery {
            force: Some(value.to_string()),
        };
        assert!(login_force_requested(&query), "value={value}");
    }
    for value in ["", "0", "false", "no", "off"] {
        let query = LoginQuery {
            force: Some(value.to_string()),
        };
        assert!(!login_force_requested(&query), "value={value}");
    }
    assert!(!login_force_requested(&LoginQuery::default()));
}

/// 函数 `login_success_html_marks_current_tab_session`
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
fn login_success_html_marks_current_tab_session() {
    let html = login_success_html();
    assert!(html.contains("sessionStorage.setItem"));
    assert!(html.contains(WEB_AUTH_TAB_SESSION_STORAGE_KEY));
    assert!(html.contains("location.replace(\"/\")"));
}

#[test]
fn web_auth_allows_static_assets_without_session() {
    for path in [
        "/_next/static/chunks/app/page.js",
        "/_next/static/css/app.css",
        "/favicon.ico",
        "/author-alipay.jpg",
        "/manifest.json",
    ] {
        assert!(is_public_static_asset_path(path), "path={path}");
    }
    assert!(!is_public_static_asset_path("/settings"));
    assert!(!is_public_static_asset_path("/api/rpc"));
}
