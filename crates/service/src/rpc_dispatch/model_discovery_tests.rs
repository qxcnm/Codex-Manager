use super::try_handle;
use crate::RpcActor;
use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};
use serde_json::{json, Value};

fn request(method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        id: 515.into(),
        method: method.into(),
        params: Some(params),
        trace: None,
    }
}

fn admin_rpc(method: &str, params: Value) -> Value {
    match crate::handle_request_with_actor(request(method, params), RpcActor::system_admin()) {
        JsonRpcMessage::Response(response) => response.result,
        _ => panic!("expected discovery response"),
    }
}

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl Environment {
    fn publish(database: &std::path::Path, codex_home: &std::path::Path) -> Self {
        let mut restore = Self(Vec::new());
        for (key, value) in [
            (
                "CODEXMANAGER_STORAGE_BACKEND",
                Some(std::ffi::OsStr::new("sqlite")),
            ),
            ("CODEXMANAGER_DATABASE_URL", None),
            ("CODEXMANAGER_DB_PATH", Some(database.as_os_str())),
            ("CODEX_HOME", Some(codex_home.as_os_str())),
        ] {
            restore.0.push((key, std::env::var_os(key)));
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        restore
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[test]
fn model_discovery_local_dispatch_denies_members_before_parsing() {
    let member = RpcActor::from_parts(Some("member"), Some("discovery-member"));
    for method in ["modelDiscovery/list", "modelDiscovery/update"] {
        let response = try_handle(&request(method, json!("invalid params")), &member).unwrap();
        assert_eq!(
            response.result["error"],
            format!("permission_denied: {method}")
        );
    }
    assert!(try_handle(&request("unrelated/method", Value::Null), &member).is_none());
}

#[test]
fn model_discovery_rpc_rejects_invalid_updates_before_storage() {
    let valid = json!({"sourceKind": "openai_account", "sourceId": "account-1", "enabled": false});
    let mut invalid = vec![
        Value::Null,
        json!([]),
        json!(["openai_account", "account-1", false, 60]),
        json!({}),
        json!({"sourceKind": "openai_account", "sourceId": "account-1"}),
    ];
    for (key, value) in [
        ("enabled", json!("false")),
        ("enabled", Value::Null),
        ("sourceKind", json!("other")),
        ("sourceId", json!("  ")),
        ("intervalMinutes", json!(-1)),
        ("intervalMinutes", json!(5.5)),
        ("intervalMinutes", json!("60")),
        ("intervalMinutes", json!(4)),
        ("intervalMinutes", json!(10081)),
    ] {
        let mut params = valid.clone();
        params[key] = value;
        invalid.push(params);
    }
    for params in invalid {
        let response = try_handle(
            &request("modelDiscovery/update", params.clone()),
            &RpcActor::system_admin(),
        )
        .unwrap();
        assert!(
            response.result["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty()),
            "accepted invalid payload: {params}"
        );
    }
}

#[test]
fn model_discovery_rpc_persists_configuration_and_denies_password_mode_members() {
    let _guard = crate::test_env_guard();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "model-discovery-rpc-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let database = directory.join("test.db");
    let codex_home = directory.join("codex-home");
    std::fs::create_dir(&codex_home).unwrap();
    let storage = codexmanager_core::storage::Storage::open(&database).unwrap();
    storage.init().unwrap();
    storage
        .set_app_setting("web.auth.mode", "password", 1)
        .unwrap();
    drop(storage);
    let environment = Environment::publish(&database, &codex_home);
    crate::storage_helpers::initialize_storage().unwrap();
    crate::set_web_access_password(Some("model-discovery-test-password")).unwrap();
    assert_eq!(crate::current_web_auth_mode(), "password");

    assert_eq!(
        admin_rpc("modelDiscovery/list", json!({})),
        json!({"items": []})
    );
    let input = json!({"sourceKind": "openai_account", "sourceId": "account-1", "enabled": false, "addr": "web-forwarded-address"});
    let saved = admin_rpc("modelDiscovery/update", input.clone());
    assert_eq!(saved["sourceKind"], "openai_account");
    assert_eq!(saved["sourceId"], "account-1");
    assert_eq!(saved["enabled"], false);
    assert_eq!(saved["intervalMinutes"], 60);
    assert_eq!(saved["running"], false);
    assert_eq!(saved["nextRunAt"], Value::Null);
    assert_eq!(
        admin_rpc(
            "modelDiscovery/list",
            json!({"addr": "web-forwarded-address"})
        ),
        json!({"items": [saved]})
    );

    let member = RpcActor::from_parts(Some("member"), Some("password-mode-user"));
    for method in ["modelDiscovery/list", "modelDiscovery/update"] {
        let response =
            crate::handle_request_with_actor(request(method, input.clone()), member.clone());
        let JsonRpcMessage::Response(response) = response else {
            panic!("expected denial")
        };
        assert_eq!(response.result["errorCode"], "permission_denied");
    }
    assert_eq!(
        admin_rpc("modelDiscovery/list", json!({})),
        json!({"items": [saved]})
    );

    for interval in [5, 10080] {
        let mut input = input.clone();
        input["sourceKind"] = json!("aggregate_api");
        input["sourceId"] = json!("api-1");
        input["intervalMinutes"] = json!(interval);
        let result = admin_rpc("modelDiscovery/update", input);
        assert_eq!(result["intervalMinutes"], interval);
        assert_eq!(result["enabled"], false);
    }
    assert_eq!(
        admin_rpc("modelDiscovery/list", json!({}))["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    drop(environment);
    let _ = std::fs::remove_file(database);
    let _ = std::fs::remove_file(directory.join("test.db-wal"));
    let _ = std::fs::remove_file(directory.join("test.db-shm"));
    let _ = std::fs::remove_file(directory.join("test.db.model-catalog-v2.lock"));
    let _ = std::fs::remove_dir(codex_home);
    let _ = std::fs::remove_dir(directory);
}
