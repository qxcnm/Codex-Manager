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

#[tauri::command]
pub async fn service_managed_model_list_v2(
    addr: Option<String>,
    include_hidden: Option<bool>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "includeHidden": include_hidden.unwrap_or(false) });
    rpc_call_in_background("apikey/managedModelListV2", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_managed_model_get_v2(
    addr: Option<String>,
    slug: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "slug": slug });
    rpc_call_in_background("apikey/managedModelGetV2", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_managed_model_upsert_v2(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/managedModelUpsertV2", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_managed_model_update_state_v2(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/managedModelUpdateStateV2", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_managed_model_batch_update_state_v2(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "apikey/managedModelBatchUpdateStateV2",
        addr,
        Some(payload),
    )
    .await
}

#[tauri::command]
pub async fn service_managed_model_delete_v2(
    addr: Option<String>,
    slug: String,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "slug": slug });
    rpc_call_in_background("apikey/managedModelDeleteV2", addr, Some(params)).await
}

#[tauri::command]
pub async fn service_managed_model_import_preview_v2(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background(
        "apikey/managedModelImportPreviewV2",
        addr,
        Some(payload),
    )
    .await
}

#[tauri::command]
pub async fn service_managed_model_import_commit_v2(
    addr: Option<String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/managedModelImportCommitV2", addr, Some(payload)).await
}

#[tauri::command]
pub async fn service_managed_model_price_sync_v2(
    addr: Option<String>,
    payload: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    rpc_call_in_background("apikey/managedModelPriceSyncV2", addr, payload).await
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
/// - account_group_filter: 参数 account_group_filter
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
    account_group_filter: Option<String>,
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
      "accountGroupFilter": account_group_filter,
      "quotaLimitTokens": quota_limit_tokens,
      "customKey": custom_key,
    });
    rpc_call_in_background("apikey/create", addr, Some(params)).await
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

/// API Key 更新命令发送到服务 RPC 的参数。
#[derive(Debug, Default)]
struct ApiKeyUpdateRpcPayload {
    key_id: String,
    name: Option<String>,
    has_name: bool,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    has_model_config: bool,
    protocol_type: Option<String>,
    upstream_base_url: Option<String>,
    static_headers_json: Option<String>,
    rotation_strategy: Option<String>,
    aggregate_api_id: Option<String>,
    account_plan_filter: Option<String>,
    has_routing_config: bool,
    account_group_filter: Option<String>,
    has_account_group_filter: bool,
    quota_limit_tokens: Option<i64>,
    has_quota_limit_tokens: bool,
}

impl ApiKeyUpdateRpcPayload {
    fn into_value(self) -> serde_json::Value {
        let mut params = serde_json::Map::new();
        params.insert("id".to_string(), serde_json::json!(self.key_id));
        if self.has_name {
            params.insert("name".to_string(), serde_json::json!(self.name));
        }
        if self.has_model_config {
            params.insert(
                "modelSlug".to_string(),
                serde_json::json!(self.model_slug),
            );
            params.insert(
                "reasoningEffort".to_string(),
                serde_json::json!(self.reasoning_effort),
            );
            params.insert(
                "serviceTier".to_string(),
                serde_json::json!(self.service_tier),
            );
        }
        params.insert(
            "protocolType".to_string(),
            serde_json::json!(self.protocol_type),
        );
        params.insert(
            "upstreamBaseUrl".to_string(),
            serde_json::json!(self.upstream_base_url),
        );
        params.insert(
            "staticHeadersJson".to_string(),
            serde_json::json!(self.static_headers_json),
        );
        if self.has_routing_config {
            params.insert(
                "rotationStrategy".to_string(),
                serde_json::json!(self.rotation_strategy),
            );
            params.insert(
                "aggregateApiId".to_string(),
                serde_json::json!(self.aggregate_api_id),
            );
            params.insert(
                "accountPlanFilter".to_string(),
                serde_json::json!(self.account_plan_filter),
            );
        }
        if self.has_account_group_filter {
            params.insert(
                "accountGroupFilter".to_string(),
                serde_json::json!(self.account_group_filter),
            );
        }
        if self.has_quota_limit_tokens {
            params.insert(
                "quotaLimitTokens".to_string(),
                serde_json::json!(self.quota_limit_tokens),
            );
        }
        serde_json::Value::Object(params)
    }
}

