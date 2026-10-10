use codexmanager_core::rpc::types::{JsonRpcMessage, JsonRpcRequest};

pub(crate) async fn try_handle_account_request_async(
    req: &JsonRpcRequest,
    actor: &crate::RpcActor,
) -> Option<JsonRpcMessage> {
    if !matches!(
        req.method.as_str(),
        "account/warmup"
            | "account/fetchModels"
            | "account/test"
            | "account/test/cancel"
            | "account/proxy/test"
            | "system/proxy/test"
    ) {
        return None;
    }
    let permission = super::ensure_method_allowed(actor, &req.method).and_then(|_| {
        if matches!(
            req.method.as_str(),
            "account/fetchModels" | "account/test" | "account/test/cancel"
        ) && !actor.is_admin()
        {
            Err(super::permission_denied(&req.method))
        } else {
            Ok(())
        }
    });
    if let Err(error) = permission {
        return Some(JsonRpcMessage::Response(super::response(
            req,
            super::value_or_error::<()>(Err(error)),
        )));
    }
    let account_id = first_str(req, &["accountId", "account_id"]).unwrap_or("");
    let result = match req.method.as_str() {
        "account/proxy/test" => super::value_or_error(
            crate::account_proxy::test_account_proxy_settings_async(
                account_id,
                super::bool_param(req, "enabled"),
                first_str(req, &["source", "proxySource", "proxy_source"]),
                first_str(req, &["proxyProfileId", "proxy_profile_id"]),
                first_str(req, &["proxyUrl", "proxy_url"]),
            )
            .await,
        ),
        "system/proxy/test" => super::value_or_error(
            crate::proxy_registry::test_proxy_profile_async(
                first_str(req, &["id", "proxyId"]).unwrap_or(""),
            )
            .await,
        ),
        "account/warmup" => {
            let account_ids = req
                .params
                .as_ref()
                .and_then(|params| {
                    params
                        .get("accountIds")
                        .or_else(|| params.get("account_ids"))
                })
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            super::value_or_error(
                crate::account_warmup::warmup_accounts_async(
                    account_ids,
                    first_str(req, &["message"]).unwrap_or(""),
                )
                .await,
            )
        }
        "account/fetchModels" => super::value_or_error(
            crate::account_models::fetch_account_models_async(account_id).await,
        ),
        "account/test" => super::value_or_error(crate::account_test::start_account_test(
            account_id,
            first_str(req, &["model", "modelSlug", "model_slug"]).map(str::to_string),
            first_str(req, &["prompt", "message"]).map(str::to_string),
            first_str(req, &["kind", "testType", "test_type"]).map(str::to_string),
            first_str(req, &["testId", "test_id"]).map(str::to_string),
        )),
        "account/test/cancel" => super::value_or_error(crate::account_test::cancel_account_test(
            account_id,
            first_str(req, &["testId", "test_id"]).unwrap_or(""),
        )),
        _ => unreachable!(),
    };
    Some(JsonRpcMessage::Response(super::response(req, result)))
}

fn first_str<'a>(req: &'a JsonRpcRequest, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| super::str_param(req, key))
}
