CREATE TABLE IF NOT EXISTS proxy_speed_tests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  scope TEXT NOT NULL, -- 'system_proxy' or 'account_proxy'
  proxy_profile_id TEXT REFERENCES proxy_profiles(id) ON DELETE CASCADE,
  account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  provider TEXT NOT NULL,
  observed_ip TEXT,
  observed_country TEXT,
  observed_colo TEXT,
  max_payload_bytes INTEGER,
  samples_json TEXT, -- JSON array of SpeedSample
  download_summary_json TEXT, -- JSON of SpeedMetricSummary
  upload_summary_json TEXT, -- JSON of SpeedMetricSummary
  started_at INTEGER NOT NULL,
  finished_at INTEGER NOT NULL,
  error_code TEXT,
  error TEXT
);

CREATE INDEX IF NOT EXISTS idx_proxy_speed_tests_profile ON proxy_speed_tests(proxy_profile_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_proxy_speed_tests_account ON proxy_speed_tests(account_id, started_at DESC);

CREATE TABLE IF NOT EXISTS proxy_diagnostics_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  scope TEXT NOT NULL, -- 'system_proxy' or 'account_proxy'
  proxy_profile_id TEXT REFERENCES proxy_profiles(id) ON DELETE CASCADE,
  account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  provider TEXT NOT NULL,
  file_size_id TEXT NOT NULL,
  downloaded_bytes INTEGER,
  duration_ms INTEGER,
  mbps REAL,
  tested_at INTEGER NOT NULL,
  error TEXT
);

CREATE INDEX IF NOT EXISTS idx_proxy_diagnostics_history_profile ON proxy_diagnostics_history(proxy_profile_id, tested_at DESC);
CREATE INDEX IF NOT EXISTS idx_proxy_diagnostics_history_account ON proxy_diagnostics_history(account_id, tested_at DESC);

CREATE TABLE IF NOT EXISTS account_proxy_url_tests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  status TEXT NOT NULL,
  url_latency_ms INTEGER,
  status_code INTEGER,
  test_url TEXT NOT NULL,
  final_url TEXT,
  redirected INTEGER NOT NULL DEFAULT 0,
  tested_at INTEGER NOT NULL,
  error_code TEXT,
  error TEXT
);

CREATE INDEX IF NOT EXISTS idx_account_proxy_url_tests_account ON account_proxy_url_tests(account_id, tested_at DESC, id DESC);
