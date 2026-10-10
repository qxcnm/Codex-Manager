//! Exercise injected write handlers against both adapters without global DB configuration.
use super::*;
use crate::http::state::AppState;
use codexmanager_core::storage::{
    ApiKey, ApiKeyOwner, AppUser, DomainStorage, PluginInstall, PluginTask, Storage,
    StorageBackendKind,
};
use codexmanager_storage_seaorm::{
    ApiKeysRepository, PluginsRepository, SeaOrmStorage, SqliteDomainStorage, UsersRepository,
};
use std::sync::Arc;

fn key(id: &str) -> ApiKey {
    ApiKey {
        id: id.into(),
        name: Some(id.into()),
        model_slug: None,
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: "round_robin".into(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: "codex".into(),
        protocol_type: "openai_compat".into(),
        auth_scheme: "authorization_bearer".into(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: format!("fixture-{id}"),
        status: "active".into(),
        created_at: 1,
        last_used_at: None,
    }
}

fn owner(id: &str, user_id: &str) -> ApiKeyOwner {
    ApiKeyOwner {
        key_id: id.into(),
        owner_kind: "user".into(),
        owner_user_id: Some(user_id.into()),
        project_id: None,
        updated_at: 1,
    }
}

fn plugin_install() -> PluginInstall {
    PluginInstall {
        plugin_id: "fixture-plugin".into(),
        source_url: None,
        name: "Fixture Plugin".into(),
        version: "1.0.0".into(),
        description: None,
        author: None,
        homepage_url: None,
        script_url: None,
        script_body: "export default {}".into(),
        permissions_json: "[]".into(),
        manifest_json: "{}".into(),
        status: "disabled".into(),
        installed_at: 1,
        updated_at: 1,
        last_run_at: None,
        last_error: None,
    }
}

fn plugin_task() -> PluginTask {
    PluginTask {
        id: "fixture-plugin::refresh".into(),
        plugin_id: "fixture-plugin".into(),
        name: "Refresh".into(),
        description: None,
        entrypoint: "refresh".into(),
        schedule_kind: "interval".into(),
        interval_seconds: Some(60),
        enabled: true,
        next_run_at: None,
        last_run_at: None,
        last_status: None,
        last_error: None,
        task_json: "{}".into(),
        created_at: 1,
        updated_at: 1,
    }
}

async fn fixture(seaorm: bool) -> Arc<dyn DomainStorage> {
    let rows = [key("owned"), key("foreign"), key("unowned")];
    let owners = [owner("owned", "alice"), owner("foreign", "bob")];
    let plugin = plugin_install();
    let task = plugin_task();
    let users = ["alice", "bob"].map(|id| AppUser {
        id: id.into(),
        username: format!("fixture-{id}"),
        display_name: None,
        password_hash: "fixture-only".into(),
        role: "member".into(),
        status: "active".into(),
        created_at: 1,
        updated_at: 1,
        last_login_at: None,
    });
    if seaorm {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        for user in users {
            UsersRepository::put(storage.connection(), user)
                .await
                .unwrap();
        }
        for row in rows {
            ApiKeysRepository::upsert(storage.connection(), row.into())
                .await
                .unwrap();
        }
        for row in owners {
            UsersRepository::put_owner(storage.connection(), row)
                .await
                .unwrap();
        }
        PluginsRepository::replace_plugin_install(storage.connection(), &plugin, &[task])
            .await
            .unwrap();
        Arc::new(storage)
    } else {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        for user in users {
            storage.insert_app_user(&user).unwrap();
        }
        for row in rows {
            storage.insert_api_key(&row).unwrap();
        }
        for row in owners {
            storage.upsert_api_key_owner(&row).unwrap();
        }
        storage.replace_plugin_install(&plugin, &[task]).unwrap();
        Arc::new(SqliteDomainStorage::new(storage))
    }
}

async fn call(
    state: Arc<AppState>,
    method: &str,
    params: Option<Value>,
    actor: &RpcActor,
) -> Value {
    let req = JsonRpcRequest {
        id: 42.into(),
        method: method.into(),
        params,
        trace: None,
    };
    let Some(JsonRpcMessage::Response(response)) = storage_async::handle(state, &req, actor).await
    else {
        panic!("expected native storage response");
    };
    assert_eq!(response.id, req.id);
    response.result
}

async fn plugin_status_contract(seaorm: bool) {
    let store = fixture(seaorm).await;
    let state = AppState::with_storage(store.clone());
    let admin = RpcActor::system_admin();
    let member = RpcActor::from_parts(Some("member"), Some("alice"));

    let denied = call(
        state.clone(),
        "plugin/disable",
        Some(serde_json::json!({"pluginId": "fixture-plugin"})),
        &member,
    )
    .await;
    assert_eq!(denied["errorCode"], "permission_denied");

    let missing = call(state.clone(), "plugin/disable", None, &admin).await;
    assert_eq!(missing["error"], "missing pluginId");

    let disabled = call(
        state.clone(),
        "plugin/disable",
        Some(serde_json::json!({"plugin_id": "fixture-plugin"})),
        &admin,
    )
    .await;
    assert_eq!(disabled, serde_json::json!({"ok": true}));
    let plugin = store
        .plugins()
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.plugin_id == "fixture-plugin")
        .unwrap();
    assert_eq!(plugin.status, "disabled");
    assert!(store
        .plugin_tasks(Some("fixture-plugin".into()))
        .await
        .unwrap()
        .into_iter()
        .all(|task| task.next_run_at.is_none()));

    let enabled = call(
        state,
        "plugin/enable",
        Some(serde_json::json!({"pluginId": "fixture-plugin"})),
        &admin,
    )
    .await;
    assert_eq!(enabled, serde_json::json!({"ok": true}));
    let plugin = store
        .plugins()
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.plugin_id == "fixture-plugin")
        .unwrap();
    assert_eq!(plugin.status, "enabled");
    let task = store
        .plugin_tasks(Some("fixture-plugin".into()))
        .await
        .unwrap()
        .into_iter()
        .find(|task| task.id == "fixture-plugin::refresh")
        .unwrap();
    assert!(task.next_run_at.is_some());
}

