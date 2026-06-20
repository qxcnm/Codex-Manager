CREATE TABLE IF NOT EXISTS proxy_profile_url_tests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  proxy_profile_id TEXT NOT NULL REFERENCES proxy_profiles(id) ON DELETE CASCADE,
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

CREATE INDEX IF NOT EXISTS idx_proxy_profile_url_tests_profile_tested_at
  ON proxy_profile_url_tests(proxy_profile_id, tested_at DESC, id DESC);

