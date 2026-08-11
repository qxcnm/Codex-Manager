CREATE TABLE IF NOT EXISTS credential_vault_keys (
    id TEXT PRIMARY KEY,
    protector TEXT NOT NULL,
    wrapped_key BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS kiro_credentials (
    id TEXT PRIMARY KEY,
    auth_method TEXT NOT NULL,
    identity_hash TEXT NOT NULL,
    email TEXT,
    auth_region TEXT,
    api_region TEXT,
    subscription TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    priority INTEGER NOT NULL DEFAULT 0,
    weight REAL NOT NULL DEFAULT 1.0,
    proxy_url TEXT,
    encrypted_secret BLOB NOT NULL,
    secret_nonce BLOB NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    credit_limit REAL,
    credit_used REAL,
    expires_at INTEGER,
    cooldown_until INTEGER,
    failure_count INTEGER NOT NULL DEFAULT 0,
    last_success_at INTEGER,
    last_failure_at INTEGER,
    last_failure_code TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(auth_method, identity_hash)
);

CREATE INDEX IF NOT EXISTS idx_kiro_credentials_route
    ON kiro_credentials(status, cooldown_until, priority DESC, weight DESC);
CREATE INDEX IF NOT EXISTS idx_kiro_credentials_email
    ON kiro_credentials(email);
