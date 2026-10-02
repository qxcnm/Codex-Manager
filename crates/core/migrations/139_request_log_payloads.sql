-- Request payload previews for the request log detail view.
-- One row per gateway trace; the payload is sanitized and size-capped at
-- ingest time by the service layer (see observability/request_log.rs).
CREATE TABLE IF NOT EXISTS request_log_payloads (
  trace_id TEXT PRIMARY KEY,
  payload TEXT NOT NULL,
  payload_bytes INTEGER NOT NULL,
  payload_truncated INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_request_log_payloads_created_at
  ON request_log_payloads(created_at DESC);
