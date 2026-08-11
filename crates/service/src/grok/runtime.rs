use bytes::Bytes;
use codexmanager_core::storage::{now_ts, GrokCredentialRecord, Storage};
use codexmanager_grok_provider::{
    build_web_headers, infer_tier_from_quota, models_for_tier, GrokChatMode, GrokHeaderError,
    GrokModelCapability, GrokQuotaMode, GrokRateLimitRequest, GrokRateLimitWindow,
    GrokWebChatRequest, GrokWebEvent, GrokWebHeaderProfile, GrokWebStreamParser, SecretSsoToken,
    StatsigSignRequest, StatsigSignResponse,
};
use rand::{rngs::OsRng, RngCore};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    sync::{mpsc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use crate::gateway::{
    canonical::CanonicalRequest,
    upstream::{
        GatewayByteStream, GatewayByteStreamItem, GatewayStreamResponse, GatewayUpstreamResponse,
    },
};

const GROK_BASE_URL: &str = "https://grok.com";
const DEFAULT_STATSIG_SIGNER_URL: &str = "https://grok.wodf.de/sign";
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const CHAT_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_INDEX_BYTES: usize = 4 << 20;
const GROK_STREAM_CHANNEL_CAPACITY: usize = 128;
const STATSIG_CACHE_TTL: Duration = Duration::from_secs(3_600);

type StatsigCache = HashMap<String, (String, Instant)>;
static STATSIG_CACHE: OnceLock<Mutex<StatsigCache>> = OnceLock::new();

pub(crate) fn is_grok_model(model: Option<&str>) -> bool {
    model.is_some_and(|model| model.trim().to_ascii_lowercase().starts_with("grok/"))
}

pub(crate) fn is_supported_grok_model(model: &str) -> bool {
    let public_id = model
        .trim()
        .strip_prefix("grok/")
        .or_else(|| model.trim().strip_prefix("GROK/"))
        .unwrap_or(model.trim());
    codexmanager_grok_provider::GROK_WEB_MODELS
        .iter()
        .any(|spec| {
            spec.public_id.eq_ignore_ascii_case(public_id)
                && spec.capability == GrokModelCapability::Chat
        })
}

pub(crate) fn execute_responses_request(
    storage: &Storage,
    body: &Bytes,
    platform_model: &str,
) -> Result<GatewayUpstreamResponse, String> {
    let public_id = platform_model
        .trim()
        .strip_prefix("grok/")
        .or_else(|| platform_model.trim().strip_prefix("GROK/"))
        .unwrap_or(platform_model.trim());
    let mode = grok_mode(public_id).ok_or_else(|| "grok_model_not_supported".to_string())?;
    let canonical = CanonicalRequest::from_responses_bytes(body.as_ref())?;
    let prompt = canonical_prompt(&canonical)?;
    let candidates = load_runtime_candidates(storage, platform_model, mode)?;
    if candidates.is_empty() {
        return Err("no_active_grok_credentials_for_model".into());
    }

    let mut last_error = "grok_upstream_unavailable".to_string();
    for candidate in candidates {
        let started = Instant::now();
        match open_chat_response(&candidate, &prompt, mode) {
            Ok(response) => {
                return stream_chat_response(
                    storage,
                    response,
                    candidate.record.id,
                    platform_model.to_string(),
                    prompt,
                    started,
                );
            }
            Err(error) => {
                persist_open_failure(storage, &candidate.record.id, &error, started.elapsed());
                last_error = error;
            }
        }
    }
    Err(last_error)
}

struct RuntimeCandidate {
    record: GrokCredentialRecord,
    token: SecretSsoToken,
}

fn load_runtime_candidates(
    storage: &Storage,
    platform_model: &str,
    mode: GrokChatMode,
) -> Result<Vec<RuntimeCandidate>, String> {
    let now = now_ts();
    let proven_ids = storage
        .list_grok_credential_model_availability(None)
        .map_err(|_| "credential storage operation failed".to_string())?
        .into_iter()
        .filter(|item| {
            item.status == "available"
                && item.model_slug.eq_ignore_ascii_case(platform_model)
                && item.checked_at >= now.saturating_sub(86_400)
        })
        .map(|item| item.credential_id)
        .collect::<HashSet<_>>();
    let exhausted_ids = storage
        .list_grok_quota_windows(None)
        .map_err(|_| "credential storage operation failed".to_string())?
        .into_iter()
        .filter(|window| {
            window.mode == chat_mode_name(mode)
                && window.remaining_queries <= 0
                && window.reset_at > now
        })
        .map(|window| window.credential_id)
        .collect::<HashSet<_>>();
    let mut candidates = Vec::new();
    for record in storage
        .list_grok_credentials()
        .map_err(|_| "credential storage operation failed".to_string())?
    {
        if record.status != "active"
            || record.cooldown_until.is_some_and(|until| until > now)
            || !proven_ids.contains(&record.id)
            || exhausted_ids.contains(&record.id)
        {
            continue;
        }
        let Some(secret) = storage
            .read_grok_credential_secret(&record.id)
            .map_err(|_| "credential storage operation failed".to_string())?
        else {
            continue;
        };
        let token = SecretSsoToken::parse(secret.sso_token).map_err(safe_header_error)?;
        candidates.push(RuntimeCandidate { record, token });
    }
    Ok(candidates)
}

fn open_chat_response(
    candidate: &RuntimeCandidate,
    prompt: &str,
    mode: GrokChatMode,
) -> Result<reqwest::blocking::Response, String> {
    let client = build_client_with_timeout(candidate.record.proxy_url.as_deref(), CHAT_TIMEOUT)?;
    let profile = GrokWebHeaderProfile::default();
    let path = "/rest/app-chat/conversations/new";
    let mut force_refresh = false;
    for attempt in 0..2 {
        let statsig_id = cached_statsig_signature(
            &client,
            &candidate.token,
            &profile,
            &candidate.record.id,
            path,
            force_refresh,
        )?;
        let headers = build_web_headers(
            &candidate.token,
            &profile,
            &request_id(),
            Some(&statsig_id),
            None,
        )
        .map_err(safe_header_error)?
        .into_inner();
        let response = client
            .post(format!("{GROK_BASE_URL}{path}"))
            .headers(headers)
            .json(&GrokWebChatRequest::new(prompt, mode))
            .send()
            .map_err(|_| "grok_chat_network_error".to_string())?;
        match response.status() {
            status if status.is_success() => return Ok(response),
            reqwest::StatusCode::UNAUTHORIZED => return Err("grok_credential_unauthorized".into()),
            reqwest::StatusCode::FORBIDDEN if attempt == 0 => {
                invalidate_statsig(&candidate.record.id, path);
                force_refresh = true;
            }
            reqwest::StatusCode::FORBIDDEN => return Err("grok_antibot_challenge".into()),
            reqwest::StatusCode::TOO_MANY_REQUESTS => return Err("grok_chat_rate_limited".into()),
            status if status.is_server_error() => {
                return Err(format!("grok_chat_http_{}", status.as_u16()))
            }
            status => return Err(format!("grok_chat_http_{}", status.as_u16())),
        }
    }
    Err("grok_antibot_challenge".into())
}

fn stream_chat_response(
    storage: &Storage,
    response: reqwest::blocking::Response,
    credential_id: String,
    model: String,
    prompt: String,
    started: Instant,
) -> Result<GatewayUpstreamResponse, String> {
    let (body_tx, body_rx) =
        mpsc::sync_channel::<GatewayByteStreamItem>(GROK_STREAM_CHANNEL_CAPACITY);
    let storage_path = storage.path().map(std::path::Path::to_path_buf);
    let stream_credential_id = credential_id.clone();
    thread::Builder::new()
        .name("codexmanager-grok-stream".into())
        .spawn(move || {
            consume_chat_stream(
                response,
                &body_tx,
                &model,
                &prompt,
                &stream_credential_id,
                storage_path.as_deref(),
                started,
            );
        })
        .map_err(|_| "spawn_grok_stream_worker_failed".to_string())?;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    Ok(GatewayUpstreamResponse::Stream(
        GatewayStreamResponse::new(
            reqwest::StatusCode::OK,
            headers,
            GatewayByteStream::from_receiver(body_rx),
        )
        .with_actual_source_id(Some(credential_id)),
    ))
}

fn consume_chat_stream(
    mut response: reqwest::blocking::Response,
    tx: &mpsc::SyncSender<GatewayByteStreamItem>,
    model: &str,
    prompt: &str,
    credential_id: &str,
    storage_path: Option<&std::path::Path>,
    started: Instant,
) {
    let response_id = format!("resp_{}", request_id().replace('-', ""));
    let message_id = format!("msg_{}", request_id().replace('-', ""));
    let mut sequence = 0_i64;
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut parser = GrokWebStreamParser::default();
    let mut buffer = [0_u8; 16 * 1024];
    let mut text_started = false;
    if !send_response_event(
        tx,
        "response.created",
        serde_json::json!({
            "type":"response.created", "sequence_number":sequence,
            "response":{"id":response_id,"object":"response","status":"in_progress","model":model,"output":[]}
        }),
    ) {
        return;
    }
    sequence += 1;
    loop {
        let read = match response.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => {
                persist_stream_failure(
                    storage_path,
                    credential_id,
                    "grok_stream_read_failed",
                    started,
                );
                let _ = tx.send(GatewayByteStreamItem::Error(
                    "grok_stream_read_failed".into(),
                ));
                return;
            }
        };
        let events = match parser.push(&buffer[..read]) {
            Ok(events) => events,
            Err(_) => {
                persist_stream_failure(
                    storage_path,
                    credential_id,
                    "grok_stream_parse_failed",
                    started,
                );
                let _ = tx.send(GatewayByteStreamItem::Error(
                    "grok_stream_parse_failed".into(),
                ));
                return;
            }
        };
        for event in events {
            match event {
                GrokWebEvent::ReasoningDelta(delta) => {
                    reasoning.push_str(&delta);
                    if !send_response_event(
                        tx,
                        "response.reasoning_summary_text.delta",
                        serde_json::json!({
                            "type":"response.reasoning_summary_text.delta", "sequence_number":sequence,
                            "response_id":response_id, "delta":delta
                        }),
                    ) {
                        return;
                    }
                    sequence += 1;
                }
                GrokWebEvent::TextDelta(delta) => {
                    if !text_started {
                        if !send_response_event(
                            tx,
                            "response.output_item.added",
                            serde_json::json!({
                                "type":"response.output_item.added", "sequence_number":sequence,
                                "output_index":0, "item":{"id":message_id,"type":"message","status":"in_progress","role":"assistant","content":[]}
                            }),
                        ) {
                            return;
                        }
                        sequence += 1;
                        if !send_response_event(
                            tx,
                            "response.content_part.added",
                            serde_json::json!({
                                "type":"response.content_part.added", "sequence_number":sequence,
                                "item_id":message_id, "output_index":0, "content_index":0,
                                "part":{"type":"output_text","text":"","annotations":[]}
                            }),
                        ) {
                            return;
                        }
                        sequence += 1;
                        text_started = true;
                    }
                    text.push_str(&delta);
                    if !send_response_event(
                        tx,
                        "response.output_text.delta",
                        serde_json::json!({
                            "type":"response.output_text.delta", "sequence_number":sequence,
                            "item_id":message_id, "output_index":0, "content_index":0, "delta":delta
                        }),
                    ) {
                        return;
                    }
                    sequence += 1;
                }
                GrokWebEvent::Image { url, completed } if completed => {
                    if !text_started {
                        if !send_response_event(
                            tx,
                            "response.output_item.added",
                            serde_json::json!({
                                "type":"response.output_item.added", "sequence_number":sequence,
                                "output_index":0, "item":{"id":message_id,"type":"message","status":"in_progress","role":"assistant","content":[]}
                            }),
                        ) {
                            return;
                        }
                        sequence += 1;
                        if !send_response_event(
                            tx,
                            "response.content_part.added",
                            serde_json::json!({
                                "type":"response.content_part.added", "sequence_number":sequence,
                                "item_id":message_id, "output_index":0, "content_index":0,
                                "part":{"type":"output_text","text":"","annotations":[]}
                            }),
                        ) {
                            return;
                        }
                        sequence += 1;
                        text_started = true;
                    }
                    let delta = format!("\n\n![Grok image]({url})");
                    text.push_str(&delta);
                    if !send_response_event(
                        tx,
                        "response.output_text.delta",
                        serde_json::json!({
                            "type":"response.output_text.delta", "sequence_number":sequence,
                            "item_id":message_id, "output_index":0, "content_index":0, "delta":delta
                        }),
                    ) {
                        return;
                    }
                    sequence += 1;
                }
                GrokWebEvent::UpstreamError { code, .. } => {
                    let error = code
                        .map(|code| format!("grok_upstream_error_{code}"))
                        .unwrap_or_else(|| "grok_upstream_error".into());
                    persist_stream_failure(storage_path, credential_id, &error, started);
                    let _ = tx.send(GatewayByteStreamItem::Error(error));
                    return;
                }
                GrokWebEvent::Conversation { .. }
                | GrokWebEvent::ParentResponse { .. }
                | GrokWebEvent::Image { .. } => {}
            }
        }
    }
    if parser.finish().is_err() {
        persist_stream_failure(
            storage_path,
            credential_id,
            "grok_stream_incomplete",
            started,
        );
        let _ = tx.send(GatewayByteStreamItem::Error(
            "grok_stream_incomplete".into(),
        ));
        return;
    }
    if !text_started {
        let _ = send_response_event(
            tx,
            "response.output_item.added",
            serde_json::json!({
                "type":"response.output_item.added", "sequence_number":sequence,
                "output_index":0, "item":{"id":message_id,"type":"message","status":"in_progress","role":"assistant","content":[]}
            }),
        );
        sequence += 1;
        let _ = send_response_event(
            tx,
            "response.content_part.added",
            serde_json::json!({
                "type":"response.content_part.added", "sequence_number":sequence,
                "item_id":message_id, "output_index":0, "content_index":0,
                "part":{"type":"output_text","text":"","annotations":[]}
            }),
        );
        sequence += 1;
    }
    let _ = send_response_event(
        tx,
        "response.output_text.done",
        serde_json::json!({
            "type":"response.output_text.done", "sequence_number":sequence,
            "item_id":message_id, "output_index":0, "content_index":0, "text":text
        }),
    );
    sequence += 1;
    let _ = send_response_event(
        tx,
        "response.content_part.done",
        serde_json::json!({
            "type":"response.content_part.done", "sequence_number":sequence,
            "item_id":message_id, "output_index":0, "content_index":0,
            "part":{"type":"output_text","text":text,"annotations":[]}
        }),
    );
    sequence += 1;
    let output_item = serde_json::json!({
        "id":message_id,"type":"message","status":"completed","role":"assistant",
        "content":[{"type":"output_text","text":text,"annotations":[]}]
    });
    let _ = send_response_event(
        tx,
        "response.output_item.done",
        serde_json::json!({
            "type":"response.output_item.done", "sequence_number":sequence,
            "output_index":0, "item":output_item
        }),
    );
    sequence += 1;
    let input_tokens = estimate_tokens(prompt);
    let output_tokens = estimate_tokens(&format!("{reasoning}{text}"));
    let _ = send_response_event(
        tx,
        "response.completed",
        serde_json::json!({
            "type":"response.completed", "sequence_number":sequence,
            "response":{
                "id":response_id,"object":"response","status":"completed","model":model,
                "output":[output_item],
                "usage":{"input_tokens":input_tokens,"output_tokens":output_tokens,"total_tokens":input_tokens + output_tokens}
            }
        }),
    );
    if let Some(path) = storage_path {
        if let Ok(storage) = Storage::open(path) {
            let _ = storage.record_grok_credential_success(
                credential_id,
                started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            );
        }
    }
    let _ = tx.send(GatewayByteStreamItem::Eof);
}

