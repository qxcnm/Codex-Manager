use super::*;

#[tokio::test]
async fn bps_rpc_denies_members_before_storage_or_network() {
    use codexmanager_core::rpc::types::JsonRpcRequest;
    let member = crate::RpcActor::from_parts(Some(crate::ROLE_MEMBER), Some("member-fixture"));
    for method in [
        "account/bps/update",
        "account/bps/login/start",
        "account/bps/login/status",
    ] {
        let req = JsonRpcRequest {
            id: 1.into(),
            method: method.into(),
            params: Some(
                json!({"accountId":"local","accountIds":["local"],"enabled":true,"loginId":"fixture"}),
            ),
            trace: None,
        };
        let response = crate::rpc_dispatch::handle_request_with_actor(req, member.clone());
        assert!(serde_json::to_string(&response)
            .unwrap()
            .contains("permission_denied"));
    }
    let req = JsonRpcRequest {
        id: 1.into(),
        method: "account/bps/login/start".into(),
        params: Some(json!({"accountId":"local"})),
        trace: None,
    };
    let response = crate::rpc_dispatch::try_handle_network_request_async(&req, &member)
        .await
        .unwrap();
    assert!(serde_json::to_string(&response)
        .unwrap()
        .contains("permission_denied"));
}

fn account() -> Account {
    Account {
        id: "local".into(),
        label: "local".into(),
        issuer: AUTH_BASE.into(),
        chatgpt_account_id: Some("upstream".into()),
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".into(),
        created_at: 1,
        updated_at: 1,
    }
}
fn jwt(client: &str, account: &str, user: &str, expiry: i64) -> String {
    let claims = json!({"client_id":client,"exp":expiry,"sub":user,"https://api.openai.com/auth":{"chatgpt_account_id":account,"chatgpt_user_id":user}});
    format!(
        "e30.{}.fixture",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
    )
}
#[test]
fn bps_authorization_requires_matching_application_account_user_and_expiry() {
    let original = Token {
        account_id: "local".into(),
        access_token: jwt("codex", "upstream", "user-a", 300),
        id_token: String::new(),
        refresh_token: "preserve".into(),
        api_key_access_token: None,
        last_refresh: 1,
    };
    let valid = json!({"access_token":jwt(BPS_CLIENT_ID,"upstream","user-a",300),"expires_in":50,"refresh_token":"independent","token_type":"Bearer"});
    let saved = validate_credentials(&account(), Some(&original), &valid, 100).unwrap();
    assert_eq!(saved.expires_at, Some(150));
    assert_eq!(saved.upstream_account_id.as_deref(), Some("upstream"));
    assert_eq!(original.refresh_token, "preserve");
    for token in [
        jwt("codex", "upstream", "user-a", 300),
        jwt(BPS_CLIENT_ID, "other", "user-a", 300),
        jwt(BPS_CLIENT_ID, "upstream", "other", 300),
        jwt(BPS_CLIENT_ID, "upstream", "user-a", 99),
    ] {
        assert!(validate_credentials(
            &account(),
            Some(&original),
            &json!({"access_token":token,"token_type":"Bearer"}),
            100
        )
        .is_err());
    }
}
#[test]
fn bps_login_status_is_bound_to_account_and_expires_without_exposing_secrets() {
    let id = codexmanager_core::auth::generate_state();
    sessions().lock().unwrap().insert(
        id.clone(),
        LoginSession {
            account_id: "local".into(),
            expires_at: 0,
            result: BpsLoginStatus {
                status: "pending".into(),
                error: None,
            },
            prompt: None,
        },
    );
    assert!(login_status("other", &id).is_err());
    let status = login_status("local", &id).unwrap();
    assert_eq!(status.status, "failed");
    assert!(!serde_json::to_string(&status).unwrap().contains("token"));
    sessions().lock().unwrap().remove(&id);
}

#[test]
fn bps_pkce_rejects_missing_mismatched_and_invalid_verifiers() {
    let verifier = "x".repeat(43);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    assert!(validate_pkce(&verifier, &challenge).is_ok());
    assert!(validate_pkce(&verifier, "wrong").is_err());
    assert!(validate_pkce("", "").is_err());
    assert!(validate_pkce(&"x".repeat(129), &challenge).is_err());
    assert!(validate_pkce(&format!("{}!", "x".repeat(42)), &challenge).is_err());
}

