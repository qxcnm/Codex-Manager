pub(crate) mod gateway;
pub(crate) mod proxy_profiles;
pub(crate) mod service_listen;
pub(crate) mod tray_state;
pub(crate) mod ui;

/// 函数 `effective_lightweight_mode_on_close_to_tray`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - requested: 参数 requested
/// - close_to_tray_effective: 参数 close_to_tray_effective
///
/// # 返回
/// 返回函数执行结果
#[cfg_attr(not(test), allow(dead_code))]
pub fn effective_lightweight_mode_on_close_to_tray(
    requested: bool,
    close_to_tray_effective: bool,
) -> bool {
    tray_state::effective_lightweight_mode_on_close_to_tray(requested, close_to_tray_effective)
}

/// 函数 `sync_window_runtime_state_from_settings`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - settings: 参数 settings
///
/// # 返回
/// 无
pub fn sync_window_runtime_state_from_settings(settings: &mut serde_json::Value) {
    tray_state::sync_window_runtime_state_from_settings(settings)
}