fn send_response_event(
    tx: &mpsc::SyncSender<GatewayByteStreamItem>,
    event_type: &str,
    value: Value,
) -> bool {
    let Ok(data) = serde_json::to_string(&value) else {
        return false;
    };
    tx.send(GatewayByteStreamItem::Chunk(Bytes::from(format!(
        "event: {event_type}\ndata: {data}\n\n"
    ))))
    .is_ok()
}

fn canonical_prompt(request: &CanonicalRequest) -> Result<String, String> {
    if request.tools.as_ref().is_some_and(|value| {
        !value.as_array().is_some_and(|items| items.is_empty()) && !value.is_null()
    }) {
        return Err("grok_tools_not_supported_yet".into());
    }
    if request.input.as_ref().is_some_and(contains_image) {
        return Err("grok_image_input_not_supported_yet".into());
    }
    let mut sections = Vec::new();
    if let Some(instructions) = request.instructions.as_ref() {
        let text = value_text(instructions);
        if !text.is_empty() {
            sections.push(format!("[SYSTEM]\n{text}"));
        }
    }
    if let Some(input) = request.input.as_ref() {
        append_input_sections(input, &mut sections);
    }
    if sections.is_empty() {
        return Err("grok_input_text_required".into());
    }
    Ok(sections.join("\n\n"))
}