fn resolve_has_quota_limit_tokens(
    quota_limit_tokens: Option<i64>,
    explicit_presence: Option<bool>,
) -> bool {
    explicit_presence.unwrap_or(quota_limit_tokens.is_some())
}

fn resolve_has_account_group_filter(
    account_group_filter: Option<&str>,
    explicit_presence: Option<bool>,
) -> bool {
    explicit_presence.unwrap_or(account_group_filter.is_some())
}

fn resolve_has_name(name: Option<&str>, explicit_presence: Option<bool>) -> bool {
    explicit_presence.unwrap_or(name.is_some())
}

fn resolve_has_model_config(
    model_slug: Option<&str>,
    reasoning_effort: Option<&str>,
    service_tier: Option<&str>,
    explicit_presence: Option<bool>,
) -> bool {
    explicit_presence
        .unwrap_or(model_slug.is_some() || reasoning_effort.is_some() || service_tier.is_some())
}

fn resolve_has_routing_config(
    rotation_strategy: Option<&str>,
    aggregate_api_id: Option<&str>,
    account_plan_filter: Option<&str>,
    explicit_presence: Option<bool>,
) -> bool {
    explicit_presence.unwrap_or(
        rotation_strategy.is_some()
            || aggregate_api_id.is_some()
            || account_plan_filter.is_some(),
    )
}

