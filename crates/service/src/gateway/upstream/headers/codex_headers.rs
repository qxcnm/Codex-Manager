const X_CODEX_INSTALLATION_ID_HEADER_NAME: &str = "x-codex-installation-id";
const X_CODEX_WINDOW_ID_HEADER_NAME: &str = "x-codex-window-id";
const X_CODEX_PARENT_THREAD_ID_HEADER_NAME: &str = "x-codex-parent-thread-id";
const X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER_NAME: &str =
    "x-responsesapi-include-timing-metrics";
const X_CODEX_INFERENCE_CALL_ID_HEADER_NAME: &str = "x-codex-inference-call-id";
const X_OAI_ATTESTATION_HEADER_NAME: &str = "x-oai-attestation";
const CODEX_UPSTREAM_MIN_VERSION: &str = "0.144.0";

fn anchor_fingerprint_or_dash(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(crate::gateway::anchor_fingerprint::fingerprint_anchor)
        .unwrap_or_else(|| "-".to_string())
}

fn normalize_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn authorization_header_value(auth_token: &str) -> String {
    let value = auth_token.trim();
    if value.starts_with("AgentAssertion ") || value.starts_with("Bearer ") {
        value.to_string()
    } else {
        format!("Bearer {value}")
    }
}

fn identity_product(value: &str) -> Option<&str> {
    let value = value.trim();
    let end = value
        .find(|ch: char| matches!(ch, '/' | ' ' | '('))
        .unwrap_or(value.len());
    let product = value[..end].trim();
    (!product.is_empty()).then_some(product)
}

fn identity_version(user_agent: &str) -> Option<&str> {
    let token = user_agent.trim().split_whitespace().next()?;
    token
        .split_once('/')
        .map(|(_, version)| version.trim())
        .filter(|version| !version.is_empty())
}

fn numeric_version_triplet(value: &str) -> Option<[u64; 3]> {
    let core = value.trim().split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some([major, minor, patch])
}

fn enforce_minimum_codex_version(version: Option<&str>) -> String {
    let configured = crate::gateway::current_codex_user_agent_version();
    let Some(version) = version.map(str::trim).filter(|value| !value.is_empty()) else {
        return configured;
    };
    let Some(candidate) = numeric_version_triplet(version) else {
        return configured;
    };
    let minimum = numeric_version_triplet(CODEX_UPSTREAM_MIN_VERSION)
        .expect("hard-coded Codex minimum version must be valid");
    if candidate < minimum {
        configured
    } else {
        version.to_string()
    }
}

fn looks_like_codex_identity(value: &str) -> bool {
    identity_product(value).is_some_and(|product| product.to_ascii_lowercase().contains("codex"))
}

fn identities_match(user_agent: &str, originator: &str) -> bool {
    let Some(ua_product) = identity_product(user_agent) else {
        return false;
    };
    let Some(originator_product) = identity_product(originator) else {
        return false;
    };
    let canonical = |value: &str| {
        value
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .map(|ch| ch.to_ascii_lowercase())
            .collect::<String>()
    };
    canonical(ua_product) == canonical(originator_product)
}

fn resolve_codex_identity(
    incoming_user_agent: Option<&str>,
    incoming_originator: Option<&str>,
    preserve_client_identity: bool,
) -> (String, String, String) {
    let incoming_user_agent = normalize_non_empty(incoming_user_agent);
    let incoming_originator = normalize_non_empty(incoming_originator);
    if let (Some(user_agent), Some(originator)) = (incoming_user_agent, incoming_originator) {
        let allowed = preserve_client_identity
            || (looks_like_codex_identity(user_agent) && looks_like_codex_identity(originator));
        if allowed && identities_match(user_agent, originator) {
            let version = enforce_minimum_codex_version(identity_version(user_agent));
            return (user_agent.to_string(), originator.to_string(), version);
        }
        log::info!(
            "event=gateway_codex_identity_rebuilt reason=unpaired_identity incoming_ua_product={} incoming_originator_product={}",
            identity_product(user_agent).unwrap_or("-"),
            identity_product(originator).unwrap_or("-"),
        );
    }
    (
        crate::gateway::current_codex_user_agent(),
        crate::gateway::current_wire_originator(),
        crate::gateway::current_codex_user_agent_version(),
    )
}

pub(crate) struct CodexUpstreamHeaderInput<'a> {
    pub(crate) auth_token: &'a str,
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
    let (user_agent, originator, version) = resolve_codex_identity(
        input.incoming_user_agent,
        input.incoming_originator,
        input.preserve_client_identity,
    );
    let mut headers = Vec::with_capacity(16);
    headers.push((
        "Authorization".to_string(),
        authorization_header_value(input.auth_token),
    ));
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
    headers.push(("version".to_string(), version));
    headers.push((
        "OpenAI-Beta".to_string(),
        "responses=experimental".to_string(),
    ));
    if let Some(residency_requirement) = crate::gateway::current_residency_requirement() {
        headers.push((
            crate::gateway::runtime_config::RESIDENCY_HEADER_NAME.to_string(),
            residency_requirement,
        ));
    }
    if let Some(client_request_id) = resolve_client_request_id(input.incoming_client_request_id) {
        headers.push(("x-client-request-id".to_string(), client_request_id));
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
        headers.push(("session_id".to_string(), session_id.to_string()));
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
    let (user_agent, originator, version) = resolve_codex_identity(
        input.incoming_user_agent,
        input.incoming_originator,
        input.preserve_client_identity,
    );
    let mut headers = Vec::with_capacity(13);
    headers.push((
        "Authorization".to_string(),
        authorization_header_value(input.auth_token),
    ));
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
    headers.push(("version".to_string(), version));
    headers.push((
        "OpenAI-Beta".to_string(),
        "responses=experimental".to_string(),
    ));
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
        headers.push(("session_id".to_string(), session_id));
    }
    if let Some(thread_id) = normalize_non_empty(input.thread_id) {
        headers.push(("thread_id".to_string(), thread_id.to_string()));
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
        {
            let matches_session = match normalized_session_id {
                Some(session_id) => {
                    window_id == session_id
                        || window_id.starts_with(format!("{session_id}:").as_str())
                }
                None => true,
            };
            if matches_session {
                return Some(window_id.to_string());
            }
            log::info!(
                "event=gateway_window_id_rebuilt reason=session_mismatch incoming_window_fp={} resolved_session_fp={}",
                anchor_fingerprint_or_dash(Some(window_id)),
                anchor_fingerprint_or_dash(normalized_session_id),
            );
        }
    }
    normalized_session_id.map(|session_id| format!("{session_id}:0"))
}

fn append_passthrough_codex_headers(
    headers: &mut Vec<(String, String)>,
    passthrough_headers: &[(String, String)],
    enabled: bool,
) {
    // 中文注释：Codex wire shape 不接受额外透传头；这里保留参数只为兼容调用签名，
    // 但实际行为是完全丢弃。
    let _ = headers;
    let _ = passthrough_headers;
    let _ = enabled;
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