fn append_input_sections(value: &Value, sections: &mut Vec<String>) {
    match value {
        Value::String(text) if !text.trim().is_empty() => sections.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                append_input_sections(item, sections);
            }
        }
        Value::Object(object) => {
            let role = object.get("role").and_then(Value::as_str).unwrap_or("user");
            let content = object
                .get("content")
                .or_else(|| object.get("output"))
                .map(value_text)
                .unwrap_or_else(|| value_text(value));
            if !content.trim().is_empty() {
                sections.push(format!("[{}]\n{content}", role.to_ascii_uppercase()));
            }
        }
        _ => {}
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(value_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(object) => object
            .get("text")
            .or_else(|| object.get("output_text"))
            .or_else(|| object.get("input_text"))
            .or_else(|| object.get("output"))
            .or_else(|| object.get("content"))
            .map(value_text)
            .unwrap_or_default(),
        Value::Number(number) => number.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
    }
}

fn contains_image(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(contains_image),
        Value::Object(object) => {
            object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "input_image" | "image_url" | "image"))
                || object.contains_key("image_url")
                || object.values().any(contains_image)
        }
        Value::String(text) => text.starts_with("data:image/"),
        _ => false,
    }
}

fn grok_mode(public_id: &str) -> Option<GrokChatMode> {
    match public_id.to_ascii_lowercase().as_str() {
        "grok-chat-fast" => Some(GrokChatMode::Fast),
        "grok-chat-auto" => Some(GrokChatMode::Auto),
        "grok-chat-expert" => Some(GrokChatMode::Expert),
        "grok-chat-heavy" => Some(GrokChatMode::Heavy),
        _ => None,
    }
}

