use super::*;

fn sample_api_key(
    protocol_type: &str,
    model_slug: Option<&str>,
    reasoning: Option<&str>,
) -> ApiKey {
    ApiKey {
        id: "gk_test".to_string(),
        name: Some("test".to_string()),
        model_slug: model_slug.map(|value| value.to_string()),
        reasoning_effort: reasoning.map(|value| value.to_string()),
        client_type: "codex".to_string(),
        protocol_type: protocol_type.to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: None,
        static_headers_json: None,
        key_hash: "hash".to_string(),
        status: "active".to_string(),
        created_at: 0,
        last_used_at: None,
    }
}

#[test]
fn anthropic_key_keeps_empty_overrides() {
    let api_key = sample_api_key(crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE, None, None);
    let (model, reasoning) = resolve_effective_request_overrides(&api_key);
    assert_eq!(model, None);
    assert_eq!(reasoning, None);
}

#[test]
fn anthropic_key_applies_custom_model_and_reasoning() {
    let api_key = sample_api_key(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        Some("gpt-5.3-codex"),
        Some("extra_high"),
    );
    let (model, reasoning) = resolve_effective_request_overrides(&api_key);
    assert_eq!(model.as_deref(), Some("gpt-5.3-codex"));
    assert_eq!(reasoning.as_deref(), Some("xhigh"));
}

#[test]
fn openai_key_keeps_empty_overrides() {
    let api_key = sample_api_key("openai_compat", None, None);
    let (model, reasoning) = resolve_effective_request_overrides(&api_key);
    assert_eq!(model, None);
    assert_eq!(reasoning, None);
}

#[test]
fn gateway_protocol_is_restricted_to_openai_and_anthropic() {
    assert!(is_supported_gateway_protocol("openai_compat"));
    assert!(is_supported_gateway_protocol(
        crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE
    ));
    assert!(!is_supported_gateway_protocol(
        crate::apikey_profile::PROTOCOL_AZURE_OPENAI
    ));
}

#[test]
fn gateway_path_is_restricted_to_chat_completions_and_responses() {
    assert!(is_supported_gateway_path("/v1/chat/completions"));
    assert!(is_supported_gateway_path("/v1/chat/completions/"));
    assert!(is_supported_gateway_path("/v1/chat/completions?stream=true"));
    assert!(is_supported_gateway_path("/v1/responses"));
    assert!(is_supported_gateway_path("/v1/responses/"));
    assert!(is_supported_gateway_path("/v1/responses?stream=true"));

    assert!(!is_supported_gateway_path("/v1/models"));
    assert!(!is_supported_gateway_path("/v1/messages"));
    assert!(!is_supported_gateway_path("/v1/embeddings"));
    assert!(!is_supported_gateway_path("/v1/chat/completions-extra"));
    assert!(!is_supported_gateway_path("/v1/responses123"));
}
