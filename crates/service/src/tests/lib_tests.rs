use super::*;
use codexmanager_core::rpc::types::{
    JsonRpcMessage, JsonRpcResponse, ModelGroupModelUpsertParams, ModelGroupModelsSetParams,
    ModelGroupUsersSetParams,
};
use codexmanager_core::storage::{
    Account, Event, ModelCatalogModelRecord, ModelGroupModel, PluginInstall, PluginRunLog,
    PluginTask, RequestLog, RequestTokenStat, Token, UsageSnapshotRecord,
};
use rusqlite::Connection;

/// 函数 `response_result`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - resp: 参数 resp
///
/// # 返回
/// 返回函数执行结果
fn response_result(resp: JsonRpcMessage) -> JsonRpcResponse {
    match resp {
        JsonRpcMessage::Response(resp) => resp,
        JsonRpcMessage::Error(err) => panic!("unexpected rpc error: {}", err.error.message),
        JsonRpcMessage::Notification(_) => panic!("unexpected rpc notification"),
        JsonRpcMessage::Request(_) => panic!("unexpected rpc request"),
    }
}

/// 函数 `login_complete_requires_params`
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
fn login_complete_requires_params() {
    let req = JsonRpcRequest {
        id: 1.into(),
        method: "account/login/complete".to_string(),
        params: None,
        trace: None,
    };
    let resp = response_result(handle_request(req));
    let err = resp
        .result
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("missing"));

    let req = JsonRpcRequest {
        id: 2.into(),
        method: "account/login/complete".to_string(),
        params: Some(serde_json::json!({ "code": "x" })),
        trace: None,
    };
    let resp = response_result(handle_request(req));
    let err = resp
        .result
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("missing"));

    let req = JsonRpcRequest {
        id: 3.into(),
        method: "account/login/complete".to_string(),
        params: Some(serde_json::json!({ "state": "y" })),
        trace: None,
    };
    let resp = response_result(handle_request(req));
    let err = resp
        .result
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(err.contains("missing"));
}

/// 函数 `unknown_method_returns_jsonrpc_error`
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
fn unknown_method_returns_jsonrpc_error() {
    let req = JsonRpcRequest {
        id: 9.into(),
        method: "not/a/method".to_string(),
        params: None,
        trace: None,
    };

    match handle_request(req) {
        JsonRpcMessage::Error(err) => {
            assert_eq!(err.id, 9.into());
            assert_eq!(err.error.code, -32601);
            assert_eq!(err.error.message, "unknown_method");
        }
        other => panic!("expected rpc error, got {other:?}"),
    }
}

#[test]
fn member_actor_cannot_call_admin_only_rpc() {
    for method in [
        "accountManager/users/list",
        "codexProfile/repairHistory",
        "codexProfile/pruneHistoryBackups",
        "system/proxy/list",
    ] {
        let req = JsonRpcRequest {
            id: 21.into(),
            method: method.to_string(),
            params: None,
            trace: None,
        };

        let resp = response_result(handle_request_with_actor(
            req,
            RpcActor::from_parts(Some(ROLE_MEMBER), Some("user-1")),
        ));
        let err = resp
            .result
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        assert!(err.contains("permission_denied"), "{method}: {err}");
    }
}

