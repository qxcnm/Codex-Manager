use super::*;
use crate::gateway::conversation_binding::RouteConversationSource;
use crate::gateway::{
    adapt_request_for_protocol, apply_request_overrides_with_service_tier_and_prompt_cache_key,
    apply_request_overrides_with_service_tier_and_prompt_cache_key_scope,
};
use axum::http::{HeaderMap, HeaderValue};
use codexmanager_core::rpc::types::{ModelInfo, ModelsResponse};
use codexmanager_core::storage::Storage;
use serde_json::Value;

const COMPACT_API_PATH_ENV: &str = "CODEXMANAGER_COMPACT_API_PATH";

struct RuntimeEnvGuard {
    name: &'static str,
    previous_value: Option<String>,
}

impl RuntimeEnvGuard {
    fn set(name: &'static str, value: &str) -> Self {
        let previous_value = std::env::var(name).ok();
        std::env::set_var(name, value);
        crate::gateway::reload_runtime_config_from_env();
        Self {
            name,
            previous_value,
        }
    }
}

impl Drop for RuntimeEnvGuard {
    fn drop(&mut self) {
        match self.previous_value.as_deref() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
        crate::gateway::reload_runtime_config_from_env();
    }
}

/// 函数 `sample_api_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - protocol_type: 参数 protocol_type
/// - model_slug: 参数 model_slug
/// - reasoning: 参数 reasoning
/// - service_tier: 参数 service_tier
///
/// # 返回
/// 返回函数执行结果
fn sample_api_key(
    protocol_type: &str,
    model_slug: Option<&str>,
    reasoning: Option<&str>,
    service_tier: Option<&str>,
) -> ApiKey {
    ApiKey {
        id: "gk_test".to_string(),
        name: Some("test".to_string()),
        model_slug: model_slug.map(|value| value.to_string()),
        reasoning_effort: reasoning.map(|value| value.to_string()),
        service_tier: service_tier.map(|value| value.to_string()),
        client_type: "codex".to_string(),
        protocol_type: protocol_type.to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: "hash".to_string(),
        status: "active".to_string(),
        created_at: 0,
        last_used_at: None,
        rotation_strategy: crate::apikey_profile::ROTATION_ACCOUNT.to_string(),
        aggregate_api_id: None,
        aggregate_api_url: None,
        account_plan_filter: None,
    }
}

#[test]
fn api_key_model_policy_enforces_whitelist_and_platform_scope() {
    let kiro_only = codexmanager_core::storage::ApiKeyPolicy {
        key_id: "key".into(),
        allowed_models: vec!["smart".into(), "kiro/claude-sonnet-4.5".into()],
        allowed_platforms: vec!["kiro".into()],
        model_visibility: "selectable".into(),
        expires_at: None,
        concurrency_limit: Some(2),
    };
    assert!(ensure_api_key_policy_allows_model(&kiro_only, Some("smart")).is_ok());
    assert!(ensure_api_key_policy_allows_model(&kiro_only, Some("kiro/claude-sonnet-4.5")).is_ok());
    let codex = ensure_api_key_policy_allows_model(&kiro_only, Some("codex/gpt-5.3-codex"))
        .expect_err("codex must be rejected");
    assert_eq!(codex.status_code, 403);
    let grok = ensure_api_key_policy_allows_model(&kiro_only, Some("grok/grok-chat-fast"))
        .expect_err("grok must be rejected");
    assert_eq!(grok.status_code, 403);
    let other = ensure_api_key_policy_allows_model(&kiro_only, Some("kiro/claude-opus-4.8"))
        .expect_err("model must be rejected");
    assert_eq!(other.status_code, 403);
}

#[test]
fn api_key_model_policy_accepts_grok_when_platform_is_enabled() {
    let grok_only = codexmanager_core::storage::ApiKeyPolicy {
        key_id: "key".into(),
        allowed_models: vec!["grok/grok-chat-fast".into()],
        allowed_platforms: vec!["grok".into()],
        model_visibility: "selectable".into(),
        expires_at: None,
        concurrency_limit: None,
    };
    assert!(ensure_api_key_policy_allows_model(&grok_only, Some("grok/grok-chat-fast")).is_ok());
}

/// 函数 `anthropic_key_keeps_empty_overrides`
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
#[test]
fn anthropic_key_keeps_empty_overrides() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        None,
        None,
        None,
    );
    let (model, reasoning, service_tier) = resolve_effective_request_overrides(&api_key);
    assert_eq!(model, None);
    assert_eq!(reasoning, None);
    assert_eq!(service_tier, None);
}

