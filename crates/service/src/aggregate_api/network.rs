//! Provider probes, model discovery, and balance HTTP requests use native async I/O.
use super::*;

pub(super) async fn send_aggregate_api_request(
    client: &reqwest::Client,
    builder: reqwest::RequestBuilder,
    api: &AggregateApi,
) -> Result<reqwest::Response, String> {
    let user_agent = resolved_aggregate_api_user_agent(api)?;
    let mut request = builder
        .build()
        .map_err(|err| format!("build aggregate api request failed: {err}"))?;
    request.headers_mut().insert(
        reqwest::header::USER_AGENT,
        HeaderValue::from_str(user_agent.as_str())
            .map_err(|_| "aggregate api user agent is not a valid HTTP header value".to_string())?,
    );
    client.execute(request).await.map_err(|err| err.to_string())
}

pub(super) fn apply_probe_auth(
    mut builder: reqwest::RequestBuilder,
    mut url: String,
    api: &AggregateApi,
    secret: &str,
) -> Result<(reqwest::RequestBuilder, String), String> {
    let auth_type = normalize_auth_type(Some(api.auth_type.clone()))?;
    let auth_params = api
        .auth_params_json
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if auth_type == AGGREGATE_API_AUTH_USERPASS {
        let parsed: UserPassSecret = serde_json::from_str(secret.trim())
            .map_err(|_| "invalid aggregate api secret".to_string())?;
        if let Some(raw) = auth_params {
            let params: UserPassAuthParams =
                serde_json::from_str(raw).map_err(|_| "invalid authParams".to_string())?;
            let mode = params.mode.trim().to_ascii_lowercase();
            if mode == "headerpair" {
                let username_name = params.username_name.as_deref().unwrap_or("username").trim();
                let password_name = params.password_name.as_deref().unwrap_or("password").trim();
                builder = builder
                    .header(username_name, parsed.username.as_str())
                    .header(password_name, parsed.password.as_str());
                return Ok((builder, url));
            }
            if mode == "querypair" {
                let username_name = params.username_name.as_deref().unwrap_or("username").trim();
                let password_name = params.password_name.as_deref().unwrap_or("password").trim();
                url = with_query_param(url.as_str(), username_name, parsed.username.as_str());
                url = with_query_param(url.as_str(), password_name, parsed.password.as_str());
                return Ok((builder, url));
            }
        }
        builder = builder.basic_auth(parsed.username, Some(parsed.password));
        return Ok((builder, url));
    }

    if let Some(raw) = auth_params {
        let params: ApiKeyAuthParams =
            serde_json::from_str(raw).map_err(|_| "invalid authParams".to_string())?;
        let location = params.location.trim().to_ascii_lowercase();
        if location == "query" {
            url = with_query_param(url.as_str(), params.name.trim(), secret.trim());
            return Ok((builder, url));
        }
        let value_format = params
            .header_value_format
            .as_deref()
            .unwrap_or("bearer")
            .trim()
            .to_ascii_lowercase();
        let header_value = if value_format == "raw" {
            secret.trim().to_string()
        } else {
            format!("Bearer {}", secret.trim())
        };
        builder = builder.header(params.name.trim(), header_value);
        return Ok((builder, url));
    }

    let auth_value = format!("Bearer {}", secret.trim());
    builder = builder
        .header(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(auth_value.as_str())
                .map_err(|_| "invalid aggregate api key".to_string())?,
        )
        .header("x-api-key", secret.trim())
        .header("api-key", secret.trim());
    Ok((builder, url))
}

pub(super) async fn read_first_chunk(mut response: reqwest::Response) -> Result<(), String> {
    while let Some(chunk) = response.chunk().await.map_err(|err| err.to_string())? {
        if !chunk.is_empty() {
            return Ok(());
        }
    }
    Err("No response data received".to_string())
}

pub(super) fn apply_balance_auth(
    client: &reqwest::Client,
    url: String,
    api: &AggregateApi,
    secret: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let builder = client.get(url.as_str());
    let (builder, updated_url) = apply_probe_auth(builder, url.clone(), api, secret)?;
    if updated_url == url {
        return Ok(builder);
    }
    let rebuilt = client.get(updated_url.as_str());
    let (rebuilt, _) = apply_probe_auth(rebuilt, updated_url, api, secret)?;
    Ok(rebuilt)
}

pub(super) async fn read_json_response(
    response: reqwest::Response,
) -> Result<serde_json::Value, String> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(|err| err.to_string())?;
    let body = String::from_utf8_lossy(bytes.as_ref()).to_string();
    if !status.is_success() {
        let detail = short_error_body(body.as_str());
        if detail.is_empty() {
            return Err(format!("balance query http_status={}", status.as_u16()));
        }
        return Err(format!(
            "balance query http_status={}; {detail}",
            status.as_u16()
        ));
    }
    serde_json::from_str(body.as_str())
        .map_err(|_| "balance response is not valid JSON".to_string())
}

