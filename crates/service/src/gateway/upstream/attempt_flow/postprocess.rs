use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use std::time::Instant;

use crate::account_status::mark_account_unavailable_for_refresh_token_error;
use crate::usage_token_refresh::token_refresh_ahead_secs;

use super::super::support::backoff;
use super::super::support::outcome::{decide_upstream_outcome, UpstreamOutcomeDecision};
use super::super::support::retry::{retry_with_alternate_path, AltPathRetryResult};
use super::super::GatewayUpstreamResponse;
use super::fallback_branch::{handle_openai_fallback_branch, FallbackBranchResult};
use super::stateless_retry::{retry_stateless_then_optional_alt, StatelessRetryResult};
use super::transport::UpstreamRequestContext;

fn first_header_value<'a>(headers: &'a reqwest::header::HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn should_treat_as_challenge_for_retry(
    status: reqwest::StatusCode,
    upstream_content_type: Option<&reqwest::header::HeaderValue>,
    upstream_cf_ray: Option<&str>,
) -> bool {
    if !matches!(status.as_u16(), 401 | 403) {
        return false;
    }
    super::super::super::is_upstream_challenge_response(status.as_u16(), upstream_content_type)
        || upstream_cf_ray.is_some()
}

fn should_failover_immediately_for_cloudflare(
    status: reqwest::StatusCode,
    upstream_content_type: Option<&reqwest::header::HeaderValue>,
    upstream_cf_ray: Option<&str>,
    has_more_candidates: bool,
) -> bool {
    has_more_candidates
        && should_treat_as_challenge_for_retry(status, upstream_content_type, upstream_cf_ray)
}

fn challenge_cooldown_reason(protocol_type: &str) -> super::super::super::CooldownReason {
    if protocol_type == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE {
        return super::super::super::CooldownReason::AnthropicChallenge;
    }
    super::super::super::CooldownReason::Challenge
}

/// 函数 `try_refresh_chatgpt_access_token`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - storage: 参数 storage
/// - upstream_base: 参数 upstream_base
/// - account: 参数 account
/// - token: 参数 token
///
/// # 返回
/// 返回函数执行结果
fn try_refresh_chatgpt_access_token(
    storage: &Storage,
    upstream_base: &str,
    account: &Account,
    token: &mut Token,
) -> Result<Option<String>, String> {
    if super::super::super::is_openai_api_base(upstream_base) {
        return Ok(None);
    }
    if token.refresh_token.trim().is_empty() {
        return Ok(None);
    }
    let issuer = if account.issuer.trim().is_empty() {
        super::super::super::runtime_config::token_exchange_default_issuer()
    } else {
        account.issuer.clone()
    };
    let client_id = super::super::super::runtime_config::token_exchange_client_id();
    crate::usage_token_refresh::refresh_and_persist_access_token(
        storage,
        token,
        issuer.as_str(),
        client_id.as_str(),
        token_refresh_ahead_secs(),
    )?;
    let refreshed = token.access_token.trim();
    if refreshed.is_empty() {
        return Err("refreshed chatgpt access token is empty".to_string());
    }
    Ok(Some(refreshed.to_string()))
}

/// 函数 `retry_upstream_server_error_once`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - client: 参数 client
/// - method: 参数 method
/// - url: 参数 url
/// - request_deadline: 参数 request_deadline
/// - request_ctx: 参数 request_ctx
/// - incoming_headers: 参数 incoming_headers
/// - body: 参数 body
/// - is_stream: 参数 is_stream
/// - auth_token: 参数 auth_token
/// - account: 参数 account
/// - strip_session_affinity: 参数 strip_session_affinity
/// - debug: 参数 debug
/// - status: 参数 status
///
/// # 返回
/// 返回函数执行结果
#[allow(clippy::too_many_arguments)]
fn retry_upstream_server_error_once(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    url: &str,
    request_deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    debug: bool,
    status: reqwest::StatusCode,
) -> Result<Option<GatewayUpstreamResponse>, ()> {
    if status.as_u16() != 500 {
        return Ok(None);
    }
    if debug {
        log::warn!(
            "event=gateway_upstream_server_error_retry path={} status={} account_id={}",
            request_ctx.request_path,
            status.as_u16(),
            account.id
        );
    }
    if !backoff::sleep_with_exponential_jitter(
        std::time::Duration::from_millis(120),
        std::time::Duration::from_millis(900),
        1,
        request_deadline,
    ) {
        return Err(());
    }

    match super::transport::send_upstream_request(
        client,
        method,
        url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        auth_token,
        account,
        strip_session_affinity,
    ) {
        Ok(resp) => Ok(Some(resp)),
        Err(err) => {
            log::warn!(
                "event=gateway_upstream_server_error_retry_error path={} status=502 account_id={} err={}",
                request_ctx.request_path,
                account.id,
                err
            );
            Ok(None)
        }
    }
}

