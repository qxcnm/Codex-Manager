use super::{normalize_ui_locale, normalize_ui_zoom_factor, DEFAULT_UI_LOCALE};

#[test]
fn ui_locale_normalization_defaults_to_auto() {
    assert_eq!(normalize_ui_locale(None), DEFAULT_UI_LOCALE);
    assert_eq!(normalize_ui_locale(Some("")), DEFAULT_UI_LOCALE);
    assert_eq!(normalize_ui_locale(Some("unknown")), DEFAULT_UI_LOCALE);
    assert_eq!(normalize_ui_locale(Some("AUTO")), "auto");
}

#[test]
fn ui_locale_normalization_accepts_supported_aliases() {
    assert_eq!(normalize_ui_locale(Some("zh-cn")), "zh-CN");
    assert_eq!(normalize_ui_locale(Some("EN-US")), "en");
    assert_eq!(normalize_ui_locale(Some("ru-RU")), "ru");
    assert_eq!(normalize_ui_locale(Some("ko-kr")), "ko");
}

#[test]
fn ui_zoom_factor_is_clamped_and_rounded_to_five_percent_steps() {
    assert_eq!(normalize_ui_zoom_factor(0.5), 0.75);
    assert_eq!(normalize_ui_zoom_factor(0.82), 0.8);
    assert_eq!(normalize_ui_zoom_factor(1.18), 1.2);
    assert_eq!(normalize_ui_zoom_factor(2.0), 1.25);
    assert_eq!(normalize_ui_zoom_factor(f64::NAN), 1.0);
}
