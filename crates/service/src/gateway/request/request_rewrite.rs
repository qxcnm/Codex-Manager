use serde_json::Value;

mod request_rewrite_chat_completions;
mod request_rewrite_multipart;
mod request_rewrite_shared;

use super::official_responses_http as responses;
use request_rewrite_chat_completions as chat_completions;
use request_rewrite_multipart as multipart;

type RetainFn = fn(&str, &mut serde_json::Map<String, Value>) -> Vec<String>;
const RETAIN_FN_PROBE_KEY: &str = "__codexmanager_allowlist_probe__";

/// 函数 `compute_upstream_url`
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
pub(super) fn compute_upstream_url(base: &str, path: &str) -> (String, Option<String>) {
    let base = base.trim_end_matches('/');
    let upstream_path = resolve_upstream_request_path(path);
    let url = if base.contains("/backend-api/codex") && upstream_path.starts_with("/v1/") {
        // 中文注释：兼容 ChatGPT backend-api/codex 的路径约定；不做映射会导致 /v1/* 请求 404。
        format!("{}{}", base, upstream_path.trim_start_matches("/v1"))
    } else if base.ends_with("/v1") && upstream_path.starts_with("/v1") {
        format!("{}{}", base.trim_end_matches("/v1"), upstream_path)
    } else {
        format!("{}{}", base, upstream_path)
    };
    let url_alt = if base.contains("/backend-api/codex") && upstream_path.starts_with("/v1/") {
        Some(format!("{}{}", base, upstream_path))
    } else {
        None
    };
    (url, url_alt)
}

fn resolve_upstream_request_path(path: &str) -> String {
    if !responses::is_compact_path(path) {
        return path.to_string();
    }
    let compact_api_path = super::current_compact_api_path();
    rewrite_path_preserving_query(path, compact_api_path.as_str())
}

fn rewrite_path_preserving_query(path: &str, replacement_path: &str) -> String {
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .filter(|query| !query.trim().is_empty());
    match query {
        Some(query) => format!("{replacement_path}?{query}"),
        None => replacement_path.to_string(),
    }
}

/// 函数 `is_codex_backend_base`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - base: 参数 base
///
/// # 返回
/// 返回函数执行结果
fn is_codex_backend_base(base: &str) -> bool {
    base.to_ascii_lowercase().contains("/backend-api/codex")
}

/// 函数 `should_apply_codex_responses_compat`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
/// - explicit_upstream_base: 参数 explicit_upstream_base
///
/// # 返回
/// 返回函数执行结果
fn should_apply_codex_responses_compat(
    path: &str,
    explicit_upstream_base: Option<&str>,
    resolve_default_upstream_base: bool,
) -> bool {
    if !responses::is_responses_path(path) {
        return false;
    }
    let resolved_base = match explicit_upstream_base {
        Some(base) => base.to_string(),
        None if resolve_default_upstream_base => {
            super::upstream::config::resolve_upstream_base_url()
        }
        None => return false,
    };
    let normalized_base = super::upstream::config::normalize_upstream_base_url(&resolved_base);
    is_codex_backend_base(&normalized_base)
}

/// 函数 `path_matches_retain_fn`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
/// - retain_fn: 参数 retain_fn
///
/// # 返回
/// 返回函数执行结果
fn path_matches_retain_fn(path: &str, retain_fn: RetainFn) -> bool {
    let mut probe = serde_json::Map::new();
    probe.insert(RETAIN_FN_PROBE_KEY.to_string(), Value::Null);
    retain_fn(path, &mut probe);
    !probe.contains_key(RETAIN_FN_PROBE_KEY)
}

/// 函数 `resolve_retain_fn`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
/// - use_codex_responses_compat: 参数 use_codex_responses_compat
///
/// # 返回
/// 返回函数执行结果
fn resolve_retain_fn(path: &str, use_codex_responses_compat: bool) -> Option<RetainFn> {
    if path_matches_retain_fn(path, chat_completions::retain_official_fields) {
        return Some(chat_completions::retain_official_fields);
    }
    if use_codex_responses_compat {
        if path_matches_retain_fn(path, responses::retain_codex_fields) {
            return Some(responses::retain_codex_fields);
        }
    } else if path_matches_retain_fn(path, responses::retain_official_fields) {
        return Some(responses::retain_official_fields);
    }
    None
}

