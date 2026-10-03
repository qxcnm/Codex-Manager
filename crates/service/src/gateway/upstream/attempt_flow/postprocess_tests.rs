use super::*;
use crate::gateway::IncomingHeaderSnapshot;
use axum::http::{HeaderMap, HeaderValue};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use codexmanager_core::storage::{now_ts, Account, AccountAgentIdentity, Storage, Token};
use ed25519_dalek::pkcs8::EncodePrivateKey;
use ed25519_dalek::SigningKey;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
use tiny_http::{Response, Server, StatusCode};

fn oauth_authorization(value: &str) -> super::super::primary_flow::PrimaryAuthorization {
    super::super::primary_flow::PrimaryAuthorization {
        value: value.to_string(),
        task_id: None,
        uses_agent_identity: false,
        is_fedramp: false,
        account_scope_id: None,
    }
}

/// 函数 `build_account`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - id: 参数 id
/// - now: 参数 now
///
/// # 返回
/// 返回函数执行结果
fn build_account(id: &str, now: i64) -> Account {
    Account {
        id: id.to_string(),
        label: id.to_string(),
        issuer: "https://auth.openai.com".to_string(),
        chatgpt_account_id: Some("chatgpt-account".to_string()),
        workspace_id: Some("workspace-account".to_string()),
        group_name: None,
        sort: 0,
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
    }
}

/// 函数 `build_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account_id: 参数 account_id
/// - now: 参数 now
///
/// # 返回
/// 返回函数执行结果
fn build_token(account_id: &str, now: i64) -> Token {
    Token {
        account_id: account_id.to_string(),
        id_token: "id-token".to_string(),
        access_token: "access-token".to_string(),
        refresh_token: String::new(),
        api_key_access_token: Some("api-key-token".to_string()),
        last_refresh: now,
    }
}

fn codex_session_headers() -> IncomingHeaderSnapshot {
    let mut headers = HeaderMap::new();
    for (name, value) in [
        ("session-id", "session-current"),
        ("thread-id", "thread-current"),
        ("x-client-request-id", "request-current"),
        ("x-codex-window-id", "session-current:7"),
        ("x-codex-turn-state", "turn-state-current"),
    ] {
        headers.insert(
            name,
            HeaderValue::from_str(value).expect("valid session header"),
        );
    }
    IncomingHeaderSnapshot::from_http_headers(&headers)
}

fn request_has_header(request: &tiny_http::Request, name: &str) -> bool {
    request
        .headers()
        .iter()
        .any(|header| header.field.to_string().eq_ignore_ascii_case(name))
}

fn request_header_value(request: &tiny_http::Request, name: &str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|header| header.field.to_string().eq_ignore_ascii_case(name))
        .map(|header| header.value.as_str().to_string())
}

fn captured_session_headers(request: &tiny_http::Request) -> Vec<bool> {
    [
        "session-id",
        "thread-id",
        "x-client-request-id",
        "x-codex-window-id",
        "x-codex-turn-state",
    ]
    .iter()
    .map(|name| request_has_header(request, name))
    .collect()
}

#[test]
fn anthropic_challenge_uses_extended_cooldown_reason() {
    assert_eq!(
        challenge_cooldown_reason(crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE),
        crate::gateway::CooldownReason::AnthropicChallenge
    );
    assert_eq!(
        challenge_cooldown_reason(crate::apikey_profile::PROTOCOL_OPENAI_COMPAT),
        crate::gateway::CooldownReason::Challenge
    );
}

#[test]
fn agent_identity_always_disables_openai_fallback() {
    let agent_identity = super::super::primary_flow::PrimaryAuthorization {
        value: "AgentAssertion encoded-envelope".to_string(),
        task_id: Some("task-id".to_string()),
        uses_agent_identity: true,
        is_fedramp: false,
        account_scope_id: Some("workspace-account".to_string()),
    };

    assert!(!allow_openai_fallback_for_authorization(
        true,
        &agent_identity
    ));
    assert!(allow_openai_fallback_for_authorization(
        true,
        &oauth_authorization("access-token")
    ));
}