/// 函数 `anthropic_key_applies_custom_model_and_reasoning`
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
#[test]
fn anthropic_key_applies_custom_model_and_reasoning() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        Some("gpt-5.3-codex"),
        Some("extra_high"),
        Some("fast"),
    );
    let (model, reasoning, service_tier) = resolve_effective_request_overrides(&api_key);
    assert_eq!(model.as_deref(), Some("gpt-5.3-codex"));
    assert_eq!(reasoning.as_deref(), Some("xhigh"));
    assert_eq!(service_tier.as_deref(), Some("fast"));
}

#[test]
fn anthropic_key_maps_fast_service_tier_to_priority_on_adapted_responses_request() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        Some("gpt-5.3-codex"),
        Some("high"),
        Some("fast"),
    );
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [{ "role": "user", "content": "hi" }],
        "stream": false
    });
    let body = serde_json::to_vec(&body).expect("serialize anthropic request");

    let adapted = adapt_request_for_protocol(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/messages",
        body,
    )
    .expect("adapt anthropic request");
    let (model, reasoning, service_tier) = resolve_effective_request_overrides(&api_key);
    let rewritten = apply_request_overrides_with_service_tier_and_prompt_cache_key(
        adapted.path.as_str(),
        adapted.body,
        model.as_deref(),
        reasoning.as_deref(),
        service_tier.as_deref(),
        None,
        None,
    );
    let normalized = normalize_compat_service_tier_for_codex_backend(rewritten);
    let payload: Value = serde_json::from_slice(&normalized).expect("json body");

    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
}

#[test]
fn compat_service_tier_normalizer_maps_auto_to_priority() {
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "input": [],
        "service_tier": "auto"
    });
    let normalized = normalize_compat_service_tier_for_codex_backend(
        serde_json::to_vec(&body).expect("serialize request"),
    );
    let payload: Value = serde_json::from_slice(&normalized).expect("json body");

    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
}

#[test]
fn anthropic_key_ignores_unsupported_flex_service_tier_on_responses_request() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        Some("gpt-5.3-codex"),
        Some("high"),
        Some("flex"),
    );
    let body = serde_json::json!({
        "model": "gpt-5.3-codex",
        "messages": [{ "role": "user", "content": "hi" }],
        "stream": false
    });
    let body = serde_json::to_vec(&body).expect("serialize anthropic request");

    let adapted = adapt_request_for_protocol(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        "/v1/messages",
        body,
    )
    .expect("adapt anthropic request");
    let (model, reasoning, service_tier) = resolve_effective_request_overrides(&api_key);
    let rewritten = apply_request_overrides_with_service_tier_and_prompt_cache_key(
        adapted.path.as_str(),
        adapted.body,
        model.as_deref(),
        reasoning.as_deref(),
        service_tier.as_deref(),
        None,
        None,
    );
    let payload: Value = serde_json::from_slice(&rewritten).expect("json body");

    assert!(payload.get("service_tier").is_none());
}

/// 函数 `openai_key_keeps_empty_overrides`
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
#[test]
fn openai_key_keeps_empty_overrides() {
    let api_key = sample_api_key("openai_compat", None, None, None);
    let (model, reasoning, service_tier) = resolve_effective_request_overrides(&api_key);
    assert_eq!(model, None);
    assert_eq!(reasoning, None);
    assert_eq!(service_tier, None);
}

#[test]
fn openai_key_keeps_codex_long_tail_slug_override() {
    let api_key = sample_api_key(
        "openai_compat",
        Some("gpt-5.3-codex-spark"),
        Some("medium"),
        None,
    );
    let (model, reasoning, service_tier) = resolve_effective_request_overrides(&api_key);
    assert_eq!(model.as_deref(), Some("gpt-5.3-codex-spark"));
    assert_eq!(reasoning.as_deref(), Some("medium"));
    assert_eq!(service_tier, None);
}

#[test]
fn gateway_blocked_path_matches_default_props_probe() {
    assert!(is_gateway_blocked_request_path("/v1/props"));
    assert!(is_gateway_blocked_request_path("/v1/props?client=hermes"));
    assert!(!is_gateway_blocked_request_path("/v1/responses"));
}

#[test]
fn gateway_blocked_path_patterns_support_custom_exact_and_prefix_rules() {
    let patterns = parse_gateway_blocked_path_patterns("/internal/health; /v1/debug/*\n/v1/audit");

    assert!(patterns
        .iter()
        .any(|pattern| gateway_blocked_path_matches("/internal/health?probe=1", pattern)));
    assert!(patterns
        .iter()
        .any(|pattern| gateway_blocked_path_matches("/v1/debug/trace", pattern)));
    assert!(patterns
        .iter()
        .any(|pattern| gateway_blocked_path_matches("/v1/audit", pattern)));
    assert!(!patterns
        .iter()
        .any(|pattern| gateway_blocked_path_matches("/v1/responses", pattern)));
}

