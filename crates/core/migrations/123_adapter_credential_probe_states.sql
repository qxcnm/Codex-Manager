CREATE TABLE IF NOT EXISTS adapter_credential_probe_states (
  pool_id TEXT NOT NULL,
  credential_id TEXT NOT NULL,
  status TEXT NOT NULL,
  error_code TEXT,
  checked_at INTEGER NOT NULL,
  retry_after INTEGER,
  PRIMARY KEY (pool_id, credential_id)
);

CREATE INDEX IF NOT EXISTS idx_adapter_credential_probe_states_pool_status_retry
  ON adapter_credential_probe_states(pool_id, status, retry_after, checked_at DESC);

INSERT OR IGNORE INTO adapter_credential_probe_states (
  pool_id,
  credential_id,
  status,
  error_code,
  checked_at,
  retry_after
)
SELECT
  'codex',
  ranked.account_id,
  CASE
    WHEN ranked.used_percent IS NOT NULL
      AND ranked.window_minutes IS NOT NULL
      AND ranked.used_percent < 100
      AND (ranked.secondary_used_percent IS NULL OR ranked.secondary_used_percent < 100)
      THEN 'available'
    ELSE 'unavailable'
  END,
  CASE
    WHEN ranked.used_percent IS NULL OR ranked.window_minutes IS NULL THEN 'usage_unknown'
    WHEN ranked.used_percent >= 100
      OR ranked.secondary_used_percent >= 100 THEN 'quota_exhausted'
    ELSE NULL
  END,
  ranked.captured_at,
  NULL
FROM (
  SELECT
    account_id,
    used_percent,
    window_minutes,
    secondary_used_percent,
    captured_at,
    ROW_NUMBER() OVER (
      PARTITION BY account_id
      ORDER BY captured_at DESC, id DESC
    ) AS rn
  FROM usage_snapshots
) ranked
WHERE ranked.rn = 1;

