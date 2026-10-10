//! Remote SeaORM authentication acceptance through the production listener.
//! These tests intentionally use isolated databases and fixture credentials.
#[path = "gateway_logs/support.rs"]
mod support;

use codexmanager_core::storage::StorageBackendKind;
use codexmanager_service::{
    bootstrap_app_admin, login_app_user, resolve_app_user_session, start_one_shot_server,
    update_app_user, AppUserUpdateInput,
};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};
use support::*;

fn fixture_name(prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros()
    )
}

fn rpc(method: &str, params: Value, role: &str, user_id: Option<&str>) -> Value {
    let server = start_one_shot_server().expect("start production listener");
    let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    let mut headers = vec![
        ("Content-Type", "application/json"),
        ("X-CodexManager-Rpc-Actor-Role", role),
    ];
    if let Some(user_id) = user_id {
        headers.push(("X-CodexManager-Rpc-Actor-User-Id", user_id));
    }
    headers.push((
        "X-CodexManager-Rpc-Token",
        &codexmanager_service::rpc_auth_token(),
    ));
    let (status, raw) = post_http_raw_with_read_timeout(
        &server.addr,
        "/rpc",
        &body.to_string(),
        &headers,
        std::time::Duration::from_secs(20),
    );
    server.join();
    assert_eq!(status, 200, "rpc transport failed: {raw}");
    serde_json::from_str(&raw).expect("rpc JSON response")
}

fn exercise(backend: &str, url_env: &str) {
    let _lock = test_env_guard();
    let _backend = EnvGuard::set("CODEXMANAGER_STORAGE_BACKEND", backend);
    let url = std::env::var(url_env).expect("isolated database URL");
    let _url = EnvGuard::set("CODEXMANAGER_DATABASE_URL", &url);
    let _source = EnvGuard::set("CODEXMANAGER_DB_PATH", "");
    let admin_name = fixture_name("listener-admin");
    let member_name = fixture_name("listener-member");

    let admin = bootstrap_app_admin(&admin_name, "fixture-admin-password", Some("Fixture admin"))
        .expect("remote admin bootstrap");
    assert_eq!(admin.user.role, "admin");
    let admin_session = resolve_app_user_session(&admin.token)
        .expect("resolve admin session")
        .unwrap();
    assert_eq!(admin_session.user.role, "admin");

    let status = rpc("accountManager/status", Value::Null, "admin", None);
    assert!(status["result"]["mode"].is_string(), "{status}");
    let created = rpc(
        "accountManager/users/create",
        serde_json::json!({"username":member_name,"password":"fixture-member-password","role":"member"}),
        "admin",
        None,
    );
    let member_id = created["result"]["id"]
        .as_str()
        .expect("created member id")
        .to_owned();

    let denied_list = rpc(
        "accountManager/users/list",
        Value::Null,
        "member",
        Some(&member_id),
    );
    assert_eq!(denied_list["result"]["errorCode"], "permission_denied");
    let profile = rpc(
        "accountManager/profile/update",
        serde_json::json!({"displayName":"Fixture member"}),
        "member",
        Some(&member_id),
    );
    assert_eq!(profile["result"]["displayName"], "Fixture member");
    let denied_wallet = rpc(
        "accountManager/wallet/topUp",
        serde_json::json!({"ownerKind":"user","ownerId":member_id,"amountCreditMicros":1}),
        "member",
        Some(&member_id),
    );
    assert_eq!(denied_wallet["result"]["errorCode"], "permission_denied");

    let member_login =
        login_app_user(&member_name, "fixture-member-password").expect("member login");
    assert_eq!(member_login.user.role, "member");
    assert!(resolve_app_user_session(&member_login.token)
        .expect("resolve member session")
        .is_some());
    update_app_user(AppUserUpdateInput {
        id: member_id.clone(),
        status: Some("disabled".into()),
        ..Default::default()
    })
    .expect("disable member");
    assert!(resolve_app_user_session(&member_login.token)
        .expect("resolve disabled member")
        .is_none());
    assert!(login_app_user(&member_name, "fixture-member-password").is_err());

    // Revocation is durable in the remote sessions table and remains effective
    // after a listener restart.
    let admin_again = login_app_user(&admin_name, "fixture-admin-password").expect("admin relogin");
    assert!(resolve_app_user_session(&admin_again.token)
        .expect("resolve relogin")
        .is_some());
    codexmanager_service::logout_app_user_session(&admin_again.token).expect("remote logout");
    assert!(resolve_app_user_session(&admin_again.token)
        .expect("resolve revoked admin")
        .is_none());
    let final_session = rpc(
        "accountManager/session/current",
        Value::Null,
        "member",
        Some(&member_id),
    );
    assert!(
        final_session["result"]["role"].is_string(),
        "{final_session}"
    );
    let _ = StorageBackendKind::Sqlite; // keep the fixture backend enum linked in integration builds
}

#[test]
#[ignore = "requires isolated MySQL URL"]
fn mysql_remote_auth_permission_listener() {
    exercise("mysql", "CODEXMANAGER_TEST_MYSQL_URL");
}

#[test]
#[ignore = "requires isolated PostgreSQL URL"]
fn postgres_remote_auth_permission_listener() {
    exercise("postgres", "CODEXMANAGER_TEST_POSTGRES_URL");
}
