-- Health records contain typed, expiring report enums only.
-- Owner identifiers are internal and never included in shared health snapshots.
CREATE TABLE IF NOT EXISTS service_health_records (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL,
    owner TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_service_health_kind_expiry
    ON service_health_records(kind, expires_at);
CREATE INDEX IF NOT EXISTS idx_service_health_owner_kind
    ON service_health_records(owner, kind);
