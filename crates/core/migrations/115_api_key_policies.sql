CREATE TABLE IF NOT EXISTS api_key_policies (
  key_id TEXT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
  allowed_models_json TEXT,
  allowed_platforms_json TEXT,
  model_visibility TEXT NOT NULL DEFAULT 'selectable',
  expires_at INTEGER,
  concurrency_limit INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (model_visibility IN ('selectable', 'managed')),
  CHECK (concurrency_limit IS NULL OR concurrency_limit > 0)
);

CREATE INDEX IF NOT EXISTS idx_api_key_policies_expires_at
  ON api_key_policies(expires_at);