/// 函数 `retry_chatgpt_challenge_without_compression`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-04
///
/// # 参数
/// - upstream_content_type: 参数 upstream_content_type
///
/// # 返回
/// 返回函数执行结果
#[allow(clippy::too_many_arguments)]
fn retry_chatgpt_challenge_without_compression(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    upstream_base: &str,
    url: &str,
    request_deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    debug: bool,
    status: reqwest::StatusCode,
    upstream_content_type: Option<&reqwest::header::HeaderValue>,
    upstream_cf_ray: Option<&str>,
) -> Result<Option<GatewayUpstreamResponse>, ()> {
    if !super::super::config::is_chatgpt_backend_base(upstream_base) {
        return Ok(None);
    }
    if !is_stream || !request_ctx.request_path.starts_with("/v1/responses") {
        return Ok(None);
    }
    if !should_treat_as_challenge_for_retry(status, upstream_content_type, upstream_cf_ray) {
        return Ok(None);
    }

    if debug {
        log::warn!(
            "event=gateway_chatgpt_challenge_retry_without_compression path={} status={} account_id={} upstream_url={}",
            request_ctx.request_path,
            status.as_u16(),
            account.id,
            url
        );
    }
    match super::transport::send_upstream_request_without_compression(
        client,
        method,
        url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        auth_token,
        account,
        strip_session_affinity,
    ) {
        Ok(resp) => Ok(resp.status().is_success().then_some(resp)),
        Err(err) => {
            log::warn!(
                "event=gateway_chatgpt_challenge_retry_without_compression_error path={} status=502 account_id={} err={}",
                request_ctx.request_path,
                account.id,
                err
            );
            Ok(None)
        }
    }
}

pub(in crate::gateway::upstream) enum PostRetryFlowDecision {
    Failover,
    Terminal { status_code: u16, message: String },
    RespondUpstream(GatewayUpstreamResponse),
}