async fn mutation_contract(seaorm: bool) {
    let store = fixture(seaorm).await;
    let state = AppState::with_storage(store.clone());
    let member = RpcActor::from_parts(Some("member"), Some("alice"));
    let outsider = RpcActor::from_parts(Some("member"), Some("bob"));
    let no_session = RpcActor::from_parts(Some("member"), None);
    let admin = RpcActor::system_admin();

    for method in ["apikey/disable", "apikey/enable", "apikey/delete"] {
        let before = format!("{:?}", store.api_keys().await.unwrap());
        for (actor, id) in [
            (&outsider, "owned"),
            (&member, "foreign"),
            (&member, "unowned"),
            (&member, "missing"),
            (&no_session, "owned"),
        ] {
            let result = call(
                state.clone(),
                method,
                Some(serde_json::json!({"id": id})),
                actor,
            )
            .await;
            assert_eq!(
                result["errorCode"], "permission_denied",
                "{method}: {result}"
            );
        }
        for params in [
            None,
            Some(serde_json::json!({})),
            Some(serde_json::json!({"id": ""})),
            Some(serde_json::json!({"id": 12})),
        ] {
            let result = call(state.clone(), method, params, &admin).await;
            assert_eq!(result["error"], "missing id", "{method}: {result}");
        }
        assert_eq!(
            format!("{:?}", store.api_keys().await.unwrap()),
            before,
            "rejected {method} must not mutate any key"
        );
    }

    for actor in [&member, &admin] {
        for (method, expected) in [("apikey/disable", "disabled"), ("apikey/enable", "active")] {
            let result = call(
                state.clone(),
                method,
                Some(serde_json::json!({"id": "owned"})),
                actor,
            )
            .await;
            assert_eq!(result, serde_json::json!({"ok": true}));
            let rows = store.api_keys().await.unwrap();
            assert_eq!(
                rows.iter().find(|key| key.id == "owned").unwrap().status,
                expected
            );
            assert!(rows
                .iter()
                .filter(|key| key.id != "owned")
                .all(|key| key.status == "active"));
            assert_eq!(
                store
                    .api_key_owner("owned".into())
                    .await
                    .unwrap()
                    .unwrap()
                    .owner_user_id
                    .as_deref(),
                Some("alice")
            );
        }
    }
    assert_eq!(
        call(
            state.clone(),
            "apikey/delete",
            Some(serde_json::json!({"id": "owned"})),
            &member
        )
        .await,
        serde_json::json!({"ok": true})
    );
    assert!(!store
        .api_keys()
        .await
        .unwrap()
        .iter()
        .any(|key| key.id == "owned"));
    assert!(store.api_key_owner("owned".into()).await.unwrap().is_none());

    let owner_result = call(
        state.clone(),
        "accountManager/apiKeyOwners/set",
        Some(serde_json::json!({
            "keyId": "unowned",
            "ownerKind": "user",
            "ownerUserId": "alice"
        })),
        &admin,
    )
    .await;
    assert_eq!(owner_result["keyId"], "unowned");
    assert_eq!(owner_result["ownerKind"], "user");
    assert_eq!(owner_result["ownerUserId"], "alice");
    assert!(store
        .wallet("user".into(), "alice".into())
        .await
        .unwrap()
        .is_some());
    assert_eq!(
        store
            .api_key_owner("unowned".into())
            .await
            .unwrap()
            .unwrap()
            .owner_user_id
            .as_deref(),
        Some("alice")
    );

    // Keep existing backend-specific unknown-ID behavior explicit during migration:
    // SQLite's status UPDATE is a no-op; SeaORM's locked profile update reports not found.
    let missing = call(
        state,
        "apikey/enable",
        Some(serde_json::json!({"id": "missing"})),
        &admin,
    )
    .await;
    if seaorm {
        assert!(missing["error"]
            .as_str()
            .unwrap()
            .contains("api key not found"));
    } else {
        assert_eq!(missing, serde_json::json!({"ok": true}));
    }
}

