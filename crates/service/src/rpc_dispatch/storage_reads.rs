use codexmanager_core::{rpc::types::*, storage::DomainStorage};
use serde_json::Value;

pub(super) async fn api_keys(
    storage: &dyn DomainStorage,
    actor: &crate::RpcActor,
) -> Result<Value, String> {
    let user_id = if actor.is_admin() {
        None
    } else {
        Some(
            actor
                .user_id
                .clone()
                .ok_or("permission_denied: apikey requires user session")?,
        )
    };
    let items = storage
        .api_key_summaries(user_id)
        .await?
        .into_iter()
        .map(crate::apikey_list::map_api_key_list_summary)
        .collect();
    Ok(super::as_json(ApiKeyListResult { items }))
}

pub(super) async fn users(storage: &dyn DomainStorage) -> Result<Value, String> {
    let mut result = Vec::new();
    for user in storage.users().await? {
        let wallet = storage.wallet("user".into(), user.id.clone()).await?;
        result.push(crate::auth::app_manager::public_user(user, wallet));
    }
    Ok(super::as_json(result))
}

pub(super) async fn plugins(storage: &dyn DomainStorage) -> Result<Value, String> {
    let tasks = storage.plugin_tasks(None).await?;
    let items: Vec<_> = storage
        .plugins()
        .await?
        .into_iter()
        .map(|plugin| {
            let own: Vec<_> = tasks
                .iter()
                .filter(|t| t.plugin_id == plugin.plugin_id)
                .collect();
            crate::plugin::store::to_installed_plugin_summary(
                &plugin,
                own.len() as i64,
                own.iter().filter(|t| t.enabled).count() as i64,
            )
        })
        .collect();
    Ok(serde_json::json!({ "items": items }))
}

pub(super) async fn tasks(
    storage: &dyn DomainStorage,
    req: &JsonRpcRequest,
) -> Result<Value, String> {
    let id = super::str_param(req, "pluginId")
        .or_else(|| super::str_param(req, "plugin_id"))
        .map(str::to_owned);
    let items: Vec<_> = storage
        .plugin_tasks(id)
        .await?
        .into_iter()
        .map(|task| PluginTaskSummary {
            id: task.id,
            plugin_id: task.plugin_id,
            plugin_name: task.plugin_name,
            name: task.name,
            description: task.description,
            entrypoint: task.entrypoint,
            schedule_kind: task.schedule_kind,
            interval_seconds: task.interval_seconds,
            enabled: task.enabled,
            next_run_at: task.next_run_at,
            last_run_at: task.last_run_at,
            last_status: task.last_status,
            last_error: task.last_error,
        })
        .collect();
    Ok(serde_json::json!({ "items": items }))
}
