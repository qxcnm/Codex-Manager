use super::{
    apply_model_fast_policy_with_storage, build_socks5_connect_request,
    build_upstream_websocket_request, infer_ws_terminal_status, inspect_ws_terminal_event,
    is_previous_response_not_found_terminal, merge_client_metadata,
    missing_ws_tool_call_from_terminal, parse_websocket_target, parse_ws_usage,
    prepare_missing_ws_tool_call_retry, proxy_basic_auth_header,
    rebase_ws_request_for_account_change, rewrite_client_frame, should_buffer_ws_upstream_preamble,
    strip_previous_response_id_from_ws_text, ws_request_has_tool_call_output,
    CompletedWsResponseCache, CompletedWsToolCallCache, WsRequestContext, WsToolCallKind,
    WsUpstreamAuthorization,
};
use axum::http::{HeaderMap, HeaderValue};
use codexmanager_core::storage::{
    now_ts, Account, ApiKey, ConversationBinding, ManagedModelV2Upsert, ModelFastPolicyV2, Storage,
    Token,
};
use serde_json::{json, Value};

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

#[test]
fn websocket_frame_applies_model_fast_policy() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers(None, None),
        prompt_cache_key: None,
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
    };

    for (policy, client_tier, expected_upstream_tier, expected_source) in [
        (
            ModelFastPolicyV2::Passthrough,
            Some("fast"),
            Some("priority"),
            Some("client_request"),
        ),
        (
            ModelFastPolicyV2::Filter,
            Some("fast"),
            None,
            Some("model_policy"),
        ),
        (
            ModelFastPolicyV2::Force,
            None,
            Some("priority"),
            Some("model_policy"),
        ),
        (ModelFastPolicyV2::Block, None, None, Some("unset")),
    ] {
        let mut model = storage
            .get_managed_model_v2("gpt-5.4")
            .expect("read managed model")
            .expect("managed model");
        model.fast_policy = policy;
        storage
            .upsert_managed_model_v2(&ManagedModelV2Upsert {
                previous_slug: Some("gpt-5.4".to_string()),
                model,
            })
            .expect("update model fast policy");

        let mut frame = json!({
            "type": "response.create",
            "model": "gpt-5.4",
            "input": "hello"
        });
        if let Some(client_tier) = client_tier {
            frame["service_tier"] = Value::String(client_tier.to_string());
        }
        let prepared = rewrite_client_frame(frame.to_string().as_str(), &context)
            .expect("rewrite websocket frame");
        let prepared = apply_model_fast_policy_with_storage(prepared, &storage)
            .expect("apply websocket model fast policy");
        let value: Value = serde_json::from_str(&prepared.text).expect("parse rewritten frame");

        assert_eq!(
            value.get("service_tier").and_then(Value::as_str),
            expected_upstream_tier,
            "unexpected upstream service tier for {policy:?}"
        );
        assert_eq!(
            prepared.service_tier_source.as_deref(),
            expected_source,
            "unexpected service tier source for {policy:?}"
        );
    }

    let mut model = storage
        .get_managed_model_v2("gpt-5.4")
        .expect("read managed model")
        .expect("managed model");
    model.fast_policy = ModelFastPolicyV2::Block;
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some("gpt-5.4".to_string()),
            model,
        })
        .expect("update block policy");
    let prepared = rewrite_client_frame(
        r#"{"type":"response.create","model":"gpt-5.4","input":"hello","service_tier":"fast"}"#,
        &context,
    )
    .expect("rewrite blocked frame");
    let err = match apply_model_fast_policy_with_storage(prepared, &storage) {
        Ok(_) => panic!("block policy must reject explicit fast request"),
        Err(err) => err,
    };
    assert_eq!(err.status, 400);
    assert_eq!(
        err.code,
        crate::models_v2::fast_policy::FAST_REQUEST_BLOCKED
    );

    let mut api_key_fast_context = context.clone();
    api_key_fast_context.api_key.service_tier = Some("fast".to_string());
    let prepared = rewrite_client_frame(
        r#"{"type":"response.create","model":"gpt-5.4","input":"hello"}"#,
        &api_key_fast_context,
    )
    .expect("rewrite API key fast frame");
    let prepared = apply_model_fast_policy_with_storage(prepared, &storage)
        .expect("block policy must allow API key injected fast tier");
    let value: Value = serde_json::from_str(&prepared.text).expect("parse API key fast frame");
    assert_eq!(
        value.get("service_tier").and_then(Value::as_str),
        Some("priority")
    );

    let mut overridden_model = storage
        .get_managed_model_v2("gpt-5.4-mini")
        .expect("read overridden managed model")
        .expect("overridden managed model");
    overridden_model.fast_policy = ModelFastPolicyV2::Filter;
    storage
        .upsert_managed_model_v2(&ManagedModelV2Upsert {
            previous_slug: Some("gpt-5.4-mini".to_string()),
            model: overridden_model,
        })
        .expect("update overridden model fast policy");
    let mut model_override_context = context;
    model_override_context.api_key.model_slug = Some("gpt-5.4-mini".to_string());
    let prepared = rewrite_client_frame(
        r#"{"type":"response.create","model":"gpt-5.4","input":"hello","service_tier":"fast"}"#,
        &model_override_context,
    )
    .expect("rewrite overridden model frame");
    let prepared = apply_model_fast_policy_with_storage(prepared, &storage)
        .expect("apply final model fast policy");
    let value: Value = serde_json::from_str(&prepared.text).expect("parse overridden model frame");
    assert_eq!(prepared.model.as_deref(), Some("gpt-5.4-mini"));
    assert!(value.get("service_tier").is_none());
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