pub(super) async fn query_generic_balance_path(
    client: &reqwest::Client,
    api: &AggregateApi,
    secret: &str,
    base_url: &str,
    path: &str,
) -> Result<AggregateApiBalanceSnapshot, String> {
    let url = join_api_path(base_url, path);
    let builder = apply_balance_auth(client, url, api, secret)?
        .header("accept", "application/json")
        .header("accept-encoding", "identity");
    let response = send_aggregate_api_request(client, builder, api).await?;
    let value = read_json_response(response).await?;
    extract_generic_balance(&value)
}

pub(super) async fn query_generic_balance(
    client: &reqwest::Client,
    api: &AggregateApi,
    secret: &str,
) -> Result<AggregateApiBalanceSnapshot, String> {
    let base_url = balance_query_base_url(api, AGGREGATE_API_BALANCE_TEMPLATE_GENERIC);
    match query_generic_balance_path(client, api, secret, base_url.as_str(), "/user/balance").await
    {
        Ok(snapshot) => Ok(snapshot),
        Err(err) if should_try_usage_balance_fallback(err.as_str()) => {
            let usage_base_url = balance_query_usage_base_url(api);
            query_generic_balance_path(client, api, secret, usage_base_url.as_str(), "/v1/usage")
                .await
                .map_err(|fallback_err| format!("{err}; fallback /v1/usage failed: {fallback_err}"))
        }
        Err(err) => Err(err),
    }
}

pub(super) async fn query_custom_balance(
    client: &reqwest::Client,
    api: &AggregateApi,
    provider_secret: &str,
    balance_secret: Option<String>,
) -> Result<AggregateApiBalanceSnapshot, String> {
    let config = parse_custom_balance_query_config(api.balance_query_config_json.as_deref())?;
    let base_url = balance_query_base_url(api, AGGREGATE_API_BALANCE_TEMPLATE_CUSTOM);
    let url = join_api_path(base_url.as_str(), config.path.as_str());
    let method = config.method.as_deref().unwrap_or("GET");
    let mut builder = if method == "POST" {
        client.post(url.as_str())
    } else {
        client.get(url.as_str())
    }
    .header("accept", "application/json")
    .header("accept-encoding", "identity");
    match config
        .auth
        .as_deref()
        .unwrap_or(CUSTOM_BALANCE_AUTH_PROVIDER_BEARER)
    {
        CUSTOM_BALANCE_AUTH_NONE => {}
        CUSTOM_BALANCE_AUTH_BALANCE_BEARER => {
            let access_token = balance_secret
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| provider_secret.trim());
            if access_token.is_empty() {
                return Err("custom balance access token is required".to_string());
            }
            builder = builder.bearer_auth(access_token);
        }
        _ => {
            let access_token = provider_secret.trim();
            if access_token.is_empty() {
                return Err("aggregate api secret is required".to_string());
            }
            builder = builder.bearer_auth(access_token);
        }
    }
    let response = send_aggregate_api_request(client, builder, api).await?;
    let value = read_json_response(response).await?;
    extract_custom_balance(&value, &config)
}

pub(super) async fn query_new_api_balance(
    client: &reqwest::Client,
    api: &AggregateApi,
    provider_secret: &str,
    balance_secret: Option<String>,
) -> Result<AggregateApiBalanceSnapshot, String> {
    let base_url = balance_query_base_url(api, AGGREGATE_API_BALANCE_TEMPLATE_NEW_API);
    let url = join_api_path(base_url.as_str(), "/api/user/self");
    let access_token = balance_secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| provider_secret.trim());
    if access_token.is_empty() {
        return Err("balance access token is required".to_string());
    }
    let mut builder = client
        .get(url.as_str())
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("accept-encoding", "identity")
        .bearer_auth(access_token);
    if let Some(user_id) = api
        .balance_query_user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        builder = builder.header("New-Api-User", user_id);
    }
    let response = send_aggregate_api_request(client, builder, api).await?;
    let value = read_json_response(response).await?;
    extract_new_api_balance(&value)
}

pub(super) async fn probe_http_error(
    probe: &str,
    status_code: u16,
    response: reqwest::Response,
) -> String {
    let detail = response.bytes().await.ok().and_then(|body| {
        gateway::summarize_upstream_error_hint_from_body(status_code, body.as_ref()).or_else(|| {
            let detail = short_error_body(String::from_utf8_lossy(body.as_ref()).as_ref());
            (!detail.is_empty()).then_some(detail)
        })
    });
    match detail {
        Some(detail) => format!("{probe} probe http_status={status_code}; {detail}"),
        None => format!("{probe} probe http_status={status_code}"),
    }
}

pub(super) fn add_codex_probe_headers(
    mut builder: reqwest::RequestBuilder,
) -> Result<reqwest::RequestBuilder, String> {
    let request_id = gateway::next_trace_id();
    builder = builder
        .header("originator", gateway::current_wire_originator())
        .header("session-id", request_id.as_str())
        .header("thread-id", request_id.as_str())
        .header("x-client-request-id", request_id.as_str())
        .header("x-codex-window-id", format!("{request_id}:0"));
    Ok(builder
        .header("accept", "application/json")
        .header("accept-encoding", "identity"))
}

