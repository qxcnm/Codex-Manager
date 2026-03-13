use codexmanager_core::storage::{now_ts, Storage};
use serde_json::json;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn unique_temp_db_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("codexmanager-app-settings-test-{unique}.db"))
}

fn reset_runtime_defaults() {
    let _ = codexmanager_service::set_service_bind_mode(
        codexmanager_service::SERVICE_BIND_MODE_LOOPBACK,
    );
    let _ = codexmanager_service::app_settings_set(Some(&json!({
        "routeStrategy": "balanced",
        "lightweightModeOnCloseToTray": false,
        "cpaNoCookieHeaderModeEnabled": false,
        "upstreamProxyUrl": "",
        "upstreamStreamTimeoutMs": 1800000,
        "sseKeepaliveIntervalMs": 15000,
        "envOverrides": {},
        "backgroundTasks": {
            "usagePollingEnabled": true,
            "usagePollIntervalSecs": 600,
            "gatewayKeepaliveEnabled": true,
            "gatewayKeepaliveIntervalSecs": 180,
            "tokenRefreshPollingEnabled": true,
            "tokenRefreshPollIntervalSecs": 60,
            "usageRefreshWorkers": 4,
            "httpWorkerFactor": 4,
            "httpWorkerMin": 8,
            "httpStreamWorkerFactor": 1,
            "httpStreamWorkerMin": 2
        }
    })));
}

fn with_temp_db(test: impl FnOnce(&PathBuf)) {
    let _guard = env_lock().lock().expect("env lock");
    let db_path = unique_temp_db_path();
    let previous_db_path = std::env::var("CODEXMANAGER_DB_PATH").ok();
    std::env::set_var("CODEXMANAGER_DB_PATH", &db_path);
    codexmanager_service::initialize_storage_if_needed().expect("init storage");
    reset_runtime_defaults();

    test(&db_path);

    reset_runtime_defaults();
    if let Some(value) = previous_db_path {
        std::env::set_var("CODEXMANAGER_DB_PATH", value);
    } else {
        std::env::remove_var("CODEXMANAGER_DB_PATH");
    }
    let _ = std::fs::remove_file(&db_path);
}

struct EnvRestore(Vec<(String, Option<OsString>)>);

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            if let Some(value) = value {
                std::env::set_var(&key, value);
            } else {
                std::env::remove_var(&key);
            }
        }
    }
}

fn override_env_vars(vars: &[(&str, Option<&str>)]) -> EnvRestore {
    let previous = vars
        .iter()
        .map(|(key, _)| ((*key).to_string(), std::env::var_os(key)))
        .collect::<Vec<_>>();
    for (key, value) in vars {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
    EnvRestore(previous)
}

fn read_env_overrides_map(db_path: &PathBuf) -> serde_json::Map<String, serde_json::Value> {
    let storage = Storage::open(db_path).expect("open storage");
    let raw = storage
        .get_app_setting(codexmanager_service::APP_SETTING_ENV_OVERRIDES_KEY)
        .expect("read env overrides")
        .expect("env overrides exists");
    serde_json::from_str(&raw).expect("parse env overrides json")
}

#[test]
fn sync_runtime_settings_from_storage_preserves_process_env_when_override_not_persisted() {
    with_temp_db(|db_path| {
        let storage = Storage::open(db_path).expect("open storage");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_ENV_OVERRIDES_KEY,
                "",
                now_ts(),
            )
            .expect("clear env overrides");
        drop(storage);

        let _env = override_env_vars(&[(
            "CODEXMANAGER_UPSTREAM_BASE_URL",
            Some("http://127.0.0.1:41002"),
        )]);

        codexmanager_service::sync_runtime_settings_from_storage();

        assert_eq!(
            std::env::var("CODEXMANAGER_UPSTREAM_BASE_URL")
                .ok()
                .as_deref(),
            Some("http://127.0.0.1:41002")
        );
    });
}

#[test]
fn sync_runtime_settings_from_storage_preserves_explicit_process_env_over_persisted_override() {
    with_temp_db(|db_path| {
        let storage = Storage::open(db_path).expect("open storage");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_ENV_OVERRIDES_KEY,
                &serde_json::to_string(&json!({
                    "CODEXMANAGER_WEB_ADDR": "localhost:48761"
                }))
                .expect("serialize env overrides"),
                now_ts(),
            )
            .expect("save env overrides");
        drop(storage);

        let _env = override_env_vars(&[("CODEXMANAGER_WEB_ADDR", Some("0.0.0.0:48761"))]);

        codexmanager_service::sync_runtime_settings_from_storage();

        assert_eq!(
            std::env::var("CODEXMANAGER_WEB_ADDR").ok().as_deref(),
            Some("0.0.0.0:48761")
        );
    });
}

