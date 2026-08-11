CREATE TABLE IF NOT EXISTS codex_agent_identities (
    account_id TEXT PRIMARY KEY,
    agent_runtime_id TEXT NOT NULL,
    encrypted_private_key TEXT NOT NULL,
    encrypted_task_id TEXT,
    chatgpt_user_id TEXT,
    email TEXT,
    plan_type TEXT,
    is_fedramp INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'ready',
    last_error TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_codex_agent_identities_status
    ON codex_agent_identities(status, updated_at DESC);
