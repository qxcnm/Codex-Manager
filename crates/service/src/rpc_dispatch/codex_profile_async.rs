use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) async fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "codexProfile/applyDirectAccount" => super::value_or_error(
            crate::codex_profile::apply_direct_account_async(
                super::str_param(req, "accountId"),
                super::str_param(req, "codexHome"),
                super::bool_param(req, "reloadAfterSwitch").unwrap_or(false),
            )
            .await,
        ),
        "codexProfile/applyDirectAggregate" => super::value_or_error(
            crate::codex_profile::apply_direct_aggregate_async(
                super::str_param(req, "aggregateApiId"),
                super::str_param(req, "codexHome"),
                super::bool_param(req, "reloadAfterSwitch").unwrap_or(false),
            )
            .await,
        ),
        "codexProfile/applyGateway" => super::value_or_error(
            crate::codex_profile::apply_gateway_async(
                super::str_param(req, "apiKeyId"),
                super::str_param(req, "codexHome"),
                super::str_param(req, "baseUrl"),
                super::bool_param(req, "supportsWebsockets"),
                super::bool_param(req, "reloadAfterSwitch").unwrap_or(false),
            )
            .await,
        ),
        "codexProfile/applyModels" => super::value_or_error(
            crate::codex_profile::apply_models_async(
                super::str_param(req, "codexHome"),
                super::string_array_param(req, "modelSlugs"),
                super::bool_param(req, "reloadAfterSwitch").unwrap_or(false),
            )
            .await,
        ),
        "codexProfile/restore" => super::value_or_error(
            crate::codex_profile::restore_async(super::str_param(req, "codexHome")).await,
        ),
        _ => return None,
    };
    Some(super::response(req, result))
}
