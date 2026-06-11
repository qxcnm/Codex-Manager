use bytes::Bytes;
use codexmanager_core::storage::Account;
use futures_util::StreamExt;
use rand::Rng;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tiny_http::Request;
use tokio::runtime::Builder;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{
    HeaderMap as WsHeaderMap, HeaderName as WsHeaderName, HeaderValue as WsHeaderValue,
};

use super::super::GatewayUpstreamResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestCompression {
    None,
    Zstd,
}

#[derive(Debug, Clone, Copy)]
pub(in super::super) struct UpstreamRequestContext<'a> {
    pub(in super::super) request_path: &'a str,
    pub(in super::super) protocol_type: &'a str,
}

impl<'a> UpstreamRequestContext<'a> {
    /// 函数 `from_request`
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
    pub(in super::super) fn from_request(request: &'a Request, protocol_type: &'a str) -> Self {
        Self {
            request_path: request.url(),
            protocol_type,
        }
    }
}

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
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn strip_compact_service_tier_for_transport(body: &Bytes, preserve_service_tier: bool) -> Bytes {
    if preserve_service_tier || body.is_empty() {
        return body.clone();
    }
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.clone();
    };
    let Some(object) = value.as_object_mut() else {
        return body.clone();
    };
    if object.remove("service_tier").is_none() {
        return body.clone();
    }
    serde_json::to_vec(&value)
        .map(Bytes::from)
        .unwrap_or_else(|_| body.clone())
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

fn should_preserve_client_identity(protocol_type: &str) -> bool {
    let _ = protocol_type;
    false
}

fn is_gemini_codex_compat(protocol_type: &str, request_path: &str, target_url: &str) -> bool {
    protocol_type == crate::apikey_profile::PROTOCOL_GEMINI_NATIVE
        && request_path.starts_with("/v1/responses")
        && super::super::config::is_chatgpt_backend_base(target_url)
}

fn is_anthropic_codex_compat(protocol_type: &str, request_path: &str, target_url: &str) -> bool {
    protocol_type == crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE
        && request_path.starts_with("/v1/responses")
        && super::super::config::is_chatgpt_backend_base(target_url)
}

const CPA_GEMINI_CODEX_USER_AGENT: &str =
    "codex-tui/0.118.0 (Mac OS 26.3.1; arm64) iTerm.app/3.6.9 (codex-tui; 0.118.0)";
const CPA_GEMINI_CODEX_ORIGINATOR: &str = "codex-tui";

fn normalize_header_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn set_or_replace_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    if let Some((_, current)) = headers
        .iter_mut()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    {
        *current = value;
    } else {
        headers.push((name.to_string(), value));
    }
}

fn remove_header(headers: &mut Vec<(String, String)>, name: &str) {
    headers.retain(|(header_name, _)| !header_name.eq_ignore_ascii_case(name));
}

fn random_cpa_session_id() -> String {
    let mut rng = rand::thread_rng();
    let a: u32 = rng.gen();
    let b: u16 = rng.gen();
    let c: u16 = (rng.gen::<u16>() & 0x0fff) | 0x4000;
    let d: u16 = (rng.gen::<u16>() & 0x3fff) | 0x8000;
    let e: u64 = rng.gen::<u64>() & 0x0000_ffff_ffff_ffff;
    format!("{a:08x}-{b:04x}-{c:04x}-{d:04x}-{e:012x}")
}

fn apply_gemini_codex_compat_header_profile(
    headers: &mut Vec<(String, String)>,
    incoming_originator: Option<&str>,
) {
    set_or_replace_header(
        headers,
        "User-Agent",
        CPA_GEMINI_CODEX_USER_AGENT.to_string(),
    );
    set_or_replace_header(
        headers,
        "originator",
        normalize_header_value(incoming_originator)
            .unwrap_or(CPA_GEMINI_CODEX_ORIGINATOR)
            .to_string(),
    );
    set_or_replace_header(headers, "Connection", "Keep-Alive".to_string());
    // 中文注释：CPA 的 Gemini->Codex 兼容路径只补 Session_id，不带窗口/turn 粘性头。
    remove_header(headers, "x-codex-window-id");
    remove_header(headers, "x-codex-turn-state");
    remove_header(headers, "x-codex-parent-thread-id");
    remove_header(headers, "x-openai-subagent");
    if !has_header(headers, "session_id") {
        headers.push(("Session_id".to_string(), random_cpa_session_id()));
    }
}

/// 函数 `has_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - headers: 参数 headers
/// - name: 参数 name
///
/// # 返回
/// 返回函数执行结果
fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}

/// 函数 `resolve_chatgpt_account_header`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - account: 参数 account
/// - target_url: 参数 target_url
///
/// # 返回
/// 返回函数执行结果
fn resolve_chatgpt_account_header<'a>(account: &'a Account, target_url: &str) -> Option<&'a str> {
    if !super::super::config::should_send_chatgpt_account_header(target_url) {
        return None;
    }
    account
        .chatgpt_account_id
        .as_deref()
        .or(account.workspace_id.as_deref())
}

/// 函数 `resolve_request_compression_with_flag`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - enabled: 参数 enabled
/// - target_url: 参数 target_url
/// - request_path: 参数 request_path
/// - is_stream: 参数 is_stream
///
/// # 返回
/// 返回函数执行结果
fn resolve_request_compression_with_flag(
    enabled: bool,
    target_url: &str,
    request_path: &str,
    is_stream: bool,
) -> RequestCompression {
    if !enabled {
        return RequestCompression::None;
    }
    if !is_stream {
        return RequestCompression::None;
    }
    if is_compact_request_path(request_path) || !request_path.starts_with("/v1/responses") {
        return RequestCompression::None;
    }
    if !super::super::config::is_chatgpt_backend_base(target_url) {
        return RequestCompression::None;
    }
    RequestCompression::Zstd
}

/// 函数 `resolve_request_compression`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - target_url: 参数 target_url
/// - request_path: 参数 request_path
/// - is_stream: 参数 is_stream
///
/// # 返回
/// 返回函数执行结果
fn resolve_request_compression(
    protocol_type: &str,
    target_url: &str,
    request_path: &str,
    is_stream: bool,
) -> RequestCompression {
    if is_gemini_codex_compat(protocol_type, request_path, target_url) {
        // 中文注释：CPA 的 Gemini->Codex 路径不做 zstd 请求压缩。
        return RequestCompression::None;
    }
    if is_anthropic_codex_compat(protocol_type, request_path, target_url) {
        // 中文注释：Claude Code 兼容路径经过 /v1/responses 适配，但不是原生 Codex 客户端；
        // 禁用 zstd 请求压缩可降低 ChatGPT 边缘把兼容请求误判为 challenge 的概率。
        return RequestCompression::None;
    }
    resolve_request_compression_with_flag(
        super::super::super::request_compression_enabled(),
        target_url,
        request_path,
        is_stream,
    )
}

fn should_retry_transport_without_compression(
    target_url: &str,
    request_path: &str,
    is_stream: bool,
    compression: RequestCompression,
) -> bool {
    compression == RequestCompression::Zstd
        && is_stream
        && request_path.starts_with("/v1/responses")
        && !is_compact_request_path(request_path)
        && super::super::config::is_chatgpt_backend_base(target_url)
}

fn should_wrap_upstream_as_stream_response(request_path: &str, is_stream: bool) -> bool {
    is_stream && request_path.starts_with("/v1/responses") && !is_compact_request_path(request_path)
}

const STREAM_ERROR_PREVIEW_MAX_BYTES: usize = 64 * 1024;
const STREAM_ERROR_PREVIEW_TIMEOUT: Duration = Duration::from_millis(250);

fn first_header_value<'a>(headers: &'a reqwest::header::HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn header_value_contains(headers: &reqwest::header::HeaderMap, name: &str, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    first_header_value(headers, name)
        .map(str::to_ascii_lowercase)
        .is_some_and(|value| value.contains(needle.as_str()))
}