fn chat_mode_name(mode: GrokChatMode) -> &'static str {
    match mode {
        GrokChatMode::Fast => "fast",
        GrokChatMode::Auto | GrokChatMode::Expert => "auto",
        GrokChatMode::Heavy => "heavy",
    }
}

fn estimate_tokens(text: &str) -> i64 {
    ((text.chars().count() + 3) / 4).max(1) as i64
}

fn persist_open_failure(storage: &Storage, id: &str, code: &str, elapsed: Duration) {
    let (cooldown, status) = match code {
        "grok_credential_unauthorized" => (None, Some("isolated")),
        "grok_antibot_challenge" => (Some(900), None),
        "grok_chat_rate_limited" => (Some(300), None),
        _ => (Some(30), None),
    };
    let _ = storage.record_grok_credential_failure(
        id,
        code,
        cooldown,
        status,
        elapsed.as_millis().min(u64::MAX as u128) as u64,
    );
}

fn persist_stream_failure(
    storage_path: Option<&std::path::Path>,
    id: &str,
    code: &str,
    started: Instant,
) {
    if let Some(path) = storage_path {
        if let Ok(storage) = Storage::open(path) {
            persist_open_failure(&storage, id, code, started.elapsed());
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GrokModelProbeSummary {
    pub credential_id: String,
    pub tier: String,
    pub available_models: Vec<String>,
    pub quota_windows: Vec<GrokProbeQuotaWindow>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GrokProbeQuotaWindow {
    pub mode: String,
    pub remaining_queries: i64,
    pub total_queries: i64,
    pub window_size_seconds: i64,
    pub reset_at: i64,
}

pub(crate) fn probe_credential_models(
    storage: &Storage,
    credential_id: &str,
) -> Result<GrokModelProbeSummary, String> {
    let started = Instant::now();
    let result = probe_credential_models_inner(storage, credential_id, started);
    let latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    match &result {
        Ok(_) => {
            let _ = storage.record_grok_credential_success(credential_id, latency_ms);
        }
        Err(code) => {
            let (cooldown, status) = match code.as_str() {
                "grok_credential_unauthorized" => (None, Some("isolated")),
                "grok_antibot_challenge" => (Some(900), None),
                "grok_quota_rate_limited" => (Some(300), None),
                _ => (Some(30), None),
            };
            let _ = storage.record_grok_credential_failure(
                credential_id,
                code,
                cooldown,
                status,
                latency_ms,
            );
        }
    }
    result
}

fn probe_credential_models_inner(
    storage: &Storage,
    credential_id: &str,
    started: Instant,
) -> Result<GrokModelProbeSummary, String> {
    let record = storage
        .list_grok_credentials()
        .map_err(|_| "credential storage operation failed".to_string())?
        .into_iter()
        .find(|record| record.id == credential_id)
        .ok_or_else(|| "grok_credential_not_found".to_string())?;
    if record.status != "active" {
        return Err("grok_credential_not_active".into());
    }
    let secret = storage
        .read_grok_credential_secret(credential_id)
        .map_err(|_| "credential storage operation failed".to_string())?
        .ok_or_else(|| "grok_credential_secret_not_found".to_string())?;
    let token = SecretSsoToken::parse(secret.sso_token).map_err(safe_header_error)?;
    let client = build_client(record.proxy_url.as_deref())?;
    let profile = GrokWebHeaderProfile::default();
    let statsig_id = cached_statsig_signature(
        &client,
        &token,
        &profile,
        credential_id,
        "/rest/rate-limits",
        false,
    )?;
    let mut observed = Vec::new();
    let mut summaries = Vec::new();
    for mode in [GrokQuotaMode::Auto, GrokQuotaMode::Fast] {
        let window = fetch_quota_window(&client, &token, &profile, &statsig_id, mode)?;
        let reset_at = now_ts().saturating_add(window.window_size_seconds.max(1));
        storage
            .upsert_grok_quota_window(
                credential_id,
                quota_mode_name(mode),
                window.remaining_queries,
                window.total_queries,
                window.window_size_seconds,
                reset_at,
            )
            .map_err(|_| "credential storage operation failed".to_string())?;
        summaries.push(GrokProbeQuotaWindow {
            mode: quota_mode_name(mode).into(),
            remaining_queries: window.remaining_queries,
            total_queries: window.total_queries,
            window_size_seconds: window.window_size_seconds,
            reset_at,
        });
        observed.push((mode, window));
    }
    let Some(tier) = infer_tier_from_quota(&observed) else {
        let _ = storage.set_grok_credential_tier(credential_id, "unknown");
        let _ = storage.replace_grok_credential_models(credential_id, &[], None);
        return Err("grok_quota_shape_unknown".into());
    };
    let tier_name = tier_name(tier);
    let available_models = models_for_tier(tier)
        .filter(|spec| spec.capability == GrokModelCapability::Chat)
        .map(|spec| format!("grok/{}", spec.public_id))
        .collect::<Vec<_>>();
    storage
        .set_grok_credential_tier(credential_id, tier_name)
        .and_then(|_| {
            storage.replace_grok_credential_models(
                credential_id,
                &available_models,
                Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
            )
        })
        .map_err(|_| "credential storage operation failed".to_string())?;
    Ok(GrokModelProbeSummary {
        credential_id: credential_id.into(),
        tier: tier_name.into(),
        available_models,
        quota_windows: summaries,
        checked_at: now_ts(),
    })
}

fn build_client(proxy_url: Option<&str>) -> Result<Client, String> {
    build_client_with_timeout(proxy_url, PROBE_TIMEOUT)
}

fn build_client_with_timeout(proxy_url: Option<&str>, timeout: Duration) -> Result<Client, String> {
    let mut builder = Client::builder()
        .connect_timeout(Duration::from_secs(12))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none());
    if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
        let proxy = reqwest::Proxy::all(proxy_url).map_err(|_| "invalid_grok_proxy_url")?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|_| "initialize_grok_http_client_failed".into())
}

fn fetch_statsig_meta(
    client: &Client,
    token: &SecretSsoToken,
    profile: &GrokWebHeaderProfile,
) -> Result<String, String> {
    let response = client
        .get(format!("{GROK_BASE_URL}/index"))
        .header(
            "accept",
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header("accept-language", &profile.accept_language)
        .header("user-agent", &profile.user_agent)
        .header("cookie", sso_cookie(token, profile)?)
        .send()
        .map_err(|_| "grok_statsig_meta_request_failed".to_string())?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("grok_credential_unauthorized".into());
    }
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        return Err("grok_antibot_challenge".into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "grok_statsig_meta_http_{}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_INDEX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "grok_statsig_meta_read_failed".to_string())?;
    if bytes.len() > MAX_INDEX_BYTES {
        return Err("grok_statsig_meta_too_large".into());
    }
    extract_meta_content(&bytes).ok_or_else(|| "grok_statsig_meta_missing".into())
}

fn fetch_statsig_signature(
    client: &Client,
    meta_content: &str,
    path: &str,
) -> Result<String, String> {
    let signer_url = std::env::var("CODEXMANAGER_GROK_STATSIG_SIGNER_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_STATSIG_SIGNER_URL.into());
    validate_signer_url(&signer_url)?;
    let response = client
        .post(signer_url)
        .json(&StatsigSignRequest::new("POST", path, meta_content))
        .send()
        .map_err(|_| "grok_statsig_signer_request_failed".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "grok_statsig_signer_http_{}",
            response.status().as_u16()
        ));
    }
    let mut body = Vec::new();
    response
        .take(4_097)
        .read_to_end(&mut body)
        .map_err(|_| "grok_statsig_signer_response_invalid".to_string())?;
    if body.len() > 4_096 {
        return Err("grok_statsig_signer_response_invalid".into());
    }
    let response = serde_json::from_slice::<StatsigSignResponse>(&body)
        .map_err(|_| "grok_statsig_signer_response_invalid".to_string())?;
    response
        .validated_id()
        .map(str::to_string)
        .ok_or_else(|| "grok_statsig_signer_response_invalid".into())
}

fn cached_statsig_signature(
    client: &Client,
    token: &SecretSsoToken,
    profile: &GrokWebHeaderProfile,
    credential_id: &str,
    path: &str,
    force_refresh: bool,
) -> Result<String, String> {
    let key = format!("{credential_id}\0POST\0{path}");
    if !force_refresh {
        if let Ok(cache) = STATSIG_CACHE
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        {
            if let Some((value, expires_at)) = cache.get(&key) {
                if Instant::now() < *expires_at {
                    return Ok(value.clone());
                }
            }
        }
    }
    let meta_content = fetch_statsig_meta(client, token, profile)?;
    let value = fetch_statsig_signature(client, &meta_content, path)?;
    if let Ok(mut cache) = STATSIG_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.insert(key, (value.clone(), Instant::now() + STATSIG_CACHE_TTL));
    }
    Ok(value)
}

fn invalidate_statsig(credential_id: &str, path: &str) {
    if let Ok(mut cache) = STATSIG_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        cache.remove(&format!("{credential_id}\0POST\0{path}"));
    }
}