#[test]
fn system_proxy_rpc_crud_roundtrip() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-system-proxy-crud");

    let created = response_result(handle_request_with_actor(
        rpc_request(
            "system/proxy/create",
            serde_json::json!({
                "name": "DE-Frankfurt-1",
                "proxyUrl": "http://user:pass@example.com:8080/path",
                "enabled": true,
                "tagsJson": "[\"de\",\"work\"]",
                "notes": "Primary proxy"
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        created.result.get("error").is_none(),
        "{:?}",
        created.result
    );
    let proxy_id = created.result["id"].as_str().expect("proxy id").to_string();
    assert_eq!(created.result["status"], "unchecked");
    assert_eq!(
        created.result["proxyUrlRedacted"],
        "http://example.com:8080"
    );
    assert_eq!(created.result["scheme"], "http");
    assert_eq!(created.result["host"], "example.com");
    assert!(created.result.get("proxyUrl").is_none());

    let listed = response_result(handle_request_with_actor(
        rpc_request("system/proxy/list", serde_json::json!({})),
        RpcActor::system_admin(),
    ));
    assert!(listed.result.get("error").is_none(), "{:?}", listed.result);
    let items = listed.result["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], proxy_id);
    assert_eq!(items[0]["notes"], "Primary proxy");

    let updated = response_result(handle_request_with_actor(
        rpc_request(
            "system/proxy/update",
            serde_json::json!({
                "id": proxy_id,
                "name": "DE-Frankfurt-2",
                "proxyUrl": "socks5h://user:pass@proxy.example:1080/private",
                "enabled": false,
                "tagsJson": "[\"updated\"]",
                "notes": "Updated proxy"
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        updated.result.get("error").is_none(),
        "{:?}",
        updated.result
    );
    assert_eq!(updated.result["name"], "DE-Frankfurt-2");
    assert_eq!(
        updated.result["proxyUrlRedacted"],
        "socks5h://proxy.example:1080"
    );
    assert_eq!(updated.result["scheme"], "socks5h");
    assert_eq!(updated.result["status"], "unchecked");
    assert_eq!(updated.result["enabled"], false);

    let deleted = response_result(handle_request_with_actor(
        rpc_request(
            "system/proxy/delete",
            serde_json::json!({
                "id": updated.result["id"].as_str().expect("updated proxy id")
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        deleted.result.get("error").is_none(),
        "{:?}",
        deleted.result
    );
    assert_eq!(deleted.result["ok"], true);

    let empty = response_result(handle_request_with_actor(
        rpc_request("system/proxy/list", serde_json::json!({})),
        RpcActor::system_admin(),
    ));
    assert_eq!(
        empty.result["items"].as_array().expect("items array").len(),
        0
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn system_proxy_rpc_rejects_invalid_schemes() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-system-proxy-schemes");

    for proxy_url in [
        "vless://example.com:443",
        "ss://example.com:8388",
        "hysteria2://example.com:443",
        "ftp://example.com:21",
    ] {
        let resp = response_result(handle_request_with_actor(
            rpc_request(
                "system/proxy/create",
                serde_json::json!({
                    "name": "Invalid",
                    "proxyUrl": proxy_url
                }),
            ),
            RpcActor::system_admin(),
        ));
        let err = rpc_error(&resp);
        assert!(
            err.contains("For sing-box, paste the local mixed inbound address"),
            "{proxy_url}: {err}"
        );
    }

    let update = response_result(handle_request_with_actor(
        rpc_request(
            "system/proxy/update",
            serde_json::json!({
                "id": "pp_test",
                "proxyUrl": "trojan://example.com:443"
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        rpc_error(&update).contains("For sing-box, paste the local mixed inbound address"),
        "{:?}",
        update.result
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn system_proxy_delete_rejects_bound_accounts() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-system-proxy-bound-delete");

    let created = response_result(handle_request_with_actor(
        rpc_request(
            "system/proxy/create",
            serde_json::json!({
                "name": "Bound Proxy",
                "proxyUrl": "http://127.0.0.1:7891"
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        created.result.get("error").is_none(),
        "{:?}",
        created.result
    );
    let proxy_id = created.result["id"].as_str().expect("proxy id").to_string();

    {
        let storage = storage_helpers::open_storage().expect("open storage");
        let now = codexmanager_core::storage::now_ts();
        storage
            .insert_account(&Account {
                id: "acc-bound".to_string(),
                label: "Bound account".to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: None,
                workspace_id: None,
                group_name: None,
                sort: 0,
                status: "active".to_string(),
                created_at: now,
                updated_at: now,
            })
            .expect("insert account");
        storage
            .upsert_account_proxy_settings(
                "acc-bound",
                true,
                Some("custom"),
                None,
                Some("http://127.0.0.1:7891"),
                "unchecked",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("seed account proxy settings");
    }

    let conn = Connection::open(&db_path).expect("open raw sqlite connection");
    conn.execute(
        "UPDATE account_proxy_settings
         SET proxy_source = 'profile', proxy_profile_id = ?1
         WHERE account_id = 'acc-bound'",
        [&proxy_id],
    )
    .expect("bind account to proxy profile");
    drop(conn);

    let deleted = response_result(handle_request_with_actor(
        rpc_request("system/proxy/delete", serde_json::json!({ "id": proxy_id })),
        RpcActor::system_admin(),
    ));
    let err = rpc_error(&deleted);
    assert!(
        err.contains("proxy profile is still bound to accounts"),
        "{err}"
    );
    assert!(err.contains("acc-bound"), "{err}");

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn password_mode_can_call_admin_and_model_source_rpcs() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-password-model-source-rpc");
    set_web_access_password(Some("password123")).expect("set web password");
    set_web_auth_mode("password").expect("enable password mode");
    let actor = RpcActor::from_parts(Some(ROLE_MEMBER), Some("password-mode-user"));

    let admin_resp = response_result(handle_request_with_actor(
        JsonRpcRequest {
            id: 22.into(),
            method: "accountManager/users/list".to_string(),
            params: None,
            trace: None,
        },
        actor.clone(),
    ));
    let admin_err = rpc_error(&admin_resp);
    assert!(
        !admin_err.contains("permission_denied"),
        "password mode unexpectedly denied accountManager/users/list: {admin_err}"
    );

    for (method, params) in [
        (
            "apikey/modelSourceSync",
            serde_json::json!({ "sourceKind": "aggregate_api" }),
        ),
        (
            "apikey/modelSourceModelSave",
            serde_json::json!({
                "sourceKind": "aggregate_api",
                "sourceId": "ag_test",
                "upstreamModel": "gpt-4o"
            }),
        ),
        (
            "apikey/modelSourceMappingSave",
            serde_json::json!({
                "platformModelSlug": "gpt-4o",
                "sourceKind": "aggregate_api",
                "sourceId": "ag_test",
                "upstreamModel": "gpt-4o"
            }),
        ),
        (
            "apikey/modelSourceMappingDelete",
            serde_json::json!({
                "id": "map_test",
                "sourceKind": "openai_account",
                "sourceId": "acc_test",
                "upstreamModel": "gpt-test",
            }),
        ),
    ] {
        let resp = response_result(handle_request_with_actor(
            rpc_request(method, params),
            actor.clone(),
        ));
        let err = rpc_error(&resp);
        assert!(
            !err.contains("permission_denied"),
            "{method} unexpectedly denied: {err}"
        );
    }

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn password_mode_member_cannot_prune_stale_remote_catalog() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-prune-stale-remote-denied");
    set_web_access_password(Some("password123")).expect("set web password");
    set_web_auth_mode("password").expect("enable password mode");

    let resp = response_result(handle_request_with_actor(
        rpc_request("apikey/modelCatalogPruneStaleRemote", serde_json::json!({})),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some("member-user")),
    ));

    assert!(
        rpc_error(&resp).contains("permission_denied"),
        "{:?}",
        resp.result
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn admin_user_update_edits_member_and_protects_last_active_admin() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-user-update");
    let admin = create_app_user(AppUserCreateInput {
        username: "admin-update-one".to_string(),
        password: "password-one".to_string(),
        display_name: None,
        role: Some(ROLE_ADMIN.to_string()),
        initial_balance_credit_micros: None,
    })
    .expect("create admin");
    let member = create_test_member("member-update-one", Some(1_000_000));

    let updated = update_app_user(AppUserUpdateInput {
        id: member.id.clone(),
        display_name: Some("Updated Member".to_string()),
        role: Some(ROLE_MEMBER.to_string()),
        status: Some("disabled".to_string()),
        password: Some("new-password".to_string()),
    })
    .expect("update member");
    assert_eq!(updated.display_name.as_deref(), Some("Updated Member"));
    assert_eq!(updated.status, "disabled");
    assert_eq!(updated.role, ROLE_MEMBER);
    assert!(updated.wallet.is_some());

    let last_admin_error = update_app_user(AppUserUpdateInput {
        id: admin.id.clone(),
        display_name: None,
        role: Some(ROLE_ADMIN.to_string()),
        status: Some("disabled".to_string()),
        password: None,
    })
    .expect_err("last active admin should be protected");
    assert!(last_admin_error.contains("至少需要保留一个启用的管理员"));

    let _second_admin = create_app_user(AppUserCreateInput {
        username: "admin-update-two".to_string(),
        password: "password-two".to_string(),
        display_name: None,
        role: Some(ROLE_ADMIN.to_string()),
        initial_balance_credit_micros: None,
    })
    .expect("create second admin");
    let disabled_admin = update_app_user(AppUserUpdateInput {
        id: admin.id,
        display_name: Some("Disabled Admin".to_string()),
        role: Some(ROLE_ADMIN.to_string()),
        status: Some("disabled".to_string()),
        password: None,
    })
    .expect("disable admin when another admin exists");
    assert_eq!(disabled_admin.status, "disabled");
    assert!(disabled_admin.wallet.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn account_manager_user_list_batches_wallets() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-user-list-wallets");
    let admin = create_app_user(AppUserCreateInput {
        username: "admin-list-wallets".to_string(),
        password: "password-one".to_string(),
        display_name: None,
        role: Some(ROLE_ADMIN.to_string()),
        initial_balance_credit_micros: None,
    })
    .expect("create admin");
    let member = create_test_member("member-list-wallets", Some(1_000_000));

    let resp = response_result(handle_request_with_actor(
        rpc_request("accountManager/users/list", serde_json::json!({})),
        RpcActor::from_parts(Some(ROLE_ADMIN), Some(&admin.id)),
    ));
    let users = resp.result.as_array().expect("users");
    let listed_admin = users
        .iter()
        .find(|user| user.get("id").and_then(|value| value.as_str()) == Some(admin.id.as_str()))
        .expect("listed admin");
    let listed_member = users
        .iter()
        .find(|user| user.get("id").and_then(|value| value.as_str()) == Some(member.id.as_str()))
        .expect("listed member");

    assert!(listed_admin
        .get("wallet")
        .is_none_or(|value| value.is_null()));
    assert_eq!(
        listed_member
            .get("wallet")
            .and_then(|wallet| wallet.get("availableCreditMicros"))
            .and_then(|value| value.as_i64()),
        Some(1_000_000)
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn plugin_logs_rpc_resolves_names_from_current_log_page() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-plugin-log-names");
    let storage = storage_helpers::open_storage().expect("open storage");
    let install = PluginInstall {
        plugin_id: "cleanup-banned-accounts".to_string(),
        source_url: Some("builtin://codexmanager".to_string()),
        name: "清理封禁账号".to_string(),
        version: "1.0.0".to_string(),
        description: Some("test".to_string()),
        author: Some("CodexManager".to_string()),
        homepage_url: None,
        script_url: None,
        script_body: "fn run(context) { context }".to_string(),
        permissions_json: serde_json::json!(["accounts:cleanup"]).to_string(),
        manifest_json: serde_json::json!({ "id": "cleanup-banned-accounts" }).to_string(),
        status: "enabled".to_string(),
        installed_at: 1,
        updated_at: 1,
        last_run_at: None,
        last_error: None,
    };
    let task = PluginTask {
        id: "cleanup-banned-accounts::run".to_string(),
        plugin_id: install.plugin_id.clone(),
        name: "手动清理".to_string(),
        description: None,
        entrypoint: "run".to_string(),
        schedule_kind: "manual".to_string(),
        interval_seconds: None,
        enabled: true,
        next_run_at: None,
        last_run_at: None,
        last_status: None,
        last_error: None,
        task_json: serde_json::json!({
            "id": "run",
            "name": "手动清理",
            "entrypoint": "run",
            "scheduleKind": "manual",
            "enabled": true
        })
        .to_string(),
        created_at: 1,
        updated_at: 1,
    };
    storage
        .replace_plugin_install(&install, &[task])
        .expect("seed plugin");
    storage
        .insert_plugin_run_log(&PluginRunLog {
            id: None,
            plugin_id: "cleanup-banned-accounts".to_string(),
            task_id: Some("cleanup-banned-accounts::run".to_string()),
            run_type: "manual".to_string(),
            status: "success".to_string(),
            started_at: 100,
            finished_at: Some(101),
            duration_ms: Some(1),
            output_json: Some(serde_json::json!({ "deleted": 0 }).to_string()),
            error: None,
        })
        .expect("insert plugin log");

    let resp = response_result(handle_request_with_actor(
        rpc_request(
            "plugin/logs/list",
            serde_json::json!({
                "pluginId": "cleanup-banned-accounts",
                "taskId": "cleanup-banned-accounts::run",
                "limit": 5
            }),
        ),
        RpcActor::from_parts(Some(ROLE_ADMIN), Some("admin-plugin-logs")),
    ));
    let items = resp
        .result
        .get("items")
        .and_then(|value| value.as_array())
        .expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("pluginName").and_then(|value| value.as_str()),
        Some("清理封禁账号")
    );
    assert_eq!(
        items[0].get("taskName").and_then(|value| value.as_str()),
        Some("手动清理")
    );

    let _ = std::fs::remove_file(db_path);
}

fn setup_dashboard_test_db(name: &str) -> String {
    let db_path = std::env::temp_dir()
        .join(format!(
            "{name}-{}-{}.sqlite",
            std::process::id(),
            codexmanager_core::storage::now_ts()
        ))
        .to_string_lossy()
        .to_string();
    let _ = std::fs::remove_file(&db_path);
    std::env::set_var("CODEXMANAGER_DB_PATH", &db_path);
    storage_helpers::initialize_storage().expect("init storage");
    db_path
}

fn rpc_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        id: 31.into(),
        method: method.to_string(),
        params: Some(params),
        trace: None,
    }
}

fn rpc_error(resp: &JsonRpcResponse) -> String {
    resp.result
        .get("error")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

fn create_test_member(
    username: &str,
    initial_balance_credit_micros: Option<i64>,
) -> AppUserPublicResult {
    create_app_user(AppUserCreateInput {
        username: username.to_string(),
        password: format!("{username}-password"),
        display_name: None,
        role: Some(ROLE_MEMBER.to_string()),
        initial_balance_credit_micros,
    })
    .expect("create member")
}

fn create_owned_test_api_key(user_id: &str, name: &str, model: &str) -> String {
    let created = apikey_create::create_api_key(
        Some(name.to_string()),
        Some(model.to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("create api key");
    set_api_key_owner(&created.id, "user", Some(user_id), None).expect("own api key");
    created.id
}

fn seed_test_catalog_model(slug: &str) {
    let storage = storage_helpers::open_storage().expect("open storage");
    let now = codexmanager_core::storage::now_ts();
    storage
        .upsert_model_catalog_models(&[ModelCatalogModelRecord {
            scope: "default".to_string(),
            slug: slug.to_string(),
            display_name: slug.to_string(),
            source_kind: "remote".to_string(),
            user_edited: false,
            description: None,
            default_reasoning_level: None,
            shell_type: None,
            visibility: Some("list".to_string()),
            supported_in_api: Some(true),
            priority: Some(0),
            availability_nux_json: None,
            upgrade_json: None,
            base_instructions: None,
            model_messages_json: None,
            supports_reasoning_summaries: None,
            default_reasoning_summary: None,
            support_verbosity: None,
            default_verbosity_json: None,
            apply_patch_tool_type: None,
            web_search_tool_type: None,
            truncation_mode: None,
            truncation_limit: None,
            truncation_extra_json: None,
            supports_parallel_tool_calls: None,
            supports_image_detail_original: None,
            context_window: None,
            auto_compact_token_limit: None,
            effective_context_window_percent: None,
            minimal_client_version_json: None,
            supports_search_tool: None,
            extra_json: "{}".to_string(),
            sort_index: 0,
            updated_at: now,
        }])
        .expect("seed catalog model");
}

#[test]
fn startup_snapshot_can_skip_api_model_catalog_for_light_dashboard_reads() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-startup-snapshot-light");
    seed_test_catalog_model("gpt-startup-heavy");

    let full_resp = response_result(handle_request_with_actor(
        rpc_request("startup/snapshot", serde_json::json!({})),
        RpcActor::system_admin(),
    ));
    assert!(
        full_resp.result["apiModels"]["models"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        "{:?}",
        full_resp.result
    );

    let light_resp = response_result(handle_request_with_actor(
        rpc_request(
            "startup/snapshot",
            serde_json::json!({ "includeApiModels": false }),
        ),
        RpcActor::system_admin(),
    ));
    assert_eq!(
        light_resp.result["apiModels"]["models"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn startup_snapshot_can_skip_account_details_for_light_dashboard_reads() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-startup-snapshot-account-light");
    let now = codexmanager_core::storage::now_ts();
    let account_id = "acc-startup-light";
    let mut storage = storage_helpers::open_storage().expect("open storage");
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: "Startup Light Account".to_string(),
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
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(20.0),
            window_minutes: Some(180),
            resets_at: Some(now + 300),
            secondary_used_percent: Some(30.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: Some(now + 600),
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");
    storage
        .upsert_account_metadata(account_id, Some("note"), Some("tag-a,tag-b"))
        .expect("insert metadata");
    storage
        .upsert_account_subscription(account_id, true, Some("team"), Some("plus"), None, None)
        .expect("insert subscription");
    storage
        .set_quota_source_model_assignments(
            "openai_account",
            account_id,
            &["gpt-visible".to_string()],
        )
        .expect("insert model assignment");
    storage
        .upsert_account_quota_capacity_override(account_id, Some(100), Some(200))
        .expect("insert quota override");

    let full_resp = response_result(handle_request_with_actor(
        rpc_request("startup/snapshot", serde_json::json!({})),
        RpcActor::system_admin(),
    ));
    let full_account = &full_resp.result["accounts"][0];
    assert_eq!(full_account["note"].as_str(), Some("note"));
    assert_eq!(full_account["subscriptionPlan"].as_str(), Some("plus"));
    assert_eq!(full_account["modelSlugs"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        full_account["quotaCapacityPrimaryWindowTokens"].as_i64(),
        Some(100)
    );

    let light_resp = response_result(handle_request_with_actor(
        rpc_request(
            "startup/snapshot",
            serde_json::json!({ "includeAccountDetails": false }),
        ),
        RpcActor::system_admin(),
    ));
    let light_account = &light_resp.result["accounts"][0];
    assert_eq!(light_account["id"].as_str(), Some(account_id));
    assert_eq!(light_account["hasToken"].as_bool(), Some(true));
    assert_eq!(
        light_resp.result["usageSnapshots"].as_array().map(Vec::len),
        Some(1)
    );
    assert!(light_account["note"].is_null());
    assert!(light_account["subscriptionPlan"].is_null());
    assert_eq!(
        light_account["modelSlugs"].as_array().map(Vec::len),
        Some(0)
    );
    assert!(light_account["quotaCapacityPrimaryWindowTokens"].is_null());

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn startup_snapshot_can_skip_api_keys_for_light_dashboard_reads() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-startup-snapshot-api-key-light");
    let user = create_test_member("startup-api-key-light", None);
    let key_id = create_owned_test_api_key(&user.id, "startup light key", "gpt-5-mini");

    let full_resp = response_result(handle_request_with_actor(
        rpc_request("startup/snapshot", serde_json::json!({})),
        RpcActor::system_admin(),
    ));
    assert!(
        full_resp.result["apiKeys"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == key_id)),
        "{:?}",
        full_resp.result
    );

    let light_resp = response_result(handle_request_with_actor(
        rpc_request(
            "startup/snapshot",
            serde_json::json!({ "includeApiKeys": false }),
        ),
        RpcActor::system_admin(),
    ));
    assert_eq!(
        light_resp.result["apiKeys"].as_array().map(Vec::len),
        Some(0)
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn startup_snapshot_can_skip_account_runtime_for_light_dashboard_reads() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-startup-snapshot-runtime-light");
    let now = codexmanager_core::storage::now_ts();
    let account_id = "acc-startup-runtime-light";
    let mut storage = storage_helpers::open_storage().expect("open storage");
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: "Startup Runtime Light".to_string(),
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
    storage
        .set_preferred_account(Some(account_id))
        .expect("set preferred account");
    storage
        .insert_token(&Token {
            account_id: account_id.to_string(),
            id_token: "id".to_string(),
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert token");
    storage
        .insert_event(&Event {
            account_id: Some(account_id.to_string()),
            event_type: "account_status".to_string(),
            message: "status=unavailable reason=usage_http_401".to_string(),
            created_at: now,
        })
        .expect("insert status reason event");
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(20.0),
            window_minutes: Some(180),
            resets_at: Some(now + 300),
            secondary_used_percent: Some(30.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: Some(now + 600),
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");

    let light_resp = response_result(handle_request_with_actor(
        rpc_request(
            "startup/snapshot",
            serde_json::json!({
                "includeAccountRuntime": false,
                "includeAccountDetails": false
            }),
        ),
        RpcActor::system_admin(),
    ));
    let account = &light_resp.result["accounts"][0];
    assert_eq!(account["id"].as_str(), Some(account_id));
    assert_eq!(account["preferred"].as_bool(), Some(false));
    assert_eq!(account["hasToken"].as_bool(), Some(false));
    assert!(account["statusReason"].is_null());
    assert_eq!(
        light_resp.result["usageSnapshots"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(
        light_resp.result["usageAggregateSummary"]["primaryKnownCount"].as_i64(),
        Some(1)
    );
    assert_eq!(
        light_resp.result["accountSummary"]["accountCount"].as_i64(),
        Some(1)
    );
    assert_eq!(
        light_resp.result["accountSummary"]["primaryRemainPercent"].as_i64(),
        Some(80)
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn startup_snapshot_can_skip_account_payload_for_dashboard_summary_reads() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-startup-snapshot-summary-only");
    let now = codexmanager_core::storage::now_ts();
    let account_id = "acc-startup-summary-only";
    let storage = storage_helpers::open_storage().expect("open storage");
    storage
        .insert_account(&Account {
            id: account_id.to_string(),
            label: "Startup Summary Only".to_string(),
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
    storage
        .insert_usage_snapshot(&UsageSnapshotRecord {
            account_id: account_id.to_string(),
            used_percent: Some(20.0),
            window_minutes: Some(180),
            resets_at: Some(now + 300),
            secondary_used_percent: Some(30.0),
            secondary_window_minutes: Some(10_080),
            secondary_resets_at: Some(now + 600),
            credits_json: None,
            captured_at: now,
        })
        .expect("insert usage snapshot");

    let light_resp = response_result(handle_request_with_actor(
        rpc_request(
            "startup/snapshot",
            serde_json::json!({
                "requestLogLimit": 0,
                "includeApiModels": false,
                "includeApiKeys": false,
                "includeAccounts": false,
                "includeUsageSnapshots": false,
                "includeAccountRuntime": false,
                "includeAccountDetails": false
            }),
        ),
        RpcActor::system_admin(),
    ));

    assert_eq!(
        light_resp.result["accounts"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        light_resp.result["usageSnapshots"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        light_resp.result["accountSummary"]["accountCount"].as_i64(),
        Some(1)
    );
    assert_eq!(
        light_resp.result["usageAggregateSummary"]["primaryKnownCount"].as_i64(),
        Some(1)
    );
    assert_eq!(
        light_resp.result["usageAggregateSummary"]["primaryRemainPercent"].as_i64(),
        Some(80)
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn request_log_summary_reuses_filtered_count_for_all_status() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-requestlog-summary-count");
    let day_start = 1_700_000_000;

    insert_test_request_log(
        "key-summary",
        "trace-summary-ok",
        "gpt-5-mini",
        200,
        10,
        0,
        20,
        0.01,
        day_start + 10,
    );
    insert_test_request_log(
        "key-summary",
        "trace-summary-error",
        "gpt-5-mini",
        502,
        30,
        0,
        40,
        0.02,
        day_start + 20,
    );

    let all_resp = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/summary",
            serde_json::json!({ "statusFilter": "all" }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        all_resp.result.get("error").is_none(),
        "{:?}",
        all_resp.result
    );
    assert_eq!(all_resp.result["totalCount"].as_i64(), Some(2));
    assert_eq!(all_resp.result["filteredCount"].as_i64(), Some(2));

    let success_resp = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/summary",
            serde_json::json!({ "statusFilter": "2xx" }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        success_resp.result.get("error").is_none(),
        "{:?}",
        success_resp.result
    );
    assert_eq!(success_resp.result["totalCount"].as_i64(), Some(2));
    assert_eq!(success_resp.result["filteredCount"].as_i64(), Some(1));

    let _ = std::fs::remove_file(db_path);
}

fn insert_test_request_log(
    key_id: &str,
    trace_id: &str,
    model: &str,
    status_code: i64,
    input_tokens: i64,
    cached_input_tokens: i64,
    output_tokens: i64,
    estimated_cost_usd: f64,
    created_at: i64,
) {
    let total_tokens = input_tokens + output_tokens;
    let storage = storage_helpers::open_storage().expect("open storage");
    storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                trace_id: Some(trace_id.to_string()),
                key_id: Some(key_id.to_string()),
                request_path: "/v1/chat/completions".to_string(),
                method: "POST".to_string(),
                model: Some(model.to_string()),
                upstream_model: Some(format!("{model}-upstream")),
                actual_source_kind: Some("openai_account".to_string()),
                actual_source_id: Some("private-account-id".to_string()),
                status_code: Some(status_code),
                input_tokens: Some(input_tokens),
                cached_input_tokens: Some(cached_input_tokens),
                output_tokens: Some(output_tokens),
                total_tokens: Some(total_tokens),
                estimated_cost_usd: Some(estimated_cost_usd),
                created_at,
                ..RequestLog::default()
            },
            &RequestTokenStat {
                key_id: Some(key_id.to_string()),
                model: Some(model.to_string()),
                input_tokens: Some(input_tokens),
                cached_input_tokens: Some(cached_input_tokens),
                output_tokens: Some(output_tokens),
                total_tokens: Some(total_tokens),
                estimated_cost_usd: Some(estimated_cost_usd),
                created_at,
                ..RequestTokenStat::default()
            },
        )
        .expect("insert request log");
}

#[test]
fn set_model_group_models_validates_requested_catalog_slugs_only() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-model-group-model-slug-validation");
    set_web_auth_mode("accounts").expect("enable accounts mode");
    seed_test_catalog_model("gpt-5-mini");
    let storage = storage_helpers::open_storage().expect("open storage");
    let group_id = storage
        .default_model_group_id()
        .expect("read default model group")
        .expect("default model group");
    drop(storage);

    let result = set_model_group_models(ModelGroupModelsSetParams {
        group_id: group_id.clone(),
        models: vec![
            ModelGroupModelUpsertParams {
                platform_model_slug: "gpt-5-mini".to_string(),
                enabled: Some(true),
                rate_multiplier_millis: Some(1200),
                billing_model_slug: None,
                note: Some("primary".to_string()),
            },
            ModelGroupModelUpsertParams {
                platform_model_slug: "gpt-5-mini".to_string(),
                enabled: Some(false),
                rate_multiplier_millis: Some(900),
                billing_model_slug: None,
                note: Some("duplicate ignored".to_string()),
            },
        ],
    })
    .expect("set model group models");

    let saved = result
        .models
        .iter()
        .filter(|item| item.group_id == group_id && item.platform_model_slug == "gpt-5-mini")
        .collect::<Vec<_>>();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].enabled);
    assert_eq!(saved[0].rate_multiplier_millis, Some(1200));
    assert_eq!(saved[0].note.as_deref(), Some("primary"));

    let err = set_model_group_models(ModelGroupModelsSetParams {
        group_id,
        models: vec![ModelGroupModelUpsertParams {
            platform_model_slug: "missing-model".to_string(),
            enabled: Some(true),
            rate_multiplier_millis: None,
            billing_model_slug: None,
            note: None,
        }],
    })
    .expect_err("missing catalog model should fail");
    assert!(err.contains("平台模型 `missing-model` 不存在"));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn set_model_group_users_batches_member_validation_and_dedupes() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-model-group-users-batch-validation");
    set_web_auth_mode("accounts").expect("enable accounts mode");
    let admin = create_app_user(AppUserCreateInput {
        username: "admin-model-group-users".to_string(),
        password: "password-one".to_string(),
        display_name: None,
        role: Some(ROLE_ADMIN.to_string()),
        initial_balance_credit_micros: None,
    })
    .expect("create admin");
    let member_one = create_test_member("member-model-group-users-one", None);
    let member_two = create_test_member("member-model-group-users-two", None);
    let storage = storage_helpers::open_storage().expect("open storage");
    let group_id = storage
        .default_model_group_id()
        .expect("read default model group")
        .expect("default model group");
    drop(storage);

    let result = set_model_group_users(ModelGroupUsersSetParams {
        group_id: group_id.clone(),
        user_ids: vec![
            member_two.id.clone(),
            member_one.id.clone(),
            member_two.id.clone(),
        ],
    })
    .expect("set model group users");

    let saved = result
        .user_assignments
        .iter()
        .filter(|item| item.group_id == group_id)
        .collect::<Vec<_>>();
    assert_eq!(saved.len(), 2);
    assert!(saved.iter().any(|item| item.user_id == member_one.id));
    assert!(saved.iter().any(|item| item.user_id == member_two.id));

    let err = set_model_group_users(ModelGroupUsersSetParams {
        group_id,
        user_ids: vec![admin.id],
    })
    .expect_err("admin should not be assigned to member model group");
    assert!(err.contains("不是成员账号"));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn wallet_charge_uses_model_group_billing_model_override() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-model-group-billing-override");
    set_web_auth_mode("accounts").expect("enable accounts mode");
    set_distribution_enabled(true).expect("enable distribution");
    let user = create_test_member("member-model-group-billing", Some(1_000_000));
    let key_id = create_owned_test_api_key(&user.id, "member model group key", "gpt-5-mini");
    seed_test_catalog_model("gpt-5-mini");
    let storage = storage_helpers::open_storage().expect("open storage");
    let group_id = storage
        .default_model_group_id()
        .expect("read default model group")
        .expect("default model group");
    let now = codexmanager_core::storage::now_ts();
    storage
        .replace_model_group_models(
            &group_id,
            &[ModelGroupModel {
                group_id: group_id.clone(),
                platform_model_slug: "gpt-5-mini".to_string(),
                enabled: true,
                rate_multiplier_millis: Some(1000),
                billing_model_slug: Some("gpt-5.5".to_string()),
                note: None,
                created_at: now,
                updated_at: now,
            }],
        )
        .expect("save model group models");

    let ledger = wallet_charge_for_request(
        &storage,
        Some(&key_id),
        42,
        0.00225,
        Some("gpt-5-mini"),
        None,
        Some(
            serde_json::json!({
                "inputTokens": 1000,
                "cachedInputTokens": 0,
                "outputTokens": 1000
            })
            .to_string(),
        ),
    )
    .expect("charge wallet")
    .expect("ledger entry");

    assert_eq!(ledger.amount_credit_micros, -35_000);
    let usage: serde_json::Value =
        serde_json::from_str(ledger.raw_usage_json.as_deref().unwrap()).expect("usage json");
    assert_eq!(usage["billingModelSlug"], "gpt-5.5");
    assert_eq!(usage["platformEstimatedCostUsd"], 0.00225);
    assert!((usage["baseEstimatedCostUsd"].as_f64().unwrap() - 0.035).abs() < 0.000_001);
    assert!((usage["chargedCostUsd"].as_f64().unwrap() - 0.035).abs() < 0.000_001);
    let wallet = storage
        .find_wallet_by_owner("user", &user.id)
        .expect("read wallet")
        .expect("wallet");
    assert_eq!(wallet.balance_credit_micros, 965_000);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_dashboard_filters_to_current_user_keys() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-dashboard-filter");
    let day_start = 1_700_000_000;
    let day_end = day_start + 86_400;
    let user_one = create_app_user(AppUserCreateInput {
        username: "member-one".to_string(),
        password: "password-one".to_string(),
        display_name: None,
        role: Some(ROLE_MEMBER.to_string()),
        initial_balance_credit_micros: Some(2_000_000),
    })
    .expect("create member one");
    let user_two = create_app_user(AppUserCreateInput {
        username: "member-two".to_string(),
        password: "password-two".to_string(),
        display_name: None,
        role: Some(ROLE_MEMBER.to_string()),
        initial_balance_credit_micros: Some(2_000_000),
    })
    .expect("create member two");
    let key_one = apikey_create::create_api_key(
        Some("member one key".to_string()),
        Some("gpt-5-mini".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("create key one");
    let key_two = apikey_create::create_api_key(
        Some("member two key".to_string()),
        Some("gpt-5-mini".to_string()),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("create key two");
    set_api_key_owner(&key_one.id, "user", Some(&user_one.id), None).expect("own key one");
    set_api_key_owner(&key_two.id, "user", Some(&user_two.id), None).expect("own key two");

    let storage = storage_helpers::open_storage().expect("open storage");
    storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                trace_id: Some("trace-one".to_string()),
                key_id: Some(key_one.id.clone()),
                request_path: "/v1/chat/completions".to_string(),
                method: "POST".to_string(),
                model: Some("gpt-5-mini".to_string()),
                status_code: Some(200),
                input_tokens: Some(40),
                cached_input_tokens: Some(10),
                output_tokens: Some(30),
                total_tokens: Some(70),
                estimated_cost_usd: Some(0.01),
                created_at: day_start + 10,
                ..RequestLog::default()
            },
            &RequestTokenStat {
                key_id: Some(key_one.id.clone()),
                model: Some("gpt-5-mini".to_string()),
                input_tokens: Some(40),
                cached_input_tokens: Some(10),
                output_tokens: Some(30),
                total_tokens: Some(70),
                estimated_cost_usd: Some(0.01),
                created_at: day_start + 10,
                ..RequestTokenStat::default()
            },
        )
        .expect("insert member one log");
    storage
        .insert_request_log_with_token_stat(
            &RequestLog {
                trace_id: Some("trace-two".to_string()),
                key_id: Some(key_two.id.clone()),
                request_path: "/v1/chat/completions".to_string(),
                method: "POST".to_string(),
                model: Some("gpt-5-mini".to_string()),
                status_code: Some(200),
                input_tokens: Some(400),
                cached_input_tokens: Some(0),
                output_tokens: Some(300),
                total_tokens: Some(700),
                estimated_cost_usd: Some(0.1),
                created_at: day_start + 20,
                ..RequestLog::default()
            },
            &RequestTokenStat {
                key_id: Some(key_two.id.clone()),
                model: Some("gpt-5-mini".to_string()),
                input_tokens: Some(400),
                output_tokens: Some(300),
                total_tokens: Some(700),
                estimated_cost_usd: Some(0.1),
                created_at: day_start + 20,
                ..RequestTokenStat::default()
            },
        )
        .expect("insert member two log");

    let resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/memberSummary",
            serde_json::json!({
                "dayStartTs": day_start,
                "dayEndTs": day_end
            }),
        ),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user_one.id)),
    ));

    assert!(resp.result.get("error").is_none(), "{:?}", resp.result);
    assert_eq!(resp.result["apiKeySummary"]["totalCount"], 1);
    assert_eq!(resp.result["usageToday"]["totalTokens"], 70);
    assert_eq!(resp.result["recentLogs"][0]["keyId"], key_one.id);
    assert_eq!(resp.result["topKeys"][0]["keyId"], key_one.id);
    assert_eq!(resp.result["topKeys"][0]["todayTokens"], 70);
    assert_eq!(
        resp.result["availableModels"]
            .as_array()
            .expect("available models array")
            .len(),
        0
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_dashboard_can_skip_detail_payloads() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-dashboard-light");
    let day_start = 1_700_000_000;
    let day_end = day_start + 86_400;
    let user = create_test_member("member-light", Some(2_000_000));
    let key_id = create_owned_test_api_key(&user.id, "member light key", "gpt-5-mini");
    seed_test_catalog_model("gpt-5-mini");

    insert_test_request_log(
        &key_id,
        "trace-member-light",
        "gpt-5-mini",
        200,
        40,
        4,
        30,
        0.01,
        day_start + 10,
    );

    let resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/memberSummary",
            serde_json::json!({
                "dayStartTs": day_start,
                "dayEndTs": day_end,
                "includeDetails": false
            }),
        ),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user.id)),
    ));

    assert!(resp.result.get("error").is_none(), "{:?}", resp.result);
    assert_eq!(resp.result["apiKeySummary"]["totalCount"], 1);
    assert_eq!(resp.result["usageToday"]["totalTokens"], 70);
    assert_eq!(
        resp.result["availableModels"]
            .as_array()
            .expect("available models array")
            .len(),
        0
    );
    assert_eq!(
        resp.result["recentLogs"]
            .as_array()
            .expect("recent logs array")
            .len(),
        0
    );
    assert!(resp.result["alerts"]
        .as_array()
        .map(|items| !items
            .iter()
            .any(|item| item["kind"] == "no_available_model"))
        .unwrap_or(false));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_dashboard_no_key_returns_alert() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-dashboard-empty");
    let user = create_app_user(AppUserCreateInput {
        username: "member-empty".to_string(),
        password: "password-empty".to_string(),
        display_name: None,
        role: Some(ROLE_MEMBER.to_string()),
        initial_balance_credit_micros: None,
    })
    .expect("create member");

    let resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/memberSummary",
            serde_json::json!({
                "dayStartTs": 1_700_000_000,
                "dayEndTs": 1_700_086_400
            }),
        ),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user.id)),
    ));

    assert!(resp.result.get("error").is_none(), "{:?}", resp.result);
    assert_eq!(resp.result["apiKeySummary"]["totalCount"], 0);
    assert!(resp.result["alerts"]
        .as_array()
        .map(|items| items.iter().any(|item| item["kind"] == "no_api_key"))
        .unwrap_or(false));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_dashboard_ignores_requested_user_id() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-dashboard-user-id-spoof");
    let day_start = 1_700_000_000;
    let day_end = day_start + 86_400;
    let user_one = create_test_member("member-spoof-one", Some(2_000_000));
    let user_two = create_test_member("member-spoof-two", Some(2_000_000));
    let key_one = create_owned_test_api_key(&user_one.id, "member spoof one key", "gpt-5-mini");
    let key_two = create_owned_test_api_key(&user_two.id, "member spoof two key", "gpt-5-mini");

    insert_test_request_log(
        &key_one,
        "trace-spoof-one",
        "gpt-5-mini",
        200,
        12,
        2,
        8,
        0.01,
        day_start + 10,
    );
    insert_test_request_log(
        &key_two,
        "trace-spoof-two",
        "gpt-5-mini",
        200,
        1200,
        0,
        800,
        1.0,
        day_start + 20,
    );

    let resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/memberSummary",
            serde_json::json!({
                "userId": user_two.id,
                "dayStartTs": day_start,
                "dayEndTs": day_end
            }),
        ),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user_one.id)),
    ));

    assert!(resp.result.get("error").is_none(), "{:?}", resp.result);
    assert_eq!(resp.result["userId"], user_one.id);
    assert_eq!(resp.result["apiKeySummary"]["totalCount"], 1);
    assert_eq!(resp.result["usageToday"]["totalTokens"], 20);
    assert_eq!(resp.result["recentLogs"][0]["keyId"], key_one);
    assert_eq!(resp.result["topKeys"][0]["keyId"], key_one);
    assert_ne!(resp.result["recentLogs"][0]["keyId"], key_two);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn admin_member_dashboard_can_query_requested_user() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-admin-member-dashboard-debug");
    let day_start = 1_700_000_000;
    let day_end = day_start + 86_400;
    let user_one = create_test_member("admin-debug-one", Some(2_000_000));
    let user_two = create_test_member("admin-debug-two", Some(2_000_000));
    let key_one = create_owned_test_api_key(&user_one.id, "admin debug one key", "gpt-5-mini");
    let key_two = create_owned_test_api_key(&user_two.id, "admin debug two key", "gpt-5-mini");

    insert_test_request_log(
        &key_one,
        "trace-admin-debug-one",
        "gpt-5-mini",
        200,
        10,
        0,
        10,
        0.01,
        day_start + 10,
    );
    insert_test_request_log(
        &key_two,
        "trace-admin-debug-two",
        "gpt-5-mini",
        200,
        40,
        5,
        20,
        0.02,
        day_start + 20,
    );

    let resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/memberSummary",
            serde_json::json!({
                "userId": user_two.id,
                "dayStartTs": day_start,
                "dayEndTs": day_end
            }),
        ),
        RpcActor::system_admin(),
    ));

    assert!(resp.result.get("error").is_none(), "{:?}", resp.result);
    assert_eq!(resp.result["userId"], user_two.id);
    assert_eq!(resp.result["apiKeySummary"]["totalCount"], 1);
    assert_eq!(resp.result["usageToday"]["totalTokens"], 60);
    assert_eq!(resp.result["recentLogs"][0]["keyId"], key_two);
    assert_ne!(resp.result["recentLogs"][0]["keyId"], key_one);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn admin_usage_summary_requires_admin_and_returns_range_rollups() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-admin-usage-summary");
    let day_start = 1_700_000_000;
    let day_end = day_start + 86_400;
    let user = create_test_member("admin-usage-member", Some(2_000_000));
    let key_id = create_owned_test_api_key(&user.id, "admin usage key", "gpt-5-mini");

    insert_test_request_log(
        &key_id,
        "trace-admin-usage",
        "gpt-5-mini",
        200,
        20,
        5,
        10,
        0.03,
        day_start + 10,
    );

    let member_resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/adminUsageSummary",
            serde_json::json!({
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user.id)),
    ));
    assert!(
        rpc_error(&member_resp).contains("permission_denied"),
        "{:?}",
        member_resp.result
    );

    let admin_resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/adminUsageSummary",
            serde_json::json!({
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        admin_resp.result.get("error").is_none(),
        "{:?}",
        admin_resp.result
    );
    assert_eq!(admin_resp.result["rangeStartTs"], day_start);
    assert_eq!(admin_resp.result["rangeEndTs"], day_end);
    assert_eq!(admin_resp.result["dailyUsage"].as_array().unwrap().len(), 1);
    assert_eq!(
        admin_resp.result["dailyUsage"][0]["usage"]["totalTokens"],
        30
    );
    assert_eq!(
        admin_resp.result["dailyUsage"][0]["usage"]["requestCount"],
        1
    );

    let user_item = admin_resp.result["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["userId"] == user.id)
        .expect("user usage item");
    assert_eq!(user_item["rangeUsage"]["totalTokens"], 30);

    let account_item = admin_resp.result["openaiAccounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["sourceId"] == "private-account-id")
        .expect("account usage item");
    assert_eq!(account_item["rangeUsage"]["totalTokens"], 30);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn admin_usage_summary_can_skip_breakdowns_for_light_dashboard() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-admin-usage-light");
    let day_start = 1_700_000_000;
    let day_end = day_start + 86_400;
    let user = create_test_member("admin-usage-light-member", Some(2_000_000));
    let key_id = create_owned_test_api_key(&user.id, "admin usage light key", "gpt-5-mini");

    insert_test_request_log(
        &key_id,
        "trace-admin-usage-light",
        "gpt-5-mini",
        200,
        20,
        5,
        10,
        0.03,
        day_start + 10,
    );

    let admin_resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/adminUsageSummary",
            serde_json::json!({
                "startTs": day_start,
                "endTs": day_end,
                "includeBreakdowns": false
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        admin_resp.result.get("error").is_none(),
        "{:?}",
        admin_resp.result
    );
    assert_eq!(admin_resp.result["dailyUsage"].as_array().unwrap().len(), 1);
    assert_eq!(
        admin_resp.result["dailyUsage"][0]["usage"]["totalTokens"],
        30
    );
    assert_eq!(admin_resp.result["users"].as_array().unwrap().len(), 0);
    assert_eq!(
        admin_resp.result["openaiAccounts"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        admin_resp.result["aggregateApis"].as_array().unwrap().len(),
        0
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn admin_usage_summary_daily_trend_includes_token_stats_without_request_logs() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-admin-usage-orphan-stats");
    let day_start = 1_700_000_000;
    let day_end = day_start + 3 * 86_400;
    let user = create_test_member("admin-usage-orphan-member", Some(2_000_000));
    let key_id = create_owned_test_api_key(&user.id, "admin orphan key", "gpt-5-mini");
    let storage = storage_helpers::open_storage().expect("open storage");

    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 98_001,
            key_id: Some(key_id.clone()),
            account_id: Some("private-account-id".to_string()),
            model: Some("gpt-5-mini".to_string()),
            input_tokens: Some(400),
            cached_input_tokens: Some(0),
            output_tokens: Some(100),
            total_tokens: Some(500),
            estimated_cost_usd: Some(0.5),
            created_at: day_start + 120,
            ..RequestTokenStat::default()
        })
        .expect("insert orphan day one stat");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id: 98_002,
            key_id: Some(key_id.clone()),
            account_id: Some("private-account-id".to_string()),
            model: Some("gpt-5-mini".to_string()),
            input_tokens: Some(300),
            cached_input_tokens: Some(0),
            output_tokens: Some(100),
            total_tokens: Some(400),
            estimated_cost_usd: Some(0.4),
            created_at: day_start + 86_400 + 180,
            ..RequestTokenStat::default()
        })
        .expect("insert orphan day two stat");

    insert_test_request_log(
        &key_id,
        "trace-admin-usage-current-day",
        "gpt-5-mini",
        200,
        20,
        5,
        10,
        0.03,
        day_start + 2 * 86_400 + 240,
    );

    let admin_resp = response_result(handle_request_with_actor(
        rpc_request(
            "dashboard/adminUsageSummary",
            serde_json::json!({
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        admin_resp.result.get("error").is_none(),
        "{:?}",
        admin_resp.result
    );
    assert_eq!(admin_resp.result["dailyUsage"].as_array().unwrap().len(), 3);
    assert_eq!(
        admin_resp.result["dailyUsage"][0]["usage"]["totalTokens"],
        500
    );
    assert_eq!(
        admin_resp.result["dailyUsage"][0]["usage"]["requestCount"],
        1
    );
    assert_eq!(
        admin_resp.result["dailyUsage"][1]["usage"]["totalTokens"],
        400
    );
    assert_eq!(
        admin_resp.result["dailyUsage"][1]["usage"]["requestCount"],
        1
    );
    assert_eq!(
        admin_resp.result["dailyUsage"][2]["usage"]["totalTokens"],
        30
    );
    assert_eq!(
        admin_resp.result["dailyUsage"][2]["usage"]["requestCount"],
        1
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_cannot_read_or_mutate_other_user_api_key() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-apikey-cross-user-deny");
    let user_one = create_test_member("apikey-deny-one", Some(2_000_000));
    let user_two = create_test_member("apikey-deny-two", Some(2_000_000));
    let key_one = create_owned_test_api_key(&user_one.id, "member one private key", "gpt-5-mini");
    let key_two = create_owned_test_api_key(&user_two.id, "member two private key", "gpt-5-mini");
    let actor_one = RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user_one.id));

    for (method, params) in [
        ("apikey/readSecret", serde_json::json!({ "id": key_two })),
        (
            "apikey/updateModel",
            serde_json::json!({ "id": key_two, "name": "stolen", "modelSlug": "gpt-5" }),
        ),
        ("apikey/disable", serde_json::json!({ "id": key_two })),
        ("apikey/delete", serde_json::json!({ "id": key_two })),
    ] {
        let resp = response_result(handle_request_with_actor(
            rpc_request(method, params),
            actor_one.clone(),
        ));
        assert!(
            rpc_error(&resp).contains("permission_denied"),
            "{method} should deny cross-user access: {:?}",
            resp.result
        );
    }

    let member_one_list = response_result(handle_request_with_actor(
        rpc_request("apikey/list", serde_json::json!({})),
        actor_one,
    ));
    assert_eq!(member_one_list.result["items"].as_array().unwrap().len(), 1);
    assert_eq!(member_one_list.result["items"][0]["id"], key_one);

    let member_two_list = response_result(handle_request_with_actor(
        rpc_request("apikey/list", serde_json::json!({})),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user_two.id)),
    ));
    assert_eq!(member_two_list.result["items"].as_array().unwrap().len(), 1);
    assert_eq!(member_two_list.result["items"][0]["id"], key_two);
    assert_eq!(
        member_two_list.result["items"][0]["name"],
        "member two private key"
    );
    assert_eq!(member_two_list.result["items"][0]["status"], "active");

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_api_key_usage_stats_filter_to_owned_keys() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-apikey-usage-filter");
    let day_start = 1_700_000_000;
    let user_one = create_test_member("apikey-usage-one", Some(2_000_000));
    let user_two = create_test_member("apikey-usage-two", Some(2_000_000));
    let key_one = create_owned_test_api_key(&user_one.id, "usage one key", "gpt-5-mini");
    let key_two = create_owned_test_api_key(&user_two.id, "usage two key", "gpt-5");

    insert_test_request_log(
        &key_one,
        "trace-usage-one",
        "gpt-5-mini",
        200,
        80,
        10,
        40,
        0.08,
        day_start + 10,
    );
    insert_test_request_log(
        &key_two,
        "trace-usage-two",
        "gpt-5",
        200,
        800,
        0,
        500,
        0.8,
        day_start + 20,
    );

    let member_stats = response_result(handle_request_with_actor(
        rpc_request("apikey/usageStats", serde_json::json!({})),
        RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user_one.id)),
    ));
    assert!(
        member_stats.result.get("error").is_none(),
        "{:?}",
        member_stats.result
    );
    let member_items = member_stats.result["items"].as_array().unwrap();
    assert_eq!(member_items.len(), 1);
    assert_eq!(member_items[0]["keyId"], key_one);
    assert_eq!(member_items[0]["totalTokens"], 120);

    let admin_stats = response_result(handle_request_with_actor(
        rpc_request("apikey/usageStats", serde_json::json!({})),
        RpcActor::system_admin(),
    ));
    assert_eq!(admin_stats.result["items"].as_array().unwrap().len(), 2);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_created_api_key_ignores_admin_only_routing_fields() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-apikey-create-sanitizes");
    let user = create_test_member("apikey-create-sanitize", Some(2_000_000));
    let actor = RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user.id));

    let created = response_result(handle_request_with_actor(
        rpc_request(
            "apikey/create",
            serde_json::json!({
                "name": "member safe key",
                "modelSlug": "gpt-5-mini",
                "rotationStrategy": "aggregate_api_rotation",
                "aggregateApiId": "agg-secret",
                "upstreamBaseUrl": "https://example.invalid/v1",
                "staticHeadersJson": "{\"x-admin\":\"secret\"}",
                "accountPlanFilter": "pro"
            }),
        ),
        actor.clone(),
    ));
    assert!(
        created.result.get("error").is_none(),
        "{:?}",
        created.result
    );

    let listed = response_result(handle_request_with_actor(
        rpc_request("apikey/list", serde_json::json!({})),
        actor,
    ));
    assert_eq!(listed.result["items"].as_array().unwrap().len(), 1);
    let item = &listed.result["items"][0];
    assert_eq!(item["id"], created.result["id"]);
    assert_eq!(item["rotationStrategy"], "account_rotation");
    assert!(item["aggregateApiId"].is_null());
    assert!(item["upstreamBaseUrl"].is_null());
    assert!(item["staticHeadersJson"].is_null());
    assert!(item["accountPlanFilter"].is_null());

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn member_requestlog_queries_filter_to_owned_keys() {
    let _guard = test_env_guard();
    let db_path = setup_dashboard_test_db("codexmanager-member-requestlog-filter");
    let day_start = 1_700_000_000;
    let day_end = day_start + 86_400;
    let user_one = create_test_member("log-filter-one", Some(2_000_000));
    let user_two = create_test_member("log-filter-two", Some(2_000_000));
    let key_one = create_owned_test_api_key(&user_one.id, "log filter one key", "gpt-5-mini");
    let key_two = create_owned_test_api_key(&user_two.id, "log filter two key", "gpt-5-mini");
    let actor_one = RpcActor::from_parts(Some(ROLE_MEMBER), Some(&user_one.id));

    insert_test_request_log(
        &key_one,
        "trace-log-filter-one",
        "gpt-5-mini",
        200,
        30,
        10,
        20,
        0.03,
        day_start + 10,
    );
    insert_test_request_log(
        &key_two,
        "trace-log-filter-two",
        "gpt-5",
        500,
        300,
        0,
        200,
        0.3,
        day_start + 20,
    );

    let list = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/list",
            serde_json::json!({
                "page": 1,
                "pageSize": 20,
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        actor_one.clone(),
    ));
    assert!(list.result.get("error").is_none(), "{:?}", list.result);
    assert_eq!(list.result["total"], 1);
    assert_eq!(list.result["items"][0]["keyId"], key_one);
    assert_eq!(list.result["items"][0]["model"], "gpt-5-mini");
    assert!(list.result["items"][0]["upstreamModel"].is_null());
    assert!(list.result["items"][0]["actualSourceKind"].is_null());
    assert!(list.result["items"][0]["actualSourceId"].is_null());

    let list_with_summary = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/list_with_summary",
            serde_json::json!({
                "page": 1,
                "pageSize": 20,
                "query": "trace-log-filter-one",
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        actor_one.clone(),
    ));
    assert!(
        list_with_summary.result.get("error").is_none(),
        "{:?}",
        list_with_summary.result
    );
    assert_eq!(list_with_summary.result["total"], 1);
    assert_eq!(list_with_summary.result["items"][0]["keyId"], key_one);
    assert_eq!(list_with_summary.result["summary"]["totalCount"], 1);
    assert_eq!(list_with_summary.result["summary"]["filteredCount"], 1);
    assert_eq!(list_with_summary.result["summary"]["successCount"], 1);

    let hidden_model_query = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/list",
            serde_json::json!({
                "page": 1,
                "pageSize": 20,
                "query": "upstream_model:=gpt-5-mini-upstream",
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        actor_one.clone(),
    ));
    assert!(
        hidden_model_query.result.get("error").is_none(),
        "{:?}",
        hidden_model_query.result
    );
    assert_eq!(hidden_model_query.result["total"], 0);

    let hidden_model_global_query = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/list",
            serde_json::json!({
                "page": 1,
                "pageSize": 20,
                "query": "gpt-5-mini-upstream",
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        actor_one.clone(),
    ));
    assert!(
        hidden_model_global_query.result.get("error").is_none(),
        "{:?}",
        hidden_model_global_query.result
    );
    assert_eq!(hidden_model_global_query.result["total"], 0);

    let admin_list = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/list",
            serde_json::json!({
                "page": 1,
                "pageSize": 20,
                "query": "upstream_model:=gpt-5-mini-upstream",
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        RpcActor::system_admin(),
    ));
    assert!(
        admin_list.result.get("error").is_none(),
        "{:?}",
        admin_list.result
    );
    assert_eq!(admin_list.result["total"], 1);
    assert_eq!(
        admin_list.result["items"][0]["upstreamModel"],
        "gpt-5-mini-upstream"
    );
    assert_eq!(
        admin_list.result["items"][0]["actualSourceKind"],
        "openai_account"
    );
    assert_eq!(
        admin_list.result["items"][0]["actualSourceId"],
        "private-account-id"
    );

    let summary = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/summary",
            serde_json::json!({
                "page": 1,
                "pageSize": 20,
                "startTs": day_start,
                "endTs": day_end
            }),
        ),
        actor_one.clone(),
    ));
    assert!(
        summary.result.get("error").is_none(),
        "{:?}",
        summary.result
    );
    assert_eq!(summary.result["totalCount"], 1);
    assert_eq!(summary.result["filteredCount"], 1);
    assert_eq!(summary.result["successCount"], 1);
    assert_eq!(summary.result["errorCount"], 0);
    assert_eq!(summary.result["totalTokens"], 50);

    let today = response_result(handle_request_with_actor(
        rpc_request(
            "requestlog/today_summary",
            serde_json::json!({
                "dayStartTs": day_start,
                "dayEndTs": day_end
            }),
        ),
        actor_one.clone(),
    ));
    assert!(today.result.get("error").is_none(), "{:?}", today.result);
    assert_eq!(today.result["todayTokens"], 40);
    assert_eq!(today.result["estimatedCost"], 0.03);

    let clear = response_result(handle_request_with_actor(
        rpc_request("requestlog/clear", serde_json::json!({})),
        actor_one,
    ));
    assert!(
        rpc_error(&clear).contains("permission_denied"),
        "member must not clear global logs: {:?}",
        clear.result
    );

    let _ = std::fs::remove_file(db_path);
}
