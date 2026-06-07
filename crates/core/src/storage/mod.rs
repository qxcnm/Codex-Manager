use rusqlite::{Connection, Result};
use std::path::Path;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

mod account_manager;
mod account_metadata;
mod account_subscriptions;
mod accounts;
mod aggregate_apis;
mod api_key_quota_limits;
mod api_keys;
mod conversation_bindings;
mod events;
mod key_id_filters;
mod model_groups;
mod model_options;
mod model_price_rules;
mod model_sources;
mod plugins;
mod quota_pools;
mod request_log_query;
mod request_logs;
mod request_token_stats;
mod settings;
mod tokens;
mod usage;

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub issuer: String,
    pub chatgpt_account_id: Option<String>,
    pub workspace_id: Option<String>,
    pub group_name: Option<String>,
    pub sort: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct AccountMetadata {
    pub account_id: String,
    pub note: Option<String>,
    pub tags: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct AccountSubscription {
    pub account_id: String,
    pub has_subscription: bool,
    pub account_plan_type: Option<String>,
    pub plan_type: Option<String>,
    pub expires_at: Option<i64>,
    pub renews_at: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct QuotaSourceModelAssignment {
    pub source_kind: String,
    pub source_id: String,
    pub model_slug: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ModelSourceModel {
    pub source_kind: String,
    pub source_id: String,
    pub upstream_model: String,
    pub display_name: Option<String>,
    pub status: String,
    pub discovery_kind: String,
    pub last_synced_at: Option<i64>,
    pub extra_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ModelSourceMapping {
    pub id: String,
    pub platform_model_slug: String,
    pub source_kind: String,
    pub source_id: String,
    pub upstream_model: String,
    pub enabled: bool,
    pub priority: i64,
    pub weight: i64,
    pub billing_model_slug: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ModelSourceMappingPreference {
    pub source_kind: String,
    pub source_id: String,
    pub upstream_model: String,
    pub preference: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct AccountQuotaCapacityTemplate {
    pub plan_type: String,
    pub primary_window_tokens: Option<i64>,
    pub secondary_window_tokens: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct AccountQuotaCapacityOverride {
    pub account_id: String,
    pub primary_window_tokens: Option<i64>,
    pub secondary_window_tokens: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub account_id: String,
    pub id_token: String,
    pub access_token: String,
    pub refresh_token: String,
    pub api_key_access_token: Option<String>,
    pub last_refresh: i64,
}

#[derive(Debug, Clone)]
pub struct LoginSession {
    pub login_id: String,
    pub code_verifier: String,
    pub state: String,
    pub status: String,
    pub error: Option<String>,
    pub workspace_id: Option<String>,
    pub note: Option<String>,
    pub tags: Option<String>,
    pub group_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct UsageSnapshotRecord {
    pub account_id: String,
    pub used_percent: Option<f64>,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub credits_json: Option<String>,
    pub captured_at: i64,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub account_id: Option<String>,
    pub event_type: String,
    pub message: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct ConversationBinding {
    pub platform_key_hash: String,
    pub conversation_id: String,
    pub account_id: String,
    pub thread_epoch: i64,
    pub thread_anchor: String,
    pub status: String,
    pub last_model: Option<String>,
    pub last_switch_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_used_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RequestLog {
    pub trace_id: Option<String>,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub initial_account_id: Option<String>,
    pub attempted_account_ids_json: Option<String>,
    pub initial_aggregate_api_id: Option<String>,
    pub attempted_aggregate_api_ids_json: Option<String>,
    pub request_path: String,
    pub original_path: Option<String>,
    pub adapted_path: Option<String>,
    pub method: String,
    pub request_type: Option<String>,
    pub gateway_mode: Option<String>,
    pub transparent_mode: Option<bool>,
    pub enhanced_mode: Option<bool>,
    pub model: Option<String>,
    pub upstream_model: Option<String>,
    pub actual_source_kind: Option<String>,
    pub actual_source_id: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub effective_service_tier: Option<String>,
    pub response_adapter: Option<String>,
    pub upstream_url: Option<String>,
    pub aggregate_api_supplier_name: Option<String>,
    pub aggregate_api_url: Option<String>,
    pub status_code: Option<i64>,
    pub duration_ms: Option<i64>,
    pub first_response_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RequestTokenStat {
    pub request_log_id: i64,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub estimated_cost_usd: Option<f64>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct RequestLogTodaySummary {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct RequestLogQuerySummary {
    pub count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct ApiKeyTokenUsageSummary {
    pub key_id: String,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsageSummary {
    pub model: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ApiKeyModelTokenUsageSummary {
    pub key_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsageRollup {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct DailyTokenUsageRollup {
    pub day_start_ts: i64,
    pub day_end_ts: i64,
    pub usage: TokenUsageRollup,
}

#[derive(Debug, Clone, Default)]
pub struct UserTokenUsageRollup {
    pub user_id: String,
    pub usage: TokenUsageRollup,
}

#[derive(Debug, Clone, Default)]
pub struct SourceTokenUsageRollup {
    pub source_kind: String,
    pub source_id: String,
    pub usage: TokenUsageRollup,
}

#[derive(Debug, Clone)]
pub struct AppUser {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub password_hash: String,
    pub role: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_login_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AppUserSession {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub expires_at: i64,
    pub created_at: i64,
    pub last_seen_at: Option<i64>,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AppProject {
    pub id: String,
    pub name: String,
    pub owner_user_id: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct AppWallet {
    pub id: String,
    pub owner_kind: String,
    pub owner_id: String,
    pub balance_credit_micros: i64,
    pub frozen_credit_micros: i64,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct AppWalletLedgerEntry {
    pub id: String,
    pub wallet_id: String,
    pub entry_kind: String,
    pub amount_credit_micros: i64,
    pub balance_after_credit_micros: i64,
    pub request_log_id: Option<i64>,
    pub api_key_id: Option<String>,
    pub pricing_rule_id: Option<String>,
    pub raw_usage_json: Option<String>,
    pub note: Option<String>,
    pub created_by_user_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct ApiKeyOwner {
    pub key_id: String,
    pub owner_kind: String,
    pub owner_user_id: Option<String>,
    pub project_id: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct BillingRule {
    pub id: String,
    pub name: String,
    pub status: String,
    pub priority: i64,
    pub multiplier_millis: i64,
    pub model_pattern: Option<String>,
    pub service_tier: Option<String>,
    pub user_id: Option<String>,
    pub project_id: Option<String>,
    pub api_key_id: Option<String>,
    pub starts_at: Option<i64>,
    pub ends_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ModelGroup {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub sort: i64,
    pub is_default: bool,
    pub rate_multiplier_millis: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ModelGroupModel {
    pub group_id: String,
    pub platform_model_slug: String,
    pub enabled: bool,
    pub rate_multiplier_millis: Option<i64>,
    pub billing_model_slug: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct UserModelGroup {
    pub user_id: String,
    pub group_id: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ModelGroupAccess {
    pub group_id: String,
    pub group_name: String,
    pub platform_model_slug: String,
    pub rate_multiplier_millis: i64,
    pub billing_model_slug: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModelPriceRule {
    pub id: String,
    pub provider: String,
    pub model_pattern: String,
    pub match_type: String,
    pub billing_mode: String,
    pub currency: String,
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
    pub source: String,
    pub source_url: Option<String>,
    pub seed_version: Option<String>,
    pub enabled: bool,
    pub priority: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct ApiKey {
    pub id: String,
    pub name: Option<String>,
    pub model_slug: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub rotation_strategy: String,
    pub aggregate_api_id: Option<String>,
    pub account_plan_filter: Option<String>,
    pub aggregate_api_url: Option<String>,
    pub client_type: String,
    pub protocol_type: String,
    pub auth_scheme: String,
    pub upstream_base_url: Option<String>,
    pub static_headers_json: Option<String>,
    pub key_hash: String,
    pub status: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct AggregateApi {
    pub id: String,
    pub provider_type: String,
    pub supplier_name: Option<String>,
    pub sort: i64,
    pub url: String,
    pub auth_type: String,
    pub auth_params_json: Option<String>,
    pub action: Option<String>,
    pub model_override: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_test_at: Option<i64>,
    pub last_test_status: Option<String>,
    pub last_test_error: Option<String>,
    pub balance_query_enabled: bool,
    pub balance_query_template: Option<String>,
    pub balance_query_base_url: Option<String>,
    pub balance_query_user_id: Option<String>,
    pub balance_query_config_json: Option<String>,
    pub last_balance_at: Option<i64>,
    pub last_balance_status: Option<String>,
    pub last_balance_error: Option<String>,
    pub last_balance_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AggregateApiSupplierModel {
    pub supplier_key: String,
    pub provider_type: String,
    pub upstream_model: String,
    pub display_name: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct PluginInstall {
    pub plugin_id: String,
    pub source_url: Option<String>,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage_url: Option<String>,
    pub script_url: Option<String>,
    pub script_body: String,
    pub permissions_json: String,
    pub manifest_json: String,
    pub status: String,
    pub installed_at: i64,
    pub updated_at: i64,
    pub last_run_at: Option<i64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginTask {
    pub id: String,
    pub plugin_id: String,
    pub name: String,
    pub description: Option<String>,
    pub entrypoint: String,
    pub schedule_kind: String,
    pub interval_seconds: Option<i64>,
    pub enabled: bool,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub task_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct PluginRunLog {
    pub id: Option<i64>,
    pub plugin_id: String,
    pub task_id: Option<String>,
    pub run_type: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub output_json: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ModelCatalogScopeRecord {
    pub scope: String,
    pub extra_json: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelCatalogModelRecord {
    pub scope: String,
    pub slug: String,
    pub display_name: String,
    pub source_kind: String,
    pub user_edited: bool,
    pub description: Option<String>,
    pub default_reasoning_level: Option<String>,
    pub shell_type: Option<String>,
    pub visibility: Option<String>,
    pub supported_in_api: Option<bool>,
    pub priority: Option<i64>,
    pub availability_nux_json: Option<String>,
    pub upgrade_json: Option<String>,
    pub base_instructions: Option<String>,
    pub model_messages_json: Option<String>,
    pub supports_reasoning_summaries: Option<bool>,
    pub default_reasoning_summary: Option<String>,
    pub support_verbosity: Option<bool>,
    pub default_verbosity_json: Option<String>,
    pub apply_patch_tool_type: Option<String>,
    pub web_search_tool_type: Option<String>,
    pub truncation_mode: Option<String>,
    pub truncation_limit: Option<i64>,
    pub truncation_extra_json: Option<String>,
    pub supports_parallel_tool_calls: Option<bool>,
    pub supports_image_detail_original: Option<bool>,
    pub context_window: Option<i64>,
    pub auto_compact_token_limit: Option<i64>,
    pub effective_context_window_percent: Option<i64>,
    pub minimal_client_version_json: Option<String>,
    pub supports_search_tool: Option<bool>,
    pub extra_json: String,
    pub sort_index: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelCatalogReasoningLevelRecord {
    pub scope: String,
    pub slug: String,
    pub effort: String,
    pub description: String,
    pub extra_json: String,
    pub sort_index: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ModelCatalogStringItemRecord {
    pub scope: String,
    pub slug: String,
    pub value: String,
    pub sort_index: i64,
    pub updated_at: i64,
}

#[derive(Debug)]
pub struct Storage {
    conn: Connection,
}

impl Storage {
    /// 函数 `open`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - path: 参数 path
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        // 中文注释：并发写入时给 SQLite 一点等待时间，避免瞬时 lock 导致请求直接失败。
        conn.busy_timeout(Duration::from_millis(3000))?;
        // 中文注释：文件库启用 WAL + NORMAL，可明显降低并发读写互斥开销；
        // 仅在 open(path) 上设置，避免影响 open_in_memory 的行为预期。
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )?;
        Ok(Self { conn })
    }

    /// 函数 `open_in_memory`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// 无
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.busy_timeout(Duration::from_millis(3000))?;
        Ok(Self { conn })
    }

    /// 函数 `init`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn init(&self) -> Result<()> {
        self.ensure_migrations_table()?;

        self.apply_sql_migration("001_init", include_str!("../../migrations/001_init.sql"))?;
        self.apply_sql_migration(
            "002_login_sessions",
            include_str!("../../migrations/002_login_sessions.sql"),
        )?;
        self.apply_sql_migration(
            "003_api_keys",
            include_str!("../../migrations/003_api_keys.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "004_api_key_model",
            include_str!("../../migrations/004_api_key_model.sql"),
            |s| s.ensure_api_key_model_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "005_request_logs",
            include_str!("../../migrations/005_request_logs.sql"),
            |s| s.ensure_request_logs_table(),
        )?;
        self.apply_sql_migration(
            "006_usage_snapshots_latest_index",
            include_str!("../../migrations/006_usage_snapshots_latest_index.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "007_usage_secondary_columns",
            include_str!("../../migrations/007_usage_secondary_columns.sql"),
            |s| s.ensure_usage_secondary_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "008_token_api_key_access_token",
            include_str!("../../migrations/008_token_api_key_access_token.sql"),
            |s| s.ensure_token_api_key_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "009_api_key_reasoning_effort",
            include_str!("../../migrations/009_api_key_reasoning_effort.sql"),
            |s| s.ensure_api_key_reasoning_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "010_request_log_reasoning_effort",
            include_str!("../../migrations/010_request_log_reasoning_effort.sql"),
            |s| s.ensure_request_log_reasoning_column(),
        )?;

        // 中文注释：先走 SQL 迁移，遇到历史库重复列冲突再回退 compat；不这样写会导致老库和新库长期两套机制并存。
        self.apply_sql_or_compat_migration(
            "011_account_meta_columns",
            include_str!("../../migrations/011_account_meta_columns.sql"),
            |s| s.ensure_account_meta_columns(),
        )?;
        self.apply_sql_migration(
            "012_request_logs_search_indexes",
            include_str!("../../migrations/012_request_logs_search_indexes.sql"),
        )?;
        self.apply_sql_migration(
            "013_drop_accounts_note_tags",
            include_str!("../../migrations/013_drop_accounts_note_tags.sql"),
        )?;
        self.apply_sql_migration(
            "014_drop_accounts_workspace_name",
            include_str!("../../migrations/014_drop_accounts_workspace_name.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "015_api_key_profiles",
            include_str!("../../migrations/015_api_key_profiles.sql"),
            |s| s.ensure_api_key_profiles_table(),
        )?;
        self.apply_sql_migration(
            "016_api_keys_key_hash_index",
            include_str!("../../migrations/016_api_keys_key_hash_index.sql"),
        )?;
        self.apply_sql_migration(
            "017_usage_snapshots_captured_id_index",
            include_str!("../../migrations/017_usage_snapshots_captured_id_index.sql"),
        )?;
        self.apply_sql_migration(
            "018_accounts_sort_updated_at_index",
            include_str!("../../migrations/018_accounts_sort_updated_at_index.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "019_api_key_secrets",
            include_str!("../../migrations/019_api_key_secrets.sql"),
            |s| s.ensure_api_key_secrets_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "020_request_logs_account_tokens_cost",
            include_str!("../../migrations/020_request_logs_account_tokens_cost.sql"),
            |s| s.ensure_request_log_account_tokens_cost_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "021_request_logs_cached_reasoning_tokens",
            include_str!("../../migrations/021_request_logs_cached_reasoning_tokens.sql"),
            |s| s.ensure_request_log_cached_reasoning_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "022_request_token_stats",
            include_str!("../../migrations/022_request_token_stats.sql"),
            |s| s.ensure_request_token_stats_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "023_request_token_stats_total_tokens",
            include_str!("../../migrations/023_request_token_stats_total_tokens.sql"),
            |s| s.ensure_request_token_stats_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "025_tokens_refresh_schedule",
            include_str!("../../migrations/025_tokens_refresh_schedule.sql"),
            |s| s.ensure_token_refresh_schedule_columns(),
        )?;
        self.apply_sql_migration(
            "026_api_key_profiles_constraints_azure",
            include_str!("../../migrations/026_api_key_profiles_constraints_azure.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "027_request_logs_trace_context",
            include_str!("../../migrations/027_request_logs_trace_context.sql"),
            |s| s.ensure_request_log_trace_context_columns(),
        )?;
        // 中文注释：旧版 request_logs 里遗留的 token 字段，需要先回填到 request_token_stats，
        // 再做表瘦身；否则压缩后会丢失历史 token 统计。
        self.ensure_request_token_stats_table()?;
        self.apply_compat_migration("028_request_logs_drop_legacy_usage_columns", |s| {
            s.compact_request_logs_legacy_usage_columns()
        })?;
        self.apply_sql_migration(
            "029_app_settings",
            include_str!("../../migrations/029_app_settings.sql"),
        )?;
        self.apply_sql_migration(
            "030_accounts_scale_indexes",
            include_str!("../../migrations/030_accounts_scale_indexes.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "031_request_logs_duration_ms",
            include_str!("../../migrations/031_request_logs_duration_ms.sql"),
            |s| s.ensure_request_log_duration_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "032_request_logs_attempt_chain",
            include_str!("../../migrations/032_request_logs_attempt_chain.sql"),
            |s| s.ensure_request_log_attempt_chain_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "033_login_sessions_workspace_id",
            include_str!("../../migrations/033_login_sessions_workspace_id.sql"),
            |s| s.ensure_login_session_workspace_column(),
        )?;
        self.apply_sql_migration(
            "034_conversation_bindings",
            include_str!("../../migrations/034_conversation_bindings.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "035_api_key_profiles_service_tier",
            include_str!("../../migrations/035_api_key_profiles_service_tier.sql"),
            |s| s.ensure_api_key_service_tier_column(),
        )?;
        self.apply_sql_migration(
            "036_accounts_metadata_and_drop_group_name",
            include_str!("../../migrations/036_accounts_metadata_and_drop_group_name.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "037_aggregate_api_routing",
            include_str!("../../migrations/037_aggregate_api_routing.sql"),
            |s| {
                s.ensure_api_key_rotation_columns()?;
                s.ensure_aggregate_apis_table()?;
                s.ensure_aggregate_api_secrets_table()
            },
        )?;
        self.apply_sql_or_compat_migration(
            "038_request_logs_aggregate_api_context",
            include_str!("../../migrations/038_request_logs_aggregate_api_context.sql"),
            |s| s.ensure_request_log_aggregate_api_context_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "039_request_logs_aggregate_api_attempt_chain",
            include_str!("../../migrations/039_request_logs_aggregate_api_attempt_chain.sql"),
            |s| s.ensure_request_log_aggregate_api_attempt_chain_columns(),
        )?;
        self.apply_sql_migration(
            "040_plugins",
            include_str!("../../migrations/040_plugins.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "042_request_logs_request_type_service_tier",
            include_str!("../../migrations/042_request_logs_request_type_service_tier.sql"),
            |s| s.ensure_request_log_request_type_and_service_tier_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "043_request_logs_effective_service_tier",
            include_str!("../../migrations/043_request_logs_effective_service_tier.sql"),
            |s| s.ensure_request_log_effective_service_tier_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "044_api_keys_account_plan_filter",
            include_str!("../../migrations/044_api_keys_account_plan_filter.sql"),
            |s| s.ensure_api_key_rotation_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "045_accounts_preferred",
            include_str!("../../migrations/045_accounts_preferred.sql"),
            |s| s.ensure_account_meta_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "046_request_logs_gateway_mode",
            include_str!("../../migrations/046_request_logs_gateway_mode.sql"),
            |s| s.ensure_request_log_request_type_and_service_tier_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "047_model_catalog_models",
            include_str!("../../migrations/047_model_catalog_models.sql"),
            |s| s.ensure_model_catalog_models_table(),
        )?;
        self.apply_sql_migration(
            "048_drop_model_options_cache",
            include_str!("../../migrations/048_drop_model_options_cache.sql"),
        )?;
        self.apply_model_catalog_string_items_migration()?;
        self.ensure_model_catalog_models_table()?;
        self.apply_sql_migration(
            "050_api_key_profiles_drop_azure_protocol",
            include_str!("../../migrations/050_api_key_profiles_drop_azure_protocol.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "051_request_logs_first_response_ms",
            include_str!("../../migrations/051_request_logs_first_response_ms.sql"),
            |s| s.ensure_request_log_first_response_column(),
        )?;
        self.apply_sql_or_compat_migration(
            "052_account_subscriptions",
            include_str!("../../migrations/052_account_subscriptions.sql"),
            |s| s.ensure_account_subscriptions_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "053_aggregate_api_model_override",
            include_str!("../../migrations/053_aggregate_api_model_override.sql"),
            |s| s.ensure_aggregate_apis_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "053_api_key_quota_limits",
            include_str!("../../migrations/053_api_key_quota_limits.sql"),
            |s| s.ensure_api_key_quota_limits_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "054_aggregate_api_balance_query",
            include_str!("../../migrations/054_aggregate_api_balance_query.sql"),
            |s| {
                s.ensure_aggregate_apis_table()?;
                s.ensure_aggregate_api_balance_secrets_table()
            },
        )?;
        self.apply_sql_or_compat_migration(
            "055_model_price_rules",
            include_str!("../../migrations/055_model_price_rules.sql"),
            |s| s.ensure_model_price_rules_table(),
        )?;
        self.apply_sql_or_compat_migration(
            "056_quota_pools",
            include_str!("../../migrations/056_quota_pools.sql"),
            |s| s.ensure_quota_pool_tables(),
        )?;
        self.apply_sql_or_compat_migration(
            "057_account_manager",
            include_str!("../../migrations/057_account_manager.sql"),
            |s| s.ensure_account_manager_tables(),
        )?;
        self.apply_sql_or_compat_migration(
            "058_model_source_mappings",
            include_str!("../../migrations/058_model_source_mappings.sql"),
            |s| s.ensure_model_source_tables(),
        )?;
        self.apply_sql_or_compat_migration(
            "059_aggregate_api_supplier_models",
            include_str!("../../migrations/059_aggregate_api_supplier_models.sql"),
            |s| s.ensure_aggregate_api_supplier_model_tables(),
        )?;
        self.apply_sql_or_compat_migration(
            "060_request_logs_route_details",
            include_str!("../../migrations/060_request_logs_route_details.sql"),
            |s| s.ensure_request_log_route_detail_columns(),
        )?;
        self.apply_sql_or_compat_migration(
            "061_model_groups",
            include_str!("../../migrations/061_model_groups.sql"),
            |s| s.ensure_model_group_tables(),
        )?;
        self.apply_compat_migration("062_observability_storage_compaction", |s| {
            s.compact_observability_storage_for_existing_databases()
        })?;
        self.apply_compat_migration("063_account_subscriptions_account_plan_type", |s| {
            s.ensure_account_subscriptions_table()
        })?;
        self.apply_sql_migration(
            "064_drop_gateway_error_logs",
            include_str!("../../migrations/064_drop_gateway_error_logs.sql"),
        )?;
        self.apply_sql_or_compat_migration(
            "065_model_source_mapping_preferences",
            include_str!("../../migrations/065_model_source_mapping_preferences.sql"),
            |s| s.ensure_model_source_tables(),
        )?;
        self.ensure_api_key_rotation_columns()?;
        self.ensure_aggregate_apis_table()?;
        self.ensure_aggregate_api_supplier_model_tables()?;
        self.ensure_aggregate_api_secrets_table()?;
        self.ensure_aggregate_api_balance_secrets_table()?;
        self.ensure_api_key_quota_limits_table()?;
        self.ensure_model_price_rules_table()?;
        self.ensure_request_token_stats_table()?;
        self.ensure_request_log_request_type_and_service_tier_columns()?;
        self.ensure_request_log_effective_service_tier_column()?;
        self.ensure_request_log_first_response_column()?;
        self.ensure_request_log_route_detail_columns()?;
        self.ensure_model_catalog_models_table()?;
        self.ensure_account_subscriptions_table()?;
        self.ensure_quota_pool_tables()?;
        self.ensure_account_manager_tables()?;
        self.ensure_model_source_tables()?;
        self.ensure_aggregate_api_supplier_model_tables()?;
        self.ensure_model_group_tables()?;
        Ok(())
    }

    fn compact_observability_storage_for_existing_databases(&self) -> Result<()> {
        self.ensure_request_token_stats_table()?;
        self.ensure_request_logs_table()?;
        self.ensure_usage_secondary_columns()?;

        let now = now_ts();
        let mut touched = 0_usize;
        if let Some(cutoff) = request_token_stats::retention_cutoff(
            now,
            request_token_stats::request_token_stats_retain_days(),
        ) {
            touched = touched.saturating_add(self.rollup_request_token_stats_before(cutoff)?);
        }
        touched = touched.saturating_add(self.prune_request_logs_by_retention(now)?);
        touched = touched.saturating_add(
            self.prune_usage_snapshots_all_accounts(usage::usage_snapshots_retain_per_account())?,
        );

        if touched > 0 {
            let _ = self
                .conn
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
        }
        Ok(())
    }

    fn apply_model_catalog_string_items_migration(&self) -> Result<()> {
        const VERSION: &str = "049_model_catalog_string_items";
        if self.has_migration(VERSION)? {
            return Ok(());
        }

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS model_catalog_string_items (
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

        for (legacy_table, item_kind) in [
            (
                "model_catalog_additional_speed_tiers",
                "additional_speed_tiers",
            ),
            (
                "model_catalog_experimental_supported_tools",
                "experimental_supported_tools",
            ),
            ("model_catalog_input_modalities", "input_modalities"),
            ("model_catalog_available_in_plans", "available_in_plans"),
        ] {
            if self.has_table(legacy_table)? {
                let sql = format!(
                    "INSERT OR REPLACE INTO model_catalog_string_items
                        (scope, slug, item_kind, value, sort_index, updated_at)
                     SELECT scope, slug, '{item_kind}', value, sort_index, updated_at
                     FROM {legacy_table};"
                );
                self.conn.execute_batch(&sql)?;
            }
        }

        self.conn.execute_batch(
            "DROP TABLE IF EXISTS model_catalog_additional_speed_tiers;
             DROP TABLE IF EXISTS model_catalog_experimental_supported_tools;
             DROP TABLE IF EXISTS model_catalog_input_modalities;
             DROP TABLE IF EXISTS model_catalog_available_in_plans;",
        )?;
        self.mark_migration(VERSION)
    }

    /// 函数 `insert_login_session`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - session: 参数 session
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn insert_login_session(&self, session: &LoginSession) -> Result<()> {
        self.conn.execute(
            "INSERT INTO login_sessions (login_id, code_verifier, state, status, error, workspace_id, note, tags, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            (
                &session.login_id,
                &session.code_verifier,
                &session.state,
                &session.status,
                &session.error,
                &session.workspace_id,
                &session.note,
                &session.tags,
                session.created_at,
                session.updated_at,
            ),
        )?;
        Ok(())
    }

    /// 函数 `get_login_session`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - login_id: 参数 login_id
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn get_login_session(&self, login_id: &str) -> Result<Option<LoginSession>> {
        let mut stmt = self.conn.prepare(
            "SELECT login_id, code_verifier, state, status, error, workspace_id, note, tags, created_at, updated_at FROM login_sessions WHERE login_id = ?1",
        )?;
        let mut rows = stmt.query([login_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(LoginSession {
                login_id: row.get(0)?,
                code_verifier: row.get(1)?,
                state: row.get(2)?,
                status: row.get(3)?,
                error: row.get(4)?,
                workspace_id: row.get(5)?,
                note: row.get(6)?,
                tags: row.get(7)?,
                group_name: None,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// 函数 `update_login_session_status`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - login_id: 参数 login_id
    /// - status: 参数 status
    /// - error: 参数 error
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_login_session_status(
        &self,
        login_id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE login_sessions SET status = ?1, error = ?2, updated_at = ?3 WHERE login_id = ?4",
            (status, error, now_ts(), login_id),
        )?;
        Ok(())
    }

    /// 函数 `update_login_session_code_verifier`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - login_id: 参数 login_id
    /// - code_verifier: 参数 code_verifier
    ///
    /// # 返回
    /// 返回函数执行结果
    pub fn update_login_session_code_verifier(
        &self,
        login_id: &str,
        code_verifier: &str,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE login_sessions SET code_verifier = ?1, updated_at = ?2 WHERE login_id = ?3",
            (code_verifier, now_ts(), login_id),
        )?;
        Ok(())
    }

    /// 函数 `ensure_column`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - table: 参数 table
    /// - column: 参数 column
    /// - column_type: 参数 column_type
    ///
    /// # 返回
    /// 返回函数执行结果
    fn ensure_column(&self, table: &str, column: &str, column_type: &str) -> Result<()> {
        if self.has_column(table, column)? {
            return Ok(());
        }
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}");
        self.conn.execute(&sql, [])?;
        Ok(())
    }

    /// 函数 `has_column`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - table: 参数 table
    /// - column: 参数 column
    ///
    /// # 返回
    /// 返回函数执行结果
    fn has_column(&self, table: &str, column: &str) -> Result<bool> {
        let sql = format!("PRAGMA table_info({table})");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            if name == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn has_table(&self, table: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
    }

    /// 函数 `ensure_migrations_table`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    ///
    /// # 返回
    /// 返回函数执行结果
    fn ensure_migrations_table(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version TEXT PRIMARY KEY,
                applied_at INTEGER NOT NULL
            )",
            [],
        )?;
        Ok(())
    }

    /// 函数 `has_migration`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - version: 参数 version
    ///
    /// # 返回
    /// 返回函数执行结果
    fn has_migration(&self, version: &str) -> Result<bool> {
        let mut stmt = self
            .conn
            .prepare("SELECT 1 FROM schema_migrations WHERE version = ?1 LIMIT 1")?;
        let mut rows = stmt.query([version])?;
        Ok(rows.next()?.is_some())
    }

    /// 函数 `mark_migration`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - version: 参数 version
    ///
    /// # 返回
    /// 返回函数执行结果
    fn mark_migration(&self, version: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            (version, now_ts()),
        )?;
        Ok(())
    }

    /// 函数 `apply_sql_migration`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - version: 参数 version
    /// - sql: 参数 sql
    ///
    /// # 返回
    /// 返回函数执行结果
    fn apply_sql_migration(&self, version: &str, sql: &str) -> Result<()> {
        if self.has_migration(version)? {
            return Ok(());
        }
        self.conn.execute_batch(sql)?;
        self.mark_migration(version)
    }

    /// 函数 `apply_sql_or_compat_migration`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - version: 参数 version
    /// - sql: 参数 sql
    /// - compat: 参数 compat
    ///
    /// # 返回
    /// 返回函数执行结果
    fn apply_sql_or_compat_migration<F>(&self, version: &str, sql: &str, compat: F) -> Result<()>
    where
        F: FnOnce(&Self) -> Result<()>,
    {
        if self.has_migration(version)? {
            return Ok(());
        }

        match self.conn.execute_batch(sql) {
            Ok(_) => {}
            Err(err) if Self::is_schema_conflict_error(&err) => {
                // 中文注释：历史库可能已通过旧版 ensure_* 加过列/表，不走 fallback 会让迁移在“重复列/表”上失败。
                compat(self)?;
            }
            Err(err) => return Err(err),
        }

        self.mark_migration(version)
    }

    /// 函数 `apply_compat_migration`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - version: 参数 version
    /// - compat: 参数 compat
    ///
    /// # 返回
    /// 返回函数执行结果
    fn apply_compat_migration<F>(&self, version: &str, compat: F) -> Result<()>
    where
        F: FnOnce(&Self) -> Result<()>,
    {
        if self.has_migration(version)? {
            return Ok(());
        }
        compat(self)?;
        self.mark_migration(version)
    }

    /// 函数 `is_schema_conflict_error`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - err: 参数 err
    ///
    /// # 返回
    /// 返回函数执行结果
    fn is_schema_conflict_error(err: &rusqlite::Error) -> bool {
        match err {
            rusqlite::Error::SqliteFailure(_, maybe_message) => maybe_message
                .as_deref()
                .map(|message| {
                    message.contains("duplicate column name") || message.contains("already exists")
                })
                .unwrap_or(false),
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/storage/migration_tests.rs"]
mod migration_tests;

/// 函数 `now_ts`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