fn fetch_quota_window(
    client: &Client,
    token: &SecretSsoToken,
    profile: &GrokWebHeaderProfile,
    statsig_id: &str,
    mode: GrokQuotaMode,
) -> Result<GrokRateLimitWindow, String> {
    let headers = build_web_headers(token, profile, &request_id(), Some(statsig_id), None)
        .map_err(safe_header_error)?
        .into_inner();
    let response = client
        .post(format!("{GROK_BASE_URL}/rest/rate-limits"))
        .headers(headers)
        .json(&GrokRateLimitRequest::new(mode))
        .send()
        .map_err(|_| "grok_quota_request_failed".to_string())?;
    match response.status() {
        reqwest::StatusCode::UNAUTHORIZED => return Err("grok_credential_unauthorized".into()),
        reqwest::StatusCode::FORBIDDEN => return Err("grok_antibot_challenge".into()),
        reqwest::StatusCode::TOO_MANY_REQUESTS => return Err("grok_quota_rate_limited".into()),
        status if !status.is_success() => {
            return Err(format!("grok_quota_http_{}", status.as_u16()))
        }
        _ => {}
    }
    let mut body = Vec::new();
    response
        .take(65_537)
        .read_to_end(&mut body)
        .map_err(|_| "grok_quota_response_invalid".to_string())?;
    if body.len() > 65_536 {
        return Err("grok_quota_response_invalid".into());
    }
    let window = serde_json::from_slice::<GrokRateLimitWindow>(&body)
        .map_err(|_| "grok_quota_response_invalid".to_string())?
        .normalized();
    if window.total_queries <= 0 {
        return Err("grok_quota_response_invalid".into());
    }
    Ok(window)
}

