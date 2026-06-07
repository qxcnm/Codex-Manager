CREATE TABLE IF NOT EXISTS app_users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    display_name TEXT,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'operator', 'member')),
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_login_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_app_users_role_status ON app_users(role, status);

CREATE TABLE IF NOT EXISTS app_user_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    last_seen_at INTEGER,
    revoked_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_app_user_sessions_user_id ON app_user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_app_user_sessions_token_hash ON app_user_sessions(token_hash);

CREATE TABLE IF NOT EXISTS app_projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner_user_id TEXT REFERENCES app_users(id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS app_project_members (
    project_id TEXT NOT NULL REFERENCES app_projects(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, user_id)
);

CREATE TABLE IF NOT EXISTS app_wallets (
    id TEXT PRIMARY KEY,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'project')),
    owner_id TEXT NOT NULL,
    balance_credit_micros INTEGER NOT NULL DEFAULT 0,
    frozen_credit_micros INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(owner_kind, owner_id)
);

CREATE INDEX IF NOT EXISTS idx_app_wallets_owner ON app_wallets(owner_kind, owner_id);

CREATE TABLE IF NOT EXISTS app_wallet_ledger_entries (
    id TEXT PRIMARY KEY,
    wallet_id TEXT NOT NULL REFERENCES app_wallets(id) ON DELETE CASCADE,
    entry_kind TEXT NOT NULL,
    amount_credit_micros INTEGER NOT NULL,
    balance_after_credit_micros INTEGER NOT NULL,
    request_log_id INTEGER,
    api_key_id TEXT REFERENCES api_keys(id) ON DELETE SET NULL,
    pricing_rule_id TEXT,
    raw_usage_json TEXT,
    note TEXT,
    created_by_user_id TEXT REFERENCES app_users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_app_wallet_ledger_wallet_created ON app_wallet_ledger_entries(wallet_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_app_wallet_ledger_api_key ON app_wallet_ledger_entries(api_key_id);
CREATE INDEX IF NOT EXISTS idx_app_wallet_ledger_request_log_kind ON app_wallet_ledger_entries(request_log_id, entry_kind);

CREATE TABLE IF NOT EXISTS api_key_owners (
    key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('user', 'project')),
    owner_user_id TEXT REFERENCES app_users(id) ON DELETE SET NULL,
    project_id TEXT REFERENCES app_projects(id) ON DELETE SET NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_api_key_owners_user ON api_key_owners(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_api_key_owners_project ON api_key_owners(project_id);

CREATE TABLE IF NOT EXISTS billing_rules (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    priority INTEGER NOT NULL DEFAULT 0,
    multiplier_millis INTEGER NOT NULL DEFAULT 1000,
    model_pattern TEXT,
    service_tier TEXT,
    user_id TEXT REFERENCES app_users(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES app_projects(id) ON DELETE CASCADE,
    api_key_id TEXT REFERENCES api_keys(id) ON DELETE CASCADE,
    starts_at INTEGER,
    ends_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_billing_rules_status_priority ON billing_rules(status, priority DESC);

CREATE TABLE IF NOT EXISTS redeem_code_batches (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    value_credit_micros INTEGER NOT NULL,
    total_count INTEGER NOT NULL,
    expires_at INTEGER,
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    created_by_user_id TEXT REFERENCES app_users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS redeem_codes (
    id TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES redeem_code_batches(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL UNIQUE,
    max_redemptions INTEGER NOT NULL DEFAULT 1,
    redeemed_count INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    expires_at INTEGER,
    created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_redeem_codes_batch ON redeem_codes(batch_id);

CREATE TABLE IF NOT EXISTS redeem_records (
    id TEXT PRIMARY KEY,
    code_id TEXT NOT NULL REFERENCES redeem_codes(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
    wallet_id TEXT NOT NULL REFERENCES app_wallets(id) ON DELETE CASCADE,
    amount_credit_micros INTEGER NOT NULL,
    ledger_entry_id TEXT REFERENCES app_wallet_ledger_entries(id) ON DELETE SET NULL,
    redeemed_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_redeem_records_user ON redeem_records(user_id, redeemed_at DESC);
