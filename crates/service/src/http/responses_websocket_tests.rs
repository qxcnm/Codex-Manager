use super::{
    build_socks5_connect_request, build_upstream_websocket_request, infer_ws_terminal_status,
    inspect_ws_terminal_event, is_previous_response_not_found_terminal, merge_client_metadata,
    parse_websocket_target, parse_ws_usage, proxy_basic_auth_header, rewrite_client_frame,
    strip_previous_response_id_from_ws_text, WsRequestContext,
};
use axum::http::{HeaderMap, HeaderValue};
use codexmanager_core::storage::{Account, ApiKey};
use serde_json::json;

fn sample_api_key() -> ApiKey {
    ApiKey {
        id: "gk_test".to_string(),
        name: Some("test".to_string()),
        model_slug: None,
        reasoning_effort: None,
        service_tier: None,
        client_type: "codex".to_string(),
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT.to_string(),
        auth_scheme: "authorization_bearer".to_string(),
        upstream_base_url: Some("https://chatgpt.com/backend-api/codex".to_string()),
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

fn sample_account() -> Account {
    Account {
        id: "acc-test".to_string(),
        label: "test".to_string(),
        issuer: "".to_string(),
        chatgpt_account_id: Some("workspace-test".to_string()),
        workspace_id: None,
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: 0,
        updated_at: 0,
    }
}

fn sample_incoming_headers(
    conversation_id: Option<&str>,
    turn_state: Option<&str>,
) -> crate::gateway::IncomingHeaderSnapshot {
    let mut headers = HeaderMap::new();
    if let Some(conversation_id) = conversation_id {
        headers.insert(
            "conversation_id",
            HeaderValue::from_str(conversation_id).expect("conversation header"),
        );
    }
    if let Some(turn_state) = turn_state {
        headers.insert(
            "x-codex-turn-state",
            HeaderValue::from_str(turn_state).expect("turn-state header"),
        );
    }
    crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers)
}

fn sample_incoming_headers_with_metadata() -> crate::gateway::IncomingHeaderSnapshot {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-codex-turn-metadata",
        HeaderValue::from_static("turn-meta-1"),
    );
    headers.insert("x-codex-window-id", HeaderValue::from_static("window-1:0"));
    headers.insert("x-openai-subagent", HeaderValue::from_static("review"));
    headers.insert(
        "x-codex-parent-thread-id",
        HeaderValue::from_static("parent-thread-1"),
    );
    crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers)
}

#[test]
fn websocket_target_authority_brackets_ipv6_host() {
    let target = parse_websocket_target("wss://[::1]/backend-api/codex/v1/responses")
        .expect("parse websocket target");

    assert_eq!(target.host, "::1");
    assert_eq!(target.port, 443);
    assert_eq!(target.authority, "[::1]:443");
}

#[test]
fn socks5_connect_request_uses_domain_target() {
    let target = parse_websocket_target("wss://chatgpt.com/backend-api/codex/v1/responses")
        .expect("parse websocket target");
    let request = build_socks5_connect_request(&target).expect("build socks request");

    assert_eq!(
        request,
        vec![
            0x05, 0x01, 0x00, 0x03, 11, b'c', b'h', b'a', b't', b'g', b'p', b't', b'.', b'c', b'o',
            b'm', 0x01, 0xbb
        ]
    );
}

#[test]
fn proxy_basic_auth_header_encodes_credentials() {
    let proxy = url::Url::parse("http://user:pass@127.0.0.1:7890").expect("parse proxy");

    assert_eq!(
        proxy_basic_auth_header(&proxy).expect("build proxy auth"),
        Some("Basic dXNlcjpwYXNz".to_string())
    );
}

#[test]
fn websocket_connect_error_preserves_http_unauthorized_status() {
    let mut response = super::WsClientResponse::new(None);
    *response.status_mut() = axum::http::StatusCode::UNAUTHORIZED;
    let err = super::WsConnectError::from_tungstenite(tokio_tungstenite::tungstenite::Error::Http(
        Box::new(response),
    ));

    assert!(err.is_unauthorized());
    assert_eq!(err.status_code, Some(401));
}

