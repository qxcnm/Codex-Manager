use codexmanager_core::rpc::types::{
    InstalledPluginSummary, JsonRpcRequest, PluginRunLogSummary, PluginTaskSummary,
};
use codexmanager_core::storage::{now_ts, PluginInstallListSummary, Storage};
use serde_json::Value;

use crate::storage_helpers::open_storage;

/// 函数 `error_result`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - message: 参数 message
///
/// # 返回
/// 返回函数执行结果
fn error_result(message: impl Into<String>) -> Value {
    crate::error_codes::rpc_error_payload(message.into())
}

/// 函数 `parse_permissions`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - raw: 参数 raw
///
/// # 返回
/// 返回函数执行结果
fn parse_permissions(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 函数 `rearm_enabled_interval_tasks_for_plugin`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn rearm_enabled_interval_tasks_for_plugin(
    storage: &Storage,
    plugin_id: Option<&str>,
    now: i64,
) -> Result<(), String> {
    let storage = crate::account::remote_storage::AccountStorage::new(storage);
    storage
        .repair_plugin_task_schedules(plugin_id, now)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// 函数 `handle_list_installed`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn handle_list_installed(
    req: &JsonRpcRequest,
) -> codexmanager_core::rpc::types::JsonRpcResponse {
    match list_installed_plugins() {
        Ok(items) => super::json_response(req, serde_json::json!({ "items": items })),
        Err(err) => super::json_response(req, error_result(err)),
    }
}

/// 函数 `handle_enable`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn handle_enable(
    req: &JsonRpcRequest,
    enabled: bool,
) -> codexmanager_core::rpc::types::JsonRpcResponse {
    let Some(plugin_id) = req
        .params
        .as_ref()
        .and_then(|value| value.get("pluginId").or_else(|| value.get("plugin_id")))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return super::json_response(req, error_result("missing pluginId"));
    };

    let Some(storage) = open_storage() else {
        return super::json_response(req, error_result("storage unavailable"));
    };
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    if enabled
        && rearm_enabled_interval_tasks_for_plugin(&storage, Some(&plugin_id), now_ts()).is_err()
    {
        return super::json_response(req, error_result("rearm plugin tasks failed"));
    }
    if storage
        .update_plugin_install_status(
            &plugin_id,
            if enabled { "enabled" } else { "disabled" },
            None,
        )
        .is_err()
    {
        return super::json_response(req, error_result("update plugin status failed"));
    }
    super::json_response(req, serde_json::json!({ "ok": true }))
}

/// 函数 `handle_task_update`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn handle_task_update(
    req: &JsonRpcRequest,
) -> codexmanager_core::rpc::types::JsonRpcResponse {
    let Some(task_id) = req
        .params
        .as_ref()
        .and_then(|value| value.get("taskId").or_else(|| value.get("task_id")))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return super::json_response(req, error_result("missing taskId"));
    };

    let Some(interval_seconds) = req
        .params
        .as_ref()
        .and_then(|value| {
            value
                .get("intervalSeconds")
                .or_else(|| value.get("interval_seconds"))
        })
        .and_then(|value| value.as_i64())
    else {
        return super::json_response(req, error_result("missing intervalSeconds"));
    };

    if interval_seconds <= 0 {
        return super::json_response(req, error_result("intervalSeconds must be greater than 0"));
    }

    let Some(storage) = open_storage() else {
        return super::json_response(req, error_result("storage unavailable"));
    };
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let Some(task) = storage.find_plugin_task(&task_id).ok().flatten() else {
        return super::json_response(req, error_result("task not found"));
    };

    let task_json = match serde_json::from_str::<serde_json::Value>(&task.task_json) {
        Ok(mut value) => {
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "intervalSeconds".to_string(),
                    serde_json::json!(interval_seconds),
                );
                obj.insert("scheduleKind".to_string(), serde_json::json!("interval"));
            }
            match serde_json::to_string(&value) {
                Ok(text) => text,
                Err(_) => task.task_json.clone(),
            }
        }
        Err(_) => task.task_json.clone(),
    };

    let next_run_at = if task.enabled {
        Some(now_ts() + interval_seconds)
    } else {
        None
    };
    if storage
        .update_plugin_task_definition(
            &task.id,
            &task.name,
            task.description.as_deref(),
            &task.entrypoint,
            "interval",
            Some(interval_seconds),
            task.enabled,
            next_run_at,
            &task_json,
        )
        .is_err()
    {
        return super::json_response(req, error_result("update task failed"));
    }

    super::json_response(
        req,
        serde_json::json!({
            "ok": true,
            "taskId": task.id,
            "intervalSeconds": interval_seconds,
        }),
    )
}