fn sample_request_metadata(prompt_cache_key: Option<&str>) -> ParsedRequestMetadata {
    ParsedRequestMetadata {
        prompt_cache_key: prompt_cache_key.map(str::to_string),
        has_prompt_cache_key: prompt_cache_key.is_some(),
        ..Default::default()
    }
}

fn sample_request_metadata_with_previous_response(
    prompt_cache_key: Option<&str>,
    previous_response_id: Option<&str>,
) -> ParsedRequestMetadata {
    ParsedRequestMetadata {
        prompt_cache_key: prompt_cache_key.map(str::to_string),
        has_prompt_cache_key: prompt_cache_key.is_some(),
        has_previous_response_id: previous_response_id
            .map(str::trim)
            .is_some_and(|value| !value.is_empty()),
        ..Default::default()
    }
}

fn sample_incoming_headers(
    conversation_id: Option<&str>,
    turn_state: Option<&str>,
    user_agent: Option<&str>,
    originator: Option<&str>,
    session_affinity: Option<&str>,
) -> super::super::super::IncomingHeaderSnapshot {
    sample_incoming_headers_with_session_id(
        conversation_id,
        turn_state,
        user_agent,
        originator,
        session_affinity,
        None,
        None,
    )
}

fn sample_incoming_headers_with_session_id(
    conversation_id: Option<&str>,
    turn_state: Option<&str>,
    user_agent: Option<&str>,
    originator: Option<&str>,
    session_affinity: Option<&str>,
    session_id: Option<&str>,
    subagent: Option<&str>,
) -> super::super::super::IncomingHeaderSnapshot {
    let mut headers = HeaderMap::new();
    if let Some(conversation_id) = conversation_id {
        headers.insert(
            "conversation_id",
            HeaderValue::from_str(conversation_id).expect("header"),
        );
    }
    if let Some(turn_state) = turn_state {
        headers.insert(
            "x-codex-turn-state",
            HeaderValue::from_str(turn_state).expect("header"),
        );
    }
    if let Some(user_agent) = user_agent {
        headers.insert(
            "User-Agent",
            HeaderValue::from_str(user_agent).expect("header"),
        );
    }
    if let Some(originator) = originator {
        headers.insert(
            "originator",
            HeaderValue::from_str(originator).expect("header"),
        );
    }
    if let Some(session_affinity) = session_affinity {
        headers.insert(
            "x-session-affinity",
            HeaderValue::from_str(session_affinity).expect("header"),
        );
    }
    if let Some(session_id) = session_id {
        headers.insert(
            "session_id",
            HeaderValue::from_str(session_id).expect("header"),
        );
    }
    if let Some(subagent) = subagent {
        headers.insert(
            "x-openai-subagent",
            HeaderValue::from_str(subagent).expect("header"),
        );
    }
    super::super::super::IncomingHeaderSnapshot::from_http_headers(&headers)
}

