use bytes::Bytes;
use codexmanager_core::storage::{AggregateApi, Storage};
use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use std::collections::HashSet;
use std::time::Instant;
use tiny_http::Request;

use super::super::GatewayUpstreamResponse;
use crate::aggregate_api::{
    AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_AUTH_USERPASS, AGGREGATE_API_PROVIDER_CLAUDE,
    AGGREGATE_API_PROVIDER_CODEX, AGGREGATE_API_PROVIDER_GEMINI,
};
use crate::gateway::protocol_adapter::adapt_openai_responses_to_anthropic_messages;
use crate::gateway::request_log::RequestLogUsage;
use serde_json::Value;

const AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL: usize = 3;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyAuthParams {
    location: String,
    name: String,
    #[serde(default)]
    header_value_format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPassAuthParams {
    mode: String,
    #[serde(default)]
    username_name: Option<String>,
    #[serde(default)]
    password_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPassSecret {
    username: String,
    password: String,
}

#[derive(Debug, Clone)]
enum AggregateApiAuthConfig {
    ApiKeyDefaultBearer,
    ApiKeyHeader {
        name: String,
        format: String,
    },
    ApiKeyQuery {
        name: String,
    },
    UserPassBasic,
    UserPassHeaderPair {
        username_name: String,
        password_name: String,
    },
    UserPassQueryPair {
        username_name: String,
        password_name: String,
    },
}

fn normalize_header_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn normalize_action_path(action: &str) -> String {
    let action_trimmed = action.trim();
    if action_trimmed.is_empty() {
        return String::new();
    }
    if action_trimmed.starts_with('/') {
        action_trimmed.to_string()
    } else {
        format!("/{action_trimmed}")
    }
}

fn effective_action_path(candidate: &AggregateApi, path: &str) -> String {
    match candidate.action.as_deref().map(str::trim) {
        Some("") => String::new(),
        Some(value) => normalize_action_path(value),
        None => path.to_string(),
    }
}

fn build_upstream_url(base_url: &str, effective_path: &str) -> Result<reqwest::Url, ()> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| ())?;
    let trimmed_path = effective_path.trim();
    if trimmed_path.is_empty() {
        return Ok(url);
    }
    let (path_part, query_part) = trimmed_path
        .split_once('?')
        .map_or((trimmed_path, None), |(path, query)| (path, Some(query)));
    let suffix = path_part.trim_start_matches('/');
    let base_path = url.path().trim_end_matches('/').to_string();
    let combined_path = if base_path.is_empty() || base_path == "/" {
        format!("/{}", suffix)
    } else if suffix.is_empty() {
        base_path
    } else {
        format!("{}/{}", base_path, suffix)
    };
    url.set_path(combined_path.as_str());
    url.set_query(query_part.filter(|query| !query.trim().is_empty()));
    Ok(url)
}

fn rewrite_body_model_override(body: &Bytes, model_override: Option<&str>) -> Bytes {
    let Some(model_override) = model_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return body.clone();
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(body.as_ref()) else {
        return body.clone();
    };
    let Some(object) = value.as_object_mut() else {
        return body.clone();
    };
    if object
        .get("model")
        .and_then(Value::as_str)
        .is_some_and(|current| current == model_override)
    {
        return body.clone();
    }
    object.insert(
        "model".to_string(),
        Value::String(model_override.to_string()),
    );
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone())
}

fn aggregate_upstream_model_for_log<'a>(
    candidate: &'a AggregateApi,
    platform_model: Option<&'a str>,
) -> Option<&'a str> {
    candidate.model_override.as_deref().or(platform_model)
}

fn should_bridge_responses_to_anthropic(candidate: &AggregateApi, path: &str) -> bool {
    normalize_provider_type_value(candidate.provider_type.as_str()) == AGGREGATE_API_PROVIDER_CLAUDE
        && (path == "/v1/responses" || path.starts_with("/v1/responses?"))
}

fn responses_to_anthropic_messages_action_path(candidate: &AggregateApi, path: &str) -> String {
    if candidate.action.is_some() {
        return effective_action_path(candidate, path);
    }

    let base_path = reqwest::Url::parse(candidate.url.as_str())
        .ok()
        .map(|url| url.path().trim_end_matches('/').to_string())
        .unwrap_or_default();
    if base_path == "/v1" || base_path.ends_with("/v1") {
        "/messages".to_string()
    } else {
        "/v1/messages".to_string()
    }
}

fn replace_query_param(mut url: reqwest::Url, name: &str, value: &str) -> reqwest::Url {
    let name_trimmed = name.trim();
    if name_trimmed.is_empty() {
        return url;
    }
    let existing = url.query_pairs().into_owned().collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut qp = url.query_pairs_mut();
        for (k, v) in existing {
            if k == name_trimmed {
                continue;
            }
            qp.append_pair(k.as_str(), v.as_str());
        }
        qp.append_pair(name_trimmed, value);
    }
    url
}