/// 函数 `process_upstream_post_retry_flow`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
#[allow(clippy::too_many_arguments)]
pub(in crate::gateway::upstream) fn process_upstream_post_retry_flow<F>(
    client: &reqwest::blocking::Client,
    storage: &Storage,
    method: &reqwest::Method,
    upstream_base: &str,
    path: &str,
    url: &str,
    url_alt: Option<&str>,
    request_deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    auth_token: &str,
    account: &Account,
    token: &mut Token,
    upstream_fallback_base: Option<&str>,
    strip_session_affinity: bool,
    debug: bool,
    allow_openai_fallback: bool,
    disable_challenge_stateless_retry: bool,
    has_more_candidates: bool,
    mut upstream: GatewayUpstreamResponse,
    mut log_gateway_result: F,
) -> PostRetryFlowDecision
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let mut current_auth_token = auth_token.to_string();
    let mut status = upstream.status();
    let mut upstream_content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE);
    let mut upstream_cf_ray = first_header_value(upstream.headers(), "cf-ray");
    if !status.is_success() {
        log::warn!(
            "gateway upstream non-success: status={}, account_id={}",
            status,
            account.id
        );
    }

    if should_failover_immediately_for_cloudflare(
        status,
        upstream_content_type,
        upstream_cf_ray,
        has_more_candidates,
    ) {
        super::super::super::mark_account_cooldown(
            &account.id,
            challenge_cooldown_reason(request_ctx.protocol_type),
        );
        log_gateway_result(
            Some(url),
            status.as_u16(),
            Some("upstream challenge blocked"),
        );
        return PostRetryFlowDecision::Failover;
    }

    if status.as_u16() == 401 {
        match try_refresh_chatgpt_access_token(storage, upstream_base, account, token) {
            Ok(Some(refreshed_auth_token)) => {
                current_auth_token = refreshed_auth_token;
                if debug {
                    log::warn!(
                        "event=gateway_upstream_unauthorized_refresh_retry path={} account_id={}",
                        path,
                        account.id
                    );
                }
                match super::transport::send_upstream_request(
                    client,
                    method,
                    url,
                    request_deadline,
                    request_ctx,
                    incoming_headers,
                    body,
                    is_stream,
                    current_auth_token.as_str(),
                    account,
                    strip_session_affinity,
                ) {
                    Ok(resp) => {
                        upstream = resp;
                        status = upstream.status();
                        upstream_content_type =
                            upstream.headers().get(reqwest::header::CONTENT_TYPE);
                        upstream_cf_ray = first_header_value(upstream.headers(), "cf-ray");
                    }
                    Err(err) => {
                        log::warn!(
                            "event=gateway_upstream_unauthorized_refresh_retry_error path={} status=502 account_id={} err={}",
                            path,
                            account.id,
                            err
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                let _ =
                    mark_account_unavailable_for_refresh_token_error(storage, &account.id, &err);
                log::warn!(
                    "event=gateway_upstream_unauthorized_refresh_failed path={} account_id={} err={}",
                    path,
                    account.id,
                    err
                );
            }
        }
    }

    if let Some(alt_url) = url_alt {
        match retry_with_alternate_path(
            client,
            method,
            Some(alt_url),
            request_deadline,
            request_ctx,
            incoming_headers,
            body,
            is_stream,
            current_auth_token.as_str(),
            account,
            strip_session_affinity,
            status,
            debug,
            has_more_candidates,
            &mut log_gateway_result,
        ) {
            AltPathRetryResult::NotTriggered => {}
            AltPathRetryResult::Upstream(resp) => {
                upstream = resp;
                status = upstream.status();
                upstream_content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE);
                upstream_cf_ray = first_header_value(upstream.headers(), "cf-ray");
            }
            AltPathRetryResult::Failover => {
                return PostRetryFlowDecision::Failover;
            }
            AltPathRetryResult::Terminal {
                status_code,
                message,
            } => {
                return PostRetryFlowDecision::Terminal {
                    status_code,
                    message,
                };
            }
        }
    }

    match retry_upstream_server_error_once(
        client,
        method,
        url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        current_auth_token.as_str(),
        account,
        strip_session_affinity,
        debug,
        status,
    ) {
        Ok(Some(resp)) => {
            upstream = resp;
            status = upstream.status();
            upstream_content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE);
            upstream_cf_ray = first_header_value(upstream.headers(), "cf-ray");
        }
        Ok(None) => {}
        Err(()) => {
            return PostRetryFlowDecision::Terminal {
                status_code: 504,
                message: "upstream total timeout exceeded".to_string(),
            };
        }
    }

    if should_treat_as_challenge_for_retry(status, upstream_content_type, upstream_cf_ray) {
        match retry_chatgpt_challenge_without_compression(
            client,
            method,
            upstream_base,
            url,
            request_deadline,
            request_ctx,
            incoming_headers,
            body,
            is_stream,
            current_auth_token.as_str(),
            account,
            strip_session_affinity,
            debug,
            status,
            upstream_content_type,
            upstream_cf_ray,
        ) {
            Ok(Some(resp)) => {
                upstream = resp;
                status = upstream.status();
                upstream_content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE);
                upstream_cf_ray = first_header_value(upstream.headers(), "cf-ray");
            }
            Ok(None) => {}
            Err(()) => {
                return PostRetryFlowDecision::Terminal {
                    status_code: 504,
                    message: "upstream total timeout exceeded".to_string(),
                };
            }
        }
    }

    if !super::super::config::is_chatgpt_backend_base(upstream_base)
        && !should_treat_as_challenge_for_retry(status, upstream_content_type, upstream_cf_ray)
    {
        match retry_stateless_then_optional_alt(
            client,
            method,
            url,
            url_alt,
            request_deadline,
            request_ctx,
            incoming_headers,
            body,
            is_stream,
            current_auth_token.as_str(),
            account,
            strip_session_affinity,
            status,
            debug,
            disable_challenge_stateless_retry,
        ) {
            StatelessRetryResult::NotTriggered => {}
            StatelessRetryResult::Upstream(resp) => {
                upstream = resp;
                status = upstream.status();
                upstream_content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE);
                upstream_cf_ray = first_header_value(upstream.headers(), "cf-ray");
            }
            StatelessRetryResult::Terminal {
                status_code,
                message,
            } => {
                return PostRetryFlowDecision::Terminal {
                    status_code,
                    message,
                };
            }
        }
    }

    if !(path == "/v1/responses/compact" || path.starts_with("/v1/responses/compact?"))
        && !should_treat_as_challenge_for_retry(status, upstream_content_type, upstream_cf_ray)
    {
        // 中文注释：compact 失败直接返回自身的结构化错误，不再进入通用 fallback。
        // 主流程 fallback 只覆盖首跳响应，这里补齐“重试后仍 challenge/401/403/429”场景。
        match handle_openai_fallback_branch(
            client,
            storage,
            method,
            incoming_headers,
            body,
            is_stream,
            upstream_base,
            path,
            upstream_fallback_base,
            account,
            token,
            strip_session_affinity,
            debug,
            allow_openai_fallback,
            status,
            upstream_content_type,
            has_more_candidates,
            &mut log_gateway_result,
        ) {
            FallbackBranchResult::NotTriggered => {}
            FallbackBranchResult::RespondUpstream(resp) => {
                return PostRetryFlowDecision::RespondUpstream(resp);
            }
            FallbackBranchResult::Failover => {
                return PostRetryFlowDecision::Failover;
            }
            FallbackBranchResult::Terminal {
                status_code,
                message,
            } => {
                return PostRetryFlowDecision::Terminal {
                    status_code,
                    message,
                };
            }
        }
    }

    if request_ctx.protocol_type == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE
        && should_treat_as_challenge_for_retry(status, upstream_content_type, upstream_cf_ray)
    {
        super::super::super::mark_account_cooldown(
            &account.id,
            challenge_cooldown_reason(request_ctx.protocol_type),
        );
    }

    match decide_upstream_outcome(
        storage,
        &account.id,
        status,
        upstream_content_type,
        url,
        has_more_candidates,
        &mut log_gateway_result,
    ) {
        UpstreamOutcomeDecision::Failover => PostRetryFlowDecision::Failover,
        UpstreamOutcomeDecision::RespondUpstream => {
            PostRetryFlowDecision::RespondUpstream(upstream)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::IncomingHeaderSnapshot;
    use codexmanager_core::storage::{now_ts, Account, Storage, Token};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use tiny_http::{Response, Server, StatusCode};

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    struct RuntimeConfigReloadGuard;

    impl Drop for RuntimeConfigReloadGuard {
        fn drop(&mut self) {
            let _ = std::panic::catch_unwind(crate::gateway::reload_runtime_config_from_env);
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
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
        let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
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
                std::io::Read::read_to_end(request.as_reader(), &mut body)
                    .expect("read request body");
                hit_count_thread.fetch_add(1, Ordering::SeqCst);
                let response = if index == 0 {
                    Response::from_string(
                        "<html><title>Just a moment...</title><body>cf</body></html>",
                    )
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
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
        let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
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
            let response = Response::from_string(
                "<html><title>Just a moment...</title><body>cf</body></html>",
            )
            .with_status_code(StatusCode(403));
            let response = response.with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/html; charset=utf-8"[..],
                )
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
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
        let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
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
            let response = Response::from_string("{\"error\":\"challenge\"}")
                .with_status_code(StatusCode(403));
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
}
