-- A completed Responses API response ID is the only reliable predecessor of
-- a request carrying previous_response_id. Scope it to the authenticated key.
CREATE TABLE IF NOT EXISTS request_log_response_links (
    key_id TEXT NOT NULL,
    response_id TEXT NOT NULL,
    trace_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (key_id, response_id)
);
CREATE INDEX IF NOT EXISTS idx_request_log_response_links_trace
    ON request_log_response_links(trace_id);
CREATE INDEX IF NOT EXISTS idx_request_log_response_links_created
    ON request_log_response_links(created_at);

-- Prevent payload jobs queued before a clear/retention operation from
-- restoring deleted content after the operation commits.
CREATE TABLE IF NOT EXISTS request_log_payload_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    generation INTEGER NOT NULL DEFAULT 0,
    retention_cutoff INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO request_log_payload_state (id, generation, retention_cutoff)
VALUES (1, 0, 0);

-- One row per actual outbound send attempt, including retries. The body is
-- stored in the existing preview/manifest tables under the same stage.
CREATE TABLE IF NOT EXISTS request_log_upstream_attempts (
    trace_id TEXT NOT NULL,
    stage TEXT NOT NULL,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    transport TEXT NOT NULL,
    content_encoding TEXT,
    wire_sha256 TEXT NOT NULL,
    identical_to_client INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (trace_id, stage)
);
CREATE INDEX IF NOT EXISTS idx_request_log_upstream_attempts_created
    ON request_log_upstream_attempts(created_at);
