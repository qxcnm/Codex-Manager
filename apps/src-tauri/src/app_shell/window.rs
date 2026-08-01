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

struct TrayPreviewWindowHandle {
    window: tauri::WebviewWindow,
    created: bool,
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
        log::info!("show main window skipped because app exit is already requested");
        return false;
    }
    log::info!("show main window requested");
    dismiss_tray_preview_window(app);
    KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE.store(false, Ordering::Relaxed);
    let Some(main_window) = ensure_main_window(app) else {
        return false;
    };
    if should_navigate_created_main_window_to_app(
        main_window.created,
        main_window.created_after_initial,
    ) {
        navigate_main_window_to_app(&main_window.window);
    }
    reveal_main_window(&main_window.window)
}

fn reveal_main_window(window: &tauri::WebviewWindow) -> bool {
    if let Err(err) = window.unminimize() {
        log::debug!("unminimize main window before show skipped: {}", err);
    }
    if let Err(err) = window.show() {
        log::warn!("show main window failed: {}", err);
        return false;
    }
    if let Err(err) = window.unminimize() {
        log::warn!("unminimize main window after show failed: {}", err);
    }
    if let Err(err) = window.set_focus() {
        log::warn!("focus main window failed: {}", err);
    }
    log::info!("show main window completed");
    true
}

pub(crate) fn request_show_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
        return Err("app is exiting; show main window request skipped".to_string());
    }
    if SHOW_MAIN_WINDOW_PENDING.swap(true, Ordering::AcqRel) {
        log::debug!("show main window request coalesced because one is already pending");
        return Ok(());
    }

    let app = app.clone();
    std::thread::spawn(move || {
        if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
            SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
            return;
        }
        let app_for_show = app.clone();
        if let Err(err) = app.run_on_main_thread(move || {
            if APP_EXIT_REQUESTED.load(Ordering::Relaxed) {
                log::info!("show main window skipped on main thread because app is exiting");
                SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
                return;
            }
            let shown = show_main_window(&app_for_show);
            if !shown {
                log::warn!("show main window request completed without showing a window");
            }
            SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
        }) {
            log::warn!("schedule show main window on main thread failed: {}", err);
            KEEP_ALIVE_FOR_LIGHTWEIGHT_CLOSE.store(false, Ordering::Relaxed);
            SHOW_MAIN_WINDOW_PENDING.store(false, Ordering::Release);
        }
    });
    Ok(())
}

pub(crate) fn navigate_main_window_to_startup_app(app: &tauri::AppHandle) -> Result<(), String> {
    let app_handle = app.clone();
    let app_for_callback = app_handle.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    if let Err(err) = app_handle.run_on_main_thread(move || {
        let Some(window) = app_for_callback.get_webview_window(MAIN_WINDOW_LABEL) else {
            let _ = sender.send(Err("main window is missing".to_string()));
            return;
        };
        let result = navigate_window_to_app_url(&window).map_err(|err| err.to_string());
        let _ = sender.send(result);
    }) {
        return Err(format!("schedule startup app navigation failed: {err}"));
    }
    receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|err| format!("startup app navigation callback timed out: {err}"))?
}

pub(crate) fn dismiss_tray_preview_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
        let result = if KEEP_WINDOW_UI_MOUNTED.load(Ordering::Relaxed) {
            window.hide()
        } else {
            window.close()
        };
        if let Err(err) = result {
            log::warn!("dismiss tray preview window failed: {}", err);
        }
    }
}

pub(crate) fn release_tray_preview_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
        if let Err(err) = window.close() {
            log::warn!("release tray preview window failed: {}", err);
        }
    }
}

pub(crate) fn sync_window_ui_mount_state(app: &tauri::AppHandle) {
    let app = app.clone();
    let app_for_callback = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        if KEEP_WINDOW_UI_MOUNTED.load(Ordering::Relaxed) {
            let _ = ensure_tray_preview_window(&app_for_callback);
        } else {
            release_tray_preview_window(&app_for_callback);
        }
    }) {
        log::warn!("schedule window UI mount state sync failed: {}", err);
    }
}

