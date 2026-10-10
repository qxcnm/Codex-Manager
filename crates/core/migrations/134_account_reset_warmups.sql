CREATE TABLE IF NOT EXISTS account_reset_warmups (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  pending_reset_at INTEGER,
  consumed_reset_at INTEGER,
  last_claimed_at INTEGER
);

CREATE INDEX IF NOT EXISTS idx_account_reset_warmups_due
  ON account_reset_warmups(pending_reset_at, account_id)
  WHERE enabled = 1 AND pending_reset_at IS NOT NULL;

-- Register the latest exhausted main 5-hour window on upgrade. Optional model
-- buckets stored in credits_json deliberately do not start account warmups.
INSERT OR IGNORE INTO account_reset_warmups (account_id, pending_reset_at)
SELECT account_id, MAX(reset_at)
FROM (
  SELECT s.account_id,
         CASE WHEN s.window_minutes = 300 AND s.used_percent >= 100
              THEN s.resets_at END AS reset_at
  FROM accounts a
  JOIN usage_snapshots s ON s.id = (
    SELECT id FROM usage_snapshots WHERE account_id = a.id
    ORDER BY captured_at DESC, id DESC LIMIT 1
  )
  UNION ALL
  SELECT s.account_id,
         CASE WHEN s.secondary_window_minutes = 300 AND s.secondary_used_percent >= 100
              THEN s.secondary_resets_at END AS reset_at
  FROM accounts a
  JOIN usage_snapshots s ON s.id = (
    SELECT id FROM usage_snapshots WHERE account_id = a.id
    ORDER BY captured_at DESC, id DESC LIMIT 1
  )
)
WHERE reset_at > 0
GROUP BY account_id;
