//! Request payload capture for the request log detail view.
//!
//! Two switches (persisted as app settings) control what is stored:
//!
//! * redaction (default on): credential-like JSON keys are replaced with
//!   `[REDACTED]` before anything is written. When off, the body is stored
//!   exactly as sent upstream.
//! * preview (default on): only the first 16 KB is kept in
//!   `request_log_payloads`. When off, the full body is split into
//!   content-addressed fragments (top-level fields + list items) and stored
//!   once per distinct fragment, so consecutive requests of a conversation
//!   only add their new tail.
//!
//! Splitting, hashing and the database write run on a dedicated writer
//! thread; the gateway hot path only enqueues a cheap job.

use base64::Engine;
use bytes::Bytes;
use codexmanager_core::storage::{
    now_ts, RequestLogPayload, RequestLogPayloadManifestInput, RequestLogPayloadParentHint,
    RequestLogPayloadPart, RequestLogUpstreamAttempt, Storage, PAYLOAD_STAGE_CLIENT,
    PAYLOAD_STAGE_UPSTREAM,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Size cap of a stored preview when the preview mode is enabled.
pub(crate) const REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES: usize = 16 * 1024;
const REDACTED_PLACEHOLDER: &str = "[REDACTED]";
/// Field name used for bodies that are not a JSON object.
pub(crate) const RAW_BODY_FIELD: &str = "$body";
/// Candidate list fields, in detection order: OpenAI Responses `input`,
/// Chat Completions / Anthropic `messages`, Gemini `contents`.
const LIST_FIELDS: [&str; 3] = ["input", "messages", "contents"];
const PARENT_CACHE_CAPACITY: usize = 256;
/// Bounded queue of the async writer; full queues drop payload jobs instead
/// of blocking the gateway hot path.
#[cfg(not(test))]
const WRITER_QUEUE_CAPACITY: usize = 64;

static REDACTION_ENABLED: AtomicBool = AtomicBool::new(true);
static PREVIEW_ENABLED: AtomicBool = AtomicBool::new(true);
static NEXT_ATTEMPT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static ATTEMPT_STAGES: OnceLock<Mutex<AttemptStages>> = OnceLock::new();
const ATTEMPT_STAGES_CACHE_CAPACITY: usize = 65_536;

#[derive(Default)]
struct AttemptStages {
    counts: HashMap<String, usize>,
    order: VecDeque<String>,
}

fn stage_for_outbound_attempt(trace_id: &str) -> String {
    let state = ATTEMPT_STAGES.get_or_init(|| Mutex::new(AttemptStages::default()));
    let Ok(mut state) = state.lock() else {
        return format!(
            "upstream:{:020}",
            NEXT_ATTEMPT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
    };
    let first = match state.counts.get_mut(trace_id) {
        Some(count) => {
            *count += 1;
            false
        }
        None => {
            state.counts.insert(trace_id.to_string(), 1);
            state.order.push_back(trace_id.to_string());
            if state.order.len() > ATTEMPT_STAGES_CACHE_CAPACITY {
                if let Some(expired) = state.order.pop_front() {
                    state.counts.remove(&expired);
                }
            }
            true
        }
    };
    if first {
        PAYLOAD_STAGE_UPSTREAM.to_string()
    } else {
        format!(
            "upstream:{:020}",
            NEXT_ATTEMPT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )
    }
}

pub(crate) fn request_log_payload_redaction_enabled() -> bool {
    REDACTION_ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn set_request_log_payload_redaction_enabled(enabled: bool) -> bool {
    REDACTION_ENABLED.store(enabled, Ordering::Relaxed);
    enabled
}

pub(crate) fn request_log_payload_preview_enabled() -> bool {
    PREVIEW_ENABLED.load(Ordering::Relaxed)
}

pub(crate) fn set_request_log_payload_preview_enabled(enabled: bool) -> bool {
    PREVIEW_ENABLED.store(enabled, Ordering::Relaxed);
    enabled
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutboundPayloadContext<'a> {
    pub trace_id: &'a str,
    pub key_id: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct OutboundAttemptCapture {
    pub method: String,
    pub url: String,
    pub transport: String,
    pub content_encoding: Option<String>,
    pub wire_body: Bytes,
}

/// One captured request body plus the switches in effect when it was sent.
#[derive(Debug, Clone)]
pub(crate) struct RequestLogPayloadJob {
    pub trace_id: String,
    /// [`PAYLOAD_STAGE_CLIENT`] for the body as received from the client,
    /// [`PAYLOAD_STAGE_UPSTREAM`] for the body actually forwarded upstream.
    pub stage: String,
    pub body: Bytes,
    pub conversation_key: Option<String>,
    pub redact: bool,
    pub preview: bool,
    pub created_at: i64,
    /// Snapshot taken before enqueue, checked against SQLite at commit time.
    pub generation: i64,
    pub attempt: Option<OutboundAttemptCapture>,
}

/// Build the conversation key used to find the parent request whose items
/// can be shared. Scoped by API key so different callers never share a
/// chain; requests without a conversation id fall back to a per-key chain
/// (prefix sharing is still verified item by item, so this is always safe).
pub(crate) fn request_log_payload_conversation_key(
    key_id: &str,
    conversation_id: Option<&str>,
) -> String {
    let conversation = conversation_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("~");
    format!("{}|{}", key_id.trim(), conversation)
}

/// Hot-path entry: capture a request body for the given stage.
/// Never blocks on the database and never fails the request.
pub(crate) fn store_request_log_payload(
    storage: &Storage,
    trace_id: &str,
    stage: &str,
    body: &Bytes,
    conversation_key: Option<String>,
    attempt: Option<OutboundAttemptCapture>,
) {
    let trace_id = trace_id.trim();
    if trace_id.is_empty() || crate::storage_helpers::seaorm_enabled() {
        return;
    }
    let generation = match storage.request_log_payload_generation() {
        Ok(generation) => generation,
        Err(err) => {
            log::warn!("event=request_log_payload_generation_failed trace_id={trace_id} err={err}");
            return;
        }
    };
    let job = RequestLogPayloadJob {
        trace_id: trace_id.to_string(),
        generation,
        stage: stage.to_string(),
        body: body.clone(),
        conversation_key,
        redact: request_log_payload_redaction_enabled(),
        preview: request_log_payload_preview_enabled(),
        created_at: now_ts(),
        attempt,
    };
    #[cfg(test)]
    {
        persist_request_log_payload(storage, job);
    }
    #[cfg(not(test))]
    {
        let _ = storage;
        enqueue_request_log_payload(job);
    }
}

/// Capture the body exactly as received from the client. Called for every
/// authenticated gateway request so that logs which never reach the upstream
/// (validation rejects, local responses, aggregate-API failures) still have
/// content to show.
pub(crate) fn store_client_request_log_payload(
    storage: &Storage,
    trace_id: &str,
    body: &Bytes,
    conversation_key: Option<String>,
) {
    store_request_log_payload(
        storage,
        trace_id,
        PAYLOAD_STAGE_CLIENT,
        body,
        conversation_key,
        None,
    );
}

/// Called immediately before a transport submits the body, once per actual
/// HTTP or WebSocket send (including retries). No stage is created for a
/// rejected candidate that never reaches this call.
pub(crate) fn capture_outbound_payload(
    scope: OutboundPayloadContext<'_>,
    method: &str,
    target_url: &str,
    transport: &str,
    headers: &[(String, String)],
    wire_body: &Bytes,
    logical_body: Option<&Bytes>,
) {
    let Some(storage) = crate::storage_helpers::open_storage() else {
        return;
    };
    let stage = stage_for_outbound_attempt(scope.trace_id);
    let content_encoding = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-encoding"))
        .map(|(_, value)| value.clone());
    let safe_url = reqwest::Url::parse(target_url)
        .map(|mut url| {
            url.set_query(None);
            url.set_fragment(None);
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.to_string()
        })
        .unwrap_or_else(|_| "<invalid upstream URL>".to_string());
    store_request_log_payload(
        &storage,
        scope.trace_id,
        &stage,
        logical_body.unwrap_or(wire_body),
        Some(request_log_payload_conversation_key(scope.key_id, None)),
        Some(OutboundAttemptCapture {
            method: method.to_string(),
            url: safe_url,
            transport: transport.to_string(),
            content_encoding,
            wire_body: wire_body.clone(),
        }),
    );
}

fn bytes_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    hash
}

/// True when this is an upstream capture whose body was already stored as the
/// client capture, i.e. the gateway forwarded the request unchanged.
fn upstream_matches_client(storage: &Storage, job: &RequestLogPayloadJob, hash: &str) -> bool {
    if !job.stage.starts_with(PAYLOAD_STAGE_UPSTREAM) || hash.is_empty() {
        return false;
    }
    matches!(
        storage.find_request_log_payload_body_hash(&job.trace_id, PAYLOAD_STAGE_CLIENT),
        Ok(Some(existing)) if existing == hash
    )
}

#[cfg(not(test))]
struct PayloadWriter {
    tx: std::sync::mpsc::SyncSender<RequestLogPayloadJob>,
    dropped: std::sync::atomic::AtomicU64,
}

#[cfg(not(test))]
static PAYLOAD_WRITER: std::sync::OnceLock<PayloadWriter> = std::sync::OnceLock::new();

#[cfg(not(test))]
fn payload_writer() -> &'static PayloadWriter {
    PAYLOAD_WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<RequestLogPayloadJob>(WRITER_QUEUE_CAPACITY);
        let spawned = std::thread::Builder::new()
            .name("request-log-payload-writer".to_string())
            .spawn(move || run_payload_writer(rx, crate::storage_helpers::open_storage, || {}));
        if let Err(err) = spawned {
            log::warn!("event=request_log_payload_writer_spawn_failed err={err}");
        }
        PayloadWriter {
            tx,
            dropped: std::sync::atomic::AtomicU64::new(0),
        }
    })
}