/// 更新 API Key 的模型、路由和可选配额配置。
///
/// `has_*` 参数仅用于桌面命令边界区分未提供的字段组与显式传入 `null` 清空字段；
/// 它们不会作为业务字段转发给服务 RPC。
#[tauri::command]
pub async fn service_apikey_update_model(
    addr: Option<String>,
    key_id: String,
    name: Option<String>,
    has_name: Option<bool>,
    model_slug: Option<String>,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    has_model_config: Option<bool>,
    protocol_type: Option<String>,
    upstream_base_url: Option<String>,
    static_headers_json: Option<String>,
    rotation_strategy: Option<String>,
    aggregate_api_id: Option<String>,
    account_plan_filter: Option<String>,
    has_routing_config: Option<bool>,
    account_group_filter: Option<String>,
    has_account_group_filter: Option<bool>,
    quota_limit_tokens: Option<i64>,
    has_quota_limit_tokens: Option<bool>,
) -> Result<serde_json::Value, String> {
    let has_name = resolve_has_name(name.as_deref(), has_name);
    let has_model_config = resolve_has_model_config(
        model_slug.as_deref(),
        reasoning_effort.as_deref(),
        service_tier.as_deref(),
        has_model_config,
    );
    let has_routing_config = resolve_has_routing_config(
        rotation_strategy.as_deref(),
        aggregate_api_id.as_deref(),
        account_plan_filter.as_deref(),
        has_routing_config,
    );
    let has_account_group_filter = resolve_has_account_group_filter(
        account_group_filter.as_deref(),
        has_account_group_filter,
    );
    let has_quota_limit_tokens =
        resolve_has_quota_limit_tokens(quota_limit_tokens, has_quota_limit_tokens);
    let params = ApiKeyUpdateRpcPayload {
        key_id,
        name,
        has_name,
        model_slug,
        reasoning_effort,
        service_tier,
        has_model_config,
        protocol_type,
        upstream_base_url,
        static_headers_json,
        rotation_strategy,
        aggregate_api_id,
        account_plan_filter,
        has_routing_config,
        account_group_filter,
        has_account_group_filter,
        quota_limit_tokens,
        has_quota_limit_tokens,
    }
    .into_value();
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

#[cfg(test)]
mod tests {
    use super::{
        resolve_has_account_group_filter, resolve_has_model_config, resolve_has_name,
        resolve_has_quota_limit_tokens, resolve_has_routing_config, ApiKeyUpdateRpcPayload,
    };

    #[test]
    fn api_key_update_payload_preserves_partial_field_groups() {
        assert!(resolve_has_name(Some("renamed"), None));
        assert!(!resolve_has_name(None, None));
        assert!(resolve_has_name(None, Some(true)));
        assert!(resolve_has_model_config(
            Some("gpt-5"),
            None,
            None,
            None
        ));
        assert!(!resolve_has_model_config(None, None, None, None));
        assert!(resolve_has_model_config(None, None, None, Some(true)));
        assert!(resolve_has_routing_config(
            Some("account_rotation"),
            None,
            None,
            None
        ));
        assert!(!resolve_has_routing_config(None, None, None, None));
        assert!(resolve_has_routing_config(None, None, None, Some(true)));

        let group_only = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            account_group_filter: Some("team-a".to_string()),
            has_account_group_filter: true,
            ..Default::default()
        }
        .into_value();
        for field in [
            "name",
            "modelSlug",
            "reasoningEffort",
            "serviceTier",
            "rotationStrategy",
            "aggregateApiId",
            "accountPlanFilter",
        ] {
            assert!(group_only.get(field).is_none(), "{field} must be omitted");
        }
        assert_eq!(
            group_only
                .get("accountGroupFilter")
                .and_then(serde_json::Value::as_str),
            Some("team-a")
        );

        let explicit_clears = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            has_name: true,
            has_model_config: true,
            has_routing_config: true,
            ..Default::default()
        }
        .into_value();
        for field in [
            "name",
            "modelSlug",
            "reasoningEffort",
            "serviceTier",
            "rotationStrategy",
            "aggregateApiId",
            "accountPlanFilter",
        ] {
            assert!(
                explicit_clears
                    .get(field)
                    .is_some_and(serde_json::Value::is_null),
                "{field} must be forwarded as an explicit null"
            );
        }
    }

    #[test]
    fn api_key_update_payload_preserves_account_group_filter_presence() {
        assert!(resolve_has_account_group_filter(Some("team-a"), None));
        assert!(!resolve_has_account_group_filter(None, None));
        assert!(resolve_has_account_group_filter(None, Some(true)));

        let omitted = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            account_group_filter: Some("team-a".to_string()),
            has_account_group_filter: false,
            ..Default::default()
        }
        .into_value();
        assert!(omitted.get("accountGroupFilter").is_none());

        let updated = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            account_group_filter: Some("team-a".to_string()),
            has_account_group_filter: true,
            ..Default::default()
        }
        .into_value();
        assert_eq!(
            updated
                .get("accountGroupFilter")
                .and_then(serde_json::Value::as_str),
            Some("team-a")
        );

        let cleared = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            account_group_filter: None,
            has_account_group_filter: true,
            ..Default::default()
        }
        .into_value();
        assert!(cleared
            .get("accountGroupFilter")
            .is_some_and(serde_json::Value::is_null));
    }

    #[test]
    fn api_key_update_payload_preserves_quota_field_presence() {
        assert!(resolve_has_quota_limit_tokens(Some(123), None));
        assert!(!resolve_has_quota_limit_tokens(None, None));
        assert!(resolve_has_quota_limit_tokens(None, Some(true)));

        let omitted = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            quota_limit_tokens: Some(123),
            has_quota_limit_tokens: false,
            ..Default::default()
        }
        .into_value();
        assert!(omitted.get("quotaLimitTokens").is_none());

        let updated = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            quota_limit_tokens: Some(123),
            has_quota_limit_tokens: true,
            ..Default::default()
        }
        .into_value();
        assert_eq!(
            updated
                .get("quotaLimitTokens")
                .and_then(serde_json::Value::as_i64),
            Some(123)
        );

        let cleared = ApiKeyUpdateRpcPayload {
            key_id: "key-1".to_string(),
            quota_limit_tokens: None,
            has_quota_limit_tokens: true,
            ..Default::default()
        }
        .into_value();
        assert!(cleared
            .get("quotaLimitTokens")
            .is_some_and(serde_json::Value::is_null));
    }
}
