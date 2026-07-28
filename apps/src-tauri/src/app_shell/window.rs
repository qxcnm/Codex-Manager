use std::sync::atomic::{AtomicBool, Ordering};

use tauri::webview::{Color, PageLoadEvent};
#[cfg(not(target_os = "windows"))]
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::Manager;
use tauri::{
    LogicalSize, PhysicalPosition, PhysicalRect, Rect, Size, WebviewUrl, WebviewWindowBuilder,
};

#[cfg(debug_assertions)]
use tauri::Url;

use super::state::{APP_EXIT_REQUESTED, KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE, KEEP_WINDOW_UI_MOUNTED};

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";
pub(crate) const TRAY_PREVIEW_WINDOW_LABEL: &str = "tray-preview";
const TRAY_PREVIEW_WIDTH: f64 = 360.0;
const TRAY_PREVIEW_HEIGHT: f64 = 430.0;
const TRAY_PREVIEW_MARGIN: f64 = 8.0;
static SHOW_MAIN_WINDOW_PENDING: AtomicBool = AtomicBool::new(false);
static MAIN_WINDOW_CREATED_ONCE: AtomicBool = AtomicBool::new(false);

struct MainWindowHandle {
    window: tauri::WebviewWindow,
    created: bool,
    created_after_initial: bool,
}

/// 函数 `show_main_window`
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
fn show_main_window(app: &tauri::AppHandle) -> bool {
    if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
        log::info!(
            "window event: action=show window={} status=skipped reason=app_exit_requested",
            MAIN_WINDOW_LABEL
        );
        return false;
    }
    log::info!(
        "window event: action=show window={} status=requested",
        MAIN_WINDOW_LABEL
    );
    dismiss_tray_preview_window(app);
    KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE.store(false, Ordering::Relaxed);
    let Some(main_window) = ensure_main_window(app) else {
        log::error!(
            "window event: action=show window={} status=failed reason=window_unavailable",
            MAIN_WINDOW_LABEL
        );
        return false;
    };
    log::debug!(
        "window event: action=ensure window={} status=ready created={} created_after_initial={}",
        MAIN_WINDOW_LABEL,
        main_window.created,
        main_window.created_after_initial
    );
    if should_navigate_created_main_window_to_app(
        main_window.created,
        main_window.created_after_initial,
    ) {
        navigate_created_main_window_to_app(&main_window.window);
    }
    reveal_main_window(&main_window.window)
}

fn reveal_main_window(window: &tauri::WebviewWindow) -> bool {
    if let Err(err) = window.unminimize() {
        log::debug!(
            "window event: action=unminimize window={} phase=before_show status=skipped error={}",
            window.label(),
            err
        );
    }
    if let Err(err) = window.show() {
        log::error!(
            "window event: action=show window={} status=failed error={}",
            window.label(),
            err
        );
        return false;
    }
    if let Err(err) = window.unminimize() {
        log::warn!(
            "window event: action=unminimize window={} phase=after_show status=failed error={}",
            window.label(),
            err
        );
    }
    if let Err(err) = window.set_focus() {
        log::warn!(
            "window event: action=focus window={} status=failed error={}",
            window.label(),
            err
        );
    }
    log::info!(
        "window event: action=show window={} status=completed",
        window.label()
    );
    true
}

