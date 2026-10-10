use bytes::Bytes;
use codexmanager_core::storage::Account;
use std::time::Instant;

use super::super::support::deadline;
use super::super::GatewayUpstreamResponse;
use super::transport::UpstreamRequestContext;

pub(super) enum PrimaryAttemptResult {
    Upstream(GatewayUpstreamResponse),
    Failover,
    Terminal { status_code: u16, message: String },
}

/// 函数 `run_primary_upstream_attempt`
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
pub(super) async fn run_primary_upstream_attempt<F>(
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
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> PrimaryAttemptResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    if deadline::is_expired(request_deadline) {
        log_gateway_result(Some(url), 504, Some("upstream total timeout exceeded"));
        return PrimaryAttemptResult::Terminal {
            status_code: 504,
            message: "upstream total timeout exceeded".to_string(),
        };
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
        Ok(resp) => PrimaryAttemptResult::Upstream(resp),
        Err(err) => {
            let err_msg = err.to_string();
            super::super::super::mark_account_cooldown(
                &account.id,
                super::super::super::CooldownReason::Network,
            );
            log_gateway_result(Some(url), 502, Some(err_msg.as_str()));
            if should_failover_transport_error(url, has_more_candidates) {
                PrimaryAttemptResult::Failover
            } else {
                PrimaryAttemptResult::Terminal {
                    status_code: 502,
                    message: format!("upstream error: {err}"),
                }
            }
        }
    }
}

fn should_failover_transport_error(url: &str, has_more_candidates: bool) -> bool {
    if !has_more_candidates {
        return false;
    }

    super::super::config::is_chatgpt_backend_base(url)
        || !super::super::config::is_official_openai_target(url)
}

#[cfg(test)]
#[path = "primary_attempt_tests.rs"]
mod tests;