/// 函数 `is_allowed_field`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
/// - key: 参数 key
/// - retain_fn: 参数 retain_fn
///
/// # 返回
/// 返回函数执行结果
fn is_allowed_field(path: &str, key: &str, retain_fn: RetainFn) -> bool {
    let mut one = serde_json::Map::new();
    one.insert(key.to_string(), Value::Null);
    retain_fn(path, &mut one);
    one.contains_key(key)
}

/// 函数 `filter_form_urlencoded_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
/// - body: 参数 body
/// - retain_fn: 参数 retain_fn
///
/// # 返回
/// 返回函数执行结果
fn filter_form_urlencoded_body(
    path: &str,
    body: &[u8],
    retain_fn: RetainFn,
) -> Option<(Vec<u8>, Vec<String>)> {
    if !body.contains(&b'=') {
        return None;
    }
    let pairs = url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return None;
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    let mut dropped_keys = Vec::new();
    for (key, value) in pairs {
        if is_allowed_field(path, &key, retain_fn) {
            serializer.append_pair(&key, &value);
        } else {
            dropped_keys.push(key);
        }
    }
    if dropped_keys.is_empty() {
        return None;
    }
    Some((serializer.finish().into_bytes(), dropped_keys))
}

/// 函数 `apply_model_forward_rule_if_needed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-05
///
/// # 参数
/// - obj: 参数 obj
///
/// # 返回
/// 返回函数执行结果
fn apply_model_forward_rule_if_needed(obj: &mut serde_json::Map<String, Value>) -> bool {
    let Some(current_model) = obj
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let Some(forwarded_model) = super::resolve_forwarded_model(current_model) else {
        return false;
    };
    if forwarded_model.eq_ignore_ascii_case(current_model) {
        return false;
    }
    obj.insert("model".to_string(), Value::String(forwarded_model));
    true
}

fn normalize_client_ultra_for_upstream(obj: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    if let Some(Value::String(effort)) = obj.get_mut("reasoning_effort") {
        if effort.trim().eq_ignore_ascii_case("ultra") {
            *effort =
                crate::reasoning_effort::normalize_client_reasoning_effort_for_upstream(effort)
                    .expect("ultra must normalize to an upstream effort")
                    .to_string();
            changed = true;
        }
    }
    if let Some(Value::String(effort)) = obj
        .get_mut("reasoning")
        .and_then(Value::as_object_mut)
        .and_then(|reasoning| reasoning.get_mut("effort"))
    {
        if effort.trim().eq_ignore_ascii_case("ultra") {
            *effort =
                crate::reasoning_effort::normalize_client_reasoning_effort_for_upstream(effort)
                    .expect("ultra must normalize to an upstream effort")
                    .to_string();
            changed = true;
        }
    }
    changed
}

fn compact_model_override_for_path(path: &str) -> Option<String> {
    if responses::is_compact_path(path) {
        return super::current_compact_model_override();
    }
    None
}

fn chat_request_rules_path(path: &str) -> String {
    if responses::is_compact_path(path) && super::compact_api_path_uses_chat_completions() {
        return super::current_compact_api_path();
    }
    path.to_string()
}

/// 函数 `apply_request_overrides`
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
#[allow(dead_code)]
pub(super) fn apply_request_overrides(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_base_url: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier(
        path,
        body,
        model_slug,
        reasoning_effort,
        None,
        upstream_base_url,
    )
}

/// 函数 `apply_request_overrides_with_service_tier`
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
pub(super) fn apply_request_overrides_with_service_tier(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    upstream_base_url: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        path,
        body,
        model_slug,
        reasoning_effort,
        service_tier,
        upstream_base_url,
        None,
        false,
    )
}

/// 函数 `apply_request_overrides_with_prompt_cache_key`
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
#[allow(dead_code)]
pub(super) fn apply_request_overrides_with_prompt_cache_key(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        path,
        body,
        model_slug,
        reasoning_effort,
        None,
        upstream_base_url,
        prompt_cache_key,
        false,
    )
}

/// 函数 `apply_request_overrides_with_service_tier_and_prompt_cache_key`
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
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn apply_request_overrides_with_service_tier_and_prompt_cache_key(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        path,
        body,
        model_slug,
        reasoning_effort,
        service_tier,
        upstream_base_url,
        prompt_cache_key,
        false,
    )
}

pub(super) fn apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
    allow_codex_compat_rewrite: bool,
) -> Vec<u8> {
    apply_request_overrides_with_prompt_cache_key_mode(
        path,
        body,
        model_slug,
        reasoning_effort,
        upstream_base_url,
        prompt_cache_key,
        false,
        service_tier,
        allow_codex_compat_rewrite,
        true,
    )
}

