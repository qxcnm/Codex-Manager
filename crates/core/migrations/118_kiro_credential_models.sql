CREATE TABLE IF NOT EXISTS kiro_credential_models (
    credential_id TEXT NOT NULL,
    model_slug TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('available', 'unavailable', 'unknown')),
    error_code TEXT,
    latency_ms INTEGER,
    checked_at INTEGER NOT NULL,
    PRIMARY KEY (credential_id, model_slug),
    FOREIGN KEY (credential_id) REFERENCES kiro_credentials(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_kiro_credential_models_available
    ON kiro_credential_models(status, model_slug, credential_id);
