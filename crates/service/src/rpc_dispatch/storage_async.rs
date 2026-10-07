//! Native persistence RPCs use the listener's injected domain store.
use super::*;
use crate::http::state::AppState;
use std::sync::Arc;

pub(crate) async fn handle(
    state: Arc<AppState>,
    req: &JsonRpcRequest,
    actor: &RpcActor,
) -> Option<JsonRpcMessage> {
    if !matches!(
        req.method.as_str(),
        "modelGroups/list"
            | "modelGroups/save"
            | "modelGroups/delete"
            | "modelGroups/setModels"
            | "modelGroups/setUsers"
            | "apikey/list"
            | "apikey/create"
            | "apikey/updateModel"
            | "apikey/disable"
            | "apikey/enable"
            | "apikey/delete"
            | "accountManager/profile/update"
            | "accountManager/password/change"
            | "accountManager/apiKeyOwners/set"
            | "accountManager/wallet/topUp"
            | "accountManager/wallet/setAvailable"
            | "accountManager/users/list"
            | "accountManager/users/create"
            | "accountManager/users/update"
            | "accountManager/users/delete"
            | "plugin/list"
            | "plugin/tasks/list"
            | "plugin/enable"
            | "plugin/disable"
            | "requestlog/clear"
    ) {
        return None;
    }
    if let Err(error) = ensure_method_allowed(actor, &req.method) {
        return Some(JsonRpcMessage::Response(response(
            req,
            crate::error_codes::rpc_error_payload(error),
        )));
    }
    let result = async {
        if *state.shutdown.borrow() {
            return Err("service is shutting down".to_owned());
        }
        state.signal_task("rpc.storage");
        let _permit = state
            .rpc_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| "service busy".to_owned())?;
        let storage = state.storage().await?;
        use crate::model_groups::injected as groups;
        match req.method.as_str() {
            "apikey/list" => super::storage_reads::api_keys(storage.as_ref(), actor).await,
            "apikey/create" => crate::apikey::injected::create(
                storage.as_ref(),
                actor,
                string_param(req, "name"),
                string_param(req, "modelSlug"),
                string_param(req, "reasoningEffort"),
                string_param(req, "serviceTier"),
                string_param(req, "protocolType"),
                if actor.is_admin() {
                    string_param(req, "upstreamBaseUrl")
                } else {
                    None
                },
                if actor.is_admin() {
                    string_param(req, "staticHeadersJson")
                } else {
                    None
                },
                if actor.is_admin() {
                    string_param(req, "rotationStrategy")
                } else {
                    None
                },
                if actor.is_admin() {
                    string_param(req, "aggregateApiId")
                } else {
                    None
                },
                if actor.is_admin() {
                    string_param(req, "accountPlanFilter")
                } else {
                    None
                },
                if actor.is_admin() {
                    string_param(req, "accountGroupFilter")
                } else {
                    None
                },
                i64_param(req, "quotaLimitTokens"),
                string_param(req, "customKey"),
            )
            .await
            .map(as_json),
            "apikey/updateModel" => {
                let params = req.params.as_ref().and_then(|value| value.as_object());
                crate::apikey::injected::update(
                    storage.as_ref(),
                    actor,
                    str_param(req, "id").unwrap_or(""),
                    string_param(req, "name"),
                    params.is_some_and(|p| p.contains_key("name")),
                    string_param(req, "modelSlug"),
                    string_param(req, "reasoningEffort"),
                    string_param(req, "serviceTier"),
                    string_param(req, "protocolType"),
                    if actor.is_admin() {
                        string_param(req, "upstreamBaseUrl")
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        string_param(req, "staticHeadersJson")
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        string_param(req, "rotationStrategy")
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        string_param(req, "aggregateApiId")
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        string_param(req, "accountPlanFilter")
                    } else {
                        None
                    },
                    if actor.is_admin() {
                        string_param(req, "accountGroupFilter")
                    } else {
                        None
                    },
                    params.is_some_and(|p| {
                        p.contains_key("modelSlug")
                            || p.contains_key("reasoningEffort")
                            || p.contains_key("serviceTier")
                    }),
                    actor.is_admin()
                        && params.is_some_and(|p| {
                            p.contains_key("rotationStrategy")
                                || p.contains_key("aggregateApiId")
                                || p.contains_key("accountPlanFilter")
                        }),
                    actor.is_admin()
                        && params.is_some_and(|p| p.contains_key("accountGroupFilter")),
                    params.is_some_and(|p| p.contains_key("quotaLimitTokens")),
                    i64_param(req, "quotaLimitTokens"),
                )
                .await
                .map(|_| ok_result())
            }
            "apikey/delete" | "apikey/disable" | "apikey/enable" => {
                let id = str_param(req, "id").unwrap_or("");
                if !actor.is_admin() {
                    let user_id = actor
                        .user_id
                        .as_deref()
                        .ok_or("permission_denied: apikey requires user session")?;
                    let owner = storage.api_key_owner(id.to_owned()).await?;
                    if !owner.is_some_and(|owner| {
                        owner.owner_kind == "user"
                            && owner.owner_user_id.as_deref() == Some(user_id)
                    }) {
                        return Err("permission_denied: apikey".to_owned());
                    }
                }
                if id.is_empty() {
                    return Err("missing id".to_owned());
                }
                if req.method == "apikey/delete" {
                    return storage
                        .delete_api_key(id.to_owned())
                        .await
                        .map(|_| ok_result());
                }
                let status = if req.method == "apikey/enable" {
                    "active"
                } else {
                    "disabled"
                };
                storage
                    .set_api_key_status(id.to_owned(), status.to_owned())
                    .await
                    .map(|_| ok_result())
            }
            "accountManager/users/list" => super::storage_reads::users(storage.as_ref()).await,
            "accountManager/users/create" => {
                let input = req
                    .params
                    .clone()
                    .ok_or_else(|| "missing user payload".to_owned())
                    .and_then(|value| {
                        serde_json::from_value::<crate::AppUserCreateInput>(value)
                            .map_err(|error| format!("invalid user payload: {error}"))
                    })?;
                super::account_manager_storage::create_user(storage.as_ref(), input)
                    .await
                    .map(as_json)
            }
            "accountManager/users/update" => {
                let input = req
                    .params
                    .clone()
                    .ok_or_else(|| "missing user payload".to_owned())
                    .and_then(|value| {
                        serde_json::from_value::<crate::AppUserUpdateInput>(value)
                            .map_err(|error| format!("invalid user payload: {error}"))
                    })?;
                super::account_manager_storage::update_user(storage.as_ref(), input)
                    .await
                    .map(as_json)
            }
            "accountManager/users/delete" => super::account_manager_storage::delete_user(
                storage.as_ref(),
                str_param(req, "id").unwrap_or(""),
            )
            .await
            .map(|_| ok_result()),
            "accountManager/profile/update" => super::account_manager_storage::update_profile(
                storage.as_ref(),
                actor,
                str_param(req, "displayName"),
            )
            .await
            .map(as_json),
            "accountManager/password/change" => super::account_manager_storage::change_password(
                storage.as_ref(),
                actor,
                str_param(req, "currentPassword").unwrap_or(""),
                str_param(req, "newPassword").unwrap_or(""),
            )
            .await
            .map(|_| ok_result()),
            "accountManager/apiKeyOwners/set" => super::account_manager_storage::set_api_key_owner(
                storage.as_ref(),
                str_param(req, "keyId").unwrap_or(""),
                str_param(req, "ownerKind").unwrap_or("user"),
                str_param(req, "ownerUserId"),
                str_param(req, "projectId"),
            )
            .await
            .map(as_json),
            "accountManager/wallet/topUp" => super::account_manager_storage::wallet_top_up(
                storage.as_ref(),
                str_param(req, "ownerKind").unwrap_or("user"),
                str_param(req, "ownerId").unwrap_or(""),
                i64_param(req, "amountCreditMicros").unwrap_or(0),
                str_param(req, "note"),
                str_param(req, "createdByUserId"),
            )
            .await
            .map(as_json),
            "accountManager/wallet/setAvailable" => {
                super::account_manager_storage::wallet_set_available(
                    storage.as_ref(),
                    str_param(req, "ownerKind").unwrap_or("user"),
                    str_param(req, "ownerId").unwrap_or(""),
                    i64_param(req, "availableCreditMicros").unwrap_or(0),
                    str_param(req, "note"),
                    str_param(req, "createdByUserId"),
                )
                .await
                .map(as_json)
            }
            "plugin/list" => super::storage_reads::plugins(storage.as_ref()).await,
            "plugin/tasks/list" => super::storage_reads::tasks(storage.as_ref(), req).await,
            "plugin/enable" => crate::plugin::injected::set_enabled(storage.as_ref(), req, true)
                .await
                .map(as_json),
            "plugin/disable" => crate::plugin::injected::set_enabled(storage.as_ref(), req, false)
                .await
                .map(as_json),
            "requestlog/clear" => {
                // Keep the in-memory payload clear generation and the spill
                // files in sync with the database clear.
                let token = crate::gateway::begin_request_log_payload_clear();
                let result = storage.clear_request_logs().await;
                crate::gateway::finish_request_log_payload_clear(token, result.is_ok());
                result.map(|_| ok_result())
            }
            "modelGroups/list" => groups::list(storage.as_ref()).await.map(as_json),
            "modelGroups/save" => groups::save(storage.as_ref(), params(req)?)
                .await
                .map(as_json),
            "modelGroups/delete" => groups::delete(
                storage.as_ref(),
                str_param(req, "id").unwrap_or("").to_owned(),
            )
            .await
            .map(as_json),
            "modelGroups/setModels" => groups::models(storage.as_ref(), params(req)?)
                .await
                .map(as_json),
            "modelGroups/setUsers" => groups::users(storage.as_ref(), params(req)?)
                .await
                .map(as_json),
            _ => unreachable!(),
        }
    }
    .await;
    Some(JsonRpcMessage::Response(response(
        req,
        value_or_error(result),
    )))
}

