mod items;

use serde_json::Value;

pub(crate) use items::ENV_OVERRIDE_CATALOG;
use items::{APP_SETTINGS_ENV_RESERVED_KEYS, APP_SETTINGS_ENV_UNSUPPORTED_KEYS};

const ENV_OVERRIDE_RISK_LOW: &str = "low";
const ENV_OVERRIDE_RISK_MEDIUM: &str = "medium";
const ENV_OVERRIDE_RISK_HIGH: &str = "high";
const ENV_OVERRIDE_EFFECT_SCOPE_DEPLOYMENT: &str = "deployment";
const ENV_OVERRIDE_EFFECT_SCOPE_RUNTIME_GLOBAL: &str = "runtime-global";
const ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC: &str = "request-semantic";

#[derive(Clone, Copy)]
pub(super) struct EnvOverrideCatalogItem {
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    pub(super) scope: &'static str,
    pub(super) apply_mode: &'static str,
    pub(super) default_value: &'static str,
}

impl EnvOverrideCatalogItem {
    pub(super) const fn new(
        key: &'static str,
        label: &'static str,
        scope: &'static str,
        apply_mode: &'static str,
        default_value: &'static str,
    ) -> Self {
        Self {
            key,
            label,
            scope,
            apply_mode,
            default_value,
        }
    }
}

/// 函数 `env_override_reserved_keys`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn env_override_reserved_keys() -> &'static [&'static str] {
    APP_SETTINGS_ENV_RESERVED_KEYS
}

/// 函数 `env_override_unsupported_keys`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn env_override_unsupported_keys() -> &'static [&'static str] {
    APP_SETTINGS_ENV_UNSUPPORTED_KEYS
}

/// 函数 `editable_env_override_catalog`
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
pub(super) fn editable_env_override_catalog(
) -> impl Iterator<Item = &'static EnvOverrideCatalogItem> {
    ENV_OVERRIDE_CATALOG
        .iter()
        .filter(|item| !is_env_override_reserved_key(item.key))
}

/// 函数 `env_override_catalog_item`
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
pub(super) fn env_override_catalog_item(key: &str) -> Option<&'static EnvOverrideCatalogItem> {
    editable_env_override_catalog().find(|item| item.key.eq_ignore_ascii_case(key))
}

/// 函数 `is_env_override_catalog_key`
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
pub(super) fn is_env_override_catalog_key(key: &str) -> bool {
    env_override_catalog_item(key).is_some()
}

/// 函数 `is_env_override_unsupported_key`
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
pub(super) fn is_env_override_unsupported_key(key: &str) -> bool {
    APP_SETTINGS_ENV_UNSUPPORTED_KEYS
        .iter()
        .any(|item| item.eq_ignore_ascii_case(key))
}

/// 函数 `is_env_override_reserved_key`
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
pub(super) fn is_env_override_reserved_key(key: &str) -> bool {
    APP_SETTINGS_ENV_RESERVED_KEYS
        .iter()
        .any(|item| item.eq_ignore_ascii_case(key))
}

fn normalized_env_key(key: &str) -> String {
    key.trim().to_ascii_uppercase()
}

