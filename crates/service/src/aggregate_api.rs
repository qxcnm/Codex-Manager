use codexmanager_core::rpc::types::{
    AggregateApiAssociateModelsResult, AggregateApiBalanceRefreshResult,
    AggregateApiBalanceSnapshot, AggregateApiCreateResult, AggregateApiFetchModelsResult,
    AggregateApiFetchedModel, AggregateApiSecretResult, AggregateApiSummary,
    AggregateApiTestResult,
};
use codexmanager_core::storage::{
    now_ts, AggregateApi, ManagedModelV2, ManagedModelV2Upsert, ModelFastPolicyV2, ModelPriceV2,
    ModelRouteV2,
};
use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

use crate::apikey_profile::normalize_upstream_base_url;
use crate::gateway;
use crate::storage_helpers::{generate_aggregate_api_id, open_storage};

mod network;
mod operations;
#[cfg(test)]
use network::{probe_claude_endpoint, probe_codex_endpoint, query_generic_balance_path};
pub(crate) use operations::{
    fetch_aggregate_api_models_async, refresh_aggregate_api_balance_async, run_aggregate_future,
    run_aggregate_storage, test_aggregate_api_connection_async,
};

pub(crate) const AGGREGATE_API_PROVIDER_CODEX: &str = "codex";
pub(crate) const AGGREGATE_API_PROVIDER_CLAUDE: &str = "claude";
pub(crate) const AGGREGATE_API_PROVIDER_GEMINI: &str = "gemini";
pub(crate) const AGGREGATE_API_PROVIDER_COMPATIBLE: &str = "compatible";
pub(crate) const AGGREGATE_API_AUTH_APIKEY: &str = "apikey";
pub(crate) const AGGREGATE_API_AUTH_USERPASS: &str = "userpass";
const AGGREGATE_API_BALANCE_TEMPLATE_GENERIC: &str = "generic";
const AGGREGATE_API_BALANCE_TEMPLATE_NEW_API: &str = "new_api";
const AGGREGATE_API_BALANCE_TEMPLATE_CUSTOM: &str = "custom";
const CUSTOM_BALANCE_AUTH_PROVIDER_BEARER: &str = "provider_bearer";
const CUSTOM_BALANCE_AUTH_BALANCE_BEARER: &str = "balance_bearer";
const CUSTOM_BALANCE_AUTH_NONE: &str = "none";
const MAX_FETCHED_MODELS: usize = 500;
const MAX_MODELS_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_AGGREGATE_API_USER_AGENT_BYTES: usize = 512;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UserPassSecret {
    username: String,
    password: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CustomBalanceQueryConfig {
    #[serde(default)]
    method: Option<String>,
    path: String,
    #[serde(default)]
    auth: Option<String>,
    remaining_path: String,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    multiplier: Option<f64>,
    #[serde(default)]
    total_path: Option<String>,
    #[serde(default)]
    used_path: Option<String>,
    #[serde(default)]
    plan_path: Option<String>,
    #[serde(default)]
    valid_path: Option<String>,
    #[serde(default)]
    invalid_message_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyAuthParams {
    location: String,
    name: String,
    #[serde(default)]
    header_value_format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPassAuthParams {
    mode: String,
    #[serde(default)]
    username_name: Option<String>,
    #[serde(default)]
    password_name: Option<String>,
}

/// 函数 `normalize_secret`
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
fn normalize_secret(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 函数 `normalize_supplier_name`
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
fn normalize_supplier_name(value: Option<String>) -> Result<String, String> {
    let normalized = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "supplier name is required".to_string())?;
    Ok(normalized)
}

/// 函数 `normalize_sort`
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
fn normalize_sort(value: Option<i64>) -> i64 {
    value.unwrap_or(0)
}

fn normalize_status(value: Option<String>) -> Result<String, String> {
    match value {
        Some(raw) => {
            let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
            match normalized.as_str() {
                "active" | "enabled" | "enable" => Ok("active".to_string()),
                "disabled" | "disable" | "inactive" => Ok("disabled".to_string()),
                other => Err(format!("unsupported aggregate api status: {other}")),
            }
        }
        None => Ok("active".to_string()),
    }
}

fn normalize_auth_type(value: Option<String>) -> Result<String, String> {
    match value {
        Some(raw) => {
            let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
            match normalized.as_str() {
                "apikey" | "api_key" | "key" => Ok(AGGREGATE_API_AUTH_APIKEY.to_string()),
                "userpass" | "username_password" | "account_password" | "basic" | "http_basic" => {
                    Ok(AGGREGATE_API_AUTH_USERPASS.to_string())
                }
                other => Err(format!("unsupported aggregate api auth type: {other}")),
            }
        }
        None => Ok(AGGREGATE_API_AUTH_APIKEY.to_string()),
    }
}

fn normalize_action(value: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let normalized = trimmed.to_string();
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Err("aggregate api action must be a path, not a full url".to_string());
    }
    if normalized.contains("://") {
        return Err("aggregate api action is invalid".to_string());
    }
    let with_slash = if normalized.starts_with('/') {
        normalized
    } else {
        format!("/{normalized}")
    };
    Ok(Some(with_slash))
}

fn normalize_model_override(value: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    if trimmed
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':')))
    {
        return Err("aggregate api modelOverride contains unsupported characters".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

fn normalize_balance_query_template(value: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
    if normalized.is_empty() {
        return Ok(None);
    }
    match normalized.as_str() {
        AGGREGATE_API_BALANCE_TEMPLATE_GENERIC => {
            Ok(Some(AGGREGATE_API_BALANCE_TEMPLATE_GENERIC.to_string()))
        }
        "newapi" | "new_api" => Ok(Some(AGGREGATE_API_BALANCE_TEMPLATE_NEW_API.to_string())),
        "custom" | "custom_json" => Ok(Some(AGGREGATE_API_BALANCE_TEMPLATE_CUSTOM.to_string())),
        other => Err(format!(
            "unsupported aggregate api balance template: {other}"
        )),
    }
}

fn default_balance_query_template(template: Option<String>) -> String {
    template.unwrap_or_else(|| AGGREGATE_API_BALANCE_TEMPLATE_GENERIC.to_string())
}

fn normalize_custom_balance_method(value: Option<String>) -> Result<String, String> {
    let method = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("GET")
        .to_ascii_uppercase();
    match method.as_str() {
        "GET" | "POST" => Ok(method),
        _ => Err("custom balance method must be GET or POST".to_string()),
    }
}

fn normalize_custom_balance_auth(value: Option<String>) -> Result<String, String> {
    let auth = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(CUSTOM_BALANCE_AUTH_PROVIDER_BEARER)
        .to_ascii_lowercase()
        .replace('-', "_");
    match auth.as_str() {
        "provider" | "provider_bearer" | "api_key" | "apikey" => {
            Ok(CUSTOM_BALANCE_AUTH_PROVIDER_BEARER.to_string())
        }
        "balance" | "balance_bearer" | "access_token" => {
            Ok(CUSTOM_BALANCE_AUTH_BALANCE_BEARER.to_string())
        }
        "none" | "no_auth" => Ok(CUSTOM_BALANCE_AUTH_NONE.to_string()),
        _ => Err("custom balance auth is invalid".to_string()),
    }
}

fn normalize_custom_balance_endpoint_path(value: String) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("custom balance path is required".to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Err("custom balance path must be relative, not a full url".to_string());
    }
    if trimmed.contains("://") {
        return Err("custom balance path is invalid".to_string());
    }
    Ok(if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    })
}

