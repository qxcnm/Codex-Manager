use codexmanager_core::rpc::types::{AggregateApiListResult, JsonRpcRequest, JsonRpcResponse};

use crate::{
    create_aggregate_api, delete_aggregate_api, fetch_aggregate_api_models, list_aggregate_apis,
    read_aggregate_api_secret, refresh_aggregate_api_balance, test_aggregate_api_connection,
    update_aggregate_api, RpcActor,
};

/// 函数 `api_id_param`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
///
/// # 返回
/// 返回函数执行结果
fn api_id_param(req: &JsonRpcRequest) -> Option<&str> {
    super::str_param(req, "id").or_else(|| super::str_param(req, "apiId"))
}

/// 函数 `try_handle`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - super: 参数 super
///
/// # 返回
/// 返回函数执行结果
pub(super) fn try_handle(req: &JsonRpcRequest, actor: &RpcActor) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "aggregateApi/list" => super::value_or_error(
            list_aggregate_apis().map(|items| AggregateApiListResult { items }),
        ),
        "aggregateApi/create" => {
            let provider_type = super::string_param(req, "providerType");
            let supplier_name = super::string_param(req, "supplierName");
            let sort = super::i64_param(req, "sort");
            let url = super::string_param(req, "url");
            let key = super::string_param(req, "key");
            let auth_type = super::string_param(req, "authType");
            let auth_custom_enabled = super::bool_param(req, "authCustomEnabled");
            let auth_params = req
                .params
                .as_ref()
                .and_then(|v| v.get("authParams"))
                .cloned();
            let action_custom_enabled = super::bool_param(req, "actionCustomEnabled");
            let action = super::string_param(req, "action");
            let model_override = super::string_param(req, "modelOverride");
            let user_agent = super::string_param(req, "userAgent");
            let username = super::string_param(req, "username");
            let password = super::string_param(req, "password");
            let balance_query_enabled = super::bool_param(req, "balanceQueryEnabled");
            let balance_query_template = super::string_param(req, "balanceQueryTemplate");
            let balance_query_base_url = super::string_param(req, "balanceQueryBaseUrl");
            let balance_query_access_token = super::string_param(req, "balanceQueryAccessToken");
            let balance_query_user_id = super::string_param(req, "balanceQueryUserId");
            let balance_query_config_json = super::string_param(req, "balanceQueryConfigJson");
            super::value_or_error(create_aggregate_api(
                url,
                key,
                provider_type,
                supplier_name,
                sort,
                auth_type,
                auth_custom_enabled,
                auth_params,
                action_custom_enabled,
                action,
                model_override,
                user_agent,
                username,
                password,
                balance_query_enabled,
                balance_query_template,
                balance_query_base_url,
                balance_query_access_token,
                balance_query_user_id,
                balance_query_config_json,
            ))
        }
        "aggregateApi/update" => {
            let api_id = api_id_param(req).unwrap_or("");
            let provider_type = super::string_param(req, "providerType");
            let supplier_name = super::string_param(req, "supplierName");
            let sort = super::i64_param(req, "sort");
            let status = super::string_param(req, "status");
            let url = super::string_param(req, "url");
            let key = super::string_param(req, "key");
            let auth_type = super::string_param(req, "authType");
            let auth_custom_enabled = super::bool_param(req, "authCustomEnabled");
            let auth_params = req
                .params
                .as_ref()
                .and_then(|v| v.get("authParams"))
                .cloned();
            let action_custom_enabled = super::bool_param(req, "actionCustomEnabled");
            let action = super::string_param(req, "action");
            let model_override = super::string_param(req, "modelOverride");
            let user_agent = super::string_param(req, "userAgent");
            let username = super::string_param(req, "username");
            let password = super::string_param(req, "password");
            let balance_query_enabled = super::bool_param(req, "balanceQueryEnabled");
            let balance_query_template = super::string_param(req, "balanceQueryTemplate");
            let balance_query_base_url = super::string_param(req, "balanceQueryBaseUrl");
            let balance_query_access_token = super::string_param(req, "balanceQueryAccessToken");
            let balance_query_user_id = super::string_param(req, "balanceQueryUserId");
            let balance_query_config_json = super::string_param(req, "balanceQueryConfigJson");
            super::ok_or_error(update_aggregate_api(
                api_id,
                url,
                key,
                provider_type,
                supplier_name,
                sort,
                status,
                auth_type,
                auth_custom_enabled,
                auth_params,
                action_custom_enabled,
                action,
                model_override,
                user_agent,
                username,
                password,
                balance_query_enabled,
                balance_query_template,
                balance_query_base_url,
                balance_query_access_token,
                balance_query_user_id,
                balance_query_config_json,
            ))
        }
        "aggregateApi/readSecret" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::value_or_error(read_aggregate_api_secret(api_id))
        }
        "aggregateApi/delete" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::ok_or_error(delete_aggregate_api(api_id))
        }
        "aggregateApi/testConnection" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::value_or_error(test_aggregate_api_connection(api_id))
        }
        "aggregateApi/refreshBalance" => {
            let api_id = api_id_param(req).unwrap_or("");
            super::value_or_error(refresh_aggregate_api_balance(api_id))
        }
        "aggregateApi/fetchModels" => {
            if !actor.is_admin() {
                return Some(super::response(
                    req,
                    super::value_or_error::<()>(Err(
                        "permission_denied: aggregateApi/fetchModels".to_string()
                    )),
                ));
            }
            let api_id = api_id_param(req).unwrap_or("");
            super::value_or_error(fetch_aggregate_api_models(api_id))
        }
        "aggregateApi/associateModels" => {
            if !actor.is_admin() {
                return Some(super::response(
                    req,
                    super::value_or_error::<()>(Err(
                        "permission_denied: aggregateApi/associateModels".to_string(),
                    )),
                ));
            }
            let api_id = api_id_param(req).unwrap_or("").to_string();
            let upstream_models = req
                .params
                .as_ref()
                .and_then(|value| value.get("upstreamModels"))
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let display_names = req
                .params
                .as_ref()
                .and_then(|value| value.get("displayNames"))
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default();
            let model_metadata = req
                .params
                .as_ref()
                .and_then(|params| params.get("modelMetadata"))
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default();
            super::value_or_error(
                crate::aggregate_api::associate_aggregate_api_models_with_metadata(
                    &api_id,
                    upstream_models,
                    display_names,
                    model_metadata,
                ),
            )
        }
        _ => return None,
    };

    Some(super::response(req, result))
}

#[cfg(test)]
#[path = "aggregate_api_tests.rs"]
mod tests;
