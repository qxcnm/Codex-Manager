use codexmanager_core::storage::{now_ts, Storage};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

mod support;
use support::{test_env_guard, EnvGuard};

/// 函数 `unique_temp_db_path`
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
fn unique_temp_db_path() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("codexmanager-service-test-{unique}.db"))
}

/// 函数 `with_bind_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - mode: 参数 mode
/// - test: 参数 test
///
/// # 返回
/// 无
fn with_bind_mode(mode: Option<&str>, test: impl FnOnce()) {
    let _guard = test_env_guard();
    let db_path = unique_temp_db_path();
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    if let Some(mode) = mode {
        storage
            .set_app_setting(
                codexmanager_service::SERVICE_BIND_MODE_SETTING_KEY,
                mode,
                now_ts(),
            )
            .expect("set service bind mode");
    }
    drop(storage);

    test();

    let _ = std::fs::remove_file(&db_path);
}

/// 函数 `default_addr_is_localhost`
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
fn default_addr_is_localhost() {
    assert_eq!(codexmanager_service::DEFAULT_ADDR, "localhost:48760");
}

/// 函数 `default_bind_addr_is_all_interfaces`
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
fn default_bind_addr_is_all_interfaces() {
    assert_eq!(codexmanager_service::DEFAULT_BIND_ADDR, "0.0.0.0:48760");
}

/// 函数 `default_web_addr_is_localhost`
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
fn default_web_addr_is_localhost() {
    assert_eq!(codexmanager_service::DEFAULT_WEB_ADDR, "localhost:48761");
}

/// 函数 `default_web_bind_addr_is_all_interfaces`
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
fn default_web_bind_addr_is_all_interfaces() {
    assert_eq!(codexmanager_service::DEFAULT_WEB_BIND_ADDR, "0.0.0.0:48761");
}

/// 函数 `listener_bind_addr_defaults_to_loopback`
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
fn listener_bind_addr_defaults_to_loopback() {
    with_bind_mode(None, || {
        assert_eq!(
            codexmanager_service::default_listener_bind_addr(),
            "localhost:48760"
        );
        assert_eq!(
            codexmanager_service::listener_bind_addr("localhost:48760"),
            "localhost:48760"
        );
        assert_eq!(
            codexmanager_service::listener_bind_addr("127.0.0.1:48760"),
            "localhost:48760"
        );
    });
}

/// 函数 `listener_bind_addr_maps_loopback_to_all_interfaces_when_enabled`
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
fn listener_bind_addr_maps_loopback_to_all_interfaces_when_enabled() {
    with_bind_mode(
        Some(codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES),
        || {
            assert_eq!(
                codexmanager_service::default_listener_bind_addr(),
                "0.0.0.0:48760"
            );
            assert_eq!(
                codexmanager_service::listener_bind_addr("localhost:48760"),
                "0.0.0.0:48760"
            );
            assert_eq!(
                codexmanager_service::listener_bind_addr("127.0.0.1:48760"),
                "0.0.0.0:48760"
            );
        },
    );
}

/// 函数 `default_web_listener_addr_tracks_service_bind_mode`
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
fn default_web_listener_addr_tracks_service_bind_mode() {
    with_bind_mode(None, || {
        assert_eq!(
            codexmanager_service::default_web_listener_addr(),
            "localhost:48761"
        );
    });

    with_bind_mode(
        Some(codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES),
        || {
            assert_eq!(
                codexmanager_service::default_web_listener_addr(),
                "0.0.0.0:48761"
            );
        },
    );
}

/// 函数 `default_web_listener_addr_tracks_service_port_offset`
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
fn default_web_listener_addr_tracks_service_port_offset() {
    let _guard = test_env_guard();
    let _service_addr_guard = EnvGuard::set("CODEXMANAGER_SERVICE_ADDR", "localhost:49760");

    let db_path = unique_temp_db_path();
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let storage = Storage::open(&db_path).expect("open storage");
    storage.init().expect("init storage");
    storage
        .set_app_setting(
            codexmanager_service::SERVICE_BIND_MODE_SETTING_KEY,
            codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES,
            now_ts(),
        )
        .expect("set service bind mode");
    drop(storage);

    assert_eq!(
        codexmanager_service::default_web_listener_addr(),
        "0.0.0.0:49761"
    );

    let _ = std::fs::remove_file(&db_path);
}

/// 函数 `listener_bind_addr_keeps_explicit_all_interfaces`
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
fn listener_bind_addr_keeps_explicit_all_interfaces() {
    with_bind_mode(None, || {
        assert_eq!(
            codexmanager_service::listener_bind_addr("0.0.0.0:48760"),
            "0.0.0.0:48760"
        );
        assert_eq!(
            codexmanager_service::listener_bind_addr("192.168.1.10:48760"),
            "192.168.1.10:48760"
        );
    });
}

/// 函数 `current_service_bind_mode_prefers_runtime_env`
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
fn current_service_bind_mode_prefers_runtime_env() {
    with_bind_mode(
        Some(codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES),
        || {
            std::env::set_var("CODEXMANAGER_SERVICE_ADDR", "localhost:48760");
            assert_eq!(
                codexmanager_service::current_service_bind_mode(),
                codexmanager_service::SERVICE_BIND_MODE_LOOPBACK
            );
            std::env::set_var("CODEXMANAGER_SERVICE_ADDR", "0.0.0.0:48760");
            assert_eq!(
                codexmanager_service::current_service_bind_mode(),
                codexmanager_service::SERVICE_BIND_MODE_ALL_INTERFACES
            );
            std::env::remove_var("CODEXMANAGER_SERVICE_ADDR");
        },
    );
}