fn password_user() -> AppUser {
    AppUser {
        id: "profile-user".into(),
        username: "profile-user".into(),
        display_name: Some("before".into()),
        password_hash: crate::hash_password("old-password"),
        role: "member".into(),
        status: "active".into(),
        created_at: 1,
        updated_at: 1,
        last_login_at: None,
    }
}

async fn profile_and_password_contract(seaorm: bool) {
    let user = password_user();
    let store: Arc<dyn DomainStorage> = if seaorm {
        let storage = SeaOrmStorage::connect(StorageBackendKind::Sqlite, "sqlite::memory:")
            .await
            .unwrap();
        storage.migrate().await.unwrap();
        UsersRepository::put(storage.connection(), user.clone())
            .await
            .unwrap();
        Arc::new(storage)
    } else {
        let storage = Storage::open_in_memory().unwrap();
        storage.init().unwrap();
        storage.insert_app_user(&user).unwrap();
        Arc::new(SqliteDomainStorage::new(storage))
    };
    let verifier = store.clone();
    let state = AppState::with_storage(store);
    let actor = RpcActor::from_parts(Some("member"), Some("profile-user"));

    let profile = call(
        state.clone(),
        "accountManager/profile/update",
        Some(serde_json::json!({"displayName": "  after  "})),
        &actor,
    )
    .await;
    assert_eq!(profile["id"], "profile-user");
    assert_eq!(profile["displayName"], "after");

    let wrong_password = call(
        state.clone(),
        "accountManager/password/change",
        Some(serde_json::json!({
            "currentPassword": "wrong-password",
            "newPassword": "new-password"
        })),
        &actor,
    )
    .await;
    assert_eq!(wrong_password["error"], "当前密码不正确");

    let changed = call(
        state,
        "accountManager/password/change",
        Some(serde_json::json!({
            "currentPassword": "old-password",
            "newPassword": "new-password"
        })),
        &actor,
    )
    .await;
    assert_eq!(changed, serde_json::json!({"ok": true}));
    let updated = verifier.user("profile-user".into()).await.unwrap().unwrap();
    assert_eq!(updated.display_name.as_deref(), Some("after"));
    assert!(crate::verify_password_hash(
        "new-password",
        &updated.password_hash
    ));
    assert!(!crate::verify_password_hash(
        "old-password",
        &updated.password_hash
    ));
}