fn websocket_bearer_authorization(value: &str) -> WsUpstreamAuthorization {
    WsUpstreamAuthorization {
        value: value.to_string(),
        task_id: None,
        uses_agent_identity: false,
        is_fedramp: false,
        account_scope_id: None,
    }
}

fn insert_ws_candidate(storage: &Storage, id: &str, sort: i64, group_name: &str) {
    let now = now_ts();
    storage
        .insert_account(&Account {
            id: id.to_string(),
            label: id.to_string(),
            issuer: "issuer".to_string(),
            chatgpt_account_id: None,
            workspace_id: None,
            group_name: Some(group_name.to_string()),
            sort,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert websocket account");
    storage
        .insert_token(&Token {
            account_id: id.to_string(),
            id_token: "header.payload.sig".to_string(),
            access_token: "header.payload.sig".to_string(),
            refresh_token: "refresh".to_string(),
            api_key_access_token: None,
            last_refresh: now,
        })
        .expect("insert websocket token");
    crate::gateway::invalidate_candidate_cache();
}

#[test]
fn websocket_initial_and_terminal_failover_candidates_stay_in_key_group() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let mut api_key = sample_api_key();
    api_key.id = "gk-ws-group".to_string();
    api_key.key_hash = "hash-ws-group".to_string();
    storage.insert_api_key(&api_key).expect("insert api key");
    storage
        .update_api_key_account_group_filter(&api_key.id, Some("team-a"))
        .expect("set api key group filter");
    insert_ws_candidate(&storage, "acc-a-first", 0, "team-a");
    insert_ws_candidate(&storage, "acc-b-forbidden", 1, "team-b");
    insert_ws_candidate(&storage, "acc-a-failover", 2, "team-a");

    let initial = crate::gateway::gateway_collect_routed_candidates_with_log_source(
        &storage,
        &api_key.id,
        Some("gpt-5.4"),
    )
    .expect("collect initial websocket candidates");
    let mut initial_ids = initial
        .candidates
        .iter()
        .map(|(account, _)| account.id.as_str())
        .collect::<Vec<_>>();
    initial_ids.sort_unstable();
    assert_eq!(initial_ids, vec!["acc-a-failover", "acc-a-first"]);

    let current_account_id = initial.candidates[0].0.id.clone();
    let failover = crate::gateway::gateway_collect_routed_candidates_with_log_source(
        &storage,
        &api_key.id,
        Some("gpt-5.4"),
    )
    .expect("collect failover websocket candidates");
    let replacement = failover
        .candidates
        .iter()
        .find(|(account, _)| account.id != current_account_id)
        .expect("same-group failover candidate");
    assert_ne!(replacement.0.id, "acc-b-forbidden");
    assert!(failover
        .candidates
        .iter()
        .all(|(account, _)| account.group_name.as_deref() == Some("team-a")));
}

#[test]
fn websocket_reselection_excludes_disabled_and_runtime_limited_accounts() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let mut api_key = sample_api_key();
    api_key.id = "gk-ws-reselection".to_string();
    api_key.key_hash = "hash-ws-reselection".to_string();
    storage.insert_api_key(&api_key).expect("insert api key");
    insert_ws_candidate(&storage, "acc-disabled", 0, "team-a");
    insert_ws_candidate(&storage, "acc-available", 1, "team-a");
    storage
        .update_account_status("acc-disabled", "disabled")
        .expect("disable account");
    crate::gateway::invalidate_candidate_cache();

    let routed = crate::gateway::gateway_collect_routed_candidates_for_ws(
        &storage,
        &api_key.id,
        Some("gpt-5.4"),
        Some("conversation-reselection"),
        Some(crate::gateway::conversation_binding::RouteConversationSource::NativeConversation),
    )
    .expect("collect websocket candidates");
    assert_eq!(
        routed
            .candidates
            .iter()
            .map(|(account, _)| account.id.as_str())
            .collect::<Vec<_>>(),
        vec!["acc-available"]
    );
    assert!(crate::gateway::gateway_ws_account_requires_switch(
        &routed,
        "acc-disabled"
    ));

    crate::gateway::gateway_mark_account_cooldown_for_status("acc-available", 429);
    let cooled = crate::gateway::gateway_collect_routed_candidates_for_ws(
        &storage,
        &api_key.id,
        Some("gpt-5.4"),
        Some("conversation-reselection"),
        Some(crate::gateway::conversation_binding::RouteConversationSource::NativeConversation),
    )
    .expect("collect websocket candidates after cooldown");
    assert!(cooled.candidates.is_empty());
    assert!(crate::gateway::gateway_ws_account_requires_switch(
        &cooled,
        "acc-available"
    ));
    crate::gateway::reload_runtime_config_from_env();
}