fn normalize_custom_balance_json_path(
    value: Option<String>,
    field_name: &str,
    required: bool,
) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        if required {
            return Err(format!("custom balance {field_name} is required"));
        }
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        if required {
            return Err(format!("custom balance {field_name} is required"));
        }
        return Ok(None);
    }
    for segment in trimmed.split('.') {
        if segment.is_empty() {
            return Err(format!(
                "custom balance {field_name} contains an empty segment"
            ));
        }
        if !segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        {
            return Err(format!(
                "custom balance {field_name} contains unsupported characters"
            ));
        }
    }
    Ok(Some(trimmed.to_string()))
}

fn normalize_custom_balance_unit(value: Option<String>) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(Some("USD".to_string()));
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Some("USD".to_string()));
    }
    if trimmed.chars().count() > 16 {
        return Err("custom balance unit is too long".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

fn normalize_custom_balance_multiplier(value: Option<f64>) -> Result<Option<f64>, String> {
    let multiplier = value.unwrap_or(1.0);
    if !multiplier.is_finite() || multiplier <= 0.0 {
        return Err("custom balance multiplier must be greater than 0".to_string());
    }
    Ok(Some(multiplier))
}

fn normalize_custom_balance_query_config(value: Option<String>) -> Result<Option<String>, String> {
    let raw = normalize_optional_text(value)
        .ok_or_else(|| "custom balance query config is required".to_string())?;
    if raw.len() > 4096 {
        return Err("custom balance query config is too large".to_string());
    }
    let mut config: CustomBalanceQueryConfig = serde_json::from_str(raw.as_str())
        .map_err(|_| "custom balance query config is invalid JSON".to_string())?;
    config.method = Some(normalize_custom_balance_method(config.method.take())?);
    config.path = normalize_custom_balance_endpoint_path(config.path)?;
    config.auth = Some(normalize_custom_balance_auth(config.auth.take())?);
    config.remaining_path =
        normalize_custom_balance_json_path(Some(config.remaining_path), "remainingPath", true)?
            .expect("required remainingPath");
    config.unit = normalize_custom_balance_unit(config.unit.take())?;
    config.multiplier = normalize_custom_balance_multiplier(config.multiplier)?;
    config.total_path =
        normalize_custom_balance_json_path(config.total_path.take(), "totalPath", false)?;
    config.used_path =
        normalize_custom_balance_json_path(config.used_path.take(), "usedPath", false)?;
    config.plan_path =
        normalize_custom_balance_json_path(config.plan_path.take(), "planPath", false)?;
    config.valid_path =
        normalize_custom_balance_json_path(config.valid_path.take(), "validPath", false)?;
    config.invalid_message_path = normalize_custom_balance_json_path(
        config.invalid_message_path.take(),
        "invalidMessagePath",
        false,
    )?;
    serde_json::to_string(&config)
        .map(Some)
        .map_err(|_| "serialize custom balance query config failed".to_string())
}

fn normalize_balance_query_config_json(
    template: Option<&str>,
    value: Option<String>,
) -> Result<Option<String>, String> {
    if template == Some(AGGREGATE_API_BALANCE_TEMPLATE_CUSTOM) {
        return normalize_custom_balance_query_config(value);
    }
    Ok(None)
}

fn normalize_optional_url(
    value: Option<String>,
    field_name: &str,
) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed =
        reqwest::Url::parse(trimmed.as_str()).map_err(|_| format!("invalid {field_name}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(format!("invalid {field_name} scheme"));
    }
    Ok(Some(trimmed))
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_aggregate_api_user_agent(value: Option<String>) -> Result<Option<String>, String> {
    let Some(value) = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if value.len() > MAX_AGGREGATE_API_USER_AGENT_BYTES {
        return Err(format!(
            "aggregate api user agent must not exceed {MAX_AGGREGATE_API_USER_AGENT_BYTES} bytes"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("aggregate api user agent contains control characters".to_string());
    }
    if !value.is_ascii() {
        return Err("aggregate api user agent is not a valid HTTP header value".to_string());
    }
    HeaderValue::from_str(value)
        .map_err(|_| "aggregate api user agent is not a valid HTTP header value".to_string())?;
    Ok(Some(value.to_string()))
}

pub(crate) fn resolved_aggregate_api_user_agent(api: &AggregateApi) -> Result<String, String> {
    Ok(normalize_aggregate_api_user_agent(api.user_agent.clone())?
        .unwrap_or_else(gateway::current_gateway_user_agent))
}

fn normalize_auth_params_json(
    auth_type: &str,
    enabled: Option<bool>,
    auth_params: Option<serde_json::Value>,
) -> Result<Option<String>, String> {
    match enabled {
        None => Ok(None),
        Some(false) => Ok(Some(String::new())),
        Some(true) => {
            let value = auth_params.ok_or_else(|| "authParams is required".to_string())?;
            let obj = value
                .as_object()
                .ok_or_else(|| "authParams must be a JSON object".to_string())?;
            if obj.is_empty() {
                return Err("authParams must not be empty".to_string());
            }
            if auth_type == AGGREGATE_API_AUTH_APIKEY {
                let parsed: ApiKeyAuthParams = serde_json::from_value(value.clone())
                    .map_err(|_| "authParams is invalid".to_string())?;
                let location = parsed.location.trim().to_ascii_lowercase();
                if location != "header" && location != "query" {
                    return Err("authParams.location must be header or query".to_string());
                }
                if parsed.name.trim().is_empty() {
                    return Err("authParams.name is required".to_string());
                }
                if location == "header" {
                    let format = parsed
                        .header_value_format
                        .as_deref()
                        .unwrap_or("bearer")
                        .trim()
                        .to_ascii_lowercase();
                    if format != "bearer" && format != "raw" {
                        return Err(
                            "authParams.headerValueFormat must be bearer or raw".to_string()
                        );
                    }
                }
            } else if auth_type == AGGREGATE_API_AUTH_USERPASS {
                let parsed: UserPassAuthParams = serde_json::from_value(value.clone())
                    .map_err(|_| "authParams is invalid".to_string())?;
                let mode = parsed.mode.trim().to_ascii_lowercase();
                match mode.as_str() {
                    "basic" => {}
                    "headerpair" | "querypair" => {
                        if parsed
                            .username_name
                            .as_deref()
                            .map(str::trim)
                            .unwrap_or("")
                            .is_empty()
                        {
                            return Err("authParams.usernameName is required".to_string());
                        }
                        if parsed
                            .password_name
                            .as_deref()
                            .map(str::trim)
                            .unwrap_or("")
                            .is_empty()
                        {
                            return Err("authParams.passwordName is required".to_string());
                        }
                    }
                    _ => {
                        return Err(
                            "authParams.mode must be basic, headerPair, or queryPair".to_string()
                        );
                    }
                }
            }
            serde_json::to_string(&value)
                .map(Some)
                .map_err(|_| "authParams must be a valid JSON object".to_string())
        }
    }
}

fn normalize_action_override(
    enabled: Option<bool>,
    action: Option<String>,
) -> Result<Option<Option<String>>, String> {
    match enabled {
        None => Ok(None),
        Some(false) => Ok(Some(None)),
        Some(true) => {
            normalize_action(action).map(|value| Some(Some(value.unwrap_or_else(String::new))))
        }
    }
}

#[cfg(test)]
#[path = "aggregate_api_tests.rs"]
mod tests;
fn serialize_userpass_secret(username: &str, password: &str) -> Result<String, String> {
    let secret = UserPassSecret {
        username: username.trim().to_string(),
        password: password.trim().to_string(),
    };
    serde_json::to_string(&secret).map_err(|_| "invalid username/password".to_string())
}

fn action_path_or_default(api: &AggregateApi, default: &str) -> String {
    match api.action.as_deref().map(str::trim) {
        Some("") => String::new(),
        Some(value) => {
            if value.starts_with('/') {
                value.to_string()
            } else {
                format!("/{value}")
            }
        }
        None => default.to_string(),
    }
}

fn with_query_param(url: &str, name: &str, value: &str) -> String {
    let mut parsed = match reqwest::Url::parse(url) {
        Ok(value) => value,
        Err(_) => return url.to_string(),
    };
    let existing = parsed.query_pairs().into_owned().collect::<Vec<_>>();
    parsed.set_query(None);
    {
        let mut query = parsed.query_pairs_mut();
        for (key, val) in existing {
            if key == name {
                continue;
            }
            query.append_pair(key.as_str(), val.as_str());
        }
        query.append_pair(name, value);
    }
    parsed.to_string()
}

/// 函数 `normalize_provider_type`
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
fn normalize_provider_type(value: Option<String>) -> Result<String, String> {
    match value {
        Some(raw) => {
            let normalized = raw.trim().to_ascii_lowercase().replace('-', "_");
            match normalized.as_str() {
                "codex" | "openai" | "openai_compat" | "gpt" => {
                    Ok(AGGREGATE_API_PROVIDER_CODEX.to_string())
                }
                "gemini" | "gemini_native" | "google" | "google_ai" | "google_gemini" => {
                    Ok(AGGREGATE_API_PROVIDER_GEMINI.to_string())
                }
                "claude" | "anthropic" | "anthropic_native" | "claude_code" => {
                    Ok(AGGREGATE_API_PROVIDER_CLAUDE.to_string())
                }
                "compatible" => Ok(AGGREGATE_API_PROVIDER_COMPATIBLE.to_string()),
                other => Err(format!("unsupported aggregate api provider type: {other}")),
            }
        }
        None => Ok(AGGREGATE_API_PROVIDER_CODEX.to_string()),
    }
}

/// 函数 `normalize_provider_type_value`
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
fn normalize_provider_type_value(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "claude" | "anthropic" | "anthropic_native" | "claude_code" => {
            AGGREGATE_API_PROVIDER_CLAUDE.to_string()
        }
        "gemini" | "gemini_native" | "google" | "google_ai" | "google_gemini" => {
            AGGREGATE_API_PROVIDER_GEMINI.to_string()
        }
        "compatible" => AGGREGATE_API_PROVIDER_COMPATIBLE.to_string(),
        _ => AGGREGATE_API_PROVIDER_CODEX.to_string(),
    }
}

/// 函数 `provider_default_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - provider_type: 参数 provider_type
///
/// # 返回
/// 返回函数执行结果
fn provider_default_url(provider_type: &str) -> &'static str {
    match provider_type {
        AGGREGATE_API_PROVIDER_CLAUDE => "https://api.anthropic.com/v1",
        AGGREGATE_API_PROVIDER_GEMINI => "https://generativelanguage.googleapis.com",
        _ => "https://api.openai.com/v1",
    }
}

/// 函数 `normalize_probe_url`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - base_url: 参数 base_url
/// - suffix: 参数 suffix
///
/// # 返回
/// 返回函数执行结果
fn normalize_probe_url(base_url: &str, suffix: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if suffix.trim().is_empty() {
        return base.to_string();
    }
    if base.ends_with("/v1") {
        format!("{base}{suffix}")
    } else {
        format!("{base}/v1{suffix}")
    }
}

fn join_api_path(base_url: &str, path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let suffix = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    format!("{base}{suffix}")
}

fn balance_query_base_url(api: &AggregateApi, template: &str) -> String {
    let mut base = api
        .balance_query_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(api.url.as_str())
        .trim()
        .trim_end_matches('/')
        .to_string();
    if template == AGGREGATE_API_BALANCE_TEMPLATE_NEW_API && api.balance_query_base_url.is_none() {
        if let Some(stripped) = base.strip_suffix("/v1") {
            base = stripped.to_string();
        }
    }
    base
}

fn balance_query_usage_base_url(api: &AggregateApi) -> String {
    let base = balance_query_base_url(api, AGGREGATE_API_BALANCE_TEMPLATE_GENERIC);
    if api.balance_query_base_url.is_none() {
        if let Some(stripped) = base.strip_suffix("/v1") {
            return stripped.to_string();
        }
    }
    base
}

fn short_error_body(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= 240 {
        return compact;
    }
    compact.chars().take(240).collect::<String>()
}

fn json_path<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn json_path_dot<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for segment in path.split('.') {
        if let Ok(index) = segment.parse::<usize>() {
            current = current.as_array()?.get(index)?;
        } else {
            current = current.get(segment)?;
        }
    }
    Some(current)
}

fn json_number(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(value) => value.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn repair_mojibake_utf8(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    for ch in value.chars() {
        let code = ch as u32;
        if code > u8::MAX as u32 {
            return value.to_string();
        }
        bytes.push(code as u8);
    }
    match String::from_utf8(bytes) {
        Ok(repaired)
            if repaired
                .chars()
                .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)) =>
        {
            repaired
        }
        _ => value.to_string(),
    }
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(repair_mojibake_utf8(trimmed))
            }
        }
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn json_bool(value: Option<&serde_json::Value>) -> Option<bool> {
    match value? {
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::Number(number) => Some(number.as_i64().unwrap_or(0) != 0),
        serde_json::Value::String(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "true" | "1" | "yes" | "on" | "active" => Some(true),
                "false" | "0" | "no" | "off" | "disabled" | "inactive" => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

fn first_number(value: &serde_json::Value, paths: &[&[&str]]) -> Option<f64> {
    paths
        .iter()
        .find_map(|path| json_number(json_path(value, path)))
}

fn first_string(value: &serde_json::Value, paths: &[&[&str]]) -> Option<String> {
    paths
        .iter()
        .find_map(|path| json_string(json_path(value, path)))
}

fn custom_number(value: &serde_json::Value, path: Option<&str>, multiplier: f64) -> Option<f64> {
    path.and_then(|path| json_number(json_path_dot(value, path)))
        .map(|value| value * multiplier)
}

fn custom_string(value: &serde_json::Value, path: Option<&str>) -> Option<String> {
    path.and_then(|path| json_string(json_path_dot(value, path)))
}

fn custom_bool(value: &serde_json::Value, path: Option<&str>) -> Option<bool> {
    path.and_then(|path| json_bool(json_path_dot(value, path)))
}

fn extract_generic_balance(
    value: &serde_json::Value,
) -> Result<AggregateApiBalanceSnapshot, String> {
    let success = json_bool(json_path(value, &["success"])).unwrap_or(true);
    let is_active = json_bool(json_path(value, &["is_active"]))
        .or_else(|| json_bool(json_path(value, &["active"])))
        .or_else(|| json_bool(json_path(value, &["data", "is_active"])))
        .or_else(|| json_bool(json_path(value, &["data", "active"])))
        .or_else(|| json_bool(json_path(value, &["isValid"])))
        .or_else(|| json_bool(json_path(value, &["is_valid"])))
        .or_else(|| json_bool(json_path(value, &["data", "isValid"])))
        .or_else(|| json_bool(json_path(value, &["data", "is_valid"])))
        .unwrap_or(true);
    let status = first_string(value, &[&["status"], &["data", "status"]]);
    let status_valid = status
        .as_deref()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !matches!(
                normalized.as_str(),
                "expired" | "quota_exhausted" | "disabled"
            )
        })
        .unwrap_or(true);
    let invalid_message = first_string(
        value,
        &[
            &["message"],
            &["error"],
            &["status"],
            &["data", "message"],
            &["data", "error"],
        ],
    );
    let is_valid = success && is_active && status_valid;
    let remaining = first_number(
        value,
        &[
            &["remaining"],
            &["balance"],
            &["available"],
            &["quota", "remaining"],
            &["data", "remaining"],
            &["data", "balance"],
            &["data", "available"],
            &["data", "quota", "remaining"],
            &["credits", "balance"],
        ],
    );
    if is_valid && remaining.is_none() {
        return Err("balance response missing remaining field".to_string());
    }
    Ok(AggregateApiBalanceSnapshot {
        is_valid,
        invalid_message: if is_valid { None } else { invalid_message },
        remaining,
        unit: first_string(
            value,
            &[
                &["unit"],
                &["currency"],
                &["data", "unit"],
                &["data", "currency"],
            ],
        )
        .or_else(|| Some("USD".to_string())),
        plan_name: first_string(
            value,
            &[
                &["planName"],
                &["plan_name"],
                &["mode"],
                &["data", "planName"],
                &["data", "plan_name"],
                &["data", "group"],
                &["data", "mode"],
            ],
        ),
        total: first_number(
            value,
            &[
                &["total"],
                &["quota", "limit"],
                &["data", "total"],
                &["data", "quota", "limit"],
            ],
        ),
        used: first_number(
            value,
            &[
                &["used"],
                &["used_quota"],
                &["quota", "used"],
                &["data", "used"],
                &["data", "used_quota"],
                &["data", "quota", "used"],
            ],
        ),
        extra: None,
    })
}

fn extract_new_api_balance(
    value: &serde_json::Value,
) -> Result<AggregateApiBalanceSnapshot, String> {
    let success = json_bool(json_path(value, &["success"])).unwrap_or(true);
    let data = json_path(value, &["data"]).unwrap_or(value);
    let quota = json_number(data.get("quota"));
    let used_quota = json_number(data.get("used_quota")).unwrap_or(0.0);
    if success && quota.is_none() {
        return Err("new api balance response missing data.quota".to_string());
    }
    let remaining = quota.map(|value| value / 500_000.0);
    let used = used_quota / 500_000.0;
    let total = remaining.map(|value| value + used);
    Ok(AggregateApiBalanceSnapshot {
        is_valid: success,
        invalid_message: if success {
            None
        } else {
            first_string(value, &[&["message"], &["error"]])
        },
        remaining,
        unit: Some("USD".to_string()),
        plan_name: json_string(data.get("group")).or_else(|| json_string(data.get("plan"))),
        total,
        used: Some(used),
        extra: None,
    })
}

fn extract_custom_balance(
    value: &serde_json::Value,
    config: &CustomBalanceQueryConfig,
) -> Result<AggregateApiBalanceSnapshot, String> {
    let success = json_bool(json_path(value, &["success"])).unwrap_or(true);
    let explicit_valid = custom_bool(value, config.valid_path.as_deref()).unwrap_or(true);
    let is_valid = success && explicit_valid;
    let multiplier = config.multiplier.unwrap_or(1.0);
    let remaining = custom_number(value, Some(config.remaining_path.as_str()), multiplier);
    if is_valid && remaining.is_none() {
        return Err("custom balance response missing remaining field".to_string());
    }
    Ok(AggregateApiBalanceSnapshot {
        is_valid,
        invalid_message: if is_valid {
            None
        } else {
            custom_string(value, config.invalid_message_path.as_deref()).or_else(|| {
                first_string(
                    value,
                    &[
                        &["message"],
                        &["error"],
                        &["data", "message"],
                        &["data", "error"],
                    ],
                )
            })
        },
        remaining,
        unit: config.unit.clone().or_else(|| Some("USD".to_string())),
        plan_name: custom_string(value, config.plan_path.as_deref()),
        total: custom_number(value, config.total_path.as_deref(), multiplier),
        used: custom_number(value, config.used_path.as_deref(), multiplier),
        extra: None,
    })
}

fn should_try_usage_balance_fallback(error: &str) -> bool {
    error.contains("http_status=404")
        || error.contains("http_status=405")
        || error.contains("http_status=501")
        || error.contains("balance response is not valid JSON")
        || error.contains("balance response missing remaining field")
}

fn parse_custom_balance_query_config(
    value: Option<&str>,
) -> Result<CustomBalanceQueryConfig, String> {
    let normalized = normalize_custom_balance_query_config(value.map(str::to_string))?
        .ok_or_else(|| "custom balance query config is required".to_string())?;
    serde_json::from_str(normalized.as_str())
        .map_err(|_| "custom balance query config is invalid JSON".to_string())
}

/// 函数 `build_claude_probe_body`
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
fn build_claude_probe_body(model: &str) -> serde_json::Value {
    json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{
            "role": "user",
            "content": "Who are you?"
        }],
        "stream": true
    })
}

