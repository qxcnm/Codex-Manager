use codexmanager_core::rpc::types::{
    ApiKeyListResult, ApiKeyUsageStatListResult, JsonRpcRequest, JsonRpcResponse,
};
use codexmanager_core::storage::{
    ManagedModelBatchStateV2Update, ManagedModelStateV2Update, ManagedModelV2,
    ManagedModelV2Upsert, ModelCatalogV2Stats,
};

use crate::RpcActor;
use crate::{
    apikey_create, apikey_delete, apikey_disable, apikey_enable, apikey_list, apikey_read_secret,
    apikey_update_model, apikey_usage_stats,
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
    let storage = &crate::account::remote_storage::AccountStorage::new(&storage);
    let slugs = storage
        .allowed_model_slugs_for_user_v2(user_id, codexmanager_core::storage::now_ts())
        .map_err(|err| format!("read allowed model groups failed: {err}"))?
        .into_iter()
        .collect();
    Ok(Some(slugs))
}

fn filter_models_v2_for_actor(
    actor: &RpcActor,
    mut result: crate::models_v2::ManagedModelListV2Result,
) -> Result<crate::models_v2::ManagedModelListV2Result, String> {
    let Some(allowed) = allowed_model_slugs_for_actor(actor)? else {
        return Ok(result);
    };
    result
        .items
        .retain(|model| allowed.contains(model.slug.as_str()));
    for model in &mut result.items {
        model.routes.clear();
        model.permission_group_ids.clear();
        model.instructions_text = None;
    }
    result.stats = ModelCatalogV2Stats {
        total: result.items.len() as i64,
        enabled: result.items.iter().filter(|model| model.enabled).count() as i64,
        builtin: result
            .items
            .iter()
            .filter(|model| model.origin == "builtin")
            .count() as i64,
        custom: result
            .items
            .iter()
            .filter(|model| model.origin == "custom")
            .count() as i64,
        price_missing: result
            .items
            .iter()
            .filter(|model| model.price.price_status == "missing")
            .count() as i64,
        missing_route: 0,
    };
    Ok(result)
}

