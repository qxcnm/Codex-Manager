use super::{builtin_catalog_entries, catalog_list_result, market_source_mode_for_request};
use codexmanager_core::rpc::types::{JsonRpcRequest, RequestId};

/// 函数 `catalog_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - params: 参数 params
///
/// # 返回
/// 返回函数执行结果
fn catalog_request(params: serde_json::Value) -> JsonRpcRequest {
    JsonRpcRequest {
        id: RequestId::from(1),
        method: "plugin/catalog/list".to_string(),
        params: Some(params),
        trace: None,
    }
}

/// 函数 `builtin_catalog_exposes_cleanup_plugins`
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
fn builtin_catalog_exposes_cleanup_plugins() {
    let items = builtin_catalog_entries();
    assert_eq!(items.len(), 2);
    let banned = items
        .iter()
        .find(|item| item.id == "cleanup-banned-accounts")
        .expect("banned cleanup plugin");
    assert_eq!(banned.manifest_version, "1");
    assert_eq!(banned.category.as_deref(), Some("official"));
    assert_eq!(banned.runtime_kind, "rhai");
    assert!(banned
        .permissions
        .iter()
        .any(|item| item == "accounts:cleanup"));
    assert!(!banned.tags.is_empty());
    assert_eq!(banned.tasks.len(), 1);
    assert_eq!(banned.tasks[0].entrypoint, "run");
    assert_eq!(banned.tasks[0].schedule_kind, "interval");
    assert_eq!(
        banned.tasks[0].interval_seconds,
        Some(super::BUILTIN_CLEANUP_TASK_INTERVAL_SECS)
    );

    let unavailable_free = items
        .iter()
        .find(|item| item.id == "cleanup-unavailable-free-accounts")
        .expect("unavailable free cleanup plugin");
    assert_eq!(unavailable_free.manifest_version, "1");
    assert_eq!(unavailable_free.category.as_deref(), Some("official"));
    assert_eq!(unavailable_free.runtime_kind, "rhai");
    assert!(unavailable_free
        .permissions
        .iter()
        .any(|item| item == "accounts:cleanup"));
    assert!(!unavailable_free.tags.is_empty());
    assert_eq!(unavailable_free.tasks.len(), 1);
    assert_eq!(unavailable_free.tasks[0].entrypoint, "run");
    assert_eq!(unavailable_free.tasks[0].schedule_kind, "interval");
    assert_eq!(
        unavailable_free.tasks[0].interval_seconds,
        Some(super::BUILTIN_UNAVAILABLE_FREE_CLEANUP_TASK_INTERVAL_SECS)
    );
}

/// 函数 `request_market_mode_normalizes_private_to_custom`
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
fn request_market_mode_normalizes_private_to_custom() {
    let request = catalog_request(serde_json::json!({
        "marketMode": "private"
    }));
    assert_eq!(market_source_mode_for_request(&request), "custom");
}

/// 函数 `custom_market_with_unreachable_source_returns_empty_items`
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
fn custom_market_with_unreachable_source_returns_empty_items() {
    let request = catalog_request(serde_json::json!({
        "marketMode": "custom",
        "sourceUrl": "http://127.0.0.1:9/unreachable-plugin-market.json"
    }));
    let response = catalog_list_result(&request).expect("catalog response");
    let items = response
        .get("items")
        .and_then(serde_json::Value::as_array)
        .expect("items array");
    assert!(items.is_empty());
    assert_eq!(
        response
            .get("sourceUrl")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        "http://127.0.0.1:9/unreachable-plugin-market.json"
    );
}

/// 函数 `custom_market_without_source_returns_empty_items`
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
fn custom_market_without_source_returns_empty_items() {
    let request = catalog_request(serde_json::json!({
        "marketMode": "custom"
    }));
    let response = catalog_list_result(&request).expect("catalog response");
    let items = response
        .get("items")
        .and_then(serde_json::Value::as_array)
        .expect("items array");
    assert!(items.is_empty());
    assert_eq!(
        response
            .get("sourceUrl")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        ""
    );
}

/// 函数 `builtin_market_never_uses_custom_source`
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
fn builtin_market_never_uses_custom_source() {
    let request = catalog_request(serde_json::json!({
        "marketMode": "builtin",
        "sourceUrl": "http://127.0.0.1:48888/plugin-market.json"
    }));
    let response = catalog_list_result(&request).expect("catalog response");
    let items = response
        .get("items")
        .and_then(serde_json::Value::as_array)
        .expect("items array");
    assert_eq!(items.len(), 2);
    assert_eq!(
        response
            .get("sourceUrl")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        ""
    );
}

#[test]
fn native_catalog_download_install_update_and_script_run_persist_results() {
    let _guard = crate::test_env_guard();
    struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let _restore = Restore(
        [
            "CODEXMANAGER_DB_PATH",
            "CODEXMANAGER_STORAGE_BACKEND",
            "CODEXMANAGER_DATABASE_URL",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect(),
    );
    let path = std::env::temp_dir().join(format!("plugin-native-{}.sqlite", rand::random::<u64>()));
    std::env::set_var("CODEXMANAGER_DB_PATH", &path);
    std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
    std::env::remove_var("CODEXMANAGER_DATABASE_URL");
    crate::storage_helpers::initialize_storage().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let version = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1));
        let catalog_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app = axum::Router::new().route("/catalog", axum::routing::get({
            let base = base.clone(); let version = version.clone(); let hits = catalog_hits.clone();
            move || {
                let base = base.clone();
                let version = version.load(std::sync::atomic::Ordering::SeqCst);
                hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move { axum::Json(serde_json::json!({"items": [{
                    "id": "native-plugin", "name": "Native plugin", "version": version.to_string(),
                    "scriptUrl": format!("{base}/script"), "permissions": ["network"],
                    "tasks": [{"id": "run", "name": "Run", "entrypoint": "run", "scheduleKind": "manual"}]
                }]})) }
            }
        })).route("/script", axum::routing::get(|| async { "fn run(context) { #{ accepted: true } }" }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let mut req = catalog_request(serde_json::json!({
            "marketMode": "custom", "sourceUrl": format!("{base}/catalog"), "pluginId": "native-plugin"
        }));
        let catalog = super::handle_catalog_list_async(&req).await;
        assert_eq!(catalog.result["items"][0]["version"], "1");
        req.method = "plugin/install".to_string();
        let installed = super::handle_install_async(&req, false).await;
        assert_eq!(installed.result["plugin"]["version"], "1");
        version.store(2, std::sync::atomic::Ordering::SeqCst);
        req.method = "plugin/update".to_string();
        let updated = super::handle_install_async(&req, true).await;
        assert_eq!(updated.result["plugin"]["version"], "2");
        assert_eq!(catalog_hits.load(std::sync::atomic::Ordering::SeqCst), 3);
        req.method = "plugin/tasks/run".to_string();
        req.params = Some(serde_json::json!({"taskId": "native-plugin::run"}));
        let ran = super::super::runtime::handle_task_run_async(&req).await;
        assert_eq!(ran.result["output"]["accepted"], true);
        server.abort();
    });
    let storage = crate::storage_helpers::open_storage().unwrap();
    let plugin = storage
        .find_plugin_install("native-plugin")
        .unwrap()
        .unwrap();
    assert_eq!(plugin.version, "2");
    let task = storage
        .find_plugin_task("native-plugin::run")
        .unwrap()
        .unwrap();
    assert_eq!(task.last_status.as_deref(), Some("ok"));
}