fn parse_auth_config(
    candidate: &AggregateApi,
) -> Result<(AggregateApiAuthConfig, HashSet<String>), String> {
    let auth_type = candidate.auth_type.trim().to_ascii_lowercase();
    let raw_params = candidate
        .auth_params_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut injected_headers = HashSet::new();

    if raw_params.is_none() {
        if auth_type == AGGREGATE_API_AUTH_USERPASS {
            return Ok((AggregateApiAuthConfig::UserPassBasic, injected_headers));
        }
        return Ok((
            AggregateApiAuthConfig::ApiKeyDefaultBearer,
            injected_headers,
        ));
    }

    let value: serde_json::Value = serde_json::from_str(raw_params.unwrap())
        .map_err(|_| "invalid aggregate api authParams".to_string())?;

    if auth_type == AGGREGATE_API_AUTH_APIKEY {
        let parsed: ApiKeyAuthParams = serde_json::from_value(value)
            .map_err(|_| "invalid aggregate api authParams".to_string())?;
        let location = parsed.location.trim().to_ascii_lowercase();
        if location == "query" {
            return Ok((
                AggregateApiAuthConfig::ApiKeyQuery {
                    name: parsed.name.trim().to_string(),
                },
                injected_headers,
            ));
        }
        let header_name = parsed.name.trim().to_string();
        injected_headers.insert(normalize_header_key(header_name.as_str()));
        let format = parsed
            .header_value_format
            .as_deref()
            .unwrap_or("bearer")
            .trim()
            .to_ascii_lowercase();
        return Ok((
            AggregateApiAuthConfig::ApiKeyHeader {
                name: header_name,
                format,
            },
            injected_headers,
        ));
    }

    if auth_type == AGGREGATE_API_AUTH_USERPASS {
        let parsed: UserPassAuthParams = serde_json::from_value(value)
            .map_err(|_| "invalid aggregate api authParams".to_string())?;
        let mode = parsed.mode.trim().to_ascii_lowercase();
        match mode.as_str() {
            "basic" => return Ok((AggregateApiAuthConfig::UserPassBasic, injected_headers)),
            "headerpair" => {
                let username_name = parsed
                    .username_name
                    .as_deref()
                    .unwrap_or("username")
                    .trim()
                    .to_string();
                let password_name = parsed
                    .password_name
                    .as_deref()
                    .unwrap_or("password")
                    .trim()
                    .to_string();
                injected_headers.insert(normalize_header_key(username_name.as_str()));
                injected_headers.insert(normalize_header_key(password_name.as_str()));
                return Ok((
                    AggregateApiAuthConfig::UserPassHeaderPair {
                        username_name,
                        password_name,
                    },
                    injected_headers,
                ));
            }
            "querypair" => {
                let username_name = parsed
                    .username_name
                    .as_deref()
                    .unwrap_or("username")
                    .trim()
                    .to_string();
                let password_name = parsed
                    .password_name
                    .as_deref()
                    .unwrap_or("password")
                    .trim()
                    .to_string();
                return Ok((
                    AggregateApiAuthConfig::UserPassQueryPair {
                        username_name,
                        password_name,
                    },
                    injected_headers,
                ));
            }
            _ => return Err("invalid aggregate api authParams".to_string()),
        }
    }

    Ok((
        AggregateApiAuthConfig::ApiKeyDefaultBearer,
        injected_headers,
    ))
}

fn resolve_passthrough_sse_protocol(
    path: &str,
    response_adapter: super::super::super::ResponseAdapter,
) -> Option<super::super::super::PassthroughSseProtocol> {
    if response_adapter != super::super::super::ResponseAdapter::Passthrough {
        return None;
    }
    if path == "/v1/messages" || path.starts_with("/v1/messages?") {
        return Some(super::super::super::PassthroughSseProtocol::AnthropicNative);
    }
    None
}

/// 函数 `should_skip_forward_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn should_skip_forward_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "x-api-key"
            | "api-key"
            | "content-length"
            | "connection"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
    )
}

fn should_skip_forward_header_with_overrides(name: &str, injected: &HashSet<String>) -> bool {
    if should_skip_forward_header(name) {
        return true;
    }
    injected.contains(normalize_header_key(name).as_str())
}

fn should_skip_forward_header_for_aggregate_request(
    name: &str,
    injected: &HashSet<String>,
    is_stream: bool,
) -> bool {
    if should_skip_forward_header_with_overrides(name, injected) {
        return true;
    }
    is_stream && normalize_header_key(name) == "accept"
}

/// 函数 `respond_error`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request: 参数 request
/// - status: 参数 status
/// - message: 参数 message
/// - trace_id: 参数 trace_id
///
/// # 返回
/// 无
fn respond_error(request: Request, status: u16, message: &str, trace_id: Option<&str>) {
    let response_message = super::super::super::error_message_for_client(
        super::super::super::prefers_raw_errors_for_tiny_http_request(&request),
        message,
    );
    let response = super::super::super::error_response::terminal_text_response(
        status,
        response_message,
        trace_id,
    );
    let _ = request.respond(response);
}

/// 函数 `normalize_candidate_order`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - candidates: 参数 candidates
///
/// # 返回
/// 返回函数执行结果
fn normalize_candidate_order(mut candidates: Vec<AggregateApi>) -> Vec<AggregateApi> {
    candidates.sort_by(|left, right| {
        left.sort
            .cmp(&right.sort)
            .then(right.created_at.cmp(&left.created_at))
            .then(left.id.cmp(&right.id))
    });
    candidates
}

/// 函数 `apply_gateway_route_strategy_to_aggregate_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn apply_gateway_route_strategy_to_aggregate_candidates(
    candidates: &mut [AggregateApi],
    key_id: &str,
    model: Option<&str>,
    preferred_aggregate_api_id: Option<&str>,
) {
    if candidates.len() <= 1 {
        return;
    }
    if crate::gateway::current_route_strategy() != "balanced" {
        return;
    }

    let preferred_id = preferred_aggregate_api_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let preserves_head = preferred_id
        .and_then(|preferred_id| candidates.first().map(|first| (preferred_id, first)))
        .is_some_and(|(preferred_id, first)| first.id == preferred_id);

    if preserves_head {
        if candidates.len() > 1 {
            super::super::super::route_hint::apply_balanced_round_robin(
                &mut candidates[1..],
                key_id,
                model,
            );
        }
    } else {
        super::super::super::route_hint::apply_balanced_round_robin(candidates, key_id, model);
    }
}

/// 函数 `normalize_provider_type_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn normalize_provider_type_value(value: &str) -> String {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "claude" | "anthropic" | "anthropic_native" | "claude_code" => {
            AGGREGATE_API_PROVIDER_CLAUDE.to_string()
        }
        "gemini" | "gemini_native" | "google" | "google_ai" | "google_gemini" => {
            AGGREGATE_API_PROVIDER_GEMINI.to_string()
        }
        _ => AGGREGATE_API_PROVIDER_CODEX.to_string(),
    }
}