/// 函数 `apply_request_overrides_with_forced_prompt_cache_key`
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
#[cfg(test)]
pub(super) fn apply_request_overrides_with_forced_prompt_cache_key(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier_and_forced_prompt_cache_key(
        path,
        body,
        model_slug,
        reasoning_effort,
        None,
        upstream_base_url,
        prompt_cache_key,
    )
}

/// 函数 `apply_request_overrides_with_service_tier_and_forced_prompt_cache_key`
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
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn apply_request_overrides_with_service_tier_and_forced_prompt_cache_key(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope(
        path,
        body,
        model_slug,
        reasoning_effort,
        service_tier,
        upstream_base_url,
        prompt_cache_key,
        false,
    )
}

pub(super) fn apply_request_overrides_with_service_tier_and_forced_prompt_cache_key_scope(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
    allow_codex_compat_rewrite: bool,
) -> Vec<u8> {
    apply_request_overrides_with_prompt_cache_key_mode(
        path,
        body,
        model_slug,
        reasoning_effort,
        upstream_base_url,
        prompt_cache_key,
        true,
        service_tier,
        allow_codex_compat_rewrite,
        true,
    )
}

pub(super) fn apply_request_overrides_for_deferred_aggregate(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
) -> Vec<u8> {
    apply_request_overrides_with_prompt_cache_key_mode(
        path,
        body,
        model_slug,
        reasoning_effort,
        None,
        None,
        false,
        service_tier,
        false,
        false,
    )
}

pub(super) fn apply_codex_candidate_transport_rules(path: &str, body: Vec<u8>) -> Vec<u8> {
    if body.is_empty() {
        return body;
    }
    let Ok(mut payload) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(obj) = payload.as_object_mut() else {
        return body;
    };
    let installation_id = crate::process_env::resolve_installation_id()
        .inspect_err(|err| {
            log::warn!("event=gateway_installation_id_resolve_failed error={}", err);
        })
        .ok();
    let result = responses::apply_codex_http_request_rules(
        path,
        obj,
        true,
        None,
        false,
        installation_id.as_deref(),
    );
    if !result.changed {
        return body;
    }
    serde_json::to_vec(&payload).unwrap_or(body)
}

pub(super) fn apply_external_dynamic_tools_transport_rules(path: &str, body: Vec<u8>) -> Vec<u8> {
    if body.is_empty() || !responses::is_responses_path(path) {
        return body;
    }
    let Ok(mut payload) = serde_json::from_slice::<Value>(&body) else {
        return body;
    };
    let Some(obj) = payload.as_object_mut() else {
        return body;
    };
    if !responses::normalize_dynamic_tools_to_tools(path, obj) {
        return body;
    }
    serde_json::to_vec(&payload).unwrap_or(body)
}

