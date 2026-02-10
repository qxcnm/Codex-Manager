use std::collections::HashSet;

use gpttools_core::rpc::types::ModelOption;
use gpttools_core::storage::{Account, Storage, Token};
use reqwest::blocking::Client;
use reqwest::header::CONTENT_TYPE;
use reqwest::Method;
use serde_json::Value;

pub(crate) fn fetch_models_for_picker() -> Result<Vec<ModelOption>, String> {
    let storage = super::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let mut candidates = super::collect_gateway_candidates(&storage)?;
    if candidates.is_empty() {
        return Err("no available account".to_string());
    }

    let upstream_base = super::resolve_upstream_base_url();
    let base = upstream_base.as_str();
    let upstream_fallback_base = super::resolve_upstream_fallback_base_url(base);
    let path = super::normalize_models_path("/v1/models");
    let method = Method::GET;
    let client = super::upstream_client();
    let upstream_cookie = std::env::var("GPTTOOLS_UPSTREAM_COOKIE").ok();
    candidates.sort_by_key(|(account, _)| {
        (
            super::is_account_in_cooldown(&account.id),
            super::account_inflight_count(&account.id),
        )
    });
    super::rotate_candidates_for_fairness(&mut candidates);

    let mut last_error = "models request failed".to_string();
    for (account, mut token) in candidates {
        match send_models_request(
            &client,
            &storage,
            &method,
            &upstream_base,
            &path,
            &account,
            &mut token,
            upstream_cookie.as_deref(),
        ) {
            Ok(response_body) => return Ok(parse_model_options(&response_body)),
            Err(err) => {
                // ChatGPT upstream occasionally returns HTML challenge. Try OpenAI fallback.
                if err.contains("text/html") || err.contains("cloudflare") {
                    if let Some(fallback_base) = upstream_fallback_base.as_deref() {
                        if let Ok(response_body) = send_models_request(
                            &client,
                            &storage,
                            &method,
                            fallback_base,
                            &path,
                            &account,
                            &mut token,
                            upstream_cookie.as_deref(),
                        ) {
                            return Ok(parse_model_options(&response_body));
                        }
                    }
                }
                last_error = err;
            }
        }
    }
    Err(last_error)
}

fn send_models_request(
    client: &Client,
    storage: &Storage,
    method: &Method,
    upstream_base: &str,
    path: &str,
    account: &Account,
    token: &mut Token,
    upstream_cookie: Option<&str>,
) -> Result<Vec<u8>, String> {
    let (url, _url_alt) = super::compute_upstream_url(upstream_base, path);
    let mut builder = client.request(method.clone(), &url);
    builder = builder.header("User-Agent", "codex-cli");
    if let Some(cookie) = upstream_cookie {
        if !cookie.trim().is_empty() {
            builder = builder.header("Cookie", cookie);
        }
    }

    // OpenAI upstream requires api_key_access_token; backend-api/codex keeps access_token.
    let bearer = if super::is_openai_api_base(upstream_base) {
        super::resolve_openai_bearer_token(storage, account, token)?
    } else {
        token.access_token.clone()
    };
    builder = builder.header("Authorization", format!("Bearer {}", bearer));
    if let Some(acc) = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref())
    {
        builder = builder.header("ChatGPT-Account-Id", acc);
    }

    let response = builder.send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(format!("models upstream failed: status={} body={}", status, body));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if super::is_html_content_type(content_type) {
        return Err("models upstream returned text/html (cloudflare challenge)".to_string());
    }
    response.bytes().map(|v| v.to_vec()).map_err(|e| e.to_string())
}

fn parse_model_options(body: &[u8]) -> Vec<ModelOption> {
    let mut items: Vec<ModelOption> = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(models) = value.get("models").and_then(|v| v.as_array()) {
            for item in models {
                let slug = item
                    .get("slug")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty());
                if let Some(slug) = slug {
                    if seen.insert(slug.to_string()) {
                        let display_name = item
                            .get("title")
                            .or_else(|| item.get("display_name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(slug)
                            .to_string();
                        items.push(ModelOption {
                            slug: slug.to_string(),
                            display_name,
                        });
                    }
                }
            }
        }
        if let Some(models) = value.get("data").and_then(|v| v.as_array()) {
            for item in models {
                let slug = item
                    .get("id")
                    .or_else(|| item.get("slug"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty());
                if let Some(slug) = slug {
                    if seen.insert(slug.to_string()) {
                        let display_name = item
                            .get("display_name")
                            .or_else(|| item.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(slug)
                            .to_string();
                        items.push(ModelOption {
                            slug: slug.to_string(),
                            display_name,
                        });
                    }
                }
            }
        }
    }

    items.sort_by(|a, b| a.slug.cmp(&b.slug));
    items
}
