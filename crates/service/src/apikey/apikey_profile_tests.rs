use super::{
    is_anthropic_request_path, is_gemini_count_tokens_request_path,
    is_gemini_generate_content_request_path, normalize_protocol_type, normalize_rotation_strategy,
    resolve_gateway_protocol_type, PROTOCOL_ANTHROPIC_NATIVE, PROTOCOL_GEMINI_NATIVE,
    PROTOCOL_OPENAI_COMPAT, ROTATION_ACCOUNT, ROTATION_AGGREGATE_API, ROTATION_HYBRID,
    ROTATION_HYBRID_AGGREGATE_FIRST,
};

#[test]
fn normalize_rotation_strategy_accepts_hybrid_aliases() {
    for value in [
        "hybrid_rotation",
        "hybrid",
        "mixed",
        "mixed-rotation",
        "混合轮转",
        "账号优先聚合兜底",
    ] {
        assert_eq!(
            normalize_rotation_strategy(Some(value.to_string())).as_deref(),
            Ok(ROTATION_HYBRID)
        );
    }
}

#[test]
fn normalize_rotation_strategy_accepts_hybrid_aggregate_first_aliases() {
    for value in [
        "hybrid_aggregate_first_rotation",
        "hybrid_aggregate_first",
        "mixed_aggregate_first",
        "aggregate_first",
        "聚合优先",
        "聚合优先账号兜底",
    ] {
        assert_eq!(
            normalize_rotation_strategy(Some(value.to_string())).as_deref(),
            Ok(ROTATION_HYBRID_AGGREGATE_FIRST)
        );
    }
}

#[test]
fn normalize_rotation_strategy_keeps_existing_values() {
    assert_eq!(
        normalize_rotation_strategy(None).as_deref(),
        Ok(ROTATION_ACCOUNT)
    );
    assert_eq!(
        normalize_rotation_strategy(Some("aggregate_api_rotation".to_string())).as_deref(),
        Ok(ROTATION_AGGREGATE_API)
    );
}

#[test]
fn wildcard_protocol_routes_messages_path_to_anthropic() {
    assert!(is_anthropic_request_path("/v1/messages"));
    assert_eq!(
        resolve_gateway_protocol_type(PROTOCOL_OPENAI_COMPAT, "/v1/messages"),
        PROTOCOL_ANTHROPIC_NATIVE
    );
}

#[test]
fn wildcard_protocol_routes_responses_path_to_openai() {
    assert_eq!(
        resolve_gateway_protocol_type(PROTOCOL_ANTHROPIC_NATIVE, "/v1/responses"),
        PROTOCOL_OPENAI_COMPAT
    );
}

#[test]
fn wildcard_protocol_routes_gemini_generate_content_path_to_gemini() {
    assert!(is_gemini_generate_content_request_path(
        "/v1beta/models/gemini-2.5-pro:generateContent"
    ));
    assert_eq!(
        resolve_gateway_protocol_type(
            PROTOCOL_OPENAI_COMPAT,
            "/v1beta/models/gemini-2.5-pro:generateContent"
        ),
        PROTOCOL_GEMINI_NATIVE
    );
}

#[test]
fn wildcard_protocol_routes_gemini_count_tokens_path_to_gemini() {
    assert!(is_gemini_count_tokens_request_path(
        "/v1beta/models/gemini-2.5-pro:countTokens?alt=json"
    ));
    assert_eq!(
        resolve_gateway_protocol_type(
            PROTOCOL_OPENAI_COMPAT,
            "/v1beta/models/gemini-2.5-pro:countTokens?alt=json"
        ),
        PROTOCOL_GEMINI_NATIVE
    );
}

#[test]
fn wildcard_protocol_routes_gemini_cli_internal_generate_content_path_to_gemini() {
    assert!(is_gemini_generate_content_request_path(
        "/v1internal:streamGenerateContent?alt=sse"
    ));
    assert_eq!(
        resolve_gateway_protocol_type(
            PROTOCOL_OPENAI_COMPAT,
            "/v1internal:streamGenerateContent?alt=sse"
        ),
        PROTOCOL_GEMINI_NATIVE
    );
}

#[test]
fn wildcard_protocol_routes_gemini_cli_internal_count_tokens_path_to_gemini() {
    assert!(is_gemini_count_tokens_request_path(
        "/v1internal:countTokens"
    ));
    assert_eq!(
        resolve_gateway_protocol_type(PROTOCOL_OPENAI_COMPAT, "/v1internal:countTokens"),
        PROTOCOL_GEMINI_NATIVE
    );
}

#[test]
fn removed_azure_protocol_falls_back_to_wildcard_routing() {
    assert_eq!(
        resolve_gateway_protocol_type("azure_openai", "/v1/messages"),
        PROTOCOL_ANTHROPIC_NATIVE
    );
    assert_eq!(
        resolve_gateway_protocol_type("azure_openai", "/v1/responses"),
        PROTOCOL_OPENAI_COMPAT
    );
}

#[test]
fn removed_azure_protocol_is_rejected_for_profile_configuration() {
    let err = normalize_protocol_type(Some("azure_openai".to_string()))
        .expect_err("azure profile protocol should be rejected");
    assert!(err.contains("unsupported protocol type: azure_openai"));
}
