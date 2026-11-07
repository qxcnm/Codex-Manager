const X_CODEX_INSTALLATION_ID_HEADER_NAME: &str = "x-codex-installation-id";
const X_CODEX_WINDOW_ID_HEADER_NAME: &str = "x-codex-window-id";
const X_CODEX_PARENT_THREAD_ID_HEADER_NAME: &str = "x-codex-parent-thread-id";
const X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER_NAME: &str =
    "x-responsesapi-include-timing-metrics";
const X_CODEX_INFERENCE_CALL_ID_HEADER_NAME: &str = "x-codex-inference-call-id";
const X_OAI_ATTESTATION_HEADER_NAME: &str = "x-oai-attestation";
const X_OPENAI_INTERNAL_CODEX_RESPONSES_LITE_HEADER_NAME: &str =
    "x-openai-internal-codex-responses-lite";
const X_OPENAI_ACTOR_AUTHORIZATION_HEADER_NAME: &str = "x-openai-actor-authorization";
const X_CODEX_IMAGE_TURN_ID_HEADER_NAME: &str = "x-codex-image-turn-id";

fn normalize_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn is_codex_images_target(target_url: &str) -> bool {
    let path = target_url
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(target_url)
        .trim_end_matches('/');
    path.ends_with("/images/generations") || path.ends_with("/images/edits")
}

pub(crate) fn apply_codex_target_accept_header(
    headers: &mut Vec<(String, String)>,
    target_url: &str,
) {
    if !is_codex_images_target(target_url) {
        return;
    }
    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("Accept"));
    headers.push(("Accept".to_string(), "application/json".to_string()));
}

fn looks_like_codex_identity(value: &str) -> bool {
    value.to_ascii_lowercase().contains("codex")
}

fn resolve_originator_header(
    incoming_originator: Option<&str>,
    preserve_client_identity: bool,
) -> String {
    normalize_non_empty(incoming_originator)
        .filter(|value| preserve_client_identity || looks_like_codex_identity(value))
        .map(str::to_string)
        .unwrap_or_else(crate::gateway::current_wire_originator)
}

fn resolve_user_agent_header(
    _incoming_user_agent: Option<&str>,
    _preserve_client_identity: bool,
) -> String {
    crate::gateway::current_gateway_user_agent()
}

pub(crate) struct CodexUpstreamHeaderInput<'a> {
    pub(crate) auth_token: &'a str,
    pub(crate) is_fedramp: bool,
    pub(crate) chatgpt_account_id: Option<&'a str>,
    pub(crate) incoming_user_agent: Option<&'a str>,
    pub(crate) incoming_originator: Option<&'a str>,
    pub(crate) preserve_client_identity: bool,
    pub(crate) incoming_session_id: Option<&'a str>,
    pub(crate) incoming_window_id: Option<&'a str>,
    pub(crate) incoming_client_request_id: Option<&'a str>,
    pub(crate) incoming_subagent: Option<&'a str>,
    pub(crate) incoming_beta_features: Option<&'a str>,
    pub(crate) incoming_turn_metadata: Option<&'a str>,
    pub(crate) incoming_parent_thread_id: Option<&'a str>,
    pub(crate) incoming_responsesapi_include_timing_metrics: Option<&'a str>,
    pub(crate) incoming_inference_call_id: Option<&'a str>,
    pub(crate) incoming_oai_attestation: Option<&'a str>,
    pub(crate) passthrough_codex_headers: &'a [(String, String)],
    pub(crate) fallback_session_id: Option<&'a str>,
    pub(crate) incoming_turn_state: Option<&'a str>,
    pub(crate) include_turn_state: bool,
    pub(crate) strip_session_affinity: bool,
    pub(crate) has_body: bool,
}

pub(crate) struct CodexCompactUpstreamHeaderInput<'a> {
    pub(crate) auth_token: &'a str,
    pub(crate) is_fedramp: bool,
    pub(crate) chatgpt_account_id: Option<&'a str>,
    pub(crate) installation_id: Option<&'a str>,
    pub(crate) incoming_user_agent: Option<&'a str>,
    pub(crate) incoming_originator: Option<&'a str>,
    pub(crate) preserve_client_identity: bool,
    pub(crate) incoming_session_id: Option<&'a str>,
    pub(crate) thread_id: Option<&'a str>,
    pub(crate) incoming_window_id: Option<&'a str>,
    pub(crate) incoming_subagent: Option<&'a str>,
    pub(crate) incoming_parent_thread_id: Option<&'a str>,
    pub(crate) incoming_oai_attestation: Option<&'a str>,
    pub(crate) passthrough_codex_headers: &'a [(String, String)],
    pub(crate) fallback_session_id: Option<&'a str>,
    pub(crate) strip_session_affinity: bool,
    pub(crate) has_body: bool,
}

