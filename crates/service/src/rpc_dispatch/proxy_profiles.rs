use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};

pub(super) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "proxyProfiles/list" => super::value_or_error(crate::proxy_profiles::list()),
        "proxyProfiles/upsert" => super::value_or_error(crate::proxy_profiles::upsert(
            super::str_param(req, "id"),
            super::str_param(req, "name").unwrap_or(""),
            super::str_param(req, "proxyUrl").unwrap_or(""),
            super::str_param(req, "username"),
            super::str_param(req, "password"),
            super::bool_param(req, "keepExistingPassword").unwrap_or(true),
            super::str_param(req, "status"),
            super::str_param(req, "fallbackMode"),
            super::str_param(req, "backupProxyId"),
        )),
        "proxyProfiles/delete" => super::ok_or_error(crate::proxy_profiles::delete(
            super::str_param(req, "id").unwrap_or(""),
        )),
        "proxyProfiles/probe" => super::value_or_error(crate::proxy_profiles::probe(
            super::str_param(req, "id").unwrap_or(""),
        )),
        "proxyProfiles/bindAccounts" => {
            let account_ids = req.params.as_ref().and_then(|params| params.get("accountIds"))
                .and_then(|value| value.as_array()).map(|items| items.iter()
                    .filter_map(|item| item.as_str()).map(str::to_string).collect::<Vec<_>>())
                .unwrap_or_default();
            super::value_or_error(crate::proxy_profiles::bind_accounts(
                account_ids,
                super::str_param(req, "mode").unwrap_or("inherit"),
                super::str_param(req, "proxyProfileId"),
            ))
        }
        _ => return None,
    };
    Some(super::response(req, result))
}
