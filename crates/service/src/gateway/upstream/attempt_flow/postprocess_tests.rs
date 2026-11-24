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
        replay_guard: None,
        original_body: None,
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
        replay_guard: None,
        original_body: None,
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
        replay_guard: None,
        original_body: None,
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
        replay_guard: None,
        original_body: None,
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

struct RequestCompressionFixture {
    original: Option<std::ffi::OsString>,
}

impl RequestCompressionFixture {
    fn enable() -> Self {
        let original = std::env::var_os("CODEXMANAGER_ENABLE_REQUEST_COMPRESSION");
        std::env::set_var("CODEXMANAGER_ENABLE_REQUEST_COMPRESSION", "1");
        crate::gateway::reload_runtime_config_from_env();
        Self { original }
    }
}

impl Drop for RequestCompressionFixture {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var("CODEXMANAGER_ENABLE_REQUEST_COMPRESSION", value),
            None => std::env::remove_var("CODEXMANAGER_ENABLE_REQUEST_COMPRESSION"),
        }
        crate::gateway::reload_runtime_config_from_env();
    }
}

fn compressed_responses_bad_request_case(accept_plain_retry: bool) {
    let _environment = crate::test_env_guard();
    let _compression = RequestCompressionFixture::enable();
    let storage = Storage::open_in_memory().expect("open storage");
    storage.init().expect("init storage");
    let now = now_ts();
    let account = build_account("acc-compressed-responses-400", now);
    let mut token = build_token(account.id.as_str(), now);
    storage.insert_account(&account).expect("insert account");
    storage.insert_token(&token).expect("insert token");

    let input = serde_json::json!({
        "model":"gpt-6-astra",
        "stream":true,
        "store":false,
        "prompt_cache_key":"thread-compressed-context",
        "previous_response_id":"resp-before-compaction",
        "input":[
            {"type":"compaction","id":"cmp-original","encrypted_content":"opaque-compacted-history"},
            {"type":"reasoning","id":"rs-original","encrypted_content":"opaque-reasoning","summary":[]},
            {"type":"custom_tool_call","id":"ct-original","call_id":"call-original","name":"exec","input":"original command"},
            {"type":"custom_tool_call_output","call_id":"call-original","output":"complete original result"},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"继续原来的会话"}]}
        ]
    });
    let body = Bytes::from(serde_json::to_vec(&input).expect("serialize original request"));
    let expected_body = body.clone();
    let server = Server::http("127.0.0.1:0").expect("start local server");
    // The path selects the production ChatGPT compression policy, but the host
    // and explicit no_proxy client keep every fixture request on loopback.
    let url = format!(
        "http://{}/chatgpt.com/backend-api/codex/responses",
        server.server_addr()
    );
    let (capture_tx, capture_rx) = mpsc::channel();
    let join = thread::spawn(move || {
        for attempt in 0..2 {
            let Some(mut request) = server
                .recv_timeout(Duration::from_secs(3))
                .expect("receive compatibility attempt")
            else {
                break;
            };
            let encoding = request_header_value(&request, "Content-Encoding");
            let headers = (
                request.url().to_string(),
                captured_session_headers(&request),
                request_header_value(&request, "Authorization"),
                request_header_value(&request, "ChatGPT-Account-ID"),
            );
            let mut received = Vec::new();
            std::io::Read::read_to_end(request.as_reader(), &mut received)
                .expect("read compatibility request body");
            let plain = if encoding.as_deref() == Some("zstd") {
                zstd::stream::decode_all(std::io::Cursor::new(&received))
                    .expect("decode initial zstd request")
            } else {
                received
            };
            let accepts_request = attempt == 1
                && accept_plain_retry
                && encoding.is_none()
                && plain == expected_body.as_ref();
            capture_tx
                .send((headers, encoding, plain))
                .expect("capture compatibility request");
            let (status, response_body, content_type) = if accepts_request {
                (
                    200,
                    "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-recovered\"}}\n\n",
                    "text/event-stream",
                )
            } else if attempt == 0 {
                (
                    400,
                    r#"{"detail":"original bad request"}"#,
                    "application/json",
                )
            } else {
                (
                    400,
                    r#"{"detail":"retry still invalid"}"#,
                    "application/json",
                )
            };
            request
                .respond(
                    Response::from_string(response_body)
                        .with_status_code(StatusCode(status))
                        .with_header(
                            tiny_http::Header::from_bytes("Content-Type", content_type)
                                .expect("response content type"),
                        ),
                )
                .expect("respond compatibility attempt");
        }
        if let Some(mut request) = server
            .recv_timeout(Duration::from_millis(250))
            .expect("check compatibility retry bound")
        {
            let mut body = Vec::new();
            std::io::Read::read_to_end(request.as_reader(), &mut body)
                .expect("read unexpected third attempt");
            capture_tx
                .send(((request.url().to_string(), vec![], None, None), None, body))
                .expect("capture unexpected third attempt");
            request
                .respond(
                    Response::from_string("unexpected extra attempt")
                        .with_status_code(StatusCode(400)),
                )
                .expect("reject unexpected third attempt");
        }
    });

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("build loopback-only fixture client");
    let incoming_headers = codex_session_headers();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
        replay_guard: None,
        original_body: Some(body.as_ref()),
    };
    let authorization = oauth_authorization(&token.access_token);
    let upstream = crate::gateway::run_upstream_io(super::super::transport::send_upstream_request(
        &client,
        &reqwest::Method::POST,
        &url,
        None,
        request_ctx,
        &incoming_headers,
        &body,
        true,
        &authorization.value,
        &account,
        false,
    ))
    .expect("gateway async test runtime")
    .expect("send initial compressed request");
    let decision = crate::gateway::run_upstream_io(process_upstream_post_retry_flow(
        &client,
        &storage,
        &reqwest::Method::POST,
        "https://chatgpt.com/backend-api/codex",
        "/v1/responses",
        &url,
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
    let final_status = match decision {
        PostRetryFlowDecision::RespondUpstream(response) => {
            let status = response.status().as_u16();
            let (delivered, _) = crate::gateway::run_upstream_io(response.into_buffered_async())
                .expect("gateway async test runtime")
                .expect("read final response");
            if !accept_plain_retry {
                assert_eq!(delivered.as_ref(), br#"{"detail":"original bad request"}"#);
            }
            status
        }
        _ => panic!("compatibility retry must return an upstream response"),
    };
    join.join().expect("join compatibility server");
    let captured: Vec<_> = capture_rx.into_iter().collect();
    assert_eq!(
        captured.len(),
        2,
        "one initial request and at most one compatibility retry"
    );
    assert_eq!(captured[0].1.as_deref(), Some("zstd"));
    assert_eq!(
        captured[1].1, None,
        "a compressed 400 retry must try plain JSON"
    );
    assert_eq!(
        captured[0].0 .0, captured[1].0 .0,
        "retry must use the same path"
    );
    assert_eq!(captured[0].0 .1, vec![true; 5]);
    assert_eq!(captured[1].0 .1, vec![false; 5]);
    for (headers, _, plain) in &captured {
        assert_eq!(
            plain.as_slice(),
            body.as_ref(),
            "compaction and valid tool context must survive byte-for-byte"
        );
        assert_eq!(headers.2.as_deref(), Some("Bearer access-token"));
        assert_eq!(headers.3.as_deref(), Some("chatgpt-account"));
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(plain).unwrap(),
            input
        );
    }
    assert_eq!(final_status, if accept_plain_retry { 200 } else { 400 });
}

#[test]
fn chatgpt_responses_compressed_400_retries_plain_without_losing_context() {
    compressed_responses_bad_request_case(true);
}

#[test]
fn chatgpt_responses_compressed_payload_400_keeps_original_error_and_retry_bound() {
    compressed_responses_bad_request_case(false);
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
        replay_guard: None,
        original_body: None,
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
        replay_guard: None,
        original_body: None,
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
    let _loopback = crate::loopback_proxy_test::LoopbackProxyGuard::enter(&_guard);
    let fixture_started_at = std::time::Instant::now();

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
        let recv_started_at = std::time::Instant::now();
        let request = server
            .recv_timeout(Duration::from_secs(2))
            .expect("receive upstream request");
        eprintln!(
            "r03_fixture_timing phase=mock_receive elapsed_ms={} since_fixture_ms={} received={}",
            recv_started_at.elapsed().as_millis(),
            fixture_started_at.elapsed().as_millis(),
            request.is_some()
        );
        let mut request = request.expect("request present");
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

    let client_started_at = std::time::Instant::now();
    let client = reqwest::Client::new();
    eprintln!(
        "r03_fixture_timing phase=client_build elapsed_ms={} since_fixture_ms={}",
        client_started_at.elapsed().as_millis(),
        fixture_started_at.elapsed().as_millis()
    );
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
        is_fedramp: false,
        capture: None,
        replay_guard: None,
        original_body: None,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let send_started_at = std::time::Instant::now();
    eprintln!(
        "r03_fixture_timing phase=send_start since_fixture_ms={}",
        fixture_started_at.elapsed().as_millis()
    );
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
    assert_eq!(upstream.status(), reqwest::StatusCode::FORBIDDEN);
    eprintln!(
        "r03_fixture_timing phase=send_and_runtime elapsed_ms={} since_fixture_ms={} status={}",
        send_started_at.elapsed().as_millis(),
        fixture_started_at.elapsed().as_millis(),
        upstream.status().as_u16()
    );

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
        replay_guard: None,
        original_body: None,
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
