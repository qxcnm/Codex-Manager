use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};

pub(crate) fn is_async_method(method: &str) -> bool {
    matches!(
        method,
        "account/login/start"
            | "account/bps/login/start"
            | "account/login/complete"
            | "codexProfile/applyDirectAccount"
            | "codexProfile/applyGateway"
            | "codexProfile/applyModels"
            | "codexProfile/restore"
            | "codexSkills/repositoryAdd"
            | "codexSkills/repositoryDelete"
            | "codexSkills/repositoryRefresh"
            | "codexSkills/repositoryInstall"
            | "codexSkills/registrySearch"
            | "codexSkills/registryInstall"
            | "plugin/catalog/list"
            | "plugin/catalog/refresh"
            | "plugin/install"
            | "plugin/update"
            | "plugin/tasks/run"
            | "account/warmup"
            | "account/fetchModels"
            | "account/test"
            | "account/test/cancel"
            | "account/read"
            | "account/chatgptAuthTokens/refresh"
            | "account/chatgptAuthTokens/refreshAll"
            | "aggregateApi/testConnection"
            | "aggregateApi/refreshBalance"
            | "aggregateApi/fetchModels"
            | "account/usage/resetCredits"
            | "account/usage/resetCredit/consume"
            | "account/proxy/test"
            | "system/proxy/test"
            | "account/usage/refresh"
            | "apikey/managedModelPriceSyncV2"
            | "gateway/codexLatestVersion/get"
    )
}

fn parse_managed_model_price_sync_params(
    req: &JsonRpcRequest,
) -> Result<crate::models_v2::ManagedModelPriceSyncV2Params, String> {
    let params = req.params.clone().unwrap_or_else(|| serde_json::json!({}));
    if params.is_null() {
        return Ok(crate::models_v2::ManagedModelPriceSyncV2Params::default());
    }
    serde_json::from_value::<crate::models_v2::ManagedModelPriceSyncV2Params>(params)
        .map_err(|error| format!("parse managed model price sync payload failed: {error}"))
}

