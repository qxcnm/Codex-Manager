use bytes::Bytes;
use codexmanager_core::storage::{Account, Storage, Token};
use reqwest::blocking::Client;
use reqwest::Method;
use serde_json::Value;
use std::time::Instant;

/// 函数 `should_force_connection_close`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - target_url: 参数 target_url
///
/// # 返回
/// 返回函数执行结果
fn should_force_connection_close(target_url: &str) -> bool {
    reqwest::Url::parse(target_url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        .is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1"))
}

/// 函数 `force_connection_close`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
///
/// # 返回
/// 无
fn force_connection_close(headers: &mut Vec<(String, String)>) {
    if let Some((_, value)) = headers
        .iter_mut()
        .find(|(name, _)| name.eq_ignore_ascii_case("connection"))
    {
        *value = "close".to_string();
    } else {
        headers.push(("Connection".to_string(), "close".to_string()));
    }
}

/// 函数 `body_has_encrypted_content_hint`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn body_has_encrypted_content_hint(body: &[u8]) -> bool {
    // Fast path: avoid JSON parsing unless we hit the recovery path.
    std::str::from_utf8(body)
        .ok()
        .is_some_and(|text| text.contains("\"encrypted_content\""))
}

/// 函数 `strip_encrypted_content_value`
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
fn strip_encrypted_content_value(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = map.remove("encrypted_content").is_some();
            for v in map.values_mut() {
                if strip_encrypted_content_value(v) {
                    changed = true;
                }
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items.iter_mut() {
                if strip_encrypted_content_value(item) {
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

/// 函数 `strip_encrypted_content_from_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn strip_encrypted_content_from_body(body: &[u8]) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body).ok()?;
    if !strip_encrypted_content_value(&mut value) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

/// 函数 `extract_prompt_cache_key`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn extract_prompt_cache_key(body: &[u8]) -> Option<String> {
    if body.is_empty() || body.len() > 64 * 1024 {
        return None;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return None;
    };
    value
        .get("prompt_cache_key")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 函数 `is_compact_request_path`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - path: 参数 path
///
/// # 返回
/// 返回函数执行结果
fn is_compact_request_path(path: &str) -> bool {
    path == "/v1/responses/compact" || path.starts_with("/v1/responses/compact?")
}

/// 函数 `resolve_chatgpt_account_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account: 参数 account
/// - upstream_base: 参数 upstream_base
///
/// # 返回
/// 返回函数执行结果
fn resolve_chatgpt_account_header<'a>(
    account: &'a Account,
    upstream_base: &str,
) -> Option<&'a str> {
    if !super::upstream::config::should_send_chatgpt_account_header(upstream_base) {
        return None;
    }
    account
        .chatgpt_account_id
        .as_deref()
        .or(account.workspace_id.as_deref())
}

/// 函数 `try_openai_fallback`
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
pub(super) fn try_openai_fallback(
    client: &Client,
    storage: &Storage,
    method: &Method,
    request_path: &str,
    incoming_headers: &super::IncomingHeaderSnapshot,
    body: &Bytes,
    _is_stream: bool,
    upstream_base: &str,
    account: &Account,
    token: &mut Token,
    strip_session_affinity: bool,
    debug: bool,
) -> Result<Option<reqwest::blocking::Response>, String> {
    let (url, _url_alt) = super::compute_upstream_url(upstream_base, request_path);
    let bearer = super::resolve_openai_bearer_token(storage, account, token)?;
    let attempt_started_at = Instant::now();
    let is_openai_api_target = super::is_openai_api_base(upstream_base);

    // `x-codex-turn-state` is an org-scoped encrypted blob. When we hit API-key fallback
    // (often a different org than the ChatGPT workspace), forwarding it can trigger:
    // `invalid_encrypted_content` / organization_id mismatch. In that case, prefer
    // resetting session affinity to keep the request usable.
    let strip_session_affinity =
        strip_session_affinity || incoming_headers.turn_state().is_some() || is_openai_api_target;
    let body_for_request =
        if strip_session_affinity && body_has_encrypted_content_hint(body.as_ref()) {
            strip_encrypted_content_from_body(body.as_ref())
                .map(Bytes::from)
                .unwrap_or_else(|| body.clone())
        } else {
            body.clone()
        };
    let prompt_cache_key = if strip_session_affinity {
        None
    } else {
        extract_prompt_cache_key(body_for_request.as_ref())
    };
    let request_affinity = super::session_affinity::derive_outgoing_session_affinity(
        incoming_headers.session_id(),
        incoming_headers.client_request_id(),
        incoming_headers.turn_state(),
        incoming_headers.conversation_id(),
        prompt_cache_key.as_deref(),
    );

    let account_id = account
        .chatgpt_account_id
        .as_deref()
        .or_else(|| account.workspace_id.as_deref());
    super::session_affinity::log_thread_anchor_conflict(
        request_path,
        account_id,
        incoming_headers.conversation_id(),
        prompt_cache_key.as_deref(),
    );
    super::session_affinity::log_outgoing_session_affinity(
        request_path,
        account_id,
        incoming_headers.session_id(),
        incoming_headers.client_request_id(),
        incoming_headers.turn_state(),
        incoming_headers.conversation_id(),
        prompt_cache_key.as_deref(),
        request_affinity,
        strip_session_affinity,
    );
    let mut upstream_headers = if is_compact_request_path(request_path) {
        let header_input = super::upstream::header_profile::CodexCompactUpstreamHeaderInput {
            auth_token: bearer.as_str(),
            chatgpt_account_id: resolve_chatgpt_account_header(account, upstream_base),
            installation_id: None,
            incoming_user_agent: incoming_headers.user_agent(),
            incoming_originator: incoming_headers.originator(),
            preserve_client_identity: false,
            incoming_session_id: request_affinity.incoming_session_id,
            thread_id: request_affinity.fallback_session_id,
            incoming_window_id: incoming_headers.window_id(),
            incoming_subagent: incoming_headers.subagent(),
            incoming_parent_thread_id: incoming_headers.parent_thread_id(),
            incoming_oai_attestation: incoming_headers.oai_attestation(),
            passthrough_codex_headers: incoming_headers.passthrough_codex_headers(),
            fallback_session_id: request_affinity.fallback_session_id,
            strip_session_affinity,
            has_body: !body.is_empty(),
        };
        super::upstream::header_profile::build_codex_compact_upstream_headers(header_input)
    } else {
        let header_input = super::upstream::header_profile::CodexUpstreamHeaderInput {
            auth_token: bearer.as_str(),
            chatgpt_account_id: resolve_chatgpt_account_header(account, upstream_base),
            incoming_user_agent: incoming_headers.user_agent(),
            incoming_originator: incoming_headers.originator(),
            preserve_client_identity: false,
            incoming_session_id: request_affinity.incoming_session_id,
            incoming_window_id: incoming_headers.window_id(),
            incoming_client_request_id: request_affinity.incoming_client_request_id,
            incoming_subagent: incoming_headers.subagent(),
            incoming_beta_features: incoming_headers.beta_features(),
            incoming_turn_metadata: incoming_headers.turn_metadata(),
            incoming_parent_thread_id: incoming_headers.parent_thread_id(),
            incoming_responsesapi_include_timing_metrics: incoming_headers
                .responsesapi_include_timing_metrics(),
            incoming_inference_call_id: incoming_headers.codex_inference_call_id(),
            incoming_oai_attestation: incoming_headers.oai_attestation(),
            passthrough_codex_headers: incoming_headers.passthrough_codex_headers(),
            fallback_session_id: request_affinity.fallback_session_id,
            incoming_turn_state: request_affinity.incoming_turn_state,
            include_turn_state: !is_openai_api_target,
            strip_session_affinity,
            has_body: !body.is_empty(),
        };
        super::upstream::header_profile::build_codex_upstream_headers(header_input)
    };
    if should_force_connection_close(&url) {
        force_connection_close(&mut upstream_headers);
    }
    if debug {
        log::debug!(
            "event=gateway_upstream_token_source path={} account_id={} token_source=api_key_access_token upstream_base={}",
            request_path,
            account_id.unwrap_or("-"),
            upstream_base
        );
    }
    let build_request = |http: &Client| {
        let mut builder = http.request(method.clone(), &url);
        for (name, value) in upstream_headers.iter() {
            builder = builder.header(name, value);
        }
        if !body_for_request.is_empty() {
            builder = builder.body(body_for_request.clone());
        }
        builder
    };
    let resp = match build_request(client).send() {
        Ok(resp) => resp,
        Err(first_err) => {
            let fresh = match super::fresh_upstream_client_for_account(account.id.as_str()) {
                Ok(client) => client,
                Err(fresh_err) => {
                    return Err(format!(
                        "{}; retry_after_fresh_client_build: {}",
                        first_err, fresh_err
                    ));
                }
            };
            match build_request(&fresh).send() {
                Ok(resp) => {
                    log::info!(
                        "event=gateway_openai_fallback_retry_with_fresh_client_succeeded path={} account_id={} upstream_base={}",
                        request_path,
                        account.id,
                        upstream_base
                    );
                    resp
                }
                Err(second_err) => {
                    let duration_ms = super::duration_to_millis(attempt_started_at.elapsed());
                    super::metrics::record_gateway_upstream_attempt(duration_ms, true);
                    log::warn!(
                        "event=gateway_openai_fallback_retry_with_fresh_client_failed path={} account_id={} upstream_base={} first_err={} retry_err={}",
                        request_path,
                        account.id,
                        upstream_base,
                        first_err,
                        second_err
                    );
                    return Err(format!(
                        "{}; retry_after_fresh_client: {}",
                        first_err, second_err
                    ));
                }
            }
        }
    };
    let duration_ms = super::duration_to_millis(attempt_started_at.elapsed());
    super::metrics::record_gateway_upstream_attempt(duration_ms, false);
    Ok(Some(resp))
}
