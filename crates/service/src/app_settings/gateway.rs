use crate::gateway;
use crate::usage_refresh;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const CODEX_NPM_PACKAGE_NAME: &str = "@openai/codex";
const CODEX_NPM_LATEST_URL: &str = "https://registry.npmjs.org/@openai%2Fcodex/latest";
const CODEX_NPM_DIST_TAG: &str = "latest";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexLatestVersionInfo {
    pub package_name: String,
    pub version: String,
    pub dist_tag: String,
    pub registry_url: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CodexNpmLatestResponse {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
}

use super::{
    get_persisted_app_setting, normalize_optional_text, parse_bool_with_default,
    save_persisted_app_setting, save_persisted_bool_setting,
    APP_SETTING_GATEWAY_ACCOUNT_BATCH_ROTATION_KEY, APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY,
    APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY, APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY,
    APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY,
    APP_SETTING_GATEWAY_MODEL_CATALOG_AUTO_REMOTE_FETCH_KEY,
    APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY, APP_SETTING_GATEWAY_ORIGINATOR_KEY,
    APP_SETTING_GATEWAY_QUOTA_GUARD_KEY, APP_SETTING_GATEWAY_REQUEST_COMPRESSION_ENABLED_KEY,
    APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY, APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY,
    APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
    APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY, APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
    APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY, APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTasksInput {
    pub usage_polling_enabled: Option<bool>,
    pub usage_poll_interval_secs: Option<u64>,
    pub gateway_keepalive_enabled: Option<bool>,
    pub gateway_keepalive_interval_secs: Option<u64>,
    pub token_refresh_polling_enabled: Option<bool>,
    pub token_refresh_poll_interval_secs: Option<u64>,
    pub usage_refresh_workers: Option<usize>,
    pub http_worker_factor: Option<usize>,
    pub http_worker_min: Option<usize>,
    pub http_stream_worker_factor: Option<usize>,
    pub http_stream_worker_min: Option<usize>,
    pub warmup_cron_enabled: Option<bool>,
    pub warmup_cron_expression: Option<String>,
}

impl BackgroundTasksInput {
    /// 函数 `into_patch`
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
    pub(crate) fn into_patch(self) -> usage_refresh::BackgroundTasksSettingsPatch {
        usage_refresh::BackgroundTasksSettingsPatch {
            usage_polling_enabled: self.usage_polling_enabled,
            usage_poll_interval_secs: self.usage_poll_interval_secs,
            gateway_keepalive_enabled: self.gateway_keepalive_enabled,
            gateway_keepalive_interval_secs: self.gateway_keepalive_interval_secs,
            token_refresh_polling_enabled: self.token_refresh_polling_enabled,
            token_refresh_poll_interval_secs: self.token_refresh_poll_interval_secs,
            usage_refresh_workers: self.usage_refresh_workers,
            http_worker_factor: self.http_worker_factor,
            http_worker_min: self.http_worker_min,
            http_stream_worker_factor: self.http_stream_worker_factor,
            http_stream_worker_min: self.http_stream_worker_min,
            warmup_cron_enabled: self.warmup_cron_enabled,
            warmup_cron_expression: self.warmup_cron_expression,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaGuardInput {
    enabled: Option<bool>,
    primary_min_remaining_percent: Option<f64>,
    secondary_min_remaining_percent: Option<f64>,
    allow_all_low_quota_fallback: Option<bool>,
}

impl QuotaGuardInput {
    pub(crate) fn into_config(self) -> gateway::QuotaGuardConfig {
        let current = gateway::current_quota_guard_config();
        gateway::QuotaGuardConfig {
            enabled: self.enabled.unwrap_or(current.enabled),
            primary_min_remaining_percent: self
                .primary_min_remaining_percent
                .unwrap_or(current.primary_min_remaining_percent),
            secondary_min_remaining_percent: self
                .secondary_min_remaining_percent
                .unwrap_or(current.secondary_min_remaining_percent),
            allow_all_low_quota_fallback: self
                .allow_all_low_quota_fallback
                .unwrap_or(current.allow_all_low_quota_fallback),
        }
    }
}

/// 函数 `set_gateway_route_strategy`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - strategy: 参数 strategy
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_route_strategy(strategy: &str) -> Result<String, String> {
    let applied = gateway::set_route_strategy(strategy)?.to_string();
    save_persisted_app_setting(APP_SETTING_GATEWAY_ROUTE_STRATEGY_KEY, Some(&applied))?;
    Ok(applied)
}

/// 函数 `set_gateway_free_account_max_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - model: 参数 model
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_free_account_max_model(model: &str) -> Result<String, String> {
    let applied = gateway::set_free_account_max_model(model)?;
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_FREE_ACCOUNT_MAX_MODEL_KEY,
        Some(&applied),
    )?;
    Ok(applied)
}

/// 函数 `current_gateway_free_account_max_model`
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
pub fn current_gateway_free_account_max_model() -> String {
    gateway::current_free_account_max_model()
}

/// 函数 `set_gateway_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_model_forward_rules(raw: &str) -> Result<String, String> {
    let applied = gateway::set_model_forward_rules(raw)?;
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_MODEL_FORWARD_RULES_KEY,
        if applied.trim().is_empty() {
            None
        } else {
            Some(applied.as_str())
        },
    )?;
    Ok(applied)
}

/// 函数 `current_gateway_model_forward_rules`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
pub fn current_gateway_model_forward_rules() -> String {
    gateway::current_model_forward_rules()
}

pub fn set_gateway_compact_model_forward_rules(raw: &str) -> Result<String, String> {
    let applied = gateway::set_compact_model_forward_rules(raw)?;
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_COMPACT_MODEL_FORWARD_RULES_KEY,
        if applied.trim().is_empty() {
            None
        } else {
            Some(applied.as_str())
        },
    )?;
    Ok(applied)
}

pub fn current_gateway_compact_model_forward_rules() -> String {
    gateway::current_compact_model_forward_rules()
}

pub fn set_gateway_model_catalog_auto_remote_fetch(enabled: bool) -> Result<bool, String> {
    save_persisted_bool_setting(
        APP_SETTING_GATEWAY_MODEL_CATALOG_AUTO_REMOTE_FETCH_KEY,
        enabled,
    )?;
    Ok(enabled)
}

pub fn current_gateway_model_catalog_auto_remote_fetch() -> bool {
    get_persisted_app_setting(APP_SETTING_GATEWAY_MODEL_CATALOG_AUTO_REMOTE_FETCH_KEY)
        .map(|value| parse_bool_with_default(&value, true))
        .unwrap_or(true)
}

/// 函数 `set_gateway_account_max_inflight`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - limit: 参数 limit
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_account_max_inflight(limit: usize) -> Result<usize, String> {
    let applied = gateway::set_account_max_inflight_limit(limit);
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_ACCOUNT_MAX_INFLIGHT_KEY,
        Some(&applied.to_string()),
    )?;
    Ok(applied)
}