/// 函数 `first_upstream_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - names: 参数 names
///
/// # 返回
/// 返回函数执行结果
fn first_upstream_header(headers: &reqwest::header::HeaderMap, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

/// 函数 `aggregate_api_failure_message`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - status_code: 参数 status_code
/// - body: 参数 body
/// - request_id: 参数 request_id
/// - cf_ray: 参数 cf_ray
/// - auth_error: 参数 auth_error
/// - identity_error_code: 参数 identity_error_code
///
/// # 返回
/// 返回函数执行结果
fn aggregate_api_failure_message(
    status_code: u16,
    body: &[u8],
    request_id: Option<&str>,
    cf_ray: Option<&str>,
    auth_error: Option<&str>,
    identity_error_code: Option<&str>,
) -> String {
    let mut parts =
        vec![
            crate::gateway::summarize_upstream_error_hint_from_body(status_code, body)
                .unwrap_or_else(|| format!("aggregate api upstream status={status_code}")),
        ];
    if let Some(request_id) = request_id.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("request_id={request_id}"));
    }
    if let Some(cf_ray) = cf_ray.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("cf_ray={cf_ray}"));
    }
    if let Some(auth_error) = auth_error.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("auth_error={auth_error}"));
    }
    if let Some(identity_error_code) = identity_error_code
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("identity_error_code={identity_error_code}"));
    }
    if parts.len() == 1 {
        parts.remove(0)
    } else {
        format!("{} [{}]", parts.remove(0), parts.join(", "))
    }
}

/// 函数 `build_aggregate_api_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - client: 参数 client
/// - request: 参数 request
/// - method: 参数 method
/// - url: 参数 url
/// - body: 参数 body
/// - secret: 参数 secret
/// - request_deadline: 参数 request_deadline
/// - is_stream: 参数 is_stream
///
/// # 返回
/// 返回函数执行结果
fn build_aggregate_api_request(
    client: &reqwest::blocking::Client,
    request: &Request,
    method: &reqwest::Method,
    url: reqwest::Url,
    body: &Bytes,
    secret: &str,
    auth_config: &AggregateApiAuthConfig,
    injected_headers: &HashSet<String>,
    request_deadline: Option<Instant>,
    is_stream: bool,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    let mut builder = client.request(method.clone(), url);
    if let Some(timeout) =
        super::super::support::deadline::send_timeout(request_deadline, is_stream)
    {
        builder = builder.timeout(timeout);
    }
    let request_headers = request.headers().to_vec();
    for header in &request_headers {
        if should_skip_forward_header_for_aggregate_request(
            header.field.as_str().into(),
            injected_headers,
            is_stream,
        ) {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(header.field.as_str().as_bytes()),
            HeaderValue::from_str(header.value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    if is_stream {
        builder = builder.header(
            HeaderName::from_static("accept"),
            HeaderValue::from_static("text/event-stream"),
        );
    }

    let secret_trimmed = secret.trim();
    match auth_config {
        AggregateApiAuthConfig::ApiKeyDefaultBearer => {
            builder = builder.header(
                HeaderName::from_static("authorization"),
                HeaderValue::from_str(format!("Bearer {}", secret_trimmed).as_str())
                    .map_err(|_| "invalid aggregate api secret".to_string())?,
            );
        }
        AggregateApiAuthConfig::ApiKeyHeader { name, format } => {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| "invalid aggregate api auth header".to_string())?;
            let value = if format == "raw" {
                secret_trimmed.to_string()
            } else {
                format!("Bearer {}", secret_trimmed)
            };
            builder = builder.header(
                header_name,
                HeaderValue::from_str(value.as_str())
                    .map_err(|_| "invalid aggregate api secret".to_string())?,
            );
        }
        AggregateApiAuthConfig::ApiKeyQuery { .. } => {}
        AggregateApiAuthConfig::UserPassBasic
        | AggregateApiAuthConfig::UserPassHeaderPair { .. }
        | AggregateApiAuthConfig::UserPassQueryPair { .. } => {
            let parsed: UserPassSecret = serde_json::from_str(secret_trimmed)
                .map_err(|_| "invalid aggregate api secret".to_string())?;
            match auth_config {
                AggregateApiAuthConfig::UserPassBasic => {
                    builder = builder.basic_auth(parsed.username, Some(parsed.password));
                }
                AggregateApiAuthConfig::UserPassHeaderPair {
                    username_name,
                    password_name,
                } => {
                    let user_header = HeaderName::from_bytes(username_name.as_bytes())
                        .map_err(|_| "invalid aggregate api auth header".to_string())?;
                    let pass_header = HeaderName::from_bytes(password_name.as_bytes())
                        .map_err(|_| "invalid aggregate api auth header".to_string())?;
                    builder = builder.header(
                        user_header,
                        HeaderValue::from_str(parsed.username.as_str())
                            .map_err(|_| "invalid aggregate api secret".to_string())?,
                    );
                    builder = builder.header(
                        pass_header,
                        HeaderValue::from_str(parsed.password.as_str())
                            .map_err(|_| "invalid aggregate api secret".to_string())?,
                    );
                }
                AggregateApiAuthConfig::UserPassQueryPair { .. } => {}
                _ => {}
            }
        }
    }
    if !body.is_empty() {
        builder = builder.body(body.clone());
    }
    Ok(builder)
}

fn build_anthropic_bridge_aggregate_api_request(
    client: &reqwest::blocking::Client,
    request: &Request,
    method: &reqwest::Method,
    url: reqwest::Url,
    body: &Bytes,
    secret: &str,
    auth_config: &AggregateApiAuthConfig,
    injected_headers: &HashSet<String>,
    request_deadline: Option<Instant>,
    is_stream: bool,
) -> Result<reqwest::blocking::RequestBuilder, String> {
    let mut builder = build_aggregate_api_request(
        client,
        request,
        method,
        url,
        body,
        secret,
        auth_config,
        injected_headers,
        request_deadline,
        is_stream,
    )?;
    builder = builder.header(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static("2023-06-01"),
    );
    if matches!(auth_config, AggregateApiAuthConfig::ApiKeyDefaultBearer) {
        builder = builder.header(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(secret.trim())
                .map_err(|_| "invalid aggregate api secret".to_string())?,
        );
    }
    Ok(builder)
}

/// 函数 `resolve_aggregate_api_rotation_candidates`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn resolve_aggregate_api_rotation_candidates(
    storage: &Storage,
    protocol_type: &str,
    aggregate_api_id: Option<&str>,
) -> Result<Vec<AggregateApi>, String> {
    let provider_type = match protocol_type {
        "anthropic_native" => AGGREGATE_API_PROVIDER_CLAUDE,
        "gemini_native" => AGGREGATE_API_PROVIDER_GEMINI,
        _ => AGGREGATE_API_PROVIDER_CODEX,
    };

    let mut candidates = storage
        .list_aggregate_apis()
        .map_err(|err| err.to_string())?
        .into_iter()
        .filter(|api| {
            api.status == "active"
                && normalize_provider_type_value(api.provider_type.as_str()) == provider_type
        })
        .collect::<Vec<_>>();
    candidates = normalize_candidate_order(candidates);

    if let Some(api_id) = aggregate_api_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(preferred) = storage
            .find_aggregate_api_by_id(api_id)
            .map_err(|err| err.to_string())?
        {
            candidates.retain(|api| api.id != preferred.id);
            candidates.insert(0, preferred);
        }
    }

    if candidates.is_empty() {
        Err(format!(
            "aggregate api not found for provider {provider_type}"
        ))
    } else {
        Ok(candidates)
    }
}

