use super::*;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use codexmanager_core::storage::{Account, Token};

struct PoolFixture {
    storage: crate::storage_helpers::StorageHandle,
    previous_env: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl PoolFixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "account-pool-permissions-{}-{nonce}.db",
            std::process::id()
        ));
        let previous_env = [
            "CODEXMANAGER_STORAGE_BACKEND",
            "CODEXMANAGER_DATABASE_URL",
            "CODEXMANAGER_DB_PATH",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
        std::env::remove_var("CODEXMANAGER_DATABASE_URL");
        std::env::set_var("CODEXMANAGER_DB_PATH", &path);
        crate::storage_helpers::initialize_storage().expect("initialize pool fixture");
        let storage = crate::storage_helpers::open_storage().expect("pool fixture storage");
        storage
            .insert_account(&Account {
                id: "operator-pool".into(),
                label: "Operator account".into(),
                issuer: "https://auth.openai.com".into(),
                chatgpt_account_id: Some("chatgpt-pool".into()),
                workspace_id: Some("workspace-pool".into()),
                group_name: None,
                sort: 0,
                status: "active".into(),
                created_at: 1,
                updated_at: 1,
            })
            .unwrap();
        storage
            .insert_token(&Token {
                account_id: "operator-pool".into(),
                id_token: "fixture-id-token".into(),
                access_token: "fixture-upstream-secret".into(),
                // Admin controls terminate locally, without contacting a provider.
                refresh_token: String::new(),
                api_key_access_token: None,
                last_refresh: 1,
            })
            .unwrap();
        crate::auth_account::set_current_auth_account_id(Some("operator-pool")).unwrap();
        crate::set_web_access_password(Some("pool-permission-fixture")).unwrap();
        Self {
            storage,
            previous_env,
        }
    }

    fn set_mode(&self, mode: &str) {
        crate::set_web_auth_mode(mode).expect("set pool fixture auth mode");
        assert_eq!(crate::current_web_auth_mode(), mode);
    }
}