pub(crate) fn request_show_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
        log::info!(
            "window event: action=request_show window={} status=rejected reason=app_exit_requested",
            MAIN_WINDOW_LABEL
        );
        return Err("app is exiting; show main window request skipped".to_string());
    }
    if SHOW_MAIN_WINDOW_PENDING.swap(true, Ordering::AcqRel) {
        log::debug!(
            "window event: action=request_show window={} status=coalesced reason=request_pending",
            MAIN_WINDOW_LABEL
        );
        return Ok(());
    }
    log::debug!(
        "window event: action=request_show window={} status=queued",
        MAIN_WINDOW_LABEL
    );

    let app = app.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("show-main-window".to_string())
        .spawn(move || {
            if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
                log::info!(
                    "window event: action=request_show window={} status=cancelled reason=app_exit_requested",
                    MAIN_WINDOW_LABEL
                );
                SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
                return;
            }
            let app_for_show = app.clone();
            if let Err(err) = app.run_on_main_thread(move || {
                if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
                    log::info!(
                        "window event: action=show window={} status=skipped reason=app_exit_requested",
                        MAIN_WINDOW_LABEL
                    );
                    SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
                    return;
                }
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    show_main_window(&app_for_show)
                })) {
                    Ok(true) => {}
                    Ok(false) => {
                        log::warn!(
                            "window event: action=request_show window={} status=completed_without_show",
                            MAIN_WINDOW_LABEL
                        );
                    }
                    Err(_payload) => {
                        log::error!(
                            "window event: action=show window={} status=panicked recovery=pending_state_reset",
                            MAIN_WINDOW_LABEL
                        );
                    }
                }
                SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
            }) {
                log::error!(
                    "window event: action=schedule_show window={} status=failed error={}",
                    MAIN_WINDOW_LABEL,
                    err
                );
                KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE.store(false, Ordering::Relaxed);
                SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
            }
        })
    {
        log::error!(
            "window event: action=spawn_show_worker window={} status=failed error={}",
            MAIN_WINDOW_LABEL,
            err
        );
        SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
        return Err(format!("spawn show main window worker failed: {err}"));
    }
    Ok(())
}

pub(crate) fn navigate_main_window_to_startup_app(app: &tauri::AppHandle) -> Result<(), String> {
    log::info!(
        "window event: action=navigate window={} destination=app status=requested",
        MAIN_WINDOW_LABEL
    );
    let app_handle = app.clone();
    let app_for_callback = app_handle.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    if let Err(err) = app_handle.run_on_main_thread(move || {
        let Some(window) = app_for_callback.get_webview_window(MAIN_WINDOW_LABEL) else {
            log::warn!(
                "window event: action=navigate window={} destination=app status=failed reason=window_missing",
                MAIN_WINDOW_LABEL
            );
            if sender
                .send(Err("main window is missing".to_string()))
                .is_err()
            {
                log::warn!(
                    "window event: action=report_navigation window={} status=failed reason=receiver_dropped",
                    MAIN_WINDOW_LABEL
                );
            }
            return;
        };
        let result = navigate_window_to_app_url(&window).map_err(|err| err.to_string());
        match &result {
            Ok(()) => log::info!(
                "window event: action=navigate window={} destination=app status=completed",
                MAIN_WINDOW_LABEL
            ),
            Err(err) => log::error!(
                "window event: action=navigate window={} destination=app status=failed error={}",
                MAIN_WINDOW_LABEL,
                err
            ),
        }
        if sender.send(result).is_err() {
            log::warn!(
                "window event: action=report_navigation window={} status=failed reason=receiver_dropped",
                MAIN_WINDOW_LABEL
            );
        }
    }) {
        log::error!(
            "window event: action=schedule_navigation window={} status=failed error={}",
            MAIN_WINDOW_LABEL,
            err
        );
        return Err(format!("schedule startup app navigation failed: {err}"));
    }
    receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|err| {
            log::error!(
                "window event: action=await_navigation window={} status=failed error={}",
                MAIN_WINDOW_LABEL,
                err
            );
            format!("startup app navigation callback timed out: {err}")
        })?
}

pub(crate) fn dismiss_tray_preview_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
        let keep_mounted = KEEP_WINDOW_UI_MOUNTED.load(Ordering::Relaxed);
        let (action, result) = if keep_mounted {
            ("hide", window.hide())
        } else {
            ("close", window.close())
        };
        if let Err(err) = result {
            log::warn!(
                "window event: action={} window={} status=failed error={}",
                action,
                TRAY_PREVIEW_WINDOW_LABEL,
                err
            );
        } else {
            log::debug!(
                "window event: action={} window={} status=completed",
                action,
                TRAY_PREVIEW_WINDOW_LABEL
            );
        }
    }
}