async fn user_update_delete_contract(seaorm: bool) {
    let store = fixture(seaorm).await;
    let verifier = store.clone();
    let state = AppState::with_storage(store);
    let admin = RpcActor::system_admin();

    let created = call(
        state.clone(),
        "accountManager/users/create",
        Some(serde_json::json!({
            "username": "created-user",
            "password": "created-password",
            "displayName": " Created User ",
            "initialBalanceCreditMicros": 123
        })),
        &admin,
    )
    .await;
    assert_eq!(created["username"], "created-user");
    assert_eq!(created["displayName"], "Created User");
    let created_id = created["id"].as_str().unwrap().to_owned();
    let created_user = verifier.user(created_id.clone()).await.unwrap().unwrap();
    assert_eq!(created_user.role, "member");
    let created_wallet = verifier
        .wallet("user".into(), created_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created_wallet.balance_credit_micros, 123);

    let updated = call(
        state.clone(),
        "accountManager/users/update",
        Some(serde_json::json!({
            "id": "alice",
            "displayName": "  updated member  ",
            "status": "disabled",
            "password": "updated-password"
        })),
        &admin,
    )
    .await;
    assert_eq!(updated["id"], "alice");
    assert_eq!(updated["displayName"], "updated member");
    assert_eq!(updated["status"], "disabled");

    let alice = verifier.user("alice".into()).await.unwrap().unwrap();
    assert_eq!(alice.display_name.as_deref(), Some("updated member"));
    assert_eq!(alice.status, "disabled");
    assert!(crate::verify_password_hash(
        "updated-password",
        &alice.password_hash
    ));
    assert!(verifier
        .wallet("user".into(), "alice".into())
        .await
        .unwrap()
        .is_some());

    let deleted = call(
        state,
        "accountManager/users/delete",
        Some(serde_json::json!({"id": "bob"})),
        &admin,
    )
    .await;
    assert_eq!(deleted, serde_json::json!({"ok": true}));
    assert!(verifier.user("bob".into()).await.unwrap().is_none());
}

async fn wallet_mutation_contract(seaorm: bool) {
    let store = fixture(seaorm).await;
    let verifier = store.clone();
    let state = AppState::with_storage(store);
    let admin = RpcActor::system_admin();

    let top_up = call(
        state.clone(),
        "accountManager/wallet/topUp",
        Some(serde_json::json!({
            "ownerKind": "user",
            "ownerId": "alice",
            "amountCreditMicros": 100,
            "note": " initial top up ",
            "createdByUserId": "alice"
        })),
        &admin,
    )
    .await;
    assert_eq!(top_up["ownerId"], "alice");
    assert_eq!(top_up["balanceCreditMicros"], 100);
    assert_eq!(top_up["availableCreditMicros"], 100);

    let set_available = call(
        state.clone(),
        "accountManager/wallet/setAvailable",
        Some(serde_json::json!({
            "ownerKind": "user",
            "ownerId": "alice",
            "availableCreditMicros": 250
        })),
        &admin,
    )
    .await;
    assert_eq!(set_available["balanceCreditMicros"], 250);
    assert_eq!(set_available["availableCreditMicros"], 250);
    assert_eq!(
        verifier
            .wallet("user".into(), "alice".into())
            .await
            .unwrap()
            .unwrap()
            .balance_credit_micros,
        250
    );

    let invalid_top_up = call(
        state.clone(),
        "accountManager/wallet/topUp",
        Some(serde_json::json!({
            "ownerKind": "user",
            "ownerId": "alice",
            "amountCreditMicros": 0
        })),
        &admin,
    )
    .await;
    assert_eq!(invalid_top_up["error"], "充值金额必须大于 0");

    let invalid_admin = call(
        state,
        "accountManager/wallet/topUp",
        Some(serde_json::json!({
            "ownerKind": "user",
            "ownerId": "missing-admin",
            "amountCreditMicros": 1
        })),
        &admin,
    )
    .await;
    assert_eq!(invalid_admin["error"], "用户不存在");
}