pub(crate) fn resolve_codex_installation_id(
    incoming_installation_id: Option<&str>,
) -> Option<String> {
    normalize_non_empty(incoming_installation_id)
        .map(str::to_string)
        .or_else(|| {
            crate::process_env::resolve_installation_id()
                .inspect_err(|err| {
                    log::warn!("event=gateway_installation_id_resolve_failed error={}", err);
                })
                .ok()
        })
}

/// 函数 `build_codex_upstream_headers`
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
pub(crate) fn build_codex_upstream_headers(
    input: CodexUpstreamHeaderInput<'_>,
) -> Vec<(String, String)> {
    let user_agent =
        resolve_user_agent_header(input.incoming_user_agent, input.preserve_client_identity);
    let originator =
        resolve_originator_header(input.incoming_originator, input.preserve_client_identity);
    let mut headers = Vec::with_capacity(16);
    headers.push((
        "Authorization".to_string(),
        crate::agent_identity::format_upstream_authorization(input.auth_token),
    ));
    if input.is_fedramp {
        headers.push(("x-openai-fedramp".to_string(), "true".to_string()));
    }
    if let Some(account_id) = input
        .chatgpt_account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push(("ChatGPT-Account-ID".to_string(), account_id.to_string()));
    }
    if input.has_body {
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
    }
    headers.push(("Accept".to_string(), "text/event-stream".to_string()));
    headers.push(("User-Agent".to_string(), user_agent));
    headers.push(("originator".to_string(), originator));
    if let Some(residency_requirement) = crate::gateway::current_residency_requirement() {
        headers.push((
            crate::gateway::runtime_config::RESIDENCY_HEADER_NAME.to_string(),
            residency_requirement,
        ));
    }
    let resolved_client_request_id = resolve_client_request_id(input.incoming_client_request_id);
    if let Some(client_request_id) = resolved_client_request_id.as_deref() {
        headers.push((
            "x-client-request-id".to_string(),
            client_request_id.to_string(),
        ));
        headers.push(("thread-id".to_string(), client_request_id.to_string()));
    }
    if let Some(subagent) = input
        .incoming_subagent
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push(("x-openai-subagent".to_string(), subagent.to_string()));
    }
    if let Some(beta_features) = input
        .incoming_beta_features
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            "x-codex-beta-features".to_string(),
            beta_features.to_string(),
        ));
    }
    if let Some(turn_metadata) = input
        .incoming_turn_metadata
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            "x-codex-turn-metadata".to_string(),
            turn_metadata.to_string(),
        ));
    }
    if let Some(parent_thread_id) = input
        .incoming_parent_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            X_CODEX_PARENT_THREAD_ID_HEADER_NAME.to_string(),
            parent_thread_id.to_string(),
        ));
    }
    if let Some(include_timing_metrics) = input
        .incoming_responsesapi_include_timing_metrics
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER_NAME.to_string(),
            include_timing_metrics.to_string(),
        ));
    }
    if let Some(inference_call_id) = input
        .incoming_inference_call_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            X_CODEX_INFERENCE_CALL_ID_HEADER_NAME.to_string(),
            inference_call_id.to_string(),
        ));
    }
    if let Some(oai_attestation) = input
        .incoming_oai_attestation
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            X_OAI_ATTESTATION_HEADER_NAME.to_string(),
            oai_attestation.to_string(),
        ));
    }
    let resolved_session_id = resolve_optional_session_id(
        input.incoming_session_id,
        input.fallback_session_id,
        input.strip_session_affinity,
    );
    if let Some(session_id) = resolved_session_id.as_deref() {
        headers.push(("session-id".to_string(), session_id.to_string()));
    }
    if let Some(window_id) = resolve_window_id(
        input.incoming_window_id,
        resolved_session_id.as_deref(),
        input.strip_session_affinity,
    ) {
        headers.push((X_CODEX_WINDOW_ID_HEADER_NAME.to_string(), window_id));
    }
    append_passthrough_codex_headers(
        &mut headers,
        input.passthrough_codex_headers,
        !input.strip_session_affinity,
    );

    if !input.strip_session_affinity {
        if input.include_turn_state {
            if let Some(turn_state) = input.incoming_turn_state {
                headers.push(("x-codex-turn-state".to_string(), turn_state.to_string()));
            }
        }
    }

    headers
}

