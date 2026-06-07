use std::collections::BTreeMap;

use rusqlite::params;
use serde_json::Value;

use crate::rpc::types::{ModelInfo, ModelReasoningLevel};

use super::{
    now_ts, ModelCatalogModelRecord, ModelCatalogReasoningLevelRecord, ModelCatalogScopeRecord,
    ModelCatalogStringItemRecord, Storage,
};

const STRING_ITEM_KIND_ADDITIONAL_SPEED_TIERS: &str = "additional_speed_tiers";
const STRING_ITEM_KIND_EXPERIMENTAL_SUPPORTED_TOOLS: &str = "experimental_supported_tools";
const STRING_ITEM_KIND_INPUT_MODALITIES: &str = "input_modalities";
const STRING_ITEM_KIND_AVAILABLE_IN_PLANS: &str = "available_in_plans";

impl Storage {
    pub fn upsert_model_catalog_scope(
        &self,
        record: &ModelCatalogScopeRecord,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO model_catalog_scopes (scope, extra_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(scope) DO UPDATE SET
               extra_json = excluded.extra_json,
               updated_at = excluded.updated_at",
            params![record.scope, record.extra_json, record.updated_at],
        )?;
        Ok(())
    }

    pub fn get_model_catalog_scope(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Option<ModelCatalogScopeRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT scope, extra_json, updated_at
             FROM model_catalog_scopes
             WHERE scope = ?1
             LIMIT 1",
        )?;
        let mut rows = stmt.query([scope])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(ModelCatalogScopeRecord {
                scope: row.get(0)?,
                extra_json: row.get(1)?,
                updated_at: row.get(2)?,
            }));
        }
        Ok(None)
    }

    /// 函数 `upsert_model_catalog_models`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-12
    ///
    /// # 参数
    /// - self: 参数 self
    /// - models: 参数 models
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn upsert_model_catalog_models(
        &self,
        models: &[ModelCatalogModelRecord],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for model in models {
            tx.execute(
                "INSERT INTO model_catalog_models (
                    scope, slug, display_name, source_kind, user_edited,
                    description, default_reasoning_level, shell_type, visibility, supported_in_api, priority,
                    availability_nux_json, upgrade_json, base_instructions,
                    model_messages_json, supports_reasoning_summaries,
                    default_reasoning_summary, support_verbosity,
                    default_verbosity_json, apply_patch_tool_type,
                    web_search_tool_type, truncation_mode, truncation_limit,
                    truncation_extra_json, supports_parallel_tool_calls,
                    supports_image_detail_original, context_window,
                    auto_compact_token_limit, effective_context_window_percent,
                    minimal_client_version_json, supports_search_tool,
                    extra_json, sort_index, updated_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                    ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34
                 )
                 ON CONFLICT(scope, slug) DO UPDATE SET
                    display_name = excluded.display_name,
                    source_kind = excluded.source_kind,
                    user_edited = excluded.user_edited,
                    description = excluded.description,
                    default_reasoning_level = excluded.default_reasoning_level,
                    shell_type = excluded.shell_type,
                    visibility = excluded.visibility,
                    supported_in_api = excluded.supported_in_api,
                    priority = excluded.priority,
                    availability_nux_json = excluded.availability_nux_json,
                    upgrade_json = excluded.upgrade_json,
                    base_instructions = excluded.base_instructions,
                    model_messages_json = excluded.model_messages_json,
                    supports_reasoning_summaries = excluded.supports_reasoning_summaries,
                    default_reasoning_summary = excluded.default_reasoning_summary,
                    support_verbosity = excluded.support_verbosity,
                    default_verbosity_json = excluded.default_verbosity_json,
                    apply_patch_tool_type = excluded.apply_patch_tool_type,
                    web_search_tool_type = excluded.web_search_tool_type,
                    truncation_mode = excluded.truncation_mode,
                    truncation_limit = excluded.truncation_limit,
                    truncation_extra_json = excluded.truncation_extra_json,
                    supports_parallel_tool_calls = excluded.supports_parallel_tool_calls,
                    supports_image_detail_original = excluded.supports_image_detail_original,
                    context_window = excluded.context_window,
                    auto_compact_token_limit = excluded.auto_compact_token_limit,
                    effective_context_window_percent = excluded.effective_context_window_percent,
                    minimal_client_version_json = excluded.minimal_client_version_json,
                    supports_search_tool = excluded.supports_search_tool,
                    extra_json = excluded.extra_json,
                    sort_index = excluded.sort_index,
                    updated_at = excluded.updated_at",
                params![
                    model.scope,
                    model.slug,
                    model.display_name,
                    model.source_kind,
                    model.user_edited,
                    model.description,
                    model.default_reasoning_level,
                    model.shell_type,
                    model.visibility,
                    model.supported_in_api,
                    model.priority,
                    model.availability_nux_json,
                    model.upgrade_json,
                    model.base_instructions,
                    model.model_messages_json,
                    model.supports_reasoning_summaries,
                    model.default_reasoning_summary,
                    model.support_verbosity,
                    model.default_verbosity_json,
                    model.apply_patch_tool_type,
                    model.web_search_tool_type,
                    model.truncation_mode,
                    model.truncation_limit,
                    model.truncation_extra_json,
                    model.supports_parallel_tool_calls,
                    model.supports_image_detail_original,
                    model.context_window,
                    model.auto_compact_token_limit,
                    model.effective_context_window_percent,
                    model.minimal_client_version_json,
                    model.supports_search_tool,
                    model.extra_json,
                    model.sort_index,
                    model.updated_at,
                ],
            )?;
        }
        tx.commit()?;
        self.sync_default_model_group_models_from_catalog()?;
        Ok(())
    }

    fn sync_default_model_group_models_from_catalog(&self) -> rusqlite::Result<()> {
        if !self.has_table("model_groups")? || !self.has_table("model_group_models")? {
            return Ok(());
        }

        self.prune_default_model_group_models_not_in_catalog()?;
        let now = now_ts();
        self.conn.execute(
            "INSERT OR IGNORE INTO model_group_models (
                group_id, platform_model_slug, enabled, rate_multiplier_millis,
                billing_model_slug, note, created_at, updated_at
             )
             SELECT g.id, m.slug, 1, NULL, NULL, 'catalog_sync', ?1, ?1
             FROM model_groups g
             JOIN model_catalog_models m
               ON m.scope = 'default'
              AND COALESCE(m.supported_in_api, 1) = 1
              AND TRIM(m.slug) <> ''
             WHERE g.is_default = 1",
            params![now],
        )?;
        Ok(())
    }

    /// 函数 `list_model_catalog_models`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-12
    ///
    /// # 参数
    /// - self: 参数 self
    /// - scope: 参数 scope
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn list_model_catalog_models(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Vec<ModelCatalogModelRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                scope, slug, display_name, source_kind, user_edited,
                description, default_reasoning_level, shell_type, visibility, supported_in_api, priority,
                availability_nux_json, upgrade_json, base_instructions,
                model_messages_json, supports_reasoning_summaries,
                default_reasoning_summary, support_verbosity,
                default_verbosity_json, apply_patch_tool_type,
                web_search_tool_type, truncation_mode, truncation_limit,
                truncation_extra_json, supports_parallel_tool_calls,
                supports_image_detail_original, context_window,
                auto_compact_token_limit, effective_context_window_percent,
                minimal_client_version_json, supports_search_tool,
                extra_json, sort_index, updated_at
             FROM model_catalog_models
             WHERE scope = ?1
             ORDER BY sort_index ASC, updated_at DESC, slug ASC",
        )?;
        let rows = stmt.query_map([scope], |row| {
            Ok(ModelCatalogModelRecord {
                scope: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                source_kind: row.get(3)?,
                user_edited: row.get(4)?,
                description: row.get(5)?,
                default_reasoning_level: row.get(6)?,
                shell_type: row.get(7)?,
                visibility: row.get(8)?,
                supported_in_api: row.get(9)?,
                priority: row.get(10)?,
                availability_nux_json: row.get(11)?,
                upgrade_json: row.get(12)?,
                base_instructions: row.get(13)?,
                model_messages_json: row.get(14)?,
                supports_reasoning_summaries: row.get(15)?,
                default_reasoning_summary: row.get(16)?,
                support_verbosity: row.get(17)?,
                default_verbosity_json: row.get(18)?,
                apply_patch_tool_type: row.get(19)?,
                web_search_tool_type: row.get(20)?,
                truncation_mode: row.get(21)?,
                truncation_limit: row.get(22)?,
                truncation_extra_json: row.get(23)?,
                supports_parallel_tool_calls: row.get(24)?,
                supports_image_detail_original: row.get(25)?,
                context_window: row.get(26)?,
                auto_compact_token_limit: row.get(27)?,
                effective_context_window_percent: row.get(28)?,
                minimal_client_version_json: row.get(29)?,
                supports_search_tool: row.get(30)?,
                extra_json: row.get(31)?,
                sort_index: row.get(32)?,
                updated_at: row.get(33)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn delete_model_catalog_model(&self, scope: &str, slug: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM model_catalog_models WHERE scope = ?1 AND slug = ?2",
            params![scope, slug],
        )?;
        Ok(())
    }

    pub fn upsert_model_catalog_reasoning_levels(
        &self,
        levels: &[ModelCatalogReasoningLevelRecord],
    ) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for level in levels {
            tx.execute(
                "INSERT INTO model_catalog_reasoning_levels (
                    scope, slug, effort, description, extra_json, sort_index, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(scope, slug, effort) DO UPDATE SET
                    description = excluded.description,
                    extra_json = excluded.extra_json,
                    sort_index = excluded.sort_index,
                    updated_at = excluded.updated_at",
                params![
                    level.scope,
                    level.slug,
                    level.effort,
                    level.description,
                    level.extra_json,
                    level.sort_index,
                    level.updated_at,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_model_catalog_reasoning_levels(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Vec<ModelCatalogReasoningLevelRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT scope, slug, effort, description, extra_json, sort_index, updated_at
             FROM model_catalog_reasoning_levels
             WHERE scope = ?1
             ORDER BY slug ASC, sort_index ASC, effort ASC",
        )?;
        let rows = stmt.query_map([scope], |row| {
            Ok(ModelCatalogReasoningLevelRecord {
                scope: row.get(0)?,
                slug: row.get(1)?,
                effort: row.get(2)?,
                description: row.get(3)?,
                extra_json: row.get(4)?,
                sort_index: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    pub fn delete_model_catalog_reasoning_levels(
        &self,
        scope: &str,
        slug: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM model_catalog_reasoning_levels WHERE scope = ?1 AND slug = ?2",
            params![scope, slug],
        )?;
        Ok(())
    }

    pub fn upsert_model_catalog_additional_speed_tiers(
        &self,
        items: &[ModelCatalogStringItemRecord],
    ) -> rusqlite::Result<()> {
        self.upsert_model_catalog_string_items(STRING_ITEM_KIND_ADDITIONAL_SPEED_TIERS, items)
    }

    pub fn list_model_catalog_additional_speed_tiers(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Vec<ModelCatalogStringItemRecord>> {
        self.list_model_catalog_string_items(STRING_ITEM_KIND_ADDITIONAL_SPEED_TIERS, scope)
    }

    pub fn upsert_model_catalog_experimental_supported_tools(
        &self,
        items: &[ModelCatalogStringItemRecord],
    ) -> rusqlite::Result<()> {
        self.upsert_model_catalog_string_items(STRING_ITEM_KIND_EXPERIMENTAL_SUPPORTED_TOOLS, items)
    }

    pub fn list_model_catalog_experimental_supported_tools(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Vec<ModelCatalogStringItemRecord>> {
        self.list_model_catalog_string_items(STRING_ITEM_KIND_EXPERIMENTAL_SUPPORTED_TOOLS, scope)
    }

    pub fn upsert_model_catalog_input_modalities(
        &self,
        items: &[ModelCatalogStringItemRecord],
    ) -> rusqlite::Result<()> {
        self.upsert_model_catalog_string_items(STRING_ITEM_KIND_INPUT_MODALITIES, items)
    }

    pub fn list_model_catalog_input_modalities(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Vec<ModelCatalogStringItemRecord>> {
        self.list_model_catalog_string_items(STRING_ITEM_KIND_INPUT_MODALITIES, scope)
    }

    pub fn upsert_model_catalog_available_in_plans(
        &self,
        items: &[ModelCatalogStringItemRecord],
    ) -> rusqlite::Result<()> {
        self.upsert_model_catalog_string_items(STRING_ITEM_KIND_AVAILABLE_IN_PLANS, items)
    }

    pub fn list_model_catalog_available_in_plans(
        &self,
        scope: &str,
    ) -> rusqlite::Result<Vec<ModelCatalogStringItemRecord>> {
        self.list_model_catalog_string_items(STRING_ITEM_KIND_AVAILABLE_IN_PLANS, scope)
    }

    pub fn delete_model_catalog_string_items(
        &self,
        scope: &str,
        slug: &str,
        item_kind: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM model_catalog_string_items
             WHERE scope = ?1 AND slug = ?2 AND item_kind = ?3",
            params![scope, slug, item_kind],
        )?;
        Ok(())
    }

    fn upsert_model_catalog_string_items(
        &self,
        item_kind: &str,
        items: &[ModelCatalogStringItemRecord],
    ) -> rusqlite::Result<()> {
        let sql =
            "INSERT INTO model_catalog_string_items (scope, slug, item_kind, value, sort_index, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(scope, slug, item_kind, value) DO UPDATE SET
               sort_index = excluded.sort_index,
               updated_at = excluded.updated_at";
        let tx = self.conn.unchecked_transaction()?;
        for item in items {
            tx.execute(
                sql,
                params![
                    item.scope,
                    item.slug,
                    item_kind,
                    item.value,
                    item.sort_index,
                    item.updated_at
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn list_model_catalog_string_items(
        &self,
        item_kind: &str,
        scope: &str,
    ) -> rusqlite::Result<Vec<ModelCatalogStringItemRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT scope, slug, value, sort_index, updated_at
             FROM model_catalog_string_items
             WHERE scope = ?1 AND item_kind = ?2
             ORDER BY slug ASC, sort_index ASC, value ASC",
        )?;
        let rows = stmt.query_map(params![scope, item_kind], |row| {
            Ok(ModelCatalogStringItemRecord {
                scope: row.get(0)?,
                slug: row.get(1)?,
                value: row.get(2)?,
                sort_index: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }

    fn ensure_model_catalog_child_tables(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_model_catalog_models_scope_sort
               ON model_catalog_models(scope, sort_index, slug);
             CREATE INDEX IF NOT EXISTS idx_model_catalog_models_scope_supported_in_api
               ON model_catalog_models(scope, supported_in_api, sort_index, slug);
             CREATE INDEX IF NOT EXISTS idx_model_catalog_models_scope_visibility
               ON model_catalog_models(scope, visibility, sort_index, slug);
             CREATE TABLE IF NOT EXISTS model_catalog_reasoning_levels (
                scope TEXT NOT NULL,
                slug TEXT NOT NULL,
                effort TEXT NOT NULL,
                description TEXT NOT NULL,
                extra_json TEXT NOT NULL DEFAULT '{}',
                sort_index INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (scope, slug, effort)
             );
             CREATE INDEX IF NOT EXISTS idx_model_catalog_reasoning_levels_scope_sort
               ON model_catalog_reasoning_levels(scope, slug, sort_index, effort);
             CREATE TABLE IF NOT EXISTS model_catalog_string_items (
                scope TEXT NOT NULL,
                slug TEXT NOT NULL,
                item_kind TEXT NOT NULL,
                value TEXT NOT NULL,
                sort_index INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (scope, slug, item_kind, value)
             );
             CREATE INDEX IF NOT EXISTS idx_model_catalog_string_items_scope_kind_sort
               ON model_catalog_string_items(scope, item_kind, slug, sort_index, value);",
        )?;
        self.conn.execute_batch(
            "DROP TABLE IF EXISTS model_catalog_additional_speed_tiers;
             DROP TABLE IF EXISTS model_catalog_experimental_supported_tools;
             DROP TABLE IF EXISTS model_catalog_input_modalities;
             DROP TABLE IF EXISTS model_catalog_available_in_plans;",
        )?;
        Ok(())
    }

    fn rebuild_model_catalog_models_without_legacy_json(&self) -> rusqlite::Result<()> {
        if !self.has_table("model_catalog_models")?
            || !self.has_column("model_catalog_models", "model_json")?
        {
            return Ok(());
        }

        self.ensure_column("model_catalog_models", "description", "TEXT")?;
        self.ensure_column(
            "model_catalog_models",
            "source_kind",
            "TEXT NOT NULL DEFAULT 'remote'",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "user_edited",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("model_catalog_models", "default_reasoning_level", "TEXT")?;
        self.ensure_column("model_catalog_models", "shell_type", "TEXT")?;
        self.ensure_column("model_catalog_models", "visibility", "TEXT")?;
        self.ensure_column("model_catalog_models", "supported_in_api", "INTEGER")?;
        self.ensure_column("model_catalog_models", "priority", "INTEGER")?;
        self.ensure_column("model_catalog_models", "availability_nux_json", "TEXT")?;
        self.ensure_column("model_catalog_models", "upgrade_json", "TEXT")?;
        self.ensure_column("model_catalog_models", "base_instructions", "TEXT")?;
        self.ensure_column("model_catalog_models", "model_messages_json", "TEXT")?;
        self.ensure_column(
            "model_catalog_models",
            "supports_reasoning_summaries",
            "INTEGER",
        )?;
        self.ensure_column("model_catalog_models", "default_reasoning_summary", "TEXT")?;
        self.ensure_column("model_catalog_models", "support_verbosity", "INTEGER")?;
        self.ensure_column("model_catalog_models", "default_verbosity_json", "TEXT")?;
        self.ensure_column("model_catalog_models", "apply_patch_tool_type", "TEXT")?;
        self.ensure_column("model_catalog_models", "web_search_tool_type", "TEXT")?;
        self.ensure_column("model_catalog_models", "truncation_mode", "TEXT")?;
        self.ensure_column("model_catalog_models", "truncation_limit", "INTEGER")?;
        self.ensure_column("model_catalog_models", "truncation_extra_json", "TEXT")?;
        self.ensure_column(
            "model_catalog_models",
            "supports_parallel_tool_calls",
            "INTEGER",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "supports_image_detail_original",
            "INTEGER",
        )?;
        self.ensure_column("model_catalog_models", "context_window", "INTEGER")?;
        self.ensure_column(
            "model_catalog_models",
            "auto_compact_token_limit",
            "INTEGER",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "effective_context_window_percent",
            "INTEGER",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "minimal_client_version_json",
            "TEXT",
        )?;
        self.ensure_column("model_catalog_models", "supports_search_tool", "INTEGER")?;
        self.ensure_column(
            "model_catalog_models",
            "extra_json",
            "TEXT NOT NULL DEFAULT '{}'",
        )?;
        self.ensure_model_catalog_child_tables()?;

        let legacy_rows = self.read_legacy_model_catalog_model_rows()?;
        let mut model_rows = Vec::new();
        let mut reasoning_rows = Vec::new();
        let mut additional_speed_tiers = Vec::new();
        let mut experimental_supported_tools = Vec::new();
        let mut input_modalities = Vec::new();
        let mut available_in_plans = Vec::new();

        for row in legacy_rows {
            let parsed = row
                .model_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<ModelInfo>(raw).ok());
            model_rows.push(model_record_from_legacy_row(&row, parsed.as_ref())?);
            if let Some(model) = parsed.as_ref() {
                reasoning_rows.extend(reasoning_records_from_model(
                    &row.scope,
                    &model.slug,
                    &model.supported_reasoning_levels,
                    row.updated_at,
                )?);
                additional_speed_tiers.extend(string_records_from_values(
                    &row.scope,
                    &model.slug,
                    &model.additional_speed_tiers,
                    row.updated_at,
                ));
                experimental_supported_tools.extend(string_records_from_values(
                    &row.scope,
                    &model.slug,
                    &model.experimental_supported_tools,
                    row.updated_at,
                ));
                input_modalities.extend(string_records_from_values(
                    &row.scope,
                    &model.slug,
                    &model.input_modalities,
                    row.updated_at,
                ));
                available_in_plans.extend(string_records_from_values(
                    &row.scope,
                    &model.slug,
                    &model.available_in_plans,
                    row.updated_at,
                ));
            }
        }

        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DROP TABLE IF EXISTS model_catalog_models_rebuilt", [])?;
        tx.execute(
            "CREATE TABLE model_catalog_models_rebuilt (
                scope TEXT NOT NULL,
                slug TEXT NOT NULL,
                display_name TEXT NOT NULL,
                source_kind TEXT NOT NULL DEFAULT 'remote',
                user_edited INTEGER NOT NULL DEFAULT 0,
                description TEXT,
                default_reasoning_level TEXT,
                shell_type TEXT,
                visibility TEXT,
                supported_in_api INTEGER,
                priority INTEGER,
                availability_nux_json TEXT,
                upgrade_json TEXT,
                base_instructions TEXT,
                model_messages_json TEXT,
                supports_reasoning_summaries INTEGER,
                default_reasoning_summary TEXT,
                support_verbosity INTEGER,
                default_verbosity_json TEXT,
                apply_patch_tool_type TEXT,
                web_search_tool_type TEXT,
                truncation_mode TEXT,
                truncation_limit INTEGER,
                truncation_extra_json TEXT,
                supports_parallel_tool_calls INTEGER,
                supports_image_detail_original INTEGER,
                context_window INTEGER,
                auto_compact_token_limit INTEGER,
                effective_context_window_percent INTEGER,
                minimal_client_version_json TEXT,
                supports_search_tool INTEGER,
                extra_json TEXT NOT NULL DEFAULT '{}',
                sort_index INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (scope, slug)
            )",
            [],
        )?;
        for model in &model_rows {
            tx.execute(
                "INSERT INTO model_catalog_models_rebuilt (
                    scope, slug, display_name, source_kind, user_edited,
                    description, default_reasoning_level, shell_type, visibility, supported_in_api, priority,
                    availability_nux_json, upgrade_json, base_instructions,
                    model_messages_json, supports_reasoning_summaries,
                    default_reasoning_summary, support_verbosity,
                    default_verbosity_json, apply_patch_tool_type,
                    web_search_tool_type, truncation_mode, truncation_limit,
                    truncation_extra_json, supports_parallel_tool_calls,
                    supports_image_detail_original, context_window,
                    auto_compact_token_limit, effective_context_window_percent,
                    minimal_client_version_json, supports_search_tool,
                    extra_json, sort_index, updated_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24,
                    ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34
                )",
                params![
                    model.scope,
                    model.slug,
                    model.display_name,
                    model.source_kind,
                    model.user_edited,
                    model.description,
                    model.default_reasoning_level,
                    model.shell_type,
                    model.visibility,
                    model.supported_in_api,
                    model.priority,
                    model.availability_nux_json,
                    model.upgrade_json,
                    model.base_instructions,
                    model.model_messages_json,
                    model.supports_reasoning_summaries,
                    model.default_reasoning_summary,
                    model.support_verbosity,
                    model.default_verbosity_json,
                    model.apply_patch_tool_type,
                    model.web_search_tool_type,
                    model.truncation_mode,
                    model.truncation_limit,
                    model.truncation_extra_json,
                    model.supports_parallel_tool_calls,
                    model.supports_image_detail_original,
                    model.context_window,
                    model.auto_compact_token_limit,
                    model.effective_context_window_percent,
                    model.minimal_client_version_json,
                    model.supports_search_tool,
                    model.extra_json,
                    model.sort_index,
                    model.updated_at,
                ],
            )?;
        }
        tx.execute("DROP TABLE model_catalog_models", [])?;
        tx.execute(
            "ALTER TABLE model_catalog_models_rebuilt RENAME TO model_catalog_models",
            [],
        )?;
        tx.commit()?;

        self.ensure_model_catalog_child_tables()?;
        self.upsert_model_catalog_reasoning_levels(&reasoning_rows)?;
        self.upsert_model_catalog_additional_speed_tiers(&additional_speed_tiers)?;
        self.upsert_model_catalog_experimental_supported_tools(&experimental_supported_tools)?;
        self.upsert_model_catalog_input_modalities(&input_modalities)?;
        self.upsert_model_catalog_available_in_plans(&available_in_plans)?;
        Ok(())
    }

    /// 函数 `ensure_model_catalog_models_table`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-12
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    pub(super) fn ensure_model_catalog_models_table(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(include_str!(
            "../../migrations/047_model_catalog_models.sql"
        ))?;
        self.rebuild_model_catalog_models_without_legacy_json()?;
        self.ensure_column("model_catalog_models", "description", "TEXT")?;
        self.ensure_column(
            "model_catalog_models",
            "source_kind",
            "TEXT NOT NULL DEFAULT 'remote'",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "user_edited",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        self.ensure_column("model_catalog_models", "default_reasoning_level", "TEXT")?;
        self.ensure_column("model_catalog_models", "shell_type", "TEXT")?;
        self.ensure_column("model_catalog_models", "visibility", "TEXT")?;
        self.ensure_column("model_catalog_models", "supported_in_api", "INTEGER")?;
        self.ensure_column("model_catalog_models", "priority", "INTEGER")?;
        self.ensure_column("model_catalog_models", "availability_nux_json", "TEXT")?;
        self.ensure_column("model_catalog_models", "upgrade_json", "TEXT")?;
        self.ensure_column("model_catalog_models", "base_instructions", "TEXT")?;
        self.ensure_column("model_catalog_models", "model_messages_json", "TEXT")?;
        self.ensure_column(
            "model_catalog_models",
            "supports_reasoning_summaries",
            "INTEGER",
        )?;
        self.ensure_column("model_catalog_models", "default_reasoning_summary", "TEXT")?;
        self.ensure_column("model_catalog_models", "support_verbosity", "INTEGER")?;
        self.ensure_column("model_catalog_models", "default_verbosity_json", "TEXT")?;
        self.ensure_column("model_catalog_models", "apply_patch_tool_type", "TEXT")?;
        self.ensure_column("model_catalog_models", "web_search_tool_type", "TEXT")?;
        self.ensure_column("model_catalog_models", "truncation_mode", "TEXT")?;
        self.ensure_column("model_catalog_models", "truncation_limit", "INTEGER")?;
        self.ensure_column("model_catalog_models", "truncation_extra_json", "TEXT")?;
        self.ensure_column(
            "model_catalog_models",
            "supports_parallel_tool_calls",
            "INTEGER",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "supports_image_detail_original",
            "INTEGER",
        )?;
        self.ensure_column("model_catalog_models", "context_window", "INTEGER")?;
        self.ensure_column(
            "model_catalog_models",
            "auto_compact_token_limit",
            "INTEGER",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "effective_context_window_percent",
            "INTEGER",
        )?;
        self.ensure_column(
            "model_catalog_models",
            "minimal_client_version_json",
            "TEXT",
        )?;
        self.ensure_column("model_catalog_models", "supports_search_tool", "INTEGER")?;
        self.ensure_column(
            "model_catalog_models",
            "extra_json",
            "TEXT NOT NULL DEFAULT '{}'",
        )?;
        self.ensure_model_catalog_child_tables()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
struct LegacyModelCatalogModelRow {
    scope: String,
    slug: String,
    display_name: String,
    source_kind: Option<String>,
    user_edited: Option<bool>,
    description: Option<String>,
    default_reasoning_level: Option<String>,
    shell_type: Option<String>,
    visibility: Option<String>,
    supported_in_api: Option<bool>,
    priority: Option<i64>,
    availability_nux_json: Option<String>,
    upgrade_json: Option<String>,
    base_instructions: Option<String>,
    model_messages_json: Option<String>,
    supports_reasoning_summaries: Option<bool>,
    default_reasoning_summary: Option<String>,
    support_verbosity: Option<bool>,
    default_verbosity_json: Option<String>,
    apply_patch_tool_type: Option<String>,
    web_search_tool_type: Option<String>,
    truncation_mode: Option<String>,
    truncation_limit: Option<i64>,
    truncation_extra_json: Option<String>,
    supports_parallel_tool_calls: Option<bool>,
    supports_image_detail_original: Option<bool>,
    context_window: Option<i64>,
    auto_compact_token_limit: Option<i64>,
    effective_context_window_percent: Option<i64>,
    minimal_client_version_json: Option<String>,
    supports_search_tool: Option<bool>,
    extra_json: Option<String>,
    model_json: Option<String>,
    sort_index: i64,
    updated_at: i64,
}

impl Storage {
    fn read_legacy_model_catalog_model_rows(
        &self,
    ) -> rusqlite::Result<Vec<LegacyModelCatalogModelRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                scope, slug, display_name, source_kind, user_edited,
                description, default_reasoning_level, shell_type, visibility, supported_in_api, priority,
                availability_nux_json, upgrade_json, base_instructions,
                model_messages_json, supports_reasoning_summaries,
                default_reasoning_summary, support_verbosity,
                default_verbosity_json, apply_patch_tool_type,
                web_search_tool_type, truncation_mode, truncation_limit,
                truncation_extra_json, supports_parallel_tool_calls,
                supports_image_detail_original, context_window,
                auto_compact_token_limit, effective_context_window_percent,
                minimal_client_version_json, supports_search_tool,
                extra_json, model_json, sort_index, updated_at
             FROM model_catalog_models
             ORDER BY scope ASC, sort_index ASC, updated_at DESC, slug ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LegacyModelCatalogModelRow {
                scope: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                source_kind: row.get(3)?,
                user_edited: row.get(4)?,
                description: row.get(5)?,
                default_reasoning_level: row.get(6)?,
                shell_type: row.get(7)?,
                visibility: row.get(8)?,
                supported_in_api: row.get(9)?,
                priority: row.get(10)?,
                availability_nux_json: row.get(11)?,
                upgrade_json: row.get(12)?,
                base_instructions: row.get(13)?,
                model_messages_json: row.get(14)?,
                supports_reasoning_summaries: row.get(15)?,
                default_reasoning_summary: row.get(16)?,
                support_verbosity: row.get(17)?,
                default_verbosity_json: row.get(18)?,
                apply_patch_tool_type: row.get(19)?,
                web_search_tool_type: row.get(20)?,
                truncation_mode: row.get(21)?,
                truncation_limit: row.get(22)?,
                truncation_extra_json: row.get(23)?,
                supports_parallel_tool_calls: row.get(24)?,
                supports_image_detail_original: row.get(25)?,
                context_window: row.get(26)?,
                auto_compact_token_limit: row.get(27)?,
                effective_context_window_percent: row.get(28)?,
                minimal_client_version_json: row.get(29)?,
                supports_search_tool: row.get(30)?,
                extra_json: row.get(31)?,
                model_json: row.get(32)?,
                sort_index: row.get(33)?,
                updated_at: row.get(34)?,
            })
        })?;
        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }
        Ok(items)
    }
}

fn model_record_from_legacy_row(
    row: &LegacyModelCatalogModelRow,
    parsed: Option<&ModelInfo>,
) -> rusqlite::Result<ModelCatalogModelRecord> {
    let truncation = parsed.and_then(|model| model.truncation_policy.clone());
    let parsed_extra = parsed
        .map(|model| serialize_extra_map(&model.extra))
        .transpose()?
        .unwrap_or_else(|| "{}".to_string());
    Ok(ModelCatalogModelRecord {
        scope: row.scope.clone(),
        slug: row.slug.clone(),
        display_name: if row.display_name.trim().is_empty() {
            parsed
                .map(|model| model.display_name.clone())
                .unwrap_or_else(|| row.slug.clone())
        } else {
            row.display_name.clone()
        },
        source_kind: row
            .source_kind
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "remote".to_string()),
        user_edited: row.user_edited.unwrap_or(false),
        description: row
            .description
            .clone()
            .or_else(|| parsed.and_then(|model| model.description.clone())),
        default_reasoning_level: row
            .default_reasoning_level
            .clone()
            .or_else(|| parsed.and_then(|model| model.default_reasoning_level.clone())),
        shell_type: row
            .shell_type
            .clone()
            .or_else(|| parsed.and_then(|model| model.shell_type.clone())),
        visibility: row
            .visibility
            .clone()
            .or_else(|| parsed.and_then(|model| model.visibility.clone())),
        supported_in_api: row
            .supported_in_api
            .or_else(|| parsed.map(|model| model.supported_in_api)),
        priority: row.priority.or_else(|| parsed.map(|model| model.priority)),
        availability_nux_json: choose_json_text(
            row.availability_nux_json.as_deref(),
            parsed.and_then(|model| model.availability_nux.as_ref()),
        )?,
        upgrade_json: choose_json_text(
            row.upgrade_json.as_deref(),
            parsed.and_then(|model| model.upgrade.as_ref()),
        )?,
        base_instructions: row
            .base_instructions
            .clone()
            .or_else(|| parsed.and_then(|model| model.base_instructions.clone())),
        model_messages_json: choose_json_text(
            row.model_messages_json.as_deref(),
            parsed.and_then(|model| model.model_messages.as_ref()),
        )?,
        supports_reasoning_summaries: row
            .supports_reasoning_summaries
            .or_else(|| parsed.and_then(|model| model.supports_reasoning_summaries)),
        default_reasoning_summary: row
            .default_reasoning_summary
            .clone()
            .or_else(|| parsed.and_then(|model| model.default_reasoning_summary.clone())),
        support_verbosity: row
            .support_verbosity
            .or_else(|| parsed.and_then(|model| model.support_verbosity)),
        default_verbosity_json: choose_json_text(
            row.default_verbosity_json.as_deref(),
            parsed.and_then(|model| model.default_verbosity.as_ref()),
        )?,
        apply_patch_tool_type: row
            .apply_patch_tool_type
            .clone()
            .or_else(|| parsed.and_then(|model| model.apply_patch_tool_type.clone())),
        web_search_tool_type: row
            .web_search_tool_type
            .clone()
            .or_else(|| parsed.and_then(|model| model.web_search_tool_type.clone())),
        truncation_mode: row
            .truncation_mode
            .clone()
            .or_else(|| truncation.as_ref().map(|policy| policy.mode.clone())),
        truncation_limit: row
            .truncation_limit
            .or_else(|| truncation.as_ref().map(|policy| policy.limit)),
        truncation_extra_json: choose_extra_text(
            row.truncation_extra_json.as_deref(),
            truncation.as_ref().map(|policy| &policy.extra),
        )?,
        supports_parallel_tool_calls: row
            .supports_parallel_tool_calls
            .or_else(|| parsed.and_then(|model| model.supports_parallel_tool_calls)),
        supports_image_detail_original: row
            .supports_image_detail_original
            .or_else(|| parsed.and_then(|model| model.supports_image_detail_original)),
        context_window: row
            .context_window
            .or_else(|| parsed.and_then(|model| model.context_window)),
        auto_compact_token_limit: row
            .auto_compact_token_limit
            .or_else(|| parsed.and_then(|model| model.auto_compact_token_limit)),
        effective_context_window_percent: row
            .effective_context_window_percent
            .or_else(|| parsed.and_then(|model| model.effective_context_window_percent)),
        minimal_client_version_json: choose_json_text(
            row.minimal_client_version_json.as_deref(),
            parsed.and_then(|model| model.minimal_client_version.as_ref()),
        )?,
        supports_search_tool: row
            .supports_search_tool
            .or_else(|| parsed.and_then(|model| model.supports_search_tool)),
        extra_json: row
            .extra_json
            .clone()
            .filter(|text| !text.trim().is_empty())
            .unwrap_or(parsed_extra),
        sort_index: row.sort_index,
        updated_at: row.updated_at,
    })
}

fn reasoning_records_from_model(
    scope: &str,
    slug: &str,
    levels: &[ModelReasoningLevel],
    updated_at: i64,
) -> rusqlite::Result<Vec<ModelCatalogReasoningLevelRecord>> {
    let mut records = Vec::new();
    for (index, level) in levels.iter().enumerate() {
        records.push(ModelCatalogReasoningLevelRecord {
            scope: scope.to_string(),
            slug: slug.to_string(),
            effort: level.effort.clone(),
            description: level.description.clone(),
            extra_json: serialize_extra_map(&level.extra)?,
            sort_index: index as i64,
            updated_at,
        });
    }
    Ok(records)
}

fn string_records_from_values(
    scope: &str,
    slug: &str,
    values: &[String],
    updated_at: i64,
) -> Vec<ModelCatalogStringItemRecord> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| ModelCatalogStringItemRecord {
            scope: scope.to_string(),
            slug: slug.to_string(),
            value: value.clone(),
            sort_index: index as i64,
            updated_at,
        })
        .collect()
}

fn choose_json_text(
    existing: Option<&str>,
    fallback: Option<&Value>,
) -> rusqlite::Result<Option<String>> {
    match existing {
        Some(text) if !text.trim().is_empty() => Ok(Some(text.to_string())),
        _ => fallback
            .map(|value| {
                serde_json::to_string(value)
                    .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
            })
            .transpose(),
    }
}

fn choose_extra_text(
    existing: Option<&str>,
    fallback: Option<&BTreeMap<String, Value>>,
) -> rusqlite::Result<Option<String>> {
    match existing {
        Some(text) if !text.trim().is_empty() => Ok(Some(text.to_string())),
        _ => fallback.map(serialize_extra_map).transpose(),
    }
}

fn serialize_extra_map(extra: &BTreeMap<String, Value>) -> rusqlite::Result<String> {
    serde_json::to_string(extra)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
}
