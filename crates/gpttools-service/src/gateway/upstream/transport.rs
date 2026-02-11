use gpttools_core::storage::Account;
use reqwest::header::{HeaderName, HeaderValue};
use tiny_http::Request;

fn should_drop_header_for_attempt(name: &str, strip_session_affinity: bool) -> bool {
    if strip_session_affinity {
        super::super::should_drop_incoming_header_for_failover(name)
    } else {
        super::super::should_drop_incoming_header(name)
    }
}

pub(super) fn send_upstream_request(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    target_url: &str,
    request: &Request,
    body: &[u8],
    upstream_cookie: Option<&str>,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let mut builder = client.request(method.clone(), target_url);
    let mut has_user_agent = false;
    for header in request.headers() {
        let name = header.field.as_str().as_str();
        if should_drop_header_for_attempt(name, strip_session_affinity) {
            continue;
        }
        if header.field.equiv("User-Agent") {
            has_user_agent = true;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.field.as_str().as_bytes()),
            HeaderValue::from_str(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    if !has_user_agent {
        builder = builder.header("User-Agent", "codex-cli");
    }
    if let Some(cookie) = upstream_cookie {
        if !cookie.trim().is_empty() {
            builder = builder.header("Cookie", cookie);
        }
    }
    builder = builder.header("Authorization", format!("Bearer {}", auth_token));
    if let Some(acc) = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref())
    {
        builder = builder.header("ChatGPT-Account-Id", acc);
    }
    if !body.is_empty() {
        builder = builder.body(body.to_vec());
    }
    builder.send()
}