fn params<T: serde::de::DeserializeOwned>(req: &JsonRpcRequest) -> Result<T, String> {
    serde_json::from_value(req.params.clone().ok_or("missing model group payload")?)
        .map_err(|error| format!("invalid model group payload: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexmanager_core::storage::{RequestLog, Storage};
    use codexmanager_storage_seaorm::SqliteDomainStorage;

    fn fixture_log() -> RequestLog {
        RequestLog {
            trace_id: Some("trace-domain-storage-clear".into()),
            key_id: None,
            account_id: None,
            initial_account_id: None,
            attempted_account_ids_json: None,
            initial_aggregate_api_id: None,
            attempted_aggregate_api_ids_json: None,
            request_path: "/v1/responses".into(),
            original_path: None,
            adapted_path: None,
            method: "POST".into(),
            request_type: Some("responses".into()),
            gateway_mode: None,
            route_strategy: None,
            route_source: None,
            transparent_mode: None,
            enhanced_mode: None,
            client_model: Some("fixture-model".into()),
            model: Some("fixture-model".into()),
            model_source: None,
            upstream_model: None,
            actual_source_kind: None,
            actual_source_id: None,
            client_reasoning_effort: None,
            reasoning_effort: None,
            reasoning_source: None,
            service_tier: None,
            effective_service_tier: None,
            service_tier_source: None,
            response_adapter: None,
            upstream_url: None,
            aggregate_api_supplier_name: None,
            aggregate_api_url: None,
            status_code: Some(200),
            duration_ms: Some(1),
            first_response_ms: Some(1),
            input_tokens: Some(1),
            cached_input_tokens: Some(0),
            output_tokens: Some(1),
            total_tokens: Some(2),
            reasoning_output_tokens: Some(0),
            estimated_cost_usd: Some(0.01),
            error: None,
            created_at: 1,
        }
    }

    #[tokio::test]
    async fn request_log_clear_uses_injected_domain_storage() {
        let storage = Storage::open_in_memory().expect("storage");
        storage.init().expect("schema");
        storage
            .insert_request_log(&fixture_log())
            .expect("seed request log");
        let verifier = storage.shared_handle();
        let state = AppState::with_storage(Arc::new(SqliteDomainStorage::new(storage)));
        let request = JsonRpcRequest {
            id: 1.into(),
            method: "requestlog/clear".into(),
            params: None,
            trace: None,
        };

        let message = handle(state, &request, &RpcActor::system_admin())
            .await
            .expect("storage method response");
        let JsonRpcMessage::Response(response) = message else {
            panic!("expected JSON-RPC response");
        };
        assert_eq!(response.result["ok"], true);
        assert!(verifier
            .list_request_logs(None, 10)
            .expect("read cleared logs")
            .is_empty());
    }

    #[tokio::test]
    async fn admin_api_key_delete_uses_injected_domain_storage() {
        let storage = Storage::open_in_memory().expect("storage");
        storage.init().expect("schema");
        let key = codexmanager_core::storage::ApiKey {
            id: "domain-storage-delete-key".into(),
            name: Some("domain storage test".into()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "round_robin".into(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".into(),
            protocol_type: "openai_compat".into(),
            auth_scheme: "authorization_bearer".into(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: "domain-storage-delete-hash".into(),
            status: "active".into(),
            created_at: 1,
            last_used_at: None,
        };
        storage.insert_api_key(&key).expect("seed api key");
        let verifier = storage.shared_handle();
        let state = AppState::with_storage(Arc::new(SqliteDomainStorage::new(storage)));
        let request = JsonRpcRequest {
            id: 2.into(),
            method: "apikey/delete".into(),
            params: Some(serde_json::json!({"id": key.id})),
            trace: None,
        };

        let message = handle(state, &request, &RpcActor::system_admin())
            .await
            .expect("storage method response");
        let JsonRpcMessage::Response(response) = message else {
            panic!("expected JSON-RPC response");
        };
        assert_eq!(response.result["ok"], true);
        assert!(verifier
            .find_api_key_by_id("domain-storage-delete-key")
            .expect("read deleted key")
            .is_none());
    }

    #[tokio::test]
    async fn admin_api_key_status_uses_injected_domain_storage() {
        let storage = Storage::open_in_memory().expect("storage");
        storage.init().expect("schema");
        let key = codexmanager_core::storage::ApiKey {
            id: "domain-storage-status-key".into(),
            name: Some("domain storage status test".into()),
            model_slug: None,
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "round_robin".into(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".into(),
            protocol_type: "openai_compat".into(),
            auth_scheme: "authorization_bearer".into(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: "domain-storage-status-hash".into(),
            status: "active".into(),
            created_at: 1,
            last_used_at: None,
        };
        storage.insert_api_key(&key).expect("seed api key");
        let verifier = storage.shared_handle();
        let state = AppState::with_storage(Arc::new(SqliteDomainStorage::new(storage)));

        for (id, method, expected_status) in [
            (3, "apikey/disable", "disabled"),
            (4, "apikey/enable", "active"),
        ] {
            let request = JsonRpcRequest {
                id: id.into(),
                method: method.into(),
                params: Some(serde_json::json!({"id": key.id})),
                trace: None,
            };
            let message = handle(state.clone(), &request, &RpcActor::system_admin())
                .await
                .expect("storage method response");
            let JsonRpcMessage::Response(response) = message else {
                panic!("expected JSON-RPC response");
            };
            assert_eq!(response.result["ok"], true);
            assert_eq!(
                verifier
                    .find_api_key_by_id(&key.id)
                    .expect("read api key")
                    .expect("api key")
                    .status,
                expected_status
            );
        }
    }
}