/// 函数 `current_gateway_account_max_inflight`
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
pub fn current_gateway_account_max_inflight() -> usize {
    gateway::account_max_inflight_limit()
}

pub fn set_gateway_account_batch_rotation(
    config: gateway::AccountBatchRotationConfig,
) -> Result<gateway::AccountBatchRotationConfig, String> {
    let applied = gateway::set_account_batch_rotation_config(config);
    let raw = serde_json::to_string(&applied)
        .map_err(|err| format!("serialize account batch rotation failed: {err}"))?;
    save_persisted_app_setting(APP_SETTING_GATEWAY_ACCOUNT_BATCH_ROTATION_KEY, Some(&raw))?;
    Ok(applied)
}

pub fn current_gateway_account_batch_rotation() -> gateway::AccountBatchRotationConfig {
    gateway::account_batch_rotation_config()
}

pub fn set_gateway_thread_aware_account_distribution_enabled(
    enabled: bool,
) -> Result<bool, String> {
    let applied = gateway::set_thread_aware_account_distribution_enabled(enabled);
    save_persisted_bool_setting(
        APP_SETTING_GATEWAY_THREAD_AWARE_ACCOUNT_DISTRIBUTION_ENABLED_KEY,
        applied,
    )?;
    Ok(applied)
}

pub fn current_gateway_thread_aware_account_distribution_enabled() -> bool {
    gateway::thread_aware_account_distribution_enabled()
}

pub(crate) fn set_gateway_quota_guard(
    input: QuotaGuardInput,
) -> Result<gateway::QuotaGuardConfig, String> {
    let applied = gateway::set_quota_guard_config(input.into_config());
    let raw = serde_json::to_string(&applied)
        .map_err(|err| format!("serialize quota guard settings failed: {err}"))?;
    save_persisted_app_setting(APP_SETTING_GATEWAY_QUOTA_GUARD_KEY, Some(&raw))?;
    Ok(applied)
}

