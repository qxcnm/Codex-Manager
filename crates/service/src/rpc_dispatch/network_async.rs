use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};

pub(crate) fn is_async_method(method: &str) -> bool {
    matches!(
        method,
        "account/login/start"
            | "account/login/complete"
            | "codexProfile/applyDirectAccount"
            | "codexProfile/applyGateway"
            | "codexProfile/applyModels"
            | "codexProfile/restore"
            | "codexSkills/repositoryAdd"
            | "codexSkills/repositoryDelete"
            | "codexSkills/repositoryRefresh"
            | "codexSkills/repositoryInstall"
            | "codexSkills/registrySearch"
            | "codexSkills/registryInstall"
            | "plugin/catalog/list"
            | "plugin/catalog/refresh"
            | "plugin/install"
            | "plugin/update"
            | "plugin/tasks/run"
            | "account/warmup"
            | "account/fetchModels"
            | "account/test"
            | "account/test/cancel"
            | "account/read"
            | "account/chatgptAuthTokens/refresh"
            | "account/chatgptAuthTokens/refreshAll"
            | "aggregateApi/testConnection"
            | "aggregateApi/refreshBalance"
            | "aggregateApi/fetchModels"
            | "account/usage/resetCredits"
            | "account/usage/resetCredit/consume"
            | "account/proxy/test"
            | "system/proxy/test"
            | "account/usage/refresh"
            | "apikey/managedModelPriceSyncV2"
            | "gateway/codexLatestVersion/get"
    )
}

fn parse_managed_model_price_sync_params(
    req: &JsonRpcRequest,
) -> Result<crate::models_v2::ManagedModelPriceSyncV2Params, String> {
    let params = req.params.clone().unwrap_or_else(|| serde_json::json!({}));
    if params.is_null() {
        return Ok(crate::models_v2::ManagedModelPriceSyncV2Params::default());
    }
    serde_json::from_value::<crate::models_v2::ManagedModelPriceSyncV2Params>(params)
        .map_err(|error| format!("parse managed model price sync payload failed: {error}"))
}