#[test]
fn websocket_reselection_keeps_thread_binding_but_switches_when_bound_account_is_unavailable() {
    let _guard = crate::test_env_guard();
    let storage = Storage::open_in_memory().expect("open");
    storage.init().expect("init");
    let mut api_key = sample_api_key();
    api_key.id = "gk-ws-thread-reselection".to_string();
    api_key.key_hash = "hash-ws-thread-reselection".to_string();
    storage.insert_api_key(&api_key).expect("insert api key");
    insert_ws_candidate(&storage, "acc-bound", 0, "team-a");
    insert_ws_candidate(&storage, "acc-next", 1, "team-a");
    let now = now_ts();
    storage
        .upsert_conversation_binding(&ConversationBinding {
            platform_key_hash: api_key.key_hash.clone(),
            conversation_id: "conversation-thread".to_string(),
            account_id: "acc-bound".to_string(),
            thread_epoch: 1,
            thread_anchor: "conversation-thread".to_string(),
            status: "active".to_string(),
            last_model: Some("gpt-5.4".to_string()),
            last_switch_reason: None,
            created_at: now,
            updated_at: now,
            last_used_at: now,
        })
        .expect("insert conversation binding");
    storage
        .update_account_status("acc-bound", "disabled")
        .expect("disable bound account");
    crate::gateway::invalidate_candidate_cache();

    let routed = crate::gateway::gateway_collect_routed_candidates_for_ws(
        &storage,
        &api_key.id,
        Some("gpt-5.4"),
        Some("conversation-thread"),
        Some(crate::gateway::conversation_binding::RouteConversationSource::NativeConversation),
    )
    .expect("collect websocket candidates for bound thread");
    assert_eq!(routed.candidates[0].0.id, "acc-next");
    assert!(
        !routed
            .conversation_routing
            .as_ref()
            .expect("conversation routing")
            .bound_account_selectable
    );
    assert!(crate::gateway::gateway_ws_account_requires_switch(
        &routed,
        "acc-bound"
    ));
}

