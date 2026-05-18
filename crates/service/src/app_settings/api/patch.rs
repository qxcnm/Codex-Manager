use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use super::author_links::{
    normalize_author_link_items, serialize_author_link_items, AuthorLinkItem,
};
use super::{
    save_persisted_app_setting, set_close_to_tray_on_close_setting, set_codex_cli_guide_dismissed,
    set_env_overrides, set_gateway_account_max_inflight, set_gateway_background_tasks,
    set_gateway_free_account_max_model, set_gateway_model_forward_rules, set_gateway_originator,
    set_gateway_quota_guard, set_gateway_residency_requirement, set_gateway_route_strategy,
    set_gateway_sse_keepalive_interval_ms, set_gateway_upstream_proxy_url,
    set_gateway_upstream_stream_timeout_ms, set_gateway_upstream_total_timeout_ms,
    set_gateway_user_agent_version, set_lightweight_mode_on_close_to_tray_setting,
    set_saved_service_addr, set_service_bind_mode, set_ui_appearance_preset, set_ui_locale,
    set_ui_low_transparency_enabled, set_ui_theme, set_update_auto_check_enabled,
    BackgroundTasksInput, QuotaGuardInput, APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY,
    APP_SETTING_AUTHOR_SPONSORS_KEY, APP_SETTING_PLUGIN_MARKET_MODE_KEY,
    APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AppSettingsPatch {
    update_auto_check: Option<bool>,
    close_to_tray_on_close: Option<bool>,
    lightweight_mode_on_close_to_tray: Option<bool>,
    codex_cli_guide_dismissed: Option<bool>,
    low_transparency: Option<bool>,
    theme: Option<String>,
    appearance_preset: Option<String>,
    locale: Option<String>,
    service_addr: Option<String>,
    pub(super) service_listen_mode: Option<String>,
    route_strategy: Option<String>,
    free_account_max_model: Option<String>,
    model_forward_rules: Option<String>,
    account_max_inflight: Option<usize>,
    gateway_originator: Option<String>,
    gateway_user_agent_version: Option<String>,
    gateway_residency_requirement: Option<String>,
    plugin_market_mode: Option<String>,
    plugin_market_source_url: Option<String>,
    author_sponsors: Option<Vec<AuthorLinkItem>>,
    author_server_recommendations: Option<Vec<AuthorLinkItem>>,
    upstream_proxy_url: Option<String>,
    upstream_stream_timeout_ms: Option<u64>,
    upstream_total_timeout_ms: Option<u64>,
    sse_keepalive_interval_ms: Option<u64>,
    quota_guard: Option<QuotaGuardInput>,
    background_tasks: Option<BackgroundTasksInput>,
    env_overrides: Option<HashMap<String, String>>,
    web_access_password: Option<String>,
    web_auth_mode: Option<String>,
    distribution_enabled: Option<bool>,
}

/// 函数 `parse_app_settings_patch`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn parse_app_settings_patch(params: Option<&Value>) -> Result<AppSettingsPatch, String> {
    match params {
        Some(value) => serde_json::from_value::<AppSettingsPatch>(value.clone())
            .map_err(|err| format!("invalid app settings payload: {err}")),
        None => Ok(AppSettingsPatch::default()),
    }
}

/// 函数 `apply_app_settings_patch`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
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
    if let Some(dismissed) = patch.codex_cli_guide_dismissed {
        set_codex_cli_guide_dismissed(dismissed)?;
    }
    if let Some(enabled) = patch.low_transparency {
        set_ui_low_transparency_enabled(enabled)?;
    }
    if let Some(theme) = patch.theme {
        let _ = set_ui_theme(Some(&theme))?;
    }
    if let Some(preset) = patch.appearance_preset {
        let _ = set_ui_appearance_preset(Some(&preset))?;
    }
    if let Some(locale) = patch.locale {
        let _ = set_ui_locale(Some(&locale))?;
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
    if let Some(raw) = patch.model_forward_rules {
        let _ = set_gateway_model_forward_rules(&raw)?;
    }
    if let Some(limit) = patch.account_max_inflight {
        let _ = set_gateway_account_max_inflight(limit)?;
    }
    if let Some(originator) = patch.gateway_originator {
        let _ = set_gateway_originator(&originator)?;
    }
    if let Some(version) = patch.gateway_user_agent_version {
        let _ = set_gateway_user_agent_version(&version)?;
    }
    if let Some(residency_requirement) = patch.gateway_residency_requirement {
        let _ = set_gateway_residency_requirement(Some(&residency_requirement))?;
    }
    if let Some(plugin_market_mode) = patch.plugin_market_mode {
        let _ = save_persisted_app_setting(
            APP_SETTING_PLUGIN_MARKET_MODE_KEY,
            if plugin_market_mode.trim().is_empty() {
                None
            } else {
                Some(&plugin_market_mode)
            },
        )?;
    }
    if let Some(plugin_market_source_url) = patch.plugin_market_source_url {
        let _ = save_persisted_app_setting(
            APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY,
            if plugin_market_source_url.trim().is_empty() {
                None
            } else {
                Some(&plugin_market_source_url)
            },
        )?;
    }
    if let Some(author_sponsors) = patch.author_sponsors {
        let normalized = normalize_author_link_items(author_sponsors);
        let raw = serialize_author_link_items(&normalized)?;
        let _ = save_persisted_app_setting(APP_SETTING_AUTHOR_SPONSORS_KEY, Some(&raw))?;
    }
    if let Some(author_server_recommendations) = patch.author_server_recommendations {
        let normalized = normalize_author_link_items(author_server_recommendations);
        let raw = serialize_author_link_items(&normalized)?;
        let _ =
            save_persisted_app_setting(APP_SETTING_AUTHOR_SERVER_RECOMMENDATIONS_KEY, Some(&raw))?;
    }
    if let Some(proxy_url) = patch.upstream_proxy_url {
        let _ = set_gateway_upstream_proxy_url(Some(&proxy_url))?;
    }
    if let Some(timeout_ms) = patch.upstream_stream_timeout_ms {
        let _ = set_gateway_upstream_stream_timeout_ms(timeout_ms)?;
    }
    if let Some(timeout_ms) = patch.upstream_total_timeout_ms {
        let _ = set_gateway_upstream_total_timeout_ms(timeout_ms)?;
    }
    if let Some(interval_ms) = patch.sse_keepalive_interval_ms {
        let _ = set_gateway_sse_keepalive_interval_ms(interval_ms)?;
    }
    if let Some(quota_guard) = patch.quota_guard {
        let _ = set_gateway_quota_guard(quota_guard)?;
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
    if let Some(mode) = patch.web_auth_mode {
        let _ = crate::set_web_auth_mode(&mode)?;
    }
    if let Some(enabled) = patch.distribution_enabled {
        let _ = crate::set_distribution_enabled(enabled)?;
    }

    Ok(())
}
