//! Full request payload storage with content-addressed de-duplication.
//!
//! A request body is split by the service layer into top-level fields and
//! (optionally) one list field (`input` / `messages` / `contents`). Every
//! fragment is stored once in `request_log_payload_blobs`, keyed by its hash.
//! Each trace gets a manifest; when an earlier request of the same
//! conversation shares a prefix of list items, the manifest only records the
//! parent trace, the shared prefix length and the new tail items.

use rusqlite::{types::Value, OptionalExtension, Result};
use std::collections::HashMap;

use super::{Storage, PAYLOAD_STAGE_CLIENT, PAYLOAD_STAGE_UPSTREAM};

const BLOB_INSERT_CHUNK: usize = 200;
const ITEM_INSERT_CHUNK: usize = 300;
const ID_LOOKUP_CHUNK: usize = 400;
const MAX_PARENT_CHAIN_DEPTH: usize = 100_000;
/// Manifests a clear or retention purge is deleting in batches must never
/// become the parent of a new manifest, nor be shown to readers.
fn live_manifest_sql(alias: &str) -> String {
    super::request_log_payload_purge::live_payload_row_sql("request_log_payload_manifests", alias)
}

fn live_attempt_sql(alias: &str) -> String {
    super::request_log_payload_purge::live_payload_row_sql("request_log_upstream_attempts", alias)
}

/// Transport metadata of an actual outbound request attempt. The URL must
/// be stripped of credentials, query and fragment before calling Storage.
#[derive(Debug, Clone, Default)]
pub struct RequestLogUpstreamAttempt {
    pub trace_id: String,
    pub stage: String,
    pub method: String,
    pub url: String,
    pub transport: String,
    pub content_encoding: Option<String>,
    pub wire_sha256: String,
    pub identical_to_client: bool,
    pub created_at: i64,
}

/// One content-addressed fragment of a request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLogPayloadPart {
    pub hash: String,
    pub content: String,
}

/// Split request body ready to be persisted for one trace.
#[derive(Debug, Clone, Default)]
pub struct RequestLogPayloadManifestInput {
    pub trace_id: String,
    /// `client` or `upstream` capture stage.
    pub stage: String,
    /// SHA-256 of the captured body, used to skip redundant captures.
    pub body_hash: String,
    /// `json_object` | `json_list` | `text` | `base64`
    pub body_kind: String,
    pub list_field: Option<String>,
    pub conversation_key: Option<String>,
    pub previous_response_id: Option<String>,
    pub payload_bytes: i64,
    pub redacted: bool,
    pub created_at: i64,
    pub fields: Vec<(String, RequestLogPayloadPart)>,
    pub items: Vec<RequestLogPayloadPart>,
}

