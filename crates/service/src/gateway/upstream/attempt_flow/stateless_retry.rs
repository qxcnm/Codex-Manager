use bytes::Bytes;
use codexmanager_core::storage::Account;
use reqwest::StatusCode;
use std::time::{Duration, Instant};

use super::super::support::{backoff, deadline};
use super::super::GatewayUpstreamResponse;
use super::transport::{send_upstream_request, UpstreamRequestContext};

pub(super) enum StatelessRetryResult {
    NotTriggered,
    Upstream(GatewayUpstreamResponse),
    Terminal { status_code: u16, message: String },
}

/// 函数 `should_trigger_stateless_retry`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status: 参数 status
/// - strip_session_affinity: 参数 strip_session_affinity
/// - disable_challenge_stateless_retry: 参数 disable_challenge_stateless_retry
///
/// # 返回
/// 返回函数执行结果
fn should_trigger_stateless_retry(
    status: u16,
    strip_session_affinity: bool,
    disable_challenge_stateless_retry: bool,
) -> bool {
    if strip_session_affinity {
        return !disable_challenge_stateless_retry && matches!(status, 403 | 429);
    }
    if disable_challenge_stateless_retry {
        return matches!(status, 404);
    }
    matches!(status, 403 | 404 | 429)
}

/// 函数 `retry_stateless_then_optional_alt`
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
pub(super) async fn retry_stateless_then_optional_alt(
    client: &reqwest::Client,
    method: &reqwest::Method,
    primary_url: &str,
    alt_url: Option<&str>,
    request_deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    status: StatusCode,
    debug: bool,
    disable_challenge_stateless_retry: bool,
) -> StatelessRetryResult {
    if deadline::is_expired(request_deadline) {
        return StatelessRetryResult::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    if !should_trigger_stateless_retry(
        status.as_u16(),
        strip_session_affinity,
        disable_challenge_stateless_retry,
    ) {
        return StatelessRetryResult::NotTriggered;
    }
    if debug {
        log::warn!(
            "event=gateway_upstream_stateless_retry path={} status={} account_id={}",
            request_ctx.request_path,
            status.as_u16(),
            account.id
        );
    }
    if matches!(status.as_u16(), 403 | 429) {
        if !backoff::sleep_with_exponential_jitter_async(
            Duration::from_millis(120),
            Duration::from_millis(900),
            1,
            request_deadline,
        )
        .await
        {
            return StatelessRetryResult::Terminal {
                status_code: 504,
                message: "upstream total timeout exceeded".to_string(),
            };
        }
    }
    let mut response = match send_upstream_request(
        client,
        method,
        primary_url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        auth_token,
        account,
        true,
    )
    .await
    {
        Ok(resp) => resp,
        Err(err) => {
            log::warn!(
                "event=gateway_stateless_retry_error path={} status=502 account_id={} err={}",
                request_ctx.request_path,
                account.id,
                err
            );
            return StatelessRetryResult::NotTriggered;
        }
    };

    if let Some(alt_url) = alt_url {
        if matches!(response.status().as_u16(), 400 | 404) {
            if !backoff::sleep_with_exponential_jitter_async(
                Duration::from_millis(80),
                Duration::from_millis(500),
                2,
                request_deadline,
            )
            .await
            {
                return StatelessRetryResult::Terminal {
                    status_code: 504,
                    message: "upstream total timeout exceeded".to_string(),
                };
            }
            match send_upstream_request(
                client,
                method,
                alt_url,
                request_deadline,
                request_ctx,
                incoming_headers,
                body,
                is_stream,
                auth_token,
                account,
                true,
            )
            .await
            {
                Ok(resp) => {
                    response = resp;
                }
                Err(err) => {
                    log::warn!(
                        "event=gateway_stateless_alt_retry_error path={} status=502 account_id={} upstream_url={} err={}",
                        request_ctx.request_path,
                        account.id,
                        alt_url,
                        err
                    );
                }
            }
        }
    }

    StatelessRetryResult::Upstream(response)
}

#[cfg(test)]
#[path = "../tests/attempt_flow/stateless_retry_tests.rs"]
mod tests;