#[test]
fn inspect_ws_terminal_event_infers_usage_limit_status_without_explicit_status() {
    let event = inspect_ws_terminal_event(
        r#"{"type":"error","error":{"message":"You've hit your usage limit."}}"#,
    )
    .expect("terminal event");

    assert_eq!(event.status_code, 429);
}

#[test]
fn infer_ws_terminal_status_maps_deactivation_message_to_403() {
    let payload = json!({
        "type": "response.failed",
        "error": {
            "message": "workspace_deactivated"
        }
    });

    assert_eq!(
        infer_ws_terminal_status(&payload, payload["error"]["message"].as_str()),
        403
    );
}

#[test]
fn parse_ws_usage_reads_chat_completion_compat_details() {
    let payload = json!({
        "type": "response.completed",
        "response": {
            "usage": {
                "prompt_tokens": 100,
                "prompt_tokens_details": { "cached_tokens": 75 },
                "completion_tokens": 20,
                "total_tokens": 120,
                "completion_tokens_details": { "reasoning_tokens": 9 }
            }
        }
    });

    let usage = parse_ws_usage(&payload);

    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cached_input_tokens, Some(75));
    assert_eq!(usage.output_tokens, Some(20));
    assert_eq!(usage.total_tokens, Some(120));
    assert_eq!(usage.reasoning_output_tokens, Some(9));
}

#[test]
fn inspect_ws_terminal_event_maps_incomplete_to_terminal_error() {
    let event = inspect_ws_terminal_event(
            r#"{"type":"response.incomplete","response":{"status":"incomplete","status_details":{"error":{"message":"stream timeout at upstream","code":"stream_timeout"}},"usage":{"input_tokens":11,"output_tokens":3,"total_tokens":14}}}"#,
        )
        .expect("terminal event");

    assert_eq!(event.status_code, 502);
    assert_eq!(event.error.as_deref(), Some("stream timeout at upstream"));
    assert_eq!(event.usage.input_tokens, Some(11));
    assert_eq!(event.usage.output_tokens, Some(3));
    assert_eq!(event.usage.total_tokens, Some(14));
}

#[test]
fn websocket_frame_preserves_prompt_cache_key_when_native_conversation_anchor_exists() {
    let _guard = crate::test_env_guard();
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers(Some("conversation-1"), None),
        prompt_cache_key: Some("sticky-thread".to_string()),
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
        key_policy: Default::default(),
        _api_key_inflight_guard: crate::gateway::try_acquire_api_key_inflight(
            "ws-test-preserve",
            None,
        )
        .expect("guard"),
    };
    let prepared = rewrite_client_frame(
            r#"{"type":"response.create","model":"gpt-5.4","input":"hello","prompt_cache_key":"client-thread"}"#,
            &context,
        )
        .unwrap_or_else(|_| panic!("rewrite websocket frame failed"));
    let value: serde_json::Value =
        serde_json::from_str(&prepared.text).expect("parse prepared websocket frame");

    assert_eq!(
        value
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some("client-thread")
    );
}

#[test]
fn websocket_frame_enforces_api_key_model_allowlist() {
    let _guard = crate::test_env_guard();
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers(None, None),
        prompt_cache_key: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
        key_policy: codexmanager_core::storage::ApiKeyPolicy {
            key_id: "ws-policy".into(),
            allowed_models: vec!["gpt-5.4".into()],
            allowed_platforms: vec!["codex".into()],
            model_visibility: "selectable".into(),
            expires_at: None,
            concurrency_limit: Some(1),
        },
        _api_key_inflight_guard: crate::gateway::try_acquire_api_key_inflight(
            "ws-test-allowlist",
            Some(1),
        )
        .expect("guard"),
    };
    let error = match rewrite_client_frame(
        r#"{"type":"response.create","model":"gpt-5.3-codex","input":"hello"}"#,
        &context,
    ) {
        Ok(_) => panic!("disallowed websocket model must fail"),
        Err(error) => error,
    };
    assert_eq!(error.status, 403);
    assert!(error.message.contains("model_not_allowed_by_key"));
}

