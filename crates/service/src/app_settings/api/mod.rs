mod current;
mod patch;

use crate::initialize_storage_if_needed;
use serde_json::Value;

pub(super) use super::env_overrides::{
    current_env_overrides, env_override_catalog_value, env_override_reserved_keys,
    env_override_unsupported_keys, save_env_overrides_value, set_env_overrides,
};
pub(super) use super::gateway::{
    current_background_tasks_snapshot_value, current_gateway_free_account_max_model,
    current_gateway_sse_keepalive_interval_ms, current_gateway_upstream_stream_timeout_ms,
    set_gateway_background_tasks, set_gateway_cpa_no_cookie_header_mode,
    set_gateway_free_account_max_model, set_gateway_route_strategy,
    set_gateway_sse_keepalive_interval_ms, set_gateway_upstream_proxy_url,
    set_gateway_upstream_stream_timeout_ms, BackgroundTasksInput,
};
pub(super) use super::runtime_sync::sync_runtime_settings_from_storage;
pub(super) use super::service::{
    current_saved_service_addr, current_service_bind_mode, set_saved_service_addr,
    set_service_bind_mode, SERVICE_BIND_MODE_ALL_INTERFACES, SERVICE_BIND_MODE_LOOPBACK,
    SERVICE_BIND_MODE_SETTING_KEY,
};
pub(super) use super::store::{save_persisted_app_setting, save_persisted_bool_setting};
pub(super) use super::ui::{
    current_close_to_tray_on_close_setting, current_lightweight_mode_on_close_to_tray_setting,
    current_ui_low_transparency_enabled, current_ui_theme, current_update_auto_check_enabled,
    set_close_to_tray_on_close_setting, set_lightweight_mode_on_close_to_tray_setting,
    set_ui_low_transparency_enabled, set_ui_theme, set_update_auto_check_enabled,
};
pub(super) use super::{
    APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY, APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
    APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY, APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY, APP_SETTING_SERVICE_ADDR_KEY,
    APP_SETTING_UI_LOW_TRANSPARENCY_KEY, APP_SETTING_UI_THEME_KEY,
    APP_SETTING_UPDATE_AUTO_CHECK_KEY,
};

pub fn app_settings_get() -> Result<Value, String> {
    current::current_app_settings_value(None, None)
}

pub fn app_settings_get_with_overrides(
    close_to_tray_on_close: Option<bool>,
    close_to_tray_supported: Option<bool>,
) -> Result<Value, String> {
    current::current_app_settings_value(close_to_tray_on_close, close_to_tray_supported)
}

pub fn app_settings_set(params: Option<&Value>) -> Result<Value, String> {
    initialize_storage_if_needed()?;
    let patch = patch::parse_app_settings_patch(params)?;
    patch::apply_app_settings_patch(patch)?;
    app_settings_get()
}
