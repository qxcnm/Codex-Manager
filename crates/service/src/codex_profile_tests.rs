use super::*;
use codexmanager_core::storage::{Account, AggregateApi, ApiKey, ManagedModelV2Upsert, Storage};
use rusqlite::Connection;

#[tokio::test(flavor = "current_thread")]
async fn cancelled_profile_apply_preserves_in_progress_commit_and_mutation_lease() {
    let profile_dir = temp_profile("cancelled-profile-commit");
    fs::create_dir_all(&profile_dir).unwrap();
    let commit_path = profile_dir.join(CONFIG_FILE);
    let written_path = commit_path.clone();
    let (started, entered) = tokio::sync::oneshot::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let task = tokio::spawn(async move {
        let lease = profile_mutation_lease().await;
        profile_commit(&lease, move || {
            started.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(5)).unwrap();
            write_atomic(&written_path, "model_provider = \"cm\"\n")
        })
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .unwrap()
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(PROFILE_MUTATION_LOCK.get().unwrap().try_lock().is_err());
    // The independent runtime task remains schedulable while the file worker
    // is blocked, and cancellation cannot let a second profile writer race it.
    tokio::task::yield_now().await;
    release.send(()).unwrap();
    let _next = tokio::time::timeout(Duration::from_secs(2), profile_mutation_lease())
        .await
        .expect("commit releases its lease after the file is complete");
    assert_eq!(
        fs::read_to_string(commit_path).unwrap(),
        "model_provider = \"cm\"\n"
    );
    fs::remove_dir_all(profile_dir).unwrap();
}

fn temp_profile(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("codexmanager-{name}-{unique}"))
}

fn cleanup_profile(dir: &Path) {
    if let Ok(root) = managed_profile_root(dir) {
        let _ = fs::remove_dir_all(root);
    }
    let _ = fs::remove_dir_all(dir);
}

fn insert_custom_model_clone(storage: &Storage, source_slug: &str, slug: &str) {
    let mut model = storage
        .get_managed_model_v2(source_slug)
        .expect("read source model")
        .expect("seeded source model");
    model.id.clear();
    model.slug = slug.to_string();
    model.display_name = format!("Custom {slug}");
    model.origin = "custom".to_string();
    model.builtin_revision = None;
    model.permission_group_ids.clear();
    for route in &mut model.routes {
        route.id.clear();
    }
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: None,
            model,
        })
        .expect("insert custom model clone");
}

fn provider_http_header<'a>(provider: &'a Table, name: &str) -> Option<&'a str> {
    let headers = provider.get("http_headers")?;
    if let Some(table) = headers.as_table() {
        return table.get(name).and_then(Item::as_str);
    }
    headers
        .as_value()
        .and_then(Value::as_inline_table)
        .and_then(|inline| inline.get(name))
        .and_then(Value::as_str)
}

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn remove(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn set_test_db(dir: &Path) -> EnvGuard {
    fs::create_dir_all(dir).expect("create test db dir");
    let db_path = dir.join("codexmanager.db");
    let storage = Storage::open(&db_path).expect("open test storage");
    storage.init().expect("init test storage");
    drop(storage);
    EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref())
}

#[test]
fn managed_model_selection_changes_rename_remove_and_deduplicate() {
    let mut slugs = vec![
        "old-model".to_string(),
        "keep-model".to_string(),
        "NEW-MODEL".to_string(),
    ];
    let changed = apply_managed_model_selection_changes(
        &mut slugs,
        &[
            ManagedModelSelectionChange::rename("old-model", "new-model"),
            ManagedModelSelectionChange::remove("keep-model"),
        ],
    );

    assert!(changed);
    assert_eq!(slugs, vec!["new-model"]);
}

fn test_account(id: &str, status: &str) -> Account {
    Account {
        id: id.to_string(),
        label: format!("Label {id}"),
        issuer: format!("issuer-{id}"),
        chatgpt_account_id: Some(format!("cgpt-{id}")),
        workspace_id: Some(format!("ws-{id}")),
        group_name: Some("test-group".to_string()),
        sort: 0,
        status: status.to_string(),
        created_at: now_ts(),
        updated_at: now_ts(),
    }
}

fn test_aggregate_api(id: &str, provider_type: &str, status: &str) -> AggregateApi {
    let now = now_ts();
    AggregateApi {
        id: id.to_string(),
        provider_type: provider_type.to_string(),
        supplier_name: Some(format!("Supplier {id}")),
        sort: 0,
        url: "https://aggregate.example.test".to_string(),
        auth_type: crate::aggregate_api::AGGREGATE_API_AUTH_APIKEY.to_string(),
        auth_params_json: None,
        action: None,
        model_override: None,
        user_agent: None,
        status: status.to_string(),
        created_at: now,
        updated_at: now,
        last_test_at: None,
        last_test_status: None,
        last_test_error: None,
        balance_query_enabled: false,
        balance_query_template: None,
        balance_query_base_url: None,
        balance_query_user_id: None,
        balance_query_config_json: None,
        last_balance_at: None,
        last_balance_status: None,
        last_balance_error: None,
        last_balance_json: None,
    }
}

fn test_token(account_id: &str, access_token: &str, refresh_token: &str) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: "id-token".to_string(),
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
        api_key_access_token: None,
        last_refresh: 123,
    }
}

fn write_test_rollout(dir: &Path, thread_id: &str, provider: &str) -> (PathBuf, String) {
    let rollout_dir = dir.join("sessions").join("2026").join("06").join("06");
    fs::create_dir_all(&rollout_dir).expect("mkdir rollout");
    let path = rollout_dir.join(format!("rollout-2026-06-06T00-00-00-{thread_id}.jsonl"));
    let event_line = r#"{"timestamp":"2026-06-06T00:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"keep me"}}"#.to_string();
    let content = format!(
        "{{\"timestamp\":\"2026-06-06T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\",\"model_provider\":\"{provider}\",\"cwd\":\"/tmp\"}}}}\n{event_line}\n"
    );
    fs::write(&path, content).expect("write rollout");
    (path, event_line)
}

fn create_state_db(dir: &Path, thread_id: &str, provider: &str) {
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            model_provider TEXT,
            title TEXT,
            updated_at INTEGER,
            updated_at_ms INTEGER
        )",
        [],
    )
    .expect("create threads");
    conn.execute(
        "INSERT INTO threads (id, model_provider, title, updated_at, updated_at_ms)
         VALUES (?1, ?2, 'Thread title', 1770000000, 1770000000000)",
        params![thread_id, provider],
    )
    .expect("insert thread");
}

fn sqlite_provider(dir: &Path, thread_id: &str) -> String {
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.query_row(
        "SELECT model_provider FROM threads WHERE id = ?1",
        params![thread_id],
        |row| row.get::<_, String>(0),
    )
    .expect("read provider")
}

#[test]
fn direct_config_uses_openai_and_keeps_secret_free_legacy_provider() {
    let input = r#"
model_provider = "cm"
model = "gpt-5.4"

[model_providers.cm]
name = "CodexManager"
base_url = "http://localhost:48760/v1"
wire_api = "responses"
experimental_bearer_token = "must-be-removed"
custom_header = "must-also-be-removed"

[model_providers.other]
name = "Other"
base_url = "https://example.test/v1"
"#;

    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let output = patch_config_for_direct(Some(input.to_string()), &managed_catalog, None)
        .expect("patch direct");

    assert!(output.contains("model_provider = \"openai\""));
    assert!(output.contains("[model_providers.cm]"));
    assert!(output.contains("wire_api = \"responses\""));
    assert!(output.contains("requires_openai_auth = true"));
    assert!(!output.contains("base_url = \"http://localhost:48760/v1\""));
    assert!(!output.contains("experimental_bearer_token"));
    assert!(!output.contains("custom_header"));
    assert!(output.contains("[model_providers.other]"));
    assert!(output.contains("model = \"gpt-5.4\""));
}

#[test]
fn gateway_config_sets_managed_provider_and_preserves_other_values() {
    let input = r#"
model = "gpt-5.4"

[model_providers.other]
name = "Other"
"#;

    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let output = patch_config_for_gateway(
        Some(input.to_string()),
        "http://127.0.0.1:48770/v1",
        &managed_catalog,
        true,
        "cm-managed-key",
        true,
    )
    .expect("patch gateway");

    assert!(output.contains("model_provider = \"cm\""));
    assert!(output.contains("[model_providers.cm]"));
    assert!(output.contains("name = \"CodexManager\""));
    assert!(output.contains("base_url = \"http://127.0.0.1:48770/v1\""));
    assert!(output.contains("wire_api = \"responses\""));
    assert!(!output.contains("requires_openai_auth"));
    assert!(output.contains("supports_websockets = true"));
    assert!(output.contains("experimental_bearer_token = \"cm-managed-key\""));
    assert!(!output.contains("[model_providers.cm.auth]"));
    assert!(output.contains("model_catalog_json = \"/tmp/codexmanager/gateway-models.json\""));
    assert!(output.contains("[model_providers.other]"));
    let doc = parse_config(&output).expect("parse gateway config");
    let provider = doc
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(PROVIDER_ID))
        .and_then(Item::as_table)
        .expect("managed provider");
    assert_eq!(
        provider_http_header(
            provider,
            crate::gateway::X_OPENAI_ACTOR_AUTHORIZATION_HEADER
        ),
        Some(crate::gateway::CODEXMANAGER_IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
    );
    assert_eq!(
        provider
            .get("experimental_bearer_token")
            .and_then(Item::as_str),
        Some("cm-managed-key")
    );

    let without_websocket = patch_config_for_gateway(
        Some(output),
        "http://127.0.0.1:48770/v1",
        &managed_catalog,
        false,
        "cm-managed-key",
        true,
    )
    .expect("disable gateway websocket");
    assert!(without_websocket.contains("supports_websockets = false"));
}

