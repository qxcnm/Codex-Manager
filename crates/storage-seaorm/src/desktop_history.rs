//! Typed preservation schemas for imported desktop history and configuration.
//! Current Service behavior uses the V2 catalog; legacy catalog rows stay intact.

pub(crate) mod schema_migrations {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "schema_migrations")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub version: String,
        pub applied_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod conversation_bindings {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "conversation_bindings")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub platform_key_hash: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub conversation_id: String,
        #[sea_orm(column_type = "Text")]
        pub account_id: String,
        pub thread_epoch: i64,
        #[sea_orm(column_type = "Text")]
        pub thread_anchor: String,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_model: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_switch_reason: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub last_used_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_catalog_scopes {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_catalog_scopes")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub scope: String,
        #[sea_orm(column_type = "Text")]
        pub extra_json: String,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_catalog_models {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_catalog_models")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub scope: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub slug: String,
        #[sea_orm(column_type = "Text")]
        pub display_name: String,
        #[sea_orm(column_type = "Text")]
        pub source_kind: String,
        pub user_edited: bool,
        #[sea_orm(column_type = "Text", nullable)]
        pub description: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub default_reasoning_level: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub shell_type: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub visibility: Option<String>,
        pub supported_in_api: Option<bool>,
        pub priority: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub availability_nux_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub upgrade_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub base_instructions: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub model_messages_json: Option<String>,
        pub supports_reasoning_summaries: Option<bool>,
        #[sea_orm(column_type = "Text", nullable)]
        pub default_reasoning_summary: Option<String>,
        pub support_verbosity: Option<bool>,
        #[sea_orm(column_type = "Text", nullable)]
        pub default_verbosity_json: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub apply_patch_tool_type: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub web_search_tool_type: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub truncation_mode: Option<String>,
        pub truncation_limit: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub truncation_extra_json: Option<String>,
        pub supports_parallel_tool_calls: Option<bool>,
        pub supports_image_detail_original: Option<bool>,
        pub context_window: Option<i64>,
        pub auto_compact_token_limit: Option<i64>,
        pub effective_context_window_percent: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub minimal_client_version_json: Option<String>,
        pub supports_search_tool: Option<bool>,
        #[sea_orm(column_type = "Text")]
        pub extra_json: String,
        pub sort_index: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_catalog_reasoning_levels {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_catalog_reasoning_levels")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub scope: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub slug: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub effort: String,
        #[sea_orm(column_type = "Text")]
        pub description: String,
        #[sea_orm(column_type = "Text")]
        pub extra_json: String,
        pub sort_index: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_catalog_string_items {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_catalog_string_items")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(128))"
        )]
        pub scope: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub slug: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(64))"
        )]
        pub item_kind: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(384))"
        )]
        pub value: String,
        pub sort_index: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_price_rules {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_price_rules")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub provider: String,
        #[sea_orm(column_type = "Text")]
        pub model_pattern: String,
        #[sea_orm(column_type = "Text")]
        pub match_type: String,
        #[sea_orm(column_type = "Text")]
        pub billing_mode: String,
        #[sea_orm(column_type = "Text")]
        pub currency: String,
        #[sea_orm(column_type = "Text")]
        pub unit: String,
        pub input_price_per_1m: Option<f64>,
        pub cached_input_price_per_1m: Option<f64>,
        pub output_price_per_1m: Option<f64>,
        pub reasoning_output_price_per_1m: Option<f64>,
        pub cache_write_5m_price_per_1m: Option<f64>,
        pub cache_write_1h_price_per_1m: Option<f64>,
        pub cache_hit_price_per_1m: Option<f64>,
        pub long_context_threshold_tokens: Option<i64>,
        pub long_context_input_price_per_1m: Option<f64>,
        pub long_context_cached_input_price_per_1m: Option<f64>,
        pub long_context_output_price_per_1m: Option<f64>,
        #[sea_orm(column_type = "Text")]
        pub source: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub source_url: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub seed_version: Option<String>,
        pub enabled: bool,
        pub priority: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod quota_source_model_assignments {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "quota_source_model_assignments")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub source_kind: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub source_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub model_slug: String,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod account_quota_capacity_templates {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "account_quota_capacity_templates")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub plan_type: String,
        pub primary_window_tokens: Option<i64>,
        pub secondary_window_tokens: Option<i64>,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod app_projects {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "app_projects")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub name: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub owner_user_id: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod app_project_members {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "app_project_members")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub project_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub user_id: String,
        #[sea_orm(column_type = "Text")]
        pub role: String,
        pub created_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod redeem_code_batches {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "redeem_code_batches")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub name: String,
        pub value_credit_micros: i64,
        pub total_count: i64,
        pub expires_at: Option<i64>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub created_by_user_id: Option<String>,
        pub created_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod redeem_codes {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "redeem_codes")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub batch_id: String,
        #[sea_orm(column_type = "Text")]
        pub code_hash: String,
        pub max_redemptions: i64,
        pub redeemed_count: i64,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        pub expires_at: Option<i64>,
        pub created_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod redeem_records {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "redeem_records")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub code_id: String,
        #[sea_orm(column_type = "Text")]
        pub user_id: String,
        #[sea_orm(column_type = "Text")]
        pub wallet_id: String,
        pub amount_credit_micros: i64,
        #[sea_orm(column_type = "Text", nullable)]
        pub ledger_entry_id: Option<String>,
        pub redeemed_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_source_models {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_source_models")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub source_kind: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub source_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub upstream_model: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub display_name: Option<String>,
        #[sea_orm(column_type = "Text")]
        pub status: String,
        #[sea_orm(column_type = "Text")]
        pub discovery_kind: String,
        pub last_synced_at: Option<i64>,
        #[sea_orm(column_type = "Text")]
        pub extra_json: String,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_source_mappings {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_source_mappings")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub platform_model_slug: String,
        #[sea_orm(column_type = "Text")]
        pub source_kind: String,
        #[sea_orm(column_type = "Text")]
        pub source_id: String,
        #[sea_orm(column_type = "Text")]
        pub upstream_model: String,
        pub enabled: bool,
        pub priority: i64,
        pub weight: i64,
        #[sea_orm(column_type = "Text", nullable)]
        pub billing_model_slug: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_source_mapping_preferences {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_source_mapping_preferences")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub source_kind: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub source_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub upstream_model: String,
        #[sea_orm(column_type = "Text")]
        pub preference: String,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod model_catalog_v2_meta {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "model_catalog_v2_meta")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub key: String,
        #[sea_orm(column_type = "Text")]
        pub value: String,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod codex_skill_repositories {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "codex_skill_repositories")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(255))"
        )]
        pub id: String,
        #[sea_orm(column_type = "Text")]
        pub owner: String,
        #[sea_orm(column_type = "Text")]
        pub repository: String,
        #[sea_orm(column_type = "Text")]
        pub ref_name: String,
        pub enabled: bool,
        pub is_builtin: bool,
        #[sea_orm(column_type = "Text", nullable)]
        pub revision: Option<String>,
        pub last_scanned_at: Option<i64>,
        #[sea_orm(column_type = "Text", nullable)]
        pub last_error: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod codex_skill_repository_skills {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::StringLen;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "codex_skill_repository_skills")]
    pub struct Model {
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub repository_id: String,
        #[sea_orm(
            primary_key,
            auto_increment = false,
            column_type = "String(StringLen::N(191))"
        )]
        pub skill_id: String,
        #[sea_orm(column_type = "Text")]
        pub name: String,
        #[sea_orm(column_type = "Text")]
        pub description: String,
        #[sea_orm(column_type = "Text")]
        pub path: String,
        #[sea_orm(column_type = "Text")]
        pub source_url: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub revision: Option<String>,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) mod api_key_profiles {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, DeriveEntityModel)]
    #[sea_orm(table_name = "api_key_profiles")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub key_id: String,
        pub client_type: String,
        pub protocol_type: String,
        pub auth_scheme: String,
        #[sea_orm(column_type = "Text", nullable)]
        pub upstream_base_url: Option<String>,
        #[sea_orm(column_type = "Text", nullable)]
        pub static_headers_json: Option<String>,
        pub default_model: Option<String>,
        pub reasoning_effort: Option<String>,
        pub service_tier: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}