pub(crate) fn release_tray_preview_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
        if let Err(err) = window.close() {
            log::warn!(
                "window event: action=release window={} status=failed error={}",
                TRAY_PREVIEW_WINDOW_LABEL,
                err
            );
        } else {
            log::debug!(
                "window event: action=release window={} status=completed",
                TRAY_PREVIEW_WINDOW_LABEL
            );
        }
    }
}

pub(crate) fn sync_window_ui_mount_state(app: &tauri::AppHandle) {
    let keep_mounted = KEEP_WINDOW_UI_MOUNTED.load(Ordering::Relaxed);
    log::info!(
        "window event: action=sync_mount_state window={} keep_mounted={} status=requested",
        TRAY_PREVIEW_WINDOW_LABEL,
        keep_mounted
    );
    let app = app.clone();
    let app_for_callback = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        if keep_mounted {
            if ensure_tray_preview_window(&app_for_callback).is_some() {
                log::info!(
                    "window event: action=sync_mount_state window={} keep_mounted=true status=completed",
                    TRAY_PREVIEW_WINDOW_LABEL
                );
            } else {
                log::error!(
                    "window event: action=sync_mount_state window={} keep_mounted=true status=failed reason=window_unavailable",
                    TRAY_PREVIEW_WINDOW_LABEL
                );
            }
        } else {
            release_tray_preview_window(&app_for_callback);
            log::info!(
                "window event: action=sync_mount_state window={} keep_mounted=false status=completed",
                TRAY_PREVIEW_WINDOW_LABEL
            );
        }
    }) {
        log::error!(
            "window event: action=sync_mount_state window={} status=failed error={}",
            TRAY_PREVIEW_WINDOW_LABEL,
            err
        );
    }
}

pub(crate) fn toggle_tray_preview_window(
    app: &tauri::AppHandle,
    click_position: PhysicalPosition<f64>,
    tray_rect: Rect,
) {
    log::debug!(
        "window event: action=toggle window={} status=requested click_x={} click_y={}",
        TRAY_PREVIEW_WINDOW_LABEL,
        click_position.x,
        click_position.y
    );
    let Some(window) = ensure_tray_preview_window(app) else {
        log::error!(
            "window event: action=toggle window={} status=failed reason=window_unavailable",
            TRAY_PREVIEW_WINDOW_LABEL
        );
        return;
    };
    match window.is_visible() {
        Ok(true) => {
            log::debug!(
                "window event: action=toggle window={} decision=dismiss",
                TRAY_PREVIEW_WINDOW_LABEL
            );
            dismiss_tray_preview_window(app);
            return;
        }
        Ok(false) => {}
        Err(err) => {
            log::warn!(
                "window event: action=query_visibility window={} status=failed error={}",
                TRAY_PREVIEW_WINDOW_LABEL,
                err
            );
        }
    }

    position_tray_preview_window(app, &window, click_position, tray_rect);
    if let Err(err) = window.show() {
        log::error!(
            "window event: action=show window={} status=failed error={}",
            TRAY_PREVIEW_WINDOW_LABEL,
            err
        );
        return;
    }
    if let Err(err) = window.set_focus() {
        log::warn!(
            "window event: action=focus window={} status=failed error={}",
            TRAY_PREVIEW_WINDOW_LABEL,
            err
        );
    }
    log::info!(
        "window event: action=show window={} status=completed",
        TRAY_PREVIEW_WINDOW_LABEL
    );
}

