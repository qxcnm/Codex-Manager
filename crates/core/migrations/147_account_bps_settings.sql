CREATE TABLE IF NOT EXISTS account_bps_settings (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  access_token TEXT,
  refresh_token TEXT,
  upstream_account_id TEXT,
  expires_at INTEGER,
  updated_at INTEGER NOT NULL
);