/// 函数 `proxy_aggregate_request`
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
pub(in super::super) struct AggregateProxyRequest<'a> {
    pub request: Request,
    pub storage: &'a Storage,
    pub trace_id: &'a str,
    pub key_id: &'a str,
    pub original_path: &'a str,
    pub path: &'a str,
    pub request_method: &'a str,
    pub method: &'a reqwest::Method,
    pub body: &'a Bytes,
    pub is_stream: bool,
    pub response_adapter: super::super::super::ResponseAdapter,
    pub gateway_mode_for_log: Option<&'a str>,
    pub model_for_log: Option<&'a str>,
    pub reasoning_for_log: Option<&'a str>,
    pub effective_service_tier_for_log: Option<&'a str>,
    pub aggregate_api_candidates: Vec<AggregateApi>,
    pub request_deadline: Option<Instant>,
    pub started_at: Instant,
}

pub(in super::super) fn proxy_aggregate_request(
    params: AggregateProxyRequest<'_>,
) -> Result<(), String> {
    let AggregateProxyRequest {
        request,
        storage,
        trace_id,
        key_id,
        original_path,
        path,
        request_method,
        method,
        body,
        is_stream,
        response_adapter,
        gateway_mode_for_log,
        model_for_log,
        reasoning_for_log,
        effective_service_tier_for_log,
        aggregate_api_candidates,
        request_deadline,
        started_at,
    } = params;
    if aggregate_api_candidates.is_empty() {
        let message = "aggregate api not found".to_string();
        super::super::super::record_gateway_request_outcome(path, 404, Some("aggregate_api"));
        super::super::super::trace_log::log_request_final(
            trace_id,
            404,
            Some(key_id),
            None,
            Some(message.as_str()),
            started_at.elapsed().as_millis(),
        );
        let request = request;
        respond_error(request, 404, message.as_str(), Some(trace_id));
        return Ok(());
    }

    let client = super::super::super::fresh_upstream_client();
    let mut request = Some(request);
    let mut attempted_aggregate_api_ids = Vec::new();
    let mut last_attempt_url: Option<String> = None;
    let mut last_attempt_id: Option<String> = None;
    let mut last_attempt_upstream_model: Option<String> = None;
    let mut last_attempt_supplier_name: Option<String> = None;
    let mut last_attempt_error: Option<String> = None;
    let mut last_failure_status = 502u16;

    let total_candidates = aggregate_api_candidates.len();
    for (candidate_idx, candidate) in aggregate_api_candidates.into_iter().enumerate() {
        attempted_aggregate_api_ids.push(candidate.id.clone());
        let candidate_id = candidate.id.clone();
        let candidate_upstream_model =
            aggregate_upstream_model_for_log(&candidate, model_for_log).map(str::to_string);
        let candidate_supplier_name = candidate.supplier_name.clone();
        let candidate_url = candidate.url.clone();
        last_attempt_id = Some(candidate_id.clone());
        last_attempt_upstream_model = candidate_upstream_model.clone();
        let Some(secret) = storage
            .find_aggregate_api_secret_by_id(candidate.id.as_str())
            .map_err(|err| err.to_string())?
        else {
            last_attempt_url = Some(candidate_url.clone());
            last_attempt_supplier_name = candidate_supplier_name.clone();
            last_attempt_error = Some("aggregate api secret not found".to_string());
            last_failure_status = 403;
            continue;
        };

        let bridge_responses_to_anthropic = should_bridge_responses_to_anthropic(&candidate, path);
        let effective_path = if bridge_responses_to_anthropic {
            responses_to_anthropic_messages_action_path(&candidate, path)
        } else {
            effective_action_path(&candidate, path)
        };
        let response_adapter_for_candidate = if bridge_responses_to_anthropic {
            super::super::super::ResponseAdapter::ResponsesFromAnthropicMessages
        } else {
            response_adapter
        };
        let (auth_config, injected_headers) = match parse_auth_config(&candidate) {
            Ok(value) => value,
            Err(err) => {
                last_attempt_url = Some(candidate_url.clone());
                last_attempt_supplier_name = candidate_supplier_name.clone();
                last_attempt_error = Some(err);
                last_failure_status = 502;
                continue;
            }
        };

        let mut succeeded = false;
        for attempt_idx in 0..=AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
            if super::super::support::deadline::is_expired(request_deadline) {
                let message = "aggregate api request timeout".to_string();
                let request = request
                    .take()
                    .expect("request should still be available for timeout response");
                super::super::super::record_gateway_request_outcome(
                    path,
                    504,
                    Some("aggregate_api"),
                );
                super::super::super::trace_log::log_request_final(
                    trace_id,
                    504,
                    Some(key_id),
                    Some(candidate_url.as_str()),
                    Some(message.as_str()),
                    started_at.elapsed().as_millis(),
                );
                super::super::super::write_request_log(
                    storage,
                    super::super::super::request_log::RequestLogTraceContext {
                        trace_id: Some(trace_id),
                        original_path: Some(original_path),
                        adapted_path: Some(path),
                        gateway_mode: gateway_mode_for_log,
                        response_adapter: Some(response_adapter),
                        effective_service_tier: effective_service_tier_for_log,
                        aggregate_api_supplier_name: candidate_supplier_name.as_deref(),
                        aggregate_api_url: Some(candidate_url.as_str()),
                        attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
                        upstream_model: candidate_upstream_model.as_deref(),
                        actual_source_kind: Some("aggregate_api"),
                        actual_source_id: Some(candidate_id.as_str()),
                        ..Default::default()
                    },
                    Some(key_id),
                    None,
                    path,
                    request_method,
                    model_for_log,
                    reasoning_for_log,
                    Some(candidate_url.as_str()),
                    Some(504),
                    RequestLogUsage::default(),
                    Some(message.as_str()),
                    Some(started_at.elapsed().as_millis()),
                );
                respond_error(request, 504, message.as_str(), Some(trace_id));
                return Ok(());
            }

            let mut url = match build_upstream_url(candidate_url.as_str(), effective_path.as_str())
            {
                Ok(url) => url,
                Err(_) => {
                    last_attempt_url = Some(candidate_url.clone());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some("invalid aggregate api url".to_string());
                    last_failure_status = 502;
                    break;
                }
            };

            match &auth_config {
                AggregateApiAuthConfig::ApiKeyQuery { name } => {
                    url = replace_query_param(url, name.as_str(), secret.trim());
                }
                AggregateApiAuthConfig::UserPassQueryPair {
                    username_name,
                    password_name,
                } => {
                    let parsed: UserPassSecret = serde_json::from_str(secret.trim())
                        .map_err(|_| "invalid aggregate api secret".to_string())?;
                    url =
                        replace_query_param(url, username_name.as_str(), parsed.username.as_str());
                    url =
                        replace_query_param(url, password_name.as_str(), parsed.password.as_str());
                }
                _ => {}
            }

            let rewritten_body =
                rewrite_body_model_override(body, candidate.model_override.as_deref());
            let upstream_body = if bridge_responses_to_anthropic {
                Bytes::from(adapt_openai_responses_to_anthropic_messages(
                    rewritten_body.as_ref(),
                    candidate.model_override.as_deref(),
                )?)
            } else {
                rewritten_body
            };

            let builder = if bridge_responses_to_anthropic {
                build_anthropic_bridge_aggregate_api_request(
                    &client,
                    request.as_ref().expect("request should still be available"),
                    method,
                    url.clone(),
                    &upstream_body,
                    secret.as_str(),
                    &auth_config,
                    &injected_headers,
                    request_deadline,
                    is_stream,
                )?
            } else {
                build_aggregate_api_request(
                    &client,
                    request.as_ref().expect("request should still be available"),
                    method,
                    url.clone(),
                    &upstream_body,
                    secret.as_str(),
                    &auth_config,
                    &injected_headers,
                    request_deadline,
                    is_stream,
                )?
            };

            let attempt_started_at = Instant::now();
            let upstream = match builder.send() {
                Ok(resp) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(
                        duration_ms,
                        false,
                    );
                    resp
                }
                Err(err) => {
                    let duration_ms =
                        super::super::super::duration_to_millis(attempt_started_at.elapsed());
                    super::super::super::metrics::record_gateway_upstream_attempt(
                        duration_ms,
                        true,
                    );
                    let message = format!("aggregate api upstream error: {err}");
                    last_attempt_url = Some(url.as_str().to_string());
                    last_attempt_supplier_name = candidate_supplier_name.clone();
                    last_attempt_error = Some(message);
                    last_failure_status = 502;
                    if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                        continue;
                    }
                    break;
                }
            };

            if !upstream.status().is_success() {
                let status_code = upstream.status().as_u16();
                let upstream_request_id = first_upstream_header(
                    upstream.headers(),
                    &["x-request-id", "x-oai-request-id"],
                );
                let upstream_cf_ray = first_upstream_header(upstream.headers(), &["cf-ray"]);
                let upstream_auth_error =
                    first_upstream_header(upstream.headers(), &["x-openai-authorization-error"]);
                let upstream_identity_error_code =
                    crate::gateway::extract_identity_error_code_from_headers(upstream.headers());
                let upstream_body = upstream
                    .bytes()
                    .map_err(|err| format!("read upstream body failed: {err}"))?;
                let message = aggregate_api_failure_message(
                    status_code,
                    upstream_body.as_ref(),
                    upstream_request_id.as_deref(),
                    upstream_cf_ray.as_deref(),
                    upstream_auth_error.as_deref(),
                    upstream_identity_error_code.as_deref(),
                );
                last_attempt_url = Some(url.as_str().to_string());
                last_attempt_supplier_name = candidate_supplier_name.clone();
                last_attempt_error = Some(message);
                last_failure_status = 502;
                if attempt_idx < AGGREGATE_API_RETRY_ATTEMPTS_PER_CHANNEL {
                    continue;
                }
                break;
            }

            let inflight_guard = super::super::super::acquire_account_inflight(key_id);
            let passthrough_sse_protocol =
                resolve_passthrough_sse_protocol(path, response_adapter_for_candidate);
            let bridge = super::super::super::respond_with_upstream(
                request
                    .take()
                    .expect("request should be available before bridge"),
                GatewayUpstreamResponse::Blocking(upstream),
                inflight_guard,
                response_adapter_for_candidate,
                passthrough_sse_protocol,
                None,
                path,
                None,
                is_stream,
                false,
                Some(trace_id),
                None,
                started_at,
            )?;
            let bridge_output_text_len = bridge
                .usage
                .output_text
                .as_deref()
                .map(str::trim)
                .map(str::len)
                .unwrap_or(0);
            super::super::super::trace_log::log_bridge_result(
                super::super::super::trace_log::BridgeResultLog {
                    trace_id,
                    adapter: format!("{response_adapter_for_candidate:?}").as_str(),
                    path,
                    is_stream,
                    stream_terminal_seen: bridge.stream_terminal_seen,
                    stream_terminal_error: bridge.stream_terminal_error.as_deref(),
                    delivery_error: bridge.delivery_error.as_deref(),
                    output_text_len: bridge_output_text_len,
                    output_tokens: bridge.usage.output_tokens,
                    first_response_ms: bridge.usage.first_response_ms,
                    delivered_status_code: bridge.delivered_status_code,
                    upstream_error_hint: bridge.upstream_error_hint.as_deref(),
                    upstream_request_id: bridge.upstream_request_id.as_deref(),
                    upstream_cf_ray: bridge.upstream_cf_ray.as_deref(),
                    upstream_auth_error: bridge.upstream_auth_error.as_deref(),
                    upstream_identity_error_code: bridge.upstream_identity_error_code.as_deref(),
                    upstream_content_type: bridge.upstream_content_type.as_deref(),
                    last_sse_event_type: bridge.last_sse_event_type.as_deref(),
                },
            );
            let bridge_ok = bridge.is_ok(is_stream);
            let mut final_error = bridge.upstream_error_hint.clone();
            if final_error.is_none() && !bridge_ok {
                final_error =
                    Some(bridge.error_message(is_stream).unwrap_or_else(|| {
                        "aggregate api upstream response incomplete".to_string()
                    }));
            }
            let status_code =
                bridge
                    .delivered_status_code
                    .unwrap_or_else(|| if bridge_ok { 200 } else { 502 });
            let status_code = if final_error.is_some() && status_code < 400 {
                502
            } else {
                status_code
            };
            let usage = bridge.usage;

            super::super::super::record_gateway_request_outcome(
                path,
                status_code,
                Some("aggregate_api"),
            );
            super::super::super::trace_log::log_request_final(
                trace_id,
                status_code,
                Some(key_id),
                Some(url.as_str()),
                final_error.as_deref(),
                started_at.elapsed().as_millis(),
            );
            super::super::super::write_request_log(
                storage,
                super::super::super::request_log::RequestLogTraceContext {
                    trace_id: Some(trace_id),
                    original_path: Some(original_path),
                    adapted_path: Some(path),
                    gateway_mode: gateway_mode_for_log,
                    response_adapter: Some(response_adapter_for_candidate),
                    effective_service_tier: effective_service_tier_for_log,
                    aggregate_api_supplier_name: candidate_supplier_name.as_deref(),
                    aggregate_api_url: Some(candidate_url.as_str()),
                    attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
                    upstream_model: candidate_upstream_model.as_deref(),
                    actual_source_kind: Some("aggregate_api"),
                    actual_source_id: Some(candidate_id.as_str()),
                    ..Default::default()
                },
                Some(key_id),
                None,
                path,
                request_method,
                model_for_log,
                reasoning_for_log,
                Some(url.as_str()),
                Some(status_code),
                RequestLogUsage {
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    reasoning_output_tokens: usage.reasoning_output_tokens,
                    first_response_ms: usage.first_response_ms,
                },
                final_error.as_deref(),
                Some(started_at.elapsed().as_millis()),
            );
            succeeded = true;
            break;
        }

        if succeeded {
            return Ok(());
        }

        if candidate_idx + 1 < total_candidates {
            super::super::super::record_gateway_failover_attempt();
        }
    }

    let message =
        last_attempt_error.unwrap_or_else(|| "aggregate api upstream response failed".to_string());
    let status_code = last_failure_status;
    let request = request
        .take()
        .expect("request should still be available for failure response");
    super::super::super::record_gateway_request_outcome(path, status_code, Some("aggregate_api"));
    super::super::super::trace_log::log_request_final(
        trace_id,
        status_code,
        Some(key_id),
        last_attempt_url.as_deref(),
        Some(message.as_str()),
        started_at.elapsed().as_millis(),
    );
    super::super::super::write_request_log(
        storage,
        super::super::super::request_log::RequestLogTraceContext {
            trace_id: Some(trace_id),
            original_path: Some(original_path),
            adapted_path: Some(path),
            gateway_mode: gateway_mode_for_log,
            response_adapter: Some(response_adapter),
            effective_service_tier: effective_service_tier_for_log,
            aggregate_api_supplier_name: last_attempt_supplier_name.as_deref(),
            aggregate_api_url: last_attempt_url.as_deref(),
            attempted_aggregate_api_ids: Some(attempted_aggregate_api_ids.as_slice()),
            upstream_model: last_attempt_upstream_model.as_deref(),
            actual_source_kind: last_attempt_id.as_deref().map(|_| "aggregate_api"),
            actual_source_id: last_attempt_id.as_deref(),
            ..Default::default()
        },
        Some(key_id),
        None,
        path,
        request_method,
        model_for_log,
        reasoning_for_log,
        last_attempt_url.as_deref(),
        Some(status_code),
        RequestLogUsage::default(),
        Some(message.as_str()),
        Some(started_at.elapsed().as_millis()),
    );
    respond_error(request, status_code, message.as_str(), Some(trace_id));
    Ok(())
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    /// 函数 `candidate`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - id: 参数 id
    /// - sort: 参数 sort
    ///
    /// # 返回
    /// 返回函数执行结果
    fn candidate(id: &str, sort: i64) -> AggregateApi {
        AggregateApi {
            id: id.to_string(),
            provider_type: AGGREGATE_API_PROVIDER_CODEX.to_string(),
            supplier_name: None,
            sort,
            url: format!("https://{id}.example.com"),
            auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
            auth_params_json: None,
            action: None,
            model_override: None,
            status: "active".to_string(),
            created_at: sort,
            updated_at: sort,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
            balance_query_enabled: false,
            balance_query_template: None,
            balance_query_base_url: None,
            balance_query_user_id: None,
            balance_query_config_json: None,
            last_balance_at: None,
            last_balance_status: None,
            last_balance_error: None,
            last_balance_json: None,
        }
    }

    /// 函数 `ids`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - items: 参数 items
    ///
    /// # 返回
    /// 返回函数执行结果
    fn ids(items: &[AggregateApi]) -> Vec<String> {
        items.iter().map(|item| item.id.clone()).collect()
    }

    /// 函数 `balanced_route_strategy_rotates_aggregate_candidates`
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
    fn balanced_route_strategy_rotates_aggregate_candidates() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var("CODEXMANAGER_ROUTE_STRATEGY").ok();
        std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", "balanced");
        crate::gateway::reload_runtime_config_from_env();

        let mut candidates = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut candidates,
            "gk-aggregate-route-strategy",
            Some("gpt-5.4-mini"),
            None,
        );
        assert_eq!(ids(&candidates), vec!["agg-a", "agg-b", "agg-c"]);

        let mut second = vec![
            candidate("agg-a", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut second,
            "gk-aggregate-route-strategy",
            Some("gpt-5.4-mini"),
            None,
        );
        assert_eq!(ids(&second), vec!["agg-b", "agg-c", "agg-a"]);

        if let Some(value) = previous {
            std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", value);
        } else {
            std::env::remove_var("CODEXMANAGER_ROUTE_STRATEGY");
        }
        crate::gateway::reload_runtime_config_from_env();
    }

    #[test]
    fn aggregate_stream_requests_override_forwarded_accept_header() {
        let injected_headers = HashSet::new();

        assert!(should_skip_forward_header_for_aggregate_request(
            "Accept",
            &injected_headers,
            true,
        ));
        assert!(!should_skip_forward_header_for_aggregate_request(
            "Accept",
            &injected_headers,
            false,
        ));
    }

    /// 函数 `balanced_route_strategy_preserves_explicit_preferred_aggregate_api`
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
    fn balanced_route_strategy_preserves_explicit_preferred_aggregate_api() {
        let _guard = crate::test_env_guard();
        let previous = std::env::var("CODEXMANAGER_ROUTE_STRATEGY").ok();
        std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", "balanced");
        crate::gateway::reload_runtime_config_from_env();

        let mut candidates = vec![
            candidate("agg-preferred", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut candidates,
            "gk-aggregate-route-strategy-preferred",
            Some("gpt-5.4-mini"),
            Some("agg-preferred"),
        );
        assert_eq!(ids(&candidates), vec!["agg-preferred", "agg-b", "agg-c"]);

        let mut second = vec![
            candidate("agg-preferred", 0),
            candidate("agg-b", 1),
            candidate("agg-c", 2),
        ];
        apply_gateway_route_strategy_to_aggregate_candidates(
            &mut second,
            "gk-aggregate-route-strategy-preferred",
            Some("gpt-5.4-mini"),
            Some("agg-preferred"),
        );
        assert_eq!(ids(&second), vec!["agg-preferred", "agg-c", "agg-b"]);

        if let Some(value) = previous {
            std::env::set_var("CODEXMANAGER_ROUTE_STRATEGY", value);
        } else {
            std::env::remove_var("CODEXMANAGER_ROUTE_STRATEGY");
        }
        crate::gateway::reload_runtime_config_from_env();
    }
}