#[tokio::test(flavor = "current_thread")]
async fn reapplying_gateway_profile_refreshes_catalog_and_requests_runtime_reload() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("reapply-gateway-models");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    let managed_root = dir.join("managed");
    let _managed_root_guard = EnvGuard::set(
        "CODEXMANAGER_TEST_DB_DIR",
        managed_root.to_string_lossy().as_ref(),
    );
    let storage = Storage::open(dir.join("codexmanager.db")).expect("open test storage");
    storage.init().expect("init test storage");
    let api_key = ApiKey {
        id: "key-reapply-models".to_string(),
        name: Some("Reapply models".to_string()),
        model_slug: None,
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: crate::apikey_profile::ROTATION_HYBRID.to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: crate::apikey_profile::CLIENT_CODEX.to_string(),
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT.to_string(),
        auth_scheme: crate::apikey_profile::AUTH_BEARER.to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: "reapply-models-hash".to_string(),
        status: "active".to_string(),
        created_at: now_ts(),
        last_used_at: None,
    };
    storage
        .insert_api_key(&api_key)
        .expect("insert platform key");
    storage
        .upsert_api_key_secret(&api_key.id, "cm-reapply-secret")
        .expect("insert platform key secret");

    let codex_home = dir.to_string_lossy().to_string();
    let first = apply_gateway_async(
        Some(&api_key.id),
        Some(&codex_home),
        Some("http://127.0.0.1:48760"),
        Some(false),
        false,
    )
    .await
    .expect("apply gateway profile");
    assert_eq!(
        first.selected_api_key_id.as_deref(),
        Some(api_key.id.as_str())
    );
    let applied_models = apply_models_async(Some(&codex_home), vec!["gpt-6-sol".to_string()], true)
        .await
        .expect("apply selected gateway model");
    assert!(applied_models
        .runtime_reload
        .as_ref()
        .is_some_and(|reload| !reload.requested));
    assert_eq!(
        load_state()
            .expect("state after applying models")
            .managed_model_slugs,
        vec!["gpt-6-sol"]
    );

    let paths = managed_profile_paths(&dir).expect("managed profile paths");
    let mut model = storage
        .get_managed_model_v2("gpt-6-sol")
        .expect("read managed model")
        .expect("seeded managed model");
    model.display_name = "GPT Reapplied".to_string();
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some(model.slug.clone()),
            model,
        })
        .expect("update managed model");
    fs::write(&paths.gateway_model_catalog_path, "stale catalog")
        .expect("replace catalog with stale content");

    let reapplied = apply_gateway_async(
        Some(&api_key.id),
        Some(&codex_home),
        first.gateway_base_url.as_deref(),
        Some(first.supports_websockets),
        true,
    )
    .await
    .expect("reapply gateway profile");

    let catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&paths.gateway_model_catalog_path).expect("read refreshed catalog"),
    )
    .expect("parse refreshed catalog");
    assert!(catalog["models"].as_array().is_some_and(|models| {
        models
            .iter()
            .any(|model| model["slug"] == "gpt-6-sol" && model["display_name"] == "GPT Reapplied")
    }));

    let config =
        parse_config(&fs::read_to_string(dir.join(CONFIG_FILE)).expect("read refreshed config"))
            .expect("parse refreshed config");
    assert_eq!(
        config.get("model_catalog_json").and_then(Item::as_str),
        Some(paths.gateway_model_catalog_path.to_string_lossy().as_ref())
    );
    assert!(reapplied
        .runtime_reload
        .as_ref()
        .is_some_and(|reload| reload.requested));
    assert!(load_state()
        .expect("state after reapplying gateway")
        .managed_model_slugs
        .is_empty());
    assert!(read_marker(&paths.marker_path)
        .expect("marker after reapplying gateway")
        .managed_model_slugs
        .is_empty());

    let stale_gateway_slug = "stale-custom-gateway";
    insert_custom_model_clone(&storage, "gpt-5.6-sol", stale_gateway_slug);
    apply_models_async(
        Some(&codex_home),
        vec![stale_gateway_slug.to_string()],
        false,
    )
    .await
    .expect("apply model before stale selection recovery");
    storage
        .delete_managed_model_v2(stale_gateway_slug)
        .expect("delete selected model without updating profile state");
    assert!(sync_active_gateway_profile_from_storage_async(&storage)
        .await
        .expect("reconcile stale gateway model selection"));
    assert!(load_state()
        .expect("state after stale gateway selection recovery")
        .managed_model_slugs
        .is_empty());
    assert!(read_marker(&paths.marker_path)
        .expect("marker after stale gateway selection recovery")
        .managed_model_slugs
        .is_empty());
    let recovered_catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&paths.gateway_model_catalog_path)
            .expect("read fallback gateway catalog"),
    )
    .expect("parse fallback gateway catalog");
    assert!(recovered_catalog["models"]
        .as_array()
        .is_some_and(|models| !models.is_empty()
            && models
                .iter()
                .all(|model| model["slug"] != stale_gateway_slug)));
    let recovered_config =
        parse_config(&fs::read_to_string(dir.join(CONFIG_FILE)).expect("read recovered config"))
            .expect("parse recovered config");
    assert_eq!(
        recovered_config
            .get("model_catalog_json")
            .and_then(Item::as_str),
        Some(paths.gateway_model_catalog_path.to_string_lossy().as_ref())
    );

    cleanup_profile(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn service_startup_reconciles_stale_gateway_model_selection() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("startup-stale-gateway-model");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    let managed_root = dir.join("managed");
    let _managed_root_guard = EnvGuard::set(
        "CODEXMANAGER_TEST_DB_DIR",
        managed_root.to_string_lossy().as_ref(),
    );
    let storage = Storage::open(dir.join("codexmanager.db")).expect("open test storage");
    storage.init().expect("init test storage");
    let api_key = ApiKey {
        id: "key-startup-stale-model".to_string(),
        name: Some("Startup stale model".to_string()),
        model_slug: None,
        reasoning_effort: None,
        service_tier: None,
        rotation_strategy: crate::apikey_profile::ROTATION_HYBRID.to_string(),
        aggregate_api_id: None,
        account_plan_filter: None,
        aggregate_api_url: None,
        client_type: crate::apikey_profile::CLIENT_CODEX.to_string(),
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT.to_string(),
        auth_scheme: crate::apikey_profile::AUTH_BEARER.to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: "startup-stale-model-hash".to_string(),
        status: "active".to_string(),
        created_at: now_ts(),
        last_used_at: None,
    };
    storage
        .insert_api_key(&api_key)
        .expect("insert platform key");
    storage
        .upsert_api_key_secret(&api_key.id, "cm-startup-stale-secret")
        .expect("insert platform key secret");

    let codex_home = dir.to_string_lossy().to_string();
    apply_gateway(
        Some(&api_key.id),
        Some(&codex_home),
        Some("http://127.0.0.1:48760"),
        Some(false),
        false,
    )
    .expect("apply gateway profile");
    let stale_slug = "stale-startup-model";
    insert_custom_model_clone(&storage, "gpt-6-sol", stale_slug);
    apply_models(Some(&codex_home), vec![stale_slug.to_string()], false)
        .expect("apply model before startup reconciliation");
    storage
        .delete_managed_model_v2(stale_slug)
        .expect("delete selected model without updating profile state");
    assert_eq!(
        load_state()
            .expect("state before startup reconciliation")
            .managed_model_slugs,
        vec![stale_slug]
    );

    crate::lifecycle::startup::reconcile_active_gateway_profile_after_startup_async().await;

    let paths = managed_profile_paths(&dir).expect("managed profile paths");
    assert!(load_state()
        .expect("state after startup reconciliation")
        .managed_model_slugs
        .is_empty());
    assert!(read_marker(&paths.marker_path)
        .expect("marker after startup reconciliation")
        .managed_model_slugs
        .is_empty());
    let catalog_content = fs::read_to_string(&paths.gateway_model_catalog_path)
        .expect("read reconciled gateway catalog");
    let catalog: serde_json::Value =
        serde_json::from_str(&catalog_content).expect("parse reconciled gateway catalog");
    assert!(catalog["models"].as_array().is_some_and(|models| {
        !models.is_empty() && models.iter().all(|model| model["slug"] != stale_slug)
    }));

    cleanup_profile(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn applying_models_without_an_api_key_preserves_existing_profile_configuration() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("apply-models-without-api-key");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    let managed_root = dir.join("managed");
    let _managed_root_guard = EnvGuard::set(
        "CODEXMANAGER_TEST_DB_DIR",
        managed_root.to_string_lossy().as_ref(),
    );
    let storage = Storage::open(dir.join("codexmanager.db")).expect("open test storage");
    storage.init().expect("init test storage");
    let mut selected_model = storage
        .get_managed_model_v2("gpt-6-sol")
        .expect("read selected model")
        .expect("seeded selected model");
    selected_model.enabled = false;
    selected_model.supported_in_api = false;
    selected_model.visibility = "hide".to_string();
    selected_model
        .capabilities
        .as_object_mut()
        .expect("selected model capabilities")
        .insert(
            "supports_text_generation".to_string(),
            serde_json::json!(false),
        );
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some(selected_model.slug.clone()),
            model: selected_model,
        })
        .expect("make selected model hidden and unavailable");
    fs::create_dir_all(&dir).expect("create profile");
    let auth_json = r#"{"auth_mode":"apikey","OPENAI_API_KEY":"external-secret"}
