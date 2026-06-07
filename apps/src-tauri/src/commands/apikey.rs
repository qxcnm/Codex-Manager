use crate::commands::shared::rpc_call_in_background;

/// 函数 `service_apikey_list`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_list(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/list", addr, None).await
}

/// 函数 `service_apikey_read_secret`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - key_id: 参数 key_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_read_secret(
    addr: Option<String>,
    key_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": key_id });
    rpc_call_in_background("apikey/readSecret", addr, Some(params)).await
}

/// 函数 `service_apikey_create`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - name: 参数 name
/// - model_slug: 参数 model_slug
/// - reasoning_effort: 参数 reasoning_effort
/// - service_tier: 参数 service_tier
/// - protocol_type: 参数 protocol_type
/// - upstream_base_url: 参数 upstream_base_url
/// - static_headers_json: 参数 static_headers_json
/// - rotation_strategy: 参数 rotation_strategy
/// - aggregate_api_id: 参数 aggregate_api_id
/// - account_plan_filter: 参数 account_plan_filter
/// - quota_limit_tokens: 参数 quota_limit_tokens
/// - custom_key: 参数 custom_key
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_create(
    addr: Option<String>,
    name: Option<String>,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    protocol_type: Option<String>,
    upstream_base_url: Option<String>,
    static_headers_json: Option<String>,
    rotation_strategy: Option<String>,
    aggregate_api_id: Option<String>,
    account_plan_filter: Option<String>,
    quota_limit_tokens: Option<i64>,
    custom_key: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "name": name,
      "modelSlug": model_slug,
      "reasoningEffort": reasoning_effort,
      "serviceTier": service_tier,
      "protocolType": protocol_type,
      "upstreamBaseUrl": upstream_base_url,
      "staticHeadersJson": static_headers_json,
      "rotationStrategy": rotation_strategy,
      "aggregateApiId": aggregate_api_id,
      "accountPlanFilter": account_plan_filter,
      "quotaLimitTokens": quota_limit_tokens,
      "customKey": custom_key,
    });
    rpc_call_in_background("apikey/create", addr, Some(params)).await
}

/// 函数 `service_apikey_models`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - refresh_remote: 参数 refresh_remote
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_models(
    addr: Option<String>,
    refresh_remote: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = refresh_remote.map(|value| serde_json::json!({ "refreshRemote": value }));
    rpc_call_in_background("apikey/models", addr, params).await
}

/// 函数 `service_model_catalog_list`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// - addr: 参数 addr
/// - refresh_remote: 参数 refresh_remote
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_model_catalog_list(
    addr: Option<String>,
    refresh_remote: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = refresh_remote.map(|value| serde_json::json!({ "refreshRemote": value }));
    rpc_call_in_background("apikey/modelCatalogList", addr, params).await
}

/// 函数 `service_model_catalog_save`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// - addr: 参数 addr
/// - payload: 参数 payload
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_model_catalog_save(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/modelCatalogSave", addr, Some(payload)).await
}

/// 函数 `service_model_catalog_delete`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-12
///
/// # 参数
/// - addr: 参数 addr
/// - slug: 参数 slug
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_model_catalog_delete(
    addr: Option<String>,
    slug: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "slug": slug });
    rpc_call_in_background("apikey/modelCatalogDelete", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_catalog_prune_stale_remote(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/modelCatalogPruneStaleRemote", addr, None).await
}

#[tauri::command]
pub async fn service_model_routing(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/modelRouting", addr, None).await
}

#[tauri::command]
pub async fn service_model_source_sync(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/modelSourceSync", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_model_source_model_save(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/modelSourceModelSave", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_model_source_mapping_save(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/modelSourceMappingSave", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_model_source_mapping_delete(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/modelSourceMappingDelete", addr, Some(payload)).await
}

/// 函数 `service_apikey_usage_stats`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_usage_stats(addr: Option<String>) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/usageStats", addr, None).await
}

/// 函数 `service_apikey_update_model`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - key_id: 参数 key_id
/// - name: 参数 name
/// - model_slug: 参数 model_slug
/// - reasoning_effort: 参数 reasoning_effort
/// - service_tier: 参数 service_tier
/// - protocol_type: 参数 protocol_type
/// - upstream_base_url: 参数 upstream_base_url
/// - static_headers_json: 参数 static_headers_json
/// - rotation_strategy: 参数 rotation_strategy
/// - aggregate_api_id: 参数 aggregate_api_id
/// - account_plan_filter: 参数 account_plan_filter
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_update_model(
    addr: Option<String>,
    key_id: String,
    name: Option<String>,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    protocol_type: Option<String>,
    upstream_base_url: Option<String>,
    static_headers_json: Option<String>,
    rotation_strategy: Option<String>,
    aggregate_api_id: Option<String>,
    account_plan_filter: Option<String>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
      "id": key_id,
      "name": name,
      "modelSlug": model_slug,
      "reasoningEffort": reasoning_effort,
      "serviceTier": service_tier,
      "protocolType": protocol_type,
      "upstreamBaseUrl": upstream_base_url,
      "staticHeadersJson": static_headers_json,
      "rotationStrategy": rotation_strategy,
      "aggregateApiId": aggregate_api_id,
      "accountPlanFilter": account_plan_filter,
    });
    rpc_call_in_background("apikey/updateModel", addr, Some(params)).await
}

/// 函数 `service_apikey_delete`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - key_id: 参数 key_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_delete(
    addr: Option<String>,
    key_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": key_id });
    rpc_call_in_background("apikey/delete", addr, Some(params)).await
}

/// 函数 `service_apikey_disable`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - key_id: 参数 key_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_disable(
    addr: Option<String>,
    key_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": key_id });
    rpc_call_in_background("apikey/disable", addr, Some(params)).await
}

/// 函数 `service_apikey_enable`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - addr: 参数 addr
/// - key_id: 参数 key_id
///
/// # 返回
/// 返回函数执行结果
#[tauri::command]
pub async fn service_apikey_enable(
    addr: Option<String>,
    key_id: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "id": key_id });
    rpc_call_in_background("apikey/enable", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_price_rules_list(
    addr: Option<String>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("quota/modelPriceRules/list", addr, None).await
}

#[tauri::command]
pub async fn service_model_price_rule_read(
    addr: Option<String>,
    model_pattern: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "modelPattern": model_pattern });
    rpc_call_in_background("quota/modelPriceRule/read", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_model_price_rule_upsert(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("quota/modelPriceRule/upsert", addr, Some(payload)).await
}
