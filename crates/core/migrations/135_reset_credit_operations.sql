CREATE TABLE IF NOT EXISTS reset_credit_operations (
  operation_id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL,
  redeem_request_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending', 'completed', 'failed')),
  result_json TEXT,
  error TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_reset_credit_operations_account_created
  ON reset_credit_operations(account_id, created_at, operation_id);
