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