#[test]
fn bad_request_stateless_retry_requires_actual_responses_target() {
    assert!(should_retry_chatgpt_responses_bad_request(
        "https://chatgpt.com/backend-api/codex",
        "https://chatgpt.com/backend-api/codex/responses",
        400,
    ));
    assert!(!should_retry_chatgpt_responses_bad_request(
        "https://chatgpt.com/backend-api/codex",
        "https://chatgpt.com/backend-api/codex/chat/completions",
        400,
    ));
    assert!(!should_retry_chatgpt_responses_bad_request(
        "https://chatgpt.com/backend-api/codex",
        "https://chatgpt.com/backend-api/codex/responses",
        404,
    ));
}

#[test]
fn agent_identity_invalid_task_recovery_replays_once_without_oauth_fallback() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-agent-identity-recovery", now);
    let mut token = build_token(account.id.as_str(), now);
    token.access_token.clear();
    token.refresh_token = "must-not-be-used".to_string();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let signing_key = SigningKey::from_bytes(&[19_u8; 32]);
    let private_key = signing_key.to_pkcs8_der().expect("encode private key");
    storage
        .upsert_account_agent_identity(&AccountAgentIdentity {
            account_id: account.id.clone(),
            agent_runtime_id: "agent-runtime-recovery".to_string(),
            agent_private_key: BASE64_STANDARD.encode(private_key.as_bytes()),
            task_id: Some("task-current".to_string()),
            chatgpt_user_id: "user-recovery".to_string(),
            chatgpt_account_is_fedramp: true,
            auth_mode: "agentIdentity".to_string(),
            workspace_id: account.workspace_id.clone(),
            created_at: now,
            updated_at: now,
        })
        .expect("insert agent identity");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let (request_tx, request_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        for status in [401_u16, 200_u16] {
            let mut request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive upstream request")
                .expect("request present");
            request_tx
                .send((
                    request_header_value(&request, "Authorization"),
                    request_header_value(&request, "x-openai-fedramp"),
                ))
                .expect("capture request headers");
            let mut body = Vec::new();
            std::io::Read::read_to_end(request.as_reader(), &mut body).expect("read request body");
            let response_body = if status == 401 {
                r#"{"error":{"code":"task_expired"}}"#
            } else {
                r#"{"ok":true}"#
            };
            request
                .respond(
                    Response::from_string(response_body)
                        .with_status_code(StatusCode(status))
                        .with_header(
                            tiny_http::Header::from_bytes("Content-Type", "application/json")
                                .expect("content type"),
                        ),
                )
                .expect("respond request");
        }
    });

    let client = reqwest::Client::new();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: true,
        capture: None,
    };
    let incoming_headers = IncomingHeaderSnapshot::default();
    let body = Bytes::from_static(br#"{"model":"gpt-5.5","input":"hello"}"#);
    let authorization = super::super::primary_flow::PrimaryAuthorization {
        value: "AgentAssertion stale-envelope".to_string(),
        task_id: Some("task-stale".to_string()),
        uses_agent_identity: true,
        is_fedramp: true,
        account_scope_id: Some("workspace-account".to_string()),
    };
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        addr.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        authorization.value.as_str(),
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial request");

    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        addr.as_str(),
        None,
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        &authorization,
        &account,
        &mut token,
        None,
        false,
        false,
        true,
        false,
        false,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    let first = request_rx.recv().expect("first request");
    let second = request_rx.recv().expect("second request");
    assert_eq!(first.0.as_deref(), Some("AgentAssertion stale-envelope"));
    assert_eq!(first.1.as_deref(), Some("true"));
    assert!(second
        .0
        .as_deref()
        .is_some_and(|value| value.starts_with("AgentAssertion ")));
    assert_eq!(second.1.as_deref(), Some("true"));
    match decision {
        PostRetryFlowDecision::RespondUpstream(response) => {
            assert_eq!(response.status().as_u16(), 200)
        }
        _ => panic!("expected recovered upstream response"),
    }
}