fn filter_model_v2_for_actor(
    actor: &RpcActor,
    mut model: ManagedModelV2,
) -> Result<ManagedModelV2, String> {
    let Some(allowed) = allowed_model_slugs_for_actor(actor)? else {
        return Ok(model);
    };
    if !allowed.contains(model.slug.as_str()) {
        return Err(super::permission_denied("apikey/managedModelGetV2"));
    }
    model.routes.clear();
    model.permission_group_ids.clear();
    model.instructions_text = None;
    Ok(model)
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
            let account_group_filter = if actor.is_admin() {
                super::string_param(req, "accountGroupFilter")
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
                account_group_filter,
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
                if let Err(err) = crate::set_api_key_owner(&result.id, "user", Some(user_id), None)
                {
                    let _ = apikey_delete::delete_api_key(&result.id);
                    return Err(err);
                }
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
        "apikey/managedModelListV2" => {
            let include_hidden =
                actor.is_admin() && super::bool_param(req, "includeHidden").unwrap_or(false);
            super::value_or_error(
                crate::models_v2::list(include_hidden)
                    .and_then(|result| filter_models_v2_for_actor(actor, result)),
            )
        }
        "apikey/managedModelGetV2" => {
            let slug = super::str_param(req, "slug").unwrap_or("");
            super::value_or_error(
                crate::models_v2::get(slug)
                    .and_then(|model| filter_model_v2_for_actor(actor, model)),
            )
        }
        "apikey/managedModelUpsertV2" => {
            if !actor.is_admin() {
                super::value_or_error::<ManagedModelV2>(Err(super::permission_denied(
                    "apikey/managedModelUpsertV2",
                )))
            } else {
                let params = req
                    .params
                    .clone()
                    .ok_or_else(|| "missing managed model V2 payload".to_string())
                    .and_then(|value| {
                        serde_json::from_value::<ManagedModelV2Upsert>(value)
                            .map_err(|err| format!("parse managed model V2 payload failed: {err}"))
                    });
                super::value_or_error(params.and_then(crate::models_v2::upsert))
            }
        }
        "apikey/managedModelUpdateStateV2" => {
            if !actor.is_admin() {
                super::value_or_error::<ManagedModelV2>(Err(super::permission_denied(
                    "apikey/managedModelUpdateStateV2",
                )))
            } else {
                let params = req
                    .params
                    .clone()
                    .ok_or_else(|| "missing managed model V2 state payload".to_string())
                    .and_then(|value| {
                        serde_json::from_value::<ManagedModelStateV2Update>(value).map_err(|err| {
                            format!("parse managed model V2 state payload failed: {err}")
                        })
                    });
                super::value_or_error(params.and_then(crate::models_v2::update_state))
            }
        }
        "apikey/managedModelBatchUpdateStateV2" => {
            if !actor.is_admin() {
                super::value_or_error::<Vec<ManagedModelV2>>(Err(super::permission_denied(
                    "apikey/managedModelBatchUpdateStateV2",
                )))
            } else {
                let params = req
                    .params
                    .clone()
                    .ok_or_else(|| "missing managed model V2 batch state payload".to_string())
                    .and_then(|value| {
                        serde_json::from_value::<ManagedModelBatchStateV2Update>(value).map_err(
                            |err| {
                                format!("parse managed model V2 batch state payload failed: {err}")
                            },
                        )
                    });
                super::value_or_error(params.and_then(crate::models_v2::batch_update_state))
            }
        }
        "apikey/managedModelDeleteV2" => {
            if !actor.is_admin() {
                super::ok_or_error(Err(super::permission_denied("apikey/managedModelDeleteV2")))
            } else {
                let slug = super::str_param(req, "slug").unwrap_or("");
                super::ok_or_error(crate::models_v2::delete(slug))
            }
        }
        "apikey/managedModelImportPreviewV2" => {
            if !actor.is_admin() {
                super::value_or_error::<crate::models_v2::ManagedModelImportPreviewV2Result>(Err(
                    super::permission_denied("apikey/managedModelImportPreviewV2"),
                ))
            } else {
                let params = req
                    .params
                    .clone()
                    .ok_or_else(|| "missing model import preview payload".to_string())
                    .and_then(|value| {
                        serde_json::from_value::<
                            crate::models_v2::ManagedModelImportPreviewV2Params,
                        >(value)
                        .map_err(|err| format!("parse model import preview payload failed: {err}"))
                    });
                super::value_or_error(params.and_then(crate::models_v2::preview_import))
            }
        }
        "apikey/managedModelImportCommitV2" => {
            if !actor.is_admin() {
                super::value_or_error::<crate::models_v2::ManagedModelImportPreviewV2Result>(Err(
                    super::permission_denied("apikey/managedModelImportCommitV2"),
                ))
            } else {
                let params = req
                    .params
                    .clone()
                    .ok_or_else(|| "missing model import commit payload".to_string())
                    .and_then(|value| {
                        serde_json::from_value::<
                            crate::models_v2::ManagedModelImportCommitV2Params,
                        >(value)
                        .map_err(|err| format!("parse model import commit payload failed: {err}"))
                    });
                super::value_or_error(params.and_then(crate::models_v2::commit_import))
            }
        }
        "apikey/usageStats" => super::value_or_error(
            apikey_usage_stats::read_api_key_usage_stats_for_actor(actor)
                .map(|items| ApiKeyUsageStatListResult { items }),
        ),
        "apikey/updateModel" => {
            let key_id = super::str_param(req, "id").unwrap_or("");
            let params = req.params.as_ref().and_then(|value| value.as_object());
            let has_name = params.is_some_and(|params| params.contains_key("name"));
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
            let account_group_filter = super::string_param(req, "accountGroupFilter");
            let has_quota_limit_tokens =
                params.is_some_and(|params| params.contains_key("quotaLimitTokens"));
            let update_model_config = params.is_some_and(|params| {
                params.contains_key("modelSlug")
                    || params.contains_key("reasoningEffort")
                    || params.contains_key("serviceTier")
            });
            let update_routing_config = actor.is_admin()
                && params.is_some_and(|params| {
                    params.contains_key("rotationStrategy")
                        || params.contains_key("aggregateApiId")
                        || params.contains_key("accountPlanFilter")
                });
            let update_account_group_filter = actor.is_admin()
                && params.is_some_and(|params| params.contains_key("accountGroupFilter"));
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
                    if actor.is_admin() {
                        account_group_filter
                    } else {
                        None
                    },
                    update_model_config,
                    update_routing_config,
                    update_account_group_filter,
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