/// 函数 `handle_task_list`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn handle_task_list(
    req: &JsonRpcRequest,
) -> codexmanager_core::rpc::types::JsonRpcResponse {
    let plugin_id = req
        .params
        .as_ref()
        .and_then(|value| value.get("pluginId").or_else(|| value.get("plugin_id")))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    match list_plugin_tasks(plugin_id.as_deref()) {
        Ok(items) => super::json_response(req, serde_json::json!({ "items": items })),
        Err(err) => super::json_response(req, error_result(err)),
    }
}

/// 函数 `handle_log_list`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn handle_log_list(
    req: &JsonRpcRequest,
) -> codexmanager_core::rpc::types::JsonRpcResponse {
    let plugin_id = req
        .params
        .as_ref()
        .and_then(|value| value.get("pluginId").or_else(|| value.get("plugin_id")))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let task_id = req
        .params
        .as_ref()
        .and_then(|value| value.get("taskId").or_else(|| value.get("task_id")))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = req
        .params
        .as_ref()
        .and_then(|value| value.get("limit"))
        .and_then(|value| value.as_i64())
        .unwrap_or(50)
        .max(1);
    match list_plugin_run_logs(plugin_id.as_deref(), task_id.as_deref(), limit) {
        Ok(items) => super::json_response(req, serde_json::json!({ "items": items })),
        Err(err) => super::json_response(req, error_result(err)),
    }
}

/// 函数 `list_installed_plugins`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn list_installed_plugins() -> Result<Vec<InstalledPluginSummary>, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let installs = storage
        .list_plugin_install_summaries()
        .map_err(|err| err.to_string())?;
    let task_count_by_plugin = storage
        .plugin_task_counts_by_plugin()
        .map_err(|err| err.to_string())?;

    installs
        .into_iter()
        .map(|install| {
            let task_counts = task_count_by_plugin
                .get(&install.plugin_id)
                .cloned()
                .unwrap_or_default();
            Ok(to_installed_plugin_summary(
                &install,
                task_counts.task_count,
                task_counts.enabled_task_count,
            ))
        })
        .collect()
}

/// 函数 `list_plugin_tasks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn list_plugin_tasks(plugin_id: Option<&str>) -> Result<Vec<PluginTaskSummary>, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let tasks = storage
        .list_plugin_task_summaries(plugin_id)
        .map_err(|err| err.to_string())?;
    tasks
        .into_iter()
        .map(|task| {
            Ok(PluginTaskSummary {
                id: task.id,
                plugin_id: task.plugin_id.clone(),
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
        })
        .collect()
}

/// 函数 `list_plugin_run_logs`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn list_plugin_run_logs(
    plugin_id: Option<&str>,
    task_id: Option<&str>,
    limit: i64,
) -> Result<Vec<PluginRunLogSummary>, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let logs = storage
        .list_plugin_run_log_summaries(plugin_id, task_id, limit)
        .map_err(|err| err.to_string())?;

    logs.into_iter()
        .map(|log| {
            Ok(PluginRunLogSummary {
                id: log.id,
                plugin_id: log.plugin_id.clone(),
                plugin_name: log.plugin_name,
                task_id: log.task_id.clone(),
                task_name: log.task_name,
                run_type: log.run_type,
                status: log.status,
                started_at: log.started_at,
                finished_at: log.finished_at,
                duration_ms: log.duration_ms,
                output: log
                    .output_json
                    .as_ref()
                    .and_then(|raw| serde_json::from_str(raw).ok()),
                error: log.error,
            })
        })
        .collect()
}

/// 函数 `to_installed_plugin_summary`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - plugin: 参数 plugin
/// - task_count: 参数 task_count
/// - enabled_task_count: 参数 enabled_task_count
///
/// # 返回
/// 返回函数执行结果
pub(crate) fn to_installed_plugin_summary(
    plugin: &PluginInstallListSummary,
    task_count: i64,
    enabled_task_count: i64,
) -> InstalledPluginSummary {
    let manifest_version = plugin
        .manifest_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "1".to_string());
    let category = plugin
        .category
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let runtime_kind = plugin
        .runtime_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "rhai".to_string());
    let tags = plugin
        .tags_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default();
    InstalledPluginSummary {
        plugin_id: plugin.plugin_id.clone(),
        source_url: plugin.source_url.clone(),
        name: plugin.name.clone(),
        version: plugin.version.clone(),
        description: plugin.description.clone(),
        author: plugin.author.clone(),
        homepage_url: plugin.homepage_url.clone(),
        script_url: plugin.script_url.clone(),
        permissions: parse_permissions(&plugin.permissions_json),
        status: plugin.status.clone(),
        installed_at: plugin.installed_at,
        updated_at: plugin.updated_at,
        last_run_at: plugin.last_run_at,
        last_error: plugin.last_error.clone(),
        task_count,
        enabled_task_count,
        manifest_version,
        category,
        runtime_kind,
        tags,
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
