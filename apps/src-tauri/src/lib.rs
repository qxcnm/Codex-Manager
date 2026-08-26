use serde::Serialize;
use tauri::{Emitter, Manager};

#[cfg(target_os = "linux")]
use std::sync::OnceLock;

mod app_shell;
mod app_storage;
mod commands;
mod desktop_diagnostics;
mod rpc_client;
mod service_runtime;

use app_shell::{
    handle_main_window_event, handle_run_event, load_env_from_exe_dir,
    refresh_tray_menu_after_usage_update, request_show_main_window, schedule_startup_main_window,
    setup_tray, sync_startup_window_state, sync_window_ui_mount_state, CLOSE_TO_TRAY_ON_CLOSE,
    TRAY_AVAILABLE,
};

const USAGE_REFRESH_COMPLETED_EVENT: &str = "usage-refresh-completed";
const ACCOUNT_TEST_EVENT: &str = "account-test-event";
#[cfg(target_os = "linux")]
const AYATANA_APPINDICATOR_LOG_DOMAIN: &str = "libayatana-appindicator";
#[cfg(target_os = "linux")]
const AYATANA_DEPRECATED_MESSAGE: &str =
    "libayatana-appindicator is deprecated. Please use libayatana-appindicator-glib in newly written code.";
#[cfg(target_os = "linux")]
static AYATANA_LOG_HANDLER_ID: OnceLock<glib::LogHandlerId> = OnceLock::new();

#[derive(Clone, Serialize)]
struct UsageRefreshCompletedPayload {
    source: &'static str,
    processed: usize,
    total: usize,
    completed_at: i64,
}

fn desktop_window_state_flags() -> tauri_plugin_window_state::StateFlags {
    tauri_plugin_window_state::StateFlags::all()
        .difference(tauri_plugin_window_state::StateFlags::VISIBLE)
}

#[cfg(target_os = "linux")]
fn is_known_ayatana_deprecation_notice(domain: Option<&str>, message: &str) -> bool {
    domain == Some(AYATANA_APPINDICATOR_LOG_DOMAIN)
        && message.trim() == AYATANA_DEPRECATED_MESSAGE
}

#[cfg(target_os = "linux")]
fn install_ayatana_deprecation_notice_filter() {
    let _ = AYATANA_LOG_HANDLER_ID.get_or_init(|| {
        glib::log_set_handler(
            Some(AYATANA_APPINDICATOR_LOG_DOMAIN),
            glib::LogLevels::LEVEL_WARNING,
            false,
            false,
            |domain, level, message| {
                if is_known_ayatana_deprecation_notice(domain, message) {
                    return;
                }
                glib::log_default_handler(domain, level, Some(message));
            },
        )
    });
}

#[cfg(not(target_os = "linux"))]
fn install_ayatana_deprecation_notice_filter() {}

