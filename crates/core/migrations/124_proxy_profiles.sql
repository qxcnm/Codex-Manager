CREATE TABLE IF NOT EXISTS proxy_profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    proxy_url TEXT NOT NULL,
    proxy_username TEXT,
    encrypted_password TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    fallback_mode TEXT NOT NULL DEFAULT 'none',
    backup_proxy_id TEXT,
    exit_ip TEXT,
    country_code TEXT,
    region TEXT,
    latency_ms INTEGER,
    last_probe_status TEXT NOT NULL DEFAULT 'unknown',
    last_probe_error TEXT,
    last_probe_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (backup_proxy_id) REFERENCES proxy_profiles(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_proxy_profiles_status
    ON proxy_profiles(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS account_proxy_bindings (
    account_id TEXT PRIMARY KEY,
    mode TEXT NOT NULL DEFAULT 'inherit',
    proxy_profile_id TEXT,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (proxy_profile_id) REFERENCES proxy_profiles(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_account_proxy_bindings_profile
    ON account_proxy_bindings(proxy_profile_id, mode);