#[cfg(test)]
mod tests {
    use codexmanager_core::storage::{now_ts, AggregateApi, Storage};

    use super::{
        build_anthropic_bridge_aggregate_api_request, build_upstream_url, effective_action_path,
        resolve_aggregate_api_rotation_candidates, resolve_passthrough_sse_protocol,
        responses_to_anthropic_messages_action_path, rewrite_body_model_override,
    };
    use crate::aggregate_api::{
        AGGREGATE_API_AUTH_APIKEY, AGGREGATE_API_PROVIDER_CLAUDE, AGGREGATE_API_PROVIDER_CODEX,
        AGGREGATE_API_PROVIDER_GEMINI,
    };
    use crate::gateway::{PassthroughSseProtocol, ResponseAdapter};
    use bytes::Bytes;

    fn aggregate_api_with_action(action: Option<&str>) -> AggregateApi {
        AggregateApi {
            id: "agg-path-test".to_string(),
            provider_type: "claude".to_string(),
            supplier_name: Some("test".to_string()),
            sort: 0,
            url: "https://open.bigmodel.cn/api/anthropic".to_string(),
            auth_type: "apikey".to_string(),
            auth_params_json: None,
            action: action.map(str::to_string),
            model_override: None,
            status: "active".to_string(),
            created_at: 0,
            updated_at: 0,
            last_test_at: None,
            last_test_status: None,
            last_test_error: None,
            balance_query_enabled: false,
            balance_query_template: None,
            balance_query_base_url: None,
            balance_query_user_id: None,
            balance_query_config_json: None,
            last_balance_at: None,
            last_balance_status: None,
            last_balance_error: None,
            last_balance_json: None,
        }
    }

