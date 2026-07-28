use std::path::{Path, PathBuf};

use tauri::Manager;

use super::migration::maybe_migrate_legacy_db;

const ENV_DB_PATH: &str = "CODEXMANAGER_DB_PATH";
const ENV_RPC_TOKEN_FILE: &str = "CODEXMANAGER_RPC_TOKEN_FILE";
const ENV_SERVICE_ADDR: &str = "CODEXMANAGER_SERVICE_ADDR";
const QA_APP_IDENTIFIER: &str = "com.codexmanager.desktop.qa";
const QA_DEFAULT_SERVICE_ADDR: &str = "localhost:48762";

/// 函数 `resolve_rpc_token_path_for_db`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_rpc_token_path_for_db(db_path: &Path) -> PathBuf {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join("codexmanager.rpc-token")
}

/// 函数 `env_non_empty`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 函数 `exe_dir`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|value| value.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// 函数 `resolve_path_with_base`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
/// - base_dir: 参数 base_dir
///
/// # 返回
/// 返回函数执行结果
fn resolve_path_with_base(raw: &str, base_dir: &Path) -> PathBuf {
    let path = PathBuf::from(raw.trim());
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

/// 函数 `resolve_env_db_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn resolve_env_db_path() -> Option<PathBuf> {
    env_non_empty(ENV_DB_PATH).map(|raw| resolve_path_with_base(&raw, &exe_dir()))
}

/// 函数 `resolve_runtime_rpc_token_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - db_path: 参数 db_path
///
/// # 返回
/// 返回函数执行结果
fn resolve_runtime_rpc_token_path(db_path: &Path) -> PathBuf {
    env_non_empty(ENV_RPC_TOKEN_FILE)
        .map(|raw| resolve_path_with_base(&raw, &exe_dir()))
        .unwrap_or_else(|| resolve_rpc_token_path_for_db(db_path))
}

/// 函数 `default_app_data_db_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 返回函数执行结果
fn default_app_data_db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let mut data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| "app data dir not found".to_string())?;
    data_dir.push("codexmanager.db");
    Ok(data_dir)
}

/// 函数 `apply_runtime_storage_env`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn apply_runtime_storage_env(app: &tauri::AppHandle) {
    let data_path = match resolve_db_path_with_legacy_migration(app) {
        Ok(data_path) => data_path,
        Err(err) => {
            log::error!("event=runtime_storage_path_resolution_failed error={err}");
            return;
        }
    };
    std::env::set_var(ENV_DB_PATH, &data_path);
    let token_path = resolve_runtime_rpc_token_path(&data_path);
    std::env::set_var(ENV_RPC_TOKEN_FILE, &token_path);
    maybe_seed_profile_service_addr(app);
    log::info!(
        "event=runtime_storage_configured db_path={}",
        data_path.display()
    );
    log::debug!(
        "event=runtime_rpc_token_path_configured path={}",
        token_path.display()
    );
}

/// 函数 `profile_default_service_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - identifier: 参数 identifier
///
/// # 返回
/// 返回函数执行结果
fn profile_default_service_addr(identifier: &str) -> Option<&'static str> {
    let normalized = identifier.trim().to_ascii_lowercase();
    match normalized.as_str() {
        QA_APP_IDENTIFIER => Some(QA_DEFAULT_SERVICE_ADDR),
        _ => None,
    }
}

/// 函数 `should_seed_profile_service_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - identifier: 参数 identifier
/// - current_saved_addr: 参数 current_saved_addr
///
/// # 返回
/// 返回函数执行结果
fn should_seed_profile_service_addr(
    identifier: &str,
    current_saved_addr: &str,
) -> Option<&'static str> {
    let profile_addr = profile_default_service_addr(identifier)?;
    if current_saved_addr.eq_ignore_ascii_case(codexmanager_service::DEFAULT_ADDR) {
        Some(profile_addr)
    } else {
        None
    }
}

