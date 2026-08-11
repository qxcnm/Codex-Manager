use codexmanager_core::auth::parse_id_token_claims;
use codexmanager_core::rpc::types::ModelsResponse;
use codexmanager_core::storage::{Account, Storage, Token, UsageSnapshotRecord};
use reqwest::Method;
use serde_json::Value;
use std::collections::HashMap;

mod parse;
mod request;

pub(crate) use parse::parse_models_response;
use request::{send_models_probe_request, send_models_request};

/// 函数 `should_retry_models_with_openai_fallback`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - err: 参数 err
///
/// # 返回
/// 返回函数执行结果
fn should_retry_models_with_openai_fallback(err: &str) -> bool {
    let normalized = err.to_ascii_lowercase();
    normalized.contains("cloudflare")
        || normalized.contains("text/html")
        || normalized.contains("<html")
        || normalized.contains("<!doctype html")
        || normalized.contains("body=<html")
        || normalized.contains("challenge")
}

/// 函数 `fetch_models_for_picker`
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
pub(crate) fn fetch_models_for_picker() -> Result<ModelsResponse, String> {
    if crate::gateway::current_codex_user_agent_version()
        == crate::gateway::default_codex_user_agent_version()
    {
        if let Err(err) = crate::app_settings::sync_gateway_user_agent_version_from_codex_latest() {
            log::warn!("codex latest client_version sync before models failed: {err}");
        }
    }
    let storage = super::open_storage()
        .ok_or_else(|| crate::gateway::bilingual_error("存储不可用", "storage unavailable"))?;
    let mut candidates = super::collect_gateway_candidates(&storage)?;
    if candidates.is_empty() {
        return Err(crate::gateway::bilingual_error(
            "无可用账号 Token，请重新登录或导入 AT/RT",
            "no account token available",
        ));
    }

    let upstream_base = super::resolve_upstream_base_url();
    let base = upstream_base.as_str();
    let upstream_fallback_base = super::resolve_upstream_fallback_base_url(base);
    let path = super::normalize_models_path("/v1/models");
    let method = Method::GET;
    sort_model_picker_candidates(&storage, &mut candidates);
    let mut last_error = "models request failed".to_string();
    for (account, mut token) in candidates {
        match send_models_request(
            &storage,
            &method,
            &upstream_base,
            &path,
            &account,
            &mut token,
        ) {
            Ok(response_body) => return Ok(parse_models_response(&response_body)),
            Err(err) => {
                // ChatGPT upstream occasionally returns HTML challenge. Try OpenAI fallback.
                if should_retry_models_with_openai_fallback(&err) {
                    if let Some(fallback_base) = upstream_fallback_base.as_deref() {
                        if let Ok(response_body) = send_models_request(
                            &storage,
                            &method,
                            fallback_base,
                            &path,
                            &account,
                            &mut token,
                        ) {
                            return Ok(parse_models_response(&response_body));
                        }
                    }
                }
                last_error = err;
            }
        }
    }

    Err(last_error)
}