#[test]
fn app_settings_set_persists_snapshot_and_password_hash() {
    with_temp_db(|db_path| {
        let snapshot = codexmanager_service::app_settings_set(Some(&json!({
            "updateAutoCheck": false,
            "closeToTrayOnClose": true,
            "lightweightModeOnCloseToTray": true,
            "lowTransparency": true,
            "theme": "dark",
            "serviceAddr": "127.0.0.1:4999",
            "serviceListenMode": "all_interfaces",
            "routeStrategy": "rr",
            "cpaNoCookieHeaderModeEnabled": true,
            "upstreamProxyUrl": "http://127.0.0.1:7890",
            "upstreamStreamTimeoutMs": 654321,
            "sseKeepaliveIntervalMs": 17000,
            "backgroundTasks": {
                "usagePollingEnabled": false,
                "usagePollIntervalSecs": 900,
                "gatewayKeepaliveEnabled": false,
                "gatewayKeepaliveIntervalSecs": 240,
                "tokenRefreshPollingEnabled": true,
                "tokenRefreshPollIntervalSecs": 120,
                "usageRefreshWorkers": 6,
                "httpWorkerFactor": 5,
                "httpWorkerMin": 9,
                "httpStreamWorkerFactor": 2,
                "httpStreamWorkerMin": 3
            },
            "webAccessPassword": "secret-pass"
        })))
        .expect("save app settings");

        assert_eq!(
            snapshot
                .get("updateAutoCheck")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            snapshot
                .get("closeToTrayOnClose")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            snapshot
                .get("lightweightModeOnCloseToTray")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            snapshot.get("theme").and_then(|value| value.as_str()),
            Some("dark")
        );
        assert_eq!(
            snapshot
                .get("serviceListenMode")
                .and_then(|value| value.as_str()),
            Some(codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES)
        );
        assert_eq!(
            snapshot
                .get("upstreamStreamTimeoutMs")
                .and_then(|value| value.as_u64()),
            Some(654321)
        );
        assert_eq!(
            snapshot
                .get("sseKeepaliveIntervalMs")
                .and_then(|value| value.as_u64()),
            Some(17000)
        );
        assert_eq!(
            snapshot
                .get("routeStrategy")
                .and_then(|value| value.as_str()),
            Some("balanced")
        );
        assert_eq!(
            snapshot
                .get("webAccessPasswordConfigured")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert!(codexmanager_service::verify_web_access_password(
            "secret-pass"
        ));

        let storage = Storage::open(db_path).expect("open storage");
        assert_eq!(
            storage
                .get_app_setting(
                    codexmanager_service::APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY
                )
                .expect("read lightweight close to tray"),
            Some("1".to_string())
        );
        assert_eq!(
            storage
                .get_app_setting(
                    codexmanager_service::APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY
                )
                .expect("read upstream stream timeout"),
            Some("654321".to_string())
        );
        assert_eq!(
            storage
                .get_app_setting(
                    codexmanager_service::APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY
                )
                .expect("read sse keepalive interval"),
            Some("17000".to_string())
        );
        let stored_password = storage
            .get_app_setting(codexmanager_service::APP_SETTING_WEB_ACCESS_PASSWORD_HASH_KEY)
            .expect("read password hash");
        assert!(stored_password
            .as_deref()
            .is_some_and(|value| value.starts_with("sha256$")));
    });
}

#[test]
fn sync_runtime_settings_from_storage_applies_saved_runtime_values() {
    with_temp_db(|db_path| {
        let storage = Storage::open(db_path).expect("open storage");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
                "balanced",
                now_ts(),
            )
            .expect("save route strategy");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY,
                "1",
                now_ts(),
            )
            .expect("save cpa mode");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
                "http://127.0.0.1:8899",
                now_ts(),
            )
            .expect("save upstream proxy");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
                "456789",
                now_ts(),
            )
            .expect("save upstream stream timeout");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
                "19000",
                now_ts(),
            )
            .expect("save sse keepalive interval");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
                &serde_json::to_string(&json!({
                    "usagePollingEnabled": false,
                    "usagePollIntervalSecs": 777,
                    "gatewayKeepaliveEnabled": true,
                    "gatewayKeepaliveIntervalSecs": 180,
                    "tokenRefreshPollingEnabled": true,
                    "tokenRefreshPollIntervalSecs": 60,
                    "usageRefreshWorkers": 4,
                    "httpWorkerFactor": 4,
                    "httpWorkerMin": 8,
                    "httpStreamWorkerFactor": 1,
                    "httpStreamWorkerMin": 2
                }))
                .expect("serialize background tasks"),
                now_ts(),
            )
            .expect("save background tasks");
        storage
            .set_app_setting(
                codexmanager_service::APP_SETTING_ENV_OVERRIDES_KEY,
                &serde_json::to_string(&json!({
                    "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS": "654321"
                }))
                .expect("serialize env overrides"),
                now_ts(),
            )
            .expect("save env overrides");
        drop(storage);

        codexmanager_service::sync_runtime_settings_from_storage();

        let snapshot =
            codexmanager_service::app_settings_get().expect("get app settings after sync");
        assert_eq!(
            snapshot
                .get("routeStrategy")
                .and_then(|value| value.as_str()),
            Some("balanced")
        );
        assert_eq!(
            snapshot
                .get("cpaNoCookieHeaderModeEnabled")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            snapshot
                .get("upstreamProxyUrl")
                .and_then(|value| value.as_str()),
            Some("http://127.0.0.1:8899")
        );
        assert_eq!(
            snapshot
                .get("upstreamStreamTimeoutMs")
                .and_then(|value| value.as_u64()),
            Some(456789)
        );
        assert_eq!(
            snapshot
                .get("sseKeepaliveIntervalMs")
                .and_then(|value| value.as_u64()),
            Some(19000)
        );
        assert_eq!(
            snapshot
                .get("backgroundTasks")
                .and_then(|value| value.get("usagePollingEnabled"))
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            snapshot
                .get("backgroundTasks")
                .and_then(|value| value.get("usagePollIntervalSecs"))
                .and_then(|value| value.as_u64()),
            Some(777)
        );
        assert_eq!(
            snapshot
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS"))
                .and_then(|value| value.as_str()),
            Some("654321")
        );
        assert_eq!(
            std::env::var("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS")
                .ok()
                .as_deref(),
            Some("654321")
        );
    });
}