pub(super) async fn probe_codex_responses_endpoint(
    client: &reqwest::Client,
    api: &AggregateApi,
    secret: &str,
    model: &str,
) -> Result<i64, String> {
    let action_hint = api
        .action
        .as_deref()
        .map(str::trim)
        .unwrap_or("/responses")
        .to_ascii_lowercase();
    let default_path = if action_hint.contains("chat/completions") {
        "/chat/completions"
    } else {
        "/responses"
    };
    let probe_path = action_path_or_default(api, default_path);
    let url = normalize_probe_url(api.url.as_str(), probe_path.as_str());
    let builder = client.post(url.as_str());
    let (builder, updated_url) = apply_probe_auth(builder, url.clone(), api, secret)?;
    let builder = if updated_url != url {
        let rebuilt = client.post(updated_url.as_str());
        let (rebuilt, _) = apply_probe_auth(rebuilt, updated_url, api, secret)?;
        rebuilt
    } else {
        builder
    };
    let request_body = if probe_path.to_ascii_lowercase().contains("chat/completions") {
        json!({
            "model": model,
            "messages": [{"role":"user","content":"hi"}],
            "stream": false
        })
    } else if is_minimax_aggregate_api(api) {
        json!({
            "model": model,
            "input": "Who are you?",
            "stream": false
        })
    } else {
        build_codex_probe_body(model)
    };
    let builder = add_codex_probe_headers(builder)?
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .json(&request_body);
    let response = send_aggregate_api_request(client, builder, api).await?;

    let status_code = response.status().as_u16() as i64;
    if !response.status().is_success() {
        return Err(probe_http_error("codex", status_code as u16, response).await);
    }
    read_first_chunk(response).await?;
    Ok(status_code)
}

pub(super) async fn probe_codex_endpoint(
    client: &reqwest::Client,
    api: &AggregateApi,
    secret: &str,
    model: &str,
) -> Result<i64, String> {
    probe_codex_responses_endpoint(client, api, secret, model).await
}

pub(super) async fn probe_claude_endpoint(
    client: &reqwest::Client,
    api: &AggregateApi,
    secret: &str,
    model: &str,
) -> Result<i64, String> {
    let probe_path = action_path_or_default(api, "/messages?beta=true");
    let url = normalize_probe_url(api.url.as_str(), probe_path.as_str());
    let builder = client.post(url.as_str());
    let (builder, updated_url) = apply_probe_auth(builder, url.clone(), api, secret)?;
    let builder = if updated_url != url {
        let rebuilt = client.post(updated_url.as_str());
        let (rebuilt, _) = apply_probe_auth(rebuilt, updated_url, api, secret)?;
        rebuilt
    } else {
        builder
    };
    let builder = builder
        .header("anthropic-version", "2023-06-01")
        .header(
            "anthropic-beta",
            "claude-code-20250219,interleaved-thinking-2025-05-14",
        )
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("accept-encoding", "identity")
        .header("x-app", "cli")
        .json(&build_claude_probe_body(model));
    let response = send_aggregate_api_request(client, builder, api).await?;
    let status_code = response.status().as_u16() as i64;
    if !response.status().is_success() {
        return Err(probe_http_error("claude", status_code as u16, response).await);
    }
    read_first_chunk(response).await?;
    Ok(status_code)
}

pub(super) async fn probe_gemini_endpoint(
    client: &reqwest::Client,
    api: &AggregateApi,
    secret: &str,
    model: &str,
) -> Result<i64, String> {
    let default_path = format!("/v1beta/models/{model}:generateContent");
    let probe_path = action_path_or_default(api, default_path.as_str());
    let url = normalize_probe_url(api.url.as_str(), probe_path.as_str());
    let builder = client.post(url.as_str());
    let (builder, updated_url) = apply_probe_auth(builder, url.clone(), api, secret)?;
    let builder = if updated_url != url {
        let rebuilt = client.post(updated_url.as_str());
        let (rebuilt, _) = apply_probe_auth(rebuilt, updated_url, api, secret)?;
        rebuilt
    } else {
        builder
    };
    let builder = builder
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("accept-encoding", "identity")
        .json(&build_gemini_probe_body());
    let response = send_aggregate_api_request(client, builder, api).await?;

    let status_code = response.status().as_u16() as i64;
    if !response.status().is_success() {
        return Err(probe_http_error("gemini", status_code as u16, response).await);
    }
    read_first_chunk(response).await?;
    Ok(status_code)
}

pub(super) async fn read_models_response(mut response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_MODELS_RESPONSE_BYTES)
    {
        return Err("models response is too large".to_string());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("models request failed: {err}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_MODELS_RESPONSE_BYTES {
            return Err("models response is too large".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let detail = short_error_body(String::from_utf8_lossy(bytes.as_ref()).as_ref());
        return if detail.is_empty() {
            Err(format!("models http_status={}", status.as_u16()))
        } else {
            Err(format!("models http_status={}; {detail}", status.as_u16()))
        };
    }
    serde_json::from_slice(bytes.as_ref())
        .map_err(|_| "models response is not valid JSON".to_string())
}