fn run_payload_writer<F, S, H>(
    rx: std::sync::mpsc::Receiver<RequestLogPayloadJob>,
    mut open: F,
    mut before_write: H,
) where
    F: FnMut() -> Option<S>,
    S: std::ops::Deref<Target = Storage>,
    H: FnMut(),
{
    let mut cache = ParentCache::default();
    for job in rx {
        before_write();
        let Some(storage) = open() else {
            log::warn!(
                "event=request_log_payload_storage_unavailable trace_id={}",
                job.trace_id
            );
            continue;
        };
        persist_request_log_payload_with_cache(&storage, job, &mut cache);
    }
}

#[cfg(not(test))]
fn enqueue_request_log_payload(job: RequestLogPayloadJob) {
    let writer = payload_writer();
    match writer.tx.try_send(job) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(job)) => {
            let dropped = writer.dropped.fetch_add(1, Ordering::Relaxed) + 1;
            if dropped == 1 || dropped % 256 == 0 {
                log::warn!(
                    "event=request_log_payload_queue_full trace_id={} dropped={} capacity={}",
                    job.trace_id,
                    dropped,
                    WRITER_QUEUE_CAPACITY
                );
            }
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(job)) => {
            log::warn!(
                "event=request_log_payload_writer_unavailable trace_id={}",
                job.trace_id
            );
        }
    }
}

