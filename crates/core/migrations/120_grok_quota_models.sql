ALTER TABLE grok_credentials ADD COLUMN web_tier TEXT NOT NULL DEFAULT 'unknown';

CREATE TABLE IF NOT EXISTS grok_credential_quota_windows (
    credential_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    remaining_queries INTEGER NOT NULL,
    total_queries INTEGER NOT NULL,
    window_size_seconds INTEGER NOT NULL,
    reset_at INTEGER NOT NULL,
    checked_at INTEGER NOT NULL,
    PRIMARY KEY (credential_id, mode),
    FOREIGN KEY (credential_id) REFERENCES grok_credentials(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS grok_credential_models (
    credential_id TEXT NOT NULL,
    model_slug TEXT NOT NULL,
    status TEXT NOT NULL,
    error_code TEXT,
    latency_ms INTEGER,
    checked_at INTEGER NOT NULL,
    PRIMARY KEY (credential_id, model_slug),
    FOREIGN KEY (credential_id) REFERENCES grok_credentials(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_grok_models_available
    ON grok_credential_models(status, model_slug, checked_at DESC);