#[test]
fn preferred_client_prompt_cache_key_is_used_without_native_anchor() {
    let incoming_headers = sample_incoming_headers(None, None, None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread"));
    let client_request_meta = sample_request_metadata(Some("client_thread"));

    let actual = resolve_preferred_client_prompt_cache_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert_eq!(actual.as_deref(), Some("client_thread"));
}

#[test]
fn preferred_client_prompt_cache_key_is_ignored_when_conversation_anchor_exists() {
    let incoming_headers = sample_incoming_headers(Some("conv_anchor"), None, None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread"));
    let client_request_meta = sample_request_metadata(Some("client_thread"));

    let actual = resolve_preferred_client_prompt_cache_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert_eq!(actual, None);
}

#[test]
fn preferred_client_prompt_cache_key_is_used_when_turn_state_is_orphaned() {
    let incoming_headers =
        sample_incoming_headers(None, Some("turn_state_anchor"), None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread"));
    let client_request_meta = sample_request_metadata(Some("client_thread"));

    let actual = resolve_preferred_client_prompt_cache_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert_eq!(actual.as_deref(), Some("client_thread"));
}

#[test]
fn preferred_client_prompt_cache_key_is_ignored_when_turn_state_has_session_anchor() {
    let incoming_headers = sample_incoming_headers_with_session_id(
        None,
        Some("turn_state_anchor"),
        None,
        None,
        None,
        Some("session_anchor"),
        None,
    );
    let initial_request_meta = sample_request_metadata(Some("client_thread"));
    let client_request_meta = sample_request_metadata(Some("client_thread"));

    let actual = resolve_preferred_client_prompt_cache_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert_eq!(actual, None);
}

#[test]
fn preferred_client_prompt_cache_key_is_ignored_even_when_matching_native_anchor() {
    let incoming_headers = sample_incoming_headers(Some("shared_anchor"), None, None, None, None);
    let initial_request_meta = sample_request_metadata(Some("shared_anchor"));
    let client_request_meta = sample_request_metadata(Some("shared_anchor"));

    let actual = resolve_preferred_client_prompt_cache_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert_eq!(actual, None);
}

#[test]
fn preferred_client_prompt_cache_key_is_disabled_for_anthropic_native_requests() {
    let incoming_headers = sample_incoming_headers(None, None, None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread"));
    let client_request_meta = sample_request_metadata(Some("client_thread"));

    let actual = resolve_preferred_client_prompt_cache_key(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert_eq!(actual, None);
}

#[test]
fn route_conversation_id_uses_prompt_cache_key_without_native_anchor() {
    let incoming_headers = sample_incoming_headers(None, None, None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread_123456"));
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    )
    .expect("route id");

    assert_eq!(actual.source, RouteConversationSource::PromptCacheKey);
    assert!(actual.id.starts_with("pck:v1:"));
    assert!(!actual.id.contains("client_thread_123456"));
}

#[test]
fn route_conversation_id_uses_prompt_cache_key_when_turn_state_is_orphaned() {
    let incoming_headers =
        sample_incoming_headers(None, Some("turn_state_anchor"), None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread_123456"));
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    )
    .expect("route id");

    assert_eq!(actual.source, RouteConversationSource::PromptCacheKey);
    assert!(actual.id.starts_with("pck:v1:"));
    assert!(!actual.id.contains("client_thread_123456"));
}

#[test]
fn route_conversation_id_does_not_use_prompt_cache_key_when_turn_state_has_session_anchor() {
    let incoming_headers = sample_incoming_headers_with_session_id(
        None,
        Some("turn_state_anchor"),
        None,
        None,
        None,
        Some("session_anchor"),
        None,
    );
    let initial_request_meta = sample_request_metadata(Some("client_thread_123456"));
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert!(actual.is_none());
}

#[test]
fn route_conversation_id_prefers_native_conversation_over_prompt_cache_key() {
    let incoming_headers = sample_incoming_headers(Some("native-conv"), None, None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread_123456"));
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    )
    .expect("route id");

    assert_eq!(actual.source, RouteConversationSource::NativeConversation);
    assert_eq!(actual.id, "native-conv");
}

#[test]
fn route_conversation_id_prefers_native_conversation_when_turn_state_also_exists() {
    let incoming_headers = sample_incoming_headers(
        Some("native-conv"),
        Some("turn_state_anchor"),
        None,
        None,
        None,
    );
    let initial_request_meta = sample_request_metadata(Some("client_thread_123456"));
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    )
    .expect("route id");

    assert_eq!(actual.source, RouteConversationSource::NativeConversation);
    assert_eq!(actual.id, "native-conv");
}

#[test]
fn existing_only_prompt_cache_binding_is_not_used_as_fallback_thread_anchor() {
    let binding = codexmanager_core::storage::ConversationBinding {
        platform_key_hash: "key-hash-1".to_string(),
        conversation_id: "pck:v1:abcdef".to_string(),
        account_id: "acc-1".to_string(),
        thread_epoch: 1,
        thread_anchor: "pck:v1:abcdef".to_string(),
        status: "active".to_string(),
        last_model: Some("gpt-5.5".to_string()),
        last_switch_reason: None,
        created_at: 1,
        updated_at: 1,
        last_used_at: 1,
    };

    let actual = conversation_binding_for_thread_anchor(
        Some(RouteConversationSource::PromptCacheKeyExistingOnly),
        Some(&binding),
    );

    assert!(actual.is_none());
}

#[test]
fn route_conversation_id_uses_existing_only_prompt_cache_key_when_previous_response_id_exists() {
    let incoming_headers = sample_incoming_headers(None, None, None, None, None);
    let initial_request_meta = sample_request_metadata_with_previous_response(
        Some("client_thread_123456"),
        Some("resp_previous"),
    );
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    )
    .expect("route id");

    assert_eq!(
        actual.source,
        RouteConversationSource::PromptCacheKeyExistingOnly
    );
    assert!(actual.id.starts_with("pck:v1:"));
    assert!(!actual.id.contains("client_thread_123456"));
}

#[test]
fn route_conversation_id_uses_existing_only_prompt_cache_key_when_turn_state_is_orphaned() {
    let incoming_headers =
        sample_incoming_headers(None, Some("turn_state_anchor"), None, None, None);
    let initial_request_meta = sample_request_metadata_with_previous_response(
        Some("client_thread_123456"),
        Some("resp_previous"),
    );
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    )
    .expect("route id");

    assert_eq!(
        actual.source,
        RouteConversationSource::PromptCacheKeyExistingOnly
    );
    assert!(actual.id.starts_with("pck:v1:"));
    assert!(!actual.id.contains("client_thread_123456"));
}

#[test]
fn route_conversation_id_turn_state_only_without_prompt_cache_key_does_not_use_sticky_fallback() {
    let incoming_headers =
        sample_incoming_headers(None, Some("turn_state_anchor"), None, None, None);
    let initial_request_meta = sample_request_metadata(None);
    let client_request_meta = sample_request_metadata(None);

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert!(actual.is_none());
}

#[test]
fn route_conversation_id_does_not_use_prompt_cache_key_for_non_responses_path_prefix() {
    let incoming_headers = sample_incoming_headers(None, None, None, None, None);
    let initial_request_meta = sample_request_metadata(Some("client_thread_123456"));
    let client_request_meta = sample_request_metadata(Some("client_thread_123456"));

    let actual = resolve_route_conversation_id(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responsesxxx",
        "platform-key-hash",
        &incoming_headers,
        &initial_request_meta,
        &client_request_meta,
    );

    assert!(actual.is_none());
}

#[test]
fn prompt_cache_route_id_is_not_split_by_model() {
    let first = prompt_cache_route_id(
        "platform-key-hash",
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "client_thread_123456",
    );
    let second = prompt_cache_route_id(
        "platform-key-hash",
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "client_thread_123456",
    );

    assert_eq!(first, second);
}

/// 函数 `aggregate_passthrough_applies_model_reasoning_and_service_tier_overrides_without_forcing_log_tier`
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
#[test]
fn aggregate_passthrough_applies_model_reasoning_and_service_tier_overrides_without_forcing_log_tier(
) {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        Some("gpt-5.4"),
        Some("high"),
        Some("fast"),
    );
    let body = br#"{"model":"gpt-4.1","input":"hi","reasoning":{"effort":"low"}}"#.to_vec();

    let (
        rewritten_body,
        model_for_log,
        reasoning_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        _has_prompt_cache_key,
        _request_shape,
    ) = apply_passthrough_request_overrides("/v1/responses", body, &api_key, None, None);
    let payload: Value = serde_json::from_slice(&rewritten_body).expect("json body");

    assert_eq!(
        payload.get("model").and_then(Value::as_str),
        Some("gpt-5.4")
    );
    assert_eq!(
        payload
            .get("reasoning")
            .and_then(Value::as_object)
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
    assert_eq!(model_for_log.as_deref(), Some("gpt-5.4"));
    assert_eq!(reasoning_for_log.as_deref(), Some("high"));
    assert_eq!(service_tier_for_log, None);
    assert_eq!(effective_service_tier_for_log.as_deref(), Some("fast"));
}

#[test]
fn aggregate_passthrough_openai_responses_defaults_omitted_stream_to_json() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        None,
        None,
        None,
    );
    let body = br#"{"model":"gpt-5.4","input":"hi"}"#.to_vec();

    let (rewritten_body, ..) =
        apply_passthrough_request_overrides("/v1/responses", body, &api_key, None, None);
    let defaulted_body = default_omitted_responses_stream_to_false(rewritten_body);
    let payload: Value = serde_json::from_slice(&defaulted_body).expect("json body");
    let is_stream = resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        false,
        false,
        false,
    );

    // The upstream transport may still be forced to SSE for canonical
    // aggregation, while the omitted client flag must produce JSON downstream.
    assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(true));
    assert!(!is_stream);
}