pub(crate) async fn migrate(db: &sea_orm::DatabaseConnection) -> Result<(), sea_orm::DbErr> {
    use sea_orm::ConnectionTrait;
    let backend = db.get_database_backend();
    let schema = sea_orm::Schema::new(backend);
    for mut table in [
        schema.create_table_from_entity(api_key_profiles::Entity),
        schema.create_table_from_entity(schema_migrations::Entity),
        schema.create_table_from_entity(conversation_bindings::Entity),
        schema.create_table_from_entity(model_catalog_scopes::Entity),
        schema.create_table_from_entity(model_catalog_models::Entity),
        schema.create_table_from_entity(model_catalog_reasoning_levels::Entity),
        schema.create_table_from_entity(model_catalog_string_items::Entity),
        schema.create_table_from_entity(model_price_rules::Entity),
        schema.create_table_from_entity(quota_source_model_assignments::Entity),
        schema.create_table_from_entity(account_quota_capacity_templates::Entity),
        schema.create_table_from_entity(app_projects::Entity),
        schema.create_table_from_entity(app_project_members::Entity),
        schema.create_table_from_entity(redeem_code_batches::Entity),
        schema.create_table_from_entity(redeem_codes::Entity),
        schema.create_table_from_entity(redeem_records::Entity),
        schema.create_table_from_entity(model_source_models::Entity),
        schema.create_table_from_entity(model_source_mappings::Entity),
        schema.create_table_from_entity(model_source_mapping_preferences::Entity),
        schema.create_table_from_entity(model_catalog_v2_meta::Entity),
        schema.create_table_from_entity(codex_skill_repositories::Entity),
        schema.create_table_from_entity(codex_skill_repository_skills::Entity),
    ] {
        table.if_not_exists();
        db.execute(backend.build(&table)).await?;
    }
    Ok(())
}