pub(crate) fn current_gateway_quota_guard() -> gateway::QuotaGuardConfig {
    gateway::current_quota_guard_config()
}

/// 函数 `set_gateway_request_compression_enabled`
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
pub fn set_gateway_request_compression_enabled(enabled: bool) -> Result<bool, String> {
    let applied = gateway::set_request_compression_enabled(enabled);
    save_persisted_bool_setting(APP_SETTING_GATEWAY_REQUEST_COMPRESSION_ENABLED_KEY, applied)?;
    Ok(applied)
}

/// 函数 `current_gateway_request_compression_enabled`
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
pub fn current_gateway_request_compression_enabled() -> bool {
    gateway::request_compression_enabled()
}

/// 函数 `set_gateway_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - originator: 参数 originator
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_originator(originator: &str) -> Result<String, String> {
    let applied = gateway::set_originator(originator)?;
    save_persisted_app_setting(APP_SETTING_GATEWAY_ORIGINATOR_KEY, Some(&applied))?;
    Ok(applied)
}

/// 函数 `current_gateway_originator`
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
pub fn current_gateway_originator() -> String {
    gateway::current_originator()
}

/// 函数 `default_gateway_originator`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 返回 Codex 默认 originator
pub fn default_gateway_originator() -> &'static str {
    gateway::default_originator()
}

/// 函数 `set_gateway_user_agent_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - version: 参数 version
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_user_agent_version(version: &str) -> Result<String, String> {
    let applied = gateway::set_codex_user_agent_version(version)?;
    save_persisted_app_setting(APP_SETTING_GATEWAY_USER_AGENT_VERSION_KEY, Some(&applied))?;
    Ok(applied)
}

/// 函数 `current_gateway_user_agent_version`
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
pub fn current_gateway_user_agent_version() -> String {
    gateway::current_codex_user_agent_version()
}

/// 函数 `default_gateway_user_agent_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 返回 Codex 默认 User-Agent 版本
pub fn default_gateway_user_agent_version() -> &'static str {
    gateway::default_codex_user_agent_version()
}

/// 函数 `fetch_codex_latest_version`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-11
///
/// # 参数
/// 无
///
/// # 返回
/// 返回 npm registry 中 @openai/codex 的 latest 版本
pub fn fetch_codex_latest_version() -> Result<CodexLatestVersionInfo, String> {
    fetch_codex_latest_version_from_url(CODEX_NPM_LATEST_URL)
}

pub(crate) fn sync_gateway_user_agent_version_from_codex_latest() -> Result<String, String> {
    sync_gateway_user_agent_version_from_codex_latest_url(CODEX_NPM_LATEST_URL)
}

fn sync_gateway_user_agent_version_from_codex_latest_url(
    registry_url: &str,
) -> Result<String, String> {
    let latest = fetch_codex_latest_version_from_url(registry_url)?;
    set_gateway_user_agent_version(latest.version.as_str())
}

fn fetch_codex_latest_version_from_url(
    registry_url: &str,
) -> Result<CodexLatestVersionInfo, String> {
    let response = gateway::upstream_client()
        .get(registry_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(Duration::from_secs(10))
        .send()
        .map_err(|err| format!("请求 Codex latest 版本失败: {err}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "获取 Codex latest 版本失败: npm registry 返回 {status}"
        ));
    }
    let payload = response
        .json::<CodexNpmLatestResponse>()
        .map_err(|err| format!("解析 Codex latest 版本失败: {err}"))?;
    let version = payload.version.trim();
    if version.is_empty() {
        return Err("解析 Codex latest 版本失败: 返回中缺少 version".to_string());
    }
    let package_name = payload.name.trim();
    Ok(CodexLatestVersionInfo {
        package_name: if package_name.is_empty() {
            CODEX_NPM_PACKAGE_NAME.to_string()
        } else {
            package_name.to_string()
        },
        version: version.to_string(),
        dist_tag: CODEX_NPM_DIST_TAG.to_string(),
        registry_url: registry_url.to_string(),
    })
}

