use gpttools_core::storage::Account;
use reqwest::StatusCode;
use tiny_http::Request;

use super::transport::send_upstream_request;

pub(super) enum AltPathRetryResult {
    NotTriggered,
    Upstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retry_with_alternate_path<F>(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    alt_url: Option<&str>,
    request: &Request,
    body: &[u8],
    upstream_cookie: Option<&str>,
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
    if debug {
        eprintln!("gateway upstream retry: url={alt_url}");
    }
    match send_upstream_request(
        client,
        method,
        alt_url,
        request,
        body,
        upstream_cookie,
        auth_token,
        account,
        strip_session_affinity,
    ) {
        Ok(response) => AltPathRetryResult::Upstream(response),
        Err(err) => {
            let err_msg = err.to_string();
            super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
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