#[test]
fn app_settings_get_loads_env_backed_dedicated_settings_when_storage_missing() {
    with_temp_db(|db_path| {
        let storage = Storage::open(db_path).expect("open storage");
        for key in [
            codexmanager_service::APP_SETTING_SERVICE_ADDR_KEY,
            codexmanager_service::SERVICE_BIND_MODE_SETTING_KEY,
            codexmanager_service::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
            codexmanager_service::APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY,
            codexmanager_service::APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
            codexmanager_service::APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
            codexmanager_service::APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
            codexmanager_service::APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
        ] {
            storage.delete_app_setting(key).expect("delete app setting");
        }
        drop(storage);

        let _env = override_env_vars(&[
            ("CODEXMANAGER_SERVICE_ADDR", Some("0.0.0.0:4999")),
            ("CODEXMANAGER_ROUTE_STRATEGY", Some("balanced")),
            ("CODEXMANAGER_CPA_NO_COOKIE_HEADER_MODE", Some("1")),
            (
                "CODEXMANAGER_UPSTREAM_PROXY_URL",
                Some("http://127.0.0.1:7899"),
            ),
            ("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS", Some("432100")),
            ("CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS", Some("14000")),
            ("CODEXMANAGER_USAGE_POLLING_ENABLED", Some("0")),
            ("CODEXMANAGER_USAGE_POLL_INTERVAL_SECS", Some("777")),
            ("CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED", Some("0")),
            ("CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS", Some("240")),
            ("CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED", Some("0")),
            ("CODEXMANAGER_TOKEN_REFRESH_POLL_INTERVAL_SECS", Some("120")),
            ("CODEXMANAGER_USAGE_REFRESH_WORKERS", Some("6")),
            ("CODEXMANAGER_HTTP_WORKER_FACTOR", Some("5")),
            ("CODEXMANAGER_HTTP_WORKER_MIN", Some("9")),
            ("CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR", Some("2")),
            ("CODEXMANAGER_HTTP_STREAM_WORKER_MIN", Some("3")),
        ]);

        let snapshot = codexmanager_service::app_settings_get().expect("get app settings");

        assert_eq!(
            snapshot.get("serviceAddr").and_then(|value| value.as_str()),
            Some("localhost:4999")
        );
        assert_eq!(
            snapshot
                .get("serviceListenMode")
                .and_then(|value| value.as_str()),
            Some(codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES)
        );
        assert_eq!(
            snapshot
                .get("routeStrategy")
                .and_then(|value| value.as_str()),
            Some("balanced")
        );
        assert_eq!(
            snapshot
                .get("cpaNoCookieHeaderModeEnabled")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            snapshot
                .get("upstreamProxyUrl")
                .and_then(|value| value.as_str()),
            Some("http://127.0.0.1:7899")
        );
        assert_eq!(
            snapshot
                .get("upstreamStreamTimeoutMs")
                .and_then(|value| value.as_u64()),
            Some(432100)
        );
        assert_eq!(
            snapshot
                .get("sseKeepaliveIntervalMs")
                .and_then(|value| value.as_u64()),
            Some(14000)
        );
        assert_eq!(
            snapshot
                .get("backgroundTasks")
                .and_then(|value| value.get("usagePollingEnabled"))
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            snapshot
                .get("backgroundTasks")
                .and_then(|value| value.get("usagePollIntervalSecs"))
                .and_then(|value| value.as_u64()),
            Some(777)
        );
        assert_eq!(
            snapshot
                .get("backgroundTasks")
                .and_then(|value| value.get("gatewayKeepaliveEnabled"))
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            snapshot
                .get("backgroundTasks")
                .and_then(|value| value.get("tokenRefreshPollIntervalSecs"))
                .and_then(|value| value.as_u64()),
            Some(120)
        );

        let storage = Storage::open(db_path).expect("open storage");
        assert_eq!(
            storage
                .get_app_setting(codexmanager_service::APP_SETTING_SERVICE_ADDR_KEY)
                .expect("read service addr"),
            Some("localhost:4999".to_string())
        );
        assert_eq!(
            storage
                .get_app_setting(codexmanager_service::SERVICE_BIND_MODE_SETTING_KEY)
                .expect("read service bind mode"),
            Some(codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES.to_string())
        );
        assert_eq!(
            storage
                .get_app_setting(codexmanager_service::APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY)
                .expect("read route strategy"),
            Some("balanced".to_string())
        );
        assert_eq!(
            storage
                .get_app_setting(
                    codexmanager_service::APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY
                )
                .expect("read upstream stream timeout"),
            Some("432100".to_string())
        );
        assert_eq!(
            storage
                .get_app_setting(
                    codexmanager_service::APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY
                )
                .expect("read sse keepalive interval"),
            Some("14000".to_string())
        );
    });
}