#[test]
fn hybrid_passthrough_fallback_body_uses_aggregate_override_shape() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        Some("gpt-5.4"),
        Some("high"),
        Some("fast"),
    );
    let body = br#"{"model":"gpt-4.1","input":"hi","reasoning":{"effort":"low"}}"#.to_vec();

    let mut passthrough_body =
        apply_passthrough_request_overrides("/v1/responses", body, &api_key, None, None).0;
    passthrough_body = default_omitted_responses_stream_to_false(passthrough_body);
    let payload: Value = serde_json::from_slice(&passthrough_body).expect("json body");

    assert_eq!(
        payload.get("model").and_then(Value::as_str),
        Some("gpt-5.4")
    );
    assert_eq!(
        payload
            .get("reasoning")
            .and_then(Value::as_object)
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
    assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(true));
}

#[test]
fn native_codex_client_detection_uses_codex_signals_instead_of_client_brand() {
    let native_headers = sample_incoming_headers(
        None,
        None,
        Some("codex_exec/0.999.0"),
        Some("codex_exec"),
        Some("affinity-1"),
    );
    assert!(is_native_codex_client_request(&native_headers));

    let plain_opencode_headers = sample_incoming_headers(
        None,
        None,
        Some("opencode/0.1.0"),
        Some("opencode"),
        Some("affinity-1"),
    );
    assert!(!is_native_codex_client_request(&plain_opencode_headers));

    let opencode_with_codex_signals = sample_incoming_headers(
        None,
        Some("turn-state-1"),
        Some("opencode/0.1.0"),
        Some("opencode"),
        Some("affinity-1"),
    );
    assert!(is_native_codex_client_request(&opencode_with_codex_signals));
}

