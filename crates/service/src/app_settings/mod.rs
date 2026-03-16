mod api;
mod env_overrides;
mod gateway;
mod runtime_sync;
mod service;
mod shared;
mod store;
mod ui;

pub use api::{app_settings_get, app_settings_get_with_overrides, app_settings_set};
pub(crate) use env_overrides::{
    apply_env_overrides_to_process, persisted_env_overrides_missing_process_env,
    reload_runtime_after_env_override_apply,
};
pub use gateway::{
    current_gateway_free_account_max_model, current_gateway_sse_keepalive_interval_ms,
    current_gateway_upstream_stream_timeout_ms, set_gateway_background_tasks,
    set_gateway_cpa_no_cookie_header_mode, set_gateway_free_account_max_model,
    set_gateway_route_strategy, set_gateway_sse_keepalive_interval_ms,
    set_gateway_upstream_proxy_url, set_gateway_upstream_stream_timeout_ms, BackgroundTasksInput,
};
pub use runtime_sync::sync_runtime_settings_from_storage;
pub use service::{
    bind_all_interfaces_enabled, current_saved_service_addr, current_service_bind_mode,
    default_listener_bind_addr, listener_bind_addr, set_saved_service_addr, set_service_bind_mode,
    DEFAULT_ADDR, DEFAULT_BIND_ADDR, SERVICE_BIND_MODE_ALL_INTERFACES, SERVICE_BIND_MODE_LOOPBACK,
    SERVICE_BIND_MODE_SETTING_KEY,
};
pub(crate) use shared::{normalize_optional_text, parse_bool_with_default};
pub use shared::{
    APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY, APP_SETTING_ENV_OVERRIDES_KEY,
    APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY, APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY, APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY, APP_SETTING_SERVICE_ADDR_KEY,
    APP_SETTING_UI_LOW_TRANSPARENCY_KEY, APP_SETTING_UI_THEME_KEY,
    APP_SETTING_UPDATE_AUTO_CHECK_KEY, APP_SETTING_WEB_ACCESS_PASSWORD_HASH_KEY,
    WEB_ACCESS_SESSION_COOKIE_NAME,
};
pub(crate) use store::{
    get_persisted_app_setting, list_app_settings_map, save_persisted_app_setting,
    save_persisted_bool_setting,
};
pub use ui::{
    current_close_to_tray_on_close_setting, current_lightweight_mode_on_close_to_tray_setting,
    current_ui_low_transparency_enabled, current_ui_theme, current_update_auto_check_enabled,
    set_close_to_tray_on_close_setting, set_lightweight_mode_on_close_to_tray_setting,
    set_ui_low_transparency_enabled, set_ui_theme, set_update_auto_check_enabled,
};
