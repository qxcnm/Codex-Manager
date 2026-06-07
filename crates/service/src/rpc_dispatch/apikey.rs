use codexmanager_core::rpc::types::{
    ApiKeyListResult, ApiKeyUsageStatListResult, JsonRpcRequest, JsonRpcResponse,
    ManagedModelCatalogResult, ManagedModelCatalogUpsertParams, ManagedModelRoutingResult,
    ManagedModelSourceMappingUpsertParams, ManagedModelSourceModelUpsertParams,
    ManagedModelSourceSyncParams, ModelsResponse,
};

use crate::RpcActor;
use crate::{
    apikey_create, apikey_delete, apikey_disable, apikey_enable, apikey_list, apikey_models,
    apikey_read_secret, apikey_update_model, apikey_usage_stats,
};

fn ensure_api_key_access(actor: &RpcActor, key_id: &str) -> Result<(), String> {
    if actor.is_admin() {
        return Ok(());
    }
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: apikey requires user session".to_string())?;
    if crate::api_key_belongs_to_user(key_id, user_id)? {
        return Ok(());
    }
    Err("permission_denied: apikey".to_string())
}

fn allowed_model_slugs_for_actor(
    actor: &RpcActor,
) -> Result<Option<std::collections::HashSet<String>>, String> {
    if actor.is_admin() {
        return Ok(None);
    }
    let user_id = actor
        .user_id
        .as_deref()
        .ok_or_else(|| "permission_denied: models requires user session".to_string())?;
    let storage =
        crate::storage_helpers::open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let slugs = storage
        .allowed_model_slugs_for_user(user_id, codexmanager_core::storage::now_ts())
        .map_err(|err| format!("read allowed model groups failed: {err}"))?
        .into_iter()
        .collect();
    Ok(Some(slugs))
}

fn filter_models_for_actor(
    actor: &RpcActor,
    models: ModelsResponse,
) -> Result<ModelsResponse, String> {
    let Some(allowed) = allowed_model_slugs_for_actor(actor)? else {
        return Ok(models);
    };
    Ok(ModelsResponse {
        models: models
            .models
            .into_iter()
            .filter(|model| allowed.contains(model.slug.as_str()))
            .collect(),
        extra: models.extra,
    })
}

