use std::sync::atomic::Ordering;

use tauri::Manager;
use tauri_plugin_window_state::AppHandleExt;

use crate::service_runtime::stop_service;

use super::prompts::{
    confirm_discard_unsaved_settings_for_app_exit,
    confirm_discard_unsaved_settings_for_window_close,
};
use super::state::{
    clear_skip_next_unsaved_settings_confirms, has_unsaved_settings_draft_sections,
    mark_skip_next_unsaved_settings_exit_confirm,
    mark_skip_next_unsaved_settings_window_close_confirm, should_keep_alive_for_lightweight_close,
    take_skip_next_unsaved_settings_exit_confirm,
    take_skip_next_unsaved_settings_window_close_confirm, APP_EXIT_REQUESTED,
    CLOSE_TO_TRAY_ON_CLOSE, KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE, LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY,
    TRAY_AVAILABLE,
};
#[cfg(target_os = "macos")]
use super::window::request_show_main_window;
use super::window::{hide_tray_preview_window, MAIN_WINDOW_LABEL, TRAY_PREVIEW_WINDOW_LABEL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainWindowCloseMode {
    AllowWindowClose,
    HideToTray,
    CloseForLightweightTray,
}

/// 函数 `resolve_main_window_close_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - close_to_tray_on_close: 参数 close_to_tray_on_close
/// - tray_available: 参数 tray_available
/// - lightweight_tray_close: 参数 lightweight_tray_close
///
/// # 返回
/// 返回函数执行结果
fn resolve_main_window_close_mode(
    close_to_tray_on_close: bool,
    tray_available: bool,
    lightweight_tray_close: bool,
) -> MainWindowCloseMode {
    if !close_to_tray_on_close || !tray_available {
        return MainWindowCloseMode::AllowWindowClose;
    }
    if cfg!(target_os = "windows") || lightweight_tray_close {
        return MainWindowCloseMode::CloseForLightweightTray;
    }
    MainWindowCloseMode::HideToTray
}

/// 函数 `should_confirm_unsaved_settings_before_window_close`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - close_mode: 参数 close_mode
/// - has_unsaved_settings: 参数 has_unsaved_settings
///
/// # 返回
/// 返回函数执行结果
fn should_confirm_unsaved_settings_before_window_close(
    close_mode: MainWindowCloseMode,
    has_unsaved_settings: bool,
) -> bool {
    has_unsaved_settings && close_mode != MainWindowCloseMode::HideToTray
}

/// 函数 `should_confirm_unsaved_settings_before_app_exit`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - keep_alive_for_lightweight_close: 参数 keep_alive_for_lightweight_close
/// - skip_unsaved_settings_confirm: 参数 skip_unsaved_settings_confirm
/// - has_unsaved_settings: 参数 has_unsaved_settings
///
/// # 返回
/// 返回函数执行结果
fn should_confirm_unsaved_settings_before_app_exit(
    keep_alive_for_lightweight_close: bool,
    skip_unsaved_settings_confirm: bool,
    has_unsaved_settings: bool,
) -> bool {
    has_unsaved_settings && !keep_alive_for_lightweight_close && !skip_unsaved_settings_confirm
}

/// 函数 `handle_main_window_event`
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
pub(crate) fn handle_main_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() == TRAY_PREVIEW_WINDOW_LABEL {
        if let tauri::WindowEvent::Focused(false) = event {
            hide_tray_preview_window(window.app_handle());
        }
        return;
    }
    if window.label() != MAIN_WINDOW_LABEL {
        return;
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
            return;
        }

        let close_to_tray_on_close = CLOSE_TO_TRAY_ON_CLOSE.load(Ordering::Relaxed);
        let tray_available = TRAY_AVAILABLE.load(Ordering::Relaxed);
        let lightweight_tray_close = LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY.load(Ordering::Relaxed);
        let close_mode = resolve_main_window_close_mode(
            close_to_tray_on_close,
            tray_available,
            lightweight_tray_close,
        );
        let skip_unsaved_settings_window_close_confirm =
            take_skip_next_unsaved_settings_window_close_confirm();

        if should_confirm_unsaved_settings_before_window_close(
            close_mode,
            has_unsaved_settings_draft_sections(),
        ) && !skip_unsaved_settings_window_close_confirm
        {
            api.prevent_close();
            if !confirm_discard_unsaved_settings_for_window_close() {
                log::info!("prevented window close because settings drafts are still unsaved");
                return;
            }
            mark_skip_next_unsaved_settings_window_close_confirm();
            mark_skip_next_unsaved_settings_exit_confirm();
            if let Err(err) = window.close() {
                clear_skip_next_unsaved_settings_confirms();
                log::warn!(
                    "confirmed unsaved settings discard but failed to re-issue window close: {}",
                    err
                );
            }
            return;
        }

        if let Err(err) = window.app_handle().save_window_state(tauri_plugin_window_state::StateFlags::all()) {
            log::warn!("save window state before close failed: {}", err);
        }

        match close_mode {
            MainWindowCloseMode::AllowWindowClose => {
                if close_to_tray_on_close && !tray_available {
                    CLOSE_TO_TRAY_ON_CLOSE.store(false, Ordering::Relaxed);
                }
            }
            MainWindowCloseMode::CloseForLightweightTray => {
                KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE.store(true, Ordering::Relaxed);
                log::info!(
                    "window close intercepted; lightweight mode enabled, closing main window to release webview"
                );
            }
            MainWindowCloseMode::HideToTray => {
                api.prevent_close();
                if let Err(err) = window.hide() {
                    log::warn!("hide window to tray failed: {}", err);
                } else {
                    log::info!("window close intercepted; app hidden to tray");
                }
            }
        }
        return;
    }
    if let tauri::WindowEvent::Destroyed = event {
        if should_keep_alive_for_lightweight_close() {
            log::info!("main window destroyed for lightweight tray mode");
            return;
        }
        stop_service();
    }
}

