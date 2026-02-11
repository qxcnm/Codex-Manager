use gpttools_core::storage::Account;

pub(super) enum PrimaryAttemptResult {
    Upstream(reqwest::blocking::Response),
    Failover,
    Terminal { status_code: u16, message: String },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_primary_upstream_attempt<F>(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    url: &str,
    request: &tiny_http::Request,
    body: &[u8],
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    has_more_candidates: bool,
    mut log_gateway_result: F,
) -> PrimaryAttemptResult
where
    F: FnMut(Option<&str>, u16, Option<&str>),
{
    match super::transport::send_upstream_request(
        client,
        method,
        url,
        request,
        body,
        upstream_cookie,
        auth_token,
        account,
        strip_session_affinity,
    ) {
        Ok(resp) => PrimaryAttemptResult::Upstream(resp),
        Err(err) => {
            let err_msg = err.to_string();
            super::super::mark_account_cooldown(&account.id, super::super::CooldownReason::Network);
            log_gateway_result(Some(url), 502, Some(err_msg.as_str()));
            // 中文注释：主链路首次请求失败不代表所有候选都失败，
            // 先 failover 才能避免单账号抖动放大成全局不可用。
            if has_more_candidates {
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