/// 函数 `maybe_seed_profile_service_addr`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - app: 参数 app
///
/// # 返回
/// 无
fn maybe_seed_profile_service_addr(app: &tauri::AppHandle) {
    if env_non_empty(ENV_SERVICE_ADDR).is_some() {
        return;
    }
    let identifier = app.config().identifier.as_str();
    let current_saved_addr = codexmanager_service::current_saved_service_addr();
    let Some(profile_addr) = should_seed_profile_service_addr(identifier, &current_saved_addr)
    else {
        return;
    };

    match codexmanager_service::set_saved_service_addr(Some(profile_addr)) {
        Ok(applied_addr) => {
            log::info!(
                "service addr profile migration: identifier={} {} -> {}",
                identifier,
                current_saved_addr,
                applied_addr
            );
        }
        Err(err) => {
            log::warn!(
                "service addr profile migration failed: identifier={} target={} error={}",
                identifier,
                profile_addr,
                err
            );
        }
    }
}

/// 函数 `resolve_db_path_with_legacy_migration`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_db_path_with_legacy_migration(
    app: &tauri::AppHandle,
) -> Result<PathBuf, String> {
    let data_path = resolve_env_db_path().unwrap_or(default_app_data_db_path(app)?);
    if let Some(parent) = data_path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            log::warn!("Failed to create db dir {}: {}", parent.display(), err);
        }
    }
    maybe_migrate_legacy_db(&data_path);
    Ok(data_path)
}

#[cfg(test)]
mod tests {
    use super::{
        profile_default_service_addr, resolve_path_with_base, resolve_runtime_rpc_token_path,
        should_seed_profile_service_addr, ENV_RPC_TOKEN_FILE, QA_DEFAULT_SERVICE_ADDR,
    };
    use std::path::{Path, PathBuf};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        /// 函数 `set`
        ///
        /// 作者: gaohongshun
        ///
        /// 时间: 2026-04-02
        ///
        /// # 参数
        /// - key: 参数 key
        /// - value: 参数 value
        ///
        /// # 返回
        /// 返回函数执行结果
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        /// 函数 `drop`
        ///
        /// 作者: gaohongshun
        ///
        /// 时间: 2026-04-02
        ///
        /// # 参数
        /// - self: 参数 self
        ///
        /// # 返回
        /// 无
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_deref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    /// 函数 `profile_default_service_addr_is_only_defined_for_qa_profile`
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
    fn profile_default_service_addr_is_only_defined_for_qa_profile() {
        assert_eq!(
            profile_default_service_addr("com.codexmanager.desktop.qa"),
            Some(QA_DEFAULT_SERVICE_ADDR)
        );
        assert_eq!(
            profile_default_service_addr(" COM.CODEXMANAGER.DESKTOP.QA "),
            Some(QA_DEFAULT_SERVICE_ADDR)
        );
        assert_eq!(
            profile_default_service_addr("com.codexmanager.desktop"),
            None
        );
    }

    /// 函数 `profile_service_addr_migration_only_applies_to_legacy_default_port`
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
    fn profile_service_addr_migration_only_applies_to_legacy_default_port() {
        assert_eq!(
            should_seed_profile_service_addr(
                "com.codexmanager.desktop.qa",
                codexmanager_service::DEFAULT_ADDR
            ),
            Some(QA_DEFAULT_SERVICE_ADDR)
        );
        assert_eq!(
            should_seed_profile_service_addr("com.codexmanager.desktop.qa", "localhost:48762"),
            None
        );
        assert_eq!(
            should_seed_profile_service_addr("com.codexmanager.desktop.qa", "localhost:4999"),
            None
        );
        assert_eq!(
            should_seed_profile_service_addr(
                "com.codexmanager.desktop",
                codexmanager_service::DEFAULT_ADDR
            ),
            None
        );
    }

    /// 函数 `resolve_path_with_base_uses_base_for_relative_paths`
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
    fn resolve_path_with_base_uses_base_for_relative_paths() {
        let resolved =
            resolve_path_with_base("./data/codexmanager.db", Path::new("D:/apps/CodexManager"));
        assert_eq!(
            resolved,
            PathBuf::from("D:/apps/CodexManager").join("./data/codexmanager.db")
        );
    }

    /// 函数 `runtime_rpc_token_path_prefers_env_relative_to_exe_dir`
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
    fn runtime_rpc_token_path_prefers_env_relative_to_exe_dir() {
        let _guard = EnvGuard::set(ENV_RPC_TOKEN_FILE, Some("./data/custom.rpc-token"));
        let db_path =
            PathBuf::from("C:/Users/test/AppData/Roaming/com.codexmanager.desktop/codexmanager.db");
        let expected = super::exe_dir().join("./data/custom.rpc-token");
        assert_eq!(resolve_runtime_rpc_token_path(&db_path), expected);
    }
}
