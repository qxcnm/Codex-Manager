pub const APP_SETTING_UPDATE_AUTO_CHECK_KEY: &str = "app.update.auto_check";
pub const APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY: &str = "app.close_to_tray_on_close";
pub const APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY: &str =
    "app.lightweight_mode_on_close_to_tray";
pub const APP_SETTING_UI_LOW_TRANSPARENCY_KEY: &str = "ui.low_transparency";
pub const APP_SETTING_UI_THEME_KEY: &str = "ui.theme";
pub const APP_SETTING_SERVICE_ADDR_KEY: &str = "app.service_addr";
pub const APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY: &str = "gateway.route_strategy";
pub const APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY: &str = "gateway.free_account_max_model";
pub const APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY: &str =
    "gateway.cpa_no_cookie_header_mode";
pub const APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY: &str = "gateway.upstream_proxy_url";
pub const APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY: &str =
    "gateway.upstream_stream_timeout_ms";
pub const APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY: &str =
    "gateway.sse_keepalive_interval_ms";
pub const APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY: &str = "gateway.background_tasks";
pub const APP_SETTING_ENV_OVERRIDES_KEY: &str = "app.env_overrides";
pub const APP_SETTING_WEB_ACCESS_PASSWORD_HASH_KEY: &str = "web.auth.password_hash";
pub const WEB_ACCESS_SESSION_COOKIE_NAME: &str = "codexmanager_web_auth";

pub(crate) fn parse_bool_with_default(raw: &str, default: bool) -> bool {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

pub(crate) fn normalize_optional_text(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}
