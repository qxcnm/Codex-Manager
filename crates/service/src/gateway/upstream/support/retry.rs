use bytes::Bytes;
use codexmanager_core::storage::Account;
use reqwest::StatusCode;
use std::time::{Duration, Instant};

use super::super::attempt_flow::transport::send_upstream_request;
use super::super::attempt_flow::transport::UpstreamRequestContext;
use super::super::GatewayUpstreamResponse;

fn should_skip_codex_v1_alt_for_responses(
    request_ctx: UpstreamRequestContext<'_>,
    alt_url: &str,
) -> bool {
    request_ctx.protocol_type == crate::apikey_profile::PROTOCOL_OPENAI_COMPAT
        && reqwest::Url::parse(alt_url).ok().is_some_and(|url| {
            url.path()
                .trim_end_matches('/')
                .to_ascii_lowercase()
                .ends_with("/backend-api/codex/v1/responses")
        })
}

pub(in super::super) enum AltPathRetryResult {
    NotTriggered,
    Upstream(GatewayUpstreamResponse),
    Failover,
    Terminal { status_code: u16, message: String },
}

/// 函数 `retry_with_alternate_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
#[allow(clippy::too_many_arguments)]
pub(in super::super) async fn retry_with_alternate_path<F>(
    client: &reqwest::Client,
    method: &reqwest::Method,
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
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> AltPathRetryResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    let Some(alt_url) = alt_url else {
        return AltPathRetryResult::NotTriggered;
    };
    if !matches!(status.as_u16(), 400 | 404) {
        return AltPathRetryResult::NotTriggered;
    }
    if should_skip_codex_v1_alt_for_responses(request_ctx, alt_url) {
        log::warn!(
            "event=gateway_upstream_alt_retry_skipped path={} status={} reason=responses_codex_v1_alt_blocked upstream_url={}",
            request_ctx.request_path,
            status.as_u16(),
            alt_url
        );
        return AltPathRetryResult::NotTriggered;
    }
    if debug {
        log::warn!(
            "event=gateway_upstream_alt_retry path={} status={} account_id={} upstream_url={}",
            request_ctx.request_path,
            status.as_u16(),
            account.id,
            alt_url
        );
    }
    if super::deadline::is_expired(request_deadline) {
        return AltPathRetryResult::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
    }
    if !super::backoff::sleep_with_exponential_jitter_async(
        Duration::from_millis(40),
        Duration::from_millis(200),
        0,
        request_deadline,
    )
    .await
    {
        return AltPathRetryResult::Terminal {
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
        strip_session_affinity,
    )
    .await
    {
        Ok(response) => AltPathRetryResult::Upstream(response),
        Err(err) => {
            let err_msg = err.to_string();
            super::super::super::mark_account_cooldown(
                &account.id,
                super::super::super::CooldownReason::Network,
            );
            log_gateway_result(Some(alt_url), 502, Some(err_msg.as_str()));
            // 中文注释：alt 路径失败时若还有候选账号必须优先切换，
            // 不这样做会把单账号路径差异放大成整次请求失败。
            if has_more_candidates {
                AltPathRetryResult::Failover
            } else {
                AltPathRetryResult::Terminal {
                    status_code: 502,
                    message: format!("upstream error: {err}"),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;
