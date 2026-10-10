//! Preprocessing (CPU only) and database persistence of captured request
//! payloads. The same code path serves the group-committing writer
//! ([`RequestLogPayloadBatch`]) and single-job writes on [`Storage`].

use super::{
    bytes_hash, preview_payload_text, split_request_payload, truncate_utf8_payload,
    RequestLogPayloadJob, REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES,
};
use codexmanager_core::storage::{
    RequestLogPayload, RequestLogPayloadBatch, RequestLogPayloadManifest,
    RequestLogPayloadManifestInput, RequestLogPayloadManifestWrite, RequestLogPayloadParentHint,
    RequestLogUpstreamAttempt, Storage, PAYLOAD_STAGE_CLIENT, PAYLOAD_STAGE_UPSTREAM,
};
use std::collections::{HashMap, VecDeque};

const PARENT_CACHE_CAPACITY: usize = 256;

/// Database operations needed to persist one prepared job. Implemented by
/// [`Storage`] (one transaction per call) and by [`RequestLogPayloadBatch`]
/// (group commit: everything inside the batch transaction).
pub(crate) trait PayloadSink {
    fn preview_body_hash(&self, trace_id: &str, stage: &str) -> rusqlite::Result<Option<String>>;
    fn manifest(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> rusqlite::Result<Option<RequestLogPayloadManifest>>;
    fn insert_preview_if_current(
        &self,
        payload: &RequestLogPayload,
        generation: i64,
    ) -> rusqlite::Result<bool>;
    fn insert_manifest_if_current(
        &self,
        input: &RequestLogPayloadManifestInput,
        parent_hint: Option<&RequestLogPayloadParentHint>,
        generation: i64,
    ) -> rusqlite::Result<RequestLogPayloadManifestWrite>;
    fn record_attempt_if_current(
        &self,
        attempt: &RequestLogUpstreamAttempt,
        generation: i64,
    ) -> rusqlite::Result<bool>;
}

impl PayloadSink for Storage {
    fn preview_body_hash(&self, trace_id: &str, stage: &str) -> rusqlite::Result<Option<String>> {
        self.find_request_log_payload_body_hash(trace_id, stage)
    }
    fn manifest(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> rusqlite::Result<Option<RequestLogPayloadManifest>> {
        self.find_request_log_payload_manifest(trace_id, stage)
    }
    fn insert_preview_if_current(
        &self,
        payload: &RequestLogPayload,
        generation: i64,
    ) -> rusqlite::Result<bool> {
        self.insert_request_log_payload_if_current(payload, generation)
    }
    fn insert_manifest_if_current(
        &self,
        input: &RequestLogPayloadManifestInput,
        parent_hint: Option<&RequestLogPayloadParentHint>,
        generation: i64,
    ) -> rusqlite::Result<RequestLogPayloadManifestWrite> {
        self.insert_request_log_payload_manifest_if_current(input, parent_hint, generation)
    }
    fn record_attempt_if_current(
        &self,
        attempt: &RequestLogUpstreamAttempt,
        generation: i64,
    ) -> rusqlite::Result<bool> {
        self.record_request_log_upstream_attempt_if_current(attempt, generation)
    }
}

impl PayloadSink for RequestLogPayloadBatch<'_> {
    fn preview_body_hash(&self, trace_id: &str, stage: &str) -> rusqlite::Result<Option<String>> {
        self.find_request_log_payload_body_hash(trace_id, stage)
    }
    fn manifest(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> rusqlite::Result<Option<RequestLogPayloadManifest>> {
        self.find_request_log_payload_manifest(trace_id, stage)
    }
    fn insert_preview_if_current(
        &self,
        payload: &RequestLogPayload,
        generation: i64,
    ) -> rusqlite::Result<bool> {
        self.insert_request_log_payload_if_current(payload, generation)
    }
    fn insert_manifest_if_current(
        &self,
        input: &RequestLogPayloadManifestInput,
        parent_hint: Option<&RequestLogPayloadParentHint>,
        generation: i64,
    ) -> rusqlite::Result<RequestLogPayloadManifestWrite> {
        self.insert_request_log_payload_manifest_if_current(input, parent_hint, generation)
    }
    fn record_attempt_if_current(
        &self,
        attempt: &RequestLogUpstreamAttempt,
        generation: i64,
    ) -> rusqlite::Result<bool> {
        self.record_request_log_upstream_attempt_if_current(attempt, generation)
    }
}

/// CPU-only result of preprocessing one job (redaction, 16 KB truncation,
/// splitting and hashing). Holds no reference to the original body.
pub(crate) struct PreparedPayload {
    pub trace_id: String,
    pub stage: String,
    pub created_at: i64,
    pub generation: i64,
    pub clear_epoch: u64,
    pub boot_id: u64,
    pub has_attempt: bool,
    pub body: PreparedBody,
    /// Attempt metadata with `identical_to_client` decided at write time.
    pub attempt: Option<RequestLogUpstreamAttempt>,
}

pub(crate) enum PreparedBody {
    Preview(RequestLogPayload),
    Manifest(RequestLogPayloadManifestInput),
}

/// Outcome of one write: `inserted` is false for stale or duplicate jobs.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PayloadWriteResult {
    pub inserted: bool,
}

/// Pure CPU work, safe to run on any thread.
pub(crate) fn prepare_request_log_payload(job: &RequestLogPayloadJob) -> PreparedPayload {
    // A job replayed from a redacted spill record describes the original
    // bytes through its digests; its body is the redacted copy.
    let (body_hash, body_len) = match job.original.as_ref() {
        Some(original) => (original.body_sha256.clone(), original.body_len as i64),
        None => (bytes_hash(&job.body), job.body.len() as i64),
    };
    let body = if job.preview {
        let text = preview_payload_text(&job.body, job.redact);
        let (payload, truncated) =
            truncate_utf8_payload(&text, REQUEST_LOG_PAYLOAD_PREVIEW_MAX_BYTES);
        PreparedBody::Preview(RequestLogPayload {
            trace_id: job.trace_id.clone(),
            stage: job.stage.clone(),
            payload,
            payload_bytes: body_len,
            payload_truncated: truncated,
            redacted: job.redact,
            body_hash,
            created_at: job.created_at,
        })
    } else {
        let mut input = split_request_payload(job);
        input.body_hash = body_hash;
        input.payload_bytes = body_len;
        PreparedBody::Manifest(input)
    };
    let attempt = job
        .attempt
        .as_ref()
        .map(|attempt| RequestLogUpstreamAttempt {
            trace_id: job.trace_id.clone(),
            stage: job.stage.clone(),
            method: attempt.method.clone(),
            url: attempt.url.clone(),
            transport: attempt.transport.clone(),
            content_encoding: attempt.content_encoding.clone(),
            wire_sha256: match job.original.as_ref() {
                Some(original) => original
                    .wire_sha256
                    .clone()
                    .unwrap_or_else(|| original.body_sha256.clone()),
                None => bytes_hash(&attempt.wire_body),
            },
            identical_to_client: false,
            created_at: job.created_at,
        });
    PreparedPayload {
        trace_id: job.trace_id.clone(),
        stage: job.stage.clone(),
        created_at: job.created_at,
        generation: job.generation,
        clear_epoch: job.clear_epoch,
        boot_id: job.boot_id,
        has_attempt: job.attempt.is_some(),
        body,
        attempt,
    }
}

/// Persist one prepared job. Errors of the main insert are returned (the
/// group commit rolls the job back); attempt metadata stays best-effort.
pub(crate) fn write_prepared_payload<S: PayloadSink + ?Sized>(
    sink: &S,
    prepared: &PreparedPayload,
    generation: i64,
    cache: &mut ParentCache,
) -> rusqlite::Result<PayloadWriteResult> {
    match &prepared.body {
        PreparedBody::Preview(record) => {
            let upstream_matches_client = prepared.stage.starts_with(PAYLOAD_STAGE_UPSTREAM)
                && !record.body_hash.is_empty()
                && matches!(
                    sink.preview_body_hash(&prepared.trace_id, PAYLOAD_STAGE_CLIENT),
                    Ok(Some(existing)) if existing == record.body_hash
                );
            if upstream_matches_client {
                persist_attempt_metadata(sink, prepared, generation, true);
                return Ok(PayloadWriteResult { inserted: true });
            }
            let inserted = sink.insert_preview_if_current(record, generation)?;
            if inserted {
                persist_attempt_metadata(sink, prepared, generation, false);
            }
            Ok(PayloadWriteResult { inserted })
        }
        PreparedBody::Manifest(input) => {
            let mut input_owned;
            let mut input = input;
            if prepared.stage.starts_with(PAYLOAD_STAGE_UPSTREAM) {
                let conversation_key = match sink.manifest(&prepared.trace_id, PAYLOAD_STAGE_CLIENT)
                {
                    Ok(Some(client)) => Some(client.conversation_key),
                    Ok(None) if prepared.has_attempt => Some(None),
                    Ok(None) => None,
                    Err(err) => {
                        log::warn!(
                            "event=request_log_payload_client_context_read_failed trace_id={} err={err}",
                            prepared.trace_id
                        );
                        Some(None)
                    }
                };
                if let Some(conversation_key) = conversation_key {
                    input_owned = input.clone();
                    input_owned.conversation_key = conversation_key;
                    input = &input_owned;
                }
            }
            let cache_key = input
                .conversation_key
                .as_ref()
                .zip(input.list_field.as_ref())
                .map(|(conversation, field)| format!("{conversation}#{field}#{}", prepared.stage));
            let hint = cache_key.as_deref().and_then(|key| cache.get(key)).cloned();
            let write = sink.insert_manifest_if_current(input, hint.as_ref(), generation)?;
            if write.inserted || write.identical_to_client {
                persist_attempt_metadata(sink, prepared, generation, write.identical_to_client);
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
            Ok(PayloadWriteResult {
                inserted: write.inserted || write.identical_to_client,
            })
        }
    }
}

/// Most recent manifest per conversation with its resolved item ids, so a
/// follow-up request can compute its shared prefix without re-walking the
/// parent chain in the database. Hints are validated before use, so stale
/// entries (rolled back batch, retention prune) are harmless.
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

/// Prepare and persist one job with its captured generation (one
/// transaction per database call).
#[cfg(test)]
pub(crate) fn persist_request_log_payload_with_cache(
    storage: &Storage,
    job: RequestLogPayloadJob,
    cache: &mut ParentCache,
) {
    let prepared = prepare_request_log_payload(&job);
    if let Err(err) = write_prepared_payload(storage, &prepared, job.generation, cache) {
        log::warn!(
            "event=request_log_payload_insert_failed trace_id={} err={}",
            job.trace_id,
            err
        );
    }
}

/// Writer loop over a plain channel (one group commit per job), kept for the
/// paused-writer regression test.
#[cfg(test)]
pub(super) fn run_payload_writer<F, S, H>(
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
            continue;
        };
        let prepared = vec![prepare_request_log_payload(&job)];
        let _ = super::writer::write_prepared_batch(
            &storage,
            prepared,
            &mut cache,
            super::clear::boot_id(),
            super::clear::global_clear_state(),
            &|| false,
        );
    }
}

fn persist_attempt_metadata<S: PayloadSink + ?Sized>(
    sink: &S,
    prepared: &PreparedPayload,
    generation: i64,
    identical_to_client: bool,
) {
    let Some(attempt) = prepared.attempt.as_ref() else {
        return;
    };
    let mut record = attempt.clone();
    record.identical_to_client = identical_to_client;
    if let Err(err) = sink.record_attempt_if_current(&record, generation) {
        log::warn!(
            "event=request_log_attempt_insert_failed trace_id={} err={err}",
            prepared.trace_id
        );
    }
}
