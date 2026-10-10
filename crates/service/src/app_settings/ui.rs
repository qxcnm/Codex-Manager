use super::parse_bool_with_default;
use super::{
    get_persisted_app_setting, save_persisted_app_setting, save_persisted_bool_setting,
    APP_SETTING_AUTO_START_ENABLED_KEY, APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY,
    APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY, APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY,
    APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY, APP_SETTING_UI_APPEARANCE_PRESET_KEY,
    APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY, APP_SETTING_UI_LOCALE_KEY,
    APP_SETTING_UI_LOW_TRANSPARENCY_KEY, APP_SETTING_UI_THEME_KEY, APP_SETTING_UI_ZOOM_FACTOR_KEY,
    APP_SETTING_UPDATE_AUTO_CHECK_KEY,
};

const DEFAULT_UI_THEME: &str = "tech";
const DEFAULT_UI_APPEARANCE_PRESET: &str = "classic";
const DEFAULT_UI_LOCALE: &str = "auto";
pub const DEFAULT_UI_ZOOM_FACTOR: f64 = 1.0;
pub const MIN_UI_ZOOM_FACTOR: f64 = 0.75;
pub const MAX_UI_ZOOM_FACTOR: f64 = 1.25;
const VALID_UI_THEMES: &[&str] = &[
    "tech", "dark", "dark-one", "business", "mint", "sunset", "grape", "ocean", "forest", "rose",
    "slate", "aurora",
];
const VALID_UI_APPEARANCE_PRESETS: &[&str] = &["modern", "classic"];
const VALID_UI_LOCALES: &[&str] = &["auto", "zh-CN", "en", "ru", "ko"];

/// 函数 `normalize_ui_theme`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
pub(super) fn normalize_ui_theme(raw: Option<&str>) -> String {
    let candidate = raw.unwrap_or(DEFAULT_UI_THEME).trim().to_ascii_lowercase();
    if VALID_UI_THEMES.iter().any(|theme| *theme == candidate) {
        candidate
    } else {
        DEFAULT_UI_THEME.to_string()
    }
}

/// 函数 `normalize_ui_appearance_preset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
pub(super) fn normalize_ui_appearance_preset(raw: Option<&str>) -> String {
    let candidate = raw
        .unwrap_or(DEFAULT_UI_APPEARANCE_PRESET)
        .trim()
        .to_ascii_lowercase();
    if VALID_UI_APPEARANCE_PRESETS
        .iter()
        .any(|preset| *preset == candidate)
    {
        candidate
    } else {
        DEFAULT_UI_APPEARANCE_PRESET.to_string()
    }
}

pub(super) fn normalize_ui_locale(raw: Option<&str>) -> String {
    let candidate = raw.unwrap_or(DEFAULT_UI_LOCALE).trim();
    let normalized = candidate.to_ascii_lowercase();
    let next_value = match normalized.as_str() {
        "auto" => "auto",
        "zh" | "zh-cn" | "zh_hans" | "zh-hans" => "zh-CN",
        "en" | "en-us" | "en-gb" => "en",
        "ru" | "ru-ru" => "ru",
        "ko" | "ko-kr" => "ko",
        _ => DEFAULT_UI_LOCALE,
    };
    if VALID_UI_LOCALES.iter().any(|locale| *locale == next_value) {
        next_value.to_string()
    } else {
        DEFAULT_UI_LOCALE.to_string()
    }
}

pub(super) fn normalize_ui_zoom_factor(value: f64) -> f64 {
    if !value.is_finite() {
        return DEFAULT_UI_ZOOM_FACTOR;
    }
    let clamped = value.clamp(MIN_UI_ZOOM_FACTOR, MAX_UI_ZOOM_FACTOR);
    (clamped * 20.0).round() / 20.0
}

/// 函数 `current_update_auto_check_enabled`
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
pub fn current_update_auto_check_enabled() -> bool {
    get_persisted_app_setting(APP_SETTING_UPDATE_AUTO_CHECK_KEY)
        .map(|value| parse_bool_with_default(&value, true))
        .unwrap_or(true)
}

/// 函数 `set_update_auto_check_enabled`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - enabled: 参数 enabled
///
/// # 返回
/// 返回函数执行结果
pub fn set_update_auto_check_enabled(enabled: bool) -> Result<bool, String> {
    save_persisted_bool_setting(APP_SETTING_UPDATE_AUTO_CHECK_KEY, enabled)?;
    Ok(enabled)
}

pub fn current_auto_start_enabled_setting() -> bool {
    get_persisted_app_setting(APP_SETTING_AUTO_START_ENABLED_KEY)
        .map(|value| parse_bool_with_default(&value, false))
        .unwrap_or(false)
}

pub fn set_auto_start_enabled_setting(enabled: bool) -> Result<bool, String> {
    save_persisted_bool_setting(APP_SETTING_AUTO_START_ENABLED_KEY, enabled)?;
    Ok(enabled)
}

pub fn current_show_main_window_on_startup_setting() -> bool {
    get_persisted_app_setting(APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY)
        .map(|value| parse_bool_with_default(&value, true))
        .unwrap_or(true)
}

