use crate::initialize_storage_if_needed;
use crate::web_access_password_configured;
use codexmanager_core::rpc::types::ModelOption;
use serde_json::Value;
use std::collections::BTreeMap;

use super::{
    current_background_tasks_snapshot_value, current_close_to_tray_on_close_setting,
    current_env_overrides, current_gateway_free_account_max_model,
    current_gateway_sse_keepalive_interval_ms, current_gateway_upstream_stream_timeout_ms,
    current_lightweight_mode_on_close_to_tray_setting, current_saved_service_addr,
    current_service_bind_mode, current_ui_low_transparency_enabled, current_ui_theme,
    current_update_auto_check_enabled, env_override_catalog_value, env_override_reserved_keys,
    env_override_unsupported_keys, save_env_overrides_value, save_persisted_app_setting,
    save_persisted_bool_setting, sync_runtime_settings_from_storage,
    APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY, APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
    APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY, APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY, APP_SETTING_SERVICE_ADDR_KEY,
    APP_SETTING_UI_LOW_TRANSPARENCY_KEY, APP_SETTING_UI_THEME_KEY,
    APP_SETTING_UPDATE_AUTO_CHECK_KEY, SERVICE_BIND_MODE_ALL_INTERFACES,
    SERVICE_BIND_MODE_LOOPBACK, SERVICE_BIND_MODE_SETTING_KEY,
};

const DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS: &[&str] = &[
    "gpt-5",
    "gpt-5-codex",
    "gpt-5-codex-mini",
    "gpt-5.1",
    "gpt-5.1-codex",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
    "gpt-5.2",
    "gpt-5.2-codex",
    "gpt-5.3-codex",
    "gpt-5.4",
];

pub(super) fn current_app_settings_value(
    close_to_tray_on_close: Option<bool>,
    close_to_tray_supported: Option<bool>,
) -> Result<Value, String> {
    initialize_storage_if_needed()?;
    sync_runtime_settings_from_storage();
    let background_tasks = current_background_tasks_snapshot_value()?;
    let update_auto_check = current_update_auto_check_enabled();
    let persisted_close_to_tray = current_close_to_tray_on_close_setting();
    let close_to_tray = close_to_tray_on_close.unwrap_or(persisted_close_to_tray);
    let lightweight_mode_on_close_to_tray = current_lightweight_mode_on_close_to_tray_setting();
    let low_transparency = current_ui_low_transparency_enabled();
    let theme = current_ui_theme();
    let service_addr = current_saved_service_addr();
    let service_listen_mode = current_service_bind_mode();
    let route_strategy = crate::gateway::current_route_strategy().to_string();
    let free_account_max_model = current_gateway_free_account_max_model();
    let free_account_max_model_options =
        load_free_account_max_model_options(&free_account_max_model);
    let cpa_no_cookie_header_mode_enabled = crate::gateway::cpa_no_cookie_header_mode_enabled();
    let upstream_proxy_url = crate::gateway::current_upstream_proxy_url();
    let upstream_stream_timeout_ms = current_gateway_upstream_stream_timeout_ms();
    let sse_keepalive_interval_ms = current_gateway_sse_keepalive_interval_ms();
    let background_tasks_raw = serde_json::to_string(&background_tasks)
        .map_err(|err| format!("serialize background tasks failed: {err}"))?;
    let env_overrides = current_env_overrides();

    persist_current_snapshot(
        update_auto_check,
        persisted_close_to_tray,
        lightweight_mode_on_close_to_tray,
        low_transparency,
        &theme,
        &service_addr,
        &service_listen_mode,
        &route_strategy,
        &free_account_max_model,
        cpa_no_cookie_header_mode_enabled,
        upstream_proxy_url.as_deref(),
        upstream_stream_timeout_ms,
        sse_keepalive_interval_ms,
        &background_tasks_raw,
        &env_overrides,
    );

    Ok(serde_json::json!({
        "updateAutoCheck": update_auto_check,
        "closeToTrayOnClose": close_to_tray,
        "closeToTraySupported": close_to_tray_supported,
        "lightweightModeOnCloseToTray": lightweight_mode_on_close_to_tray,
        "lowTransparency": low_transparency,
        "theme": theme,
        "serviceAddr": service_addr,
        "serviceListenMode": service_listen_mode,
        "serviceListenModeOptions": [
            SERVICE_BIND_MODE_LOOPBACK,
            SERVICE_BIND_MODE_ALL_INTERFACES
        ],
        "routeStrategy": route_strategy,
        "routeStrategyOptions": ["ordered", "balanced"],
        "freeAccountMaxModel": free_account_max_model,
        "freeAccountMaxModelOptions": free_account_max_model_options,
        "cpaNoCookieHeaderModeEnabled": cpa_no_cookie_header_mode_enabled,
        "upstreamProxyUrl": upstream_proxy_url.unwrap_or_default(),
        "upstreamStreamTimeoutMs": upstream_stream_timeout_ms,
        "sseKeepaliveIntervalMs": sse_keepalive_interval_ms,
        "backgroundTasks": background_tasks,
        "envOverrides": env_overrides,
        "envOverrideCatalog": env_override_catalog_value(),
        "envOverrideReservedKeys": env_override_reserved_keys(),
        "envOverrideUnsupportedKeys": env_override_unsupported_keys(),
        "webAccessPasswordConfigured": web_access_password_configured(),
    }))
}