/// 函数 `ensure_main_window`
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
fn ensure_main_window(app: &tauri::AppHandle) -> Option<MainWindowHandle> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        MAIN_WINDOW_CREATED_ONCE.store(true, Ordering::Release);
        log::debug!(
            "window event: action=ensure window={} status=reused",
            MAIN_WINDOW_LABEL
        );
        return Some(MainWindowHandle {
            window,
            created: false,
            created_after_initial: false,
        });
    }

    log::info!(
        "window event: action=create window={} status=requested",
        MAIN_WINDOW_LABEL
    );
    let Some(mut config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN_WINDOW_LABEL)
        .cloned()
        .or_else(|| app.config().app.windows.first().cloned())
    else {
        log::error!(
            "window event: action=create window={} status=failed reason=window_config_missing",
            MAIN_WINDOW_LABEL
        );
        return None;
    };
    config.label = MAIN_WINDOW_LABEL.to_string();
    #[cfg(debug_assertions)]
    {
        config.url = startup_loading_url();
    }

    let builder = match WebviewWindowBuilder::from_config(app, &config) {
        Ok(builder) => builder,
        Err(err) => {
            log::error!(
                "window event: action=create_builder window={} status=failed error={}",
                MAIN_WINDOW_LABEL,
                err
            );
            return None;
        }
    };

    match builder
        .on_page_load(|window, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }
            log::info!(
                "window event: action=page_load window={} status=completed",
                MAIN_WINDOW_LABEL
            );
        })
        .build()
    {
        Ok(window) => {
            let created_after_initial = MAIN_WINDOW_CREATED_ONCE.swap(true, Ordering::AcqRel);
            log::info!(
                "window event: action=create window={} status=completed created_after_initial={}",
                MAIN_WINDOW_LABEL,
                created_after_initial
            );
            Some(MainWindowHandle {
                window,
                created: true,
                created_after_initial,
            })
        }
        Err(err) => {
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                MAIN_WINDOW_CREATED_ONCE.store(true, Ordering::Release);
                log::warn!(
                    "window event: action=create window={} status=recovered reason=concurrent_creation error={}",
                    MAIN_WINDOW_LABEL,
                    err
                );
                return Some(MainWindowHandle {
                    window,
                    created: false,
                    created_after_initial: false,
                });
            }
            log::error!(
                "window event: action=create window={} status=failed error={}",
                MAIN_WINDOW_LABEL,
                err
            );
            None
        }
    }
}

fn should_navigate_created_main_window_to_app(created: bool, created_after_initial: bool) -> bool {
    cfg!(debug_assertions) && created && created_after_initial
}

fn navigate_created_main_window_to_app(window: &tauri::WebviewWindow) {
    if let Err(err) = navigate_window_to_app_url(window) {
        log::error!(
            "window event: action=navigate window={} destination=app status=failed error={}",
            window.label(),
            err
        );
    }
}

#[cfg(debug_assertions)]
fn startup_loading_url() -> WebviewUrl {
    WebviewUrl::App("startup.html".into())
}

#[cfg(debug_assertions)]
fn navigate_window_to_app_url(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let url = Url::parse("http://127.0.0.1:3005/")
        .expect("hard-coded dev server startup url must be valid");
    log::info!(
        "window event: action=navigate window={} destination=dev_app_root status=requested",
        window.label()
    );
    window.navigate(url)
}

#[cfg(not(debug_assertions))]
fn navigate_window_to_app_url(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    log::debug!(
        "window event: action=navigate window={} destination=app status=not_required",
        window.label()
    );
    Ok(())
}