/// The HTTP boundary authenticates the RPC token first. Keep actor authorization
/// here, before any async domain can contact a provider or mutate local state.
pub(crate) async fn try_handle_network_request_async(
    req: &JsonRpcRequest,
    actor: &crate::RpcActor,
) -> Option<JsonRpcMessage> {
    if !is_async_method(&req.method) {
        return None;
    }
    if let Err(error) = super::ensure_method_allowed(actor, &req.method) {
        return Some(JsonRpcMessage::Response(super::response(
            req,
            super::value_or_error::<()>(Err(error)),
        )));
    }
    if req.method == "gateway/codexLatestVersion/get" {
        let result =
            super::value_or_error(crate::app_settings::fetch_codex_latest_version_async().await);
        return Some(JsonRpcMessage::Response(super::response(req, result)));
    }
    if req.method == "apikey/managedModelPriceSyncV2" {
        let result = super::value_or_error(match parse_managed_model_price_sync_params(req) {
            Ok(params) => crate::models_v2::sync_prices(params).await,
            Err(error) => Err(error),
        });
        return Some(JsonRpcMessage::Response(super::response(req, result)));
    }
    if let Some(message) = super::auth_async::try_handle_auth_request_async(req, actor).await {
        return Some(message);
    }
    if let Some(message) = super::plugin_async::try_handle_plugin_request_async(req, actor).await {
        return Some(message);
    }
    if let Some(message) = super::account_async::try_handle_account_request_async(req, actor).await
    {
        return Some(message);
    }
    if let Some(message) =
        super::aggregate_async::try_handle_aggregate_request_async(req, actor).await
    {
        return Some(message);
    }
    if let Some(response) = super::account_auth_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    if let Some(response) = super::reset_credits_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    if let Some(response) = super::usage_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    if let Some(response) = super::codex_profile_async::try_handle(req).await {
        return Some(JsonRpcMessage::Response(response));
    }
    super::codex_skills_async::try_handle(req)
        .await
        .map(JsonRpcMessage::Response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price_sync_request(params: Option<serde_json::Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            id: 1.into(),
            method: "apikey/managedModelPriceSyncV2".into(),
            params,
            trace: None,
        }
    }

    #[test]
    fn managed_model_price_sync_params_default_to_all_models() {
        assert!(
            parse_managed_model_price_sync_params(&price_sync_request(None))
                .unwrap()
                .model_slugs
                .is_empty()
        );
        assert!(
            parse_managed_model_price_sync_params(&price_sync_request(Some(
                serde_json::Value::Null,
            )))
            .unwrap()
            .model_slugs
            .is_empty()
        );
        assert!(
            parse_managed_model_price_sync_params(&price_sync_request(
                Some(serde_json::json!({}),)
            ))
            .unwrap()
            .model_slugs
            .is_empty()
        );
    }

    #[test]
    fn managed_model_price_sync_params_parse_selected_slugs_and_reject_invalid_shape() {
        let parsed = parse_managed_model_price_sync_params(&price_sync_request(Some(
            serde_json::json!({"modelSlugs": ["gpt-6-sol", "gpt-6-luna"]}),
        )))
        .unwrap();
        assert_eq!(parsed.model_slugs, ["gpt-6-sol", "gpt-6-luna"]);

        let error = parse_managed_model_price_sync_params(&price_sync_request(Some(
            serde_json::json!({"modelSlugs": "gpt-6-sol"}),
        )))
        .expect_err("non-array modelSlugs must be rejected");
        assert!(error.contains("parse managed model price sync payload failed"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn network_methods_preserve_member_denials_before_side_effects() {
        let _guard = crate::test_env_guard();
        struct Restore(Vec<(&'static str, Option<std::ffi::OsString>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for (key, value) in self.0.drain(..) {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
        let _restore = Restore(
            [
                "CODEXMANAGER_DB_PATH",
                "CODEXMANAGER_DATABASE_URL",
                "CODEXMANAGER_STORAGE_BACKEND",
            ]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
        );
        let path = std::env::temp_dir().join(format!(
            "async-rpc-permissions-{}.db",
            rand::random::<u64>()
        ));
        std::env::set_var("CODEXMANAGER_DB_PATH", &path);
        std::env::set_var("CODEXMANAGER_STORAGE_BACKEND", "sqlite");
        std::env::remove_var("CODEXMANAGER_DATABASE_URL");
        crate::storage_helpers::initialize_storage().unwrap();
        crate::storage_helpers::open_storage()
            .unwrap()
            .set_app_setting("web.auth.mode", "accounts", 1)
            .unwrap();
        let actor = crate::RpcActor::from_parts(Some("member"), Some("fixture-member"));
        for method in [
            "account/login/start",
            "account/login/complete",
            "account/fetchModels",
            "account/test",
            "aggregateApi/fetchModels",
            "aggregateApi/testConnection",
            "aggregateApi/refreshBalance",
            "plugin/catalog/refresh",
            "plugin/install",
            "plugin/update",
            "plugin/tasks/run",
            "codexSkills/repositoryAdd",
            "codexSkills/registryInstall",
            "codexProfile/applyDirectAccount",
            "codexProfile/applyGateway",
            "codexProfile/applyModels",
            "system/proxy/test",
            "account/usage/resetCredits",
            "account/usage/resetCredit/consume",
            "apikey/managedModelPriceSyncV2",
        ] {
            let req = JsonRpcRequest {
                id: 42.into(),
                method: method.into(),
                params: None,
                trace: None,
            };
            let expected = crate::handle_request_with_actor(req.clone(), actor.clone());
            let actual = try_handle_network_request_async(&req, &actor)
                .await
                .expect("async method routed");
            let actual = serde_json::to_value(actual).unwrap();
            assert_eq!(actual, serde_json::to_value(expected).unwrap(), "{method}");
            assert_eq!(
                actual["result"]["errorCode"], "permission_denied",
                "{method}"
            );
        }
        for method in [
            "account/usage/resetCredits",
            "account/usage/resetCredit/consume",
        ] {
            let req = JsonRpcRequest {
                id: 44.into(),
                method: method.into(),
                params: None,
                trace: None,
            };
            let actual = serde_json::to_value(
                try_handle_network_request_async(&req, &crate::RpcActor::system_admin())
                    .await
                    .expect("admin reset-credit method is routed"),
            )
            .unwrap();
            assert_ne!(
                actual["result"]["errorCode"], "permission_denied",
                "{method}"
            );
        }
        let req = JsonRpcRequest {
            id: 43.into(),
            method: "account/proxy/test".into(),
            params: None,
            trace: None,
        };
        let expected = crate::handle_request_with_actor(req.clone(), actor.clone());
        let actual = serde_json::to_value(
            try_handle_network_request_async(&req, &actor)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(actual, serde_json::to_value(expected).unwrap());
        assert_ne!(actual["result"]["errorCode"], "permission_denied");
    }
}