#[test]
fn websocket_reselection_honors_manual_preference_without_conversation_binding() {
    let mut preferred = sample_account();
    preferred.id = "acc-preferred".to_string();
    let mut current = sample_account();
    current.id = "acc-current".to_string();
    let token = |account_id: &str| Token {
        account_id: account_id.to_string(),
        id_token: "header.payload.sig".to_string(),
        access_token: "header.payload.sig".to_string(),
        refresh_token: "refresh".to_string(),
        api_key_access_token: None,
        last_refresh: now_ts(),
    };
    let routed = crate::gateway::GatewayRoutedCandidates {
        candidates: vec![
            (preferred, token("acc-preferred")),
            (current, token("acc-current")),
        ],
        route_strategy: "manual_preferred_account",
        route_source: "manual_preferred_account",
        conversation_routing: None,
    };

    assert!(crate::gateway::gateway_ws_account_requires_switch(
        &routed,
        "acc-current"
    ));
    assert!(!crate::gateway::gateway_ws_account_requires_switch(
        &routed,
        "acc-preferred"
    ));
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
fn websocket_connect_error_detects_invalid_agent_identity_task_body() {
    let mut response =
        super::WsClientResponse::new(Some(br#"{"error":{"code":"task_expired"}}"#.to_vec()));
    *response.status_mut() = axum::http::StatusCode::UNAUTHORIZED;
    let err = super::WsConnectError::from_tungstenite(tokio_tungstenite::tungstenite::Error::Http(
        Box::new(response),
    ));

    assert!(err.is_agent_identity_task_invalid());
}

#[test]
fn websocket_connect_error_detects_connection_limit_body() {
    let mut response = super::WsClientResponse::new(Some(
        br#"{"type":"error","status":400,"error":{"code":"websocket_connection_limit_reached"}}"#
            .to_vec(),
    ));
    *response.status_mut() = axum::http::StatusCode::BAD_REQUEST;
    let err = super::WsConnectError::from_tungstenite(tokio_tungstenite::tungstenite::Error::Http(
        Box::new(response),
    ));

    assert!(err.is_websocket_connection_limit_reached());
}

#[test]
fn websocket_connect_error_detects_compression_negotiation_rejection() {
    let mut response = super::WsClientResponse::new(Some(
        br#"unsupported extension: permessage-deflate"#.to_vec(),
    ));
    *response.status_mut() = axum::http::StatusCode::BAD_REQUEST;
    let err = super::WsConnectError::from_tungstenite(tokio_tungstenite::tungstenite::Error::Http(
        Box::new(response),
    ));

    assert!(err.is_compression_negotiation_rejection());
}

#[test]
fn websocket_connect_error_does_not_treat_unrelated_bad_request_as_compression_rejection() {
    let mut response = super::WsClientResponse::new(Some(br#"invalid response.create"#.to_vec()));
    *response.status_mut() = axum::http::StatusCode::BAD_REQUEST;
    let err = super::WsConnectError::from_tungstenite(tokio_tungstenite::tungstenite::Error::Http(
        Box::new(response),
    ));

    assert!(!err.is_compression_negotiation_rejection());
}

#[test]
fn inspect_ws_terminal_event_infers_usage_limit_status_without_explicit_status() {
    let event = inspect_ws_terminal_event(
        r#"{"type":"error","error":{"message":"You've hit your usage limit."}}"#,
    )
    .expect("terminal event");

    assert_eq!(event.status_code, 429);
    assert!(event.is_usage_limit);
}

#[test]
fn inspect_ws_terminal_event_reads_standard_nested_usage_limit_error() {
    let event = inspect_ws_terminal_event(
        r#"{"type":"response.failed","response":{"id":"resp_limited","status":"failed","error":{"code":"usage_limit_reached","message":"The usage limit has been reached"}}}"#,
    )
    .expect("terminal event");

    assert_eq!(event.status_code, 429);
    assert_eq!(
        event.error.as_deref(),
        Some("The usage limit has been reached")
    );
    assert!(event.is_usage_limit);
}

#[test]
fn inspect_ws_terminal_event_recognizes_usage_limit_code_without_message() {
    let event = inspect_ws_terminal_event(
        r#"{"type":"response.failed","response":{"error":{"code":"usage_limit_reached"}}}"#,
    )
    .expect("terminal event");

    assert_eq!(event.status_code, 429);
    assert_eq!(event.error.as_deref(), Some("usage_limit_reached"));
    assert!(event.is_usage_limit);
}

#[test]
fn inspect_ws_terminal_event_recognizes_websocket_connection_limit() {
    let event = inspect_ws_terminal_event(
        r#"{"type":"error","status":400,"error":{"code":"websocket_connection_limit_reached","message":"Responses websocket connection limit reached (60 minutes). Create a new websocket connection to continue."}}"#,
    )
    .expect("terminal event");

    assert_eq!(event.status_code, 400);
    assert!(event.is_websocket_connection_limit);
    assert!(!event.is_usage_limit);
}

#[test]
fn usage_limit_words_in_normal_output_are_not_treated_as_terminal() {
    assert!(inspect_ws_terminal_event(
        r#"{"type":"response.output_text.delta","delta":"The usage limit has been reached is an English error message."}"#,
    )
    .is_none());
}

#[test]
fn websocket_created_event_is_buffered_but_actual_output_is_not() {
    assert!(should_buffer_ws_upstream_preamble(
        r#"{"type":"response.created","response":{"id":"resp_a"}}"#,
        0,
    ));
    assert!(!should_buffer_ws_upstream_preamble(
        r#"{"type":"response.output_item.added","item":{"type":"message"}}"#,
        1,
    ));
}

#[test]
fn websocket_tool_output_request_keeps_retry_preamble_buffered() {
    assert!(ws_request_has_tool_call_output(
        json!({
            "type": "response.create",
            "input": [{
                "type": "custom_tool_call_output",
                "call_id": "call_buffered",
                "output": "done"
            }]
        })
        .to_string()
        .as_str()
    ));
    assert!(!ws_request_has_tool_call_output(
        json!({
            "type": "response.create",
            "input": "ordinary prompt"
        })
        .to_string()
        .as_str()
    ));
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
                "prompt_tokens_details": {
                    "cached_tokens": 75,
                    "cache_write_tokens": 7
                },
                "completion_tokens": 20,
                "total_tokens": 120,
                "completion_tokens_details": { "reasoning_tokens": 9 }
            }
        }
    });

    let usage = parse_ws_usage(&payload);

    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cached_input_tokens, Some(75));
    assert_eq!(usage.cache_write_tokens, Some(7));
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
fn inspect_ws_terminal_event_requires_response_completed() {
    assert!(
        inspect_ws_terminal_event(r#"{"type":"response.done","response":{"id":"resp_done"}}"#,)
            .is_none()
    );
}

#[test]
fn websocket_frame_aligns_prompt_cache_key_with_native_conversation_anchor() {
    let _guard = crate::test_env_guard();
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers(Some("conversation-1"), None),
        prompt_cache_key: Some("sticky-thread".to_string()),
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
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
        Some("conversation-1")
    );
}

#[test]
fn upstream_websocket_request_forwards_oai_attestation_header() {
    let mut headers = HeaderMap::new();
    headers.insert("x-oai-attestation", HeaderValue::from_static("attest-ws"));
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers),
        prompt_cache_key: None,
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
    };
    let account = sample_account();
    let authorization = websocket_bearer_authorization("bearer-ws");

    let request = build_upstream_websocket_request(
        "wss://chatgpt.com/backend-api/codex/v1/responses",
        &account,
        &authorization,
        &context,
        false,
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
    assert_eq!(
        request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer bearer-ws")
    );
    assert!(request.headers().get("x-openai-fedramp").is_none());
}