fn sso_cookie(token: &SecretSsoToken, profile: &GrokWebHeaderProfile) -> Result<String, String> {
    let headers = build_web_headers(token, profile, &request_id(), None, None)
        .map_err(safe_header_error)?
        .into_inner();
    headers
        .get(reqwest::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| "grok_cookie_build_failed".into())
}

fn extract_meta_content(body: &[u8]) -> Option<String> {
    let html = String::from_utf8_lossy(body);
    let lower = html.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(relative) = lower[cursor..].find("<meta") {
        let start = cursor + relative;
        let end = lower[start..].find('>').map(|index| start + index + 1)?;
        let tag = &html[start..end];
        let name = html_attr(tag, "name").map(normalize_meta_name);
        if name.as_deref() == Some("grok-site-verification") {
            if let Some(content) = html_attr(tag, "content") {
                let content = content.trim();
                if !content.is_empty() {
                    return Some(content.into());
                }
            }
        }
        cursor = end;
    }
    None
}

fn html_attr(tag: &str, wanted: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && !bytes[index].is_ascii_alphabetic() {
            index += 1;
        }
        let key_start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'-' | b'_'))
        {
            index += 1;
        }
        if key_start == index {
            continue;
        }
        let key = &tag[key_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let quote = bytes[index];
        let (value_start, value_end) = if matches!(quote, b'\'' | b'"') {
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            (start, index)
        } else {
            let start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'>'
            {
                index += 1;
            }
            (start, index)
        };
        if key.eq_ignore_ascii_case(wanted) {
            return Some(tag[value_start..value_end].into());
        }
    }
    None
}