#[test]
fn openai_responses_api_clients_use_codex_compat_rewrite_but_native_codex_does_not() {
    assert!(allow_codex_compat_rewrite_for_client(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        false,
    ));
    assert!(allow_codex_compat_rewrite_for_client(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/chat/completions",
        false,
    ));
    assert!(!allow_codex_compat_rewrite_for_client(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        true,
    ));
    assert!(!allow_codex_compat_rewrite_for_client(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/chat/completions",
        true,
    ));
}

#[test]
fn openai_chat_completions_api_body_is_adapted_to_responses_for_codex_backend() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "stream": true,
        "messages": [{ "role": "user", "content": "你好" }],
        "tools": [{
            "type": "function",
            "function": {
                "name": "ping",
                "description": "Ping",
                "parameters": { "type": "object", "properties": {} }
            }
        }],
        "tool_choice": { "type": "function", "function": { "name": "ping" } }
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");

    assert_eq!(
        payload.get("model").and_then(Value::as_str),
        Some("gpt-5.5")
    );
    assert_eq!(
        payload
            .get("input")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("content"))
            .and_then(Value::as_array)
            .and_then(|parts| parts.first())
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str),
        Some("你好")
    );
    assert_eq!(
        payload
            .get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| tools.first())
            .and_then(|tool| tool.get("name"))
            .and_then(Value::as_str),
        Some("ping")
    );
    assert_eq!(
        payload
            .get("tool_choice")
            .and_then(|choice| choice.get("name"))
            .and_then(Value::as_str),
        Some("ping")
    );
    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str),
        Some("medium")
    );
    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("summary"))
            .and_then(Value::as_str),
        Some("auto")
    );
}

#[test]
fn openai_chat_completions_prompt_cache_key_is_reinjected_after_responses_adaptation() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "你好" }],
        "prompt_cache_key": "chat-pck-123456"
    });
    let initial_meta =
        super::super::super::parse_request_metadata(&serde_json::to_vec(&body).expect("body"));
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let rewritten = apply_request_overrides_with_service_tier_and_prompt_cache_key_scope(
        "/v1/responses",
        adapted,
        None,
        None,
        None,
        Some("https://chatgpt.com/backend-api/codex"),
        initial_meta.prompt_cache_key.as_deref(),
        true,
    );
    let payload: Value = serde_json::from_slice(&rewritten).expect("json body");

    assert_eq!(
        payload.get("prompt_cache_key").and_then(Value::as_str),
        Some("chat-pck-123456")
    );
}

#[test]
fn openai_chat_completions_reasoning_effort_adds_summary_for_responses() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "你好" }],
        "reasoning_effort": "high"
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");

    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("summary"))
            .and_then(Value::as_str),
        Some("auto")
    );
}

#[test]
fn openai_chat_completions_reasoning_object_preserves_fields_and_adds_missing_summary() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "你好" }],
        "reasoning": {
            "effort": "low"
        }
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");

    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str),
        Some("low")
    );
    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("summary"))
            .and_then(Value::as_str),
        Some("auto")
    );
}

#[test]
fn openai_chat_completions_reasoning_object_keeps_existing_summary() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "你好" }],
        "reasoning": {
            "effort": "low",
            "summary": "detailed"
        }
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");

    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("effort"))
            .and_then(Value::as_str),
        Some("low")
    );
    assert_eq!(
        payload
            .get("reasoning")
            .and_then(|reasoning| reasoning.get("summary"))
            .and_then(Value::as_str),
        Some("detailed")
    );
}

#[test]
fn openai_chat_completions_response_format_json_object_adapts_to_responses_text_format() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "return json" }],
        "response_format": { "type": "json_object" }
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");

    assert_eq!(
        payload
            .get("text")
            .and_then(|text| text.get("format"))
            .and_then(|format| format.get("type"))
            .and_then(Value::as_str),
        Some("json_object")
    );
}

