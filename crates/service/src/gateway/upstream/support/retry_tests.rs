use super::{should_skip_codex_v1_alt_for_api_client, UpstreamRequestContext};
use crate::gateway::IncomingHeaderSnapshot;
use axum::http::{HeaderMap, HeaderValue};

fn headers(user_agent: Option<&str>, originator: Option<&str>) -> IncomingHeaderSnapshot {
    let mut headers = HeaderMap::new();
    if let Some(user_agent) = user_agent {
        headers.insert(
            "user-agent",
            HeaderValue::from_str(user_agent).expect("user agent header"),
        );
    }
    if let Some(originator) = originator {
        headers.insert(
            "originator",
            HeaderValue::from_str(originator).expect("originator header"),
        );
    }
    IncomingHeaderSnapshot::from_http_headers(&headers)
}

#[test]
fn api_client_responses_request_skips_codex_v1_alt_retry() {
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        force_fresh_transport: false,
    };
    let incoming_headers = headers(Some("CherryStudio/1.0"), None);

    assert!(should_skip_codex_v1_alt_for_api_client(
        request_ctx,
        &incoming_headers,
        "https://chatgpt.com/backend-api/codex/v1/responses"
    ));
}

#[test]
fn native_codex_responses_request_keeps_codex_v1_alt_retry_available() {
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        force_fresh_transport: false,
    };
    let incoming_headers = headers(Some("codex-cli/0.1.0"), Some("codex_cli_rs"));

    assert!(!should_skip_codex_v1_alt_for_api_client(
        request_ctx,
        &incoming_headers,
        "https://chatgpt.com/backend-api/codex/v1/responses"
    ));
}