fn ensure_tray_preview_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
        log::debug!(
            "window event: action=ensure window={} status=reused",
            TRAY_PREVIEW_WINDOW_LABEL
        );
        apply_tray_preview_window_size(&window);
        return Some(window);
    }

    log::info!(
        "window event: action=create window={} status=requested",
        TRAY_PREVIEW_WINDOW_LABEL
    );
    let builder = WebviewWindowBuilder::new(
        app,
        TRAY_PREVIEW_WINDOW_LABEL,
        WebviewUrl::App("tray-preview/".into()),
    )
    .title("CodexManager")
    .inner_size(TRAY_PREVIEW_WIDTH, TRAY_PREVIEW_HEIGHT)
    .min_inner_size(TRAY_PREVIEW_WIDTH, TRAY_PREVIEW_HEIGHT)
    .max_inner_size(TRAY_PREVIEW_WIDTH, TRAY_PREVIEW_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .transparent(true)
    .background_color(Color(0, 0, 0, 0))
    .shadow(false)
    .always_on_top(true)
    .visible_on_all_workspaces(true)
    .skip_taskbar(true)
    .visible(false)
    .focused(false);

    #[cfg(not(target_os = "windows"))]
    let builder = builder.effects(
        EffectsBuilder::new()
            .effect(Effect::Popover)
            .state(EffectState::Active)
            .radius(18.0)
            .build(),
    );

    match builder.build() {
        Ok(window) => {
            apply_tray_preview_window_size(&window);
            log::info!(
                "window event: action=create window={} status=completed",
                TRAY_PREVIEW_WINDOW_LABEL
            );
            Some(window)
        }
        Err(err) => {
            if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
                apply_tray_preview_window_size(&window);
                log::warn!(
                    "window event: action=create window={} status=recovered reason=concurrent_creation error={}",
                    TRAY_PREVIEW_WINDOW_LABEL,
                    err
                );
                return Some(window);
            }
            log::error!(
                "window event: action=create window={} status=failed error={}",
                TRAY_PREVIEW_WINDOW_LABEL,
                err
            );
            None
        }
    }
}

fn tray_preview_window_size() -> Size {
    LogicalSize::new(TRAY_PREVIEW_WIDTH, TRAY_PREVIEW_HEIGHT).into()
}

fn apply_tray_preview_window_size(window: &tauri::WebviewWindow) {
    let size = tray_preview_window_size();
    if let Err(err) = window.set_min_size(None::<Size>) {
        log::warn!(
            "window event: action=clear_min_size window={} status=failed error={}",
            window.label(),
            err
        );
    }
    if let Err(err) = window.set_max_size(None::<Size>) {
        log::warn!(
            "window event: action=clear_max_size window={} status=failed error={}",
            window.label(),
            err
        );
    }
    if let Err(err) = window.set_size(size) {
        log::warn!(
            "window event: action=set_size window={} status=failed error={}",
            window.label(),
            err
        );
    }
    if let Err(err) = window.set_min_size(Some(size)) {
        log::warn!(
            "window event: action=set_min_size window={} status=failed error={}",
            window.label(),
            err
        );
    }
    if let Err(err) = window.set_max_size(Some(size)) {
        log::warn!(
            "window event: action=set_max_size window={} status=failed error={}",
            window.label(),
            err
        );
    }
}

fn position_tray_preview_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    click_position: PhysicalPosition<f64>,
    tray_rect: Rect,
) {
    let monitor = match app.monitor_from_point(click_position.x, click_position.y) {
        Ok(Some(monitor)) => Some(monitor),
        Ok(None) => match app.primary_monitor() {
            Ok(monitor) => monitor,
            Err(err) => {
                log::warn!(
                    "window event: action=resolve_monitor window={} source=primary status=failed error={}",
                    TRAY_PREVIEW_WINDOW_LABEL,
                    err
                );
                None
            }
        },
        Err(err) => {
            log::warn!(
                "window event: action=resolve_monitor window={} source=click_position status=failed error={}",
                TRAY_PREVIEW_WINDOW_LABEL,
                err
            );
            match app.primary_monitor() {
                Ok(monitor) => monitor,
                Err(fallback_err) => {
                    log::warn!(
                        "window event: action=resolve_monitor window={} source=primary status=failed error={}",
                        TRAY_PREVIEW_WINDOW_LABEL,
                        fallback_err
                    );
                    None
                }
            }
        }
    };
    let Some(monitor) = monitor else {
        log::warn!(
            "window event: action=position window={} status=skipped reason=monitor_unavailable",
            TRAY_PREVIEW_WINDOW_LABEL
        );
        return;
    };
    let position =
        resolve_tray_preview_position(tray_rect, *monitor.work_area(), monitor.scale_factor());
    if let Err(err) = window.set_position(position) {
        log::warn!(
            "window event: action=position window={} status=failed x={} y={} error={}",
            window.label(),
            position.x,
            position.y,
            err
        );
    } else {
        log::debug!(
            "window event: action=position window={} status=completed x={} y={}",
            window.label(),
            position.x,
            position.y
        );
    }
}

