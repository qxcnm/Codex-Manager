-- Full (untruncated) request payload storage with content-addressed
-- de-duplication. Used when the preview (16 KB cap) storage mode is off.
--
-- Every trace can keep two captures, distinguished by `stage`:
-- `client` (body as received from the client) and `upstream` (body actually
-- forwarded upstream after local rewriting, only stored when it differs).
--
-- * request_log_payload_blobs: every distinct JSON fragment (one message /
--   input item, or one top-level field such as `tools` / `instructions`) is
--   stored exactly once, keyed by its SHA-256, and shared by both stages.
-- * request_log_payload_manifests: one row per trace and stage describing
--   how to rebuild the body. `parent_trace_id` + `shared_prefix_len` point
--   at an earlier request of the same conversation whose first
--   `shared_prefix_len` list items are identical, so only the new tail is
--   recorded for this trace.
-- * request_log_payload_manifest_items: the list items owned by a manifest,
--   at their absolute position in the rebuilt list.
-- * request_log_payload_manifest_fields: the top-level fields of the body
--   other than the list field.
--
-- Blobs are garbage-collected once no manifest item / field references
-- them, and manifests that lose their parent during pruning are rebased so
-- they never point at a deleted row.
CREATE TABLE IF NOT EXISTS request_log_payload_blobs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  hash TEXT NOT NULL UNIQUE,
  content TEXT NOT NULL,
  content_bytes INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS request_log_payload_manifests (
  trace_id TEXT NOT NULL,
  stage TEXT NOT NULL DEFAULT 'upstream',
  body_kind TEXT NOT NULL,
  list_field TEXT,
  conversation_key TEXT,
  parent_trace_id TEXT,
  shared_prefix_len INTEGER NOT NULL DEFAULT 0,
  item_count INTEGER NOT NULL DEFAULT 0,
  previous_response_id TEXT,
  payload_bytes INTEGER NOT NULL,
  redacted INTEGER NOT NULL DEFAULT 1,
  body_hash TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  PRIMARY KEY (trace_id, stage)
);

CREATE INDEX IF NOT EXISTS idx_request_log_payload_manifests_conversation
  ON request_log_payload_manifests(conversation_key, stage, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_request_log_payload_manifests_parent
  ON request_log_payload_manifests(parent_trace_id);

CREATE INDEX IF NOT EXISTS idx_request_log_payload_manifests_created_at
  ON request_log_payload_manifests(created_at);

CREATE TABLE IF NOT EXISTS request_log_payload_manifest_items (
  trace_id TEXT NOT NULL,
  stage TEXT NOT NULL DEFAULT 'upstream',
  position INTEGER NOT NULL,
  blob_id INTEGER NOT NULL,
  PRIMARY KEY (trace_id, stage, position)
);

CREATE INDEX IF NOT EXISTS idx_request_log_payload_manifest_items_blob
  ON request_log_payload_manifest_items(blob_id);

CREATE TABLE IF NOT EXISTS request_log_payload_manifest_fields (
  trace_id TEXT NOT NULL,
  stage TEXT NOT NULL DEFAULT 'upstream',
  position INTEGER NOT NULL,
  field_name TEXT NOT NULL,
  blob_id INTEGER NOT NULL,
  PRIMARY KEY (trace_id, stage, position)
);

CREATE INDEX IF NOT EXISTS idx_request_log_payload_manifest_fields_blob
  ON request_log_payload_manifest_fields(blob_id);
