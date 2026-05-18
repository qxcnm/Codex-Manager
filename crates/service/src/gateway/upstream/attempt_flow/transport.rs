use bytes::Bytes;
use codexmanager_core::storage::Account;
use futures_util::StreamExt;
use rand::Rng;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tiny_http::Request;
use tokio::runtime::Builder;

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
) -> Result<GatewayUpstreamResponse, reqwest::Error> {
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
) -> Result<GatewayUpstreamResponse, reqwest::Error> {
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
) -> Result<GatewayUpstreamResponse, reqwest::Error> {
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
    let result = if use_async_stream_transport {
        let async_client =
            super::super::super::async_upstream_client_for_account(account.id.as_str());
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
                let fresh_async = super::super::super::fresh_async_upstream_client_for_account(
                    account.id.as_str(),
                );
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
                            Err(second_err)
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
                            Err(second_err)
                        }
                    }
                }
            }
        }
    } else {
        match build_request(client, upstream_headers.as_slice(), &body_for_request).send() {
            Ok(resp) => Ok(resp.into()),
            Err(first_err) => {
                let fresh =
                    super::super::super::fresh_upstream_client_for_account(account.id.as_str());
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
                            Err(second_err)
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
                            Err(second_err)
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

#[cfg(test)]
mod tests {
    use super::{
        apply_gemini_codex_compat_header_profile, encode_request_body, resolve_request_compression,
        resolve_request_compression_with_flag, send_async_stream_request,
        should_retry_transport_without_compression, should_wrap_upstream_as_stream_response,
        strip_compact_service_tier_for_transport, RequestCompression, CPA_GEMINI_CODEX_USER_AGENT,
    };
    use bytes::Bytes;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

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
}
