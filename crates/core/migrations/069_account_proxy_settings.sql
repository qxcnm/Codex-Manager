CREATE TABLE IF NOT EXISTS account_proxy_settings (
  account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 0,
  proxy_source TEXT,
  proxy_profile_id TEXT REFERENCES proxy_profiles(id) ON DELETE SET NULL,
  proxy_url TEXT,
  status TEXT NOT NULL DEFAULT 'unchecked',
  latency_ms INTEGER,
  last_download_mbps REAL,
  last_upload_mbps REAL,
  last_check_at INTEGER,
  last_error TEXT,
  ip TEXT,
  country_code TEXT,
  country_name TEXT,
  region_name TEXT,
  city_name TEXT,
  geo_checked_at INTEGER,
  geo_error TEXT,
  asn INTEGER,
  as_org TEXT,
  isp TEXT,
  as_domain TEXT,
  timezone_id TEXT,
  timezone_offset INTEGER,
  timezone_utc TEXT,
  flag_img_url TEXT,
  flag_emoji TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_account_proxy_settings_updated_at
  ON account_proxy_settings(updated_at DESC, account_id ASC);

CREATE INDEX IF NOT EXISTS idx_account_proxy_settings_proxy_profile_id
  ON account_proxy_settings(proxy_profile_id);
