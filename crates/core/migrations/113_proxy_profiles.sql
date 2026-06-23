CREATE TABLE IF NOT EXISTS proxy_profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  proxy_url TEXT NOT NULL,
  proxy_url_redacted TEXT NOT NULL,
  scheme TEXT,
  host TEXT,
  port INTEGER,
  enabled INTEGER NOT NULL DEFAULT 1,
  status TEXT NOT NULL DEFAULT 'unchecked',
  last_error TEXT,
  last_url_latency_ms INTEGER,
  last_download_mbps REAL,
  last_upload_mbps REAL,
  last_tested_at INTEGER,
  ip TEXT,
  country_code TEXT,
  country_name TEXT,
  region_name TEXT,
  city_name TEXT,
  asn INTEGER,
  as_org TEXT,
  isp TEXT,
  as_domain TEXT,
  flag_img_url TEXT,
  flag_emoji TEXT,
  timezone_id TEXT,
  timezone_offset INTEGER,
  timezone_utc TEXT,
  tags_json TEXT,
  notes TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_proxy_profiles_status
  ON proxy_profiles(status);

CREATE INDEX IF NOT EXISTS idx_proxy_profiles_enabled
  ON proxy_profiles(enabled);