fn resolve_tray_preview_position(
    tray_rect: Rect,
    work_area: PhysicalRect<i32, u32>,
    scale_factor: f64,
) -> PhysicalPosition<i32> {
    let tray_position = tray_rect.position.to_physical::<f64>(scale_factor);
    let tray_size = tray_rect.size.to_physical::<f64>(scale_factor);
    let margin = TRAY_PREVIEW_MARGIN * scale_factor;
    let preview_width = TRAY_PREVIEW_WIDTH * scale_factor;
    let preview_height = TRAY_PREVIEW_HEIGHT * scale_factor;
    let work_x = f64::from(work_area.position.x);
    let work_y = f64::from(work_area.position.y);
    let work_width = f64::from(work_area.size.width);
    let work_height = f64::from(work_area.size.height);

    let min_x = work_x + margin;
    let max_x = (work_x + work_width - preview_width - margin).max(min_x);
    let center_x = tray_position.x + tray_size.width / 2.0;
    let x = (center_x - preview_width / 2.0).clamp(min_x, max_x);

    let min_y = work_y + margin;
    let max_y = (work_y + work_height - preview_height - margin).max(min_y);
    let below_tray_y = tray_position.y + tray_size.height + margin;
    let above_tray_y = tray_position.y - preview_height - margin;
    let y = if below_tray_y <= max_y {
        below_tray_y
    } else {
        above_tray_y
    }
    .clamp(min_y, max_y);

    PhysicalPosition::new(x.round() as i32, y.round() as i32)
}

#[cfg(test)]
mod tests {
    use super::{resolve_tray_preview_position, should_navigate_created_main_window_to_app};
    use tauri::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalRect, PhysicalSize, Rect};

    #[test]
    fn tray_preview_position_stays_inside_work_area() {
        let rect = Rect {
            position: LogicalPosition::new(1410.0, 0.0).into(),
            size: LogicalSize::new(24.0, 24.0).into(),
        };
        let work_area = PhysicalRect {
            position: PhysicalPosition::new(0, 24),
            size: PhysicalSize::new(1440, 876),
        };

        let position = resolve_tray_preview_position(rect, work_area, 1.0);

        assert!(position.x <= 1440 - 360 - 8);
        assert_eq!(position.y, 32);
    }

    #[test]
    fn tray_preview_position_can_flip_above_bottom_tray() {
        let rect = Rect {
            position: LogicalPosition::new(720.0, 870.0).into(),
            size: LogicalSize::new(24.0, 24.0).into(),
        };
        let work_area = PhysicalRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(1440, 900),
        };

        let position = resolve_tray_preview_position(rect, work_area, 1.0);

        assert!(position.y < 870);
        assert!(position.y >= 8);
    }

    #[test]
    fn created_main_window_navigation_is_only_for_recreated_windows() {
        assert!(!should_navigate_created_main_window_to_app(false, true));
        assert!(!should_navigate_created_main_window_to_app(true, false));
        #[cfg(debug_assertions)]
        assert!(should_navigate_created_main_window_to_app(true, true));
        #[cfg(not(debug_assertions))]
        assert!(!should_navigate_created_main_window_to_app(true, true));
    }
}
