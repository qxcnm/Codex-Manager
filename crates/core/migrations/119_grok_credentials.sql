CREATE TABLE IF NOT EXISTS grok_credentials (
    id TEXT PRIMARY KEY,
    identity_hash TEXT NOT NULL UNIQUE,
    account_masked TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    priority INTEGER NOT NULL DEFAULT 0,
    weight REAL NOT NULL DEFAULT 1.0,
    proxy_url TEXT,
    encrypted_secret TEXT NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    expires_at INTEGER,
    cooldown_until INTEGER,
    failure_count INTEGER NOT NULL DEFAULT 0,
    request_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    last_latency_ms INTEGER,
    last_success_at INTEGER,
    last_failure_at INTEGER,
    last_failure_code TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_grok_credentials_route
    ON grok_credentials(status, cooldown_until, priority DESC, weight DESC);
