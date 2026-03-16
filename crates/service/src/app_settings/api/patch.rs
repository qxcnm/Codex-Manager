use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use super::{
    set_close_to_tray_on_close_setting, set_env_overrides, set_gateway_background_tasks,
    set_gateway_cpa_no_cookie_header_mode, set_gateway_free_account_max_model,
    set_gateway_route_strategy, set_gateway_sse_keepalive_interval_ms,
    set_gateway_upstream_proxy_url, set_gateway_upstream_stream_timeout_ms,
    set_lightweight_mode_on_close_to_tray_setting, set_saved_service_addr, set_service_bind_mode,
    set_ui_low_transparency_enabled, set_ui_theme, set_update_auto_check_enabled,
    BackgroundTasksInput,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AppSettingsPatch {
    update_auto_check: Option<bool>,
    close_to_tray_on_close: Option<bool>,
    lightweight_mode_on_close_to_tray: Option<bool>,
    low_transparency: Option<bool>,
    theme: Option<String>,
    service_addr: Option<String>,
    service_listen_mode: Option<String>,
    route_strategy: Option<String>,
    free_account_max_model: Option<String>,
    cpa_no_cookie_header_mode_enabled: Option<bool>,
    upstream_proxy_url: Option<String>,
    upstream_stream_timeout_ms: Option<u64>,
    sse_keepalive_interval_ms: Option<u64>,
    background_tasks: Option<BackgroundTasksInput>,
    env_overrides: Option<HashMap<String, String>>,
    web_access_password: Option<String>,
}

pub(super) fn parse_app_settings_patch(params: Option<&Value>) -> Result<AppSettingsPatch, String> {
    match params {
        Some(value) => serde_json::from_value::<AppSettingsPatch>(value.clone())
            .map_err(|err| format!("invalid app settings payload: {err}")),
        None => Ok(AppSettingsPatch::default()),
    }
}

pub(super) fn apply_app_settings_patch(patch: AppSettingsPatch) -> Result<(), String> {
    if let Some(enabled) = patch.update_auto_check {
        set_update_auto_check_enabled(enabled)?;
    }
    if let Some(enabled) = patch.close_to_tray_on_close {
        set_close_to_tray_on_close_setting(enabled)?;
    }
    if let Some(enabled) = patch.lightweight_mode_on_close_to_tray {
        set_lightweight_mode_on_close_to_tray_setting(enabled)?;
    }
    if let Some(enabled) = patch.low_transparency {
        set_ui_low_transparency_enabled(enabled)?;
    }
    if let Some(theme) = patch.theme {
        let _ = set_ui_theme(Some(&theme))?;
    }
    if let Some(service_addr) = patch.service_addr {
        let _ = set_saved_service_addr(Some(&service_addr))?;
    }
    if let Some(mode) = patch.service_listen_mode {
        let _ = set_service_bind_mode(&mode)?;
    }
    if let Some(strategy) = patch.route_strategy {
        let _ = set_gateway_route_strategy(&strategy)?;
    }
    if let Some(model) = patch.free_account_max_model {
        let _ = set_gateway_free_account_max_model(&model)?;
    }
    if let Some(enabled) = patch.cpa_no_cookie_header_mode_enabled {
        let _ = set_gateway_cpa_no_cookie_header_mode(enabled)?;
    }
    if let Some(proxy_url) = patch.upstream_proxy_url {
        let _ = set_gateway_upstream_proxy_url(Some(&proxy_url))?;
    }
    if let Some(timeout_ms) = patch.upstream_stream_timeout_ms {
        let _ = set_gateway_upstream_stream_timeout_ms(timeout_ms)?;
    }
    if let Some(interval_ms) = patch.sse_keepalive_interval_ms {
        let _ = set_gateway_sse_keepalive_interval_ms(interval_ms)?;
    }
    if let Some(background_tasks) = patch.background_tasks {
        let _ = set_gateway_background_tasks(background_tasks)?;
    }
    if let Some(env_overrides) = patch.env_overrides {
        let _ = set_env_overrides(env_overrides)?;
    }
    if let Some(password) = patch.web_access_password {
        let _ = crate::set_web_access_password(Some(&password))?;
    }

    Ok(())
}