impl Drop for PoolFixture {
    fn drop(&mut self) {
        for (key, value) in self.previous_env.drain(..) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn request(method: &str, params: Option<Value>) -> JsonRpcRequest {
    JsonRpcRequest {
        id: 91.into(),
        method: method.into(),
        params,
        trace: None,
    }
}

fn result(message: JsonRpcMessage) -> Value {
    match message {
        JsonRpcMessage::Response(response) => response.result,
        other => panic!("expected RPC response, got {other:?}"),
    }
}

fn assert_denied(value: &Value, method: &str) {
    assert_eq!(value["errorCode"], "permission_denied", "{method}: {value}");
    assert!(value.get("accessToken").is_none(), "{method}: {value}");
    assert!(value.get("items").is_none(), "{method}: {value}");
}

#[test]
fn member_response_settings_preserve_bootstrap_without_deployment_configuration() {
    let _guard = crate::test_env_guard();
    let _fixture = PoolFixture::new();
    let member = RpcActor::from_parts(Some("member"), Some("member-user"));
    let settings = result(handle_request_with_actor(
        request("appSettings/get", None),
        member,
    ));
    for field in [
        "serviceAddr",
        "theme",
        "appearancePreset",
        "locale",
        "lowTransparency",
        "webAuthMode",
    ] {
        assert!(
            settings.get(field).is_some(),
            "missing bootstrap field {field}"
        );
    }
    for field in [
        "envOverrides",
        "envOverrideCatalog",
        "upstreamProxyUrl",
        "pluginMarketSourceUrl",
        "gatewayUserAgent",
    ] {
        assert!(
            settings.get(field).is_none(),
            "member received deployment field {field}"
        );
    }
    let admin = result(handle_request_with_actor(
        request("appSettings/get", None),
        RpcActor::system_admin(),
    ));
    for field in ["envOverrides", "envOverrideCatalog", "upstreamProxyUrl"] {
        assert!(
            admin.get(field).is_some(),
            "admin lost configuration field {field}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn member_response_key_list_and_startup_hide_operator_credentials() {
    use codexmanager_core::storage::{ApiKey, ApiKeyOwner};
    let _guard = crate::test_env_guard();
    let fixture = PoolFixture::new();
    for user_id in ["member-user", "other-user"] {
        fixture
            .storage
            .insert_app_user(&codexmanager_core::storage::AppUser {
                id: user_id.into(),
                username: format!("fixture-{user_id}"),
                display_name: None,
                password_hash: "fixture-only".into(),
                role: "member".into(),
                status: "active".into(),
                created_at: 1,
                updated_at: 1,
                last_login_at: None,
            })
            .unwrap();
    }
    for id in ["member-owned", "other-owned"] {
        fixture.storage.insert_api_key(&ApiKey {
            id: id.into(), name: Some(id.into()), model_slug: Some("gpt-5".into()),
            reasoning_effort: None, service_tier: None, rotation_strategy: "account_rotation".into(),
            aggregate_api_id: None, account_plan_filter: Some("operator-plan".into()),
            aggregate_api_url: None, client_type: "codex".into(), protocol_type: "openai_compat".into(),
            auth_scheme: "authorization_bearer".into(),
            upstream_base_url: Some("https://fixture.invalid/?token=operator-secret".into()),
            static_headers_json: Some(r#"{"Authorization":"Bearer fixture-upstream-secret","Cookie":"fixture-cookie"}"#.into()),
            key_hash: format!("fixture-{id}"), status: "active".into(), created_at: 1, last_used_at: None,
        }).unwrap();
        fixture
            .storage
            .upsert_api_key_owner(&ApiKeyOwner {
                key_id: id.into(),
                owner_kind: "user".into(),
                owner_user_id: Some(
                    if id == "member-owned" {
                        "member-user"
                    } else {
                        "other-user"
                    }
                    .into(),
                ),
                project_id: None,
                updated_at: 1,
            })
            .unwrap();
    }
    let member = RpcActor::from_parts(Some("member"), Some("member-user"));
    let listed = result(handle_request_with_actor(
        request("apikey/list", None),
        member.clone(),
    ));
    let startup = crate::startup_snapshot::read_startup_snapshot_for_actor(
        &member,
        Some(1),
        None,
        None,
        false,
        true,
        false,
        false,
        false,
        false,
    )
    .unwrap();
    for keys in [
        listed["items"].clone(),
        serde_json::to_value(startup.api_keys).unwrap(),
    ] {
        assert_eq!(keys.as_array().unwrap().len(), 1);
        assert_eq!(keys[0]["id"], "member-owned");
        assert_eq!(keys[0]["modelSlug"], "gpt-5");
        for field in [
            "upstreamBaseUrl",
            "staticHeadersJson",
            "aggregateApiUrl",
            "aggregateApiId",
            "accountPlanFilter",
            "accountGroupFilter",
        ] {
            assert!(
                keys[0][field].is_null(),
                "member received operator field {field}"
            );
        }
    }
    let admin = result(handle_request_with_actor(
        request("apikey/list", None),
        RpcActor::system_admin(),
    ));
    assert_eq!(admin["items"].as_array().unwrap().len(), 2);
    assert!(admin["items"][0]["staticHeadersJson"]
        .as_str()
        .unwrap()
        .contains("fixture-upstream-secret"));

    // Exercise the real listener, trusted actor headers, and both RPC dispatch paths.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(crate::http::router::serve(
        listener,
        crate::http::state::AppState::new(),
        async move {
            let _ = stop_rx.await;
        },
    ));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for req in [
        request("apikey/list", None),
        request("appSettings/get", None),
        request(
            "startup/snapshot",
            Some(serde_json::json!({
                "includeApiModels": false, "includeAccounts": false,
                "includeUsageSnapshots": false, "includeAccountRuntime": false,
                "includeAccountDetails": false,
            })),
        ),
    ] {
        let response = client
            .post(format!("http://{addr}/rpc"))
            .header("x-codexmanager-rpc-token", crate::rpc_auth_token())
            .header("x-codexmanager-rpc-actor-role", "member")
            .header("x-codexmanager-rpc-actor-user-id", "member-user")
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let payload: Value = response.json().await.unwrap();
        assert!(
            payload["result"].get("error").is_none(),
            "RPC {} failed",
            req.method
        );
        let serialized = payload.to_string();
        assert!(!serialized.contains("fixture-upstream-secret"));
        assert!(!serialized.contains("fixture-cookie"));
        assert!(!serialized.contains("operator-secret"));
        assert!(!serialized.contains("envOverrideCatalog"));
        if req.method == "apikey/list" {
            assert_eq!(payload["result"]["items"].as_array().unwrap().len(), 1);
        }
    }
    stop_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}

#[test]
fn account_pool_policy_is_admin_only_in_every_auth_mode() {
    let _guard = crate::test_env_guard();
    let fixture = PoolFixture::new();
    let member = RpcActor::from_parts(Some("member"), Some("member-user"));
    for mode in ["accounts", "password", "none"] {
        fixture.set_mode(mode);
        for method in [
            "account/list",
            "account/read",
            "account/chatgptAuthTokens/refresh",
            "account/chatgptAuthTokens/refreshAll",
            "account/update",
            "account/updateSorts",
            "account/resetWarmup/update",
            "account/proxy/get",
            "account/proxy/set",
            "account/proxy/test",
            "account/usage/read",
            "account/usage/refresh",
            "account/warmup",
            "account/login/start",
            "account/logout",
            "account/export",
            "account/futureManagementMethod",
            "accountManager/webAuthMode/set",
            "accountManager/users/list",
            "appSettings/set",
            "codexProfile/applyDirectAccount",
        ] {
            assert!(
                ensure_method_allowed(&member, method).is_err(),
                "{mode}: {method}"
            );
            for role in ["admin", "system_admin"] {
                assert!(
                    ensure_method_allowed(&RpcActor::from_parts(Some(role), None), method).is_ok()
                );
            }
        }
        for method in [
            "accountManager/profile/update",
            "accountManager/password/change",
            "accountManager/session/current",
            "apikey/list",
            "requestlog/list",
        ] {
            assert!(
                ensure_method_allowed(&member, method).is_ok(),
                "{mode}: {method}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_pool_member_requests_are_denied_before_dispatch_or_side_effects() {
    let _guard = crate::test_env_guard();
    let fixture = PoolFixture::new();
    let member = RpcActor::from_parts(Some("member"), Some("member-user"));
    let mut requests = vec![
        request("account/list", None),
        request(
            "account/read",
            Some(serde_json::json!({"refreshToken": true})),
        ),
        request("account/chatgptAuthTokens/refreshAll", None),
        request("account/chatgptAuthTokens/refresh", None),
        request(
            "accountManager/webAuthMode/set",
            Some(serde_json::json!({"mode": "none"})),
        ),
        request(
            "account/chatgptAuthTokens/refresh",
            Some(serde_json::json!({"accountId": " "})),
        ),
        request(
            "account/update",
            Some(serde_json::json!({"accountId": "operator-pool", "sort": 99})),
        ),
    ];
    for key in [
        "accountId",
        "account_id",
        "previousAccountId",
        "previous_account_id",
    ] {
        for target in ["operator-pool", "chatgpt-pool", "workspace-pool"] {
            requests.push(request(
                "account/chatgptAuthTokens/refresh",
                Some(serde_json::json!({(key): target})),
            ));
        }
    }
    for mode in ["accounts", "password", "none"] {
        fixture.set_mode(mode);
        for req in &requests {
            assert_denied(
                &result(handle_request_with_actor(req.clone(), member.clone())),
                &req.method,
            );
            if is_async_method(&req.method) {
                let message = try_handle_network_request_async(req, &member)
                    .await
                    .unwrap();
                assert_denied(&result(message), &req.method);
            }
            // Same trusted headers that the Web proxy sends, through the real HTTP handler.
            let http_request = Request::builder()
                .method("POST")
                .uri("/rpc")
                .header("content-type", "application/json")
                .header("x-codexmanager-rpc-token", crate::rpc_auth_token())
                .header("x-codexmanager-rpc-actor-role", "member")
                .header("x-codexmanager-rpc-actor-user-id", "member-user")
                .body(Body::from(serde_json::to_vec(req).unwrap()))
                .unwrap();
            let response = crate::http::rpc_endpoint::handle_rpc_http(http_request).await;
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            let payload: Value = serde_json::from_slice(&body).unwrap();
            assert_denied(&payload["result"], &req.method);
        }
        let token = fixture
            .storage
            .find_token_by_account_id("operator-pool")
            .unwrap()
            .unwrap();
        assert_eq!(token.access_token, "fixture-upstream-secret");
        assert_eq!(token.last_refresh, 1);
        assert_eq!(
            fixture
                .storage
                .find_account_by_id("operator-pool")
                .unwrap()
                .unwrap()
                .sort,
            0
        );
        for actor in [
            RpcActor::system_admin(),
            RpcActor::from_parts(Some("admin"), Some("admin-user")),
        ] {
            let listed = result(handle_request_with_actor(
                request("account/list", None),
                actor.clone(),
            ));
            assert_eq!(listed["items"][0]["id"], "operator-pool");
            let refresh = request(
                "account/chatgptAuthTokens/refresh",
                Some(serde_json::json!({"accountId": "operator-pool"})),
            );
            let expected_error = "target account does not have refresh_token";
            assert_eq!(
                result(handle_request_with_actor(refresh.clone(), actor.clone()))["error"],
                expected_error
            );
            assert_eq!(
                result(
                    try_handle_network_request_async(&refresh, &actor)
                        .await
                        .unwrap()
                )["error"],
                expected_error
            );
        }
    }
}
