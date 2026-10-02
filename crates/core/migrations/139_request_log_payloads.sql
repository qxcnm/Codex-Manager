-- Request payload previews for the request log detail view.
-- One row per gateway trace and capture stage, where the payload is optionally
-- sanitized and size-capped at ingest time by the service layer (see
-- observability/request_log_payload.rs). Used when the preview (16 KB cap)
-- storage mode is enabled.
--
-- stage is `client` (body as received from the client) or `upstream` (body
-- actually forwarded upstream after local rewriting). The upstream row is
-- only written when it differs from the client row.
CREATE TABLE IF NOT EXISTS request_log_payloads (
  trace_id TEXT NOT NULL,
  stage TEXT NOT NULL DEFAULT 'upstream',
  payload TEXT NOT NULL,
  payload_bytes INTEGER NOT NULL,
  payload_truncated INTEGER NOT NULL DEFAULT 0,
  redacted INTEGER NOT NULL DEFAULT 1,
  body_hash TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL,
  PRIMARY KEY (trace_id, stage)
);

CREATE INDEX IF NOT EXISTS idx_request_log_payloads_created_at
  ON request_log_payloads(created_at DESC);