/// Probe the Codex model catalog with one exact account instead of accepting a
/// successful usage/quota response as proof that the account can serve Codex.
pub(crate) fn probe_models_for_account(
    storage: &Storage,
    account_id: &str,
) -> Result<ModelsResponse, String> {
    let account = storage
        .find_account_by_id(account_id)
        .map_err(|err| format!("find probe account failed: {err}"))?
        .ok_or_else(|| "probe account not found".to_string())?;
    let mut token = storage
        .find_token_by_account_id(account_id)
        .map_err(|err| format!("find probe account token failed: {err}"))?
        .ok_or_else(|| "probe account token not found".to_string())?;
    let upstream_base = super::resolve_upstream_base_url();
    let path = super::normalize_models_path("/v1/models");
    let body = send_models_probe_request(
        storage,
        &Method::GET,
        upstream_base.as_str(),
        path.as_str(),
        &account,
        &mut token,
    )?;
    let models = parse_models_response(&body);
    if models.models.is_empty() {
        return Err("codex models probe returned no available models".to_string());
    }
    Ok(models)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ModelPickerPlanTier {
    Pro,
    Team,
    Plus,
    Go,
    Free,
    Unknown,
}

/// 函数 `sort_model_picker_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - candidates: 参数 candidates
///
/// # 返回
/// 无
fn sort_model_picker_candidates(storage: &Storage, candidates: &mut [(Account, Token)]) {
    let snapshot_map = latest_snapshot_map_for_candidates(storage, candidates);
    candidates.sort_by_key(|(account, token)| {
        (
            super::is_account_in_cooldown(&account.id),
            super::account_inflight_count(&account.id),
            resolve_model_picker_plan_tier(snapshot_map.get(account.id.as_str()), token),
        )
    });
}

fn latest_snapshot_map_for_candidates(
    storage: &Storage,
    candidates: &[(Account, Token)],
) -> HashMap<String, UsageSnapshotRecord> {
    let account_ids = candidates
        .iter()
        .filter(|(_, token)| {
            plan_tier_from_token(&token.access_token)
                .or_else(|| plan_tier_from_token(&token.id_token))
                .is_none()
        })
        .map(|(account, _)| account.id.clone())
        .collect::<Vec<_>>();
    if account_ids.is_empty() {
        return HashMap::new();
    }
    match storage.latest_usage_snapshots_for_accounts(&account_ids) {
        Ok(snapshots) => snapshots
            .into_iter()
            .map(|snapshot| (snapshot.account_id.clone(), snapshot))
            .collect(),
        Err(err) => {
            log::warn!("model picker usage snapshot prefetch failed: {err}");
            HashMap::new()
        }
    }
}

/// 函数 `resolve_model_picker_plan_tier`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
fn resolve_model_picker_plan_tier(
    snapshot: Option<&UsageSnapshotRecord>,
    token: &Token,
) -> ModelPickerPlanTier {
    plan_tier_from_token(&token.access_token)
        .or_else(|| plan_tier_from_token(&token.id_token))
        .or_else(|| plan_tier_from_usage_snapshot(snapshot))
        .unwrap_or(ModelPickerPlanTier::Unknown)
}

/// 函数 `plan_tier_from_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw_token: 参数 raw_token
///
/// # 返回
/// 返回函数执行结果
fn plan_tier_from_token(raw_token: &str) -> Option<ModelPickerPlanTier> {
    parse_id_token_claims(raw_token)
        .ok()
        .and_then(|claims| claims.auth.and_then(|auth| auth.chatgpt_plan_type))
        .and_then(|value| normalize_model_picker_plan_tier(value.as_str()))
}

/// 函数 `plan_tier_from_usage_snapshot`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - account_id: 参数 account_id
///
/// # 返回
/// 返回函数执行结果
fn plan_tier_from_usage_snapshot(
    snapshot: Option<&UsageSnapshotRecord>,
) -> Option<ModelPickerPlanTier> {
    snapshot
        .and_then(|snapshot| snapshot.credits_json.as_deref())
        .and_then(plan_tier_from_credits_json)
}

/// 函数 `plan_tier_from_credits_json`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn plan_tier_from_credits_json(raw: &str) -> Option<ModelPickerPlanTier> {
    let value = serde_json::from_str::<Value>(raw).ok()?;
    extract_plan_string_by_keys_recursive(
        &value,
        &[
            "plan_type",
            "planType",
            "subscription_tier",
            "subscriptionTier",
            "tier",
            "account_type",
            "accountType",
            "type",
        ],
    )
    .and_then(|value| normalize_model_picker_plan_tier(value.as_str()))
}

/// 函数 `extract_plan_string_by_keys_recursive`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
/// - keys: 参数 keys
///
/// # 返回
/// 返回函数执行结果
fn extract_plan_string_by_keys_recursive(value: &Value, keys: &[&str]) -> Option<String> {
    if let Some(object) = value.as_object() {
        for key in keys {
            let candidate = object
                .get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToString::to_string);
            if candidate.is_some() {
                return candidate;
            }
        }

        for child in object.values() {
            if let Some(nested) = extract_plan_string_by_keys_recursive(child, keys) {
                return Some(nested);
            }
        }
    }

    if let Some(array) = value.as_array() {
        for child in array {
            if let Some(nested) = extract_plan_string_by_keys_recursive(child, keys) {
                return Some(nested);
            }
        }
    }

    None
}

/// 函数 `normalize_model_picker_plan_tier`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn normalize_model_picker_plan_tier(raw: &str) -> Option<ModelPickerPlanTier> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    match normalized.as_str() {
        "pro" => Some(ModelPickerPlanTier::Pro),
        "team" | "business" | "enterprise" | "edu" | "education" => Some(ModelPickerPlanTier::Team),
        "plus" => Some(ModelPickerPlanTier::Plus),
        "go" => Some(ModelPickerPlanTier::Go),
        "free" => Some(ModelPickerPlanTier::Free),
        _ if normalized.contains("enterprise")
            || normalized.contains("business")
            || normalized.contains("team")
            || normalized.contains("education")
            || normalized.contains("edu") =>
        {
            Some(ModelPickerPlanTier::Team)
        }
        _ if normalized.contains("pro") => Some(ModelPickerPlanTier::Pro),
        _ if normalized.contains("plus") => Some(ModelPickerPlanTier::Plus),
        _ if normalized.contains("go") => Some(ModelPickerPlanTier::Go),
        _ if normalized.contains("free") => Some(ModelPickerPlanTier::Free),
        _ => None,
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