#[test]
fn chatgpt_responses_400_retries_same_path_without_session_headers() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-responses-400-stateless", now);
    let mut token = build_token(account.id.as_str(), now);
    let auth_token = token.access_token.clone();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let canonical_url = format!("{addr}/backend-api/codex/responses");
    let legacy_url = format!("{addr}/backend-api/codex/v1/responses");
    let (request_tx, request_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        for status in [400u16, 200u16] {
            let mut request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive upstream request")
                .expect("request present");
            let captured = (
                request.url().to_string(),
                captured_session_headers(&request),
            );
            request_tx.send(captured).expect("capture request");
            let mut body = Vec::new();
            std::io::Read::read_to_end(request.as_reader(), &mut body).expect("read request body");
            request
                .respond(
                    Response::from_string(if status == 400 {
                        r#"{"detail":"canonical bad request"}"#
                    } else {
                        r#"{"ok":true}"#
                    })
                    .with_status_code(StatusCode(status)),
                )
                .expect("respond request");
        }
    });

    let client = reqwest::Client::new();
    let incoming_headers = codex_session_headers();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
    };
    let body = Bytes::from_static(
        br#"{"model":"gpt-5.5","input":"hello","prompt_cache_key":"thread-current"}"#,
    );
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        canonical_url.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        auth_token.as_str(),
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial request");

    let authorization = oauth_authorization(auth_token.as_str());
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        canonical_url.as_str(),
        Some(legacy_url.as_str()),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        &authorization,
        &account,
        &mut token,
        None,
        false,
        false,
        false,
        false,
        false,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    let first = request_rx.recv().expect("first captured request");
    let second = request_rx.recv().expect("second captured request");
    assert_eq!(first.0, "/backend-api/codex/responses");
    assert_eq!(second.0, first.0);
    assert_eq!(first.1, vec![true, true, true, true, true]);
    assert_eq!(second.1, vec![false, false, false, false, false]);
    match decision {
        PostRetryFlowDecision::RespondUpstream(resp) => assert_eq!(resp.status(), 200),
        _ => panic!("unexpected decision"),
    }
}

#[test]
fn chatgpt_responses_failed_stateless_retry_keeps_original_400() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-responses-400-preserve", now);
    let mut token = build_token(account.id.as_str(), now);
    let auth_token = token.access_token.clone();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let canonical_url = format!("{addr}/backend-api/codex/responses");
    let legacy_url = format!("{addr}/backend-api/codex/v1/responses");
    let (path_tx, path_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        for status in [400u16, 404u16] {
            let mut request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive upstream request")
                .expect("request present");
            path_tx
                .send(request.url().to_string())
                .expect("capture request path");
            let mut body = Vec::new();
            std::io::Read::read_to_end(request.as_reader(), &mut body).expect("read request body");
            request
                .respond(
                    Response::from_string(if status == 400 {
                        r#"{"detail":"canonical bad request"}"#
                    } else {
                        r#"{"detail":"Not Found"}"#
                    })
                    .with_status_code(StatusCode(status)),
                )
                .expect("respond request");
        }
    });

    let client = reqwest::Client::new();
    let incoming_headers = codex_session_headers();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
    };
    let body = Bytes::from_static(
        br#"{"model":"gpt-5.5","input":"hello","prompt_cache_key":"thread-current"}"#,
    );
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        canonical_url.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        auth_token.as_str(),
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial request");

    let authorization = oauth_authorization(auth_token.as_str());
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        canonical_url.as_str(),
        Some(legacy_url.as_str()),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        &authorization,
        &account,
        &mut token,
        None,
        false,
        false,
        false,
        false,
        false,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    assert_eq!(
        path_rx.iter().collect::<Vec<_>>(),
        vec![
            "/backend-api/codex/responses".to_string(),
            "/backend-api/codex/responses".to_string(),
        ]
    );
    match decision {
        PostRetryFlowDecision::RespondUpstream(resp) => assert_eq!(resp.status(), 400),
        _ => panic!("unexpected decision"),
    }
}

#[test]
fn chatgpt_responses_stripped_candidate_does_not_retry_without_session_headers_again() {
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-responses-already-stateless", now);
    let mut token = build_token(account.id.as_str(), now);
    let auth_token = token.access_token.clone();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let canonical_url = format!("{addr}/backend-api/codex/responses");
    let hit_count = Arc::new(AtomicUsize::new(0));
    let hit_count_thread = Arc::clone(&hit_count);
    let join = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive initial request")
            .expect("initial request present");
        let mut body = Vec::new();
        std::io::Read::read_to_end(request.as_reader(), &mut body)
            .expect("read initial request body");
        hit_count_thread.fetch_add(1, Ordering::SeqCst);
        request
            .respond(
                Response::from_string(r#"{"detail":"canonical bad request"}"#)
                    .with_status_code(StatusCode(400)),
            )
            .expect("respond initial request");

        if let Some(mut request) = server
            .recv_timeout(Duration::from_millis(500))
            .expect("check for unexpected retry")
        {
            let mut body = Vec::new();
            std::io::Read::read_to_end(request.as_reader(), &mut body)
                .expect("read unexpected retry body");
            hit_count_thread.fetch_add(1, Ordering::SeqCst);
            request
                .respond(Response::from_string(r#"{"ok":true}"#).with_status_code(StatusCode(200)))
                .expect("respond unexpected retry");
        }
    });

    let client = reqwest::Client::new();
    let incoming_headers = codex_session_headers();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
    };
    let body = Bytes::from_static(
        br#"{"model":"gpt-5.5","input":"hello","prompt_cache_key":"thread-current"}"#,
    );
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        canonical_url.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        auth_token.as_str(),
        &account,
        true,
    ))
    .expect("gateway async test runtime")
    .expect("send stripped candidate request");

    let authorization = oauth_authorization(auth_token.as_str());
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        canonical_url.as_str(),
        None,
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        &authorization,
        &account,
        &mut token,
        None,
        true,
        false,
        false,
        false,
        false,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
    match decision {
        PostRetryFlowDecision::RespondUpstream(resp) => assert_eq!(resp.status(), 400),
        _ => panic!("unexpected decision"),
    }
}