"#;
    fs::write(dir.join(AUTH_FILE), auth_json).expect("write existing auth");
    let config_toml = r#"model_provider = "external"
base_url = "https://external.example.test/v1"
custom_setting = true

[model_providers.external]
name = "External Provider"
base_url = "https://external.example.test/v1"
wire_api = "responses"
experimental_bearer_token = "external-provider-secret"
"#;
    fs::write(dir.join(CONFIG_FILE), "model_provider = [invalid").expect("write malformed config");
    assert!(list_candidates()
        .expect("list candidates")
        .api_keys
        .is_empty());

    let paths = managed_profile_paths(&dir).expect("managed profile paths");
    fs::create_dir_all(
        paths
            .gateway_model_catalog_path
            .parent()
            .expect("catalog parent"),
    )
    .expect("create catalog parent");
    fs::write(&paths.gateway_model_catalog_path, "stale catalog").expect("write stale catalog");
    let error = apply_models_async(
        Some(dir.to_string_lossy().as_ref()),
        vec![" GPT-6-SOL ".to_string(), "gpt-6-sol".to_string()],
        false,
    )
    .await
    .expect_err("malformed config must reject model apply");
    assert!(error.contains("parse config.toml failed"), "{error}");
    assert_eq!(
        fs::read_to_string(&paths.gateway_model_catalog_path).expect("read unchanged catalog"),
        "stale catalog"
    );

    fs::write(dir.join(CONFIG_FILE), config_toml).expect("write existing config");

    let status = apply_models_async(
        Some(dir.to_string_lossy().as_ref()),
        vec![" GPT-6-SOL ".to_string(), "gpt-6-sol".to_string()],
        true,
    )
    .await
    .expect("apply managed model catalog");

    assert!(status.managed_catalog_active);
    assert_eq!(status.selected_api_key_id, None);
    assert!(status
        .runtime_reload
        .as_ref()
        .is_some_and(|reload| !reload.requested));
    assert_eq!(
        fs::read_to_string(dir.join(AUTH_FILE)).expect("read preserved auth"),
        auth_json
    );

    let written_config = fs::read_to_string(dir.join(CONFIG_FILE)).expect("read updated config");
    let config = parse_config(&written_config).expect("parse updated config");
    assert_eq!(
        config.get("model_provider").and_then(Item::as_str),
        Some("external")
    );
    assert_eq!(
        config.get("base_url").and_then(Item::as_str),
        Some("https://external.example.test/v1")
    );
    assert_eq!(
        config.get("custom_setting").and_then(Item::as_bool),
        Some(true)
    );
    assert_eq!(
        config.get("model_catalog_json").and_then(Item::as_str),
        Some(paths.gateway_model_catalog_path.to_string_lossy().as_ref())
    );
    let external_provider = config
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get("external"))
        .and_then(Item::as_table)
        .expect("external provider");
    assert_eq!(
        external_provider.get("base_url").and_then(Item::as_str),
        Some("https://external.example.test/v1")
    );
    assert_eq!(
        external_provider
            .get("experimental_bearer_token")
            .and_then(Item::as_str),
        Some("external-provider-secret")
    );
    assert!(config
        .get("model_providers")
        .and_then(Item::as_table)
        .is_some_and(|providers| !providers.contains_key(PROVIDER_ID)));

    let catalog_content =
        fs::read_to_string(&paths.gateway_model_catalog_path).expect("read managed catalog");
    let catalog: serde_json::Value =
        serde_json::from_str(&catalog_content).expect("parse managed catalog");
    let models = catalog["models"].as_array().expect("models array");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["slug"], "gpt-6-sol");
    assert_eq!(models[0]["visibility"], "list");
    assert_eq!(models[0]["supported_in_api"], true);
    assert_eq!(
        storage
            .get_managed_model_v2("gpt-6-sol")
            .expect("read persisted selected model")
            .expect("persisted selected model")
            .supported_in_api,
        false,
        "Codex picker compatibility must not change the stored gateway/API flag"
    );
    let state = load_state().expect("managed state");
    assert_eq!(state.managed_model_slugs, vec!["gpt-6-sol"]);
    let marker = read_marker(&paths.marker_path).expect("managed marker");
    assert_eq!(marker.managed_model_slugs, vec!["gpt-6-sol"]);

    let mut refreshed_model = storage
        .get_managed_model_v2("gpt-6-sol")
        .expect("read selected model for refresh")
        .expect("selected model for refresh");
    refreshed_model.display_name = "Selected model refreshed".to_string();
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some(refreshed_model.slug.clone()),
            model: refreshed_model,
        })
        .expect("refresh selected model");
    let mut unselected_model = storage
        .get_managed_model_v2("gpt-5.6-sol")
        .expect("read unselected model")
        .expect("seeded unselected model");
    unselected_model.display_name = "Unselected model refreshed".to_string();
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some(unselected_model.slug.clone()),
            model: unselected_model,
        })
        .expect("refresh unselected model");
    assert!(sync_active_gateway_profile_from_storage_async(&storage)
        .await
        .expect("sync selected managed models"));
    let refreshed_catalog: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&paths.gateway_model_catalog_path).expect("read refreshed catalog"),
    )
    .expect("parse refreshed catalog");
    let refreshed_models = refreshed_catalog["models"]
        .as_array()
        .expect("refreshed models array");
    assert_eq!(refreshed_models.len(), 1);
    assert_eq!(refreshed_models[0]["slug"], "gpt-6-sol");
    assert_eq!(
        refreshed_models[0]["display_name"],
        "Selected model refreshed"
    );
    fs::remove_file(&paths.gateway_model_catalog_path).expect("remove managed catalog");
    assert!(
        !status_for_profile(&dir)
            .expect("status without catalog")
            .managed_catalog_active
    );
    fs::write(&paths.gateway_model_catalog_path, "{}").expect("write catalog without models");
    assert!(
        !status_for_profile(&dir)
            .expect("status with invalid catalog")
            .managed_catalog_active
    );
    fs::write(&paths.gateway_model_catalog_path, catalog_content).expect("restore managed catalog");
    assert!(
        status_for_profile(&dir)
            .expect("status with valid catalog")
            .managed_catalog_active
    );
    assert!(list_candidates()
        .expect("list candidates")
        .api_keys
        .is_empty());

    assert!(sync_active_gateway_profile_from_storage_with_changes_async(
        &storage,
        vec![ManagedModelSelectionChange::remove("gpt-6-sol")],
    )
    .await
    .expect("remove the last applied model"));
    assert!(load_state()
        .expect("state after removing applied model")
        .managed_model_slugs
        .is_empty());
    assert!(read_marker(&paths.marker_path)
        .expect("marker after removing applied model")
        .managed_model_slugs
        .is_empty());
    let restored_config =
        parse_config(&fs::read_to_string(dir.join(CONFIG_FILE)).expect("read restored config"))
            .expect("parse restored config");
    assert!(restored_config.get("model_catalog_json").is_none());
    assert!(
        !status_for_profile(&dir)
            .expect("status after removing applied model")
            .managed_catalog_active
    );

    let stale_external_slug = "stale-custom-external";
    insert_custom_model_clone(&storage, "gpt-6-sol", stale_external_slug);
    apply_models_async(
        Some(dir.to_string_lossy().as_ref()),
        vec![stale_external_slug.to_string()],
        false,
    )
    .await
    .expect("reapply model before stale selection recovery");
    storage
        .delete_managed_model_v2(stale_external_slug)
        .expect("delete selected model without updating profile state");
    assert!(sync_active_gateway_profile_from_storage_async(&storage)
        .await
        .expect("reconcile stale external model selection"));
    assert!(load_state()
        .expect("state after stale external selection recovery")
        .managed_model_slugs
        .is_empty());
    assert!(read_marker(&paths.marker_path)
        .expect("marker after stale external selection recovery")
        .managed_model_slugs
        .is_empty());
    let recovered_config =
        parse_config(&fs::read_to_string(dir.join(CONFIG_FILE)).expect("read recovered config"))
            .expect("parse recovered config");
    assert!(recovered_config.get("model_catalog_json").is_none());
    assert!(
        !status_for_profile(&dir)
            .expect("status after stale external selection recovery")
            .managed_catalog_active
    );

    cleanup_profile(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn empty_or_unknown_model_selection_does_not_mutate_profile() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("reject-invalid-model-selection");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    let managed_root = dir.join("managed");
    let _managed_root_guard = EnvGuard::set(
        "CODEXMANAGER_TEST_DB_DIR",
        managed_root.to_string_lossy().as_ref(),
    );
    fs::create_dir_all(&dir).expect("create profile");
    let auth_json = "{\"auth_mode\":\"apikey\",\"OPENAI_API_KEY\":\"keep-me\"}\n";
    let config_toml = "model_provider = \"external\"\ncustom_setting = true\n";
    fs::write(dir.join(AUTH_FILE), auth_json).expect("write auth");
    fs::write(dir.join(CONFIG_FILE), config_toml).expect("write config");
    let paths = managed_profile_paths(&dir).expect("managed profile paths");
    fs::create_dir_all(
        paths
            .gateway_model_catalog_path
            .parent()
            .expect("catalog parent"),
    )
    .expect("create catalog parent");
    fs::write(&paths.gateway_model_catalog_path, "existing catalog")
        .expect("write existing catalog");

    let empty_error = apply_models_async(Some(dir.to_string_lossy().as_ref()), Vec::new(), true)
        .await
        .expect_err("empty selection must fail");
    assert!(empty_error.contains("no models selected"), "{empty_error}");
    let unknown_error = apply_models_async(
        Some(dir.to_string_lossy().as_ref()),
        vec!["does-not-exist".to_string()],
        true,
    )
    .await
    .expect_err("unknown selection must fail");
    assert!(
        unknown_error.contains("unknown managed model slug(s): does-not-exist"),
        "{unknown_error}"
    );

    assert_eq!(
        fs::read_to_string(dir.join(AUTH_FILE)).expect("read unchanged auth"),
        auth_json
    );
    assert_eq!(
        fs::read_to_string(dir.join(CONFIG_FILE)).expect("read unchanged config"),
        config_toml
    );
    assert_eq!(
        fs::read_to_string(&paths.gateway_model_catalog_path).expect("read unchanged catalog"),
        "existing catalog"
    );
    assert!(!paths.marker_path.exists());
    assert!(load_state().is_none());
    assert!(!load_backups().contains_key(&profile_key(&dir)));

    cleanup_profile(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn apply_models_commit_failures_restore_all_files_and_settings() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("apply-models-rollback");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    let managed_root = dir.join("managed");
    let _managed_root_guard = EnvGuard::set(
        "CODEXMANAGER_TEST_DB_DIR",
        managed_root.to_string_lossy().as_ref(),
    );
    fs::create_dir_all(&dir).expect("create profile");

    let config_toml = r#"model_provider = "external"
model_catalog_json = "C:/previous-models.json"
custom_setting = true
"#;
    fs::write(dir.join(CONFIG_FILE), config_toml).expect("write baseline config");
    let paths = managed_profile_paths(&dir).expect("managed profile paths");
    fs::create_dir_all(&paths.root).expect("create managed profile root");
    let catalog_content = r#"{"models":[{"slug":"previous-model"}]}"#;
    fs::write(&paths.gateway_model_catalog_path, catalog_content).expect("write baseline catalog");

    let baseline_state = ManagedState {
        profile_dir: profile_key(&dir),
        mode: CodexProfileMode::Unmanaged,
        account_id: None,
        api_key_id: None,
        aggregate_api_id: None,
        gateway_base_url: None,
        aggregate_api_base_url: None,
        supports_websockets: None,
        provider_id: "external".to_string(),
        previous_model_catalog_json: Some("C:/previous-models.json".to_string()),
        managed_model_slugs: vec!["previous-model".to_string()],
        updated_at: 123,
    };
    write_managed_state(&dir, &baseline_state).expect("write baseline managed state");
    let marker_content = fs::read_to_string(&paths.marker_path).expect("read baseline marker");
    fs::write(&paths.legacy_marker_path, &marker_content).expect("write baseline legacy marker");

    let baseline_backups = HashMap::from([(
        "C:/previous/.codex".to_string(),
        BackupEntry {
            profile_dir: "C:/previous/.codex".to_string(),
            auth_json: None,
            config_toml: Some("model_provider = \"previous\"\n".to_string()),
            created_at: 10,
            updated_at: 20,
        },
    )]);
    save_backups(&baseline_backups).expect("write baseline backups");
    crate::app_settings::save_persisted_app_setting(
        APP_SETTING_CODEX_HOME_KEY,
        Some("C:/previous/.codex"),
    )
    .expect("write baseline codex home");

    let baseline_state_setting =
        crate::app_settings::get_persisted_app_setting(APP_SETTING_STATE_KEY);
    let baseline_backups_setting =
        crate::app_settings::get_persisted_app_setting(APP_SETTING_BACKUPS_KEY);
    let baseline_codex_home_setting =
        crate::app_settings::get_persisted_app_setting(APP_SETTING_CODEX_HOME_KEY);

    for stage in [
        "after_backup",
        "after_catalog",
        "after_config",
        "after_marker",
        "after_state",
        "after_codex_home",
    ] {
        let failure_guard = EnvGuard::set("CODEXMANAGER_TEST_APPLY_MODELS_FAIL_AFTER", stage);
        let error = apply_models_async(
            Some(dir.to_string_lossy().as_ref()),
            vec!["gpt-6-sol".to_string()],
            false,
        )
        .await
        .expect_err("injected commit failure must reject model apply");
        drop(failure_guard);
        assert!(error.contains(stage), "{error}");

        assert_eq!(
            fs::read_to_string(dir.join(CONFIG_FILE)).expect("read restored config"),
            config_toml,
            "config changed after failure at {stage}"
        );
        assert_eq!(
            fs::read_to_string(&paths.gateway_model_catalog_path).expect("read restored catalog"),
            catalog_content,
            "catalog changed after failure at {stage}"
        );
        assert_eq!(
            fs::read_to_string(&paths.marker_path).expect("read restored marker"),
            marker_content,
            "marker changed after failure at {stage}"
        );
        assert_eq!(
            fs::read_to_string(&paths.legacy_marker_path).expect("read restored legacy marker"),
            marker_content,
            "legacy marker changed after failure at {stage}"
        );
        assert_eq!(
            crate::app_settings::get_persisted_app_setting(APP_SETTING_STATE_KEY),
            baseline_state_setting,
            "state setting changed after failure at {stage}"
        );
        assert_eq!(
            crate::app_settings::get_persisted_app_setting(APP_SETTING_BACKUPS_KEY),
            baseline_backups_setting,
            "backup setting changed after failure at {stage}"
        );
        assert_eq!(
            crate::app_settings::get_persisted_app_setting(APP_SETTING_CODEX_HOME_KEY),
            baseline_codex_home_setting,
            "Codex home setting changed after failure at {stage}"
        );
        assert!(!load_backups().contains_key(&profile_key(&dir)));
    }

    cleanup_profile(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn first_apply_models_failure_removes_new_managed_state() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("apply-models-first-write-rollback");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    let managed_root = dir.join("managed");
    let _managed_root_guard = EnvGuard::set(
        "CODEXMANAGER_TEST_DB_DIR",
        managed_root.to_string_lossy().as_ref(),
    );
    fs::create_dir_all(&dir).expect("create profile");
    let config_toml = "model_provider = \"external\"\n";
    fs::write(dir.join(CONFIG_FILE), config_toml).expect("write baseline config");
    let paths = managed_profile_paths(&dir).expect("managed profile paths");
    assert!(!paths.root.exists());

    let failure_guard = EnvGuard::set("CODEXMANAGER_TEST_APPLY_MODELS_FAIL_AFTER", "after_state");
    let error = apply_models_async(
        Some(dir.to_string_lossy().as_ref()),
        vec!["gpt-6-sol".to_string()],
        false,
    )
    .await
    .expect_err("injected first apply failure must reject model apply");
    drop(failure_guard);
    assert!(error.contains("after_state"), "{error}");

    assert_eq!(
        fs::read_to_string(dir.join(CONFIG_FILE)).expect("read restored config"),
        config_toml
    );
    assert!(!paths.gateway_model_catalog_path.exists());
    assert!(!paths.marker_path.exists());
    assert!(!paths.legacy_marker_path.exists());
    assert!(!paths.root.exists());
    assert!(load_state().is_none());
    assert!(!load_backups().contains_key(&profile_key(&dir)));
    assert!(crate::app_settings::get_persisted_app_setting(APP_SETTING_CODEX_HOME_KEY).is_none());

    cleanup_profile(&dir);
}

#[test]
fn gateway_config_preserves_requires_openai_auth_when_removal_is_disabled() {
    let input = r#"
model_provider = "cm"

[model_providers.cm]
name = "Custom Gateway"
requires_openai_auth = true
"#;

    let output = patch_config_for_gateway(
        Some(input.to_string()),
        "http://127.0.0.1:48770/v1",
        Path::new("/tmp/gateway-models.json"),
        false,
        "cm-managed-key",
        false,
    )
    .expect("patch gateway without removing auth requirement");
    let doc = parse_config(&output).expect("parse patched gateway config");
    let provider = doc
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(PROVIDER_ID))
        .and_then(Item::as_table)
        .expect("managed provider");

    assert_eq!(
        provider.get("requires_openai_auth").and_then(Item::as_bool),
        Some(true)
    );
}

#[test]
fn gateway_config_preserves_custom_managed_provider_values() {
    let input = r#"
model_provider = "cm"

[model_providers.cm]
name = "Custom Gateway"
base_url = "https://stale.example.test/v1"
wire_api = "chat"
requires_openai_auth = false
supports_websockets = false
experimental_bearer_token = "custom-key"
custom_header = "custom-value"
http_headers = { "x-existing-header" = "keep", "x-openai-actor-authorization" = "stale" }
"#;

    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let output = patch_config_for_gateway(
        Some(input.to_string()),
        "http://127.0.0.1:48770/v1",
        &managed_catalog,
        true,
        "cm-managed-key",
        true,
    )
    .expect("patch gateway");
    let doc = parse_config(&output).expect("parse patched gateway config");
    let provider = doc
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(PROVIDER_ID))
        .and_then(Item::as_table)
        .expect("managed provider");

    assert_eq!(
        provider.get("name").and_then(Item::as_str),
        Some("Custom Gateway")
    );
    assert_eq!(
        provider
            .get("experimental_bearer_token")
            .and_then(Item::as_str),
        Some("cm-managed-key")
    );
    assert_eq!(
        provider.get("custom_header").and_then(Item::as_str),
        Some("custom-value")
    );
    assert_eq!(
        provider.get("base_url").and_then(Item::as_str),
        Some("http://127.0.0.1:48770/v1")
    );
    assert_eq!(
        provider.get("wire_api").and_then(Item::as_str),
        Some("responses")
    );
    assert!(provider.get("requires_openai_auth").is_none());
    assert!(provider.get("auth").is_none());
    assert_eq!(
        provider.get("supports_websockets").and_then(Item::as_bool),
        Some(true)
    );
    assert_eq!(
        provider_http_header(provider, "x-existing-header"),
        Some("keep")
    );
    assert_eq!(
        provider_http_header(
            provider,
            crate::gateway::X_OPENAI_ACTOR_AUTHORIZATION_HEADER
        ),
        Some(crate::gateway::CODEXMANAGER_IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
    );
}

#[test]
fn direct_config_only_removes_manager_owned_catalog() {
    let managed_catalog = PathBuf::from("/tmp/codexmanager/gateway-models.json");
    let managed = format!(
        "model_catalog_json = {:?}\n",
        managed_catalog.to_string_lossy()
    );

    let removed = patch_config_for_direct(Some(managed), &managed_catalog, None)
        .expect("remove managed catalog");
    assert!(!removed.contains("model_catalog_json"));

    let restored = patch_config_for_direct(
        Some(format!(
            "model_catalog_json = {:?}\n",
            managed_catalog.to_string_lossy()
        )),
        &managed_catalog,
        Some("/home/test/custom-models.json"),
    )
    .expect("restore prior catalog");
    assert!(restored.contains("model_catalog_json = \"/home/test/custom-models.json\""));

    let custom = "model_catalog_json = \"/home/test/owned-by-user.json\"\n";
    let preserved = patch_config_for_direct(Some(custom.to_string()), &managed_catalog, None)
        .expect("preserve user catalog");
    assert!(preserved.contains("/home/test/owned-by-user.json"));
}

#[test]
fn direct_mode_preserves_previous_catalog_from_config_and_state() {
    let dir = temp_profile("previous-direct-catalog");
    fs::create_dir_all(&dir).expect("create profile dir");
    let config = "model_catalog_json = \"C:/user-models.json\"\n";
    assert_eq!(
        previous_model_catalog_for_direct(&dir, Some(config)).expect("read config catalog"),
        Some("C:/user-models.json".to_string())
    );

    let state = ManagedState {
        profile_dir: profile_key(&dir),
        mode: CodexProfileMode::DirectAggregate,
        account_id: None,
        api_key_id: None,
        aggregate_api_id: Some("aggregate".to_string()),
        gateway_base_url: None,
        aggregate_api_base_url: Some("https://aggregate.example.test/v1".to_string()),
        supports_websockets: Some(false),
        provider_id: DIRECT_AGGREGATE_PROVIDER_ID.to_string(),
        previous_model_catalog_json: Some("C:/user-models.json".to_string()),
        managed_model_slugs: vec!["aggregate-model".to_string()],
        updated_at: now_ts(),
    };
    write_managed_state(&dir, &state).expect("write direct aggregate state");
    assert_eq!(
        previous_model_catalog_for_direct(&dir, Some("model_catalog_json = \"other.json\"\n"))
            .expect("read state catalog"),
        Some("C:/user-models.json".to_string())
    );
    cleanup_profile(&dir);
}

#[test]
fn invalid_toml_is_rejected() {
    assert!(patch_config_for_gateway(
        Some("bad = [".to_string()),
        "http://x/v1",
        Path::new("/tmp/gateway-models.json"),
        false,
        "cm-managed-key",
        false,
    )
    .is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn gateway_auth_cleanup_setting_covers_apply_reapply_and_startup_sync() {
    let _lock = crate::test_env_guard();
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    for setting in [None, Some(false), Some(true)] {
        for existing in [None, Some(false), Some(true)] {
            let dir = temp_profile("gateway-auth-setting-matrix");
            let _db_guard = set_test_db(&dir);
            let _managed_root_guard = EnvGuard::set(
                "CODEXMANAGER_TEST_DB_DIR",
                dir.join("managed").to_string_lossy().as_ref(),
            );
            let storage = Storage::open(dir.join("codexmanager.db")).unwrap();
            storage.init().unwrap();
            if let Some(enabled) = setting {
                storage
                    .set_app_setting(
                        crate::app_settings::APP_SETTING_CODEX_PROFILE_REMOVE_REQUIRES_OPENAI_AUTH_KEY,
                        if enabled { "1" } else { "0" },
                        now_ts(),
                    )
                    .unwrap();
            }
            assert_eq!(
                remove_requires_openai_auth_enabled(),
                setting.unwrap_or(true)
            );
            let key = ApiKey {
                id: "auth-setting-key".into(),
                name: None,
                model_slug: None,
                reasoning_effort: None,
                service_tier: None,
                rotation_strategy: crate::apikey_profile::ROTATION_HYBRID.into(),
                aggregate_api_id: None,
                account_plan_filter: None,
                aggregate_api_url: None,
                client_type: crate::apikey_profile::CLIENT_CODEX.into(),
                protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT.into(),
                auth_scheme: crate::apikey_profile::AUTH_BEARER.into(),
                upstream_base_url: None,
                static_headers_json: None,
                key_hash: "auth-setting-hash".into(),
                status: "active".into(),
                created_at: now_ts(),
                last_used_at: None,
            };
            storage.insert_api_key(&key).unwrap();
            storage
                .upsert_api_key_secret(&key.id, "cm-test-secret")
                .unwrap();
            let auth_field = existing
                .map(|value| format!("requires_openai_auth = {value}\n"))
                .unwrap_or_default();
            let input = format!(
                "model_provider = \"cm\"\n[model_providers.cm]\nname = \"Keep name\"\n{auth_field}\n[model_providers.other]\nrequires_openai_auth = true\nbase_url = \"https://other.example/v1\"\n"
            );
            let expected = if setting.unwrap_or(true) {
                None
            } else {
                existing
            };
            let check = || {
                let config =
                    parse_config(&fs::read_to_string(dir.join(CONFIG_FILE)).unwrap()).unwrap();
                let providers = config["model_providers"].as_table().unwrap();
                let provider = providers[PROVIDER_ID].as_table().unwrap();
                assert_eq!(
                    provider.get("requires_openai_auth").and_then(Item::as_bool),
                    expected
                );
                assert_eq!(provider["name"].as_str(), Some("Keep name"));
                assert_eq!(
                    provider["experimental_bearer_token"].as_str(),
                    Some("cm-test-secret")
                );
                assert_eq!(provider["wire_api"].as_str(), Some("responses"));
                assert_eq!(
                    provider_http_header(
                        provider,
                        crate::gateway::X_OPENAI_ACTOR_AUTHORIZATION_HEADER
                    ),
                    Some(crate::gateway::CODEXMANAGER_IMAGE_EXTENSION_ACTOR_AUTHORIZATION)
                );
                assert_eq!(
                    providers["other"]["requires_openai_auth"].as_bool(),
                    Some(true)
                );
                assert_eq!(
                    providers["other"]["base_url"].as_str(),
                    Some("https://other.example/v1")
                );
            };
            fs::write(dir.join(CONFIG_FILE), &input).unwrap();
            let profile_path = dir.to_string_lossy().to_string();
            for _ in 0..2 {
                apply_gateway_async(
                    Some(&key.id),
                    Some(&profile_path),
                    Some("http://127.0.0.1:48760"),
                    Some(false),
                    false,
                )
                .await
                .unwrap();
                check();
            }
            // Startup sync must read the setting again instead of inheriting an old patch result.
            fs::write(dir.join(CONFIG_FILE), &input).unwrap();
            assert!(sync_active_gateway_profile_from_storage_async(&storage)
                .await
                .unwrap());
            check();
            drop(storage);
            cleanup_profile(&dir);
        }
    }
}

#[test]
fn rotation_strategy_selects_catalog_ownership() {
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_ACCOUNT
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::OfficialAccountPool
    );
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_AGGREGATE_API
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::Managed
    );
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_HYBRID
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::Managed
    );
    assert_eq!(
        crate::codex_model_catalog::gateway_catalog_policy_for_rotation_strategy(
            crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST
        ),
        crate::codex_model_catalog::GatewayCatalogPolicy::Managed
    );
}

