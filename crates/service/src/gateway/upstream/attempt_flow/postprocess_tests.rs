use super::*;
use crate::gateway::IncomingHeaderSnapshot;
use codexmanager_core::storage::{now_ts, Account, Storage, Token};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tiny_http::{Response, Server, StatusCode};

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

    let client = reqwest::blocking::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = super::super::transport::send_upstream_request(
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
    )
    .expect("send initial request");

    let decision = process_upstream_post_retry_flow(
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
        auth_token.as_str(),
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
    );

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

    let client = reqwest::blocking::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = super::super::transport::send_upstream_request(
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
    )
    .expect("send initial request");

    let decision = process_upstream_post_retry_flow(
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
        auth_token.as_str(),
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
    );

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

    let client = reqwest::blocking::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = super::super::transport::send_upstream_request(
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
    )
    .expect("send initial request");

    let decision = process_upstream_post_retry_flow(
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
        auth_token.as_str(),
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
    );

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

    let client = reqwest::blocking::Client::new();
    let incoming_headers = IncomingHeaderSnapshot::default();
    let request_ctx = UpstreamRequestContext {
        request_path: "/v1/responses",
        protocol_type: crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
    };
    let body = Bytes::from_static(br#"{"model":"gpt-5.3-codex","input":"hello"}"#);
    let upstream = super::super::transport::send_upstream_request(
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
    )
    .expect("send initial request");

    let decision = process_upstream_post_retry_flow(
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
        auth_token.as_str(),
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
    );

    join.join().expect("join server");
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);
    match decision {
        PostRetryFlowDecision::Failover => {}
        _ => panic!("unexpected decision"),
    }
}
