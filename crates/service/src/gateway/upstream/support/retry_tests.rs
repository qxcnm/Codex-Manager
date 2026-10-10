use super::{should_skip_codex_v1_alt_for_responses, UpstreamRequestContext};

fn request_ctx(path: &'static str, protocol_type: &'static str) -> UpstreamRequestContext<'static> {
    UpstreamRequestContext {
        request_path: path,
        protocol_type,
        is_fedramp: false,
        capture: None,
    }
}

#[test]
fn api_client_responses_request_skips_codex_v1_alt_retry() {
    assert!(should_skip_codex_v1_alt_for_responses(
        request_ctx(
            "/v1/responses",
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        ),
        "https://chatgpt.com/backend-api/codex/v1/responses"
    ));
}

#[test]
fn native_codex_responses_request_skips_codex_v1_alt_retry() {
    assert!(should_skip_codex_v1_alt_for_responses(
        request_ctx(
            "/v1/responses",
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        ),
        "https://chatgpt.com/backend-api/codex/v1/responses"
    ));
}

#[test]
fn non_responses_request_keeps_alternate_path_available() {
    assert!(!should_skip_codex_v1_alt_for_responses(
        request_ctx(
            "/v1/chat/completions",
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        ),
        "https://chatgpt.com/backend-api/codex/v1/chat/completions"
    ));
}

#[test]
fn compact_responses_mapped_to_chat_completions_keeps_alternate_path_available() {
    assert!(!should_skip_codex_v1_alt_for_responses(
        request_ctx(
            "/v1/responses/compact",
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        ),
        "https://chatgpt.com/backend-api/codex/v1/chat/completions"
    ));
}

#[test]
fn anthropic_responses_bridge_keeps_alternate_path_available() {
    assert!(!should_skip_codex_v1_alt_for_responses(
        request_ctx(
            "/v1/responses",
            crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
        ),
        "https://chatgpt.com/backend-api/codex/v1/responses"
    ));
}

#[test]
fn openai_responses_trailing_slash_skips_legacy_alternate_path() {
    assert!(should_skip_codex_v1_alt_for_responses(
        request_ctx(
            "/v1/responses/",
            crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        ),
        "https://chatgpt.com/backend-api/codex/v1/responses/"
    ));
}