/// Most recent manifest per conversation with its resolved item ids, so a
/// follow-up request can compute its shared prefix without re-walking the
/// parent chain in the database.
#[derive(Default)]
pub(crate) struct ParentCache {
    entries: HashMap<String, RequestLogPayloadParentHint>,
    order: VecDeque<String>,
}

impl ParentCache {
    fn get(&self, key: &str) -> Option<&RequestLogPayloadParentHint> {
        self.entries.get(key)
    }

    fn put(&mut self, key: String, hint: RequestLogPayloadParentHint) {
        if self.entries.insert(key.clone(), hint).is_none() {
            self.order.push_back(key);
            while self.order.len() > PARENT_CACHE_CAPACITY {
                if let Some(evicted) = self.order.pop_front() {
                    self.entries.remove(&evicted);
                }
            }
        }
    }
}

/// Synchronously persist one job without a shared parent cache (tests).
#[cfg(test)]
pub(crate) fn persist_request_log_payload(storage: &Storage, job: RequestLogPayloadJob) {
    let mut cache = ParentCache::default();
    persist_request_log_payload_with_cache(storage, job, &mut cache);
}

pub(crate) fn persist_request_log_payload_with_cache(
    storage: &Storage,
    job: RequestLogPayloadJob,
    cache: &mut ParentCache,
) {
    if job.preview {
        let text = preview_payload_text(&job.body, job.redact);
        let (payload, truncated) =
            truncate_utf8_payload(&text, REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES);
        let hash = bytes_hash(&job.body);
        if upstream_matches_client(storage, &job, hash.as_str()) {
            persist_attempt_metadata(storage, &job, true);
            return;
        }
        let record = RequestLogPayload {
            trace_id: job.trace_id.clone(),
            stage: job.stage.clone(),
            payload,
            payload_bytes: job.body.len() as i64,
            payload_truncated: truncated,
            redacted: job.redact,
            body_hash: hash,
            created_at: job.created_at,
        };
        match storage.insert_request_log_payload_if_current(&record, job.generation) {
            Ok(true) => persist_attempt_metadata(storage, &job, false),
            Ok(false) => {}
            Err(err) => log::warn!(
                "event=request_log_payload_insert_failed trace_id={} err={}",
                job.trace_id,
                err
            ),
        }
        return;
    }

    let input = split_request_payload(&job);
    let cache_key = input
        .conversation_key
        .as_ref()
        .zip(input.list_field.as_ref())
        .map(|(conversation, field)| format!("{conversation}#{field}#{}", job.stage));
    let hint = cache_key.as_deref().and_then(|key| cache.get(key)).cloned();
    match storage.insert_request_log_payload_manifest_if_current(
        &input,
        hint.as_ref(),
        job.generation,
    ) {
        Ok(write) => {
            if write.inserted || write.identical_to_client {
                persist_attempt_metadata(storage, &job, write.identical_to_client);
            }
            if write.inserted {
                if let Some(key) = cache_key {
                    cache.put(
                        key,
                        RequestLogPayloadParentHint {
                            trace_id: input.trace_id.clone(),
                            stage: input.stage.clone(),
                            item_blob_ids: write.item_blob_ids,
                        },
                    );
                }
            }
        }
        Err(err) => {
            log::warn!(
                "event=request_log_payload_manifest_insert_failed trace_id={} err={}",
                job.trace_id,
                err
            );
        }
    }
}