/// 函数 `retries_server_error_once_before_final_decision`
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
fn retries_server_error_once_before_final_decision() {
    let _guard = crate::test_env_guard();
    std::env::remove_var("CODEXMANAGER_UPSTREAM_PROXY_URL");
    std::env::remove_var("CODEXMANAGER_PROXY_LIST");
    crate::gateway::reload_runtime_config_from_env();

    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-500-retry", now);
    let mut token = build_token(account.id.as_str(), now);
    let auth_token = token.access_token.clone();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let hit_count = Arc::new(AtomicUsize::new(0));
    let hit_count_thread = Arc::clone(&hit_count);
    let join = thread::spawn(move || {
        for (index, status) in [500u16, 200u16].into_iter().enumerate() {
            let mut request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive upstream request")
                .expect("request present");
            let mut body = Vec::new();
            let _ = request
                .as_reader()
                .read_to_end(&mut body)
                .expect("read request body");
            hit_count_thread.fetch_add(1, Ordering::SeqCst);
            let response = Response::from_string(if index == 0 { "first" } else { "second" })
                .with_status_code(StatusCode(status));
            request.respond(response).expect("respond");
        }
    });

    let client = reqwest::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        addr.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        auth_token.as_str(),
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial request");

    let authorization = oauth_authorization(auth_token.as_str());
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        addr.as_str(),
        "/v1/responses",
        addr.as_str(),
        None,
        None,
        request_ctx,
        &incoming_headers,
        &body,
        false,
        &authorization,
        &account,
        &mut token,
        None,
        false,
        false,
        false,
        false,
        true,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    assert_eq!(hit_count.load(Ordering::SeqCst), 2);
    match decision {
        PostRetryFlowDecision::RespondUpstream(resp) => assert_eq!(resp.status(), 200),
        _ => panic!("unexpected decision"),
    }
}

#[test]
fn chatgpt_challenge_on_last_candidate_retries_without_same_account_failover() {
    let _guard = crate::test_env_guard();
    std::env::remove_var("CODEXMANAGER_UPSTREAM_PROXY_URL");
    std::env::remove_var("CODEXMANAGER_PROXY_LIST");
    crate::gateway::reload_runtime_config_from_env();

    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-challenge-recover", now);
    let mut token = build_token(account.id.as_str(), now);
    let auth_token = token.access_token.clone();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let hit_count = Arc::new(AtomicUsize::new(0));
    let hit_count_thread = Arc::clone(&hit_count);
    let join = thread::spawn(move || {
        for index in 0..2 {
            let mut request = server
                .recv_timeout(Duration::from_secs(2))
                .expect("receive upstream request")
                .expect("request present");
            let mut body = Vec::new();
            std::io::Read::read_to_end(request.as_reader(), &mut body).expect("read request body");
            hit_count_thread.fetch_add(1, Ordering::SeqCst);
            let response = if index == 0 {
                Response::from_string("<html><title>Just a moment...</title><body>cf</body></html>")
                    .with_status_code(StatusCode(403))
                    .with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"text/html; charset=utf-8"[..],
                        )
                        .expect("content type header"),
                    )
            } else {
                Response::from_string("{\"ok\":true}").with_status_code(StatusCode(200))
            };
            request.respond(response).expect("respond request");
        }
    });

    let client = reqwest::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        addr.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        true,
        auth_token.as_str(),
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial request");

    let authorization = oauth_authorization(auth_token.as_str());
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        addr.as_str(),
        None,
        None,
        request_ctx,
        &incoming_headers,
        &body,
        true,
        &authorization,
        &account,
        &mut token,
        None,
        false,
        false,
        false,
        false,
        false,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    assert_eq!(hit_count.load(Ordering::SeqCst), 2);
    match decision {
        PostRetryFlowDecision::RespondUpstream(resp) => assert_eq!(resp.status(), 200),
        _ => panic!("unexpected decision"),
    }
}