/// 函数 `build_codex_probe_body`
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
fn build_codex_probe_body(model: &str) -> serde_json::Value {
    json!({
        "model": model,
        "input": [{
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": "Who are you?"
            }]
        }],
        "stream": true,
        "store": false
    })
}

fn is_minimax_aggregate_api(api: &AggregateApi) -> bool {
    if api
        .supplier_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value.to_ascii_lowercase().contains("minimax"))
    {
        return true;
    }
    reqwest::Url::parse(api.url.as_str())
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| host == "minimax.io" || host.ends_with(".minimax.io"))
}

fn build_gemini_probe_body() -> serde_json::Value {
    json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "text": "Who are you?"
            }]
        }],
        "generationConfig": {
            "maxOutputTokens": 1
        }
    })
}

/// 函数 `list_aggregate_apis`
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
pub(crate) fn list_aggregate_apis() -> Result<Vec<AggregateApiSummary>, String> {
    let storage = open_storage().ok_or_else(|| "open storage failed".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let items = storage
        .list_aggregate_api_summaries()
        .map_err(|err| format!("load aggregate api list failed: {err}"))?;
    let mut models_by_api = std::collections::HashMap::<String, Vec<String>>::new();
    for model in storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("load model catalog V2 routes failed: {err}"))?
    {
        for route in model
            .routes
            .into_iter()
            .filter(|route| route.enabled && route.source_kind == "aggregate_api")
        {
            models_by_api
                .entry(route.source_id)
                .or_default()
                .push(model.slug.clone());
        }
    }
    Ok(items
        .into_iter()
        .map(|item| AggregateApiSummary {
            model_slugs: models_by_api.remove(item.id.as_str()).unwrap_or_default(),
            id: item.id,
            provider_type: item.provider_type,
            supplier_name: item.supplier_name,
            sort: item.sort,
            url: item.url,
            auth_type: item.auth_type,
            auth_params: item
                .auth_params_json
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok()),
            action: item.action,
            model_override: item.model_override,
            user_agent: item.user_agent,
            status: item.status,
            created_at: item.created_at,
            updated_at: item.updated_at,
            last_test_at: item.last_test_at,
            last_test_status: item.last_test_status,
            last_test_error: item.last_test_error,
            balance_query_enabled: item.balance_query_enabled,
            balance_query_template: item.balance_query_template,
            balance_query_base_url: item.balance_query_base_url,
            balance_query_user_id: item.balance_query_user_id,
            balance_query_config_json: item.balance_query_config_json,
            last_balance_at: item.last_balance_at,
            last_balance_status: item.last_balance_status,
            last_balance_error: item.last_balance_error,
            last_balance_json: item.last_balance_json,
        })
        .collect())
}