fn content_type_is(headers: &reqwest::header::HeaderMap, needle: &str) -> bool {
    header_value_contains(headers, reqwest::header::CONTENT_TYPE.as_str(), needle)
}

fn has_cloudflare_header_signal(headers: &reqwest::header::HeaderMap) -> bool {
    first_header_value(headers, "cf-ray").is_some()
        || header_value_contains(headers, "server", "cloudflare")
        || header_value_contains(headers, "cf-mitigated", "challenge")
}

fn should_fast_close_non_sse_error_stream(
    request_path: &str,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
) -> bool {
    if is_compact_request_path(request_path) || !request_path.starts_with("/v1/responses") {
        return false;
    }
    if status.as_u16() < 400 {
        return false;
    }
    if content_type_is(headers, "text/event-stream") || content_type_is(headers, "application/json")
    {
        return false;
    }
    content_type_is(headers, "text/html")
        || header_value_contains(headers, "cf-mitigated", "challenge")
        || (matches!(status.as_u16(), 401 | 403) && has_cloudflare_header_signal(headers))
}

async fn read_stream_error_preview(response: reqwest::Response) -> (Bytes, Option<String>, bool) {
    let mut stream = response.bytes_stream();
    let deadline = tokio::time::Instant::now() + STREAM_ERROR_PREVIEW_TIMEOUT;
    let mut preview = Vec::new();
    let mut read_error = None;
    let mut timed_out = false;

    while preview.len() < STREAM_ERROR_PREVIEW_MAX_BYTES {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            timed_out = true;
            break;
        }
        match tokio::time::timeout(deadline - now, stream.next()).await {
            Ok(Some(Ok(bytes))) => {
                let remaining = STREAM_ERROR_PREVIEW_MAX_BYTES - preview.len();
                let take = bytes.len().min(remaining);
                preview.extend_from_slice(&bytes[..take]);
                if take < bytes.len() {
                    break;
                }
            }
            Ok(Some(Err(err))) => {
                read_error = Some(err.to_string());
                break;
            }
            Ok(None) => break,
            Err(_) => {
                timed_out = true;
                break;
            }
        }
    }

    (Bytes::from(preview), read_error, timed_out)
}

async fn fast_close_non_sse_error_stream(
    request_path: &str,
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    response: reqwest::Response,
    body_tx: mpsc::SyncSender<super::super::GatewayByteStreamItem>,
) {
    let content_type =
        first_header_value(headers, reqwest::header::CONTENT_TYPE.as_str()).unwrap_or("-");
    let cf_ray = first_header_value(headers, "cf-ray").unwrap_or("-");
    let (preview, read_error, timed_out) = read_stream_error_preview(response).await;
    let preview_bytes = preview.len();
    log::warn!(
        "event=gateway_stream_non_sse_error_fast_closed path={} status={} content_type={} cf_ray={} preview_bytes={} timed_out={} read_error={}",
        request_path,
        status.as_u16(),
        content_type,
        cf_ray,
        preview_bytes,
        timed_out,
        read_error.as_deref().unwrap_or("-")
    );
    if !preview.is_empty()
        && body_tx
            .send(super::super::GatewayByteStreamItem::Chunk(preview))
            .is_err()
    {
        return;
    }
    let _ = body_tx.send(super::super::GatewayByteStreamItem::Eof);
}

fn send_async_stream_request(
    client: &reqwest::Client,
    method: &reqwest::Method,
    target_url: &str,
    request_path: &str,
    request_deadline: Option<Instant>,
    request_headers: &[(String, String)],
    request_body: &Bytes,
    is_stream: bool,
) -> Result<super::super::GatewayStreamResponse, reqwest::Error> {
    let client = client.clone();
    let method = method.clone();
    let target_url = target_url.to_string();
    let request_path = request_path.to_string();
    let request_headers = request_headers.to_vec();
    let request_body = request_body.clone();
    let send_timeout = super::super::support::deadline::send_timeout(request_deadline, is_stream);
    let (meta_tx, meta_rx) = mpsc::sync_channel::<
        Result<(reqwest::StatusCode, reqwest::header::HeaderMap), reqwest::Error>,
    >(1);
    let (body_tx, body_rx) = mpsc::sync_channel::<super::super::GatewayByteStreamItem>(128);
    thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|err| panic!("build gateway upstream runtime failed: {err}"));
        runtime.block_on(async move {
            let mut builder = client.request(method, target_url);
            if let Some(timeout) = send_timeout {
                builder = builder.timeout(timeout);
            }
            for (name, value) in request_headers.iter() {
                builder = builder.header(name, value);
            }
            if !request_body.is_empty() {
                builder = builder.body(request_body);
            }
            match builder.send().await {
                Ok(response) => {
                    let status = response.status();
                    let headers = response.headers().clone();
                    let should_fast_close =
                        should_fast_close_non_sse_error_stream(&request_path, status, &headers);
                    if meta_tx.send(Ok((status, headers.clone()))).is_err() {
                        return;
                    }
                    if should_fast_close {
                        fast_close_non_sse_error_stream(
                            &request_path,
                            status,
                            &headers,
                            response,
                            body_tx,
                        )
                        .await;
                        return;
                    }
                    let mut stream = response.bytes_stream();
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(bytes) => {
                                if body_tx
                                    .send(super::super::GatewayByteStreamItem::Chunk(bytes))
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Err(err) => {
                                let _ = body_tx.send(super::super::GatewayByteStreamItem::Error(
                                    err.to_string(),
                                ));
                                return;
                            }
                        }
                    }
                    let _ = body_tx.send(super::super::GatewayByteStreamItem::Eof);
                }
                Err(err) => {
                    let _ = meta_tx.send(Err(err));
                }
            }
        });
    });
    match meta_rx.recv() {
        Ok(Ok((status, headers))) => Ok(super::super::GatewayStreamResponse::new(
            status,
            headers,
            super::super::GatewayByteStream::from_receiver(body_rx),
        )),
        Ok(Err(err)) => Err(err),
        Err(_) => panic!("receive gateway async upstream response metadata failed"),
    }
}

/// 函数 `encode_request_body`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - request_path: 参数 request_path
/// - body: 参数 body
/// - compression: 参数 compression
/// - headers: 参数 headers
///
/// # 返回
/// 返回函数执行结果
fn encode_request_body(
    request_path: &str,
    body: &Bytes,
    compression: RequestCompression,
    headers: &mut Vec<(String, String)>,
) -> Bytes {
    if body.is_empty() || compression == RequestCompression::None {
        return body.clone();
    }
    if has_header(headers, "Content-Encoding") {
        log::warn!(
            "event=gateway_request_compression_skipped reason=content_encoding_exists path={}",
            request_path
        );
        return body.clone();
    }
    match compression {
        RequestCompression::None => body.clone(),
        RequestCompression::Zstd => {
            match zstd::stream::encode_all(std::io::Cursor::new(body.as_ref()), 3) {
                Ok(compressed) => {
                    let post_bytes = compressed.len();
                    headers.push(("Content-Encoding".to_string(), "zstd".to_string()));
                    log::info!(
                    "event=gateway_request_compressed path={} algorithm=zstd pre_bytes={} post_bytes={}",
                    request_path,
                    body.len(),
                    post_bytes
                );
                    Bytes::from(compressed)
                }
                Err(err) => {
                    log::warn!(
                        "event=gateway_request_compression_failed path={} algorithm=zstd err={}",
                        request_path,
                        err
                    );
                    body.clone()
                }
            }
        }
    }
}

/// 函数 `send_upstream_request`
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
pub(in super::super) fn send_upstream_request(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    target_url: &str,
    request_deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
) -> Result<GatewayUpstreamResponse, String> {
    send_upstream_request_with_compression_override(
        client,
        method,
        target_url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        auth_token,
        account,
        strip_session_affinity,
        None,
    )
}

