use crate::commands::settings::sync_window_runtime_state_from_settings;

use super::state::TRAY_AVAILABLE;
use super::window::{navigate_main_window_to_startup_app, request_show_main_window};

#[cfg(debug_assertions)]
const DEV_SERVER_STARTUP_URL: &str = "http://127.0.0.1:3005/startup.html";
#[cfg(debug_assertions)]
const DEV_SERVER_READY_TIMEOUT_MS: u64 = 60_000;
#[cfg(debug_assertions)]
const DEV_SERVER_READY_INTERVAL_MS: u64 = 500;

/// 函数 `sync_startup_window_state`
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
pub(crate) fn sync_startup_window_state() {
    if let Ok(mut settings) = codexmanager_service::app_settings_get_with_overrides(
        Some(
            codexmanager_service::current_close_to_tray_on_close_setting()
                && TRAY_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed),
        ),
        Some(TRAY_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed)),
    ) {
        sync_window_runtime_state_from_settings(&mut settings);
    }
}

pub(crate) fn schedule_startup_main_window(app: &tauri::AppHandle) {
    let tray_available = TRAY_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed);
    let configured = codexmanager_service::current_show_main_window_on_startup_setting();
    if !should_show_main_window_on_startup(configured, tray_available) {
        log::info!("startup main window remains hidden by app setting");
        return;
    }

    let app = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("startup-main-window".to_string())
        .spawn(move || {
            if let Err(err) = request_show_main_window(&app) {
                log::warn!("startup show main window request failed: {}", err);
                return;
            }
            wait_for_startup_webview_content();
            navigate_main_window_to_startup_app_when_ready(&app);
        })
    {
        log::error!("event=startup_window_worker_spawn_failed error={err}");
    }
}

fn should_show_main_window_on_startup(configured: bool, tray_available: bool) -> bool {
    configured || !tray_available
}

fn navigate_main_window_to_startup_app_when_ready(app: &tauri::AppHandle) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() <= deadline {
        match navigate_main_window_to_startup_app(app) {
            Ok(()) => return,
            Err(err) => {
                log::debug!("startup app navigation is not ready yet: {}", err);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    log::warn!("startup app navigation timed out because main window was not ready");
}

#[cfg(debug_assertions)]
fn wait_for_startup_webview_content() {
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(DEV_SERVER_READY_TIMEOUT_MS);
    let client = match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            log::warn!("create dev server readiness client failed: {}", err);
            return;
        }
    };

    while std::time::Instant::now() <= deadline {
        match client.get(DEV_SERVER_STARTUP_URL).send() {
            Ok(response) if response.status().is_success() => {
                log::info!("dev server startup page is ready before app navigation");
                return;
            }
            Ok(response) => {
                log::debug!(
                    "dev server startup page is not ready yet: status={}",
                    response.status()
                );
            }
            Err(err) => {
                log::debug!("dev server startup page readiness probe failed: {}", err);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(
            DEV_SERVER_READY_INTERVAL_MS,
        ));
    }

    log::warn!(
        "dev server startup page readiness timed out after {}ms; navigating app anyway",
        DEV_SERVER_READY_TIMEOUT_MS
    );
}

#[cfg(not(debug_assertions))]
fn wait_for_startup_webview_content() {}

#[cfg(test)]
mod tests {
    use super::should_show_main_window_on_startup;

    #[test]
    fn startup_window_respects_setting_when_tray_is_available() {
        assert!(should_show_main_window_on_startup(true, true));
        assert!(!should_show_main_window_on_startup(false, true));
    }

    #[test]
    fn startup_window_is_shown_when_tray_is_unavailable() {
        assert!(should_show_main_window_on_startup(false, false));
    }
}