pub fn set_show_main_window_on_startup_setting(enabled: bool) -> Result<bool, String> {
    save_persisted_bool_setting(APP_SETTING_SHOW_MAIN_WINDOW_ON_STARTUP_KEY, enabled)?;
    Ok(enabled)
}

/// 函数 `current_close_to_tray_on_close_setting`
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
pub fn current_close_to_tray_on_close_setting() -> bool {
    get_persisted_app_setting(APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY)
        .map(|value| parse_bool_with_default(&value, false))
        .unwrap_or(false)
}

/// 函数 `set_close_to_tray_on_close_setting`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - enabled: 参数 enabled
///
/// # 返回
/// 返回函数执行结果
pub fn set_close_to_tray_on_close_setting(enabled: bool) -> Result<bool, String> {
    save_persisted_bool_setting(APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY, enabled)?;
    Ok(enabled)
}

pub fn current_keep_window_ui_mounted_setting() -> bool {
    if let Some(value) = get_persisted_app_setting(APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY) {
        return parse_bool_with_default(&value, !cfg!(target_os = "windows"));
    }

    if cfg!(target_os = "windows") {
        return false;
    }

    !get_persisted_app_setting(APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY)
        .map(|value| parse_bool_with_default(&value, false))
        .unwrap_or(false)
}

pub fn set_keep_window_ui_mounted_setting(enabled: bool) -> Result<bool, String> {
    save_persisted_bool_setting(APP_SETTING_KEEP_WINDOW_UI_MOUNTED_KEY, enabled)?;
    save_persisted_bool_setting(APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY, !enabled)?;
    Ok(enabled)
}

/// 函数 `current_lightweight_mode_on_close_to_tray_setting`
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
pub fn current_lightweight_mode_on_close_to_tray_setting() -> bool {
    !current_keep_window_ui_mounted_setting()
}

/// 函数 `set_lightweight_mode_on_close_to_tray_setting`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - enabled: 参数 enabled
///
/// # 返回
/// 返回函数执行结果
pub fn set_lightweight_mode_on_close_to_tray_setting(enabled: bool) -> Result<bool, String> {
    set_keep_window_ui_mounted_setting(!enabled)?;
    Ok(enabled)
}

/// 函数 `current_ui_low_transparency_enabled`
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
pub fn current_ui_low_transparency_enabled() -> bool {
    get_persisted_app_setting(APP_SETTING_UI_LOW_TRANSPARENCY_KEY)
        .map(|value| parse_bool_with_default(&value, false))
        .unwrap_or(false)
}

pub fn current_codex_cli_guide_dismissed() -> bool {
    get_persisted_app_setting(APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY)
        .map(|value| parse_bool_with_default(&value, false))
        .unwrap_or(false)
}

/// 函数 `set_ui_low_transparency_enabled`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - enabled: 参数 enabled
///
/// # 返回
/// 返回函数执行结果
pub fn set_ui_low_transparency_enabled(enabled: bool) -> Result<bool, String> {
    save_persisted_bool_setting(APP_SETTING_UI_LOW_TRANSPARENCY_KEY, enabled)?;
    Ok(enabled)
}

pub fn set_ui_zoom_factor(value: f64) -> Result<f64, String> {
    let normalized = normalize_ui_zoom_factor(value);
    save_persisted_app_setting(
        APP_SETTING_UI_ZOOM_FACTOR_KEY,
        Some(&normalized.to_string()),
    )?;
    Ok(normalized)
}

pub fn set_codex_cli_guide_dismissed(dismissed: bool) -> Result<bool, String> {
    save_persisted_bool_setting(APP_SETTING_UI_CODEX_CLI_GUIDE_DISMISSED_KEY, dismissed)?;
    Ok(dismissed)
}

/// 函数 `current_ui_theme`
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
pub fn current_ui_theme() -> String {
    normalize_ui_theme(get_persisted_app_setting(APP_SETTING_UI_THEME_KEY).as_deref())
}

/// 函数 `set_ui_theme`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - theme: 参数 theme
///
/// # 返回
/// 返回函数执行结果
pub fn set_ui_theme(theme: Option<&str>) -> Result<String, String> {
    let normalized = normalize_ui_theme(theme);
    save_persisted_app_setting(APP_SETTING_UI_THEME_KEY, Some(&normalized))?;
    Ok(normalized)
}

/// 函数 `current_ui_appearance_preset`
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
pub fn current_ui_appearance_preset() -> String {
    normalize_ui_appearance_preset(
        get_persisted_app_setting(APP_SETTING_UI_APPEARANCE_PRESET_KEY).as_deref(),
    )
}

/// 函数 `set_ui_appearance_preset`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - preset: 参数 preset
///
/// # 返回
/// 返回函数执行结果
pub fn set_ui_appearance_preset(preset: Option<&str>) -> Result<String, String> {
    let normalized = normalize_ui_appearance_preset(preset);
    save_persisted_app_setting(APP_SETTING_UI_APPEARANCE_PRESET_KEY, Some(&normalized))?;
    Ok(normalized)
}

pub fn set_ui_locale(locale: Option<&str>) -> Result<String, String> {
    let normalized = normalize_ui_locale(locale);
    save_persisted_app_setting(APP_SETTING_UI_LOCALE_KEY, Some(&normalized))?;
    Ok(normalized)
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
