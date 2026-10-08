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
//! Logging must never affect receiving or forwarding a request. The gateway
//! hot path only builds a job from reference-counted `Bytes`, snapshots the
//! in-memory clear generation and hands the job to the write queue
//! ([`pipeline`]): no database access, no file IO, no storage pool, no
//! blocking, no error returned and no panic propagated. Redaction, splitting
//! and hashing run on a small preprocessing pool, database writes are group
//! committed by one writer thread, and jobs over the memory budget are
//! spilled to disk by a dedicated thread ([`spill`]).

use base64::Engine;
use bytes::Bytes;
use codexmanager_core::request_log_spill::record::SpillOriginalDigests;
use codexmanager_core::storage::{
    now_ts, RequestLogPayloadManifestInput, RequestLogPayloadPart, PAYLOAD_STAGE_CLIENT,
    PAYLOAD_STAGE_UPSTREAM,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[path = "request_log_payload_clear.rs"]
mod clear;
#[path = "request_log_payload_persist.rs"]
mod persist;
#[path = "request_log_payload_pipeline.rs"]
mod pipeline;
#[path = "request_log_payload_spill.rs"]
mod spill;
#[path = "request_log_payload_writer.rs"]
mod writer;

pub(crate) use clear::{
    begin_request_log_payload_clear, finish_request_log_payload_clear,
    guard_request_log_payload_clear,
};
#[cfg(test)]
use persist::{
    persist_request_log_payload, persist_request_log_payload_with_cache, run_payload_writer,
    ParentCache,
};
pub(crate) use pipeline::{
    initialize_request_log_payload_pipeline, request_log_payload_queue_stats,
};

/// Size cap of a stored preview when the preview mode is enabled.
pub(crate) const REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES: usize = 16 * 1024;
const REDACTED_PLACEHOLDER: &str = "[REDACTED]";
const NON_UTF8_PREVIEW_PLACEHOLDER: &str = "<non-utf8 body omitted>";
/// Field name used for bodies that are not a JSON object.
pub(crate) const RAW_BODY_FIELD: &str = "$body";
/// Candidate list fields, in detection order: OpenAI Responses `input`,
/// Chat Completions / Anthropic `messages`, Gemini `contents`.
const LIST_FIELDS: [&str; 3] = ["input", "messages", "contents"];

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
    /// In-memory mirror of the clear generation at capture time (or
    /// [`clear::GENERATION_UNRESOLVED`]); checked against SQLite at commit.
    pub generation: i64,
    /// Clears started in this process at capture time (see [`clear`]).
    pub clear_epoch: u64,
    /// Process run that captured the job.
    pub boot_id: u64,
    pub attempt: Option<OutboundAttemptCapture>,
    /// Set for a job replayed from a spill segment whose body was redacted
    /// before it reached the disk: hashes and sizes of the bytes that were
    /// actually received and sent. `None` on the capture path.
    pub original: Option<SpillOriginalDigests>,
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

/// Hot-path entry: capture a request body for the given stage. Builds the
/// job (reference-counted `Bytes`, no copy) and enqueues it. Never touches
/// the database or the file system, never blocks, never fails the request
/// and never lets a panic escape.
pub(crate) fn store_request_log_payload(
    trace_id: &str,
    stage: &str,
    body: &Bytes,
    conversation_key: Option<String>,
    attempt: Option<OutboundAttemptCapture>,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let trace_id = trace_id.trim();
        if trace_id.is_empty() || crate::storage_helpers::seaorm_enabled() {
            return;
        }
        let job = RequestLogPayloadJob {
            trace_id: trace_id.to_string(),
            generation: clear::GENERATION_UNRESOLVED,
            clear_epoch: 0,
            boot_id: 0,
            stage: stage.to_string(),
            body: body.clone(),
            conversation_key,
            redact: request_log_payload_redaction_enabled(),
            preview: request_log_payload_preview_enabled(),
            created_at: now_ts(),
            attempt,
            original: None,
        };
        dispatch_request_log_payload_job(job);
    }));
}

