CREATE TABLE IF NOT EXISTS reset_credit_operation_accounts (
  account_id TEXT PRIMARY KEY,
  operation_id TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_reset_credit_operation_accounts_operation
  ON reset_credit_operation_accounts(operation_id);

-- Preserve a pending operation created before the guard table was introduced.
-- If old data contains more than one pending operation for an account, retain
-- the oldest deterministic row as the guard and leave the other rows replayable.
INSERT OR IGNORE INTO reset_credit_operation_accounts(account_id, operation_id, created_at)
SELECT candidate.account_id, candidate.operation_id, candidate.created_at
FROM reset_credit_operations AS candidate
WHERE candidate.status = 'pending'
  AND NOT EXISTS (
    SELECT 1
    FROM reset_credit_operations AS earlier
    WHERE earlier.account_id = candidate.account_id
      AND earlier.status = 'pending'
      AND (
        earlier.created_at < candidate.created_at
        OR (
          earlier.created_at = candidate.created_at
          AND earlier.operation_id < candidate.operation_id
        )
      )
  );
