use crate::gateway;
use crate::usage_refresh;

use super::{
    apply_env_overrides_to_process, list_app_settings_map, normalize_optional_text,
    persisted_env_overrides_missing_process_env, reload_runtime_after_env_override_apply,
    set_service_bind_mode, BackgroundTasksInput, QuotaGuardInput,
    APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY, APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY, APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
    APP_SETTING_GATEWAY_ORIGINATOR_KEY, APP_SETTING_GATEWAY_QUOTA_GUARD_KEY,
    APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY, APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY, APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY,
    SERVICE_BIND_MODE_SETTING_KEY,
};

/// 函数 `process_env_has_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn process_env_has_value(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// 函数 `any_process_env_has_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - names: 参数 names
///
/// # 返回
/// 返回函数执行结果
fn any_process_env_has_value(names: &[&str]) -> bool {
    names.iter().any(|name| process_env_has_value(name))
}

/// 函数 `sync_runtime_settings_from_storage`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
pub fn sync_runtime_settings_from_storage() {
    let settings = list_app_settings_map();
    let env_overrides = persisted_env_overrides_missing_process_env();
    if !env_overrides.is_empty() {
        apply_env_overrides_to_process(&env_overrides, &env_overrides);
    }
    reload_runtime_after_env_override_apply();

    if !process_env_has_value("CODEXMANAGER_SERVICE_ADDR") {
        if let Some(mode) = settings.get(SERVICE_BIND_MODE_SETTING_KEY) {
            let _ = set_service_bind_mode(mode);
        }
    }
    if !process_env_has_value("CODEXMANAGER_ROUTE_STRATEGY") {
        if let Some(strategy) = settings.get(APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY) {
            if let Some(strategy) = normalize_optional_text(Some(strategy)) {
                if let Err(err) = gateway::set_route_strategy(&strategy) {
                    log::warn!("sync persisted route strategy failed: {err}");
                }
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_FREE_ACCOUNT_MAX_MODEL") {
        if let Some(model) = settings.get(APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY) {
            if let Some(model) = normalize_optional_text(Some(model)) {
                if let Err(err) = gateway::set_free_account_max_model(&model) {
                    log::warn!("sync persisted free account max model failed: {err}");
                }
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_MODEL_FORWARD_RULES") {
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY) {
            if let Err(err) = gateway::set_model_forward_rules(raw) {
                log::warn!("sync persisted model forward rules failed: {err}");
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_ACCOUNT_MAX_INFLIGHT") {
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY) {
            if let Ok(limit) = raw.trim().parse::<usize>() {
                gateway::set_account_max_inflight_limit(limit);
            } else {
                log::warn!("parse persisted account max inflight failed: {raw}");
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_ORIGINATOR") {
        if let Some(originator) = settings.get(APP_SETTING_GATEWAY_ORIGINATOR_KEY) {
            if let Some(originator) = normalize_optional_text(Some(originator)) {
                if let Err(err) = gateway::set_originator(&originator) {
                    log::warn!("sync persisted gateway originator failed: {err}");
                }
            }
        }
    }
    if let Some(version) = settings.get(APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY) {
        if let Some(version) = normalize_optional_text(Some(version)) {
            if let Err(err) = gateway::set_codex_user_agent_version(&version) {
                log::warn!("sync persisted gateway user agent version failed: {err}");
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_RESIDENCY_REQUIREMENT") {
        if let Some(residency_requirement) =
            settings.get(APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY)
        {
            let normalized = normalize_optional_text(Some(residency_requirement));
            if let Err(err) = gateway::set_residency_requirement(normalized.as_deref()) {
                log::warn!("sync persisted gateway residency requirement failed: {err}");
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_UPSTREAM_PROXY_URL") {
        if let Some(proxy_url) = settings.get(APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY) {
            let normalized = normalize_optional_text(Some(proxy_url));
            if let Err(err) = gateway::set_upstream_proxy_url(normalized.as_deref()) {
                log::warn!("sync persisted upstream proxy failed: {err}");
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_UPSTREAM_STREAM_TIMEOUT_MS") {
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY) {
            if let Ok(timeout_ms) = raw.trim().parse::<u64>() {
                gateway::set_upstream_stream_timeout_ms(timeout_ms);
            } else {
                log::warn!("parse persisted upstream stream timeout failed: {raw}");
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS") {
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY) {
            if let Ok(timeout_ms) = raw.trim().parse::<u64>() {
                gateway::set_upstream_total_timeout_ms(timeout_ms);
            } else {
                log::warn!("parse persisted upstream total timeout failed: {raw}");
            }
        }
    }
    if !process_env_has_value("CODEXMANAGER_SSE_KEEPALIVE_INTERVAL_MS") {
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY) {
            if let Ok(interval_ms) = raw.trim().parse::<u64>() {
                if let Err(err) = gateway::set_sse_keepalive_interval_ms(interval_ms) {
                    log::warn!("sync persisted sse keepalive interval failed: {err}");
                }
            } else {
                log::warn!("parse persisted sse keepalive interval failed: {raw}");
            }
        }
    }
    if !any_process_env_has_value(&[
        "CODEXMANAGER_QUOTA_GUARD_ENABLED",
        "CODEXMANAGER_QUOTA_GUARD_5H_MIN_REMAINING_PERCENT",
        "CODEXMANAGER_QUOTA_GUARD_WEEKLY_MIN_REMAINING_PERCENT",
        "CODEXMANAGER_QUOTA_GUARD_ALLOW_ALL_LOW_FALLBACK",
        "CODEXMANAGER_LOW_QUOTA_THRESHOLD_PERCENT",
    ]) {
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_QUOTA_GUARD_KEY) {
            match serde_json::from_str::<QuotaGuardInput>(raw) {
                Ok(input) => {
                    let _ = gateway::set_quota_guard_config(input.into_config());
                }
                Err(err) => {
                    log::warn!("parse persisted quota guard settings failed: {err}");
                }
            }
        }
    }
    if !any_process_env_has_value(&[
        "CODEXMANAGER_USAGE_POLLING_ENABLED",
        "CODEXMANAGER_USAGE_POLL_INTERVAL_SECS",
        "CODEXMANAGER_GATEWAY_KEEPALIVE_ENABLED",
        "CODEXMANAGER_GATEWAY_KEEPALIVE_INTERVAL_SECS",
        "CODEXMANAGER_TOKEN_REFRESH_POLLING_ENABLED",
        "CODEXMANAGER_TOKEN_REFRESH_POLL_INTERVAL_SECS",
        "CODEXMANAGER_USAGE_REFRESH_WORKERS",
        "CODEXMANAGER_HTTP_WORKER_FACTOR",
        "CODEXMANAGER_HTTP_WORKER_MIN",
        "CODEXMANAGER_HTTP_STREAM_WORKER_FACTOR",
        "CODEXMANAGER_HTTP_STREAM_WORKER_MIN",
    ]) {
        if let Some(raw) = settings.get(APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY) {
            match serde_json::from_str::<BackgroundTasksInput>(raw) {
                Ok(input) => {
                    usage_refresh::set_background_tasks_settings(input.into_patch());
                }
                Err(err) => {
                    log::warn!("parse persisted background tasks failed: {err}");
                }
            }
        }
    }
}