#[test]
fn app_settings_set_persists_env_overrides_and_exposes_catalog() {
    with_temp_db(|db_path| {
        let snapshot = codexmanager_service::app_settings_set(Some(&json!({
            "envOverrides": {
                "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS": "321000",
                "CODEXMANAGER_UPSTREAM_COOKIE": "cf_clearance=test"
            }
        })))
        .expect("save env overrides");

        assert_eq!(
            snapshot
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS"))
                .and_then(|value| value.as_str()),
            Some("321000")
        );
        assert_eq!(
            snapshot
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_UPSTREAM_COOKIE"))
                .and_then(|value| value.as_str()),
            Some("cf_clearance=test")
        );
        assert_eq!(
            snapshot
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_LOGIN_ADDR"))
                .and_then(|value| value.as_str()),
            Some("localhost:1455")
        );
        assert_eq!(
            std::env::var("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS")
                .ok()
                .as_deref(),
            Some("321000")
        );
        let catalog = snapshot
            .get("envOverrideCatalog")
            .and_then(|value| value.as_array())
            .expect("catalog array");
        let total_timeout = catalog
            .iter()
            .find(|item| {
                item.get("key").and_then(|value| value.as_str())
                    == Some("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS")
            })
            .expect("catalog item");
        assert_eq!(
            total_timeout.get("label").and_then(|value| value.as_str()),
            Some("上游总超时（毫秒）")
        );
        assert_eq!(
            total_timeout
                .get("defaultValue")
                .and_then(|value| value.as_str()),
            Some("120000")
        );
        let stream_timeout = catalog
            .iter()
            .find(|item| {
                item.get("key").and_then(|value| value.as_str())
                    == Some("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS")
            })
            .expect("stream timeout item");
        assert_eq!(
            stream_timeout
                .get("defaultValue")
                .and_then(|value| value.as_str()),
            Some("1800000")
        );
        assert!(snapshot
            .get("envOverrideReservedKeys")
            .and_then(|value| value.as_array())
            .is_some_and(|items| items
                .iter()
                .any(|item| item.as_str() == Some("CODEXMANAGER_ROUTE_STRATEGY"))));
        assert!(snapshot
            .get("envOverrideReservedKeys")
            .and_then(|value| value.as_array())
            .is_some_and(|items| items
                .iter()
                .any(|item| item.as_str() == Some("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS"))));

        let stored = read_env_overrides_map(db_path);
        assert_eq!(
            stored
                .get("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS")
                .and_then(|value| value.as_str()),
            Some("321000")
        );
        assert_eq!(
            stored
                .get("CODEXMANAGER_UPSTREAM_COOKIE")
                .and_then(|value| value.as_str()),
            Some("cf_clearance=test")
        );
        assert_eq!(
            stored
                .get("CODEXMANAGER_LOGIN_ADDR")
                .and_then(|value| value.as_str()),
            Some("localhost:1455")
        );
        assert!(stored.len() >= 40);
    });
}