pub(crate) fn toggle_tray_preview_window(
    app: &tauri::AppHandle,
    click_position: PhysicalPosition<f64>,
    tray_rect: Rect,
) {
    let Some(tray_preview) = ensure_tray_preview_window(app) else {
        return;
    };
    let window = tray_preview.window;
    if window.is_visible().unwrap_or(false) {
        dismiss_tray_preview_window(app);
        return;
    }

    // Only a newly created preview needs an explicit reload. Retained previews
    // keep their mounted UI and state when they are hidden and shown again.
    if should_reload_tray_preview_window(tray_preview.created) {
        if let Err(err) = window.reload() {
            log::warn!("reload newly created tray preview window failed: {}", err);
        }
    }
    position_tray_preview_window(app, &window, click_position, tray_rect);
    if let Err(err) = window.show() {
        log::warn!("show tray preview window failed: {}", err);
        return;
    }
    let _ = window.set_focus();
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
        return Some(MainWindowHandle {
            window,
            created: false,
            created_after_initial: false,
        });
    }

    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN_WINDOW_LABEL)
        .cloned()
        .or_else(|| app.config().app.windows.first().cloned())?;
    config.label = MAIN_WINDOW_LABEL.to_string();
    #[cfg(debug_assertions)]
    {
        config.url = startup_loading_url();
    }

    let builder = match WebviewWindowBuilder::from_config(app, &config) {
        Ok(builder) => builder,
        Err(err) => {
            log::warn!("create main window builder failed: {}", err);
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
            log::info!("main window page loaded: {}", payload.url());
            if should_navigate_loaded_main_window_to_app(payload.url().path()) {
                navigate_main_window_to_app(&window);
            }
        })
        .build()
    {
        Ok(window) => {
            let created_after_initial = MAIN_WINDOW_CREATED_ONCE.swap(true, Ordering::AcqRel);
            Some(MainWindowHandle {
                window,
                created: true,
                created_after_initial,
            })
        }
        Err(err) => {
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                MAIN_WINDOW_CREATED_ONCE.store(true, Ordering::Release);
                return Some(MainWindowHandle {
                    window,
                    created: false,
                    created_after_initial: false,
                });
            }
            log::warn!("create main window failed: {}", err);
            None
        }
    }
}

fn should_navigate_created_main_window_to_app(created: bool, created_after_initial: bool) -> bool {
    cfg!(debug_assertions) && created && created_after_initial
}

fn should_navigate_loaded_main_window_to_app(path: &str) -> bool {
    cfg!(debug_assertions) && path == "/startup.html"
}

fn navigate_main_window_to_app(window: &tauri::WebviewWindow) {
    if let Err(err) = navigate_window_to_app_url(window) {
        log::warn!(
            "navigate main window from startup page to app failed: {}",
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
    log::info!("navigating main window to dev app root");
    window.navigate(url)
}

#[cfg(not(debug_assertions))]
fn navigate_window_to_app_url(_window: &tauri::WebviewWindow) -> tauri::Result<()> {
    Ok(())
}

fn ensure_tray_preview_window(app: &tauri::AppHandle) -> Option<TrayPreviewWindowHandle> {
    if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
        apply_tray_preview_window_size(&window);
        return Some(TrayPreviewWindowHandle {
            window,
            created: false,
        });
    }

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
            Some(TrayPreviewWindowHandle {
                window,
                created: true,
            })
        }
        Err(err) => {
            if let Some(window) = app.get_webview_window(TRAY_PREVIEW_WINDOW_LABEL) {
                apply_tray_preview_window_size(&window);
                return Some(TrayPreviewWindowHandle {
                    window,
                    created: false,
                });
            }
            log::warn!("create tray preview window failed: {}", err);
            None
        }
    }
}

fn should_reload_tray_preview_window(created: bool) -> bool {
    created
}

fn tray_preview_window_size() -> Size {
    LogicalSize::new(TRAY_PREVIEW_WIDTH, TRAY_PREVIEW_HEIGHT).into()
}

fn apply_tray_preview_window_size(window: &tauri::WebviewWindow) {
    let size = tray_preview_window_size();
    if let Err(err) = window.set_min_size(None::<Size>) {
        log::warn!("clear tray preview min size failed: {}", err);
    }
    if let Err(err) = window.set_max_size(None::<Size>) {
        log::warn!("clear tray preview max size failed: {}", err);
    }
    if let Err(err) = window.set_size(size) {
        log::warn!("set tray preview size failed: {}", err);
    }
    if let Err(err) = window.set_min_size(Some(size)) {
        log::warn!("set tray preview min size failed: {}", err);
    }
    if let Err(err) = window.set_max_size(Some(size)) {
        log::warn!("set tray preview max size failed: {}", err);
    }
}

fn position_tray_preview_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    click_position: PhysicalPosition<f64>,
    tray_rect: Rect,
) {
    let monitor = app
        .monitor_from_point(click_position.x, click_position.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let position =
        resolve_tray_preview_position(tray_rect, *monitor.work_area(), monitor.scale_factor());
    if let Err(err) = window.set_position(position) {
        log::warn!("position tray preview window failed: {}", err);
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
    use super::{
        resolve_tray_preview_position, should_navigate_created_main_window_to_app,
        should_navigate_loaded_main_window_to_app, should_reload_tray_preview_window,
    };
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

    #[test]
    fn loaded_startup_page_navigates_to_the_dev_app() {
        #[cfg(debug_assertions)]
        {
            assert!(should_navigate_loaded_main_window_to_app("/startup.html"));
            assert!(!should_navigate_loaded_main_window_to_app("/"));
            assert!(!should_navigate_loaded_main_window_to_app("/tray-preview/"));
        }
        #[cfg(not(debug_assertions))]
        assert!(!should_navigate_loaded_main_window_to_app("/startup.html"));
    }

    #[test]
    fn tray_preview_is_reloaded_only_when_newly_created() {
        assert!(!should_reload_tray_preview_window(false));
        assert!(should_reload_tray_preview_window(true));
    }
}