#[test]
fn api_key_candidates_expose_route_and_catalog_ownership() {
    for (rotation_strategy, catalog_source) in [
        (crate::apikey_profile::ROTATION_ACCOUNT, "official"),
        (crate::apikey_profile::ROTATION_AGGREGATE_API, "managed"),
        (crate::apikey_profile::ROTATION_HYBRID, "managed"),
        (
            crate::apikey_profile::ROTATION_HYBRID_AGGREGATE_FIRST,
            "managed",
        ),
    ] {
        let candidate = api_key_candidate(ApiKeyCodexProfileCandidate {
            id: format!("key-{rotation_strategy}"),
            name: Some("Platform key".to_string()),
            model_slug: None,
            reasoning_effort: None,
            rotation_strategy: rotation_strategy.to_string(),
            status: "active".to_string(),
        })
        .expect("active candidate");

        assert_eq!(candidate.rotation_strategy, rotation_strategy);
        assert_eq!(candidate.catalog_source, catalog_source);
    }
}

#[test]
fn usable_account_token_candidates_by_account_indexes_candidates() {
    let candidates = usable_account_token_candidates_by_account(vec![
        AccountTokenCandidate {
            account_id: "acc-ready".to_string(),
            has_access_token: true,
            has_refresh_token: true,
            last_refresh: 10,
        },
        AccountTokenCandidate {
            account_id: "acc-no-access".to_string(),
            has_access_token: false,
            has_refresh_token: true,
            last_refresh: 11,
        },
        AccountTokenCandidate {
            account_id: "acc-no-refresh".to_string(),
            has_access_token: true,
            has_refresh_token: false,
            last_refresh: 12,
        },
    ]);

    assert_eq!(candidates.len(), 3);
    assert_eq!(
        candidates
            .get("acc-ready")
            .map(|candidate| candidate.last_refresh),
        Some(10)
    );
    assert_eq!(
        candidates
            .get("acc-no-access")
            .map(|candidate| candidate.last_refresh),
        Some(11)
    );
    assert_eq!(
        candidates
            .get("acc-no-refresh")
            .map(|candidate| candidate.last_refresh),
        Some(12)
    );
}

