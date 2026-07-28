use serde::Serialize;
use std::str::FromStr;
use tauri::{Emitter, Manager};

#[cfg(target_os = "linux")]
use std::sync::OnceLock;

mod app_shell;
mod app_storage;
mod commands;
mod rpc_client;
mod service_runtime;

use app_shell::{
    handle_main_window_event, handle_run_event, load_env_from_exe_dir,
    refresh_tray_menu_after_usage_update, request_show_main_window, schedule_startup_main_window,
    setup_tray, sync_startup_window_state, sync_window_ui_mount_state, CLOSE_TO_TRAY_ON_CLOSE,
    TRAY_AVAILABLE,
};

const USAGE_REFRESH_COMPLETED_EVENT: &str = "usage-refresh-completed";
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

fn configured_desktop_log_level() -> (log::LevelFilter, Option<String>) {
    let configured = std::env::var("CODEXMANAGER_LOG")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let level = configured
        .as_deref()
        .and_then(|value| log::LevelFilter::from_str(value).ok())
        .unwrap_or(log::LevelFilter::Info);
    (level, configured)
}

#[cfg(target_os = "linux")]
fn is_known_ayatana_deprecation_notice(domain: Option<&str>, message: &str) -> bool {
    domain == Some(AYATANA_APPINDICATOR_LOG_DOMAIN) && message.trim() == AYATANA_DEPRECATED_MESSAGE
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
                .with_state_flags(tauri_plugin_window_state::StateFlags::all())
                .build(),
        )
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("CodexManager")
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
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
            let env_log_events = load_env_from_exe_dir();
            let (log_level, configured_log_level) = configured_desktop_log_level();
            let log_dir = app.path().app_data_dir()?.join("logs");
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log_level)
                    .targets([tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::Folder {
                            path: log_dir.clone(),
                            file_name: None,
                        },
                    )])
                    .build(),
            )?;
            codexmanager_service::install_panic_log_hook();
            for (level, message) in env_log_events {
                log::log!(level, "{message}");
            }
            if let Some(configured) = configured_log_level {
                if log::LevelFilter::from_str(&configured).is_err() {
                    log::warn!(
                        "event=desktop_log_filter_invalid configured={} fallback=info reason=desktop_requires_single_level",
                        configured.replace(['\r', '\n'], " ")
                    );
                }
            }
            log::info!(
                "event=desktop_logging_ready level={} log_dir={}",
                log_level,
                log_dir.display()
            );
            app_storage::apply_runtime_storage_env(app.handle());
            codexmanager_service::initialize_storage_if_needed().map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("database migration failed; refusing desktop startup: {err}"),
                )
            })?;
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
        .build(tauri::generate_context!())
        .unwrap_or_else(|err| {
            log::error!("event=tauri_build_failed error={err}");
            eprintln!("event=tauri_build_failed error={err}");
            panic!("error while building tauri application: {err}");
        });

    app.run(|app_handle, event| {
        handle_run_event(app_handle, &event);
    });
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;