/// 函数 `build_codex_compact_upstream_headers`
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
pub(crate) fn build_codex_compact_upstream_headers(
    input: CodexCompactUpstreamHeaderInput<'_>,
) -> Vec<(String, String)> {
    let user_agent =
        resolve_user_agent_header(input.incoming_user_agent, input.preserve_client_identity);
    let originator =
        resolve_originator_header(input.incoming_originator, input.preserve_client_identity);
    let mut headers = Vec::with_capacity(13);
    headers.push((
        "Authorization".to_string(),
        crate::agent_identity::format_upstream_authorization(input.auth_token),
    ));
    if input.is_fedramp {
        headers.push(("x-openai-fedramp".to_string(), "true".to_string()));
    }
    if let Some(account_id) = input
        .chatgpt_account_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push(("ChatGPT-Account-ID".to_string(), account_id.to_string()));
    }
    if let Some(installation_id) = input
        .installation_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            X_CODEX_INSTALLATION_ID_HEADER_NAME.to_string(),
            installation_id.to_string(),
        ));
    }
    if input.has_body {
        headers.push(("Content-Type".to_string(), "application/json".to_string()));
    }
    headers.push(("Accept".to_string(), "application/json".to_string()));
    headers.push(("User-Agent".to_string(), user_agent));
    headers.push(("originator".to_string(), originator));
    if let Some(residency_requirement) = crate::gateway::current_residency_requirement() {
        headers.push((
            crate::gateway::runtime_config::RESIDENCY_HEADER_NAME.to_string(),
            residency_requirement,
        ));
    }
    if let Some(subagent) = input
        .incoming_subagent
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push(("x-openai-subagent".to_string(), subagent.to_string()));
    }
    if let Some(parent_thread_id) = input
        .incoming_parent_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            X_CODEX_PARENT_THREAD_ID_HEADER_NAME.to_string(),
            parent_thread_id.to_string(),
        ));
    }
    if let Some(oai_attestation) = input
        .incoming_oai_attestation
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.push((
            X_OAI_ATTESTATION_HEADER_NAME.to_string(),
            oai_attestation.to_string(),
        ));
    }
    let resolved_session_id = resolve_optional_session_id(
        input.incoming_session_id,
        input.fallback_session_id,
        input.strip_session_affinity,
    );
    if let Some(session_id) = resolved_session_id.clone() {
        headers.push(("session-id".to_string(), session_id));
    }
    if let Some(thread_id) = normalize_non_empty(input.thread_id) {
        headers.push(("thread-id".to_string(), thread_id.to_string()));
    }
    if let Some(window_id) = resolve_window_id(
        input.incoming_window_id,
        resolved_session_id.as_deref(),
        input.strip_session_affinity,
    ) {
        headers.push((X_CODEX_WINDOW_ID_HEADER_NAME.to_string(), window_id));
    }
    append_passthrough_codex_headers(
        &mut headers,
        input.passthrough_codex_headers,
        !input.strip_session_affinity,
    );
    headers
}

/// 函数 `resolve_optional_session_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - incoming: 参数 incoming
/// - fallback_session_id: 参数 fallback_session_id
/// - strip_session_affinity: 参数 strip_session_affinity
///
/// # 返回
/// 返回函数执行结果
fn resolve_optional_session_id(
    incoming: Option<&str>,
    fallback_session_id: Option<&str>,
    strip_session_affinity: bool,
) -> Option<String> {
    if strip_session_affinity {
        return fallback_session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
    }
    if let Some(value) = incoming {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    fallback_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn resolve_window_id(
    incoming_window_id: Option<&str>,
    resolved_session_id: Option<&str>,
    strip_session_affinity: bool,
) -> Option<String> {
    let normalized_session_id = resolved_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if !strip_session_affinity {
        if let Some(window_id) = incoming_window_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|value| reqwest::header::HeaderValue::from_str(value).is_ok())
        {
            // Codex windows belong to a thread, while session-id can be an
            // independent cache-affinity override. A different prefix is valid.
            return Some(window_id.to_string());
        }
    }
    normalized_session_id.map(|session_id| format!("{session_id}:0"))
}

fn append_passthrough_codex_headers(
    headers: &mut Vec<(String, String)>,
    passthrough_headers: &[(String, String)],
    _enabled: bool,
) {
    for (name, value) in passthrough_headers {
        if crate::gateway::is_codexmanager_image_extension_actor_authorization(name, value) {
            continue;
        }
        if !(name.eq_ignore_ascii_case(X_OPENAI_INTERNAL_CODEX_RESPONSES_LITE_HEADER_NAME)
            || name.eq_ignore_ascii_case(X_OPENAI_ACTOR_AUTHORIZATION_HEADER_NAME)
            || name.eq_ignore_ascii_case(X_CODEX_IMAGE_TURN_ID_HEADER_NAME))
            || headers
                .iter()
                .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
        {
            continue;
        }
        headers.push((name.clone(), value.clone()));
    }
}

/// 函数 `resolve_client_request_id`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - incoming_client_request_id: 参数 incoming_client_request_id
///
/// # 返回
/// 返回函数执行结果
fn resolve_client_request_id(incoming_client_request_id: Option<&str>) -> Option<String> {
    if let Some(value) = incoming_client_request_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    None
}

#[cfg(test)]
#[path = "codex_headers_tests.rs"]
mod tests;
