#![allow(dead_code)]

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, OnceLock};

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// 函数 `test_env_guard`
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
pub fn test_env_guard() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
    test_db_dir_original: Option<Option<OsString>>,
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
    /// 返回函数执行 erosion 结果
    pub fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);

        let mut test_db_dir_original = None;
        if key == "CODEXMANAGER_DB_PATH" {
            if let Some(parent) = std::path::Path::new(value).parent() {
                test_db_dir_original = Some(std::env::var_os("CODEXMANAGER_TEST_DB_DIR"));
                std::env::set_var("CODEXMANAGER_TEST_DB_DIR", parent);
            }
        }

        Self {
            key,
            original,
            test_db_dir_original,
        }
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
        if let Some(value) = &self.original {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }

        if let Some(extra) = &self.test_db_dir_original {
            if let Some(value) = extra {
                std::env::set_var("CODEXMANAGER_TEST_DB_DIR", value);
            } else {
                std::env::remove_var("CODEXMANAGER_TEST_DB_DIR");
            }
        }
    }
}