/// 函数 `handle_run_event`
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
pub(crate) fn handle_run_event(app: &tauri::AppHandle, event: &tauri::RunEvent) {
    #[cfg(not(target_os = "macos"))]
    let _ = app;
    match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            let skip_unsaved_settings_confirm = take_skip_next_unsaved_settings_exit_confirm();
            let keep_alive_for_lightweight_close = should_keep_alive_for_lightweight_close();
            if keep_alive_for_lightweight_close {
                api.prevent_exit();
                log::info!("prevented app exit for lightweight tray mode");
                return;
            }
            if should_confirm_unsaved_settings_before_app_exit(
                keep_alive_for_lightweight_close,
                skip_unsaved_settings_confirm,
                has_unsaved_settings_draft_sections(),
            ) && !confirm_discard_unsaved_settings_for_app_exit()
            {
                api.prevent_exit();
                log::info!("prevented app exit because settings drafts are still unsaved");
                return;
            }
            APP_EXIT_REQUESTED.store(true, Ordering::Relaxed);
            KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE.store(false, Ordering::Relaxed);
            stop_service();
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            if let Err(err) = request_show_main_window(app) {
                log::warn!("request show main window from reopen failed: {}", err);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_main_window_close_mode, should_confirm_unsaved_settings_before_app_exit,
        should_confirm_unsaved_settings_before_window_close, MainWindowCloseMode,
    };

    /// 函数 `resolves_main_window_close_modes`
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
    fn resolves_main_window_close_modes() {
        assert_eq!(
            resolve_main_window_close_mode(false, true, false),
            MainWindowCloseMode::AllowWindowClose
        );
        assert_eq!(
            resolve_main_window_close_mode(true, false, false),
            MainWindowCloseMode::AllowWindowClose
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            resolve_main_window_close_mode(true, true, false),
            MainWindowCloseMode::HideToTray
        );
        #[cfg(target_os = "windows")]
        assert_eq!(
            resolve_main_window_close_mode(true, true, false),
            MainWindowCloseMode::CloseForLightweightTray
        );
        assert_eq!(
            resolve_main_window_close_mode(true, true, true),
            MainWindowCloseMode::CloseForLightweightTray
        );
    }

    /// 函数 `confirms_window_close_only_when_window_destroy_would_drop_drafts`
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
    fn confirms_window_close_only_when_window_destroy_would_drop_drafts() {
        assert!(!should_confirm_unsaved_settings_before_window_close(
            MainWindowCloseMode::HideToTray,
            true,
        ));
        assert!(should_confirm_unsaved_settings_before_window_close(
            MainWindowCloseMode::AllowWindowClose,
            true,
        ));
        assert!(should_confirm_unsaved_settings_before_window_close(
            MainWindowCloseMode::CloseForLightweightTray,
            true,
        ));
        assert!(!should_confirm_unsaved_settings_before_window_close(
            MainWindowCloseMode::AllowWindowClose,
            false,
        ));
    }

    /// 函数 `confirms_app_exit_only_when_unsaved_drafts_would_be_lost`
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
    fn confirms_app_exit_only_when_unsaved_drafts_would_be_lost() {
        assert!(should_confirm_unsaved_settings_before_app_exit(
            false, false, true,
        ));
        assert!(!should_confirm_unsaved_settings_before_app_exit(
            false, true, true,
        ));
        assert!(!should_confirm_unsaved_settings_before_app_exit(
            true, false, true,
        ));
        assert!(!should_confirm_unsaved_settings_before_app_exit(
            false, false, false,
        ));
    }
}
