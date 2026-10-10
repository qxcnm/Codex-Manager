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
use super::primary_flow::PrimaryAuthorization;
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
async fn try_refresh_chatgpt_access_token(
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
    crate::usage_token_refresh::refresh_and_persist_access_token_async(
        storage,
        token,
        issuer.as_str(),
        client_id.as_str(),
        token_refresh_ahead_secs(),
    )
    .await?;
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
async fn retry_upstream_server_error_once(
    client: &reqwest::Client,
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
    if !backoff::sleep_with_exponential_jitter_async(
        std::time::Duration::from_millis(120),
        std::time::Duration::from_millis(900),
        1,
        request_deadline,
    )
    .await
    {
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
    )
    .await
    {
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
async fn retry_chatgpt_challenge_without_compression(
    client: &reqwest::Client,
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
    )
    .await
    {
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

fn should_retry_chatgpt_responses_bad_request(upstream_base: &str, url: &str, status: u16) -> bool {
    status == 400
        && super::super::config::is_chatgpt_backend_base(upstream_base)
        && reqwest::Url::parse(url).ok().is_some_and(|url| {
            url.path()
                .trim_end_matches('/')
                .to_ascii_lowercase()
                .ends_with("/backend-api/codex/responses")
        })
}

#[allow(clippy::too_many_arguments)]
async fn retry_chatgpt_responses_bad_request_without_session_headers(
    client: &reqwest::Client,
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
    debug: bool,
    status: reqwest::StatusCode,
) -> Option<GatewayUpstreamResponse> {
    if !should_retry_chatgpt_responses_bad_request(upstream_base, url, status.as_u16()) {
        return None;
    }
    if debug {
        log::warn!(
            "event=gateway_chatgpt_responses_retry_without_session_headers path={} status={} account_id={} upstream_url={}",
            request_ctx.request_path,
            status.as_u16(),
            account.id,
            url
        );
    }
    match super::transport::send_upstream_request_without_session_headers(
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
    )
    .await
    {
        Ok(response) if response.status().is_success() => Some(response),
        Ok(response) => {
            log::warn!(
                "event=gateway_chatgpt_responses_retry_without_session_headers_failed path={} original_status={} retry_status={} account_id={} upstream_url={}",
                request_ctx.request_path,
                status.as_u16(),
                response.status().as_u16(),
                account.id,
                url
            );
            None
        }
        Err(err) => {
            log::warn!(
                "event=gateway_chatgpt_responses_retry_without_session_headers_error path={} original_status={} account_id={} upstream_url={} err={}",
                request_ctx.request_path,
                status.as_u16(),
                account.id,
                url,
                err
            );
            None
        }
    }
}

pub(in crate::gateway::upstream) enum PostRetryFlowDecision {
    Failover,
    Terminal { status_code: u16, message: String },
    RespondUpstream(GatewayUpstreamResponse),
}

fn allow_openai_fallback_for_authorization(
    allow_openai_fallback: bool,
    authorization: &PrimaryAuthorization,
) -> bool {
    allow_openai_fallback && !authorization.uses_agent_identity
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
pub(in crate::gateway::upstream) async fn process_upstream_post_retry_flow<F>(
    client: &reqwest::Client,
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
    authorization: &PrimaryAuthorization,
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
    let scoped_account =
        super::primary_flow::account_with_authorization_scope(account, authorization);
    let account = &scoped_account;
    let mut current_auth_token = authorization.value.clone();
    let mut current_request_ctx = request_ctx.with_fedramp(authorization.is_fedramp);
    let mut status = upstream.status();

    if status.as_u16() == 401 && authorization.uses_agent_identity {
        let buffered = upstream.into_buffered_async().await;
        let (response_body, rebuilt_upstream) = match buffered {
            Ok(buffered) => buffered,
            Err(err) => {
                log::warn!(
                    "event=gateway_agent_identity_unauthorized_body_read_failed path={} account_id={} err={}",
                    path,
                    account.id,
                    err
                );
                return PostRetryFlowDecision::Terminal {
                    status_code: 502,
                    message: err,
                };
            }
        };
        upstream = rebuilt_upstream;
        if crate::agent_identity::is_agent_identity_task_invalid_response(
            status.as_u16(),
            response_body.as_ref(),
        ) {
            if let Some(failed_task_id) = authorization.task_id.as_deref() {
                match crate::agent_identity::recover_account_agent_identity_authorization_async(
                    storage,
                    client,
                    account,
                    token,
                    failed_task_id,
                )
                .await
                {
                    Ok(Some(recovered)) => {
                        current_auth_token = recovered.value;
                        current_request_ctx = request_ctx.with_fedramp(recovered.is_fedramp);
                        if debug {
                            log::warn!(
                                "event=gateway_agent_identity_task_recovery_retry path={} account_id={} recovery=success",
                                path,
                                account.id
                            );
                        }
                        match super::transport::send_upstream_request(
                            client,
                            method,
                            url,
                            request_deadline,
                            current_request_ctx,
                            incoming_headers,
                            body,
                            is_stream,
                            current_auth_token.as_str(),
                            account,
                            strip_session_affinity,
                        )
                        .await
                        {
                            Ok(response) => {
                                upstream = response;
                                status = upstream.status();
                            }
                            Err(err) => {
                                log::warn!(
                                    "event=gateway_agent_identity_task_recovery_retry_error path={} status=502 account_id={} err={}",
                                    path,
                                    account.id,
                                    err
                                );
                            }
                        }
                    }
                    Ok(None) => {
                        log::warn!(
                            "event=gateway_agent_identity_task_recovery_missing_identity path={} account_id={}",
                            path,
                            account.id
                        );
                    }
                    Err(err) => {
                        log::warn!(
                            "event=gateway_agent_identity_task_recovery_failed path={} account_id={} err={}",
                            path,
                            account.id,
                            err
                        );
                    }
                }
            }
        }
    }

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

    if status.as_u16() == 401 && !authorization.uses_agent_identity {
        match try_refresh_chatgpt_access_token(storage, upstream_base, account, token).await {
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
                    current_request_ctx,
                    incoming_headers,
                    body,
                    is_stream,
                    current_auth_token.as_str(),
                    account,
                    strip_session_affinity,
                )
                .await
                {
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

    if !strip_session_affinity {
        if let Some(resp) = retry_chatgpt_responses_bad_request_without_session_headers(
            client,
            method,
            upstream_base,
            url,
            request_deadline,
            current_request_ctx,
            incoming_headers,
            body,
            is_stream,
            current_auth_token.as_str(),
            account,
            debug,
            status,
        )
        .await
        {
            upstream = resp;
            status = upstream.status();
            upstream_content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE);
            upstream_cf_ray = first_header_value(upstream.headers(), "cf-ray");
        }
    }

    if let Some(alt_url) = url_alt {
        match retry_with_alternate_path(
            client,
            method,
            Some(alt_url),
            request_deadline,
            current_request_ctx,
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
        )
        .await
        {
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
        current_request_ctx,
        incoming_headers,
        body,
        is_stream,
        current_auth_token.as_str(),
        account,
        strip_session_affinity,
        debug,
        status,
    )
    .await
    {
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
            current_request_ctx,
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
        )
        .await
        {
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
            current_request_ctx,
            incoming_headers,
            body,
            is_stream,
            current_auth_token.as_str(),
            account,
            strip_session_affinity,
            status,
            debug,
            disable_challenge_stateless_retry,
        )
        .await
        {
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
            allow_openai_fallback_for_authorization(allow_openai_fallback, authorization),
            status,
            upstream_content_type,
            has_more_candidates,
            current_request_ctx.capture,
            &mut log_gateway_result,
        )
        .await
        {
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
#[path = "postprocess_tests.rs"]
mod tests;