/// 函数 `create_aggregate_api`
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
pub(crate) fn create_aggregate_api(
    url: Option<String>,
    key: Option<String>,
    provider_type: Option<String>,
    supplier_name: Option<String>,
    sort: Option<i64>,
    auth_type: Option<String>,
    auth_custom_enabled: Option<bool>,
    auth_params: Option<serde_json::Value>,
    action_custom_enabled: Option<bool>,
    action: Option<String>,
    model_override: Option<String>,
    user_agent: Option<String>,
    username: Option<String>,
    password: Option<String>,
    balance_query_enabled: Option<bool>,
    balance_query_template: Option<String>,
    balance_query_base_url: Option<String>,
    balance_query_access_token: Option<String>,
    balance_query_user_id: Option<String>,
    balance_query_config_json: Option<String>,
) -> Result<AggregateApiCreateResult, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let normalized_provider_type = normalize_provider_type(provider_type)?;
    let normalized_supplier_name = normalize_supplier_name(supplier_name)?;
    let normalized_sort = normalize_sort(sort);
    let normalized_url = normalize_upstream_base_url(url)?
        .unwrap_or_else(|| provider_default_url(normalized_provider_type.as_str()).to_string());
    let normalized_auth_type = normalize_auth_type(auth_type)?;
    let normalized_auth_params_json = normalize_auth_params_json(
        normalized_auth_type.as_str(),
        auth_custom_enabled,
        auth_params,
    )?;
    let normalized_action =
        normalize_action_override(action_custom_enabled, action)?.unwrap_or(None);
    let normalized_model_override = normalize_model_override(model_override)?;
    let normalized_user_agent = normalize_aggregate_api_user_agent(user_agent)?;
    let normalized_balance_query_enabled = balance_query_enabled.unwrap_or(false);
    let normalized_balance_query_template = if normalized_balance_query_enabled {
        Some(default_balance_query_template(
            normalize_balance_query_template(balance_query_template)?,
        ))
    } else {
        normalize_balance_query_template(balance_query_template)?
    };
    let normalized_balance_query_base_url =
        normalize_optional_url(balance_query_base_url, "balanceQueryBaseUrl")?;
    let normalized_balance_query_access_token = normalize_secret(balance_query_access_token);
    let normalized_balance_query_user_id = normalize_optional_text(balance_query_user_id);
    let normalized_balance_query_config_json = normalize_balance_query_config_json(
        normalized_balance_query_template.as_deref(),
        balance_query_config_json,
    )?;
    let normalized_secret = if normalized_auth_type == AGGREGATE_API_AUTH_APIKEY {
        normalize_secret(key).ok_or_else(|| "key is required".to_string())?
    } else {
        let username = username
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| "username is required".to_string())?;
        let password = password
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| "password is required".to_string())?;
        serialize_userpass_secret(username, password)?
    };
    let id = generate_aggregate_api_id();
    let created_at = now_ts();
    let record = AggregateApi {
        id: id.clone(),
        provider_type: normalized_provider_type,
        supplier_name: Some(normalized_supplier_name),
        sort: normalized_sort,
        url: normalized_url,
        auth_type: normalized_auth_type,
        auth_params_json: normalized_auth_params_json
            .map(|value| if value.is_empty() { None } else { Some(value) })
            .unwrap_or(None),
        action: normalized_action,
        model_override: normalized_model_override,
        user_agent: normalized_user_agent,
        status: "active".to_string(),
        created_at,
        updated_at: created_at,
        last_test_at: None,
        last_test_status: None,
        last_test_error: None,
        balance_query_enabled: normalized_balance_query_enabled,
        balance_query_template: normalized_balance_query_template,
        balance_query_base_url: normalized_balance_query_base_url,
        balance_query_user_id: normalized_balance_query_user_id,
        balance_query_config_json: normalized_balance_query_config_json,
        last_balance_at: None,
        last_balance_status: None,
        last_balance_error: None,
        last_balance_json: None,
    };
    storage
        .insert_aggregate_api(&record)
        .map_err(|err| err.to_string())?;
    if let Err(err) = storage.upsert_aggregate_api_secret(&id, &normalized_secret) {
        let _ = storage.delete_aggregate_api(&id);
        return Err(format!("persist aggregate api secret failed: {err}"));
    }
    if let Some(access_token) = normalized_balance_query_access_token {
        if let Err(err) = storage.upsert_aggregate_api_balance_secret(&id, &access_token) {
            let _ = storage.delete_aggregate_api(&id);
            return Err(format!(
                "persist aggregate api balance secret failed: {err}"
            ));
        }
    }
    Ok(AggregateApiCreateResult {
        id,
        key: if record.auth_type == AGGREGATE_API_AUTH_APIKEY {
            normalized_secret
        } else {
            String::new()
        },
    })
}