#[test]
fn list_candidates_uses_active_account_projection_and_usable_tokens() {
    let _lock = crate::test_env_guard();
    let dir = temp_profile("codex-profile-candidates");
    fs::create_dir_all(&dir).expect("mkdir temp dir");
    let db_path = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    let mut active = test_account("acc-active-candidate", "active");
    active.label = "Active Candidate".to_string();
    active.group_name = Some("candidate-group".to_string());
    let mut disabled = test_account("acc-disabled-candidate", "disabled");
    disabled.label = "Disabled Candidate".to_string();
    let mut force_enabled = test_account("acc-force-candidate", "force_enabled");
    force_enabled.label = "Force Candidate".to_string();
    storage
        .insert_account(&active)
        .expect("insert active account");
    storage
        .insert_account(&disabled)
        .expect("insert disabled account");
    storage
        .insert_account(&force_enabled)
        .expect("insert force-enabled account");
    storage
        .insert_token(&test_token("acc-active-candidate", "access", "refresh"))
        .expect("insert active token");
    storage
        .insert_token(&test_token("acc-disabled-candidate", "access", "refresh"))
        .expect("insert disabled token");
    storage
        .insert_token(&test_token("acc-force-candidate", "access", "refresh"))
        .expect("insert force-enabled token");
    storage
        .insert_account(&test_account("acc-missing-refresh", "active"))
        .expect("insert missing refresh account");
    storage
        .insert_token(&test_token("acc-missing-refresh", "access", ""))
        .expect("insert missing refresh token");
    drop(storage);

    let result = list_candidates().expect("list candidates");

    assert_eq!(result.accounts.len(), 2);
    let account = &result.accounts[0];
    assert_eq!(account.id, "acc-active-candidate");
    assert_eq!(account.label, "Active Candidate");
    assert_eq!(account.group_name.as_deref(), Some("candidate-group"));
    assert_eq!(account.status, "active");
    assert_eq!(
        account.chatgpt_account_id.as_deref(),
        Some("cgpt-acc-active-candidate")
    );
    assert_eq!(
        account.workspace_id.as_deref(),
        Some("ws-acc-active-candidate")
    );
    assert_eq!(account.issuer, "issuer-acc-active-candidate");
    assert_eq!(account.last_refresh, 123);
    let force_account = &result.accounts[1];
    assert_eq!(force_account.id, "acc-force-candidate");
    assert_eq!(force_account.status, "force_enabled");
    cleanup_profile(&dir);
}