#[test]
fn chatgpt_cloudflare_challenge_directly_failovers_without_same_account_retry() {
    let _guard = crate::test_env_guard();
    std::env::remove_var("CODEXMANAGER_UPSTREAM_PROXY_URL");
    std::env::remove_var("CODEXMANAGER_PROXY_LIST");
    crate::gateway::reload_runtime_config_from_env();

    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-challenge-retry", now);
    let mut token = build_token(account.id.as_str(), now);
    let auth_token = token.access_token.clone();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let hit_count = Arc::new(AtomicUsize::new(0));
    let hit_count_thread = Arc::clone(&hit_count);
    let join = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive upstream request")
            .expect("request present");
        let mut body = Vec::new();
        std::io::Read::read_to_end(request.as_reader(), &mut body).expect("read request body");
        hit_count_thread.fetch_add(1, Ordering::SeqCst);
        let response =
            Response::from_string("<html><title>Just a moment...</title><body>cf</body></html>")
                .with_status_code(StatusCode(403));
        let response = response.with_header(
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                .expect("content type header"),
        );
        request.respond(response).expect("respond first");
    });

    let client = reqwest::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        addr.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        true,
        auth_token.as_str(),
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial request");

    let authorization = oauth_authorization(auth_token.as_str());
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        addr.as_str(),
        "/v1/responses",
        addr.as_str(),
        None,
        None,
        request_ctx,
        &incoming_headers,
        &body,
        true,
        &authorization,
        &account,
        &mut token,
        None,
        false,
        false,
        false,
        false,
        true,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
    match decision {
        PostRetryFlowDecision::Failover => {}
        _ => panic!("unexpected decision"),
    }
}

#[test]
fn cloudflare_cf_ray_directly_failovers_without_same_account_retry() {
    let _guard = crate::test_env_guard();
    std::env::remove_var("CODEXMANAGER_UPSTREAM_PROXY_URL");
    std::env::remove_var("CODEXMANAGER_PROXY_LIST");
    crate::gateway::reload_runtime_config_from_env();

    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-challenge-cf-ray", now);
    let mut token = build_token(account.id.as_str(), now);
    let auth_token = token.access_token.clone();
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let server = Server::http("127.0.0.1:0").expect("start server");
    let addr = format!("http://{}", server.server_addr());
    let hit_count = Arc::new(AtomicUsize::new(0));
    let hit_count_thread = Arc::clone(&hit_count);
    let join = thread::spawn(move || {
        let mut request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive upstream request")
            .expect("request present");
        let mut body = Vec::new();
        std::io::Read::read_to_end(request.as_reader(), &mut body).expect("read request body");
        hit_count_thread.fetch_add(1, Ordering::SeqCst);
        let response =
            Response::from_string("{\"error\":\"challenge\"}").with_status_code(StatusCode(403));
        let response = response.with_header(
            tiny_http::Header::from_bytes(&b"cf-ray"[..], &b"ray-postprocess"[..])
                .expect("cf-ray header"),
        );
        request.respond(response).expect("respond first");
    });

    let client = reqwest::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        addr.as_str(),
        None,
        request_ctx,
        &incoming_headers,
        &body,
        true,
        auth_token.as_str(),
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial request");

    let authorization = oauth_authorization(auth_token.as_str());
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        addr.as_str(),
        "/v1/responses",
        addr.as_str(),
        None,
        None,
        request_ctx,
        &incoming_headers,
        &body,
        true,
        &authorization,
        &account,
        &mut token,
        None,
        false,
        false,
        false,
        false,
        true,
        upstream,
        |_, _, _| {},
    ))
    .expect("gateway async test runtime");

    join.join().expect("join server");
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
    match decision {
        PostRetryFlowDecision::Failover => {}
        _ => panic!("unexpected decision"),
    }
}