#[test]
fn upstream_websocket_request_preserves_agent_assertion_and_fedramp() {
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: crate::gateway::IncomingHeaderSnapshot::default(),
        prompt_cache_key: None,
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
    };
    let authorization = WsUpstreamAuthorization {
        value: "AgentAssertion encoded-envelope".to_string(),
        task_id: Some("task-1".to_string()),
        uses_agent_identity: true,
        is_fedramp: true,
        account_scope_id: Some("agent-bound-scope".to_string()),
    };

    let request = build_upstream_websocket_request(
        "wss://chatgpt.com/backend-api/codex/v1/responses",
        &sample_account(),
        &authorization,
        &context,
        false,
    )
    .unwrap_or_else(|err| panic!("build upstream websocket request failed: {}", err.message));

    assert_eq!(
        request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("AgentAssertion encoded-envelope")
    );
    assert_eq!(
        request
            .headers()
            .get("x-openai-fedramp")
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
    assert_eq!(
        request
            .headers()
            .get("chatgpt-account-id")
            .and_then(|value| value.to_str().ok()),
        Some("agent-bound-scope")
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
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
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
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
    };
    let prepared = rewrite_client_frame(
            json!({
                "type": "response.create",
                "model": "gpt-5.4",
                "instructions": "  stay exactly\n",
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
        "text",
        "tool_choice",
        "tools",
        "type",
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(keys, expected);
    assert_eq!(value["type"], "response.create");
    assert_eq!(value["instructions"], "  stay exactly\n");
    assert_eq!(value["previous_response_id"], "resp_previous");
    assert_eq!(value["generate"], false);
    assert!(object.get("stream").is_none());
    assert!(object.get("background").is_none());
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
fn websocket_response_create_uses_minimal_fallback_for_missing_or_blank_instructions() {
    let _guard = crate::test_env_guard();
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers_with_metadata(),
        prompt_cache_key: None,
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
    };

    for instructions in [
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!(" \n\t")),
    ] {
        let mut frame = json!({
            "type": "response.create",
            "model": "gpt-5.4",
            "input": "hello"
        });
        if let Some(instructions) = instructions {
            frame["instructions"] = instructions;
        }
        let prepared = rewrite_client_frame(frame.to_string().as_str(), &context)
            .unwrap_or_else(|_| panic!("rewrite websocket frame failed"));
        let value: Value =
            serde_json::from_str(&prepared.text).expect("parse prepared websocket frame");

        assert_eq!(value["instructions"], "Follow the user's instructions.");
    }
}

#[test]
fn websocket_logs_client_ultra_and_sends_upstream_max() {
    let _guard = crate::test_env_guard();
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: sample_incoming_headers_with_metadata(),
        prompt_cache_key: None,
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
    };
    let frame = json!({
        "type": "response.create",
        "model": "gpt-5.6-sol",
        "reasoning": { "effort": "ultra" },
        "input": "handle a complex task"
    });

    let prepared = rewrite_client_frame(frame.to_string().as_str(), &context)
        .unwrap_or_else(|_| panic!("rewrite websocket frame failed"));
    let value: Value = serde_json::from_str(&prepared.text).expect("parse rewritten frame");

    assert_eq!(prepared.client_reasoning_effort.as_deref(), Some("ultra"));
    assert_eq!(prepared.reasoning_effort.as_deref(), Some("max"));
    assert_eq!(
        prepared.reasoning_source.as_deref(),
        Some("client_request_normalized")
    );
    assert_eq!(value["reasoning"]["effort"], "max");
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
fn websocket_response_history_expands_store_false_text_chain() {
    let mut cache = CompletedWsResponseCache::default();
    assert!(cache
        .observe_completed_response(
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_history_1",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "first answer" }]
                    }]
                }
            })
            .to_string()
            .as_str(),
            None,
            &json!("first question"),
        )
        .expect("cache first completed response"));
    assert!(cache.contains("resp_history_1"));
    assert!(cache
        .observe_completed_response(
            json!({
                "type": "response.completed",
                "response": {
                    "id": "resp_history_2",
                    "output": [{
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": "second answer" }]
                    }]
                }
            })
            .to_string()
            .as_str(),
            Some("resp_history_1"),
            &json!([{
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": "second question" }]
            }]),
        )
        .expect("cache second completed response"));

    let expanded = super::expand_response_create_previous_response(
        json!({
            "type": "response.create",
            "model": "gpt-5.4",
            "store": false,
            "previous_response_id": "resp_history_2",
            "input": "third question"
        })
        .to_string()
        .as_str(),
        &cache,
    )
    .expect("expand cached response history")
    .expect("request has previous_response_id");
    let value: Value = serde_json::from_str(&expanded).expect("parse expanded history");
    let input = value["input"].as_array().expect("expanded input array");

    assert!(value.get("previous_response_id").is_none());
    assert_eq!(input.len(), 5);
    assert_eq!(input[0]["role"], "user");
    assert_eq!(input[0]["content"][0]["text"], "first question");
    assert_eq!(input[1]["role"], "assistant");
    assert_eq!(input[1]["content"][0]["text"], "first answer");
    assert_eq!(input[2]["content"][0]["text"], "second question");
    assert_eq!(input[3]["content"][0]["text"], "second answer");
    assert_eq!(input[4]["content"][0]["text"], "third question");
}

