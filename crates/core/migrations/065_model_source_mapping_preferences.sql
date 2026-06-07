CREATE TABLE IF NOT EXISTS model_source_mapping_preferences (
    source_kind     TEXT NOT NULL,
    source_id       TEXT NOT NULL,
    upstream_model  TEXT NOT NULL,
    preference      TEXT NOT NULL CHECK (preference IN ('unlinked', 'disabled')),
    updated_at      INTEGER NOT NULL,
    PRIMARY KEY (source_kind, source_id, upstream_model)
);

CREATE INDEX IF NOT EXISTS idx_model_source_mapping_preferences_source
    ON model_source_mapping_preferences(source_kind, source_id);