fn load_free_account_max_model_options(current: &str) -> Vec<String> {
    let cached = crate::apikey_models::read_model_options(false)
        .map(|result| result.items)
        .unwrap_or_default();
    collect_free_account_max_model_options(current, &cached)
}

fn collect_free_account_max_model_options(current: &str, cached: &[ModelOption]) -> Vec<String> {
    let mut items = cached
        .iter()
        .map(|item| item.slug.trim().to_ascii_lowercase())
        .filter(|slug| is_free_account_max_model_option(slug))
        .fold(Vec::<String>::new(), |mut acc, slug| {
            if !acc.iter().any(|item| item == &slug) {
                acc.push(slug);
            }
            acc
        });

    if items.is_empty() {
        items = DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS
            .iter()
            .map(|item| (*item).to_string())
            .collect();
    }

    let normalized_current = current.trim().to_ascii_lowercase();
    if is_free_account_max_model_option(&normalized_current)
        && !items.iter().any(|item| item == &normalized_current)
    {
        items.push(normalized_current);
    }

    items
}

fn is_free_account_max_model_option(slug: &str) -> bool {
    let normalized = slug.trim().to_ascii_lowercase();
    !normalized.is_empty() && normalized.starts_with("gpt-") && normalized != "gpt-5.4-pro"
}

fn persist_current_snapshot(
    update_auto_check: bool,
    persisted_close_to_tray: bool,
    lightweight_mode_on_close_to_tray: bool,
    low_transparency: bool,
    theme: &str,
    service_addr: &str,
    service_listen_mode: &str,
    route_strategy: &str,
    free_account_max_model: &str,
    cpa_no_cookie_header_mode_enabled: bool,
    upstream_proxy_url: Option<&str>,
    upstream_stream_timeout_ms: u64,
    sse_keepalive_interval_ms: u64,
    background_tasks_raw: &str,
    env_overrides: &BTreeMap<String, String>,
) {
    let _ = save_persisted_bool_setting(APP_SETTING_UPDATE_AUTO_CHECK_KEY, update_auto_check);
    let _ = save_persisted_bool_setting(
        APP_SETTING_CLOSE_TO_TRAY_ON_CLOSE_KEY,
        persisted_close_to_tray,
    );
    let _ = save_persisted_bool_setting(
        APP_SETTING_LIGHTWEIGHT_MODE_ON_CLOSE_TO_TRAY_KEY,
        lightweight_mode_on_close_to_tray,
    );
    let _ = save_persisted_bool_setting(APP_SETTING_UI_LOW_TRANSPARENCY_KEY, low_transparency);
    let _ = save_persisted_app_setting(APP_SETTING_UI_THEME_KEY, Some(theme));
    let _ = save_persisted_app_setting(APP_SETTING_SERVICE_ADDR_KEY, Some(service_addr));
    let _ = save_persisted_app_setting(SERVICE_BIND_MODE_SETTING_KEY, Some(service_listen_mode));
    let _ =
        save_persisted_app_setting(APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY, Some(route_strategy));
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY,
        Some(free_account_max_model),
    );
    let _ = save_persisted_bool_setting(
        APP_SETTING_GATEWAY_CPA_NO_COOKIE_HEADER_MODE_KEY,
        cpa_no_cookie_header_mode_enabled,
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
        upstream_proxy_url,
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
        Some(&upstream_stream_timeout_ms.to_string()),
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
        Some(&sse_keepalive_interval_ms.to_string()),
    );
    let _ = save_persisted_app_setting(
        APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
        Some(background_tasks_raw),
    );
    let _ = save_env_overrides_value(env_overrides);
}

#[cfg(test)]
mod tests {
    use super::{collect_free_account_max_model_options, DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS};
    use codexmanager_core::rpc::types::ModelOption;

    #[test]
    fn free_account_max_model_options_fallback_to_curated_defaults() {
        let actual = collect_free_account_max_model_options("gpt-5.2", &[]);
        let expected = DEFAULT_FREE_ACCOUNT_MAX_MODEL_OPTIONS
            .iter()
            .map(|item| (*item).to_string())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn free_account_max_model_options_reuse_cached_model_picker_options() {
        let actual = collect_free_account_max_model_options(
            "gpt-5.2",
            &[
                ModelOption {
                    slug: "gpt-5".to_string(),
                    display_name: "gpt-5".to_string(),
                },
                ModelOption {
                    slug: "gpt-5.1-codex".to_string(),
                    display_name: "gpt-5.1-codex".to_string(),
                },
                ModelOption {
                    slug: "gpt-5.4-pro".to_string(),
                    display_name: "gpt-5.4-pro".to_string(),
                },
                ModelOption {
                    slug: "o3".to_string(),
                    display_name: "o3".to_string(),
                },
                ModelOption {
                    slug: "gpt-5.1-codex".to_string(),
                    display_name: "gpt-5.1-codex".to_string(),
                },
            ],
        );

        assert_eq!(
            actual,
            vec![
                "gpt-5".to_string(),
                "gpt-5.1-codex".to_string(),
                "gpt-5.2".to_string()
            ]
        );
    }
}