#[test]
fn upstream_websocket_request_forwards_oai_attestation_header() {
    let mut headers = HeaderMap::new();
    headers.insert("x-oai-attestation", HeaderValue::from_static("attest-ws"));
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers),
        prompt_cache_key: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
        key_policy: Default::default(),
        _api_key_inflight_guard: crate::gateway::try_acquire_api_key_inflight(
            "ws-test-headers",
            None,
        )
        .expect("guard"),
    };
    let account = sample_account();

    let request = build_upstream_websocket_request(
        "wss://chatgpt.com/backend-api/codex/v1/responses",
        &account,
        "bearer-ws",
        &context,
    )
    .unwrap_or_else(|err| panic!("build upstream websocket request failed: {}", err.message));

    assert_eq!(
        request
            .headers()
            .get("x-oai-attestation")
            .and_then(|value| value.to_str().ok()),
        Some("attest-ws")
    );
    assert_eq!(
        request
            .headers()
            .get("openai-beta")
            .and_then(|value| value.to_str().ok()),
        Some(super::RESPONSES_WEBSOCKETS_BETA_HEADER_VALUE)
    );
}

#[test]
fn websocket_client_metadata_preserves_rewritten_codex_metadata() {
    let incoming_headers = sample_incoming_headers_with_metadata();
    let metadata = merge_client_metadata(
        Some(json!({
            "x-codex-installation-id": "install-from-rewrite",
            "source": "rewrite"
        })),
        Some(json!({
            "x-codex-installation-id": "install-from-client",
            "source": "client",
            "count": 7,
            "enabled": true
        })),
        &incoming_headers,
    )
    .expect("merged metadata");

    assert_eq!(
        metadata,
        json!({
            "x-codex-installation-id": "install-from-rewrite",
            "source": "rewrite",
            "count": "7",
            "enabled": "true",
            "x-codex-turn-metadata": "turn-meta-1",
            "x-codex-window-id": "window-1:0",
            "x-openai-subagent": "review",
            "x-codex-parent-thread-id": "parent-thread-1"
        })
    );
}

#[test]
fn websocket_frame_merges_header_metadata_into_client_metadata() {
    let _guard = crate::test_env_guard();
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers_with_metadata(),
        prompt_cache_key: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
        key_policy: Default::default(),
        _api_key_inflight_guard: crate::gateway::try_acquire_api_key_inflight(
            "ws-test-metadata",
            None,
        )
        .expect("guard"),
    };
    let prepared = rewrite_client_frame(
            r#"{"type":"response.create","model":"gpt-5.4","input":"hello","client_metadata":{"source":"client"}}"#,
            &context,
        )
        .unwrap_or_else(|_| panic!("rewrite websocket frame failed"));
    let value: serde_json::Value =
        serde_json::from_str(&prepared.text).expect("parse prepared websocket frame");

    assert_eq!(
        value["client_metadata"]["x-codex-turn-metadata"],
        "turn-meta-1"
    );
    assert_eq!(value["client_metadata"]["x-codex-window-id"], "window-1:0");
    assert_eq!(value["client_metadata"]["x-openai-subagent"], "review");
    assert_eq!(
        value["client_metadata"]["x-codex-parent-thread-id"],
        "parent-thread-1"
    );
    assert!(value["client_metadata"]["x-codex-installation-id"].is_string());
}