/// 函数 `update_aggregate_api`
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
pub(crate) fn update_aggregate_api(
    api_id: &str,
    url: Option<String>,
    key: Option<String>,
    provider_type: Option<String>,
    supplier_name: Option<String>,
    sort: Option<i64>,
    status: Option<String>,
    auth_type: Option<String>,
    auth_custom_enabled: Option<bool>,
    auth_params: Option<serde_json::Value>,
    action_custom_enabled: Option<bool>,
    action: Option<String>,
    model_override: Option<String>,
    user_agent: Option<String>,
    username: Option<String>,
    password: Option<String>,
    balance_query_enabled: Option<bool>,
    balance_query_template: Option<String>,
    balance_query_base_url: Option<String>,
    balance_query_access_token: Option<String>,
    balance_query_user_id: Option<String>,
    balance_query_config_json: Option<String>,
) -> Result<(), String> {
    if api_id.is_empty() {
        return Err("aggregate api id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let existing = storage
        .find_aggregate_api_update_config_by_id(api_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api not found".to_string())?;
    let user_agent_provided = user_agent.is_some();
    let normalized_user_agent = normalize_aggregate_api_user_agent(user_agent)?;
    let existing_auth_type = normalize_auth_type(Some(existing.auth_type.clone()))
        .unwrap_or_else(|_| AGGREGATE_API_AUTH_APIKEY.to_string());
    let normalized_auth_type = match auth_type {
        Some(raw) => Some(normalize_auth_type(Some(raw))?),
        None => None,
    };
    let next_auth_type = normalized_auth_type
        .as_deref()
        .unwrap_or(existing_auth_type.as_str())
        .to_string();
    let auth_type_changed = next_auth_type != existing_auth_type;

    if let Some(next) = normalized_auth_type.as_deref() {
        storage
            .update_aggregate_api_auth_type(api_id, next)
            .map_err(|err| err.to_string())?;
    }
    if let Some(provider_type) = provider_type {
        let normalized_provider_type = normalize_provider_type(Some(provider_type))?;
        storage
            .update_aggregate_api_type(api_id, normalized_provider_type.as_str())
            .map_err(|err| err.to_string())?;
    }
    let normalized_supplier_name = normalize_supplier_name(supplier_name)?;
    storage
        .update_aggregate_api_supplier_name(api_id, Some(normalized_supplier_name.as_str()))
        .map_err(|err| err.to_string())?;
    if sort.is_some() {
        storage
            .update_aggregate_api_sort(api_id, normalize_sort(sort))
            .map_err(|err| err.to_string())?;
    }
    if let Some(status) = status {
        let normalized_status = normalize_status(Some(status))?;
        storage
            .update_aggregate_api_status(api_id, normalized_status.as_str())
            .map_err(|err| err.to_string())?;
    }
    if let Some(url) = url {
        let normalized_url =
            normalize_upstream_base_url(Some(url))?.ok_or_else(|| "url is required".to_string())?;
        storage
            .update_aggregate_api(api_id, normalized_url.as_str())
            .map_err(|err| err.to_string())?;
    }

    if let Some(auth_params_json) =
        normalize_auth_params_json(next_auth_type.as_str(), auth_custom_enabled, auth_params)?
    {
        let normalized = auth_params_json.trim().to_string();
        if normalized.is_empty() {
            storage
                .update_aggregate_api_auth_params_json(api_id, None)
                .map_err(|err| err.to_string())?;
        } else {
            storage
                .update_aggregate_api_auth_params_json(api_id, Some(normalized.as_str()))
                .map_err(|err| err.to_string())?;
        }
    }

    if let Some(action_override) = normalize_action_override(action_custom_enabled, action)? {
        if let Some(action) = action_override {
            let normalized = action.trim().to_string();
            storage
                .update_aggregate_api_action(api_id, Some(normalized.as_str()))
                .map_err(|err| err.to_string())?;
        } else {
            storage
                .update_aggregate_api_action(api_id, None)
                .map_err(|err| err.to_string())?;
        }
    }
    if model_override.is_some() {
        let normalized = normalize_model_override(model_override)?;
        storage
            .update_aggregate_api_model_override(api_id, normalized.as_deref())
            .map_err(|err| err.to_string())?;
    }
    if user_agent_provided {
        storage
            .update_aggregate_api_user_agent(api_id, normalized_user_agent.as_deref())
            .map_err(|err| err.to_string())?;
    }

    let balance_query_base_url_provided = balance_query_base_url.is_some();
    let balance_query_user_id_provided = balance_query_user_id.is_some();
    let balance_query_config_json_provided = balance_query_config_json.is_some();
    let normalized_balance_query_template =
        normalize_balance_query_template(balance_query_template)?;
    let normalized_balance_query_base_url =
        normalize_optional_url(balance_query_base_url, "balanceQueryBaseUrl")?;
    let normalized_balance_query_access_token = normalize_secret(balance_query_access_token);
    let normalized_balance_query_user_id = normalize_optional_text(balance_query_user_id);
    let normalized_balance_query_config_json = if balance_query_config_json_provided {
        normalize_balance_query_config_json(
            normalized_balance_query_template
                .as_deref()
                .or(existing.balance_query_template.as_deref()),
            balance_query_config_json,
        )?
    } else {
        None
    };
    if balance_query_enabled.is_some()
        || normalized_balance_query_template.is_some()
        || balance_query_base_url_provided
        || balance_query_user_id_provided
        || balance_query_config_json_provided
    {
        let next_enabled = balance_query_enabled.unwrap_or(existing.balance_query_enabled);
        let next_template = if next_enabled {
            Some(default_balance_query_template(
                normalized_balance_query_template.or(existing.balance_query_template.clone()),
            ))
        } else {
            normalized_balance_query_template.or(existing.balance_query_template.clone())
        };
        let next_base_url = if balance_query_base_url_provided {
            normalized_balance_query_base_url
        } else {
            existing.balance_query_base_url
        };
        let next_user_id = if balance_query_user_id_provided {
            normalized_balance_query_user_id
        } else {
            existing.balance_query_user_id
        };
        let next_config_json = if balance_query_config_json_provided {
            normalized_balance_query_config_json
        } else if next_template.as_deref() == Some(AGGREGATE_API_BALANCE_TEMPLATE_CUSTOM) {
            normalize_balance_query_config_json(
                next_template.as_deref(),
                existing.balance_query_config_json,
            )?
        } else {
            None
        };
        storage
            .update_aggregate_api_balance_query(
                api_id,
                next_enabled,
                next_template.as_deref(),
                next_base_url.as_deref(),
                next_user_id.as_deref(),
                next_config_json.as_deref(),
            )
            .map_err(|err| err.to_string())?;
    }
    if let Some(access_token) = normalized_balance_query_access_token {
        storage
            .upsert_aggregate_api_balance_secret(api_id, &access_token)
            .map_err(|err| err.to_string())?;
    }
    if let Some(false) = balance_query_enabled {
        storage
            .delete_aggregate_api_balance_secret(api_id)
            .map_err(|err| err.to_string())?;
    }

    if next_auth_type == AGGREGATE_API_AUTH_APIKEY {
        let normalized_secret = normalize_secret(key);
        if auth_type_changed && normalized_secret.is_none() {
            return Err("key is required when switching authType to apikey".to_string());
        }
        if let Some(secret) = normalized_secret {
            storage
                .upsert_aggregate_api_secret(api_id, &secret)
                .map_err(|err| err.to_string())?;
        }
    } else {
        let username = username.as_deref().map(str::trim).unwrap_or("");
        let password = password.as_deref().map(str::trim).unwrap_or("");
        let has_user = !username.is_empty();
        let has_pass = !password.is_empty();
        if (has_user && !has_pass) || (!has_user && has_pass) {
            return Err("username and password must be provided together".to_string());
        }
        if auth_type_changed && (!has_user || !has_pass) {
            return Err(
                "username and password are required when switching authType to userpass"
                    .to_string(),
            );
        }
        if has_user && has_pass {
            let secret = serialize_userpass_secret(username, password)?;
            storage
                .upsert_aggregate_api_secret(api_id, &secret)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

/// 函数 `delete_aggregate_api`
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
pub(crate) fn delete_aggregate_api(api_id: &str) -> Result<(), String> {
    if api_id.is_empty() {
        return Err("aggregate api id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    storage
        .delete_aggregate_api(api_id)
        .map_err(|err| err.to_string())
}

/// 函数 `read_aggregate_api_secret`
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
pub(crate) fn read_aggregate_api_secret(api_id: &str) -> Result<AggregateApiSecretResult, String> {
    if api_id.is_empty() {
        return Err("aggregate api id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let config = storage
        .find_aggregate_api_secret_config_by_id(api_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api not found".to_string())?;
    let key = config
        .secret_value
        .ok_or_else(|| "aggregate api secret not found".to_string())?;
    let auth_type = normalize_auth_type(Some(config.auth_type))?;
    if auth_type == AGGREGATE_API_AUTH_USERPASS {
        let parsed: UserPassSecret = serde_json::from_str(key.as_str())
            .map_err(|_| "invalid aggregate api secret".to_string())?;
        return Ok(AggregateApiSecretResult {
            id: api_id.to_string(),
            key: String::new(),
            auth_type,
            username: Some(parsed.username),
            password: Some(parsed.password),
        });
    }
    Ok(AggregateApiSecretResult {
        id: api_id.to_string(),
        key,
        auth_type,
        username: None,
        password: None,
    })
}

fn model_id_from_value(value: &Value) -> Option<String> {
    ["id", "model", "slug", "name"]
        .iter()
        .find_map(|key| value.get(*key).and_then(|item| item.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn model_display_name_from_value(value: &Value) -> Option<String> {
    ["displayName", "display_name", "title", "name"]
        .iter()
        .find_map(|key| value.get(*key).and_then(|item| item.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn normalize_fetched_model_id(raw: &str, gemini: bool) -> Option<String> {
    let mut value = raw.trim();
    if gemini {
        value = value.strip_prefix("models/").unwrap_or(value);
    }
    if value.is_empty()
        || value.len() > 200
        || value
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    Some(value.to_string())
}

fn parse_fetched_models(body: &Value, gemini: bool) -> Vec<(String, Option<String>)> {
    let candidates = body
        .as_array()
        .cloned()
        .or_else(|| body.get("data").and_then(Value::as_array).cloned())
        .or_else(|| body.get("models").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for value in candidates {
        let Some(raw_id) = model_id_from_value(&value) else {
            continue;
        };
        let Some(upstream_model) = normalize_fetched_model_id(raw_id.as_str(), gemini) else {
            continue;
        };
        let key = upstream_model.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let display_name = model_display_name_from_value(&value).and_then(|display_name| {
            if gemini {
                normalize_fetched_model_id(display_name.as_str(), true)
            } else {
                Some(display_name)
            }
        });
        items.push((upstream_model, display_name));
        if items.len() >= MAX_FETCHED_MODELS {
            break;
        }
    }
    items
}

fn models_endpoint(api: &AggregateApi, provider_type: &str) -> String {
    let suffix = "/models";
    if provider_type == AGGREGATE_API_PROVIDER_GEMINI {
        let base = api.url.trim().trim_end_matches('/');
        if base.ends_with("/v1beta/models") {
            base.to_string()
        } else if base.ends_with("/v1beta") {
            format!("{base}{suffix}")
        } else if base.ends_with("/v1") {
            format!("{}{}", base.trim_end_matches("/v1"), "/v1beta/models")
        } else {
            format!("{base}/v1beta/models")
        }
    } else {
        normalize_probe_url(api.url.as_str(), suffix)
    }
}

pub(crate) fn fetch_aggregate_api_models(
    api_id: &str,
) -> Result<AggregateApiFetchModelsResult, String> {
    operations::run_aggregate_future(fetch_aggregate_api_models_async(api_id))
}

pub(crate) fn associate_aggregate_api_models(
    api_id: &str,
    upstream_models: Vec<String>,
    display_names: BTreeMap<String, String>,
) -> Result<AggregateApiAssociateModelsResult, String> {
    if api_id.trim().is_empty() {
        return Err("aggregate api id required".to_string());
    }
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let api = storage
        .find_aggregate_api_by_id(api_id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "aggregate api not found".to_string())?;
    let provider_type = normalize_provider_type_value(api.provider_type.as_str());
    let mut requested = Vec::new();
    let mut seen = HashSet::new();
    for raw in upstream_models {
        let Some(model) = normalize_fetched_model_id(
            raw.as_str(),
            provider_type == AGGREGATE_API_PROVIDER_GEMINI,
        ) else {
            return Err("invalid upstream model id".to_string());
        };
        if seen.insert(model.to_ascii_lowercase()) {
            requested.push(model);
        }
    }
    if requested.is_empty() {
        return Ok(AggregateApiAssociateModelsResult::default());
    }
    let mut models = storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("read model catalog V2 failed: {err}"))?;
    let next_sort = models
        .iter()
        .map(|model| model.sort_order)
        .max()
        .unwrap_or(0)
        + 1;
    let provider = api
        .supplier_name
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| Some(provider_type.clone()));
    let mut inputs = Vec::new();
    let mut created_models = Vec::new();
    let mut added_routes = Vec::new();
    let mut unchanged_routes = Vec::new();
    for (index, upstream_model) in requested.iter().enumerate() {
        if let Some(model) = models
            .iter_mut()
            .find(|model| model.slug.eq_ignore_ascii_case(upstream_model.as_str()))
        {
            let exact = model.routes.iter().any(|route| {
                route.source_kind == "aggregate_api"
                    && route.source_id == api_id
                    && route
                        .upstream_model
                        .eq_ignore_ascii_case(upstream_model.as_str())
            });
            if exact {
                unchanged_routes.push(model.slug.clone());
                continue;
            }
            let inherited = model
                .routes
                .iter()
                .find(|route| route.source_kind == "aggregate_api" && route.source_id == api_id)
                .map(|route| (route.priority, route.weight))
                .unwrap_or((0, 1));
            model.routes.push(ModelRouteV2 {
                source_kind: "aggregate_api".to_string(),
                source_id: api_id.to_string(),
                upstream_model: upstream_model.clone(),
                enabled: true,
                priority: inherited.0,
                weight: inherited.1.max(1),
                ..Default::default()
            });
            inputs.push(ManagedModelV2Upsert {
                model: model.clone(),
                ..Default::default()
            });
            added_routes.push(model.slug.clone());
        } else {
            let display_name = display_names
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(upstream_model.as_str()))
                .map(|(_, value)| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| upstream_model.clone());
            let model = ManagedModelV2 {
                slug: upstream_model.clone(),
                display_name,
                provider: provider.clone(),
                origin: "custom".to_string(),
                enabled: true,
                supported_in_api: true,
                visibility: "list".to_string(),
                sort_order: next_sort + index as i64,
                capabilities: json!({
                    "supports_text_generation": true,
                    "input_modalities": ["text"],
                    "output_modalities": ["text"]
                }),
                instructions_mode: "passthrough".to_string(),
                fast_policy: ModelFastPolicyV2::Passthrough,
                price: ModelPriceV2 {
                    price_status: "missing".to_string(),
                    ..Default::default()
                },
                routes: vec![ModelRouteV2 {
                    source_kind: "aggregate_api".to_string(),
                    source_id: api_id.to_string(),
                    upstream_model: upstream_model.clone(),
                    enabled: true,
                    priority: 0,
                    weight: 1,
                    ..Default::default()
                }],
                ..Default::default()
            };
            inputs.push(ManagedModelV2Upsert {
                model,
                ..Default::default()
            });
            created_models.push(upstream_model.clone());
            added_routes.push(upstream_model.clone());
        }
    }
    if !inputs.is_empty() {
        storage
            .upsert_managed_models_v2(&inputs)
            .map_err(|err| format!("associate models transaction failed: {err}"))?;
    }
    Ok(AggregateApiAssociateModelsResult {
        created_models,
        added_routes,
        unchanged_routes,
    })
}

/// 函数 `test_aggregate_api_connection`
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
fn configured_aggregate_probe_model(
    storage: &codexmanager_core::storage::Storage,
    api_id: &str,
) -> Result<String, String> {
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let mut routes = storage
        .list_managed_models_v2(true)
        .map_err(|err| format!("read model catalog V2 routes failed: {err}"))?
        .into_iter()
        .flat_map(|model| {
            model
                .routes
                .into_iter()
                .filter(move |route| {
                    route.enabled
                        && route.source_kind == "aggregate_api"
                        && route.source_id == api_id
                        && !route.upstream_model.trim().is_empty()
                })
                .map(move |route| (route.priority, model.sort_order, route.upstream_model))
        })
        .collect::<Vec<_>>();
    routes.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    routes
        .into_iter()
        .next()
        .map(|(_, _, model)| model)
        .ok_or_else(|| "aggregate api has no enabled model catalog V2 route".to_string())
}

pub(crate) fn test_aggregate_api_connection(
    api_id: &str,
) -> Result<AggregateApiTestResult, String> {
    operations::run_aggregate_future(test_aggregate_api_connection_async(api_id))
}

pub(crate) fn refresh_aggregate_api_balance(
    api_id: &str,
) -> Result<AggregateApiBalanceRefreshResult, String> {
    operations::run_aggregate_future(refresh_aggregate_api_balance_async(api_id))
}