fn persist_attempt_metadata(
    storage: &Storage,
    job: &RequestLogPayloadJob,
    identical_to_client: bool,
) {
    let Some(attempt) = job.attempt.as_ref() else {
        return;
    };
    let record = RequestLogUpstreamAttempt {
        trace_id: job.trace_id.clone(),
        stage: job.stage.clone(),
        method: attempt.method.clone(),
        url: attempt.url.clone(),
        transport: attempt.transport.clone(),
        content_encoding: attempt.content_encoding.clone(),
        wire_sha256: bytes_hash(&attempt.wire_body),
        identical_to_client,
        created_at: job.created_at,
    };
    if let Err(err) =
        storage.record_request_log_upstream_attempt_if_current(&record, job.generation)
    {
        log::warn!(
            "event=request_log_attempt_insert_failed trace_id={} err={err}",
            job.trace_id
        );
    }
}

fn preview_payload_text(body: &[u8], redact: bool) -> String {
    let Ok(text) = std::str::from_utf8(body) else {
        return "<non-utf8 body omitted>".to_string();
    };
    if !redact {
        return text.to_string();
    }
    sanitize_request_payload(body)
}

/// Split a request body into content-addressed fragments.
pub(crate) fn split_request_payload(job: &RequestLogPayloadJob) -> RequestLogPayloadManifestInput {
    let mut input = RequestLogPayloadManifestInput {
        trace_id: job.trace_id.clone(),
        stage: job.stage.clone(),
        body_hash: bytes_hash(&job.body),
        body_kind: "text".to_string(),
        list_field: None,
        conversation_key: job.conversation_key.clone(),
        previous_response_id: None,
        payload_bytes: job.body.len() as i64,
        redacted: job.redact,
        created_at: job.created_at,
        fields: Vec::new(),
        items: Vec::new(),
    };
    let Ok(text) = std::str::from_utf8(&job.body) else {
        input.body_kind = "base64".to_string();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&job.body);
        input
            .fields
            .push((RAW_BODY_FIELD.to_string(), text_part(encoded)));
        return input;
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => {
            let map = if job.redact { redact_object(map) } else { map };
            input.previous_response_id = map
                .get("previous_response_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let list_field = LIST_FIELDS
                .iter()
                .find(|field| map.get(**field).is_some_and(Value::is_array))
                .map(|field| field.to_string());
            input.body_kind = if list_field.is_some() {
                "json_list".to_string()
            } else {
                "json_object".to_string()
            };
            for (key, value) in map {
                if list_field.as_deref() == Some(key.as_str()) {
                    if let Value::Array(items) = value {
                        input.items = items.iter().map(json_part).collect();
                    }
                } else {
                    input.fields.push((key, json_part(&value)));
                }
            }
            input.list_field = list_field;
        }
        Ok(other) if job.redact => {
            let redacted = redact_sensitive_value("", other);
            input.fields.push((
                RAW_BODY_FIELD.to_string(),
                text_part(serde_json::to_string(&redacted).unwrap_or_default()),
            ));
        }
        _ => {
            input
                .fields
                .push((RAW_BODY_FIELD.to_string(), text_part(text.to_string())));
        }
    }
    input
}