#[test]
fn websocket_response_create_keeps_codex_field_snapshot() {
    let _guard = crate::test_env_guard();
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers_with_metadata(),
        prompt_cache_key: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
        key_policy: Default::default(),
        _api_key_inflight_guard: crate::gateway::try_acquire_api_key_inflight(
            "ws-test-snapshot",
            None,
        )
        .expect("guard"),
    };
    let prepared = rewrite_client_frame(
            json!({
                "type": "response.create",
                "model": "gpt-5.4",
                "instructions": "stay",
                "previous_response_id": "resp_previous",
                "input": "hello",
                "tools": [{ "type": "function", "name": "ping", "parameters": { "type": "object", "properties": {} } }],
                "tool_choice": "auto",
                "parallel_tool_calls": true,
                "reasoning": { "effort": "medium", "summary": "auto", "context": "current_turn" },
                "store": false,
                "stream": true,
                "include": ["reasoning.encrypted_content"],
                "service_tier": "priority",
                "prompt_cache_key": "pc_ws_snapshot",
                "text": { "format": { "type": "text" } },
                "generate": false,
                "client_metadata": {
                    "source": "ws-snapshot",
                    "traceparent": "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00",
                    "tracestate": "rojo=00f067aa0ba902b7"
                },
                "max_output_tokens": 1024,
                "metadata": { "client": "third-party" },
                "temperature": 0.2,
                "top_p": 0.9,
                "truncation": "auto",
                "user": "third-party-user",
                "unknown_field": true
            })
            .to_string()
            .as_str(),
            &context,
        )
        .unwrap_or_else(|_| panic!("rewrite websocket frame failed"));
    let value: serde_json::Value =
        serde_json::from_str(&prepared.text).expect("parse prepared websocket frame");
    let object = value.as_object().expect("prepared frame object");
    let keys = object
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = [
        "client_metadata",
        "generate",
        "include",
        "input",
        "instructions",
        "model",
        "parallel_tool_calls",
        "previous_response_id",
        "prompt_cache_key",
        "reasoning",
        "service_tier",
        "store",
        "stream",
        "text",
        "tool_choice",
        "tools",
        "type",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(keys, expected);
    assert_eq!(value["type"], "response.create");
    assert_eq!(value["previous_response_id"], "resp_previous");
    assert_eq!(value["generate"], false);
    assert_eq!(value["reasoning"]["context"], "current_turn");
    assert_eq!(value["reasoning"]["summary"], "auto");
    assert_eq!(value["client_metadata"]["source"], "ws-snapshot");
    assert_eq!(
        value["client_metadata"]["traceparent"],
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00"
    );
    assert_eq!(
        value["client_metadata"]["tracestate"],
        "rojo=00f067aa0ba902b7"
    );
    assert_eq!(
        value["client_metadata"]["x-codex-turn-metadata"],
        "turn-meta-1"
    );
    assert!(object.get("max_output_tokens").is_none());
    assert!(object.get("metadata").is_none());
    assert!(object.get("temperature").is_none());
    assert!(object.get("top_p").is_none());
    assert!(object.get("truncation").is_none());
    assert!(object.get("user").is_none());
    assert!(object.get("unknown_field").is_none());
}

#[test]
fn websocket_retry_can_strip_previous_response_id() {
    let text = json!({
        "type": "response.create",
        "model": "gpt-5.4",
        "previous_response_id": "resp_previous",
        "input": "follow up"
    })
    .to_string();

    let stripped = strip_previous_response_id_from_ws_text(text.as_str())
        .expect("previous_response_id should be stripped");
    let value: serde_json::Value =
        serde_json::from_str(stripped.as_str()).expect("parse stripped frame");

    assert_eq!(value["type"], "response.create");
    assert!(value.get("previous_response_id").is_none());
    assert_eq!(value["input"], "follow up");
}

#[test]
fn websocket_detects_previous_response_not_found_terminal() {
    let terminal = inspect_ws_terminal_event(
            r#"{"type":"response.failed","status":400,"error":{"message":"Previous response with id 'resp_123' not found."}}"#,
        )
        .expect("terminal event");

    assert!(is_previous_response_not_found_terminal(&terminal));
}