#[test]
fn list_candidates_only_exposes_directly_compatible_aggregate_apis() {
    let _lock = crate::test_env_guard();
    let dir = temp_profile("codex-profile-aggregate-candidates");
    let _db_guard = set_test_db(&dir);
    let storage = Storage::open(dir.join("codexmanager.db")).expect("open storage");
    storage.init().expect("init storage");

    let mut direct = test_aggregate_api("agg-direct", "codex", "active");
    direct.sort = -10;
    direct.user_agent = Some("Aggregate-Direct/1.0".to_string());
    storage
        .insert_aggregate_api(&direct)
        .expect("insert direct aggregate api");

    let disabled = test_aggregate_api("agg-disabled", "compatible", "disabled");
    storage
        .insert_aggregate_api(&disabled)
        .expect("insert disabled aggregate api");

    let unsupported_provider = test_aggregate_api("agg-claude", "claude", "active");
    storage
        .insert_aggregate_api(&unsupported_provider)
        .expect("insert unsupported aggregate api");

    let mut query_auth = test_aggregate_api("agg-query", "codex", "active");
    query_auth.auth_params_json =
        Some(serde_json::json!({"location":"query","name":"key"}).to_string());
    storage
        .insert_aggregate_api(&query_auth)
        .expect("insert query-auth aggregate api");
    drop(storage);

    let result = list_candidates().expect("list candidates");

    assert_eq!(result.aggregate_apis.len(), 1);
    let candidate = &result.aggregate_apis[0];
    assert_eq!(candidate.id, direct.id);
    assert_eq!(candidate.label, "Supplier agg-direct");
    assert_eq!(candidate.base_url, "https://aggregate.example.test/v1");
    assert_eq!(candidate.sort, -10);
    assert_eq!(
        candidate.user_agent.as_deref(),
        Some("Aggregate-Direct/1.0")
    );
    cleanup_profile(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn direct_aggregate_profile_writes_upstream_provider_and_tracks_selection() {
    let _lock = crate::test_env_guard();
    let dir = temp_profile("codex-profile-direct-aggregate");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    fs::create_dir_all(&dir).expect("create profile dir");
    fs::write(
        dir.join(CONFIG_FILE),
        "model_provider = \"external\"\ncustom_setting = true\n",
    )
    .expect("write existing config");

    let storage = Storage::open(dir.join("codexmanager.db")).expect("open storage");
    storage.init().expect("init storage");
    let mut aggregate = test_aggregate_api("agg-profile", "compatible", "active");
    aggregate.supplier_name = Some("Profile Aggregate".to_string());
    aggregate.url = "https://aggregate.example.test/openai".to_string();
    aggregate.action = Some("/v1/responses".to_string());
    aggregate.user_agent = Some("Profile-Aggregate/2.0".to_string());
    aggregate.model_override = Some("aggregate-model".to_string());
    storage
        .insert_aggregate_api(&aggregate)
        .expect("insert aggregate api");
    storage
        .upsert_aggregate_api_secret(&aggregate.id, "aggregate-secret")
        .expect("insert aggregate api secret");
    drop(storage);

    let status = apply_direct_aggregate_async(
        Some(&aggregate.id),
        Some(dir.to_string_lossy().as_ref()),
        false,
    )
    .await
    .expect("apply direct aggregate profile");

    assert!(matches!(status.mode, CodexProfileMode::DirectAggregate));
    assert_eq!(
        status.selected_aggregate_api_id.as_deref(),
        Some(aggregate.id.as_str())
    );
    assert_eq!(
        status.aggregate_api_base_url.as_deref(),
        Some("https://aggregate.example.test/openai/v1")
    );
    assert_eq!(status.provider_id, DIRECT_AGGREGATE_PROVIDER_ID);
    assert!(!status.supports_websockets);
    assert!(
        status.history_repair.is_none(),
        "direct aggregate switching must not synchronously rewrite full history"
    );

    let auth: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(dir.join(AUTH_FILE)).expect("read direct aggregate auth"),
    )
    .expect("parse direct aggregate auth");
    assert_eq!(auth["auth_mode"], "apikey");
    assert_eq!(auth["OPENAI_API_KEY"], "aggregate-secret");

    let config = parse_config(
        &fs::read_to_string(dir.join(CONFIG_FILE)).expect("read direct aggregate config"),
    )
    .expect("parse direct aggregate config");
    assert_eq!(
        config.get("model_provider").and_then(Item::as_str),
        Some(DIRECT_AGGREGATE_PROVIDER_ID)
    );
    assert_eq!(
        config.get("custom_setting").and_then(Item::as_bool),
        Some(true)
    );
    let provider = config
        .get("model_providers")
        .and_then(Item::as_table)
        .and_then(|providers| providers.get(DIRECT_AGGREGATE_PROVIDER_ID))
        .and_then(Item::as_table)
        .expect("direct aggregate provider");
    assert_eq!(
        provider.get("base_url").and_then(Item::as_str),
        Some("https://aggregate.example.test/openai/v1")
    );
    assert_eq!(
        provider.get("wire_api").and_then(Item::as_str),
        Some("responses")
    );
    assert_eq!(
        provider
            .get("experimental_bearer_token")
            .and_then(Item::as_str),
        Some("aggregate-secret")
    );
    assert_eq!(
        provider_http_header(provider, "User-Agent"),
        Some("Profile-Aggregate/2.0")
    );
    assert_eq!(
        config.get("model").and_then(Item::as_str),
        Some("aggregate-model")
    );
    assert!(config
        .get("model_catalog_json")
        .and_then(Item::as_str)
        .is_some());

    let marker = read_marker(
        &managed_profile_paths(&dir)
            .expect("managed profile paths")
            .marker_path,
    )
    .expect("read managed marker");
    assert!(matches!(marker.mode, CodexProfileMode::DirectAggregate));
    assert_eq!(
        marker.aggregate_api_id.as_deref(),
        Some(aggregate.id.as_str())
    );
    cleanup_profile(&dir);
}

#[test]
fn restore_optional_file_removes_files_that_were_missing() {
    let dir = temp_profile("restore-missing");
    fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("auth.json");
    fs::write(&path, "{}").expect("write");

    restore_optional_file(&path, None).expect("restore missing");

    assert!(!path.exists());
    cleanup_profile(&dir);
}

#[tokio::test(flavor = "current_thread")]
async fn restore_waits_for_the_profile_mutation_lease() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("restore-profile-lock");
    let _db_guard = set_test_db(&dir);
    let _backend_guard = EnvGuard::remove("CODEXMANAGER_STORAGE_BACKEND");
    let _database_url_guard = EnvGuard::remove("CODEXMANAGER_DATABASE_URL");
    fs::create_dir_all(&dir).expect("create profile");
    fs::write(dir.join(CONFIG_FILE), "model_provider = \"current\"\n")
        .expect("write current config");
    let key = profile_key(&dir);
    let now = now_ts();
    save_backups(&HashMap::from([(
        key.clone(),
        BackupEntry {
            profile_dir: key,
            auth_json: None,
            config_toml: Some("model_provider = \"restored\"\n".to_string()),
            created_at: now,
            updated_at: now,
        },
    )]))
    .expect("save profile backup");

    let lease = profile_mutation_lease().await;
    let codex_home = dir.to_string_lossy().to_string();
    let restore_task = tokio::spawn(async move { restore_async(Some(codex_home.as_str())).await });
    tokio::task::yield_now().await;
    assert!(!restore_task.is_finished());
    assert_eq!(
        fs::read_to_string(dir.join(CONFIG_FILE)).expect("read current config"),
        "model_provider = \"current\"\n"
    );

    drop(lease);
    tokio::time::timeout(Duration::from_secs(2), restore_task)
        .await
        .expect("restore acquires released profile lock")
        .expect("restore task")
        .expect("restore profile");
    assert_eq!(
        fs::read_to_string(dir.join(CONFIG_FILE)).expect("read restored config"),
        "model_provider = \"restored\"\n"
    );
    cleanup_profile(&dir);
}