/// Cached parent candidate (latest manifest of the same conversation) and
/// its fully resolved list item blob ids.
#[derive(Debug, Clone, Default)]
pub struct RequestLogPayloadParentHint {
    pub trace_id: String,
    pub stage: String,
    pub item_blob_ids: Vec<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct RequestLogPayloadManifestWrite {
    pub inserted: bool,
    /// Set when the upstream capture matched the stored client body hash and
    /// was therefore not duplicated.
    pub identical_to_client: bool,
    pub parent_trace_id: Option<String>,
    pub shared_prefix_len: usize,
    pub item_blob_ids: Vec<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct RequestLogPayloadManifest {
    pub trace_id: String,
    pub stage: String,
    pub body_kind: String,
    pub list_field: Option<String>,
    pub conversation_key: Option<String>,
    pub parent_trace_id: Option<String>,
    pub shared_prefix_len: i64,
    pub item_count: i64,
    pub previous_response_id: Option<String>,
    pub payload_bytes: i64,
    pub redacted: bool,
    pub body_hash: String,
    pub created_at: i64,
}

/// Rebuilt request body: top-level fields plus the resolved list items.
/// `complete` is false when part of the parent chain is missing.
#[derive(Debug, Clone, Default)]
pub struct RequestLogPayloadFull {
    pub manifest: RequestLogPayloadManifest,
    pub fields: Vec<(String, String)>,
    pub items: Vec<String>,
    pub complete: bool,
}

const MANIFEST_COLUMNS: &str =
    "trace_id, stage, body_kind, list_field, conversation_key, parent_trace_id, \
     shared_prefix_len, item_count, previous_response_id, payload_bytes, redacted, \
     body_hash, created_at";

fn manifest_from_row(row: &rusqlite::Row<'_>) -> Result<RequestLogPayloadManifest> {
    Ok(RequestLogPayloadManifest {
        trace_id: row.get(0)?,
        stage: row.get(1)?,
        body_kind: row.get(2)?,
        list_field: row.get(3)?,
        conversation_key: row.get(4)?,
        parent_trace_id: row.get(5)?,
        shared_prefix_len: row.get(6)?,
        item_count: row.get(7)?,
        previous_response_id: row.get(8)?,
        payload_bytes: row.get(9)?,
        redacted: row.get(10)?,
        body_hash: row.get(11)?,
        created_at: row.get(12)?,
    })
}

fn common_prefix_len(left: &[i64], right: &[i64]) -> usize {
    left.iter()
        .zip(right.iter())
        .take_while(|(a, b)| a == b)
        .count()
}

fn placeholders(count: usize, group: &str) -> String {
    vec![group; count].join(",")
}

impl Storage {
    fn has_request_log_payload_store(&self) -> Result<bool> {
        self.has_table("request_log_payload_manifests")
    }

    /// Snapshot the clear generation before enqueueing an asynchronous job.
    pub fn request_log_payload_generation(&self) -> Result<i64> {
        if !self.has_table("request_log_payload_state")? {
            return Ok(0);
        }
        self.conn.query_row(
            "SELECT generation FROM request_log_payload_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
    }

    pub(super) fn request_log_payload_job_is_current(
        &self,
        generation: i64,
        created_at: i64,
    ) -> Result<bool> {
        if !self.has_table("request_log_payload_state")? {
            return Ok(generation == 0);
        }
        self.conn.query_row(
            "SELECT generation = ?1 AND retention_cutoff <= ?2
             FROM request_log_payload_state WHERE id = 1",
            (generation, created_at),
            |row| row.get(0),
        )
    }

    /// Persist a split request body. Re-inserting an existing trace is a
    /// no-op (`inserted == false`).
    pub fn insert_request_log_payload_manifest(
        &self,
        input: &RequestLogPayloadManifestInput,
        parent_hint: Option<&RequestLogPayloadParentHint>,
    ) -> Result<RequestLogPayloadManifestWrite> {
        self.insert_request_log_payload_manifest_guarded(input, parent_hint, None)
    }

    pub fn insert_request_log_payload_manifest_if_current(
        &self,
        input: &RequestLogPayloadManifestInput,
        parent_hint: Option<&RequestLogPayloadParentHint>,
        generation: i64,
    ) -> Result<RequestLogPayloadManifestWrite> {
        self.insert_request_log_payload_manifest_guarded(input, parent_hint, Some(generation))
    }

    fn insert_request_log_payload_manifest_guarded(
        &self,
        input: &RequestLogPayloadManifestInput,
        parent_hint: Option<&RequestLogPayloadParentHint>,
        generation: Option<i64>,
    ) -> Result<RequestLogPayloadManifestWrite> {
        if !self.has_request_log_payload_store()? {
            return Ok(RequestLogPayloadManifestWrite::default());
        }
        // The upstream capture is redundant when the gateway forwarded the
        // client body unchanged.
        if input.stage.starts_with(PAYLOAD_STAGE_UPSTREAM) && !input.body_hash.is_empty() {
            let client_hash = self.find_request_log_payload_manifest_body_hash(
                &input.trace_id,
                PAYLOAD_STAGE_CLIENT,
            )?;
            if client_hash.as_deref() == Some(input.body_hash.as_str()) {
                return Ok(RequestLogPayloadManifestWrite {
                    identical_to_client: true,
                    ..Default::default()
                });
            }
        }
        let tx = self.conn.unchecked_transaction()?;
        if let Some(generation) = generation {
            if !self.request_log_payload_job_is_current(generation, input.created_at)? {
                return Ok(RequestLogPayloadManifestWrite::default());
            }
        }
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM request_log_payload_manifests WHERE trace_id = ?1 AND stage = ?2",
                (input.trace_id.as_str(), input.stage.as_str()),
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            tx.commit()?;
            return Ok(RequestLogPayloadManifestWrite::default());
        }

        let parts = input
            .fields
            .iter()
            .map(|(_, part)| part)
            .chain(input.items.iter())
            .collect::<Vec<_>>();
        let ids_by_hash = self.upsert_request_log_payload_blobs(&parts, input.created_at)?;
        let blob_id = |part: &RequestLogPayloadPart| -> Result<i64> {
            ids_by_hash.get(part.hash.as_str()).copied().ok_or_else(|| {
                rusqlite::Error::SqliteFailure((), Some("payload blob id missing".to_string()))
            })
        };
        let item_blob_ids = input
            .items
            .iter()
            .map(blob_id)
            .collect::<Result<Vec<_>>>()?;

        let (parent_trace_id, shared_prefix_len) =
            self.select_request_log_payload_parent(input, &item_blob_ids, parent_hint)?;

        self.conn.execute(
            "INSERT INTO request_log_payload_manifests (
                trace_id, stage, body_kind, list_field, conversation_key, parent_trace_id,
                shared_prefix_len, item_count, previous_response_id, payload_bytes,
                redacted, body_hash, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            (
                input.trace_id.as_str(),
                input.stage.as_str(),
                input.body_kind.as_str(),
                input.list_field.as_deref(),
                input.conversation_key.as_deref(),
                parent_trace_id.as_deref(),
                shared_prefix_len as i64,
                item_blob_ids.len() as i64,
                input.previous_response_id.as_deref(),
                input.payload_bytes,
                input.redacted,
                input.body_hash.as_str(),
                input.created_at,
            ),
        )?;

        for (chunk_index, chunk) in input.fields.chunks(ITEM_INSERT_CHUNK).enumerate() {
            let mut params = Vec::with_capacity(chunk.len() * 5);
            for (offset, (name, part)) in chunk.iter().enumerate() {
                params.push(Value::Text(input.trace_id.clone()));
                params.push(Value::Text(input.stage.clone()));
                params.push(Value::Integer(
                    (chunk_index * ITEM_INSERT_CHUNK + offset) as i64,
                ));
                params.push(Value::Text(name.clone()));
                params.push(Value::Integer(blob_id(part)?));
            }
            self.conn.execute(
                &format!(
                    "INSERT INTO request_log_payload_manifest_fields
                        (trace_id, stage, position, field_name, blob_id) VALUES {}",
                    placeholders(chunk.len(), "(?, ?, ?, ?, ?)")
                ),
                params,
            )?;
        }

        self.insert_request_log_payload_items(
            input.trace_id.as_str(),
            input.stage.as_str(),
            shared_prefix_len,
            &item_blob_ids[shared_prefix_len..],
        )?;
        tx.commit()?;
        Ok(RequestLogPayloadManifestWrite {
            inserted: true,
            identical_to_client: false,
            parent_trace_id,
            shared_prefix_len,
            item_blob_ids,
        })
    }

    fn insert_request_log_payload_items(
        &self,
        trace_id: &str,
        stage: &str,
        start_position: usize,
        blob_ids: &[i64],
    ) -> Result<()> {
        for (chunk_index, chunk) in blob_ids.chunks(ITEM_INSERT_CHUNK).enumerate() {
            let mut params = Vec::with_capacity(chunk.len() * 4);
            for (offset, blob_id) in chunk.iter().enumerate() {
                params.push(Value::Text(trace_id.to_string()));
                params.push(Value::Text(stage.to_string()));
                params.push(Value::Integer(
                    (start_position + chunk_index * ITEM_INSERT_CHUNK + offset) as i64,
                ));
                params.push(Value::Integer(*blob_id));
            }
            self.conn.execute(
                &format!(
                    "INSERT INTO request_log_payload_manifest_items
                        (trace_id, stage, position, blob_id) VALUES {}",
                    placeholders(chunk.len(), "(?, ?, ?, ?)")
                ),
                params,
            )?;
        }
        Ok(())
    }

    fn upsert_request_log_payload_blobs(
        &self,
        parts: &[&RequestLogPayloadPart],
        created_at: i64,
    ) -> Result<HashMap<String, i64>> {
        let mut unique: Vec<&RequestLogPayloadPart> = Vec::with_capacity(parts.len());
        let mut seen = std::collections::HashSet::with_capacity(parts.len());
        for part in parts {
            if seen.insert(part.hash.as_str()) {
                unique.push(part);
            }
        }
        for chunk in unique.chunks(BLOB_INSERT_CHUNK) {
            let mut params = Vec::with_capacity(chunk.len() * 4);
            for part in chunk {
                params.push(Value::Text(part.hash.clone()));
                params.push(Value::Text(part.content.clone()));
                params.push(Value::Integer(part.content.len() as i64));
                params.push(Value::Integer(created_at));
            }
            self.conn.execute(
                &format!(
                    "INSERT OR IGNORE INTO request_log_payload_blobs
                        (hash, content, content_bytes, created_at) VALUES {}",
                    placeholders(chunk.len(), "(?, ?, ?, ?)")
                ),
                params,
            )?;
        }
        let mut ids = HashMap::with_capacity(unique.len());
        for chunk in unique.chunks(ID_LOOKUP_CHUNK) {
            let params = chunk
                .iter()
                .map(|part| Value::Text(part.hash.clone()))
                .collect::<Vec<_>>();
            let mut stmt = self.conn.prepare(&format!(
                "SELECT id, hash FROM request_log_payload_blobs WHERE hash IN ({})",
                placeholders(chunk.len(), "?")
            ))?;
            let rows = stmt.query_map(params, |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, hash) = row?;
                ids.insert(hash, id);
            }
        }
        Ok(ids)
    }

    fn select_request_log_payload_parent(
        &self,
        input: &RequestLogPayloadManifestInput,
        item_blob_ids: &[i64],
        parent_hint: Option<&RequestLogPayloadParentHint>,
    ) -> Result<(Option<String>, usize)> {
        let Some(list_field) = input.list_field.as_deref() else {
            return Ok((None, 0));
        };
        if item_blob_ids.is_empty() {
            return Ok((None, 0));
        }
        if let Some(hint) =
            parent_hint.filter(|hint| hint.trace_id != input.trace_id && hint.stage == input.stage)
        {
            let hint_alive = self
                .conn
                .query_row(
                    &format!(
                        "SELECT 1 FROM request_log_payload_manifests m
                         WHERE m.trace_id = ?1 AND m.stage = ?2 AND m.list_field = ?3
                           AND {}",
                        live_manifest_sql("m")
                    ),
                    (hint.trace_id.as_str(), input.stage.as_str(), list_field),
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if hint_alive {
                let shared = common_prefix_len(&hint.item_blob_ids, item_blob_ids);
                return Ok(if shared > 0 {
                    (Some(hint.trace_id.clone()), shared)
                } else {
                    (None, 0)
                });
            }
        }
        let Some(conversation_key) = input.conversation_key.as_deref() else {
            return Ok((None, 0));
        };
        let candidate = self
            .conn
            .query_row(
                &format!(
                    "SELECT m.trace_id, m.item_count FROM request_log_payload_manifests m
                     WHERE m.conversation_key = ?1 AND m.stage = ?2 AND m.list_field = ?3
                       AND m.trace_id <> ?4 AND {}
                     ORDER BY m.created_at DESC, m.rowid DESC LIMIT 1",
                    live_manifest_sql("m")
                ),
                (
                    conversation_key,
                    input.stage.as_str(),
                    list_field,
                    input.trace_id.as_str(),
                ),
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((candidate_trace_id, candidate_count)) = candidate else {
            return Ok((None, 0));
        };
        let Some(candidate_ids) = self.resolve_request_log_payload_item_ids(
            &candidate_trace_id,
            input.stage.as_str(),
            candidate_count,
        )?
        else {
            return Ok((None, 0));
        };
        let shared = common_prefix_len(&candidate_ids, item_blob_ids);
        Ok(if shared > 0 {
            (Some(candidate_trace_id), shared)
        } else {
            (None, 0)
        })
    }

    /// Resolve the full ordered list of item blob ids for a manifest by
    /// walking its parent chain. Returns `None` when the chain is broken.
    pub fn resolve_request_log_payload_item_ids(
        &self,
        trace_id: &str,
        stage: &str,
        item_count: i64,
    ) -> Result<Option<Vec<i64>>> {
        let total = item_count.max(0) as usize;
        let mut resolved = vec![0_i64; total];
        let mut current = trace_id.to_string();
        let mut need = total;
        let mut depth = 0_usize;
        while need > 0 {
            depth += 1;
            if depth > MAX_PARENT_CHAIN_DEPTH {
                return Ok(None);
            }
            let meta = self
                .conn
                .query_row(
                    "SELECT parent_trace_id, shared_prefix_len
                     FROM request_log_payload_manifests WHERE trace_id = ?1 AND stage = ?2",
                    (current.as_str(), stage),
                    |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let Some((parent, shared_prefix_len)) = meta else {
                return Ok(None);
            };
            let shared = (shared_prefix_len.max(0) as usize).min(need);
            if need > shared {
                let mut stmt = self.conn.prepare(
                    "SELECT position, blob_id FROM request_log_payload_manifest_items
                     WHERE trace_id = ?1 AND stage = ?2 AND position >= ?3 AND position < ?4",
                )?;
                let rows = stmt.query_map(
                    (current.as_str(), stage, shared as i64, need as i64),
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )?;
                for row in rows {
                    let (position, blob_id) = row?;
                    if let Some(slot) = usize::try_from(position)
                        .ok()
                        .and_then(|position| resolved.get_mut(position))
                    {
                        *slot = blob_id;
                    }
                }
            }
            match parent {
                Some(parent) if shared > 0 => {
                    need = shared;
                    current = parent;
                }
                _ => break,
            }
        }
        if resolved.iter().any(|blob_id| *blob_id <= 0) {
            return Ok(None);
        }
        Ok(Some(resolved))
    }

    pub fn find_request_log_payload_manifest(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<RequestLogPayloadManifest>> {
        if !self.has_request_log_payload_store()? {
            return Ok(None);
        }
        self.conn
            .query_row(
                &format!(
                    "SELECT {MANIFEST_COLUMNS} FROM request_log_payload_manifests m
                     WHERE trace_id = ?1 AND stage = ?2 AND {}",
                    live_manifest_sql("m")
                ),
                (trace_id, stage),
                manifest_from_row,
            )
            .optional()
    }

    /// Record one actual outbound attempt after its capture job was accepted.
    /// The same transaction protects against clear/prune racing with the
    /// metadata write. A prior client capture is sufficient when both bodies
    /// were identical and the upstream stage did not need its own payload.
    pub fn record_request_log_upstream_attempt_if_current(
        &self,
        attempt: &RequestLogUpstreamAttempt,
        generation: i64,
    ) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        if !self.request_log_payload_job_is_current(generation, attempt.created_at)? {
            return Ok(false);
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO request_log_upstream_attempts
                 (trace_id, stage, method, url, transport, content_encoding,
                  wire_sha256, identical_to_client, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            (
                attempt.trace_id.as_str(),
                attempt.stage.as_str(),
                attempt.method.as_str(),
                attempt.url.as_str(),
                attempt.transport.as_str(),
                attempt.content_encoding.as_deref(),
                attempt.wire_sha256.as_str(),
                attempt.identical_to_client,
                attempt.created_at,
            ),
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn list_request_log_upstream_attempt_stages(&self, trace_id: &str) -> Result<Vec<String>> {
        if !self.has_table("request_log_upstream_attempts")? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(&format!(
            "SELECT a.stage FROM request_log_upstream_attempts a
             WHERE a.trace_id = ?1 AND {} ORDER BY a.stage",
            live_attempt_sql("a")
        ))?;
        stmt.query_map([trace_id], |row| row.get::<_, String>(0))?
            .collect()
    }

    pub fn find_request_log_upstream_attempt(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<RequestLogUpstreamAttempt>> {
        if !self.has_table("request_log_upstream_attempts")? {
            return Ok(None);
        }
        self.conn
            .query_row(
                &format!(
                    "SELECT trace_id, stage, method, url, transport, content_encoding,
                        wire_sha256, identical_to_client, created_at
                     FROM request_log_upstream_attempts a
                     WHERE trace_id = ?1 AND stage = ?2 AND {}",
                    live_attempt_sql("a")
                ),
                (trace_id, stage),
                |row| {
                    Ok(RequestLogUpstreamAttempt {
                        trace_id: row.get(0)?,
                        stage: row.get(1)?,
                        method: row.get(2)?,
                        url: row.get(3)?,
                        transport: row.get(4)?,
                        content_encoding: row.get(5)?,
                        wire_sha256: row.get(6)?,
                        identical_to_client: row.get(7)?,
                        created_at: row.get(8)?,
                    })
                },
            )
            .optional()
    }

    /// Capture stages with a stored manifest for this trace (alphabetical).
    pub fn list_request_log_payload_manifest_stages(&self, trace_id: &str) -> Result<Vec<String>> {
        if !self.has_request_log_payload_store()? {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(&format!(
            "SELECT m.stage FROM request_log_payload_manifests m
             WHERE m.trace_id = ?1 AND {} ORDER BY m.stage ASC",
            live_manifest_sql("m")
        ))?;
        let rows = stmt.query_map([trace_id], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    /// Stored body hash of one stage, used to skip a redundant upstream
    /// capture when the gateway forwarded the client body unchanged.
    pub fn find_request_log_payload_manifest_body_hash(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<String>> {
        if !self.has_request_log_payload_store()? {
            return Ok(None);
        }
        self.conn
            .query_row(
                &format!(
                    "SELECT m.body_hash FROM request_log_payload_manifests m
                     WHERE m.trace_id = ?1 AND m.stage = ?2 AND {}",
                    live_manifest_sql("m")
                ),
                (trace_id, stage),
                |row| row.get::<_, String>(0),
            )
            .optional()
    }

    /// Associate a real, completed upstream Responses ID with its request.
    /// Only a log row already owned by this key can be linked. Never replace
    /// an existing association: ambiguous or reused IDs must not leak data.
    pub fn record_request_log_response_id(
        &self,
        key_id: &str,
        response_id: &str,
        trace_id: &str,
    ) -> Result<()> {
        if !self.has_table("request_log_response_links")?
            || !response_id.starts_with("resp_")
            || response_id == "resp_proxy"
        {
            return Ok(());
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO request_log_response_links (key_id, response_id, trace_id, created_at)
             SELECT ?1, ?2, ?3, ?4 WHERE EXISTS (
                 SELECT 1 FROM request_logs WHERE trace_id = ?3 AND key_id = ?1
                   AND cleared_at IS NULL)",
            (key_id, response_id, trace_id, super::now_ts()),
        )?;
        Ok(())
    }

    /// Only return a predecessor if the completed response belongs to the
    /// same authenticated key AND its request log still exists. No temporal
    /// proximity fallback is permitted, especially for key_id|~ sessions.
    pub fn find_request_log_trace_for_response_id(
        &self,
        key_id: &str,
        response_id: &str,
    ) -> Result<Option<String>> {
        if !self.has_table("request_log_response_links")? {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT l.trace_id FROM request_log_response_links l
             JOIN request_logs r ON r.trace_id = l.trace_id AND r.key_id = l.key_id
             WHERE l.key_id = ?1 AND l.response_id = ?2
               AND r.cleared_at IS NULL LIMIT 1",
                (key_id, response_id),
                |row| row.get(0),
            )
            .optional()
    }

    /// Rebuild the stored fields and list items of a trace.
    pub fn load_request_log_payload_full(
        &self,
        trace_id: &str,
        stage: &str,
    ) -> Result<Option<RequestLogPayloadFull>> {
        let Some(manifest) = self.find_request_log_payload_manifest(trace_id, stage)? else {
            return Ok(None);
        };
        let mut stmt = self.conn.prepare(
            "SELECT f.field_name, b.content
             FROM request_log_payload_manifest_fields f
             JOIN request_log_payload_blobs b ON b.id = f.blob_id
             WHERE f.trace_id = ?1 AND f.stage = ?2 ORDER BY f.position",
        )?;
        let fields = stmt
            .query_map((trace_id, stage), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        let (items, complete) = match self.resolve_request_log_payload_item_ids(
            trace_id,
            stage,
            manifest.item_count,
        )? {
            Some(ids) => (self.load_request_log_payload_blob_contents(&ids)?, true),
            None => (Vec::new(), manifest.item_count <= 0),
        };
        Ok(Some(RequestLogPayloadFull {
            manifest,
            fields,
            items,
            complete,
        }))
    }

    fn load_request_log_payload_blob_contents(&self, ids: &[i64]) -> Result<Vec<String>> {
        let mut unique = ids.to_vec();
        unique.sort_unstable();
        unique.dedup();
        let mut contents = HashMap::with_capacity(unique.len());
        for chunk in unique.chunks(ID_LOOKUP_CHUNK) {
            let params = chunk
                .iter()
                .map(|id| Value::Integer(*id))
                .collect::<Vec<_>>();
            let mut stmt = self.conn.prepare(&format!(
                "SELECT id, content FROM request_log_payload_blobs WHERE id IN ({})",
                placeholders(chunk.len(), "?")
            ))?;
            let rows = stmt.query_map(params, |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, content) = row?;
                contents.insert(id, content);
            }
        }
        ids.iter()
            .map(|id| {
                contents.get(id).cloned().ok_or_else(|| {
                    rusqlite::Error::SqliteFailure((), Some("payload blob missing".to_string()))
                })
            })
            .collect()
    }

    /// Turn a manifest into a root that owns all of its items, so its parent
    /// can be deleted.
    pub(super) fn materialize_request_log_payload_manifest(
        &self,
        trace_id: &str,
        stage: &str,
        item_count: i64,
    ) -> Result<()> {
        let resolved = self.resolve_request_log_payload_item_ids(trace_id, stage, item_count)?;
        if let Some(ids) = resolved {
            self.conn.execute(
                "DELETE FROM request_log_payload_manifest_items WHERE trace_id = ?1 AND stage = ?2",
                (trace_id, stage),
            )?;
            self.insert_request_log_payload_items(trace_id, stage, 0, &ids)?;
        }
        self.conn.execute(
            "UPDATE request_log_payload_manifests
             SET parent_trace_id = NULL, shared_prefix_len = 0
             WHERE trace_id = ?1 AND stage = ?2",
            (trace_id, stage),
        )?;
        Ok(())
    }

    fn gc_request_log_payload_blobs(&self) -> Result<usize> {
        self.conn.execute(
            "DELETE FROM request_log_payload_blobs
             WHERE NOT EXISTS (
                 SELECT 1 FROM request_log_payload_manifest_items i
                 WHERE i.blob_id = request_log_payload_blobs.id)
               AND NOT EXISTS (
                 SELECT 1 FROM request_log_payload_manifest_fields f
                 WHERE f.blob_id = request_log_payload_blobs.id)",
            [],
        )
    }

    /// Remove every full payload manifest and blob.
    pub fn clear_request_log_payload_store(&self) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let removed = self.clear_request_log_payload_store_in_transaction()?;
        tx.commit()?;
        Ok(removed)
    }

    pub(super) fn clear_request_log_payload_store_in_transaction(&self) -> Result<usize> {
        if !self.has_request_log_payload_store()? {
            return Ok(0);
        }
        self.conn
            .execute("DELETE FROM request_log_payload_manifest_items", [])?;
        self.conn
            .execute("DELETE FROM request_log_payload_manifest_fields", [])?;
        let removed = self
            .conn
            .execute("DELETE FROM request_log_payload_manifests", [])?;
        self.conn
            .execute("DELETE FROM request_log_payload_blobs", [])?;
        Ok(removed)
    }

    /// Delete manifests older than `cutoff_ts`. Surviving manifests whose
    /// parent is deleted are materialized first; unreferenced blobs are
    /// garbage-collected afterwards.
    pub fn prune_request_log_payload_store_before(&self, cutoff_ts: i64) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let removed = self.prune_request_log_payload_store_in_transaction(cutoff_ts)?;
        tx.commit()?;
        Ok(removed)
    }

    pub(super) fn prune_request_log_payload_store_in_transaction(
        &self,
        cutoff_ts: i64,
    ) -> Result<usize> {
        if cutoff_ts <= 0 || !self.has_request_log_payload_store()? {
            return Ok(0);
        }
        let doomed: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM request_log_payload_manifests WHERE created_at < ?1",
            [cutoff_ts],
            |row| row.get(0),
        )?;
        if doomed == 0 {
            return Ok(0);
        }
        let mut stmt = self.conn.prepare(
            "SELECT m.trace_id, m.stage, m.item_count FROM request_log_payload_manifests m
             WHERE m.created_at >= ?1 AND EXISTS (
                 SELECT 1 FROM request_log_payload_manifests parent
                 WHERE parent.trace_id = m.parent_trace_id
                   AND parent.stage = m.stage
                   AND parent.created_at < ?1)",
        )?;
        let orphans = stmt
            .query_map([cutoff_ts], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>>>()?;
        for (trace_id, stage, item_count) in orphans {
            self.materialize_request_log_payload_manifest(&trace_id, stage.as_str(), item_count)?;
        }
        self.conn.execute(
            "DELETE FROM request_log_payload_manifest_items WHERE EXISTS (
                 SELECT 1 FROM request_log_payload_manifests manifest
                 WHERE manifest.trace_id = request_log_payload_manifest_items.trace_id
                   AND manifest.stage = request_log_payload_manifest_items.stage
                   AND manifest.created_at < ?1)",
            [cutoff_ts],
        )?;
        self.conn.execute(
            "DELETE FROM request_log_payload_manifest_fields WHERE EXISTS (
                 SELECT 1 FROM request_log_payload_manifests manifest
                 WHERE manifest.trace_id = request_log_payload_manifest_fields.trace_id
                   AND manifest.stage = request_log_payload_manifest_fields.stage
                   AND manifest.created_at < ?1)",
            [cutoff_ts],
        )?;
        let removed = self.conn.execute(
            "DELETE FROM request_log_payload_manifests WHERE created_at < ?1",
            [cutoff_ts],
        )?;
        self.gc_request_log_payload_blobs()?;
        Ok(removed)
    }
}

#[cfg(test)]
#[path = "request_log_payload_store_tests.rs"]
mod tests;