/// 函数 `set_gateway_residency_requirement`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_residency_requirement(value: Option<&str>) -> Result<Option<String>, String> {
    let normalized = normalize_optional_text(value);
    let applied = gateway::set_residency_requirement(normalized.as_deref())?;
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_RESIDENCY_REQUIREMENT_KEY,
        applied.as_deref(),
    )?;
    Ok(applied)
}

/// 函数 `current_gateway_residency_requirement`
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
pub fn current_gateway_residency_requirement() -> Option<String> {
    gateway::current_residency_requirement()
}

/// 函数 `residency_requirement_options`
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
pub fn residency_requirement_options() -> &'static [&'static str] {
    &["", "us"]
}

/// 函数 `set_gateway_upstream_proxy_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - proxy_url: 参数 proxy_url
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_upstream_proxy_url(proxy_url: Option<&str>) -> Result<Option<String>, String> {
    let normalized = normalize_optional_text(proxy_url);
    let applied = gateway::set_upstream_proxy_url(normalized.as_deref())?;
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_PROXY_URL_KEY,
        applied.as_deref(),
    )?;
    Ok(applied)
}

pub fn current_gateway_upstream_proxy_bypass_hosts() -> String {
    gateway::current_upstream_proxy_bypass_hosts()
}

pub fn set_gateway_upstream_proxy_bypass_hosts(raw: Option<&str>) -> Result<String, String> {
    let applied = gateway::set_upstream_proxy_bypass_hosts(raw);
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_PROXY_BYPASS_HOSTS_KEY,
        if applied.is_empty() {
            None
        } else {
            Some(applied.as_str())
        },
    )?;
    Ok(applied)
}

/// 函数 `set_gateway_upstream_stream_timeout_ms`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - timeout_ms: 参数 timeout_ms
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_upstream_stream_timeout_ms(timeout_ms: u64) -> Result<u64, String> {
    let applied = gateway::set_upstream_stream_timeout_ms(timeout_ms);
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_STREAM_TIMEOUT_MS_KEY,
        Some(&applied.to_string()),
    )?;
    Ok(applied)
}

pub fn set_gateway_upstream_total_timeout_ms(timeout_ms: u64) -> Result<u64, String> {
    let applied = gateway::set_upstream_total_timeout_ms(timeout_ms);
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_UPSTREAM_TOTAL_TIMEOUT_MS_KEY,
        Some(&applied.to_string()),
    )?;
    Ok(applied)
}

/// 函数 `current_gateway_upstream_stream_timeout_ms`
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
pub fn current_gateway_upstream_stream_timeout_ms() -> u64 {
    gateway::current_upstream_stream_timeout_ms()
}

pub fn current_gateway_upstream_total_timeout_ms() -> u64 {
    gateway::current_upstream_total_timeout_ms()
}

/// 函数 `set_gateway_sse_keepalive_interval_ms`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - interval_ms: 参数 interval_ms
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_sse_keepalive_interval_ms(interval_ms: u64) -> Result<u64, String> {
    let applied = gateway::set_sse_keepalive_interval_ms(interval_ms)?;
    save_persisted_app_setting(
        APP_SETTING_GATEWAY_SSE_KEEPALIVE_INTERVAL_MS_KEY,
        Some(&applied.to_string()),
    )?;
    Ok(applied)
}

/// 函数 `current_gateway_sse_keepalive_interval_ms`
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
pub fn current_gateway_sse_keepalive_interval_ms() -> u64 {
    gateway::current_sse_keepalive_interval_ms()
}

/// 函数 `set_gateway_background_tasks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - input: 参数 input
///
/// # 返回
/// 返回函数执行结果
pub fn set_gateway_background_tasks(
    input: BackgroundTasksInput,
) -> Result<serde_json::Value, String> {
    let patch = input.into_patch();
    usage_refresh::validate_background_tasks_settings_patch(&patch)?;
    let applied = usage_refresh::set_background_tasks_settings(patch);
    let raw = serde_json::to_string(&applied)
        .map_err(|err| format!("serialize background tasks failed: {err}"))?;
    save_persisted_app_setting(APP_SETTING_GATEWAY_BACKGROUND_TASKS_KEY, Some(&raw))?;
    serde_json::to_value(applied).map_err(|err| err.to_string())
}

/// 函数 `current_background_tasks_snapshot_value`
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
pub(crate) fn current_background_tasks_snapshot_value() -> Result<serde_json::Value, String> {
    serde_json::to_value(usage_refresh::background_tasks_settings()).map_err(|err| err.to_string())
}

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