#[test]
fn auth_json_shapes_match_codex_modes() {
    let now = now_ts();
    let account = AccountDirectAuthProfile {
        id: "acc-1".to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("chatgpt-1".to_string()),
        status: "active".to_string(),
    };
    let token = Token {
        account_id: "acc-1".to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: "refresh-token".to_string(),
        api_key_access_token: None,
        last_refresh: now,
    };

    let direct = build_direct_auth_json(&account, &token).expect("direct auth");
    let gateway = build_gateway_auth_json("cm-key").expect("gateway auth");

    assert!(auth_json_has_tokens(&direct));
    assert!(!auth_json_is_gateway(&direct));
    assert!(auth_json_is_gateway(&gateway));
}

fn detect_login_mode_for_config(name: &str, config: &str) -> CodexProfileMode {
    let _env_lock = crate::test_env_guard();
    let _service_addr = EnvGuard::set("CODEXMANAGER_SERVICE_ADDR", "127.0.0.1:48760");
    let dir = temp_profile(name);
    fs::create_dir_all(&dir).expect("mkdir profile");
    fs::write(
        dir.join(AUTH_FILE),
        r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"access-token"}}"#,
    )
    .expect("write auth");
    fs::write(dir.join(CONFIG_FILE), config).expect("write config");

    let mode = detect_mode(&dir.join(AUTH_FILE), &dir.join(CONFIG_FILE), None);
    cleanup_profile(&dir);
    mode
}

#[test]
fn experimental_gateway_base_without_token_keeps_login_mode_direct() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-without-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"

[model_providers.default]
base_url = "http://localhost:48760/v1"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_blank_token_keeps_login_mode_direct() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-blank-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "   "

[model_providers.default]
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "\t"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_ignores_token_from_other_provider() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-other-provider-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"

[model_providers.default]
base_url = "http://localhost:48760/v1"

[model_providers.other]
base_url = "https://example.test/v1"
experimental_bearer_token = "other-provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_accepts_top_level_token() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-top-level-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "top-level-key"

[model_providers.default]
base_url = "http://localhost:48760/v1"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn experimental_gateway_accepts_current_provider_token() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-current-provider-token",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"

[model_providers.default]
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn experimental_gateway_prefers_current_provider_external_base_url_over_stale_local_root() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-provider-external-root-local",
        r#"
model_provider = "default"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "top-level-key"

[model_providers.default]
base_url = "https://example.test/v1"
experimental_bearer_token = "provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
}

#[test]
fn experimental_gateway_prefers_current_provider_local_base_url_over_stale_external_root() {
    let mode = detect_login_mode_for_config(
        "experimental-gateway-provider-local-root-external",
        r#"
model_provider = "default"
base_url = "https://example.test/v1"
experimental_bearer_token = "top-level-key"

[model_providers.default]
base_url = "http://127.0.0.1:48760/v1"
experimental_bearer_token = "provider-key"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn managed_gateway_provider_does_not_require_experimental_token() {
    let mode = detect_login_mode_for_config(
        "managed-gateway-without-experimental-token",
        r#"
model_provider = "cm"

[model_providers.cm]
base_url = "http://localhost:48760/v1"
wire_api = "responses"
"#,
    );

    assert!(matches!(mode, CodexProfileMode::Gateway));
}

#[test]
fn experimental_gateway_config_overrides_login_tokens_and_stale_direct_marker() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("experimental-gateway-detection");
    let _db_guard = set_test_db(&dir);
    fs::create_dir_all(&dir).expect("mkdir profile");
    fs::write(
        dir.join(AUTH_FILE),
        r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"access-token"}}"#,
    )
    .expect("write auth");
    fs::write(
        dir.join(CONFIG_FILE),
        r#"
model_provider = "default"
wire_api = "responses"
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "cm-key"

[model_providers.default]
name = "OpenAI"
wire_api = "responses"
requires_openai_auth = true
base_url = "http://localhost:48760/v1"
experimental_bearer_token = "cm-key"
"#,
    )
    .expect("write config");
    let marker = MarkerFile {
        writer: "codexmanager".to_string(),
        mode: CodexProfileMode::DirectAccount,
        account_id: Some("acc-1".to_string()),
        api_key_id: None,
        aggregate_api_id: None,
        gateway_base_url: None,
        aggregate_api_base_url: None,
        supports_websockets: None,
        provider_id: PROVIDER_ID.to_string(),
        managed_model_slugs: Vec::new(),
        updated_at: now_ts(),
    };

    let mode = detect_mode(&dir.join(AUTH_FILE), &dir.join(CONFIG_FILE), Some(&marker));

    assert!(matches!(mode, CodexProfileMode::Gateway));
    fs::write(
        dir.join(MARKER_FILE),
        serde_json::to_string_pretty(&marker).expect("marker json"),
    )
    .expect("write stale marker");
    let status = status_for_profile(&dir).expect("profile status");
    assert!(matches!(status.mode, CodexProfileMode::Gateway));
    assert_eq!(status.provider_id, "default");
    assert_eq!(
        status.gateway_base_url.as_deref(),
        Some("http://localhost:48760/v1")
    );
    assert_eq!(status.selected_account_id, None);
    assert_eq!(status.last_applied_at, None);
    assert_eq!(
        target_history_provider_for_profile(&dir).expect("history provider"),
        "default"
    );
    cleanup_profile(&dir);
}

#[test]
fn experimental_non_gateway_base_url_keeps_login_mode_direct() {
    let dir = temp_profile("experimental-non-gateway-detection");
    fs::create_dir_all(&dir).expect("mkdir profile");
    fs::write(
        dir.join(AUTH_FILE),
        r#"{"OPENAI_API_KEY":null,"tokens":{"access_token":"access-token"}}"#,
    )
    .expect("write auth");
    fs::write(
        dir.join(CONFIG_FILE),
        r#"
model_provider = "default"
base_url = "http://localhost:12345/v1"
experimental_bearer_token = "other-key"

[model_providers.default]
base_url = "http://localhost:12345/v1"
experimental_bearer_token = "other-key"
"#,
    )
    .expect("write config");

    let mode = detect_mode(&dir.join(AUTH_FILE), &dir.join(CONFIG_FILE), None);

    assert!(matches!(mode, CodexProfileMode::DirectAccount));
    cleanup_profile(&dir);
}