#[test]
fn openai_chat_completions_response_format_json_schema_adapts_to_responses_text_format() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "return json" }],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "answer_schema",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "answer": { "type": "string" }
                    },
                    "required": ["answer"],
                    "additionalProperties": false
                }
            }
        }
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");
    let format = payload
        .get("text")
        .and_then(|text| text.get("format"))
        .expect("text format");

    assert_eq!(
        format.get("type").and_then(Value::as_str),
        Some("json_schema")
    );
    assert_eq!(
        format.get("name").and_then(Value::as_str),
        Some("answer_schema")
    );
    assert_eq!(format.get("strict").and_then(Value::as_bool), Some(true));
    assert_eq!(
        format
            .get("schema")
            .and_then(|schema| schema.get("required"))
            .and_then(Value::as_array)
            .and_then(|required| required.first())
            .and_then(Value::as_str),
        Some("answer")
    );
}

#[test]
fn openai_chat_completions_response_format_preserves_existing_text_fields() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "return json" }],
        "text": {
            "verbosity": "low",
            "format": { "type": "text" }
        },
        "response_format": { "type": "json_object" }
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");

    assert_eq!(
        payload
            .get("text")
            .and_then(|text| text.get("verbosity"))
            .and_then(Value::as_str),
        Some("low")
    );
    assert_eq!(
        payload
            .get("text")
            .and_then(|text| text.get("format"))
            .and_then(|format| format.get("type"))
            .and_then(Value::as_str),
        Some("json_object")
    );
}

#[test]
fn openai_chat_completions_ignores_non_object_text_without_response_format() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "hello" }],
        "text": "legacy-client-noise"
    });
    let adapted = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect("adapt chat body");
    let payload: Value = serde_json::from_slice(&adapted).expect("json body");

    assert!(payload.get("text").is_none());
}

#[test]
fn openai_chat_completions_response_format_rejects_non_object_text() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "return json" }],
        "text": "legacy-client-noise",
        "response_format": { "type": "json_object" }
    });
    let err = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect_err("response_format with non-object text should fail");

    assert!(err.contains("text must be an object"));
}

#[test]
fn openai_chat_completions_response_format_rejects_invalid_values() {
    let body = serde_json::json!({
        "model": "gpt-5.5",
        "messages": [{ "role": "user", "content": "return json" }],
        "response_format": { "type": "xml" }
    });
    let err = adapt_openai_chat_completions_body_to_responses(
        serde_json::to_vec(&body).expect("serialize chat body"),
    )
    .expect_err("unsupported response_format should fail");

    assert!(err.contains("unsupported response_format.type"));
}

#[test]
fn opencode_headers_with_only_session_id_are_not_treated_as_native_codex_clients() {
    let opencode_headers = sample_incoming_headers_with_session_id(
        None,
        None,
        Some("opencode/0.1.0"),
        Some("opencode"),
        Some("affinity-1"),
        Some("session-1"),
        None,
    );
    assert!(!is_native_codex_client_request(&opencode_headers));
}

#[test]
fn compact_subagent_marks_standard_responses_request_as_compact() {
    let headers = sample_incoming_headers_with_session_id(
        None,
        None,
        Some("codex_cli_rs/0.1.0"),
        Some("codex-cli"),
        None,
        Some("session-compact"),
        Some("compact"),
    );

    assert!(is_compact_subagent_request("/v1/responses", &headers));
    assert!(!is_compact_subagent_request(
        "/v1/chat/completions",
        &headers
    ));
}

#[test]
fn compact_subagent_rewrites_standard_responses_path_to_compact_path() {
    let headers = sample_incoming_headers_with_session_id(
        None,
        None,
        Some("codex_cli_rs/0.1.0"),
        Some("codex-cli"),
        None,
        Some("session-compact"),
        Some("compact"),
    );

    assert_eq!(
        resolve_logical_gateway_request_path("/v1/responses", &headers),
        "/v1/responses/compact"
    );
}

#[test]
fn compact_subagent_ignores_hidden_compact_model_forward_rules() {
    let headers = sample_incoming_headers_with_session_id(
        None,
        None,
        Some("codex_cli_rs/0.1.0"),
        Some("codex-cli"),
        None,
        Some("session-compact"),
        Some("compact"),
    );

    assert_eq!(
        resolve_compact_model_override_for_request("/v1/responses", &headers, Some("gpt-5.4"),)
            .as_deref(),
        None
    );
}

#[test]
fn compact_request_uses_chat_completions_response_adapter_when_configured() {
    let _guard = crate::test_env_guard();
    let _compact_api_path = RuntimeEnvGuard::set(COMPACT_API_PATH_ENV, "/v1/chat/completions");

    assert_eq!(
        maybe_wrap_compact_response_adapter(
            "/v1/responses/compact",
            crate::gateway::ResponseAdapter::Passthrough,
        ),
        crate::gateway::ResponseAdapter::CompactFromChatCompletions
    );
}