/// 函数 `run`
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
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_ayatana_deprecation_notice_filter();

    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(desktop_window_state_flags())
                .build(),
        )
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("CodexManager")
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            if args.iter().any(|arg| arg.eq_ignore_ascii_case("--debug")) {
                desktop_diagnostics::force_debug_mode();
            }
            log::info!(
                "secondary instance intercepted; focusing main window (args: {:?}, cwd: {})",
                args,
                cwd
            );
            match request_show_main_window(app) {
                Ok(()) => {
                    log::info!("secondary instance focus request queued without blocking dialog");
                }
                Err(err) => {
                    log::warn!("secondary instance focus request skipped: {}", err);
                }
            }
        }))
        .setup(|app| {
            load_env_from_exe_dir();
            let diagnostics_settings = desktop_diagnostics::initialize_runtime(app.handle());
            if let Err(err) = app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Debug)
                    .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                    .max_file_size(512 * 1024)
                    .targets([tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::LogDir { file_name: None },
                    )
                    .filter(|metadata| {
                        desktop_diagnostics::should_write_file_log(metadata.level())
                    })])
                    .build(),
            ) {
                let message = format!("initialize desktop logging failed: {err}");
                desktop_diagnostics::report_startup_failure(
                    app.handle(),
                    "desktop-logging",
                    &message,
                    None,
                );
                return Err(std::io::Error::other(message).into());
            }
            if let Ok(log_dir) = app.path().app_log_dir() {
                log::info!("log dir: {}", log_dir.display());
            }
            log::info!(
                "desktop diagnostics initialized: debug={}, file_logging={}",
                diagnostics_settings.debug_mode,
                diagnostics_settings.file_logging_enabled
            );

            let database_path = match app_storage::apply_runtime_storage_env_checked(app.handle()) {
                Ok(path) => path,
                Err(err) => {
                    let message = format!("resolve desktop database path failed: {err}");
                    desktop_diagnostics::report_startup_failure(
                        app.handle(),
                        "storage-path",
                        &message,
                        None,
                    );
                    return Err(std::io::Error::other(message).into());
                }
            };
            let database_backup = match app_storage::create_pre_migration_backup(
                &database_path,
                env!("CARGO_PKG_VERSION"),
            ) {
                Ok(path) => path,
                Err(err) => {
                    let message = format!(
                        "create pre-migration database backup failed for {}: {err}",
                        database_path.display()
                    );
                    desktop_diagnostics::report_startup_failure(
                        app.handle(),
                        "database-backup",
                        &message,
                        None,
                    );
                    return Err(std::io::Error::other(message).into());
                }
            };
            if let Some(path) = database_backup.as_deref() {
                log::info!("pre-migration database backup: {}", path.display());
            }
            if let Err(err) = codexmanager_service::initialize_storage_if_needed() {
                let message = format!(
                    "database migration failed; refusing desktop startup: {err}"
                );
                log::error!("{message}");
                desktop_diagnostics::report_startup_failure(
                    app.handle(),
                    "database-migration",
                    &message,
                    database_backup.as_deref(),
                );
                return Err(std::io::Error::other(message).into());
            }
            log::debug!("desktop storage initialization completed");
            let usage_refresh_event_app = app.handle().clone();
            codexmanager_service::set_usage_refresh_completed_handler(move |event| {
                refresh_tray_menu_after_usage_update(&usage_refresh_event_app);
                let payload = UsageRefreshCompletedPayload {
                    source: event.source,
                    processed: event.processed,
                    total: event.total,
                    completed_at: event.completed_at,
                };
                if let Err(err) =
                    usage_refresh_event_app.emit(USAGE_REFRESH_COMPLETED_EVENT, payload)
                {
                    log::warn!("emit usage refresh completed event failed: {}", err);
                }
            });
            let account_test_event_app = app.handle().clone();
            codexmanager_service::set_account_test_event_handler(move |event| {
                if let Err(err) = account_test_event_app.emit(ACCOUNT_TEST_EVENT, &event) {
                    log::warn!("emit account test event failed: {}", err);
                }
            });
            if let Err(err) = setup_tray(app.handle()) {
                TRAY_AVAILABLE.store(false, std::sync::atomic::Ordering::Relaxed);
                CLOSE_TO_TRAY_ON_CLOSE.store(false, std::sync::atomic::Ordering::Relaxed);
                log::warn!("tray setup unavailable, continue without tray: {}", err);
            }
            codexmanager_service::sync_runtime_settings_from_storage();
            if let Err(err) =
                commands::settings::ui::sync_auto_start_runtime_state_from_settings(app.handle())
            {
                log::warn!(
                    "sync autostart state from persisted settings failed: {}",
                    err
                );
            }
            sync_startup_window_state();
            sync_window_ui_mount_state(app.handle());
            schedule_startup_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            handle_main_window_event(window, event);
        })
        .invoke_handler(commands::invoke_handler!())
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(err) => {
            let message = format!("error while building tauri application: {err}");
            desktop_diagnostics::report_build_failure(&message);
            return;
        }
    };

    app.run(|app_handle, event| {
        handle_run_event(app_handle, &event);
    });
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;
