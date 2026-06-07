use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Integer(i64),
}

impl fmt::Display for RequestId {
    /// 函数 `fmt`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - self: 参数 self
    /// - f: 参数 f
    ///
    /// # 返回
    /// 返回函数执行结果
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => f.write_str(value),
            Self::Integer(value) => write!(f, "{value}"),
        }
    }
}

impl From<i64> for RequestId {
    /// 函数 `from`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<i32> for RequestId {
    /// 函数 `from`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn from(value: i32) -> Self {
        Self::Integer(value as i64)
    }
}

impl From<u64> for RequestId {
    /// 函数 `from`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn from(value: u64) -> Self {
        Self::Integer(value as i64)
    }
}

impl From<u32> for RequestId {
    /// 函数 `from`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn from(value: u32) -> Self {
        Self::Integer(value as i64)
    }
}

impl From<usize> for RequestId {
    /// 函数 `from`
    ///
    /// 作者: gaohongshun
    ///
    /// 时间: 2026-04-02
    ///
    /// # 参数
    /// - value: 参数 value
    ///
    /// # 返回
    /// 返回函数执行结果
    fn from(value: usize) -> Self {
        Self::Integer(value as i64)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
    Error(JsonRpcError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub id: RequestId,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub id: RequestId,
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub error: JsonRpcErrorObject,
    pub id: RequestId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcErrorObject {
    pub code: i64,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub version: String,
    pub user_agent: String,
    pub codex_home: String,
    pub platform_family: String,
    pub platform_os: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub label: String,
    pub group_name: Option<String>,
    pub preferred: bool,
    pub sort: i64,
    pub status: String,
    pub status_reason: Option<String>,
    #[serde(default)]
    pub has_token: bool,
    pub plan_type: Option<String>,
    pub plan_type_raw: Option<String>,
    pub has_subscription: Option<bool>,
    pub subscription_plan: Option<String>,
    pub subscription_expires_at: Option<i64>,
    pub subscription_renews_at: Option<i64>,
    pub note: Option<String>,
    pub tags: Option<String>,
    #[serde(default)]
    pub model_slugs: Vec<String>,
    pub quota_capacity_primary_window_tokens: Option<i64>,
    pub quota_capacity_secondary_window_tokens: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountListResult {
    pub items: Vec<AccountSummary>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthInfo {
    pub user_code_url: String,
    pub token_url: String,
    pub verification_url: String,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LoginStartResult {
    #[serde(rename = "apiKey", rename_all = "camelCase")]
    ApiKey {},
    #[serde(rename = "chatgpt", rename_all = "camelCase")]
    Chatgpt { login_id: String, auth_url: String },
    #[serde(rename = "chatgptDeviceCode", rename_all = "camelCase")]
    ChatgptDeviceCode {
        login_id: String,
        verification_url: String,
        user_code: String,
    },
    #[serde(rename = "chatgptAuthTokens", rename_all = "camelCase")]
    ChatgptAuthTokens {},
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshotResult {
    pub account_id: Option<String>,
    pub availability_status: Option<String>,
    pub used_percent: Option<f64>,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    pub secondary_used_percent: Option<f64>,
    pub secondary_window_minutes: Option<i64>,
    pub secondary_resets_at: Option<i64>,
    pub credits_json: Option<String>,
    pub captured_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageReadResult {
    pub snapshot: Option<UsageSnapshotResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitWindowResult {
    pub used_percent: i64,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshotResult {
    pub limit_id: Option<String>,
    pub limit_name: Option<String>,
    pub primary: Option<RateLimitWindowResult>,
    pub secondary: Option<RateLimitWindowResult>,
    pub credits: Option<serde_json::Value>,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRateLimitsReadResult {
    pub rate_limits: RateLimitSnapshotResult,
    pub rate_limits_by_limit_id:
        Option<std::collections::BTreeMap<String, RateLimitSnapshotResult>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageListResult {
    pub items: Vec<UsageSnapshotResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageAggregateSummaryResult {
    pub primary_bucket_count: i64,
    pub primary_known_count: i64,
    pub primary_unknown_count: i64,
    pub primary_remain_percent: Option<i64>,
    pub secondary_bucket_count: i64,
    pub secondary_known_count: i64,
    pub secondary_unknown_count: i64,
    pub secondary_remain_percent: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySummary {
    pub id: String,
    pub name: Option<String>,
    pub model_slug: Option<String>,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub rotation_strategy: String,
    pub aggregate_api_id: Option<String>,
    pub account_plan_filter: Option<String>,
    pub aggregate_api_url: Option<String>,
    pub quota_limit_tokens: Option<i64>,
    pub client_type: String,
    pub protocol_type: String,
    pub auth_scheme: String,
    pub upstream_base_url: Option<String>,
    pub static_headers_json: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyListResult {
    pub items: Vec<ApiKeySummary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyUsageStatSummary {
    pub key_id: String,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyUsageStatListResult {
    pub items: Vec<ApiKeyUsageStatSummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaApiKeyOverviewResult {
    pub key_count: i64,
    pub limited_key_count: i64,
    pub total_limit_tokens: Option<i64>,
    pub total_used_tokens: i64,
    pub total_remaining_tokens: Option<i64>,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaAggregateApiOverviewResult {
    pub source_count: i64,
    pub enabled_balance_query_count: i64,
    pub ok_count: i64,
    pub error_count: i64,
    pub total_balance_usd: Option<f64>,
    pub last_refreshed_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaOpenAiAccountOverviewResult {
    pub account_count: i64,
    pub available_count: i64,
    pub low_quota_count: i64,
    pub primary_remain_percent: Option<i64>,
    pub secondary_remain_percent: Option<i64>,
    pub last_refreshed_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaTodayUsageResult {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaOverviewResult {
    pub api_key: QuotaApiKeyOverviewResult,
    pub aggregate_api: QuotaAggregateApiOverviewResult,
    pub openai_account: QuotaOpenAiAccountOverviewResult,
    pub today_usage: QuotaTodayUsageResult,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingRuleResult {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaBillingRulesResult {
    pub items: Vec<BillingRuleResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaModelUsageItem {
    pub model: String,
    pub provider: Option<String>,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub price_status: String,
    pub api_key_remaining_tokens: Option<i64>,
    pub aggregate_estimated_remaining_tokens: Option<i64>,
    pub aggregate_balance_usd: Option<f64>,
    pub openai_available_account_count: i64,
    pub openai_primary_remain_percent: Option<i64>,
    pub openai_secondary_remain_percent: Option<i64>,
    pub openai_estimated_remaining_tokens: Option<i64>,
    pub openai_estimate_enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaModelUsageResult {
    pub items: Vec<QuotaModelUsageItem>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaApiKeyModelUsageItem {
    pub model: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: Option<f64>,
    pub price_status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaApiKeyUsageItem {
    pub key_id: String,
    pub name: Option<String>,
    pub model_slug: Option<String>,
    pub quota_limit_tokens: Option<i64>,
    pub used_tokens: i64,
    pub remaining_tokens: Option<i64>,
    pub estimated_cost_usd: f64,
    pub models: Vec<QuotaApiKeyModelUsageItem>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaApiKeyUsageResult {
    pub items: Vec<QuotaApiKeyUsageItem>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSourceSummary {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub status: String,
    pub metric_kind: String,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub used: Option<f64>,
    pub unit: Option<String>,
    pub models: Vec<String>,
    pub provider: Option<String>,
    pub captured_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSourceListResult {
    pub items: Vec<QuotaSourceSummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaRefreshSourceResult {
    pub id: String,
    pub kind: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaRefreshSourcesResult {
    pub items: Vec<QuotaRefreshSourceResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSourceModelAssignmentResult {
    pub source_kind: String,
    pub source_id: String,
    pub model_slugs: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuotaCapacityTemplateResult {
    pub plan_type: String,
    pub primary_window_tokens: Option<i64>,
    pub secondary_window_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuotaCapacityOverrideResult {
    pub account_id: String,
    pub primary_window_tokens: Option<i64>,
    pub secondary_window_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCapacityConfigResult {
    pub source_assignments: Vec<QuotaSourceModelAssignmentResult>,
    pub templates: Vec<AccountQuotaCapacityTemplateResult>,
    pub account_overrides: Vec<AccountQuotaCapacityOverrideResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaPoolSourceBreakdown {
    pub source_kind: String,
    pub source_id: String,
    pub name: String,
    pub status: String,
    pub remaining_tokens: Option<i64>,
    pub raw_remaining: Option<f64>,
    pub raw_unit: Option<String>,
    pub models: Vec<String>,
    pub captured_at: Option<i64>,
    pub price_status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaModelPoolItem {
    pub model: String,
    pub provider: Option<String>,
    pub total_remaining_tokens: Option<i64>,
    pub aggregate_remaining_tokens: Option<i64>,
    pub account_primary_remaining_tokens: Option<i64>,
    pub account_secondary_remaining_tokens: Option<i64>,
    pub account_estimated_remaining_tokens: Option<i64>,
    pub source_count: i64,
    pub sources: Vec<QuotaPoolSourceBreakdown>,
    pub price_status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaModelPoolsResult {
    pub items: Vec<QuotaModelPoolItem>,
    pub templates: Vec<AccountQuotaCapacityTemplateResult>,
    pub account_overrides: Vec<AccountQuotaCapacityOverrideResult>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSystemPoolResult {
    pub reference_model: String,
    pub provider: Option<String>,
    pub total_remaining_tokens: Option<i64>,
    pub aggregate_remaining_tokens: Option<i64>,
    pub account_primary_remaining_tokens: Option<i64>,
    pub account_secondary_remaining_tokens: Option<i64>,
    pub account_estimated_remaining_tokens: Option<i64>,
    pub aggregate_source_count: i64,
    pub account_source_count: i64,
    pub unknown_source_count: i64,
    pub price_status: String,
    pub sources: Vec<QuotaPoolSourceBreakdown>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyCreateResult {
    pub id: String,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeySecretResult {
    pub id: String,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSummary {
    pub id: String,
    pub provider_type: String,
    pub supplier_name: Option<String>,
    pub sort: i64,
    pub url: String,
    pub auth_type: String,
    pub auth_params: Option<serde_json::Value>,
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
    #[serde(default)]
    pub model_slugs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage_url: Option<String>,
    pub script_url: Option<String>,
    pub script_body: Option<String>,
    pub permissions: Vec<String>,
    pub tasks: Vec<PluginCatalogTask>,
    pub manifest_version: String,
    pub category: Option<String>,
    pub runtime_kind: String,
    pub tags: Vec<String>,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogTask {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub entrypoint: String,
    pub schedule_kind: String,
    pub interval_seconds: Option<i64>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPluginSummary {
    pub plugin_id: String,
    pub source_url: Option<String>,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage_url: Option<String>,
    pub script_url: Option<String>,
    pub permissions: Vec<String>,
    pub status: String,
    pub installed_at: i64,
    pub updated_at: i64,
    pub last_run_at: Option<i64>,
    pub last_error: Option<String>,
    pub task_count: i64,
    pub enabled_task_count: i64,
    pub manifest_version: String,
    pub category: Option<String>,
    pub runtime_kind: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginTaskSummary {
    pub id: String,
    pub plugin_id: String,
    pub plugin_name: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRunLogSummary {
    pub id: i64,
    pub plugin_id: String,
    pub plugin_name: Option<String>,
    pub task_id: Option<String>,
    pub task_name: Option<String>,
    pub run_type: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AggregateApiListResult {
    pub items: Vec<AggregateApiSummary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiCreateResult {
    pub id: String,
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSecretResult {
    pub id: String,
    pub key: String,
    pub auth_type: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiTestResult {
    pub id: String,
    pub ok: bool,
    pub status_code: Option<i64>,
    pub message: Option<String>,
    pub tested_at: i64,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiBalanceSnapshot {
    pub is_valid: bool,
    pub invalid_message: Option<String>,
    pub remaining: Option<f64>,
    pub unit: Option<String>,
    pub plan_name: Option<String>,
    pub total: Option<f64>,
    pub used: Option<f64>,
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiBalanceRefreshResult {
    pub id: String,
    pub ok: bool,
    pub balance: Option<AggregateApiBalanceSnapshot>,
    pub message: Option<String>,
    pub queried_at: i64,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSupplierModelEntry {
    pub supplier_key: String,
    pub provider_type: String,
    pub upstream_model: String,
    pub display_name: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSupplierModelListResult {
    #[serde(default)]
    pub items: Vec<AggregateApiSupplierModelEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSupplierModelUpsertParams {
    pub supplier_key: String,
    pub provider_type: String,
    pub upstream_model: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSupplierModelDeleteParams {
    pub supplier_key: String,
    pub provider_type: String,
    pub upstream_model: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSupplierModelImportParams {
    pub api_id: String,
    #[serde(default)]
    pub supplier_key: Option<String>,
    #[serde(default)]
    pub provider_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateApiSupplierModelImportResult {
    pub imported: usize,
    #[serde(default)]
    pub items: Vec<ManagedModelSourceModelEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelsResponse {
    #[serde(default)]
    pub models: Vec<ModelInfo>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ModelsResponse {
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelCatalogEntry {
    #[serde(flatten)]
    pub model: ModelInfo,
    #[serde(default = "default_model_source_kind")]
    pub source_kind: String,
    #[serde(default)]
    pub user_edited: bool,
    #[serde(default)]
    pub sort_index: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelCatalogResult {
    #[serde(default)]
    pub items: Vec<ManagedModelCatalogEntry>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelCatalogUpsertParams {
    #[serde(default)]
    pub previous_slug: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub user_edited: Option<bool>,
    #[serde(default)]
    pub sort_index: Option<i64>,
    #[serde(flatten)]
    pub model: ModelInfo,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelSourceModelEntry {
    pub source_kind: String,
    pub source_id: String,
    pub upstream_model: String,
    pub display_name: Option<String>,
    pub status: String,
    pub discovery_kind: String,
    pub last_synced_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelSourceMappingEntry {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelRoutingResult {
    #[serde(default)]
    pub source_models: Vec<ManagedModelSourceModelEntry>,
    #[serde(default)]
    pub mappings: Vec<ManagedModelSourceMappingEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelSourceSyncParams {
    pub source_kind: String,
    #[serde(default)]
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelSourceModelUpsertParams {
    pub source_kind: String,
    pub source_id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedModelSourceMappingUpsertParams {
    #[serde(default)]
    pub id: Option<String>,
    pub platform_model_slug: String,
    pub source_kind: String,
    pub source_id: String,
    pub upstream_model: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub weight: Option<i64>,
    #[serde(default)]
    pub billing_model_slug: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroupEntry {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroupModelEntry {
    pub group_id: String,
    pub platform_model_slug: String,
    pub enabled: bool,
    pub rate_multiplier_millis: Option<i64>,
    pub billing_model_slug: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserModelGroupEntry {
    pub user_id: String,
    pub group_id: String,
    pub status: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroupListResult {
    #[serde(default)]
    pub groups: Vec<ModelGroupEntry>,
    #[serde(default)]
    pub models: Vec<ModelGroupModelEntry>,
    #[serde(default)]
    pub user_assignments: Vec<UserModelGroupEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroupUpsertParams {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub sort: Option<i64>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default)]
    pub rate_multiplier_millis: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroupModelUpsertParams {
    pub platform_model_slug: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub rate_multiplier_millis: Option<i64>,
    #[serde(default)]
    pub billing_model_slug: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroupModelsSetParams {
    pub group_id: String,
    #[serde(default)]
    pub models: Vec<ModelGroupModelUpsertParams>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroupUsersSetParams {
    pub group_id: String,
    #[serde(default)]
    pub user_ids: Vec<String>,
}

fn default_model_source_kind() -> String {
    "remote".to_string()
}

fn default_supported_in_api() -> bool {
    true
}

fn default_input_modalities() -> Vec<String> {
    vec!["text".to_string(), "image".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub slug: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_level: Option<String>,
    #[serde(default)]
    pub supported_reasoning_levels: Vec<ModelReasoningLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(default = "default_supported_in_api")]
    pub supported_in_api: bool,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub additional_speed_tiers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability_nux: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_messages: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_reasoning_summaries: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_verbosity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_verbosity: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_patch_tool_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_search_tool_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_policy: Option<ModelTruncationPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_image_detail_original: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_token_limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_context_window_percent: Option<i64>,
    #[serde(default)]
    pub experimental_supported_tools: Vec<String>,
    #[serde(default = "default_input_modalities")]
    pub input_modalities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimal_client_version: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_search_tool: Option<bool>,
    #[serde(default)]
    pub available_in_plans: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for ModelInfo {
    fn default() -> Self {
        Self {
            slug: String::new(),
            display_name: String::new(),
            description: None,
            default_reasoning_level: None,
            supported_reasoning_levels: Vec::new(),
            shell_type: None,
            visibility: None,
            supported_in_api: default_supported_in_api(),
            priority: 0,
            additional_speed_tiers: Vec::new(),
            availability_nux: None,
            upgrade: None,
            base_instructions: None,
            model_messages: None,
            supports_reasoning_summaries: None,
            default_reasoning_summary: None,
            support_verbosity: None,
            default_verbosity: None,
            apply_patch_tool_type: None,
            web_search_tool_type: None,
            truncation_policy: None,
            supports_parallel_tool_calls: None,
            supports_image_detail_original: None,
            context_window: None,
            auto_compact_token_limit: None,
            effective_context_window_percent: None,
            experimental_supported_tools: Vec::new(),
            input_modalities: default_input_modalities(),
            minimal_client_version: None,
            supports_search_tool: None,
            available_in_plans: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelReasoningLevel {
    pub effort: String,
    pub description: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelTruncationPolicy {
    pub mode: String,
    pub limit: i64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogSummary {
    pub trace_id: Option<String>,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub initial_account_id: Option<String>,
    #[serde(default)]
    pub attempted_account_ids: Vec<String>,
    pub initial_aggregate_api_id: Option<String>,
    #[serde(default)]
    pub attempted_aggregate_api_ids: Vec<String>,
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
    pub canonical_source: Option<String>,
    pub size_reject_stage: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestLogListParams {
    pub page: i64,
    pub page_size: i64,
    pub query: Option<String>,
    pub status_filter: Option<String>,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
}

impl Default for RequestLogListParams {
    /// 函数 `default`
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
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 20,
            query: None,
            status_filter: None,
            start_ts: None,
            end_ts: None,
        }
    }
}

impl RequestLogListParams {
    /// 函数 `normalized`
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
    pub fn normalized(self) -> Self {
        Self {
            page: if self.page < 1 { 1 } else { self.page },
            page_size: if self.page_size < 1 {
                20
            } else {
                self.page_size
            },
            query: self.query,
            status_filter: self.status_filter,
            start_ts: self.start_ts.filter(|value| *value > 0),
            end_ts: self.end_ts.filter(|value| *value > 0),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogListResult {
    pub items: Vec<RequestLogSummary>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogFilterSummaryResult {
    pub total_count: i64,
    pub filtered_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestLogTodaySummaryResult {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub today_tokens: i64,
    pub estimated_cost: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSnapshotResult {
    pub accounts: Vec<AccountSummary>,
    pub usage_snapshots: Vec<UsageSnapshotResult>,
    #[serde(default)]
    pub usage_aggregate_summary: UsageAggregateSummaryResult,
    pub api_keys: Vec<ApiKeySummary>,
    pub api_models: ModelsResponse,
    pub manual_preferred_account_id: Option<String>,
    pub request_log_today_summary: RequestLogTodaySummaryResult,
    pub request_logs: Vec<RequestLogSummary>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardTokenUsageResult {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardDailyUsagePoint {
    pub day_start_ts: i64,
    pub day_end_ts: i64,
    pub usage: DashboardTokenUsageResult,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardUserUsageSummary {
    pub user_id: String,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub status: Option<String>,
    pub wallet_available_credit_micros: Option<i64>,
    pub today_usage: DashboardTokenUsageResult,
    pub range_usage: DashboardTokenUsageResult,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSourceUsageSummary {
    pub source_kind: String,
    pub source_id: String,
    pub name: Option<String>,
    pub status: Option<String>,
    pub provider: Option<String>,
    pub today_usage: DashboardTokenUsageResult,
    pub range_usage: DashboardTokenUsageResult,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardAdminUsageSummaryResult {
    pub range_start_ts: i64,
    pub range_end_ts: i64,
    pub today_start_ts: i64,
    pub today_end_ts: i64,
    pub today_usage: DashboardTokenUsageResult,
    #[serde(default)]
    pub daily_usage: Vec<DashboardDailyUsagePoint>,
    #[serde(default)]
    pub users: Vec<DashboardUserUsageSummary>,
    #[serde(default)]
    pub openai_accounts: Vec<DashboardSourceUsageSummary>,
    #[serde(default)]
    pub aggregate_apis: Vec<DashboardSourceUsageSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardWalletResult {
    pub id: String,
    pub balance_credit_micros: i64,
    pub frozen_credit_micros: i64,
    pub available_credit_micros: i64,
    pub status: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardApiKeySummary {
    pub total_count: i64,
    pub enabled_count: i64,
    pub disabled_count: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardUsageToday {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
    pub total_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub success_rate: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardUsagePoint {
    pub day_start_ts: i64,
    pub day_end_ts: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardKeyUsage {
    pub key_id: String,
    pub name: Option<String>,
    pub model_slug: Option<String>,
    pub status: String,
    pub today_tokens: i64,
    pub today_cost_usd: f64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardModelUsage {
    pub model: String,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardAlert {
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub action_label: Option<String>,
    pub action_href: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberDashboardSummaryResult {
    pub user_id: Option<String>,
    pub distribution_enabled: bool,
    pub wallet: Option<MemberDashboardWalletResult>,
    pub api_key_summary: MemberDashboardApiKeySummary,
    pub usage_today: MemberDashboardUsageToday,
    #[serde(default)]
    pub usage_trend_7d: Vec<MemberDashboardUsagePoint>,
    #[serde(default)]
    pub top_keys: Vec<MemberDashboardKeyUsage>,
    #[serde(default)]
    pub top_models: Vec<MemberDashboardModelUsage>,
    #[serde(default)]
    pub available_models: Vec<ModelInfo>,
    #[serde(default)]
    pub recent_logs: Vec<RequestLogSummary>,
    #[serde(default)]
    pub alerts: Vec<MemberDashboardAlert>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceRuleEntry {
    pub id: String,
    pub provider: String,
    pub model_pattern: String,
    pub match_type: String,
    #[serde(default)]
    pub input_price_per_1m: Option<f64>,
    #[serde(default)]
    pub cached_input_price_per_1m: Option<f64>,
    #[serde(default)]
    pub output_price_per_1m: Option<f64>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceRuleListResult {
    #[serde(default)]
    pub items: Vec<ModelPriceRuleEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceRuleUpsertInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    pub model_pattern: String,
    #[serde(default)]
    pub match_type: Option<String>,
    #[serde(default)]
    pub input_price_per_1m: Option<f64>,
    #[serde(default)]
    pub cached_input_price_per_1m: Option<f64>,
    #[serde(default)]
    pub output_price_per_1m: Option<f64>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub priority: Option<i64>,
}

#[cfg(test)]
#[path = "tests/types_tests.rs"]
mod tests;