#[cfg(not(test))]
fn dispatch_request_log_payload_job(job: RequestLogPayloadJob) {
    match pipeline::global_pipeline() {
        Some(pipeline) => pipeline.submit(job),
        None => pipeline::count_unstarted_drop(),
    }
}

#[cfg(test)]
thread_local! {
    static TEST_PIPELINE: std::cell::RefCell<Option<std::sync::Arc<pipeline::PipelineShared>>> =
        const { std::cell::RefCell::new(None) };
}

/// Route this thread's captures to `pipeline` instead of the synchronous
/// test writer.
#[cfg(test)]
pub(crate) fn set_test_pipeline(pipeline: Option<std::sync::Arc<pipeline::PipelineShared>>) {
    TEST_PIPELINE.with(|slot| *slot.borrow_mut() = pipeline);
}

/// Unit tests persist synchronously (existing tests read the payload right
/// after the request) unless a test pipeline is installed on this thread.
#[cfg(test)]
fn dispatch_request_log_payload_job(mut job: RequestLogPayloadJob) {
    if let Some(pipeline) = TEST_PIPELINE.with(|slot| slot.borrow().clone()) {
        pipeline.submit(job);
        return;
    }
    let Some(storage) = crate::storage_helpers::open_storage() else {
        return;
    };
    match storage.request_log_payload_generation() {
        Ok(generation) => job.generation = generation,
        Err(_) => return,
    }
    persist_request_log_payload(&storage, job);
}

/// Capture the body exactly as received from the client. Called for every
/// authenticated gateway request so that logs which never reach the upstream
/// (validation rejects, local responses, aggregate-API failures) still have
/// content to show.
pub(crate) fn store_client_request_log_payload(
    trace_id: &str,
    body: &Bytes,
    conversation_key: Option<String>,
) {
    store_request_log_payload(trace_id, PAYLOAD_STAGE_CLIENT, body, conversation_key, None);
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
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if scope.trace_id.trim().is_empty() || crate::storage_helpers::seaorm_enabled() {
            return;
        }
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
    }));
}

fn bytes_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hash = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hash.push_str(&format!("{byte:02x}"));
    }
    hash
}

fn preview_payload_text(body: &[u8], redact: bool) -> String {
    let Ok(text) = std::str::from_utf8(body) else {
        return NON_UTF8_PREVIEW_PLACEHOLDER.to_string();
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

/// Body written to a spill segment for a job that asks for redaction. It
/// never holds more than the database would store, and preprocessing it
/// again (preview or split, with redaction) gives exactly the rows the
/// original body gives:
/// * JSON: the redacted JSON (redaction is idempotent);
/// * other UTF-8 text: unchanged, as the database stores it;
/// * not UTF-8: the preview placeholder in preview mode, the raw bytes in
///   full mode (stored base64 encoded there).
///
/// The caller keeps the hashes and size of the original body separately
/// and never spills the wire body.
pub(crate) fn redacted_spill_body(job: &RequestLogPayloadJob) -> Bytes {
    let Ok(text) = std::str::from_utf8(&job.body) else {
        return if job.preview {
            Bytes::from_static(NON_UTF8_PREVIEW_PLACEHOLDER.as_bytes())
        } else {
            job.body.clone()
        };
    };
    match serde_json::from_str::<Value>(text) {
        Ok(value) => match serde_json::to_vec(&redact_sensitive_value("", value)) {
            Ok(redacted) => Bytes::from(redacted),
            Err(_) => Bytes::from_static(b"null"),
        },
        Err(_) => job.body.clone(),
    }
}

/// Digests of `job`'s original body and wire bytes, kept next to a
/// [`redacted_spill_body`].
pub(crate) fn original_digests(job: &RequestLogPayloadJob) -> SpillOriginalDigests {
    SpillOriginalDigests {
        body_sha256: bytes_hash(&job.body),
        body_len: job.body.len() as u64,
        wire_sha256: job
            .attempt
            .as_ref()
            .map(|attempt| bytes_hash(&attempt.wire_body)),
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