#[tokio::test]
async fn bps_reopening_pending_authorization_reuses_prompt_without_network() {
    let id = codexmanager_core::auth::generate_state();
    let account_id = format!("pending-{id}");
    let prompt = BpsLoginStart {
        login_id: id.clone(),
        user_code: "CODE".into(),
        verification_uri: "https://auth.openai.com/fixture".into(),
        interval: 60,
        expires_at: now_ts() + 900,
    };
    sessions().lock().unwrap().insert(
        id.clone(),
        LoginSession {
            account_id: account_id.clone(),
            expires_at: prompt.expires_at,
            result: BpsLoginStatus {
                status: "pending".into(),
                error: None,
            },
            prompt: Some(prompt),
        },
    );
    let reopened = login_start(&account_id).await.unwrap();
    assert_eq!(reopened.login_id, id);
    assert_eq!(reopened.interval, 60);
    sessions().lock().unwrap().remove(&id);
}

#[tokio::test]
async fn bps_unexplained_forbidden_is_terminal_and_server_poll_interval_is_preserved() {
    use axum::{http::StatusCode, routing::post, Json, Router};
    let app = Router::new()
        .route(
            "/api/accounts/deviceauth/usercode",
            post(|| async {
                Json(json!({"device_auth_id":"device","user_code":"CODE","interval":60}))
            }),
        )
        .route(
            "/api/accounts/deviceauth/token",
            post(|| async {
                (
                    StatusCode::FORBIDDEN,
                    Json(json!({"message":"private-error-must-not-leak"})),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let code = request_device_code(&client, &base).await.unwrap();
    assert_eq!(code.interval, 60);
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        poll_and_exchange(
            &client,
            &base,
            DeviceCode {
                interval: 1,
                ..code
            },
        ),
    )
    .await
    .expect("unknown rejection must not keep polling")
    .unwrap_err();
    assert!(error.contains("403"));
    assert!(!error.contains("private-error"));
    task.abort();
}

#[tokio::test]
async fn bps_device_flow_uses_independent_client_and_callback() {
    use axum::{
        extract::{Form, Query},
        routing::post,
        Json, Router,
    };
    use std::collections::HashMap;
    let _lock = crate::test_env_guard();
    struct EnvGuard(Option<std::ffi::OsString>);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.0 {
                std::env::set_var("CODEXMANAGER_DB_PATH", value);
            } else {
                std::env::remove_var("CODEXMANAGER_DB_PATH");
            }
        }
    }
    let path = std::env::temp_dir().join(format!(
        "bps-auth-test-{}.sqlite",
        codexmanager_core::auth::generate_state()
    ));
    let _env = EnvGuard(std::env::var_os("CODEXMANAGER_DB_PATH"));
    std::env::set_var("CODEXMANAGER_DB_PATH", &path);
    let db = codexmanager_core::storage::Storage::open(&path).unwrap();
    db.init().unwrap();
    db.insert_account(&account()).unwrap();
    let original = Token {
        account_id: "local".into(),
        access_token: jwt("codex", "upstream", "user-a", now_ts() + 3600),
        id_token: "codex-id".into(),
        refresh_token: "codex-refresh".into(),
        api_key_access_token: None,
        last_refresh: 1,
    };
    db.insert_token(&original).unwrap();
    async fn start(Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(body["client_id"], BPS_CLIENT_ID);
        Json(json!({"device_auth_id":"device","user_code":"CODE","interval":"1"}))
    }
    async fn poll(Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(
            body,
            json!({"client_id":BPS_CLIENT_ID,"device_auth_id":"device","user_code":"CODE"})
        );
        let verifier = "v".repeat(43);
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        Json(
            json!({"authorization_code":"auth-code","code_verifier":verifier,"code_challenge":challenge}),
        )
    }
    async fn exchange(
        Query(query): Query<HashMap<String, String>>,
        Form(body): Form<HashMap<String, String>>,
    ) -> Json<Value> {
        assert_eq!(query.get("unified").map(String::as_str), Some("true"));
        assert_eq!(body.get("client_id").unwrap(), BPS_CLIENT_ID);
        assert_eq!(body.get("redirect_uri").unwrap(), BPS_REDIRECT);
        assert_eq!(body.get("grant_type").unwrap(), "authorization_code");
        assert_eq!(body.get("code_verifier").unwrap(), &"v".repeat(43));
        Json(
            json!({"access_token":jwt(BPS_CLIENT_ID,"upstream","user-a",now_ts()+3600),"token_type":"Bearer","expires_in":3600,"refresh_token":"bps-refresh"}),
        )
    }
    let app = Router::new()
        .route("/api/accounts/deviceauth/usercode", post(start))
        .route("/api/accounts/deviceauth/token", post(poll))
        .route("/oauth/token", post(exchange));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let code = request_device_code(&client, &base).await.unwrap();
    assert_eq!(code.interval, 1);
    complete_login(client, &base, "local".into(), code, now_ts() + LOGIN_TTL)
        .await
        .unwrap();
    let settings = db.find_account_bps_settings("local").unwrap().unwrap();
    assert!(settings.authorized_at(now_ts()));
    assert!(
        !settings.enabled,
        "authorization alone does not change routing"
    );
    assert_eq!(settings.refresh_token.as_deref(), Some("bps-refresh"));
    let codex = db.find_token_by_account_id("local").unwrap().unwrap();
    assert_eq!(codex.access_token, original.access_token);
    assert_eq!(codex.refresh_token, original.refresh_token);
    task.abort();
}

#[tokio::test]
async fn bps_deviceauth_disabled_explains_security_setting_without_exposing_upstream_body() {
    use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Clone)]
    struct MockAuth {
        disabled_stage: usize,
        status: StatusCode,
        error: Value,
        calls: Arc<[AtomicUsize; 3]>,
    }

    fn reply(mock: MockAuth, stage: usize, success: Value) -> (StatusCode, Json<Value>) {
        mock.calls[stage].fetch_add(1, Ordering::Relaxed);
        if mock.disabled_stage == stage {
            (mock.status, Json(mock.error))
        } else {
            (StatusCode::OK, Json(success))
        }
    }

    async fn start(State(mock): State<MockAuth>) -> (StatusCode, Json<Value>) {
        reply(
            mock,
            0,
            json!({"device_auth_id":"device","user_code":"CODE","interval":1}),
        )
    }

    async fn poll(State(mock): State<MockAuth>) -> (StatusCode, Json<Value>) {
        let verifier = "v".repeat(43);
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        reply(
            mock,
            1,
            json!({"authorization_code":"code","code_verifier":verifier,"code_challenge":challenge}),
        )
    }

    async fn exchange(State(mock): State<MockAuth>) -> (StatusCode, Json<Value>) {
        reply(
            mock,
            2,
            json!({"access_token":"fixture-token","token_type":"Bearer"}),
        )
    }

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for disabled_stage in 0..3 {
        for status in [StatusCode::FORBIDDEN, StatusCode::OK] {
            for error in [
                json!({"error_code":"deviceauth_disabled","message":"private-upstream-description"}),
                json!({"error":"deviceauth_disabled","error_description":"private-upstream-description"}),
                json!({"error":{"code":"deviceauth_disabled","message":"private-upstream-description"}}),
            ] {
                let calls = Arc::new(std::array::from_fn(|_| AtomicUsize::new(0)));
                let app = Router::new()
                    .route("/api/accounts/deviceauth/usercode", post(start))
                    .route("/api/accounts/deviceauth/token", post(poll))
                    .route("/oauth/token", post(exchange))
                    .with_state(MockAuth {
                        disabled_stage,
                        status,
                        error,
                        calls: calls.clone(),
                    });
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let base = format!("http://{}", listener.local_addr().unwrap());
                let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
                let result = tokio::time::timeout(Duration::from_secs(3), async {
                    if disabled_stage == 0 {
                        request_device_code(&client, &base).await.err()
                    } else {
                        poll_and_exchange(
                            &client,
                            &base,
                            DeviceCode {
                                device_auth_id: "device".into(),
                                user_code: "CODE".into(),
                                interval: 0,
                            },
                        )
                        .await
                        .err()
                    }
                })
                .await
                .expect("disabled authorization must stop without polling again");
                server.abort();
                assert_eq!(result.as_deref(), Some(DEVICEAUTH_DISABLED_MESSAGE));
                assert!(!result.unwrap().contains("private-upstream-description"));
                assert_eq!(calls[disabled_stage].load(Ordering::Relaxed), 1);
                if disabled_stage < 2 {
                    assert_eq!(
                        calls[2].load(Ordering::Relaxed),
                        0,
                        "disabled authorization must never reach token exchange"
                    );
                }
            }
        }
    }
}