fn env_override_effect_scope(key: &str) -> &'static str {
    let key = normalized_env_key(key);
    match key.as_str() {
        "CODEXMANAGER_ACCOUNT_MAX_INFLIGHT"
        | "CODEXMANAGER_COMPACT_API_PATH"
        | "CODEXMANAGER_CODEX_IMAGE_GENERATION_ENABLED"
        | "CODEXMANAGER_CODEX_IMAGE_MAIN_MODEL"
        | "CODEXMANAGER_CODEX_IMAGE_TOOL_MODEL"
        | "CODEXMANAGER_FRONT_PROXY_MAX_BODY_BYTES"
        | "CODEXMANAGER_FRONT_PROXY_ZSTD_MAX_BODY_BYTES"
        | "CODEXMANAGER_GATEWAY_BLOCKED_PATHS"
        | "CODEXMANAGER_GATEWAY_ASYNC_STREAM_WORKERS"
        | "CODEXMANAGER_HTTP_BRIDGE_OUTPUT_TEXT_LIMIT_BYTES"
        | "CODEXMANAGER_HTTP_TIMEOUT_MS"
        | "CODEXMANAGER_MAX_TEXT_INPUT_CHARS"
        | "CODEXMANAGER_PROMPT_CACHE_CAPACITY"
        | "CODEXMANAGER_PROMPT_CACHE_CLEANUP_INTERVAL_SECS"
        | "CODEXMANAGER_PROMPT_CACHE_TTL_SECS"
        | "CODEXMANAGER_PROXY_LIST"
        | "CODEXMANAGER_REQUEST_GATE_WAIT_TIMEOUT_MS"
        | "CODEXMANAGER_ROUTE_HEALTH_P2C_BALANCED_WINDOW"
        | "CODEXMANAGER_ROUTE_HEALTH_P2C_ENABLED"
        | "CODEXMANAGER_ROUTE_HEALTH_P2C_ORDERED_WINDOW"
        | "CODEXMANAGER_ROUTE_STATE_CAPACITY"
        | "CODEXMANAGER_ROUTE_STATE_TTL_SECS"
        | "CODEXMANAGER_STRICT_REQUEST_PARAM_ALLOWLIST"
        | "CODEXMANAGER_UPSTREAM_BASE_URL"
        | "CODEXMANAGER_UPSTREAM_TOTAL_TIMEOUT_MS" => ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC,
        "CODEXMANAGER_GITHUB_TOKEN"
        | "CODEXMANAGER_NO_SERVICE"
        | "CODEXMANAGER_UPDATE_PRERELEASE"
        | "CODEXMANAGER_UPDATE_REPO"
        | "CODEXMANAGER_WEB_ADDR"
        | "CODEXMANAGER_WEB_NO_OPEN"
        | "CODEXMANAGER_WEB_NO_SPAWN_SERVICE"
        | "CODEXMANAGER_WEB_ROOT" => ENV_OVERRIDE_EFFECT_SCOPE_DEPLOYMENT,
        _ => ENV_OVERRIDE_EFFECT_SCOPE_RUNTIME_GLOBAL,
    }
}

fn env_override_risk_level(key: &str) -> &'static str {
    match env_override_effect_scope(key) {
        ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC => ENV_OVERRIDE_RISK_HIGH,
        ENV_OVERRIDE_EFFECT_SCOPE_RUNTIME_GLOBAL => ENV_OVERRIDE_RISK_MEDIUM,
        _ => ENV_OVERRIDE_RISK_LOW,
    }
}

fn env_override_safety_note(key: &str) -> &'static str {
    match env_override_effect_scope(key) {
        ENV_OVERRIDE_EFFECT_SCOPE_REQUEST_SEMANTIC => {
            "会影响请求语义、上游目标、请求体限制或路由行为；只建议排障时临时修改。"
        }
        ENV_OVERRIDE_EFFECT_SCOPE_RUNTIME_GLOBAL => {
            "会影响运行时全局行为；修改后请观察请求链路和后台任务是否稳定。"
        }
        _ => "部署级兼容覆盖项；通常需要重新启动相关进程后再确认效果。",
    }
}

/// 函数 `env_override_catalog_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn env_override_catalog_value() -> Vec<Value> {
    editable_env_override_catalog()
        .map(|item| {
            serde_json::json!({
                "key": item.key,
                "label": item.label,
                "scope": item.scope,
                "applyMode": item.apply_mode,
                "defaultValue": super::snapshot::env_override_default_value(item.key),
                "riskLevel": env_override_risk_level(item.key),
                "effectScope": env_override_effect_scope(item.key),
                "safetyNote": env_override_safety_note(item.key),
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