#[test]
fn websocket_response_history_requires_complete_cached_chain() {
    let mut cache = CompletedWsResponseCache::default();
    cache
        .observe_completed_response(
            json!({
                "type": "response.completed",
                "response": { "id": "resp_history_child", "output": [] }
            })
            .to_string()
            .as_str(),
            Some("resp_history_missing_parent"),
            &json!("child question"),
        )
        .expect("cache child response");

    let err = super::expand_response_create_previous_response(
        json!({
            "type": "response.create",
            "previous_response_id": "resp_history_child",
            "input": "continue"
        })
        .to_string()
        .as_str(),
        &cache,
    )
    .expect_err("missing parent must not produce partial context");

    assert!(err.contains("resp_history_missing_parent"));
    assert!(err.contains("not available"));
}

#[test]
fn websocket_account_rebase_prepends_cached_tool_calls_before_outputs() {
    let mut cache = CompletedWsToolCallCache::default();
    cache.observe_upstream_event(
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "custom_tool_call",
                "id": "ctc_item_1",
                "call_id": "call_custom_1",
                "name": "apply_patch",
                "input": "*** Begin Patch"
            }
        })
        .to_string()
        .as_str(),
    );
    cache.observe_upstream_event(
        json!({
            "type": "response.completed",
            "response": {
                "id": "resp_tool_calls",
                "output": [{
                    "type": "function_call",
                    "id": "fc_item_1",
                    "call_id": "call_function_1",
                    "name": "lookup",
                    "arguments": "{}"
                }]
            }
        })
        .to_string()
        .as_str(),
    );
    let request = json!({
        "type": "response.create",
        "previous_response_id": "resp_tool_calls",
        "session_id": "old-session",
        "x-codex-turn-state": "old-turn-state",
        "client_metadata": {
            "x-codex-window-id": "old-window",
            "source": "unit-test"
        },
        "input": [
            {
                "type": "custom_tool_call_output",
                "call_id": "call_custom_1",
                "output": "patched"
            },
            {
                "type": "function_call_output",
                "call_id": "call_function_1",
                "output": "found"
            }
        ]
    });

    let rebased = rebase_ws_request_for_account_change(request.to_string().as_str(), &cache)
        .expect("account rebase should restore cached tool calls");
    let value: Value = serde_json::from_str(rebased.as_str()).expect("parse rebased request");
    let input = value["input"].as_array().expect("rebased input array");

    assert_eq!(input.len(), 4);
    assert_eq!(input[0]["type"], "custom_tool_call");
    assert_eq!(input[0]["call_id"], "call_custom_1");
    assert_eq!(input[1]["type"], "custom_tool_call_output");
    assert_eq!(input[2]["type"], "function_call");
    assert_eq!(input[2]["call_id"], "call_function_1");
    assert_eq!(input[3]["type"], "function_call_output");
    assert!(value.get("previous_response_id").is_none());
    assert!(value.get("session_id").is_none());
    assert!(value.get("x-codex-turn-state").is_none());
    assert!(value["client_metadata"].get("x-codex-window-id").is_none());
    assert_eq!(value["client_metadata"]["source"], "unit-test");
}

#[test]
fn websocket_account_rebase_reorders_existing_call_without_duplicate() {
    let request = json!({
        "type": "response.create",
        "previous_response_id": "resp_existing_call",
        "input": [
            {
                "type": "custom_tool_call_output",
                "call_id": "call_existing",
                "output": "done"
            },
            {
                "type": "custom_tool_call",
                "id": "ctc_existing",
                "call_id": "call_existing",
                "name": "apply_patch",
                "input": "patch"
            }
        ]
    });

    let rebased = rebase_ws_request_for_account_change(
        request.to_string().as_str(),
        &CompletedWsToolCallCache::default(),
    )
    .expect("current input call should satisfy its output");
    let value: Value = serde_json::from_str(rebased.as_str()).expect("parse rebased request");
    let input = value["input"].as_array().expect("rebased input array");

    assert_eq!(input.len(), 2, "matching call must not be duplicated");
    assert_eq!(input[0]["type"], "custom_tool_call");
    assert_eq!(input[1]["type"], "custom_tool_call_output");
}