/// 函数 `send_upstream_request_without_compression`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-04
///
/// # 参数
/// - in super: 参数 in super
///
/// # 返回
/// 返回函数执行结果
pub(in super::super) fn send_upstream_request_without_compression(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    target_url: &str,
    request_deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
) -> Result<GatewayUpstreamResponse, String> {
    send_upstream_request_with_compression_override(
        client,
        method,
        target_url,
        request_deadline,
        request_ctx,
        incoming_headers,
        body,
        is_stream,
        auth_token,
        account,
        strip_session_affinity,
        Some(RequestCompression::None),
    )
}

/// 函数 `send_upstream_request_with_compression_override`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-04
///
/// # 参数
/// - compression_override: 参数 compression_override
///
/// # 返回
/// 返回函数执行结果
fn send_upstream_request_with_compression_override(
    client: &reqwest::blocking::Client,
    method: &reqwest::Method,
    target_url: &str,
    request_deadline: Option<Instant>,
    request_ctx: UpstreamRequestContext<'_>,
    incoming_headers: &super::super::super::IncomingHeaderSnapshot,
    body: &Bytes,
    is_stream: bool,
    auth_token: &str,
    account: &Account,
    strip_session_affinity: bool,
    compression_override: Option<RequestCompression>,
) -> Result<GatewayUpstreamResponse, String> {
    let attempt_started_at = Instant::now();
    let is_compact_request = is_compact_request_path(request_ctx.request_path);
    let chatgpt_account_header = resolve_chatgpt_account_header(account, target_url);
    let body_for_transport = if is_compact_request {
        strip_compact_service_tier_for_transport(body, chatgpt_account_header.is_some())
    } else {
        body.clone()
    };
    let prompt_cache_key = extract_prompt_cache_key(body_for_transport.as_ref());
    let request_affinity = super::super::super::session_affinity::derive_outgoing_session_affinity(
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
    let gemini_codex_compat = is_gemini_codex_compat(
        request_ctx.protocol_type,
        request_ctx.request_path,
        target_url,
    );
    super::super::super::session_affinity::log_thread_anchor_conflict(
        request_ctx.request_path,
        account_id,
        incoming_headers.conversation_id(),
        prompt_cache_key.as_deref(),
    );
    super::super::super::session_affinity::log_outgoing_session_affinity(
        request_ctx.request_path,
        account_id,
        incoming_headers.session_id(),
        incoming_headers.client_request_id(),
        incoming_headers.turn_state(),
        incoming_headers.conversation_id(),
        prompt_cache_key.as_deref(),
        request_affinity,
        strip_session_affinity,
    );
    let mut upstream_headers = if is_compact_request {
        let installation_id = if gemini_codex_compat {
            None
        } else {
            super::super::header_profile::resolve_codex_installation_id(
                incoming_headers.codex_installation_id(),
            )
        };
        let header_input = super::super::header_profile::CodexCompactUpstreamHeaderInput {
            auth_token,
            chatgpt_account_id: chatgpt_account_header,
            installation_id: installation_id.as_deref(),
            incoming_user_agent: incoming_headers.user_agent(),
            incoming_originator: incoming_headers.originator(),
            preserve_client_identity: should_preserve_client_identity(request_ctx.protocol_type),
            incoming_session_id: if gemini_codex_compat {
                incoming_headers.session_id()
            } else {
                request_affinity.incoming_session_id
            },
            thread_id: if gemini_codex_compat {
                None
            } else {
                request_affinity.fallback_session_id
            },
            incoming_window_id: if gemini_codex_compat {
                None
            } else {
                incoming_headers.window_id()
            },
            incoming_subagent: if gemini_codex_compat {
                None
            } else {
                incoming_headers.subagent()
            },
            incoming_parent_thread_id: if gemini_codex_compat {
                None
            } else {
                incoming_headers.parent_thread_id()
            },
            incoming_oai_attestation: incoming_headers.oai_attestation(),
            passthrough_codex_headers: incoming_headers.passthrough_codex_headers(),
            fallback_session_id: if gemini_codex_compat {
                None
            } else {
                request_affinity.fallback_session_id
            },
            strip_session_affinity,
            has_body: !body_for_transport.is_empty(),
        };
        super::super::header_profile::build_codex_compact_upstream_headers(header_input)
    } else {
        let header_input = super::super::header_profile::CodexUpstreamHeaderInput {
            auth_token,
            chatgpt_account_id: chatgpt_account_header,
            incoming_user_agent: incoming_headers.user_agent(),
            incoming_originator: incoming_headers.originator(),
            preserve_client_identity: should_preserve_client_identity(request_ctx.protocol_type),
            incoming_session_id: if gemini_codex_compat {
                incoming_headers.session_id()
            } else {
                request_affinity.incoming_session_id
            },
            incoming_window_id: if gemini_codex_compat {
                None
            } else {
                incoming_headers.window_id()
            },
            incoming_client_request_id: if gemini_codex_compat {
                incoming_headers.client_request_id()
            } else {
                request_affinity.incoming_client_request_id
            },
            incoming_subagent: if gemini_codex_compat {
                None
            } else {
                incoming_headers.subagent()
            },
            incoming_beta_features: incoming_headers.beta_features(),
            incoming_turn_metadata: incoming_headers.turn_metadata(),
            incoming_parent_thread_id: if gemini_codex_compat {
                None
            } else {
                incoming_headers.parent_thread_id()
            },
            incoming_responsesapi_include_timing_metrics: incoming_headers
                .responsesapi_include_timing_metrics(),
            incoming_inference_call_id: incoming_headers.codex_inference_call_id(),
            incoming_oai_attestation: incoming_headers.oai_attestation(),
            passthrough_codex_headers: incoming_headers.passthrough_codex_headers(),
            fallback_session_id: if gemini_codex_compat {
                None
            } else {
                request_affinity.fallback_session_id
            },
            incoming_turn_state: if gemini_codex_compat {
                None
            } else {
                request_affinity.incoming_turn_state
            },
            include_turn_state: !gemini_codex_compat,
            strip_session_affinity,
            has_body: !body_for_transport.is_empty(),
        };
        super::super::header_profile::build_codex_upstream_headers(header_input)
    };
    if gemini_codex_compat {
        apply_gemini_codex_compat_header_profile(
            &mut upstream_headers,
            incoming_headers.originator(),
        );
    }
    if should_force_connection_close(target_url) {
        // 中文注释：本地 loopback mock/代理更容易复用到脏 keep-alive 连接；
        // 对 localhost/127.0.0.1 强制 close，避免请求落到已失效连接。
        force_connection_close(&mut upstream_headers);
    }
    let upstream_headers_uncompressed = upstream_headers.clone();
    let request_compression = compression_override.unwrap_or_else(|| {
        resolve_request_compression(
            request_ctx.protocol_type,
            target_url,
            request_ctx.request_path,
            is_stream,
        )
    });
    let body_for_request = encode_request_body(
        request_ctx.request_path,
        &body_for_transport,
        request_compression,
        &mut upstream_headers,
    );
    let build_request = |http: &reqwest::blocking::Client,
                         request_headers: &[(String, String)],
                         request_body: &Bytes| {
        let mut builder = http.request(method.clone(), target_url);
        if let Some(timeout) =
            super::super::support::deadline::send_timeout(request_deadline, is_stream)
        {
            builder = builder.timeout(timeout);
        }
        for (name, value) in request_headers.iter() {
            builder = builder.header(name, value);
        }
        if !request_body.is_empty() {
            builder = builder.body(request_body.clone());
        }
        builder
    };

    let use_async_stream_transport =
        should_wrap_upstream_as_stream_response(request_ctx.request_path, is_stream);
    let use_websocket_upstream =
        use_async_stream_transport && should_use_websocket_upstream(target_url);

    // Try WebSocket path first (when enabled). On handshake failure return None so
    // the caller falls through to the full HTTP async-stream retry logic below.
    let ws_early_result: Option<GatewayUpstreamResponse> = if use_websocket_upstream {
        // Always pass the *uncompressed* body and headers to the WebSocket path.
        // chatgpt.com's WebSocket endpoint expects a JSON (UTF-8) body; a
        // zstd/gzip-compressed body would fail UTF-8 validation and fall back.
        match send_websocket_upstream_request(
            target_url,
            account.id.as_str(),
            request_deadline,
            upstream_headers_uncompressed.as_slice(),
            &body_for_transport,
        ) {
            Ok(resp) => Some(GatewayUpstreamResponse::Stream(resp)),
            Err(ws_err) => {
                // Redact query/fragment from the URL to avoid leaking sensitive
                // parameters in warn-level logs.
                let redacted_url = reqwest::Url::parse(target_url)
                    .map(|mut u| {
                        u.set_query(None);
                        u.set_fragment(None);
                        u.to_string()
                    })
                    .unwrap_or_else(|_| "<unparseable url>".to_string());
                log::warn!(
                        "event=gateway_websocket_upstream_fallback_to_http path={} account_id={} target_url={} err={}",
                        request_ctx.request_path,
                        account.id,
                        redacted_url,
                        ws_err
                    );
                None // fall through to the full HTTP async-stream retry below
            }
        }
    } else {
        None
    };

    let result = if let Some(r) = ws_early_result {
        Ok(r)
    } else if use_async_stream_transport {
        let async_client =
            match super::super::super::async_upstream_client_for_account(account.id.as_str()) {
                Ok(client) => client,
                Err(err) => return Err(err),
            };
        match send_async_stream_request(
            &async_client,
            method,
            target_url,
            request_ctx.request_path,
            request_deadline,
            upstream_headers.as_slice(),
            &body_for_request,
            is_stream,
        ) {
            Ok(resp) => Ok(GatewayUpstreamResponse::Stream(resp)),
            Err(first_err) => {
                let fresh_async = match super::super::super::fresh_async_upstream_client_for_account(
                    account.id.as_str(),
                ) {
                    Ok(client) => client,
                    Err(err) => return Err(err),
                };
                if should_retry_transport_without_compression(
                    target_url,
                    request_ctx.request_path,
                    is_stream,
                    request_compression,
                ) {
                    log::warn!(
                        "event=gateway_transport_retry_without_compression path={} account_id={} target_url={} first_err={}",
                        request_ctx.request_path,
                        account.id,
                        target_url,
                        first_err
                    );
                    match send_async_stream_request(
                        &fresh_async,
                        method,
                        target_url,
                        request_ctx.request_path,
                        request_deadline,
                        upstream_headers_uncompressed.as_slice(),
                        &body_for_transport,
                        is_stream,
                    ) {
                        Ok(resp) => {
                            log::warn!(
                                "event=gateway_transport_retry_without_compression_succeeded path={} account_id={} target_url={}",
                                request_ctx.request_path,
                                account.id,
                                target_url
                            );
                            Ok(GatewayUpstreamResponse::Stream(resp))
                        }
                        Err(second_err) => {
                            log::warn!(
                                "event=gateway_transport_retry_without_compression_failed path={} account_id={} target_url={} first_err={} retry_err={}",
                                request_ctx.request_path,
                                account.id,
                                target_url,
                                first_err,
                                second_err
                            );
                            Err(second_err.to_string())
                        }
                    }
                } else {
                    match send_async_stream_request(
                        &fresh_async,
                        method,
                        target_url,
                        request_ctx.request_path,
                        request_deadline,
                        upstream_headers.as_slice(),
                        &body_for_request,
                        is_stream,
                    ) {
                        Ok(resp) => {
                            log::info!(
                                "event=gateway_transport_retry_with_fresh_client_succeeded path={} account_id={} target_url={}",
                                request_ctx.request_path,
                                account.id,
                                target_url
                            );
                            Ok(GatewayUpstreamResponse::Stream(resp))
                        }
                        Err(second_err) => {
                            log::warn!(
                                "event=gateway_transport_retry_with_fresh_client_failed path={} account_id={} target_url={} first_err={} retry_err={}",
                                request_ctx.request_path,
                                account.id,
                                target_url,
                                first_err,
                                second_err
                            );
                            Err(second_err.to_string())
                        }
                    }
                }
            }
        }
    } else {
        match build_request(client, upstream_headers.as_slice(), &body_for_request).send() {
            Ok(resp) => Ok(resp.into()),
            Err(first_err) => {
                let fresh = match super::super::super::fresh_upstream_client_for_account(
                    account.id.as_str(),
                ) {
                    Ok(client) => client,
                    Err(err) => return Err(err),
                };
                if should_retry_transport_without_compression(
                    target_url,
                    request_ctx.request_path,
                    is_stream,
                    request_compression,
                ) {
                    log::warn!(
                        "event=gateway_transport_retry_without_compression path={} account_id={} target_url={} first_err={}",
                        request_ctx.request_path,
                        account.id,
                        target_url,
                        first_err
                    );
                    match build_request(&fresh, upstream_headers_uncompressed.as_slice(), body)
                        .send()
                    {
                        Ok(resp) => {
                            log::warn!(
                                "event=gateway_transport_retry_without_compression_succeeded path={} account_id={} target_url={}",
                                request_ctx.request_path,
                                account.id,
                                target_url
                            );
                            Ok(resp.into())
                        }
                        Err(second_err) => {
                            log::warn!(
                                "event=gateway_transport_retry_without_compression_failed path={} account_id={} target_url={} first_err={} retry_err={}",
                                request_ctx.request_path,
                                account.id,
                                target_url,
                                first_err,
                                second_err
                            );
                            Err(second_err.to_string())
                        }
                    }
                } else {
                    match build_request(&fresh, upstream_headers.as_slice(), &body_for_request)
                        .send()
                    {
                        Ok(resp) => {
                            log::info!(
                                "event=gateway_transport_retry_with_fresh_client_succeeded path={} account_id={} target_url={}",
                                request_ctx.request_path,
                                account.id,
                                target_url
                            );
                            Ok(resp.into())
                        }
                        Err(second_err) => {
                            log::warn!(
                                "event=gateway_transport_retry_with_fresh_client_failed path={} account_id={} target_url={} first_err={} retry_err={}",
                                request_ctx.request_path,
                                account.id,
                                target_url,
                                first_err,
                                second_err
                            );
                            Err(second_err.to_string())
                        }
                    }
                }
            }
        }
    };
    let duration_ms = super::super::super::duration_to_millis(attempt_started_at.elapsed());
    super::super::super::metrics::record_gateway_upstream_attempt(duration_ms, result.is_err());
    result
}

fn is_chatgpt_target_url(target_url: &str) -> bool {
    // Parse the URL and validate the host to avoid substring-match vulnerabilities
    // (e.g. "evilchatgpt.com" would incorrectly match a plain contains() check).
    match reqwest::Url::parse(target_url) {
        Ok(url) => {
            let host = url.host_str().unwrap_or("");
            host == "chatgpt.com" || host.ends_with(".chatgpt.com")
        }
        Err(_) => false,
    }
}

fn should_use_websocket_upstream(target_url: &str) -> bool {
    if !super::super::super::runtime_config::use_websocket_upstream() {
        return false;
    }
    is_chatgpt_target_url(target_url)
}

fn websocket_handshake_timeout(request_deadline: Option<Instant>) -> Duration {
    let connect_timeout = super::super::super::runtime_config::current_upstream_connect_timeout()
        .max(Duration::from_secs(1));
    match request_deadline {
        Some(deadline) => deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_secs(1))
            .min(connect_timeout),
        None => connect_timeout,
    }
}

