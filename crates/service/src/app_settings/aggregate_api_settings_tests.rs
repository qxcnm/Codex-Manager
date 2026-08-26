use super::{
    current_aggregate_api_probe_user_agent, current_aggregate_api_probe_user_agent_mode,
    set_aggregate_api_probe_user_agent_settings, AGGREGATE_API_PROBE_USER_AGENT_MODE_CODEX,
    AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM,
};
use crate::app_settings::{
    APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_KEY,
    APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_MODE_KEY,
};
use codexmanager_core::storage::Storage;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct EnvGuard {
    previous: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_db_path(path: &PathBuf) -> Self {
        let previous = std::env::var_os("CODEXMANAGER_DB_PATH");
        std::env::set_var("CODEXMANAGER_DB_PATH", path);
        Self { previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = self.previous.as_ref() {
            std::env::set_var("CODEXMANAGER_DB_PATH", value);
        } else {
            std::env::remove_var("CODEXMANAGER_DB_PATH");
        }
    }
}

fn unique_temp_db_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("codexmanager-aggregate-probe-settings-{unique}.db"))
}

#[test]
fn aggregate_api_probe_settings_default_to_codex_profile() {
    let _guard = crate::test_env_guard();
    let db_path = unique_temp_db_path();
    let _db_env = EnvGuard::set_db_path(&db_path);

    assert_eq!(
        current_aggregate_api_probe_user_agent_mode(),
        AGGREGATE_API_PROBE_USER_AGENT_MODE_CODEX
    );
    assert_eq!(current_aggregate_api_probe_user_agent(), "");
}

#[test]
fn aggregate_api_probe_settings_validate_and_persist_custom_user_agent() {
    let _guard = crate::test_env_guard();
    let db_path = unique_temp_db_path();
    let _db_env = EnvGuard::set_db_path(&db_path);
    crate::initialize_storage_if_needed().expect("init storage");

    let missing = set_aggregate_api_probe_user_agent_settings(
        Some(AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM),
        Some("  "),
    )
    .expect_err("custom mode requires user agent");
    assert!(missing.contains("custom user agent is required"));

    let result = set_aggregate_api_probe_user_agent_settings(
        Some(AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM),
        Some("Codex-Probe-Test/1.0"),
    )
    .expect("persist custom probe settings");
    assert_eq!(
        result,
        (
            AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM.to_string(),
            "Codex-Probe-Test/1.0".to_string()
        )
    );

    let storage = Storage::open(&db_path).expect("open storage");
    assert_eq!(
        storage
            .get_app_setting(APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_MODE_KEY)
            .expect("read mode"),
        Some(AGGREGATE_API_PROBE_USER_AGENT_MODE_CUSTOM.to_string())
    );
    assert_eq!(
        storage
            .get_app_setting(APP_SETTING_AGGREGATE_API_PROBE_USER_AGENT_KEY)
            .expect("read user agent"),
        Some("Codex-Probe-Test/1.0".to_string())
    );
    let _ = std::fs::remove_file(db_path);
}