#[test]
fn websocket_account_rebase_strips_cross_account_reasoning_and_affinity_metadata() {
    fn contains_encrypted_content(value: &Value) -> bool {
        match value {
            Value::Object(object) => {
                object.contains_key("encrypted_content")
                    || object.values().any(contains_encrypted_content)
            }
            Value::Array(items) => items.iter().any(contains_encrypted_content),
            _ => false,
        }
    }

    let request = json!({
        "type": "response.create",
        "previous_response_id": "resp_old_account",
        "x-codex-parent-thread-id": "parent-old",
        "x-codex-turn-metadata": "turn-meta-old",
        "client_metadata": {
            "session_id": "session-old",
            "conversation_id": "conversation-old",
            "x-client-request-id": "request-old",
            "x-codex-window-id": "window-old",
            "x-codex-turn-state": "turn-state-old",
            "x-codex-parent-thread-id": "parent-old",
            "x-codex-turn-metadata": "turn-meta-old",
            "x-openai-subagent": "review",
            "x-codex-beta-features": "beta-a",
            "source": "unit-test"
        },
        "input": [
            {
                "type": "reasoning",
                "id": "reasoning-old",
                "summary": [],
                "encrypted_content": "encrypted-old-account"
            },
            {
                "type": "message",
                "role": "user",
                "content": [
                    { "type": "input_text", "text": "continue" },
                    { "type": "encrypted_content", "encrypted_content": "nested-old" }
                ]
            }
        ]
    });

    let rebased = rebase_ws_request_for_account_change(
        request.to_string().as_str(),
        &CompletedWsToolCallCache::default(),
    )
    .expect("cross-account rebase should succeed");
    let value: Value = serde_json::from_str(&rebased).expect("parse rebased request");

    assert!(!contains_encrypted_content(&value));
    assert!(value.get("previous_response_id").is_none());
    assert!(value.get("x-codex-parent-thread-id").is_none());
    assert!(value.get("x-codex-turn-metadata").is_none());
    for key in [
        "session_id",
        "conversation_id",
        "x-client-request-id",
        "x-codex-window-id",
        "x-codex-turn-state",
        "x-codex-parent-thread-id",
        "x-codex-turn-metadata",
    ] {
        assert!(
            value["client_metadata"].get(key).is_none(),
            "cross-account rebase must strip {key}"
        );
    }
    assert_eq!(value["client_metadata"]["x-openai-subagent"], "review");
    assert_eq!(value["client_metadata"]["x-codex-beta-features"], "beta-a");
    assert_eq!(value["client_metadata"]["source"], "unit-test");
    assert_eq!(value["input"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        value["input"][0]["content"],
        json!([{ "type": "input_text", "text": "continue" }])
    );
}

#[test]
fn websocket_account_rebase_rejects_orphan_tool_output() {
    let request = json!({
        "type": "response.create",
        "previous_response_id": "resp_missing_call",
        "input": [{
            "type": "custom_tool_call_output",
            "call_id": "call_missing",
            "output": "done"
        }]
    });

    let err = rebase_ws_request_for_account_change(
        request.to_string().as_str(),
        &CompletedWsToolCallCache::default(),
    )
    .expect_err("orphan output must not be sent to a different account");

    assert_eq!(err.code, super::RESPONSES_WS_CONTEXT_REBASE_ERROR_CODE);
    assert!(err.message.contains("call_missing"));
    assert!(err.message.contains("custom_tool_call"));
}

#[test]
fn upstream_websocket_account_rebase_strips_session_affinity_headers() {
    let mut headers = HeaderMap::new();
    headers.insert("session_id", HeaderValue::from_static("session-old"));
    headers.insert("x-codex-window-id", HeaderValue::from_static("window-old"));
    headers.insert(
        "x-client-request-id",
        HeaderValue::from_static("request-old"),
    );
    headers.insert("x-codex-turn-state", HeaderValue::from_static("turn-old"));
    headers.insert(
        "x-codex-parent-thread-id",
        HeaderValue::from_static("parent-old"),
    );
    headers.insert(
        "x-codex-turn-metadata",
        HeaderValue::from_static("turn-meta-old"),
    );
    headers.insert("x-openai-subagent", HeaderValue::from_static("review"));
    headers.insert("x-codex-beta-features", HeaderValue::from_static("beta-a"));
    let context = WsRequestContext {
        api_key: sample_api_key(),
        incoming_headers: crate::gateway::IncomingHeaderSnapshot::from_http_headers(&headers),
        prompt_cache_key: None,
        route_conversation_id: None,
        route_conversation_source: None,
        effective_upstream_base: "https://chatgpt.com/backend-api/codex".to_string(),
        prefer_raw_errors: false,
    };
    let request = build_upstream_websocket_request(
        "wss://chatgpt.com/backend-api/codex/responses",
        &sample_account(),
        &websocket_bearer_authorization("bearer-ws"),
        &context,
        true,
    )
    .unwrap_or_else(|err| panic!("build rebased websocket request failed: {}", err.message));

    for header in [
        "session_id",
        "x-codex-window-id",
        "x-client-request-id",
        "x-codex-turn-state",
        "x-codex-parent-thread-id",
        "x-codex-turn-metadata",
    ] {
        assert!(
            request.headers().get(header).is_none(),
            "rebased websocket must strip {header}"
        );
    }
    assert_eq!(
        request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer bearer-ws")
    );
    assert_eq!(
        request
            .headers()
            .get("x-openai-subagent")
            .and_then(|value| value.to_str().ok()),
        Some("review")
    );
    assert_eq!(
        request
            .headers()
            .get("x-codex-beta-features")
            .and_then(|value| value.to_str().ok()),
        Some("beta-a")
    );
}

#[test]
fn websocket_detects_previous_response_not_found_terminal() {
    let terminal = inspect_ws_terminal_event(
            r#"{"type":"response.failed","status":400,"error":{"message":"Previous response with id 'resp_123' not found."}}"#,
        )
        .expect("terminal event");

    assert!(is_previous_response_not_found_terminal(&terminal));
}

#[test]
fn websocket_detects_exact_missing_custom_and_function_tool_call_terminals() {
    for (message, expected_kind, expected_call_id) in [
        (
            "No tool call found for custom tool call output with call_id call_custom_1.",
            WsToolCallKind::Custom,
            "call_custom_1",
        ),
        (
            "No tool call found for function call output with call_id 'call_function_1'.",
            WsToolCallKind::Function,
            "call_function_1",
        ),
        (
            "No tool call found for function tool call output with call_id call_function_2",
            WsToolCallKind::Function,
            "call_function_2",
        ),
    ] {
        let terminal = inspect_ws_terminal_event(
            json!({ "type": "error", "error": { "message": message } })
                .to_string()
                .as_str(),
        )
        .expect("missing tool call error should be terminal");

        assert_eq!(terminal.status_code, 400);
        assert_eq!(
            missing_ws_tool_call_from_terminal(&terminal),
            Some((expected_kind, expected_call_id.to_string()))
        );
    }

    let unrelated = inspect_ws_terminal_event(
        r#"{"type":"error","status":400,"error":{"message":"No tool call found while processing output"}}"#,
    )
    .expect("unrelated error terminal");
    assert!(missing_ws_tool_call_from_terminal(&unrelated).is_none());
}

#[test]
fn websocket_missing_tool_call_recovery_changes_payload_and_retries_only_once() {
    let mut cache = CompletedWsToolCallCache::default();
    cache.observe_upstream_event(
        json!({
            "type": "response.output_item.done",
            "item": {
                "type": "custom_tool_call",
                "id": "ctc_retry_once",
                "call_id": "call_retry_once",
                "name": "apply_patch",
                "input": "patch"
            }
        })
        .to_string()
        .as_str(),
    );
    let request = json!({
        "type": "response.create",
        "previous_response_id": "resp_old",
        "input": [{
            "type": "custom_tool_call_output",
            "call_id": "call_retry_once",
            "output": "done"
        }]
    })
    .to_string();
    let terminal = inspect_ws_terminal_event(
        r#"{"type":"response.failed","status":400,"error":{"message":"No tool call found for custom tool call output with call_id call_retry_once."}}"#,
    )
    .expect("missing tool call terminal");
    let mut already_retried = false;

    let recovered =
        prepare_missing_ws_tool_call_retry(&request, &cache, &terminal, &mut already_retried)
            .expect("prepare recovery")
            .expect("matching cached call should allow recovery");
    let value: Value = serde_json::from_str(&recovered).expect("parse recovered request");
    assert!(already_retried);
    assert!(value.get("previous_response_id").is_none());
    assert_eq!(value["input"][0]["type"], "custom_tool_call");
    assert_eq!(value["input"][1]["type"], "custom_tool_call_output");

    assert!(
        prepare_missing_ws_tool_call_retry(&request, &cache, &terminal, &mut already_retried,)
            .expect("second recovery check")
            .is_none()
    );
}

#[test]
fn websocket_missing_tool_call_recovery_requires_a_matching_call_and_changed_rebase() {
    let terminal = inspect_ws_terminal_event(
        r#"{"type":"error","status":400,"error":{"message":"No tool call found for custom tool call output with call_id call_unchanged."}}"#,
    )
    .expect("missing tool call terminal");
    let mut already_retried = false;
    let orphan = json!({
        "type": "response.create",
        "input": [{
            "type": "custom_tool_call_output",
            "call_id": "call_unchanged",
            "output": "done"
        }]
    })
    .to_string();
    assert!(prepare_missing_ws_tool_call_retry(
        &orphan,
        &CompletedWsToolCallCache::default(),
        &terminal,
        &mut already_retried,
    )
    .expect("orphan recovery check")
    .is_none());
    assert!(!already_retried);

    let already_rebased = json!({
        "type": "response.create",
        "input": [
            {
                "type": "custom_tool_call",
                "call_id": "call_unchanged",
                "name": "apply_patch",
                "input": "patch"
            },
            {
                "type": "custom_tool_call_output",
                "call_id": "call_unchanged",
                "output": "done"
            }
        ]
    })
    .to_string();
    assert!(prepare_missing_ws_tool_call_retry(
        &already_rebased,
        &CompletedWsToolCallCache::default(),
        &terminal,
        &mut already_retried,
    )
    .expect("unchanged recovery check")
    .is_none());
    assert!(!already_retried);
}