fn build_websocket_upstream_request(
    ws_url: &str,
    request_headers: &[(String, String)],
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    let mut request = ws_url
        .into_client_request()
        .map_err(|err| format!("failed to build WS request: {err}"))?;
    let headers = request.headers_mut();
    for (name, value) in request_headers.iter() {
        let name_lower = name.to_ascii_lowercase();
        if should_skip_websocket_upstream_header(name_lower.as_str()) {
            continue;
        }
        insert_websocket_upstream_header(headers, name.as_str(), value.as_str())?;
    }
    insert_websocket_upstream_header(headers, "OpenAI-Beta", "responses_websockets=2026-02-06")?;
    Ok(request)
}

fn should_skip_websocket_upstream_header(name_lower: &str) -> bool {
    matches!(
        name_lower,
        "connection"
            | "content-encoding"
            | "content-length"
            | "content-type"
            | "host"
            | "sec-websocket-extensions"
            | "sec-websocket-key"
            | "sec-websocket-protocol"
            | "sec-websocket-version"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn insert_websocket_upstream_header(
    headers: &mut WsHeaderMap,
    name: &str,
    value: &str,
) -> Result<(), String> {
    let header_name = WsHeaderName::from_bytes(name.as_bytes())
        .map_err(|err| format!("invalid WS request header name {name}: {err}"))?;
    let header_value = WsHeaderValue::from_str(value)
        .map_err(|err| format!("invalid WS request header value for {name}: {err}"))?;
    headers.insert(header_name, header_value);
    Ok(())
}

fn is_websocket_upstream_terminal_text(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    let Some(event_type) = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
    else {
        return false;
    };
    matches!(
        event_type.to_ascii_lowercase().as_str(),
        "response.completed"
            | "response.done"
            | "response.failed"
            | "response.incomplete"
            | "error"
    )
}

fn websocket_upstream_request_text_from_http_body(
    request_body: &Bytes,
) -> Result<Option<String>, String> {
    if request_body.is_empty() {
        return Ok(None);
    }

    let request: crate::http::codex_source::ResponseCreateWsRequest =
        serde_json::from_slice(request_body.as_ref()).map_err(|err| {
            format!("request body is not a valid WebSocket response.create payload: {err}")
        })?;
    serde_json::to_string(&crate::http::codex_source::ResponsesWsRequest::ResponseCreate(request))
        .map(Some)
        .map_err(|err| format!("serialize WebSocket response.create payload failed: {err}"))
}

fn send_websocket_upstream_request(
    target_url: &str,
    account_id: &str,
    request_deadline: Option<Instant>,
    request_headers: &[(String, String)],
    request_body: &Bytes,
) -> Result<super::super::GatewayStreamResponse, String> {
    let body_text = websocket_upstream_request_text_from_http_body(request_body)?;

    let ws_url = if target_url.starts_with("https://") {
        format!("wss://{}", &target_url["https://".len()..])
    } else if target_url.starts_with("http://") {
        format!("ws://{}", &target_url["http://".len()..])
    } else {
        target_url.to_string()
    };
    let request_headers = request_headers.to_vec();
    let proxy_url = super::super::super::current_upstream_proxy_url_for_account(account_id);
    let handshake_timeout = websocket_handshake_timeout(request_deadline);

    let (meta_tx, meta_rx) = mpsc::sync_channel::<Result<(), String>>(1);
    let (body_tx, body_rx) = mpsc::sync_channel::<super::super::GatewayByteStreamItem>(128);

    thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|err| panic!("build websocket upstream runtime failed: {err}"));
        runtime.block_on(async move {
            use futures_util::SinkExt;
            use tokio_tungstenite::tungstenite::Message;

            let req = match build_websocket_upstream_request(ws_url.as_str(), &request_headers) {
                Ok(request) => request,
                Err(err) => {
                    let _ = meta_tx.send(Err(err));
                    return;
                }
            };

            let connect_result = tokio::time::timeout(
                handshake_timeout,
                crate::http::responses_websocket::connect_upstream_websocket_request(
                    req,
                    ws_url.as_str(),
                    proxy_url.as_deref(),
                ),
            )
            .await;
            match connect_result {
                Err(_) => {
                    let _ = meta_tx.send(Err("WebSocket connect timed out".to_string()));
                }
                Ok(Err(e)) => {
                    let _ = meta_tx.send(Err(format!("WebSocket connect failed: {e}")));
                }
                Ok(Ok((mut ws_stream, _))) => {
                    if meta_tx.send(Ok(())).is_err() {
                        return;
                    }
                    // body_text was pre-validated as UTF-8 before this thread was spawned.
                    if let Some(text) = body_text {
                        if let Err(e) = ws_stream.send(Message::Text(text.into())).await {
                            let _ = body_tx.send(super::super::GatewayByteStreamItem::Error(
                                format!("WebSocket send error: {e}"),
                            ));
                            return;
                        }
                    }
                    loop {
                        // Apply the remaining deadline to each WebSocket read so the
                        // spawned thread cannot outlive the request deadline and leak.
                        let next_msg = match request_deadline {
                            Some(d) => {
                                let remaining = d
                                    .saturating_duration_since(Instant::now())
                                    .max(Duration::from_millis(100));
                                match tokio::time::timeout(remaining, ws_stream.next()).await {
                                    Ok(m) => m,
                                    Err(_) => {
                                        let _ = body_tx.send(
                                            super::super::GatewayByteStreamItem::Error(
                                                "WebSocket read deadline exceeded".to_string(),
                                            ),
                                        );
                                        return;
                                    }
                                }
                            }
                            None => ws_stream.next().await,
                        };
                        match next_msg {
                            None => {
                                let _ = body_tx.send(super::super::GatewayByteStreamItem::Eof);
                                return;
                            }
                            Some(Err(e)) => {
                                let _ = body_tx.send(super::super::GatewayByteStreamItem::Error(
                                    format!("WebSocket receive error: {e}"),
                                ));
                                return;
                            }
                            Some(Ok(Message::Text(text))) => {
                                let sse = format!("data: {text}\n\n");
                                if body_tx
                                    .send(super::super::GatewayByteStreamItem::Chunk(Bytes::from(
                                        sse.into_bytes(),
                                    )))
                                    .is_err()
                                {
                                    return;
                                }
                                if is_websocket_upstream_terminal_text(text.as_ref()) {
                                    let _ = body_tx.send(super::super::GatewayByteStreamItem::Eof);
                                    return;
                                }
                            }
                            Some(Ok(Message::Ping(payload))) => {
                                let _ = ws_stream.send(Message::Pong(payload)).await;
                            }
                            Some(Ok(Message::Close(_))) => {
                                let _ = body_tx.send(super::super::GatewayByteStreamItem::Eof);
                                return;
                            }
                            Some(Ok(_)) => {}
                        }
                    }
                }
            }
        });
    });

    // recv_timeout gives the thread a small grace window beyond handshake_timeout to
    // deliver its meta result before we declare the operation hung.
    match meta_rx.recv_timeout(handshake_timeout + Duration::from_secs(5)) {
        Ok(Ok(())) => {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::CONTENT_TYPE,
                reqwest::header::HeaderValue::from_static("text/event-stream"),
            );
            Ok(super::super::GatewayStreamResponse::new(
                reqwest::StatusCode::OK,
                headers,
                super::super::GatewayByteStream::from_receiver(body_rx),
            ))
        }
        Ok(Err(err)) => Err(err),
        Err(_) => Err("WebSocket upstream handshake timed out or thread terminated".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_gemini_codex_compat_header_profile, encode_request_body, resolve_request_compression,
        resolve_request_compression_with_flag, send_async_stream_request,
        should_retry_transport_without_compression, should_wrap_upstream_as_stream_response,
        strip_compact_service_tier_for_transport, RequestCompression, CPA_GEMINI_CODEX_USER_AGENT,
    };
    use bytes::Bytes;
    use futures_util::{SinkExt, StreamExt};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc::{self, Receiver};
    use std::thread;
    use std::time::{Duration, Instant};
    use tokio::runtime::Builder;

    type WsServerRequest = tokio_tungstenite::tungstenite::handshake::server::Request;
    type WsServerResponse = tokio_tungstenite::tungstenite::handshake::server::Response;

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    struct RuntimeConfigReloadGuard;

    impl Drop for RuntimeConfigReloadGuard {
        fn drop(&mut self) {
            let _ = std::panic::catch_unwind(crate::gateway::reload_runtime_config_from_env);
        }
    }

    fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn spawn_raw_http_response(
        status: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: Vec<u8>,
        hold_open: bool,
    ) -> (String, mpsc::Sender<()>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock upstream addr");
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mock upstream");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(read) => request.extend_from_slice(&buf[..read]),
                    Err(_) => break,
                }
            }

            let mut response = format!("HTTP/1.1 {status}\r\n");
            for (name, value) in headers {
                response.push_str(name);
                response.push_str(": ");
                response.push_str(value);
                response.push_str("\r\n");
            }
            if hold_open {
                response.push_str("Connection: keep-alive\r\n\r\n");
            } else {
                response.push_str(&format!(
                    "Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                ));
            }
            stream
                .write_all(response.as_bytes())
                .expect("write mock response headers");
            stream.write_all(&body).expect("write mock response body");
            stream.flush().expect("flush mock response");
            if hold_open {
                let _ = release_rx.recv_timeout(Duration::from_secs(5));
            }
        });
        (format!("http://{addr}/v1/responses"), release_tx, handle)
    }

    fn send_mock_stream_request(url: &str) -> super::super::super::GatewayStreamResponse {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .build()
            .expect("build reqwest client");
        send_async_stream_request(
            &client,
            &reqwest::Method::GET,
            url,
            "/v1/responses",
            Some(Instant::now() + Duration::from_secs(5)),
            &[],
            &Bytes::new(),
            true,
        )
        .expect("send stream request")
    }

    fn spawn_mock_websocket_upstream(
        response_text: &'static str,
    ) -> (
        String,
        Receiver<Vec<(String, String)>>,
        Receiver<String>,
        thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock websocket upstream");
        listener
            .set_nonblocking(true)
            .expect("set mock websocket listener nonblocking");
        let addr = listener.local_addr().expect("mock websocket upstream addr");
        let (headers_tx, headers_rx) = mpsc::channel();
        let (frame_tx, frame_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build mock websocket runtime");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener)
                    .expect("convert websocket listener");
                let (stream, _) = listener.accept().await.expect("accept websocket client");
                let mut websocket = tokio_tungstenite::accept_hdr_async(
                    stream,
                    |request: &WsServerRequest, response: WsServerResponse| {
                        let headers = request
                            .headers()
                            .iter()
                            .map(|(name, value)| {
                                (
                                    name.as_str().to_ascii_lowercase(),
                                    value.to_str().unwrap_or_default().to_string(),
                                )
                            })
                            .collect::<Vec<_>>();
                        let _ = headers_tx.send(headers);
                        Ok(response)
                    },
                )
                .await
                .expect("accept websocket handshake");

                if let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) =
                    websocket.next().await
                {
                    let _ = frame_tx.send(text.to_string());
                }
                let _ = websocket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        response_text.into(),
                    ))
                    .await;
            });
        });
        (
            format!("ws://{addr}/v1/responses"),
            headers_rx,
            frame_rx,
            handle,
        )
    }

    fn spawn_http_connect_proxy(
        target_addr: String,
    ) -> (String, Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock websocket proxy");
        let proxy_addr = listener.local_addr().expect("mock websocket proxy addr");
        let (connect_tx, connect_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut client, _) = listener.accept().expect("accept proxy client");
            client
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("set proxy client read timeout");
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = client.read(&mut buf).expect("read proxy CONNECT request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
            }
            let request_text = String::from_utf8_lossy(request.as_slice());
            let first_line = request_text.lines().next().unwrap_or_default().to_string();
            let _ = connect_tx.send(first_line);

            let mut upstream = TcpStream::connect(target_addr.as_str())
                .expect("connect proxy to mock websocket upstream");
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .expect("write proxy CONNECT response");

            let mut client_reader = client.try_clone().expect("clone proxy client reader");
            let mut upstream_writer = upstream.try_clone().expect("clone proxy upstream writer");
            let upstream_to_client = thread::spawn(move || {
                let _ = std::io::copy(&mut upstream, &mut client);
            });
            let client_to_upstream = thread::spawn(move || {
                let _ = std::io::copy(&mut client_reader, &mut upstream_writer);
            });
            let _ = client_to_upstream.join();
            let _ = upstream_to_client.join();
        });
        (format!("http://{proxy_addr}"), connect_rx, handle)
    }

    fn captured_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(header_name, _)| header_name == name)
            .map(|(_, value)| value.as_str())
    }

    /// 函数 `request_compression_only_applies_to_streaming_chatgpt_responses`
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
    fn request_compression_only_applies_to_streaming_chatgpt_responses() {
        assert_eq!(
            resolve_request_compression_with_flag(
                true,
                "https://chatgpt.com/backend-api/codex/responses",
                "/v1/responses",
                true
            ),
            RequestCompression::Zstd
        );
        assert_eq!(
            resolve_request_compression_with_flag(
                true,
                "https://chatgpt.com/backend-api/codex/responses",
                "/v1/responses/compact",
                true
            ),
            RequestCompression::None
        );
        assert_eq!(
            resolve_request_compression_with_flag(
                true,
                "https://api.openai.com/v1/responses",
                "/v1/responses",
                true
            ),
            RequestCompression::None
        );
        assert_eq!(
            resolve_request_compression_with_flag(
                true,
                "https://chatgpt.com/backend-api/codex/responses",
                "/v1/responses",
                false
            ),
            RequestCompression::None
        );
        assert_eq!(
            resolve_request_compression_with_flag(
                false,
                "https://chatgpt.com/backend-api/codex/responses",
                "/v1/responses",
                true
            ),
            RequestCompression::None
        );
    }

    #[test]
    fn gemini_codex_compat_disables_request_compression_like_cpa() {
        assert_eq!(
            resolve_request_compression(
                crate::apikey_profile::PROTOCOL_GEMINI_NATIVE,
                "https://chatgpt.com/backend-api/codex/responses",
                "/v1/responses",
                true,
            ),
            RequestCompression::None
        );
    }

    #[test]
    fn gemini_codex_compat_does_not_preserve_client_identity_like_cpa() {
        assert!(!super::should_preserve_client_identity(
            crate::apikey_profile::PROTOCOL_GEMINI_NATIVE
        ));
    }

    #[test]
    fn gemini_codex_compat_header_profile_matches_cpa_executor_shape() {
        let mut headers = vec![
            (
                "User-Agent".to_string(),
                "gemini-cli/0.1.14 (Windows 11; x86_64)".to_string(),
            ),
            ("originator".to_string(), "gemini_cli".to_string()),
            ("x-codex-window-id".to_string(), "thread:0".to_string()),
            ("x-codex-turn-state".to_string(), "turn-state".to_string()),
            (
                "x-codex-parent-thread-id".to_string(),
                "parent-thread".to_string(),
            ),
            ("x-openai-subagent".to_string(), "subagent".to_string()),
        ];

        apply_gemini_codex_compat_header_profile(&mut headers, None);

        assert_eq!(
            header_value(&headers, "User-Agent"),
            Some(CPA_GEMINI_CODEX_USER_AGENT)
        );
        assert_eq!(header_value(&headers, "originator"), Some("codex-tui"));
        assert_eq!(header_value(&headers, "Connection"), Some("Keep-Alive"));
        assert_eq!(header_value(&headers, "x-codex-window-id"), None);
        assert_eq!(header_value(&headers, "x-codex-turn-state"), None);
        assert_eq!(header_value(&headers, "x-codex-parent-thread-id"), None);
        assert_eq!(header_value(&headers, "x-openai-subagent"), None);
        assert_eq!(header_value(&headers, "session_id").map(str::len), Some(36));
    }

    /// 函数 `encode_request_body_adds_zstd_content_encoding`
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
    fn encode_request_body_adds_zstd_content_encoding() {
        let body = Bytes::from_static(br#"{"model":"gpt-5.4","input":"compress me"}"#);
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];

        let actual = encode_request_body(
            "/v1/responses",
            &body,
            RequestCompression::Zstd,
            &mut headers,
        );

        assert!(headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("Content-Encoding") && value == "zstd"
        }));
        let decoded = zstd::stream::decode_all(std::io::Cursor::new(actual.as_ref()))
            .expect("decode zstd body");
        let value: serde_json::Value =
            serde_json::from_slice(&decoded).expect("parse decompressed json");
        assert_eq!(
            value.get("model").and_then(serde_json::Value::as_str),
            Some("gpt-5.4")
        );
    }

    #[test]
    fn compact_transport_strips_service_tier_without_chatgpt_account_header() {
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.4","input":[],"service_tier":"priority","prompt_cache_key":"thread-1"}"#,
        );

        let actual = strip_compact_service_tier_for_transport(&body, false);
        let value: serde_json::Value =
            serde_json::from_slice(&actual).expect("parse stripped compact body");

        assert!(value.get("service_tier").is_none());
        assert_eq!(
            value
                .get("prompt_cache_key")
                .and_then(serde_json::Value::as_str),
            Some("thread-1")
        );
    }

    #[test]
    fn compact_transport_preserves_service_tier_with_chatgpt_account_header() {
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.4","input":[],"service_tier":"priority","prompt_cache_key":"thread-1"}"#,
        );

        let actual = strip_compact_service_tier_for_transport(&body, true);
        let value: serde_json::Value =
            serde_json::from_slice(&actual).expect("parse preserved compact body");

        assert_eq!(
            value
                .get("service_tier")
                .and_then(serde_json::Value::as_str),
            Some("priority")
        );
    }

    #[test]
    fn transport_retry_without_compression_only_targets_streaming_chatgpt_responses() {
        assert!(should_retry_transport_without_compression(
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            true,
            RequestCompression::Zstd
        ));
        assert!(!should_retry_transport_without_compression(
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses/compact",
            true,
            RequestCompression::Zstd
        ));
        assert!(!should_retry_transport_without_compression(
            "https://api.openai.com/v1/responses",
            "/v1/responses",
            true,
            RequestCompression::Zstd
        ));
        assert!(!should_retry_transport_without_compression(
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            false,
            RequestCompression::Zstd
        ));
        assert!(!should_retry_transport_without_compression(
            "https://chatgpt.com/backend-api/codex/responses",
            "/v1/responses",
            true,
            RequestCompression::None
        ));
    }

    #[test]
    fn anthropic_codex_compat_disables_request_compression_without_touching_codex() {
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _compression_guard = EnvGuard::set("CODEXMANAGER_ENABLE_REQUEST_COMPRESSION", "1");
        crate::gateway::reload_runtime_config_from_env();

        assert_eq!(
            resolve_request_compression(
                crate::apikey_profile::PROTOCOL_ANTHROPIC_NATIVE,
                "https://chatgpt.com/backend-api/codex/responses",
                "/v1/responses",
                true,
            ),
            RequestCompression::None
        );
        assert_eq!(
            resolve_request_compression(
                crate::apikey_profile::PROTOCOL_OPENAI_COMPAT,
                "https://chatgpt.com/backend-api/codex/responses",
                "/v1/responses",
                true,
            ),
            RequestCompression::Zstd
        );
    }

    #[test]
    fn transport_wraps_non_compact_responses_streams_into_stream_variant() {
        assert!(should_wrap_upstream_as_stream_response(
            "/v1/responses",
            true
        ));
        assert!(should_wrap_upstream_as_stream_response(
            "/v1/responses?stream=false",
            true
        ));
        assert!(!should_wrap_upstream_as_stream_response(
            "/v1/responses/compact",
            true
        ));
        assert!(!should_wrap_upstream_as_stream_response(
            "/v1/chat/completions",
            true
        ));
        assert!(!should_wrap_upstream_as_stream_response(
            "/v1/responses",
            false
        ));
    }

    #[test]
    fn stream_transport_fast_closes_html_challenge_without_waiting_for_eof() {
        let body = b"<html><title>Just a moment...</title><body>Cloudflare</body></html>".to_vec();
        let (url, release, handle) = spawn_raw_http_response(
            "403 Forbidden",
            vec![
                ("Content-Type", "text/html; charset=utf-8"),
                ("cf-ray", "ray-fast-close"),
            ],
            body,
            true,
        );

        let started = Instant::now();
        let response = send_mock_stream_request(url.as_str());
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        let body = response.read_all_bytes().expect("read fast-closed body");

        assert!(
            started.elapsed() < Duration::from_secs(1),
            "fast-close path waited {:?}",
            started.elapsed()
        );
        assert!(String::from_utf8_lossy(body.as_ref()).contains("Just a moment"));
        let _ = release.send(());
        handle.join().expect("join mock upstream");
    }

    #[test]
    fn stream_transport_does_not_fast_close_json_error_body() {
        let body = vec![b'a'; 70 * 1024];
        let expected_len = body.len();
        let (url, release, handle) = spawn_raw_http_response(
            "403 Forbidden",
            vec![("Content-Type", "application/json")],
            body,
            false,
        );

        let response = send_mock_stream_request(url.as_str());
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        let body = response.read_all_bytes().expect("read json error body");

        assert_eq!(body.len(), expected_len);
        let _ = release.send(());
        handle.join().expect("join mock upstream");
    }

    #[test]
    fn stream_transport_does_not_fast_close_successful_sse_body() {
        let expected = b"data: {\"type\":\"response.completed\"}\n\n".to_vec();
        let (url, release, handle) = spawn_raw_http_response(
            "200 OK",
            vec![("Content-Type", "text/event-stream")],
            expected.clone(),
            false,
        );

        let response = send_mock_stream_request(url.as_str());
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let body = response.read_all_bytes().expect("read sse body");

        assert_eq!(body.as_ref(), expected.as_slice());
        let _ = release.send(());
        handle.join().expect("join mock upstream");
    }

    // ── WebSocket upstream selection & fallback ────────────────────────────────

    #[test]
    fn websocket_upstream_not_selected_when_flag_disabled() {
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _ws_guard = EnvGuard::set("CODEXMANAGER_USE_WEBSOCKET_UPSTREAM", "0");
        crate::gateway::reload_runtime_config_from_env();
        // Runtime config flag is false by default (DEFAULT_USE_WEBSOCKET_UPSTREAM = false),
        // so should_use_websocket_upstream must return false regardless of the URL.
        assert!(!super::should_use_websocket_upstream(
            "https://chatgpt.com/backend-api/codex/responses"
        ));
        assert!(!super::should_use_websocket_upstream(
            "https://api.chatgpt.com/v1/responses"
        ));
        assert!(!super::should_use_websocket_upstream(
            "https://example.com/api"
        ));
    }

    #[test]
    fn chatgpt_target_url_accepts_valid_hosts_and_rejects_lookalikes() {
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _ws_guard = EnvGuard::set("CODEXMANAGER_USE_WEBSOCKET_UPSTREAM", "0");
        crate::gateway::reload_runtime_config_from_env();
        // Positive: exact match and subdomain
        assert!(super::is_chatgpt_target_url(
            "https://chatgpt.com/backend-api/codex/responses"
        ));
        assert!(super::is_chatgpt_target_url(
            "https://api.chatgpt.com/v1/responses"
        ));
        assert!(super::is_chatgpt_target_url(
            "https://backend.chatgpt.com/path"
        ));

        // Negative: substring lookalikes must NOT match
        assert!(!super::is_chatgpt_target_url(
            "https://evilchatgpt.com/path"
        ));
        assert!(!super::is_chatgpt_target_url(
            "https://chatgpt.com.evil.com/path"
        ));
        assert!(!super::is_chatgpt_target_url("https://notchatgpt.com/path"));
        assert!(!super::is_chatgpt_target_url("https://example.com/api"));

        // should_use_websocket_upstream with flag disabled always returns false —
        // this confirms the guard is evaluated FIRST, before the host check.
        assert!(!super::should_use_websocket_upstream(
            "https://chatgpt.com/backend-api/codex/responses"
        ));
    }

    #[test]
    fn websocket_upstream_terminal_detection_parses_json_type() {
        assert!(super::is_websocket_upstream_terminal_text(
            r#"{"type":"response.completed"}"#
        ));
        assert!(super::is_websocket_upstream_terminal_text(
            r#"{"type":"response.done"}"#
        ));
        assert!(super::is_websocket_upstream_terminal_text(
            r#"{"type":"response.incomplete"}"#
        ));
        assert!(super::is_websocket_upstream_terminal_text(
            r#"{"type":"error","error":{"message":"boom"}}"#
        ));
        assert!(!super::is_websocket_upstream_terminal_text(
            r#"{"type":"response.output_text.delta","delta":"response.done"}"#
        ));
        assert!(!super::is_websocket_upstream_terminal_text(
            r#"not json response.completed"#
        ));
    }

    #[test]
    fn websocket_upstream_request_text_from_http_body_wraps_response_create() {
        let body = Bytes::from_static(
            br#"{"model":"codex","input":"hello","stream":true,"reasoning":{"effort":"high"}}"#,
        );

        let text = super::websocket_upstream_request_text_from_http_body(&body)
            .expect("wrap valid HTTP body")
            .expect("non-empty body produces websocket frame");
        let value: serde_json::Value =
            serde_json::from_str(text.as_str()).expect("parse wrapped websocket frame");

        assert_eq!(
            value.get("type").and_then(serde_json::Value::as_str),
            Some("response.create")
        );
        assert_eq!(
            value.get("model").and_then(serde_json::Value::as_str),
            Some("codex")
        );
        assert_eq!(
            value.get("input").and_then(serde_json::Value::as_str),
            Some("hello")
        );
        assert_eq!(
            value.get("stream").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            value
                .get("reasoning")
                .and_then(|reasoning| reasoning.get("effort"))
                .and_then(serde_json::Value::as_str),
            Some("high")
        );
    }

    #[test]
    fn websocket_upstream_request_text_from_http_body_rejects_invalid_payload() {
        let body = Bytes::from_static(br#"{"model":"codex"}"#);

        let err = super::websocket_upstream_request_text_from_http_body(&body)
            .expect_err("missing input is not a valid response.create payload");

        assert!(
            err.contains("valid WebSocket response.create payload"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn send_websocket_upstream_request_builds_valid_handshake_and_stops_on_done() {
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
        let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
        crate::gateway::reload_runtime_config_from_env();
        let (url, headers_rx, frame_rx, handle) =
            spawn_mock_websocket_upstream(r#"{"type":"response.done"}"#);
        let body = Bytes::from(r#"{"model":"codex","input":"hello"}"#);
        let response = super::send_websocket_upstream_request(
            url.as_str(),
            "acct_ws_direct",
            Some(Instant::now() + Duration::from_secs(5)),
            &[
                ("Authorization".to_string(), "Bearer token_ws".to_string()),
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Connection".to_string(), "close".to_string()),
            ],
            &body,
        )
        .expect("websocket upstream request connects");
        let response_body = response.read_all_bytes().expect("read websocket body");
        assert_eq!(
            response_body.as_ref(),
            b"data: {\"type\":\"response.done\"}\n\n"
        );

        let headers = headers_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture websocket headers");
        assert_eq!(captured_header(&headers, "upgrade"), Some("websocket"));
        assert!(captured_header(&headers, "connection")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .split(',')
            .any(|token| token.trim() == "upgrade"));
        assert_eq!(
            captured_header(&headers, "sec-websocket-version"),
            Some("13")
        );
        assert!(captured_header(&headers, "sec-websocket-key").is_some());
        assert!(captured_header(&headers, "host").is_some());
        assert_eq!(
            captured_header(&headers, "authorization"),
            Some("Bearer token_ws")
        );
        assert_eq!(
            captured_header(&headers, "openai-beta"),
            Some("responses_websockets=2026-02-06")
        );
        assert!(captured_header(&headers, "content-type").is_none());
        let frame = frame_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture request frame");
        let frame: serde_json::Value =
            serde_json::from_str(frame.as_str()).expect("parse request frame");
        assert_eq!(
            frame.get("type").and_then(serde_json::Value::as_str),
            Some("response.create")
        );
        assert_eq!(
            frame.get("model").and_then(serde_json::Value::as_str),
            Some("codex")
        );
        assert_eq!(
            frame.get("input").and_then(serde_json::Value::as_str),
            Some("hello")
        );
        handle.join().expect("join websocket upstream");
    }

    #[test]
    fn send_websocket_upstream_request_uses_configured_proxy() {
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let (target_url, headers_rx, _frame_rx, target_handle) =
            spawn_mock_websocket_upstream(r#"{"type":"response.completed"}"#);
        let parsed_target = reqwest::Url::parse(target_url.as_str()).expect("parse target url");
        let target_addr = parsed_target
            .socket_addrs(|| None)
            .expect("target socket addr")
            .first()
            .expect("target has addr")
            .to_string();
        let (proxy_url, connect_rx, proxy_handle) = spawn_http_connect_proxy(target_addr);
        let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", proxy_url.as_str());
        let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
        crate::gateway::reload_runtime_config_from_env();

        let body = Bytes::from(r#"{"model":"codex","input":"hello"}"#);
        let response = super::send_websocket_upstream_request(
            "ws://example.invalid/v1/responses",
            "acct_ws_proxy",
            Some(Instant::now() + Duration::from_secs(5)),
            &[],
            &body,
        )
        .expect("websocket upstream request connects through proxy");
        let response_body = response
            .read_all_bytes()
            .expect("read proxy websocket body");
        assert_eq!(
            response_body.as_ref(),
            b"data: {\"type\":\"response.completed\"}\n\n"
        );
        assert_eq!(
            connect_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("proxy CONNECT request"),
            "CONNECT example.invalid:80 HTTP/1.1"
        );
        let headers = headers_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("capture proxied websocket headers");
        assert_eq!(captured_header(&headers, "host"), Some("example.invalid"));
        proxy_handle.join().expect("join websocket proxy");
        target_handle.join().expect("join websocket target");
    }

    #[test]
    fn send_websocket_upstream_request_returns_err_for_invalid_body_before_handshake() {
        let _env_lock = crate::test_env_guard();
        // Invalid bodies must return Err immediately (before handshake) so the
        // caller can fall back to HTTP streaming.
        let invalid_body = Bytes::from_static(br#"{"model":"codex"}"#);
        let result = super::send_websocket_upstream_request(
            "wss://chatgpt.com/backend-api/codex/responses",
            "acct_ws_invalid_body",
            None,
            &[],
            &invalid_body,
        );
        assert!(result.is_err(), "expected Err for invalid body");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("valid WebSocket response.create payload"),
            "error message should mention response.create payload, got: {msg}"
        );
    }

    #[test]
    fn send_websocket_upstream_request_falls_back_on_unreachable_host() {
        let _env_lock = crate::test_env_guard();
        let _reload_guard = RuntimeConfigReloadGuard;
        let _proxy_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_PROXY_URL", "");
        let _proxy_list_guard = EnvGuard::set("CODEXMANAGER_PROXY_LIST", "");
        crate::gateway::reload_runtime_config_from_env();
        // Attempting to connect to a port that is not listening must return Err quickly
        // (i.e., the handshake_timeout path works and does not block indefinitely).
        // We use a deadline 2 seconds from now so the test completes quickly.
        let deadline = Instant::now() + Duration::from_secs(2);
        let body = Bytes::from(r#"{"model":"codex","input":"hello"}"#);
        let result = super::send_websocket_upstream_request(
            "wss://127.0.0.1:1", // port 1 is not open
            "acct_ws_unreachable",
            Some(deadline),
            &[],
            &body,
        );
        assert!(result.is_err(), "expected Err when host is unreachable");
    }
}
