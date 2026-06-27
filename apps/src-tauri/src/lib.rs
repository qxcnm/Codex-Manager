use serde::Serialize;
use tauri::{Emitter, Manager};

mod app_shell;
mod app_storage;
mod commands;
mod rpc_client;
mod service_runtime;

use app_shell::{
    handle_main_window_event, handle_run_event, load_env_from_exe_dir, request_show_main_window,
    schedule_startup_main_window, setup_tray, sync_startup_window_state, CLOSE_TO_TRAY_ON_CLOSE,
    TRAY_AVAILABLE,
};

const USAGE_REFRESH_COMPLETED_EVENT: &str = "usage-refresh-completed";

#[derive(Clone, Serialize)]
struct UsageRefreshCompletedPayload {
    source: &'static str,
    processed: usize,
    total: usize,
    completed_at: i64,
}

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
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::all())
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
            load_env_from_exe_dir();
            app_storage::apply_runtime_storage_env(app.handle());
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .targets([tauri_plugin_log::Target::new(
                        tauri_plugin_log::TargetKind::LogDir { file_name: None },
                    )])
                    .build(),
            )?;
            if let Ok(log_dir) = app.path().app_log_dir() {
                log::info!("log dir: {}", log_dir.display());
            }
            let usage_refresh_event_app = app.handle().clone();
            codexmanager_service::set_usage_refresh_completed_handler(move |event| {
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
            sync_startup_window_state();
            schedule_startup_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            handle_main_window_event(window, event);
        })
        .invoke_handler(commands::invoke_handler!())
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        handle_run_event(app_handle, &event);
    });
}

#[cfg(test)]
#[path = "tests/lib_tests.rs"]
mod tests;
