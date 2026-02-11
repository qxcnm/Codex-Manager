use gpttools_core::storage::Account;
use reqwest::StatusCode;
use tiny_http::Request;

use super::transport::send_upstream_request;

pub(super) enum StatelessRetryResult {
    NotTriggered,
    Upstream(reqwest::blocking::Response),
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retry_stateless_then_optional_alt(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    primary_url: &str,
    alt_url: Option<&str>,
    request: &Request,
    body: &[u8],
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    status: StatusCode,
    debug: bool,
) -> StatelessRetryResult {
    if strip_session_affinity || !matches!(status.as_u16(), 401 | 403 | 404) {
        return StatelessRetryResult::NotTriggered;
    }
    if debug {
        eprintln!(
            "gateway upstream stateless retry: account_id={}, status={}",
            account.id, status
        );
    }
    let mut response = match send_upstream_request(
        client,
        method,
        primary_url,
        request,
        body,
        upstream_cookie,
        auth_token,
        account,
        true,
    ) {
        Ok(resp) => resp,
        Err(err) => {
            log::warn!(
                "gateway stateless retry error: account_id={}, err={}",
                account.id,
                err
            );
            return StatelessRetryResult::NotTriggered;
        }
    };

    if let Some(alt_url) = alt_url {
        if matches!(response.status().as_u16(), 400 | 404) {
            match send_upstream_request(
                client,
                method,
                alt_url,
                request,
                body,
                upstream_cookie,
                auth_token,
                account,
                true,
            ) {
                Ok(resp) => {
                    response = resp;
                }
                Err(err) => {
                    log::warn!(
                        "gateway stateless alt retry error: account_id={}, err={}",
                        account.id,
                        err
                    );
                }
            }
        }
    }

    StatelessRetryResult::Upstream(response)
}