/// The HTTP boundary authenticates the RPC token first. Keep actor authorization
/// here, before any async domain can contact a provider or mutate local state.
pub(crate) async fn try_handle_network_request_async(
    req: &JsonRpcRequest,
    actor: &crate::RpcActor,
) -> Option<JsonRpcMessage> {
    if !is_async_method(&req.method) {
        return None;
    }
    if let Err(error) = super::ensure_method_allowed(actor, &req.method) {
        return Some(JsonRpcMessage::Response(super::response(
            req,
            super::value_or_error::<()>(Err(error)),
        )));
    }
    if req.method == "gateway/codexLatestVersion/get" {
        let result =
            super::value_or_error(crate::app_settings::fetch_codex_latest_version_async().await);
        return Some(JsonRpcMessage::Response(super::response(req, result)));
    }
    if req.method == "account/bps/login/start" {
        let account_id = super::str_param(req, "accountId").unwrap_or("");
        let result = super::value_or_error(crate::account::bps::login_start(account_id).await);
        return Some(JsonRpcMessage::Response(super::response(req, result)));
    }
    if req.method == "apikey/managedModelPriceSyncV2" {
        let result = super::value_or_error(match parse_managed_model_price_sync_params(req) {
            Ok(params) => crate::models_v2::sync_prices(params).await,
            Err(error) => Err(error),
        });
        return Some(JsonRpcMessage::Response(super::response(req, result)));
    }
    if let Some(message) = super::auth_async::try_handle_auth_request_async(req, actor).await {
        return Some(message);
    }
    if let Some(message) = super::plugin_async::try_handle_plugin_request_async(req, actor).await {
        return Some(message);
    }
    if let Some(message) = super::account_async::try_handle_account_request_async(req, actor).await
    {
        return Some(message);
    }
    if let Some(message) =
        super::aggregate_async::try_handle_aggregate_request_async(req, actor).await
    {
        return Some(message);
    }
    if let Some(response) = super::account_auth_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    if let Some(response) = super::reset_credits_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    if let Some(response) = super::usage_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    if let Some(response) = super::codex_profile_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    super::codex_skills_async::try_handle(req)
        .await
        .map(JsonRpcMessage::Response)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StorageEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl StorageEnvironment {
        fn publish(path: &std::path::Path) -> Self {
            let previous = Self(
                [
                    "CODEXMANAGER_STORAGE_BACKEND",
                    "CODEXMANAGER_DATABASE_URL",
                    "CODEXMANAGER_DB_PATH",
                ]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
            );
            std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
            std::env::remove_var("CODEXMANAGER_DATABASE_URL");
            // Select the SQLite backend before publishing its database path.
            std::env::set_var("CODEXMANAGER_DB_PATH", path);
            previous
        }
    }

    impl Drop for StorageEnvironment {
        fn drop(&mut self) {
            for (key, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    struct PermissionDatabase {
        path: std::path::PathBuf,
    }

    impl PermissionDatabase {
        fn new_in(directory: &std::path::Path) -> Self {
            static NEXT_FIXTURE: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("fixture timestamp")
                .as_nanos();
            let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = directory.join(format!(
                "async-rpc-permissions-{}-{timestamp}-{sequence}.db",
                std::process::id()
            ));
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .expect("exclusively reserve permission database");
            // Core storage takes an explicit path and does not expose this
            // incomplete database through the process-wide environment.
            let storage = codexmanager_core::storage::Storage::open(&path)
                .expect("open private permission database");
            storage
                .init()
                .expect("initialize private permission database");
            storage
                .smoke_check_model_catalog_v2()
                .expect("check private permission model catalog");
            storage
                .set_app_setting("web.auth.mode", "accounts", 1)
                .expect("configure private permission database");
            drop(storage);
            Self { path }
        }

        fn publish(&self) -> StorageEnvironment {
            StorageEnvironment::publish(&self.path)
        }
    }

    fn migration_lock_path(path: &std::path::Path) -> std::path::PathBuf {
        std::path::PathBuf::from(format!("{}.model-catalog-v2.lock", path.display()))
    }

    fn hold_migration_lock(path: &std::path::Path) -> std::fs::File {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(migration_lock_path(path))
            .expect("exclusively create fixture migration lock");
        file.try_lock().expect("hold fixture migration lock");
        file
    }

    #[test]
    fn network_permission_fixture_is_ready_before_concurrent_initialization() {
        const CHILD_TEST: &str = "CODEXMANAGER_NETWORK_PERMISSION_FIXTURE_CHILD_TEST";
        const CHILD_DIRECTORY: &str = "CODEXMANAGER_NETWORK_PERMISSION_FIXTURE_CHILD_DIRECTORY";
        let current = std::thread::current();
        let test_name = current.name().expect("named test thread");
        if std::env::var(CHILD_TEST).as_deref() != Ok(test_name) {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("child fixture timestamp")
                .as_nanos();
            let directory = std::env::temp_dir().join(format!(
                "async-rpc-permission-controls-{}-{timestamp}",
                std::process::id()
            ));
            std::fs::create_dir(&directory).expect("create parent-owned permission directory");
            // A fresh child has no initialized-path cache or detached startup
            // task that could prepare either control before its first observer.
            let output = std::process::Command::new(
                std::env::current_exe().expect("current test executable"),
            )
            .args(["--exact", test_name, "--nocapture"])
            .env(CHILD_TEST, test_name)
            .env(CHILD_DIRECTORY, &directory)
            .output();
            let output = output.unwrap_or_else(|error| {
                panic!(
                    "could not start {test_name}: {error}; fixture retained at {}",
                    directory.display()
                )
            });
            assert!(
                output.status.success(),
                "{test_name} child status: {}; fixture retained at {}\n{}\n{}",
                output.status,
                directory.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            // Successful child exit closes every pool handle before one cleanup.
            // Failure keeps the owned directory available for diagnosis.
            std::fs::remove_dir_all(&directory).unwrap_or_else(|error| {
                panic!("cleanup of {} failed: {error}", directory.display())
            });
            return;
        }

        let _guard = crate::test_env_guard();
        let directory = std::path::PathBuf::from(
            std::env::var_os(CHILD_DIRECTORY).expect("parent-owned permission directory"),
        );
        let incomplete_path = directory.join("incomplete.db");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&incomplete_path)
            .expect("exclusively reserve incomplete control");
        let incomplete_lock = hold_migration_lock(&incomplete_path);
        {
            let _restore = StorageEnvironment::publish(&incomplete_path);
            let error = crate::storage_helpers::initialize_storage()
                .expect_err("an incomplete database must require the held migration lock");
            assert!(
                error.starts_with("model catalog migration lock is held by another process ("),
                "{error}"
            );
            assert!(
                error.contains(&migration_lock_path(&incomplete_path).display().to_string()),
                "{error}"
            );
            assert_eq!(std::fs::metadata(&incomplete_path).unwrap().len(), 0);
        }
        drop(incomplete_lock);

        let previous_path = std::env::var_os("CODEXMANAGER_DB_PATH");
        let ready = PermissionDatabase::new_in(&directory);
        assert_eq!(
            std::env::var_os("CODEXMANAGER_DB_PATH"),
            previous_path,
            "private preparation must not publish its database path"
        );
        let ready_lock = hold_migration_lock(&ready.path);
        {
            let _restore = ready.publish();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
            let observers: Vec<_> = (0..4)
                .map(|_| {
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        crate::storage_helpers::initialize_storage()
                            .expect("initialize an already prepared permission database");
                        crate::storage_helpers::open_storage()
                            .expect("open prepared permission database")
                            .get_app_setting("web.auth.mode")
                            .expect("read prepared permission mode")
                    })
                })
                .collect();
            let results: Vec<_> = observers
                .into_iter()
                .map(|observer| observer.join())
                .collect();
            for result in results {
                assert_eq!(
                    result.expect("permission observer completed").as_deref(),
                    Some("accounts")
                );
            }
            let competing_lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(migration_lock_path(&ready.path))
                .expect("open competing migration lock handle");
            assert!(matches!(
                competing_lock.try_lock(),
                Err(std::fs::TryLockError::WouldBlock)
            ));
        }
        drop(ready_lock);
    }

    fn price_sync_request(params: Option<serde_json::Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            id: 1.into(),
            method: "apikey/managedModelPriceSyncV2".into(),
            params,
            trace: None,
        }
    }

    #[test]
    fn managed_model_price_sync_params_default_to_all_models() {
        assert!(
            parse_managed_model_price_sync_params(&price_sync_request(None))
                .unwrap()
                .model_slugs
                .is_empty()
        );
        assert!(
            parse_managed_model_price_sync_params(&price_sync_request(Some(
                serde_json::Value::Null,
            )))
            .unwrap()
            .model_slugs
            .is_empty()
        );
        assert!(
            parse_managed_model_price_sync_params(&price_sync_request(
                Some(serde_json::json!({}),)
            ))
            .unwrap()
            .model_slugs
            .is_empty()
        );
    }

    #[test]
    fn managed_model_price_sync_params_parse_selected_slugs_and_reject_invalid_shape() {
        let parsed = parse_managed_model_price_sync_params(&price_sync_request(Some(
            serde_json::json!({"modelSlugs": ["gpt-6-sol", "gpt-6-luna"]}),
        )))
        .unwrap();
        assert_eq!(parsed.model_slugs, ["gpt-6-sol", "gpt-6-luna"]);

        let error = parse_managed_model_price_sync_params(&price_sync_request(Some(
            serde_json::json!({"modelSlugs": "gpt-6-sol"}),
        )))
        .expect_err("non-array modelSlugs must be rejected");
        assert!(error.contains("parse managed model price sync payload failed"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn network_methods_preserve_member_denials_before_side_effects() {
        let _guard = crate::test_env_guard();
        let database = PermissionDatabase::new_in(&std::env::temp_dir());
        let _restore = database.publish();
        crate::storage_helpers::initialize_storage().unwrap();
        let actor = crate::RpcActor::from_parts(Some("member"), Some("fixture-member"));
        for method in [
            "account/login/start",
            "account/login/complete",
            "account/read",
            "account/chatgptAuthTokens/refresh",
            "account/chatgptAuthTokens/refreshAll",
            "account/proxy/test",
            "account/fetchModels",
            "account/test",
            "aggregateApi/fetchModels",
            "aggregateApi/testConnection",
            "aggregateApi/refreshBalance",
            "plugin/catalog/refresh",
            "plugin/install",
            "plugin/update",
            "plugin/tasks/run",
            "codexSkills/repositoryAdd",
            "codexSkills/registryInstall",
            "codexProfile/applyDirectAccount",
            "codexProfile/applyGateway",
            "codexProfile/applyModels",
            "system/proxy/test",
            "account/usage/resetCredits",
            "account/usage/resetCredit/consume",
            "apikey/managedModelPriceSyncV2",
        ] {
            let req = JsonRpcRequest {
                id: 42.into(),
                method: method.into(),
                params: None,
                trace: None,
            };
            let expected = crate::handle_request_with_actor(req.clone(), actor.clone());
            let actual = try_handle_network_request_async(&req, &actor)
                .await
                .expect("async method routed");
            let actual = serde_json::to_value(actual).unwrap();
            assert_eq!(actual, serde_json::to_value(expected).unwrap(), "{method}");
            assert_eq!(
                actual["result"]["errorCode"], "permission_denied",
                "{method}"
            );
        }
        for method in [
            "account/usage/resetCredits",
            "account/usage/resetCredit/consume",
        ] {
            let req = JsonRpcRequest {
                id: 44.into(),
                method: method.into(),
                params: None,
                trace: None,
            };
            let actual = serde_json::to_value(
                try_handle_network_request_async(&req, &crate::RpcActor::system_admin())
                    .await
                    .expect("admin reset-credit method is routed"),
            )
            .unwrap();
            assert_ne!(
                actual["result"]["errorCode"], "permission_denied",
                "{method}"
            );
        }
        let req = JsonRpcRequest {
            id: 43.into(),
            method: "account/proxy/test".into(),
            params: None,
            trace: None,
        };
        let expected = crate::handle_request_with_actor(req.clone(), actor.clone());
        let actual = serde_json::to_value(
            try_handle_network_request_async(&req, &actor)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(actual, serde_json::to_value(expected).unwrap());
        assert_eq!(actual["result"]["errorCode"], "permission_denied");
    }
}