fn json_part(value: &Value) -> RequestLogPayloadPart {
    text_part(serde_json::to_string(value).unwrap_or_else(|_| "null".to_string()))
}

fn text_part(content: String) -> RequestLogPayloadPart {
    let digest = Sha256::digest(content.as_bytes());
    let mut hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    RequestLogPayloadPart { hash, content }
}

/// Redact credential-like JSON keys and keep everything else intact. When the
/// body is not valid UTF-8 JSON the raw text is returned as-is.
pub(crate) fn sanitize_request_payload(body: &[u8]) -> String {
    let Ok(text) = std::str::from_utf8(body) else {
        return "<non-utf8 body omitted>".to_string();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(value) => serde_json::to_string(&redact_sensitive_value("", value))
            .unwrap_or_else(|_| "<unserializable body omitted>".to_string()),
        Err(_) => text.to_string(),
    }
}

fn redact_object(map: Map<String, Value>) -> Map<String, Value> {
    let mut sanitized = Map::new();
    for (key, value) in map {
        let sanitized_value = redact_sensitive_value(&key, value);
        sanitized.insert(key, sanitized_value);
    }
    sanitized
}

fn redact_sensitive_value(key: &str, value: Value) -> Value {
    if payload_key_is_sensitive(key) {
        return Value::String(REDACTED_PLACEHOLDER.to_string());
    }
    match value {
        Value::Object(map) => Value::Object(redact_object(map)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| redact_sensitive_value("", item))
                .collect(),
        ),
        other => other,
    }
}

/// Key names are normalized (lowercase, separators stripped) so variants like
/// `api_key`, `apiKey` and `API-KEY` share one decision.
fn payload_key_is_sensitive(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    matches!(
        normalized.as_str(),
        "authorization"
            | "proxyauthorization"
            | "apikey"
            | "xapikey"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "idtoken"
            | "password"
            | "clientsecret"
            | "secret"
            | "credential"
            | "credentials"
            | "cookie"
            | "privatekey"
            | "sessionkey"
    ) || normalized.ends_with("apikey")
        || normalized.ends_with("secret")
        || normalized.ends_with("password")
        || normalized.ends_with("token")
}

/// Cap the stored preview at `max_bytes` without splitting a multi-byte UTF-8
/// character. Truncated previews may cut JSON structures in half; the detail
/// view renders them as plain text with a truncation marker.
pub(crate) fn truncate_utf8_payload(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    (text[..boundary].to_string(), true)
}

#[cfg(test)]
#[path = "tests/request_log_payload_tests.rs"]
mod tests;