    #[test]
    fn empty_custom_action_uses_base_url_without_original_path() {
        let api = aggregate_api_with_action(Some(""));
        let path = effective_action_path(&api, "/v1/messages?beta=true");
        assert_eq!(path, "");
    }

    #[test]
    fn messages_passthrough_uses_anthropic_native_terminal_rules_without_provider_gate() {
        let protocol = resolve_passthrough_sse_protocol(
            "/v1/messages?beta=true",
            ResponseAdapter::Passthrough,
        );
        assert_eq!(protocol, Some(PassthroughSseProtocol::AnthropicNative));
    }

    #[test]
    fn messages_passthrough_protocol_still_requires_passthrough_adapter() {
        let protocol = resolve_passthrough_sse_protocol(
            "/v1/messages?beta=true",
            ResponseAdapter::AnthropicMessagesFromResponses,
        );
        assert_eq!(protocol, None);
    }

    #[test]
    fn build_upstream_url_preserves_base_path_prefix() {
        let url = build_upstream_url(
            "https://open.bigmodel.cn/api/anthropic",
            "/v1/messages?beta=true",
        )
        .expect("build upstream url");
        assert_eq!(
            url.as_str(),
            "https://open.bigmodel.cn/api/anthropic/v1/messages?beta=true"
        );
    }