fn normalize_meta_name(value: String) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['‐', '‑', '‒', '–', '—', '―'], "-")
}

fn validate_signer_url(value: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|_| "invalid_grok_statsig_signer_url")?;
    let safe_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() != "https" && !safe_http {
        return Err("invalid_grok_statsig_signer_url".into());
    }
    Ok(())
}

fn safe_header_error(_error: GrokHeaderError) -> String {
    "invalid_grok_credential_header".into()
}

fn quota_mode_name(mode: GrokQuotaMode) -> &'static str {
    match mode {
        GrokQuotaMode::Auto => "auto",
        GrokQuotaMode::Fast => "fast",
        GrokQuotaMode::Heavy => "heavy",
    }
}

fn tier_name(tier: codexmanager_grok_provider::GrokWebTier) -> &'static str {
    match tier {
        codexmanager_grok_provider::GrokWebTier::Basic => "basic",
        codexmanager_grok_provider::GrokWebTier::Super => "super",
        codexmanager_grok_provider::GrokWebTier::Heavy => "heavy",
    }
}

fn request_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_statsig_meta_with_attribute_order_and_unicode_dash() {
        for html in [
            r#"<meta content="seed-one" name="grok-site-verification">"#,
            r#"<META NAME='grok-site―verification' CONTENT='seed-two'>"#,
        ] {
            assert!(extract_meta_content(html.as_bytes()).is_some());
        }
        assert_eq!(
            extract_meta_content(br#"<meta content="seed" name="grok-site-verification">"#),
            Some("seed".into())
        );
    }

    #[test]
    fn signer_url_requires_https_or_loopback_http() {
        assert!(validate_signer_url("https://signer.example/sign").is_ok());
        assert!(validate_signer_url("http://127.0.0.1:8788/sign").is_ok());
        assert!(validate_signer_url("http://8.8.8.8/sign").is_err());
        assert!(validate_signer_url("file:///tmp/sign").is_err());
    }

    #[test]
    fn canonical_prompt_keeps_roles_and_instructions() {
        let canonical = CanonicalRequest::from_responses_bytes(
            br#"{"model":"grok/grok-chat-fast","instructions":"Be concise","input":[{"role":"user","content":[{"type":"input_text","text":"hello"}]},{"role":"assistant","content":"prior answer"},{"role":"user","content":"continue"}]}"#,
        )
        .unwrap();
        assert_eq!(
            canonical_prompt(&canonical).unwrap(),
            "[SYSTEM]\nBe concise\n\n[USER]\nhello\n\n[ASSISTANT]\nprior answer\n\n[USER]\ncontinue"
        );
    }

    #[test]
    fn unsupported_tools_and_images_are_rejected_instead_of_dropped() {
        let with_tools = CanonicalRequest::from_responses_bytes(
            br#"{"model":"grok/grok-chat-fast","input":"hello","tools":[{"type":"function","name":"lookup"}]}"#,
        )
        .unwrap();
        assert_eq!(
            canonical_prompt(&with_tools).unwrap_err(),
            "grok_tools_not_supported_yet"
        );

        let with_image = CanonicalRequest::from_responses_bytes(
            br#"{"model":"grok/grok-chat-fast","input":[{"role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,AA=="}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            canonical_prompt(&with_image).unwrap_err(),
            "grok_image_input_not_supported_yet"
        );
    }
}