#[test]
fn gemini_stream_generate_content_path_forces_stream_mode_without_body_flag() {
    assert!(resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:streamGenerateContent",
        false,
        false,
        false,
    ));
    assert!(resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_GEMINI_NATIVE,
        "/v1internal:streamGenerateContent",
        false,
        false,
        false,
    ));
    assert!(!resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_GEMINI_NATIVE,
        "/v1beta/models/gemini-2.5-pro:generateContent",
        false,
        false,
        false,
    ));
}

#[test]
fn openai_responses_api_defaults_to_json_when_stream_is_omitted() {
    assert!(!resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        false,
        false,
        false,
    ));
    assert!(resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        true,
        true,
        false,
    ));
    assert!(!resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        false,
        true,
        false,
    ));
    assert!(!resolve_client_is_stream(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        "/v1/responses",
        false,
        false,
        true,
    ));
}

#[test]
fn openai_responses_api_body_defaults_omitted_stream_to_false_before_rewrite() {
    let body = br#"{"model":"gpt-5.4","input":"hi"}"#.to_vec();
    let rewritten = default_omitted_responses_stream_to_false(body);
    let payload: Value = serde_json::from_slice(&rewritten).expect("json body");

    assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(false));
}

#[test]
fn openai_responses_api_body_preserves_explicit_stream_false() {
    let body = br#"{"model":"gpt-5.4","input":"hi","stream":false}"#.to_vec();
    let rewritten = default_omitted_responses_stream_to_false(body);
    let payload: Value = serde_json::from_slice(&rewritten).expect("json body");

    assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(false));
}

#[test]
fn aggregate_passthrough_preserves_fast_service_tier_for_log_when_request_is_rewritten() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        Some("gpt-5.4"),
        Some("high"),
        None,
    );
    let body =
        br#"{"model":"gpt-4.1","input":"hi","reasoning":{"effort":"low"},"service_tier":"Fast"}"#
            .to_vec();

    let (
        rewritten_body,
        model_for_log,
        reasoning_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        _has_prompt_cache_key,
        _request_shape,
    ) = apply_passthrough_request_overrides(
        "/v1/responses",
        body,
        &api_key,
        Some("fast".to_string()),
        None,
    );
    let payload: Value = serde_json::from_slice(&rewritten_body).expect("json body");

    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
    assert_eq!(model_for_log.as_deref(), Some("gpt-5.4"));
    assert_eq!(reasoning_for_log.as_deref(), Some("high"));
    assert_eq!(service_tier_for_log.as_deref(), Some("fast"));
    assert_eq!(effective_service_tier_for_log.as_deref(), Some("fast"));
}

#[test]
fn codex_backend_passthrough_maps_fast_to_priority_but_keeps_fast_for_log() {
    let mut api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        Some("gpt-5.4"),
        Some("high"),
        Some("fast"),
    );
    api_key.upstream_base_url = Some("https://chatgpt.com/backend-api/codex".to_string());
    let body = br#"{"model":"gpt-4.1","input":"hi","reasoning":{"effort":"low"}}"#.to_vec();

    let (
        rewritten_body,
        model_for_log,
        reasoning_for_log,
        service_tier_for_log,
        effective_service_tier_for_log,
        _has_prompt_cache_key,
        _request_shape,
    ) = apply_passthrough_request_overrides("/v1/responses", body, &api_key, None, None);
    let payload: Value = serde_json::from_slice(&rewritten_body).expect("json body");
    let request_meta = crate::gateway::parse_request_metadata(&rewritten_body);

    assert_eq!(
        payload.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );
    assert_eq!(request_meta.service_tier.as_deref(), Some("fast"));
    assert_eq!(model_for_log.as_deref(), Some("gpt-5.4"));
    assert_eq!(reasoning_for_log.as_deref(), Some("high"));
    assert_eq!(service_tier_for_log, None);
    assert_eq!(effective_service_tier_for_log.as_deref(), Some("fast"));
}

/// 函数 `anthropic_model_must_exist_in_cached_model_options`
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
#[test]
fn anthropic_model_must_exist_in_cached_model_options() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    crate::apikey_models::save_model_options_with_storage(
        &storage,
        &ModelsResponse {
            models: vec![
                ModelInfo {
                    slug: "claude-sonnet-4".to_string(),
                    display_name: "claude-sonnet-4".to_string(),
                    ..Default::default()
                },
                ModelInfo {
                    slug: "gpt-5.4-mini".to_string(),
                    display_name: "gpt-5.4-mini".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    )
    .expect("save model catalog");

    assert!(ensure_anthropic_model_is_listed(
        &storage,
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        Some("claude-sonnet-4")
    )
    .is_ok());
    let err = ensure_anthropic_model_is_listed(
        &storage,
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        Some("claude-sonnet-4-5"),
    )
    .expect_err("missing model should fail");
    assert!(err.message.contains("claude model not found in model list"));
}