/// 函数 `apply_request_overrides_with_prompt_cache_key_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
/// - body: 参数 body
/// - model_slug: 参数 model_slug
/// - reasoning_effort: 参数 reasoning_effort
/// - upstream_base_url: 参数 upstream_base_url
/// - prompt_cache_key: 参数 prompt_cache_key
/// - force_prompt_cache_key: 参数 force_prompt_cache_key
/// - service_tier: 参数 service_tier
///
/// # 返回
/// 返回函数执行结果
fn apply_request_overrides_with_prompt_cache_key_mode(
    path: &str,
    body: Vec<u8>,
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    upstream_base_url: Option<&str>,
    prompt_cache_key: Option<&str>,
    force_prompt_cache_key: bool,
    service_tier: Option<&str>,
    allow_codex_compat_rewrite: bool,
    resolve_default_upstream_base: bool,
) -> Vec<u8> {
    let use_codex_responses_compat =
        should_apply_codex_responses_compat(path, upstream_base_url, resolve_default_upstream_base);
    let use_codex_compat_rewrite = allow_codex_compat_rewrite && use_codex_responses_compat;
    // Non-official Responses providers commonly reject Codex's dynamicTools
    // envelope. Convert it only when an explicit non-Codex upstream is known;
    // deferred routing must keep the original body until a candidate is chosen.
    let normalize_external_dynamic_tools = upstream_base_url.is_some()
        && responses::is_responses_path(path)
        && !use_codex_responses_compat;
    let chat_rules_path = chat_request_rules_path(path);
    let normalized_model = model_slug
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let compact_model_override = compact_model_override_for_path(path);
    let normalized_reasoning = reasoning_effort
        .and_then(crate::reasoning_effort::normalize_reasoning_effort)
        .map(str::to_string);
    let normalized_service_tier = service_tier
        .and_then(crate::apikey::service_tier::normalize_service_tier)
        .map(str::to_string);
    if body.is_empty() {
        return body;
    }
    if let Ok(mut payload) = serde_json::from_slice::<Value>(&body) {
        if let Some(obj) = payload.as_object_mut() {
            let mut changed = false;
            let mut dropped_keys = Vec::new();

            if normalize_external_dynamic_tools
                && responses::normalize_dynamic_tools_to_tools(path, obj)
            {
                changed = true;
            }

            // Ultra 由 Codex 客户端负责多代理编排；单个上游请求必须使用 Max。
            // 客户端原始值在进入此重写前已单独采集，供请求日志展示 ultra -> max。
            if normalize_client_ultra_for_upstream(obj) {
                changed = true;
            }

            let effective_model = compact_model_override
                .as_deref()
                .or(normalized_model.as_deref());
            if let Some(model) = effective_model {
                let forwarded_model = super::resolve_builtin_forwarded_model(model)
                    .unwrap_or_else(|| model.to_string());
                obj.insert("model".to_string(), Value::String(forwarded_model));
                changed = true;
            } else if apply_model_forward_rule_if_needed(obj) {
                changed = true;
            }

            if let Some(level) = normalized_reasoning.as_deref() {
                if responses::apply_reasoning_override(path, obj, Some(level)) {
                    changed = true;
                }
            }

            if let Some(service_tier) = normalized_service_tier.as_deref() {
                obj.insert(
                    "service_tier".to_string(),
                    Value::String(service_tier.to_string()),
                );
                changed = true;
            }

            if use_codex_responses_compat {
                let installation_id = crate::process_env::resolve_installation_id()
                    .inspect_err(|err| {
                        log::warn!("event=gateway_installation_id_resolve_failed error={}", err);
                    })
                    .ok();
                let codex_http_result = responses::apply_codex_http_request_rules(
                    path,
                    obj,
                    use_codex_compat_rewrite,
                    prompt_cache_key,
                    force_prompt_cache_key,
                    installation_id.as_deref(),
                );
                if codex_http_result.changed {
                    changed = true;
                }
                dropped_keys.extend(codex_http_result.dropped_keys);
            }

            if chat_completions::normalize_responses_payload(chat_rules_path.as_str(), obj) {
                changed = true;
            }

            if let Some(level) = normalized_reasoning.as_deref() {
                if chat_completions::apply_reasoning_override(
                    chat_rules_path.as_str(),
                    obj,
                    Some(level),
                ) {
                    changed = true;
                }
            }

            if chat_completions::ensure_reasoning_effort(chat_rules_path.as_str(), obj) {
                changed = true;
            }
            if chat_completions::ensure_stream_usage_override(chat_rules_path.as_str(), obj) {
                changed = true;
            }

            if super::strict_request_param_allowlist_enabled() {
                dropped_keys.extend(chat_completions::retain_official_fields(
                    chat_rules_path.as_str(),
                    obj,
                ));
                if !use_codex_responses_compat {
                    dropped_keys.extend(responses::retain_official_fields(path, obj));
                }
            }

            if !dropped_keys.is_empty() {
                dropped_keys.sort_unstable();
                dropped_keys.dedup();
                changed = true;
                log::debug!(
                    "event=gateway_request_param_filtered path={} dropped_keys={}",
                    path,
                    dropped_keys.join(",")
                );
            }

            if !changed {
                return body;
            }
            return serde_json::to_vec(&payload).unwrap_or(body);
        }
    }

    if !super::strict_request_param_allowlist_enabled() {
        return body;
    }
    let Some(retain_fn) = resolve_retain_fn(path, use_codex_responses_compat) else {
        return body;
    };

    let filtered = multipart::filter_multipart_form_data_body(path, &body, retain_fn)
        .or_else(|| filter_form_urlencoded_body(path, &body, retain_fn));
    let Some((filtered_body, mut dropped_keys)) = filtered else {
        return body;
    };

    dropped_keys.sort_unstable();
    dropped_keys.dedup();
    log::debug!(
        "event=gateway_request_param_filtered path={} dropped_keys={}",
        path,
        dropped_keys.join(",")
    );
    filtered_body
}

#[cfg(test)]
#[path = "tests/request_rewrite_tests.rs"]
mod tests;
