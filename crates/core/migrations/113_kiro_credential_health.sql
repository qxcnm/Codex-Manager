ALTER TABLE kiro_credentials ADD COLUMN request_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE kiro_credentials ADD COLUMN success_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE kiro_credentials ADD COLUMN last_latency_ms INTEGER;