#[test]
fn app_settings_get_seeds_full_env_override_snapshot() {
    with_temp_db(|db_path| {
        std::env::remove_var("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS");
        std::env::remove_var("CODEXMANAGER_WEB_ROOT");

        let snapshot = codexmanager_service::app_settings_get().expect("get app settings");

        assert_eq!(
            snapshot
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS"))
                .and_then(|value| value.as_str()),
            Some("120000")
        );
        assert_eq!(
            snapshot
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_WEB_ROOT"))
                .and_then(|value| value.as_str()),
            Some("")
        );

        let stored = read_env_overrides_map(db_path);
        assert_eq!(
            stored
                .get("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS")
                .and_then(|value| value.as_str()),
            Some("120000")
        );
        assert_eq!(
            stored
                .get("CODEXMANAGER_WEB_ROOT")
                .and_then(|value| value.as_str()),
            Some("")
        );
    });
}

#[test]
fn app_settings_set_env_overrides_patch_preserves_other_values_and_reset_to_default() {
    with_temp_db(|_| {
        let first = codexmanager_service::app_settings_set(Some(&json!({
            "envOverrides": {
                "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS": "321000",
                "CODEXMANAGER_UPSTREAM_COOKIE": "cf_clearance=test"
            }
        })))
        .expect("save first env overrides");
        assert_eq!(
            first
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_UPSTREAM_COOKIE"))
                .and_then(|value| value.as_str()),
            Some("cf_clearance=test")
        );

        let second = codexmanager_service::app_settings_set(Some(&json!({
            "envOverrides": {
                "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS": ""
            }
        })))
        .expect("reset timeout to default");

        assert_eq!(
            second
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS"))
                .and_then(|value| value.as_str()),
            Some("120000")
        );
        assert_eq!(
            second
                .get("envOverrides")
                .and_then(|value| value.get("CODEXMANAGER_UPSTREAM_COOKIE"))
                .and_then(|value| value.as_str()),
            Some("cf_clearance=test")
        );
        assert_eq!(
            std::env::var("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS")
                .ok()
                .as_deref(),
            Some("120000")
        );
        assert_eq!(
            std::env::var("CODEXMANAGER_UPSTREAM_COOKIE")
                .ok()
                .as_deref(),
            Some("cf_clearance=test")
        );
    });
}

#[test]
fn app_settings_set_rejects_reserved_and_bootstrap_env_override_keys() {
    with_temp_db(|_| {
        let reserved = codexmanager_service::app_settings_set(Some(&json!({
            "envOverrides": {
                "CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS": "123456"
            }
        })));
        assert!(reserved.is_err());

        let bootstrap = codexmanager_service::app_settings_set(Some(&json!({
            "envOverrides": {
                "CODEXMANAGER_DB_PATH": "D:/tmp/other.db"
            }
        })));
        assert!(bootstrap.is_err());
    });
}