#[test]
fn write_profile_files_uses_internal_marker() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("internal-marker");
    let _db_guard = set_test_db(&dir);
    let state = ManagedState {
        profile_dir: profile_key(&dir),
        mode: CodexProfileMode::Gateway,
        account_id: None,
        api_key_id: Some("key-1".to_string()),
        aggregate_api_id: None,
        gateway_base_url: Some("http://localhost:48760/v1".to_string()),
        aggregate_api_base_url: None,
        supports_websockets: Some(false),
        provider_id: PROVIDER_ID.to_string(),
        previous_model_catalog_json: None,
        managed_model_slugs: Vec::new(),
        updated_at: now_ts(),
    };

    write_profile_files(&dir, "{}", "", state).expect("write profile");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(paths.marker_path.exists());
    assert!(!dir.join(MARKER_FILE).exists());
    let status = status_for_profile(&dir).expect("status");
    assert!(matches!(status.mode, CodexProfileMode::Gateway));
    assert_eq!(
        status.marker_path,
        paths.marker_path.to_string_lossy().to_string()
    );
    cleanup_profile(&dir);
}

#[test]
fn legacy_profile_state_defaults_selected_models_to_empty() {
    let state: ManagedState = serde_json::from_value(serde_json::json!({
        "profileDir": "C:/Users/example/.codex",
        "mode": "gateway",
        "accountId": null,
        "apiKeyId": "key-1",
        "gatewayBaseUrl": "http://localhost:48760/v1",
        "providerId": "cm",
        "updatedAt": 1
    }))
    .expect("legacy managed state");
    assert!(state.managed_model_slugs.is_empty());

    let marker: MarkerFile = serde_json::from_value(serde_json::json!({
        "writer": "codexmanager",
        "mode": "gateway",
        "accountId": null,
        "apiKeyId": "key-1",
        "gatewayBaseUrl": "http://localhost:48760/v1",
        "providerId": "cm",
        "updatedAt": 1
    }))
    .expect("legacy marker");
    assert!(marker.managed_model_slugs.is_empty());
}

#[test]
fn legacy_marker_migrates_to_internal_marker() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("legacy-marker");
    let _db_guard = set_test_db(&dir);
    fs::create_dir_all(&dir).expect("mkdir profile");
    let marker = MarkerFile {
        writer: "codexmanager".to_string(),
        mode: CodexProfileMode::DirectAccount,
        account_id: Some("acc-1".to_string()),
        api_key_id: None,
        aggregate_api_id: None,
        gateway_base_url: None,
        aggregate_api_base_url: None,
        supports_websockets: None,
        provider_id: PROVIDER_ID.to_string(),
        managed_model_slugs: Vec::new(),
        updated_at: now_ts(),
    };
    fs::write(
        dir.join(MARKER_FILE),
        serde_json::to_string_pretty(&marker).expect("marker json"),
    )
    .expect("write legacy marker");

    let status = status_for_profile(&dir).expect("status");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(paths.marker_path.exists());
    assert!(!paths.legacy_marker_path.exists());
    assert!(matches!(status.mode, CodexProfileMode::DirectAccount));
    cleanup_profile(&dir);
}

#[test]
fn legacy_history_backups_migrate_and_are_pruned() {
    let _env_lock = crate::test_env_guard();
    let dir = temp_profile("legacy-history-backups");
    let _db_guard = set_test_db(&dir);
    let legacy_root = dir.join(HISTORY_BACKUP_DIR);
    fs::create_dir_all(&legacy_root).expect("mkdir legacy root");
    for index in 0..5 {
        let backup_dir = legacy_root.join(format!("backup-{index}"));
        fs::create_dir_all(&backup_dir).expect("mkdir legacy backup");
        fs::write(backup_dir.join("file.txt"), format!("backup-{index}"))
            .expect("write legacy backup");
    }

    let status = status_for_profile(&dir).expect("status");

    let paths = managed_profile_paths(&dir).expect("paths");
    assert!(!paths.legacy_history_backup_root.exists());
    assert!(paths.history_backup_root.exists());
    assert_eq!(status.history_backup_count, MAX_HISTORY_BACKUPS_PER_PROFILE);
    cleanup_profile(&dir);
}

#[test]
fn history_repair_aligns_direct_and_gateway_providers() {
    let dir = temp_profile("history-provider");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-provider";
    let (rollout_path, event_line) = write_test_rollout(&dir, thread_id, PROVIDER_ID);
    create_state_db(&dir, thread_id, PROVIDER_ID);
    fs::write(
        dir.join(SESSION_INDEX_FILE),
        format!(
            "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
        ),
    )
    .expect("write session index");

    let direct = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(direct.warnings.is_empty(), "{:?}", direct.warnings);
    assert_eq!(direct.changed_rollout_file_count, 1);
    assert_eq!(direct.updated_sqlite_row_count, 1);
    assert_eq!(
        sqlite_provider(&dir, thread_id),
        DEFAULT_HISTORY_PROVIDER_ID
    );
    let direct_rollout = fs::read_to_string(&rollout_path).expect("read direct rollout");
    assert!(direct_rollout.contains("\"model_provider\":\"openai\""));
    assert!(direct_rollout.contains(&event_line));
    assert!(!dir.join(HISTORY_BACKUP_DIR).exists());
    let direct_backup = direct.backup_dir.as_ref().expect("direct backup dir");
    assert!(direct_backup.contains(MANAGED_PROFILE_ROOT_DIR));
    let direct_backup_path = PathBuf::from(direct_backup);
    assert!(direct_backup_path.join(STATE_DB_FILE).exists());
    assert!(!direct_backup_path
        .join(format!("{STATE_DB_FILE}-wal"))
        .exists());
    assert!(!direct_backup_path
        .join(format!("{STATE_DB_FILE}-shm"))
        .exists());
    assert!(direct_backup_path
        .join(HISTORY_BACKUP_MANIFEST_FILE)
        .exists());

    let gateway = repair_history_for_provider(&dir, PROVIDER_ID);

    assert!(gateway.warnings.is_empty(), "{:?}", gateway.warnings);
    assert_eq!(gateway.changed_rollout_file_count, 1);
    assert_eq!(gateway.updated_sqlite_row_count, 1);
    assert_eq!(sqlite_provider(&dir, thread_id), PROVIDER_ID);
    let gateway_rollout = fs::read_to_string(&rollout_path).expect("read gateway rollout");
    assert!(gateway_rollout.contains("\"model_provider\":\"cm\""));
    assert!(gateway_rollout.contains(&event_line));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_appends_missing_session_index_once() {
    let dir = temp_profile("history-index");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-index";
    create_state_db(&dir, thread_id, DEFAULT_HISTORY_PROVIDER_ID);

    let first = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);
    let second = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    assert_eq!(first.added_session_index_entry_count, 1);
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
    assert_eq!(second.added_session_index_entry_count, 0);
    let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
    assert_eq!(index.lines().count(), 1);
    assert!(index.contains(thread_id));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_handles_sqlite_with_only_updated_at_ms() {
    let dir = temp_profile("history-index-updated-ms-only");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-index-ms";
    let conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open sqlite");
    conn.execute(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            model_provider TEXT,
            title TEXT,
            updated_at_ms INTEGER
        )",
        [],
    )
    .expect("create threads");
    conn.execute(
        "INSERT INTO threads (id, model_provider, title, updated_at_ms)
         VALUES (?1, ?2, 'Thread title', 1770000000000)",
        params![thread_id, DEFAULT_HISTORY_PROVIDER_ID],
    )
    .expect("insert thread");
    drop(conn);

    let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert_eq!(summary.added_session_index_entry_count, 1);
    let index = fs::read_to_string(dir.join(SESSION_INDEX_FILE)).expect("read index");
    assert!(index.contains(thread_id));
    assert!(index.contains("2026"));
    cleanup_profile(&dir);
}

#[test]
fn history_repair_reports_sqlite_lock_as_warning() {
    let dir = temp_profile("history-locked");
    fs::create_dir_all(&dir).expect("mkdir profile");
    let thread_id = "thread-locked";
    create_state_db(&dir, thread_id, PROVIDER_ID);
    fs::write(
        dir.join(SESSION_INDEX_FILE),
        format!(
            "{{\"id\":\"{thread_id}\",\"thread_name\":\"Thread title\",\"updated_at\":\"2026-06-06T00:00:00Z\"}}\n"
        ),
    )
    .expect("write session index");
    let lock_conn = Connection::open(dir.join(STATE_DB_FILE)).expect("open lock sqlite");
    lock_conn
        .execute("BEGIN IMMEDIATE", [])
        .expect("begin immediate");

    let summary = repair_history_for_provider(&dir, DEFAULT_HISTORY_PROVIDER_ID);

    assert_eq!(summary.updated_sqlite_row_count, 0);
    assert!(
        summary
            .warnings
            .iter()
            .any(|warning| warning.contains("update Codex history sqlite provider failed")),
        "{:?}",
        summary.warnings
    );
    drop(lock_conn);
    cleanup_profile(&dir);
}
