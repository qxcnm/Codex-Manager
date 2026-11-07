use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
use serde::Deserialize;

use crate::RpcActor;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateParams {
    source_kind: String,
    source_id: String,
    enabled: bool,
    interval_minutes: Option<u64>,
}

pub(super) fn try_handle(req: &JsonRpcRequest, actor: &RpcActor) -> Option<JsonRpcResponse> {
    if !matches!(
        req.method.as_str(),
        "modelDiscovery/list" | "modelDiscovery/update"
    ) {
        return None;
    }
    // Keep this guard local as well as in the shared dispatcher: password-mode
    // compatibility must not expose global discovery settings to members.
    if !actor.is_admin() {
        return Some(super::response(
            req,
            super::value_or_error::<()>(Err(super::permission_denied(&req.method))),
        ));
    }
    let result = match req.method.as_str() {
        "modelDiscovery/list" => super::value_or_error(crate::model_discovery::list_tasks()),
        "modelDiscovery/update" => {
            let params = req
                .params
                .as_ref()
                .filter(|params| params.is_object())
                .ok_or_else(|| "model discovery update params must be an object".to_owned())
                .and_then(|params| {
                    serde_json::from_value::<UpdateParams>(params.clone())
                        .map_err(|err| format!("invalid model discovery update params: {err}"))
                });
            super::value_or_error(params.and_then(|params| {
                crate::model_discovery::update_task(
                    &params.source_kind,
                    &params.source_id,
                    params.enabled,
                    params.interval_minutes,
                )
            }))
        }
        _ => unreachable!(),
    };
    Some(super::response(req, result))
}

#[cfg(test)]
#[path = "model_discovery_tests.rs"]
mod tests;