fn filter_catalog_for_actor(
    actor: &RpcActor,
    catalog: ManagedModelCatalogResult,
) -> Result<ManagedModelCatalogResult, String> {
    let Some(allowed) = allowed_model_slugs_for_actor(actor)? else {
        return Ok(catalog);
    };
    Ok(ManagedModelCatalogResult {
        items: catalog
            .items
            .into_iter()
            .filter(|item| allowed.contains(item.model.slug.as_str()))
            .collect(),
        extra: catalog.extra,
    })
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
        "apikey/list" => super::value_or_error(
            apikey_list::read_api_keys_for_actor(actor).map(|items| ApiKeyListResult { items }),
        ),
        "apikey/create" => {
            let name = super::string_param(req, "name");
            let model_slug = super::string_param(req, "modelSlug");
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let service_tier = super::string_param(req, "serviceTier");
            let protocol_type = super::string_param(req, "protocolType");
            let upstream_base_url = if actor.is_admin() {
                super::string_param(req, "upstreamBaseUrl")
            } else {
                None
            };
            let static_headers_json = if actor.is_admin() {
                super::string_param(req, "staticHeadersJson")
            } else {
                None
            };
            let rotation_strategy = if actor.is_admin() {
                super::string_param(req, "rotationStrategy")
            } else {
                None
            };
            let aggregate_api_id = if actor.is_admin() {
                super::string_param(req, "aggregateApiId")
            } else {
                None
            };
            let account_plan_filter = if actor.is_admin() {
                super::string_param(req, "accountPlanFilter")
            } else {
                None
            };
            let quota_limit_tokens = super::i64_param(req, "quotaLimitTokens");
            let custom_key = super::string_param(req, "customKey");
            let created = apikey_create::create_api_key(
                name,
                model_slug,
                reasoning_effort,
                service_tier,
                protocol_type,
                upstream_base_url,
                static_headers_json,
                rotation_strategy,
                aggregate_api_id,
                account_plan_filter,
                quota_limit_tokens,
                custom_key,
            )
            .and_then(|result| {
                if actor.is_admin() {
                    return Ok(result);
                }
                let user_id = actor
                    .user_id
                    .as_deref()
                    .ok_or_else(|| "permission_denied: apikey requires user session".to_string())?;
                crate::set_api_key_owner(&result.id, "user", Some(user_id), None)?;
                Ok(result)
            });
            super::value_or_error(created)
        }
        "apikey/readSecret" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::value_or_error(
                ensure_api_key_access(actor, key_id)
                    .and_then(|_| apikey_read_secret::read_api_key_secret(key_id)),
            )
        }
        "apikey/models" => {
            let refresh_remote = super::bool_param(req, "refreshRemote").unwrap_or(false);
            super::value_or_error(
                apikey_models::read_model_options(refresh_remote)
                    .and_then(|models| filter_models_for_actor(actor, models)),
            )
        }
        "apikey/modelCatalogList" => {
            let refresh_remote = super::bool_param(req, "refreshRemote").unwrap_or(false);
            super::value_or_error(
                apikey_models::read_managed_model_catalog(refresh_remote)
                    .and_then(|catalog| filter_catalog_for_actor(actor, catalog)),
            )
        }
        "apikey/modelCatalogSave" => {
            let params = req
                .params
                .clone()
                .ok_or_else(|| "缺少模型参数".to_string())
                .and_then(|value| {
                    serde_json::from_value::<ManagedModelCatalogUpsertParams>(value)
                        .map_err(|err| format!("解析模型参数失败: {err}"))
                });
            super::value_or_error(params.and_then(apikey_models::save_managed_model_catalog_model))
        }
        "apikey/modelCatalogDelete" => {
            let slug = super::str_param(req, "slug").unwrap_or("");
            super::ok_or_error(apikey_models::delete_managed_model_catalog_model(slug))
        }
        "apikey/modelCatalogPruneStaleRemote" => {
            if !actor.is_admin() {
                super::value_or_error::<ManagedModelCatalogResult>(Err(super::permission_denied(
                    "apikey/modelCatalogPruneStaleRemote",
                )))
            } else {
                super::value_or_error(
                    apikey_models::prune_stale_remote_managed_model_catalog()
                        .and_then(|catalog| filter_catalog_for_actor(actor, catalog)),
                )
            }
        }
        "apikey/modelRouting" => {
            if actor.is_admin() {
                super::value_or_error(apikey_models::read_managed_model_routing())
            } else {
                super::value_or_error::<ManagedModelRoutingResult>(Ok(Default::default()))
            }
        }
        "apikey/modelSourceSync" => {
            let params = req
                .params
                .clone()
                .ok_or_else(|| "缺少来源参数".to_string())
                .and_then(|value| {
                    serde_json::from_value::<ManagedModelSourceSyncParams>(value)
                        .map_err(|err| format!("解析来源参数失败: {err}"))
                });
            super::value_or_error(params.and_then(apikey_models::sync_managed_model_source_models))
        }
        "apikey/modelSourceModelSave" => {
            let params = req
                .params
                .clone()
                .ok_or_else(|| "缺少来源模型参数".to_string())
                .and_then(|value| {
                    serde_json::from_value::<ManagedModelSourceModelUpsertParams>(value)
                        .map_err(|err| format!("解析来源模型参数失败: {err}"))
                });
            super::value_or_error(params.and_then(apikey_models::upsert_managed_model_source_model))
        }
        "apikey/modelSourceMappingSave" => {
            let params = req
                .params
                .clone()
                .ok_or_else(|| "缺少映射参数".to_string())
                .and_then(|value| {
                    serde_json::from_value::<ManagedModelSourceMappingUpsertParams>(value)
                        .map_err(|err| format!("解析映射参数失败: {err}"))
                });
            super::value_or_error(params.and_then(apikey_models::save_managed_model_source_mapping))
        }
        "apikey/modelSourceMappingDelete" => {
            let id = super::str_param(req, "id").unwrap_or("");
            let source_kind = super::str_param(req, "sourceKind").unwrap_or("");
            let source_id = super::str_param(req, "sourceId").unwrap_or("");
            let upstream_model = super::str_param(req, "upstreamModel").unwrap_or("");
            super::ok_or_error(apikey_models::delete_managed_model_source_mapping(
                id, source_kind, source_id, upstream_model,
            ))
        }
        "apikey/usageStats" => super::value_or_error(
            apikey_usage_stats::read_api_key_usage_stats_for_actor(actor)
                .map(|items| ApiKeyUsageStatListResult { items }),
        ),
        "apikey/updateModel" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            let has_name = req
                .params
                .as_ref()
                .and_then(|value| value.as_object())
                .map(|params| params.contains_key("name"))
                .unwrap_or(false);
            let name = super::string_param(req, "name");
            let model_slug = super::string_param(req, "modelSlug");
            let reasoning_effort = super::string_param(req, "reasoningEffort");
            let service_tier = super::string_param(req, "serviceTier");
            let protocol_type = super::string_param(req, "protocolType");
            let upstream_base_url = super::string_param(req, "upstreamBaseUrl");
            let static_headers_json = super::string_param(req, "staticHeadersJson");
            let rotation_strategy = super::string_param(req, "rotationStrategy");
            let aggregate_api_id = super::string_param(req, "aggregateApiId");
            let account_plan_filter = super::string_param(req, "accountPlanFilter");
            let has_quota_limit_tokens = req
                .params
                .as_ref()
                .and_then(|value| value.as_object())
                .map(|params| params.contains_key("quotaLimitTokens"))
                .unwrap_or(false);
            let quota_limit_tokens = super::i64_param(req, "quotaLimitTokens");
            super::ok_or_error(ensure_api_key_access(actor, key_id).and_then(|_| {
                apikey_update_model::update_api_key_model(
                    key_id,
                    name,
                    has_name,
                    model_slug,
                    reasoning_effort,
                    service_tier,
                    protocol_type,
                    if actor.is_admin() {
                        upstream_base_url
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        static_headers_json
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        rotation_strategy
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        aggregate_api_id
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        account_plan_filter
                    } else {
                        None
                    },
                    has_quota_limit_tokens,
                    quota_limit_tokens,
                )
            }))
        }
        "apikey/delete" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(
                ensure_api_key_access(actor, key_id)
                    .and_then(|_| apikey_delete::delete_api_key(key_id)),
            )
        }
        "apikey/disable" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(
                ensure_api_key_access(actor, key_id)
                    .and_then(|_| apikey_disable::disable_api_key(key_id)),
            )
        }
        "apikey/enable" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            super::ok_or_error(
                ensure_api_key_access(actor, key_id)
                    .and_then(|_| apikey_enable::enable_api_key(key_id)),
            )
        }
        _ => return None,
    };

    Some(super::response(req, result))
}
