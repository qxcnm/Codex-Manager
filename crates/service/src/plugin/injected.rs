use codexmanager_core::rpc::types::JsonRpcRequest;
use codexmanager_core::storage::DomainStorage;
use serde_json::Value;

fn plugin_id(req: &JsonRpcRequest) -> Result<String, String> {
    req.params
        .as_ref()
        .and_then(|value| value.get("pluginId").or_else(|| value.get("plugin_id")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "missing pluginId".to_owned())
}

pub(crate) async fn set_enabled(
    storage: &dyn DomainStorage,
    req: &JsonRpcRequest,
    enabled: bool,
) -> Result<Value, String> {
    let id = plugin_id(req)?;
    if enabled {
        storage
            .repair_plugin_schedules(Some(id.clone()), codexmanager_core::storage::now_ts())
            .await
            .map_err(|_| "rearm plugin tasks failed".to_owned())?;
    }
    storage
        .update_plugin_status(
            id,
            if enabled { "enabled" } else { "disabled" }.to_owned(),
            None,
        )
        .await
        .map_err(|_| "update plugin status failed".to_owned())?;
    Ok(serde_json::json!({"ok": true}))
}