    #[test]
    fn build_upstream_url_keeps_root_base_behavior() {
        let url = build_upstream_url("https://api.example.com", "/v1/messages?beta=true")
            .expect("build upstream url");
        assert_eq!(
            url.as_str(),
            "https://api.example.com/v1/messages?beta=true"
        );
    }

    #[test]
    fn responses_bridge_uses_messages_suffix_for_anthropic_v1_base_url() {
        let mut api = aggregate_api_with_action(None);
        api.url = "https://api.anthropic.com/v1".to_string();

        let path = responses_to_anthropic_messages_action_path(&api, "/v1/responses");
        let url = build_upstream_url(api.url.as_str(), path.as_str()).expect("build upstream url");

        assert_eq!(url.as_str(), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn responses_bridge_keeps_v1_messages_for_deepseek_anthropic_base_url() {
        let mut api = aggregate_api_with_action(None);
        api.url = "https://api.deepseek.com/anthropic".to_string();

        let path = responses_to_anthropic_messages_action_path(&api, "/v1/responses");
        let url = build_upstream_url(api.url.as_str(), path.as_str()).expect("build upstream url");

        assert_eq!(
            url.as_str(),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn responses_bridge_respects_custom_action_path() {
        let mut api = aggregate_api_with_action(Some("/messages?beta=true"));
        api.url = "https://api.anthropic.com/v1".to_string();

        let path = responses_to_anthropic_messages_action_path(&api, "/v1/responses");
        let url = build_upstream_url(api.url.as_str(), path.as_str()).expect("build upstream url");

        assert_eq!(
            path.as_str(),
            "/messages?beta=true",
            "custom action should remain the upstream bridge action"
        );
        assert_eq!(
            url.as_str(),
            "https://api.anthropic.com/v1/messages?beta=true"
        );
    }

    #[test]
    fn anthropic_bridge_request_adds_required_messages_headers_with_default_auth() {
        let request: tiny_http::Request = tiny_http::TestRequest::new()
            .with_header(
                tiny_http::Header::from_bytes("Authorization", "Bearer client-key")
                    .expect("auth header"),
            )
            .into();
        let client = reqwest::blocking::Client::new();
        let builder = build_anthropic_bridge_aggregate_api_request(
            &client,
            &request,
            &reqwest::Method::POST,
            reqwest::Url::parse("https://api.anthropic.com/v1/messages").expect("url"),
            &Bytes::from_static(br#"{"model":"claude-sonnet","messages":[]}"#),
            "sk-ant-test",
            &crate::gateway::upstream::protocol::aggregate_api::AggregateApiAuthConfig::ApiKeyDefaultBearer,
            &std::collections::HashSet::new(),
            None,
            true,
        )
        .expect("build request")
        .build()
        .expect("finalize request");

        assert_eq!(
            builder
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-ant-test")
        );
        assert_eq!(
            builder
                .headers()
                .get("x-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("sk-ant-test")
        );
        assert_eq!(
            builder
                .headers()
                .get("anthropic-version")
                .and_then(|value| value.to_str().ok()),
            Some("2023-06-01")
        );
        assert_eq!(
            builder
                .headers()
                .get("accept")
                .and_then(|value| value.to_str().ok()),
            Some("text/event-stream")
        );
    }

    #[test]
    fn rewrite_body_model_override_replaces_json_model() {
        let body = Bytes::from_static(br#"{"model":"claude-sonnet","messages":[]}"#);

        let rewritten = rewrite_body_model_override(&body, Some("qwen3.5-plus"));

        let value: serde_json::Value =
            serde_json::from_slice(rewritten.as_ref()).expect("parse rewritten body");
        assert_eq!(value["model"], "qwen3.5-plus");
        assert_eq!(value["messages"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn gemini_native_candidates_resolve_to_gemini_provider_only() {
        let storage = Storage::open_in_memory().expect("open storage");
        storage.init().expect("init storage");
        let now = now_ts();
        for (id, provider_type) in [
            ("agg-codex", AGGREGATE_API_PROVIDER_CODEX),
            ("agg-claude", AGGREGATE_API_PROVIDER_CLAUDE),
            ("agg-gemini", AGGREGATE_API_PROVIDER_GEMINI),
        ] {
            storage
                .insert_aggregate_api(&AggregateApi {
                    id: id.to_string(),
                    provider_type: provider_type.to_string(),
                    supplier_name: Some(id.to_string()),
                    sort: 0,
                    url: format!("https://{id}.example.com"),
                    auth_type: AGGREGATE_API_AUTH_APIKEY.to_string(),
                    auth_params_json: None,
                    action: None,
                    model_override: None,
                    status: "active".to_string(),
                    created_at: now,
                    updated_at: now,
                    last_test_at: None,
                    last_test_status: None,
                    last_test_error: None,
                    balance_query_enabled: false,
                    balance_query_template: None,
                    balance_query_base_url: None,
                    balance_query_user_id: None,
                    balance_query_config_json: None,
                    last_balance_at: None,
                    last_balance_status: None,
                    last_balance_error: None,
                    last_balance_json: None,
                })
                .expect("insert aggregate api");
        }

        let candidates = resolve_aggregate_api_rotation_candidates(&storage, "gemini_native", None)
            .expect("resolve gemini candidates");
        let candidate_ids = candidates
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(candidate_ids, vec!["agg-gemini"]);
    }

    /// 函数 `final_error_promotes_success_status_to_bad_gateway`
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
    fn final_error_promotes_success_status_to_bad_gateway() {
        let status_code = bridge_status_code(Some(200), true, Some("unsupported model"));
        assert_eq!(status_code, 502);
    }

    /// 函数 `successful_bridge_keeps_success_status`
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
    fn successful_bridge_keeps_success_status() {
        let status_code = bridge_status_code(Some(200), true, None);
        assert_eq!(status_code, 200);
    }

    /// 函数 `incomplete_bridge_without_status_defaults_to_bad_gateway`
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
    fn incomplete_bridge_without_status_defaults_to_bad_gateway() {
        let status_code = bridge_status_code(None, false, None);
        assert_eq!(status_code, 502);
    }

    /// 函数 `bridge_status_code`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - delivered_status_code: 参数 delivered_status_code
    /// - bridge_ok: 参数 bridge_ok
    /// - final_error: 参数 final_error
    ///
    /// # 返回
    /// 返回函数执行结果
    fn bridge_status_code(
        delivered_status_code: Option<u16>,
        bridge_ok: bool,
        final_error: Option<&str>,
    ) -> u16 {
        let status_code =
            delivered_status_code.unwrap_or_else(|| if bridge_ok { 200 } else { 502 });
        if final_error.is_some() && status_code < 400 {
            502
        } else {
            status_code
        }
    }
}
