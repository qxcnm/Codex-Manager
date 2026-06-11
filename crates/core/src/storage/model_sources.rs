use super::{now_ts, ModelSourceMapping, ModelSourceMappingPreference, ModelSourceModel, Storage};
use rusqlite::{params, OptionalExtension, Result, Row};

fn map_source_model(row: &Row<'_>) -> Result<ModelSourceModel> {
    Ok(ModelSourceModel {
        source_kind: row.get(0)?,
        source_id: row.get(1)?,
        upstream_model: row.get(2)?,
        display_name: row.get(3)?,
        status: row.get(4)?,
        discovery_kind: row.get(5)?,
        last_synced_at: row.get(6)?,
        extra_json: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn map_source_mapping(row: &Row<'_>) -> Result<ModelSourceMapping> {
    let enabled: i64 = row.get(5)?;
    Ok(ModelSourceMapping {
        id: row.get(0)?,
        platform_model_slug: row.get(1)?,
        source_kind: row.get(2)?,
        source_id: row.get(3)?,
        upstream_model: row.get(4)?,
        enabled: enabled != 0,
        priority: row.get(6)?,
        weight: row.get(7)?,
        billing_model_slug: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn normalize_text(value: &str) -> String {
    value.trim().to_string()
}

impl Storage {
    pub(super) fn ensure_model_source_tables(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS model_source_models (
                source_kind TEXT NOT NULL,
                source_id TEXT NOT NULL,
                upstream_model TEXT NOT NULL,
                display_name TEXT,
                status TEXT NOT NULL DEFAULT 'available',
                discovery_kind TEXT NOT NULL DEFAULT 'synced',
                last_synced_at INTEGER,
                extra_json TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (source_kind, source_id, upstream_model)
            );
            CREATE INDEX IF NOT EXISTS idx_model_source_models_source
                ON model_source_models(source_kind, source_id);
            CREATE INDEX IF NOT EXISTS idx_model_source_models_upstream_model
                ON model_source_models(upstream_model);
            CREATE TABLE IF NOT EXISTS model_source_mappings (
                id TEXT PRIMARY KEY,
                platform_model_slug TEXT NOT NULL,
                source_kind TEXT NOT NULL,
                source_id TEXT NOT NULL,
                upstream_model TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                priority INTEGER NOT NULL DEFAULT 0,
                weight INTEGER NOT NULL DEFAULT 1,
                billing_model_slug TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                UNIQUE(platform_model_slug, source_kind, source_id, upstream_model)
            );
            CREATE INDEX IF NOT EXISTS idx_model_source_mappings_platform
                ON model_source_mappings(platform_model_slug, enabled, priority DESC);
            CREATE INDEX IF NOT EXISTS idx_model_source_mappings_source
                ON model_source_mappings(source_kind, source_id, enabled);
            CREATE TABLE IF NOT EXISTS model_source_mapping_preferences (
                source_kind     TEXT NOT NULL,
                source_id       TEXT NOT NULL,
                upstream_model  TEXT NOT NULL,
                preference      TEXT NOT NULL CHECK (preference IN ('unlinked', 'disabled')),
                updated_at      INTEGER NOT NULL,
                PRIMARY KEY (source_kind, source_id, upstream_model)
            );
            CREATE INDEX IF NOT EXISTS idx_model_source_mapping_preferences_source
                ON model_source_mapping_preferences(source_kind, source_id);",
        )
    }

    pub fn list_model_source_models(
        &self,
        source_kind: Option<&str>,
        source_id: Option<&str>,
    ) -> Result<Vec<ModelSourceModel>> {
        let mut sql = "SELECT source_kind, source_id, upstream_model, display_name, status,
                            discovery_kind, last_synced_at, extra_json, created_at, updated_at
                       FROM model_source_models"
            .to_string();
        let normalized_kind = source_kind.map(normalize_text).filter(|v| !v.is_empty());
        let normalized_id = source_id.map(normalize_text).filter(|v| !v.is_empty());
        match (&normalized_kind, &normalized_id) {
            (Some(_), Some(_)) => sql.push_str(" WHERE source_kind = ?1 AND source_id = ?2"),
            (Some(_), None) => sql.push_str(" WHERE source_kind = ?1"),
            (None, Some(_)) => sql.push_str(" WHERE source_id = ?1"),
            (None, None) => {}
        }
        sql.push_str(" ORDER BY source_kind ASC, source_id ASC, upstream_model ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = match (&normalized_kind, &normalized_id) {
            (Some(kind), Some(id)) => stmt.query_map(params![kind, id], map_source_model)?,
            (Some(kind), None) => stmt.query_map(params![kind], map_source_model)?,
            (None, Some(id)) => stmt.query_map(params![id], map_source_model)?,
            (None, None) => stmt.query_map([], map_source_model)?,
        };
        rows.collect()
    }

    pub fn list_available_source_model_ids_by_upstream_model(
        &self,
        source_kind: &str,
        upstream_model: &str,
    ) -> Result<Vec<String>> {
        let source_kind = normalize_text(source_kind);
        let upstream_model = normalize_text(upstream_model);
        if source_kind.is_empty() || upstream_model.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT source_id
             FROM model_source_models
             WHERE source_kind = ?1
               AND upstream_model = ?2
               AND status = 'available'
             ORDER BY source_id ASC",
        )?;
        let rows = stmt.query_map(params![source_kind, upstream_model], |row| {
            row.get::<_, String>(0)
        })?;
        rows.collect()
    }

    pub fn upsert_model_source_model(&self, model: &ModelSourceModel) -> Result<()> {
        self.conn.execute(
            "INSERT INTO model_source_models (
                source_kind, source_id, upstream_model, display_name, status, discovery_kind,
                last_synced_at, extra_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(source_kind, source_id, upstream_model) DO UPDATE SET
                display_name = excluded.display_name,
                status = excluded.status,
                discovery_kind = excluded.discovery_kind,
                last_synced_at = excluded.last_synced_at,
                extra_json = excluded.extra_json,
                updated_at = excluded.updated_at",
            params![
                &model.source_kind,
                &model.source_id,
                &model.upstream_model,
                &model.display_name,
                &model.status,
                &model.discovery_kind,
                model.last_synced_at,
                &model.extra_json,
                model.created_at,
                model.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn upsert_discovered_model_source_models(
        &self,
        source_kind: &str,
        source_id: &str,
        upstream_models: &[String],
        discovery_kind: &str,
    ) -> Result<Vec<ModelSourceModel>> {
        let now = now_ts();
        let source_kind = normalize_text(source_kind);
        let source_id = normalize_text(source_id);
        let discovery_kind = normalize_text(discovery_kind);
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for upstream_model in upstream_models {
            let upstream_model = normalize_text(upstream_model);
            if upstream_model.is_empty() || !seen.insert(upstream_model.clone()) {
                continue;
            }
            let record = ModelSourceModel {
                source_kind: source_kind.clone(),
                source_id: source_id.clone(),
                display_name: Some(upstream_model.clone()),
                upstream_model,
                status: "available".to_string(),
                discovery_kind: discovery_kind.clone(),
                last_synced_at: Some(now),
                extra_json: "{}".to_string(),
                created_at: now,
                updated_at: now,
            };
            self.upsert_model_source_model(&record)?;
            out.push(record);
        }

        let mut stmt = self.conn.prepare(
            "SELECT upstream_model
             FROM model_source_models
             WHERE source_kind = ?1
               AND source_id = ?2
               AND discovery_kind = ?3",
        )?;
        let existing_rows = stmt
            .query_map(params![&source_kind, &source_id, &discovery_kind], |row| {
                row.get::<_, String>(0)
            })?;
        let existing_upstream_models: std::collections::BTreeSet<String> = existing_rows
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .collect();
        let stale_upstream_models = existing_upstream_models
            .difference(&seen)
            .cloned()
            .collect::<Vec<_>>();
        for upstream_model in stale_upstream_models {
            self.conn.execute(
                "DELETE FROM model_source_mappings
                 WHERE source_kind = ?1
                   AND source_id = ?2
                   AND upstream_model = ?3",
                params![&source_kind, &source_id, &upstream_model],
            )?;
            self.conn.execute(
                "DELETE FROM model_source_models
                 WHERE source_kind = ?1
                   AND source_id = ?2
                   AND upstream_model = ?3
                   AND discovery_kind = ?4",
                params![&source_kind, &source_id, &upstream_model, &discovery_kind],
            )?;
            self.conn.execute(
                "DELETE FROM model_source_mapping_preferences
                 WHERE source_kind = ?1
                   AND source_id = ?2
                   AND upstream_model = ?3",
                params![&source_kind, &source_id, &upstream_model],
            )?;
        }
        Ok(out)
    }

    pub fn list_model_source_mappings(
        &self,
        platform_model_slug: Option<&str>,
    ) -> Result<Vec<ModelSourceMapping>> {
        let normalized_slug = platform_model_slug
            .map(normalize_text)
            .filter(|value| !value.is_empty());
        let mut stmt = if normalized_slug.is_some() {
            self.conn.prepare(
                "SELECT id, platform_model_slug, source_kind, source_id, upstream_model,
                        enabled, priority, weight, billing_model_slug, created_at, updated_at
                 FROM model_source_mappings
                 WHERE platform_model_slug = ?1
                 ORDER BY enabled DESC, priority DESC, source_kind ASC, source_id ASC, upstream_model ASC",
            )?
        } else {
            self.conn.prepare(
                "SELECT id, platform_model_slug, source_kind, source_id, upstream_model,
                        enabled, priority, weight, billing_model_slug, created_at, updated_at
                 FROM model_source_mappings
                 ORDER BY platform_model_slug ASC, enabled DESC, priority DESC,
                          source_kind ASC, source_id ASC, upstream_model ASC",
            )?
        };
        let rows = if let Some(slug) = normalized_slug {
            stmt.query_map(params![slug], map_source_mapping)?
        } else {
            stmt.query_map([], map_source_mapping)?
        };
        rows.collect()
    }

    pub fn list_enabled_model_source_mappings_for_platform(
        &self,
        platform_model_slug: &str,
    ) -> Result<Vec<ModelSourceMapping>> {
        let slug = normalize_text(platform_model_slug);
        if slug.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, platform_model_slug, source_kind, source_id, upstream_model,
                    enabled, priority, weight, billing_model_slug, created_at, updated_at
             FROM model_source_mappings
             WHERE platform_model_slug = ?1 AND enabled = 1
             ORDER BY priority DESC, weight DESC, source_kind ASC, source_id ASC, upstream_model ASC",
        )?;
        let rows = stmt.query_map(params![slug], map_source_mapping)?;
        rows.collect()
    }

    pub fn find_enabled_model_source_mapping(
        &self,
        platform_model_slug: &str,
        source_kind: &str,
        source_id: &str,
    ) -> Result<Option<ModelSourceMapping>> {
        self.conn
            .query_row(
                "SELECT id, platform_model_slug, source_kind, source_id, upstream_model,
                        enabled, priority, weight, billing_model_slug, created_at, updated_at
                 FROM model_source_mappings
                 WHERE platform_model_slug = ?1
                   AND source_kind = ?2
                   AND source_id = ?3
                   AND enabled = 1
                 ORDER BY priority DESC, weight DESC, upstream_model ASC
                 LIMIT 1",
                params![
                    normalize_text(platform_model_slug),
                    normalize_text(source_kind),
                    normalize_text(source_id),
                ],
                map_source_mapping,
            )
            .optional()
    }

    pub fn upsert_model_source_mapping(&self, mapping: &ModelSourceMapping) -> Result<()> {
        self.conn.execute(
            "INSERT INTO model_source_mappings (
                id, platform_model_slug, source_kind, source_id, upstream_model,
                enabled, priority, weight, billing_model_slug, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(platform_model_slug, source_kind, source_id, upstream_model) DO UPDATE SET
                enabled = excluded.enabled,
                priority = excluded.priority,
                weight = excluded.weight,
                billing_model_slug = excluded.billing_model_slug,
                updated_at = excluded.updated_at",
            params![
                &mapping.id,
                &mapping.platform_model_slug,
                &mapping.source_kind,
                &mapping.source_id,
                &mapping.upstream_model,
                if mapping.enabled { 1 } else { 0 },
                mapping.priority,
                mapping.weight,
                &mapping.billing_model_slug,
                mapping.created_at,
                mapping.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn delete_model_source_mapping_with_unlink_preference(
        &self,
        id: &str,
        source_kind: &str,
        source_id: &str,
        upstream_model: &str,
    ) -> Result<()> {
        let id = normalize_text(id);
        let source_kind = normalize_text(source_kind);
        let source_id = normalize_text(source_id);
        let upstream_model = normalize_text(upstream_model);
        if source_kind.is_empty() || source_id.is_empty() || upstream_model.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO model_source_mapping_preferences
             (source_kind, source_id, upstream_model, preference, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_kind, source_id, upstream_model) DO UPDATE SET
                 preference = excluded.preference,
                 updated_at = excluded.updated_at",
            params![
                &source_kind,
                &source_id,
                &upstream_model,
                "unlinked",
                now_ts()
            ],
        )?;
        tx.execute(
            "DELETE FROM model_source_mappings WHERE id = ?1",
            params![&id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_model_source_mapping(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM model_source_mappings WHERE id = ?1",
            params![normalize_text(id)],
        )?;
        Ok(())
    }

    pub fn delete_model_source_routes_for_source(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<()> {
        let source_kind = normalize_text(source_kind);
        let source_id = normalize_text(source_id);
        if source_kind.is_empty() || source_id.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM model_source_mappings WHERE source_kind = ?1 AND source_id = ?2",
            params![&source_kind, &source_id],
        )?;
        self.conn.execute(
            "DELETE FROM model_source_models WHERE source_kind = ?1 AND source_id = ?2",
            params![&source_kind, &source_id],
        )?;
        Ok(())
    }

    pub fn upsert_model_source_mapping_preference(
        &self,
        source_kind: &str,
        source_id: &str,
        upstream_model: &str,
        preference: &str,
    ) -> Result<()> {
        let source_kind = normalize_text(source_kind);
        let source_id = normalize_text(source_id);
        let upstream_model = normalize_text(upstream_model);
        if source_kind.is_empty() || source_id.is_empty() || upstream_model.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO model_source_mapping_preferences
             (source_kind, source_id, upstream_model, preference, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(source_kind, source_id, upstream_model) DO UPDATE SET
                 preference = excluded.preference,
                 updated_at = excluded.updated_at",
            params![
                &source_kind,
                &source_id,
                &upstream_model,
                normalize_text(preference),
                now_ts(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_model_source_mapping_preference(
        &self,
        source_kind: &str,
        source_id: &str,
        upstream_model: &str,
    ) -> Result<()> {
        self.conn.execute(
            "DELETE FROM model_source_mapping_preferences
             WHERE source_kind = ?1 AND source_id = ?2 AND upstream_model = ?3",
            params![
                normalize_text(source_kind),
                normalize_text(source_id),
                normalize_text(upstream_model),
            ],
        )?;
        Ok(())
    }

    pub fn delete_model_source_mapping_preferences_for_source(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<()> {
        let source_kind = normalize_text(source_kind);
        let source_id = normalize_text(source_id);
        if source_kind.is_empty() || source_id.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM model_source_mapping_preferences
             WHERE source_kind = ?1 AND source_id = ?2",
            params![&source_kind, &source_id],
        )?;
        Ok(())
    }

    pub fn list_model_source_mapping_preferences(
        &self,
        source_kind: &str,
        source_id: &str,
    ) -> Result<Vec<ModelSourceMappingPreference>> {
        let source_kind = normalize_text(source_kind);
        let source_id = normalize_text(source_id);
        if source_kind.is_empty() || source_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare(
            "SELECT source_kind, source_id, upstream_model, preference, updated_at
             FROM model_source_mapping_preferences
             WHERE source_kind = ?1 AND source_id = ?2",
        )?;
        let rows = stmt.query_map(params![&source_kind, &source_id], map_preference)?;
        rows.collect()
    }

    pub fn delete_model_source_routes_for_platform_model(
        &self,
        platform_model_slug: &str,
    ) -> Result<()> {
        let slug = normalize_text(platform_model_slug);
        if slug.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "DELETE FROM model_source_mappings
             WHERE platform_model_slug = ?1",
            params![&slug],
        )?;
        Ok(())
    }
}

fn map_preference(row: &Row<'_>) -> Result<ModelSourceMappingPreference> {
    Ok(ModelSourceMappingPreference {
        source_kind: row.get(0)?,
        source_id: row.get(1)?,
        upstream_model: row.get(2)?,
        preference: row.get(3)?,
        updated_at: row.get(4)?,
    })
}