async fn api_key_create_update_contract(seaorm: bool) {
    let store = fixture(seaorm).await;
    let verifier = store.clone();
    let state = AppState::with_storage(store);
    let member = RpcActor::from_parts(Some("member"), Some("alice"));
    let outsider = RpcActor::from_parts(Some("member"), Some("bob"));
    let admin = RpcActor::system_admin();

    let created = call(
        state.clone(),
        "apikey/create",
        Some(serde_json::json!({
            "name": "owned created",
            "modelSlug": "external-model",
            "protocolType": "anthropic",
            "rotationStrategy": "account",
            "customKey": "custom-secret",
            "quotaLimitTokens": 99
        })),
        &member,
    )
    .await;
    assert_eq!(created["key"], "custom-secret");
    let id = created["id"].as_str().unwrap().to_owned();
    assert_eq!(
        verifier
            .api_key_secret(id.clone())
            .await
            .unwrap()
            .as_deref(),
        Some("custom-secret")
    );
    assert_eq!(
        verifier
            .api_key_owner(id.clone())
            .await
            .unwrap()
            .unwrap()
            .owner_user_id
            .as_deref(),
        Some("alice")
    );
    assert_eq!(
        verifier
            .api_key(id.clone())
            .await
            .unwrap()
            .unwrap()
            .protocol_type,
        "anthropic_native"
    );

    let duplicate = call(
        state.clone(),
        "apikey/create",
        Some(serde_json::json!({"customKey": "custom-secret"})),
        &admin,
    )
    .await;
    assert!(duplicate["error"]
        .as_str()
        .unwrap_or_default()
        .contains("already exists"));
    assert_eq!(verifier.api_keys().await.unwrap().len(), 4);

    let denied = call(
        state.clone(),
        "apikey/updateModel",
        Some(serde_json::json!({"id": id, "modelSlug": "external-model"})),
        &outsider,
    )
    .await;
    assert_eq!(denied["errorCode"], "permission_denied");

    let updated = call(
        state.clone(),
        "apikey/updateModel",
        Some(serde_json::json!({
            "id": created["id"], "name": "renamed", "modelSlug": "external-model",
            "protocolType": "openai", "quotaLimitTokens": null
        })),
        &member,
    )
    .await;
    assert_eq!(updated, serde_json::json!({"ok": true}));
    let key = verifier
        .api_key(created["id"].as_str().unwrap().to_owned())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(key.name.as_deref(), Some("renamed"));
    assert_eq!(key.protocol_type, "openai_compat");
    assert!(key.upstream_base_url.is_none());
    let admin_updated = call(
        state.clone(),
        "apikey/updateModel",
        Some(serde_json::json!({
            "id": created["id"], "upstreamBaseUrl": "https://example.invalid/v1",
            "staticHeadersJson": "{\"X-Test\":\"ok\"}"
        })),
        &admin,
    )
    .await;
    assert_eq!(admin_updated, serde_json::json!({"ok": true}));
    assert_eq!(
        verifier
            .api_key(created["id"].as_str().unwrap().to_owned())
            .await
            .unwrap()
            .unwrap()
            .upstream_base_url
            .as_deref(),
        Some("https://example.invalid/v1")
    );
    assert_eq!(
        verifier
            .api_key_secret(created["id"].as_str().unwrap().to_owned())
            .await
            .unwrap()
            .as_deref(),
        Some("custom-secret")
    );

    // The owner check runs after the key, secret, quota and profile writes in
    // both adapters. A missing member must therefore roll back the complete
    // transaction instead of leaving an unusable key behind.
    let before_failed_create = verifier.api_keys().await.unwrap().len();
    let missing_member = RpcActor::from_parts(Some("member"), Some("missing-member"));
    let failed_create = call(
        state,
        "apikey/create",
        Some(serde_json::json!({
            "customKey": "rollback-owner-secret",
            "quotaLimitTokens": 123,
            "modelSlug": "external-model"
        })),
        &missing_member,
    )
    .await;
    assert!(failed_create["error"]
        .as_str()
        .is_some_and(|error| error.contains("用户不存在")));
    assert_eq!(
        verifier.api_keys().await.unwrap().len(),
        before_failed_create
    );
    assert!(verifier
        .api_key_by_hash(crate::storage_helpers::hash_platform_key(
            "rollback-owner-secret"
        ))
        .await
        .unwrap()
        .is_none());
    assert!(verifier
        .wallet("user".into(), "missing-member".into())
        .await
        .unwrap()
        .is_none());
}

async fn api_key_failure_and_routing_contract(seaorm: bool) {
    let store = fixture(seaorm).await;
    let verifier = store.clone();
    let state = AppState::with_storage(store);
    let admin = RpcActor::system_admin();
    let created = call(
        state.clone(),
        "apikey/create",
        Some(serde_json::json!({"customKey": "routing-secret", "modelSlug": "external-model"})),
        &admin,
    )
    .await;
    let id = created["id"].as_str().unwrap().to_owned();
    let route = call(state.clone(), "apikey/updateModel", Some(serde_json::json!({
        "id": id, "rotationStrategy": "aggregate_api", "aggregateApiId": "agg-1", "accountPlanFilter": "plus", "accountGroupFilter": "team-a"
    })), &admin).await;
    assert_eq!(route, serde_json::json!({"ok": true}));
    let routed = verifier.api_key(id.clone()).await.unwrap().unwrap();
    assert_eq!(routed.rotation_strategy, "aggregate_api_rotation");
    assert_eq!(routed.aggregate_api_id.as_deref(), Some("agg-1"));
    assert!(verifier.api_key_owner(id.clone()).await.unwrap().is_none());

    let image = call(
        state.clone(),
        "apikey/create",
        Some(serde_json::json!({"customKey": "image-only-secret", "modelSlug": "gpt-image-2"})),
        &admin,
    )
    .await;
    assert!(image["error"]
        .as_str()
        .unwrap_or_default()
        .contains("image-only"));
    assert!(verifier
        .api_key_by_hash(crate::storage_helpers::hash_platform_key(
            "image-only-secret"
        ))
        .await
        .unwrap()
        .is_none());

    let invalid = call(state, "apikey/updateModel", Some(serde_json::json!({"id": id, "name": "must-not-stick", "protocolType": "not-a-protocol"})), &admin).await;
    assert!(invalid["error"]
        .as_str()
        .unwrap_or_default()
        .contains("unsupported protocol"));
    assert_eq!(verifier.api_key(id).await.unwrap().unwrap().name, None);
}

#[tokio::test]
async fn sqlite_injected_api_key_create_update_readback() {
    api_key_create_update_contract(false).await;
}

#[tokio::test]
async fn sqlite_injected_plugin_status_and_schedule_readback() {
    plugin_status_contract(false).await;
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn seaorm_sqlite_injected_plugin_status_and_schedule_readback() {
    plugin_status_contract(true).await;
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn seaorm_sqlite_injected_api_key_create_update_readback() {
    api_key_create_update_contract(true).await;
}

#[tokio::test]
async fn sqlite_injected_api_key_failure_and_routing_readback() {
    api_key_failure_and_routing_contract(false).await;
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn seaorm_sqlite_injected_api_key_failure_and_routing_readback() {
    api_key_failure_and_routing_contract(true).await;
}

#[tokio::test]
async fn sqlite_injected_api_key_mutation_permissions_and_readback() {
    mutation_contract(false).await;
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn seaorm_sqlite_injected_api_key_mutation_permissions_and_readback() {
    mutation_contract(true).await;
}

#[tokio::test]
async fn sqlite_injected_account_profile_and_password_write_readback() {
    profile_and_password_contract(false).await;
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn seaorm_sqlite_injected_account_profile_and_password_write_readback() {
    profile_and_password_contract(true).await;
}

#[tokio::test]
async fn sqlite_injected_account_user_update_delete_readback() {
    user_update_delete_contract(false).await;
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn seaorm_sqlite_injected_account_user_update_delete_readback() {
    user_update_delete_contract(true).await;
}

#[tokio::test]
async fn sqlite_injected_wallet_mutation_readback() {
    wallet_mutation_contract(false).await;
}

#[cfg(feature = "storage-sqlite")]
#[tokio::test]
async fn seaorm_sqlite_injected_wallet_mutation_readback() {
    wallet_mutation_contract(true).await;
}
