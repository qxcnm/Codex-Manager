use codexmanager_core::rpc::types::{
    InstalledPluginSummary, JsonRpcRequest, JsonRpcResponse, PluginCatalogEntry, PluginCatalogTask,
    PluginTaskSummary,
};
use codexmanager_core::storage::{now_ts, PluginInstall, PluginTask};
use serde_json::Value;

use crate::storage_helpers::open_storage;

const BUILTIN_MARKET_SOURCE_URL: &str = "builtin://codexmanager";
const BUILTIN_CLEANUP_TASK_INTERVAL_SECS: i64 = 60;
const BUILTIN_UNAVAILABLE_FREE_CLEANUP_TASK_INTERVAL_SECS: i64 = 60;
const BUILTIN_MARKET_MODE: &str = "builtin";
const CUSTOM_MARKET_MODE: &str = "custom";

/// 函数 `handle_catalog_list`
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
pub(crate) fn handle_catalog_list(req: &JsonRpcRequest) -> JsonRpcResponse {
    match catalog_list_result(req) {
        Ok(value) => super::json_response(req, value),
        Err(err) => super::json_response(req, error_result(err)),
    }
}

pub(super) async fn handle_catalog_list_async(req: &JsonRpcRequest) -> JsonRpcResponse {
    let result = catalog_list_result_async(req).await;
    super::json_response(req, result.unwrap_or_else(error_result))
}

pub(super) async fn handle_install_async(req: &JsonRpcRequest, is_update: bool) -> JsonRpcResponse {
    let result = install_or_update_plugin_async(req, is_update).await;
    super::json_response(req, result.unwrap_or_else(error_result))
}

/// 函数 `handle_install`
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
pub(crate) fn handle_install(req: &JsonRpcRequest) -> JsonRpcResponse {
    match install_or_update_plugin(req, false) {
        Ok(value) => super::json_response(req, value),
        Err(err) => super::json_response(req, error_result(err)),
    }
}

/// 函数 `handle_update`
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
pub(crate) fn handle_update(req: &JsonRpcRequest) -> JsonRpcResponse {
    match install_or_update_plugin(req, true) {
        Ok(value) => super::json_response(req, value),
        Err(err) => super::json_response(req, error_result(err)),
    }
}

/// 函数 `handle_uninstall`
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
pub(crate) fn handle_uninstall(req: &JsonRpcRequest) -> JsonRpcResponse {
    let Some(plugin_id) = string_param(req, "pluginId").or_else(|| string_param(req, "plugin_id"))
    else {
        return super::json_response(req, error_result("missing pluginId"));
    };

    let Some(storage) = open_storage() else {
        return super::json_response(req, error_result("storage unavailable"));
    };
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    if storage.delete_plugin_install(&plugin_id).is_err() {
        return super::json_response(req, error_result("uninstall plugin failed"));
    }
    super::json_response(req, serde_json::json!({ "ok": true }))
}

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

/// 函数 `string_param`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
/// - key: 参数 key
///
/// # 返回
/// 返回函数执行结果
fn string_param(req: &JsonRpcRequest, key: &str) -> Option<String> {
    req.params
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 函数 `catalog_list_result`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
///
/// # 返回
/// 返回函数执行结果
fn catalog_list_result(req: &JsonRpcRequest) -> Result<Value, String> {
    super::runtime::run_network_future(catalog_list_result_async(req))
}

async fn catalog_list_result_async(req: &JsonRpcRequest) -> Result<Value, String> {
    let request = req.clone();
    let (market_mode, source_url) = super::runtime::run_storage(move || {
        Ok((
            market_source_mode_for_request(&request),
            source_url_from_request(&request),
        ))
    })
    .await?;
    let items = if market_mode == CUSTOM_MARKET_MODE {
        if source_url.is_some() {
            match fetch_catalog_entries(source_url.as_deref()).await {
                Ok(items) => items,
                Err(err) => {
                    log::warn!("fetch custom plugin catalog failed: {err}");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
    } else {
        builtin_catalog_entries()
    };
    Ok(serde_json::json!({
        "sourceUrl": source_url.unwrap_or_default(),
        "items": items,
    }))
}

/// 函数 `requested_market_source_mode`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
///
/// # 返回
/// 返回函数执行结果
fn requested_market_source_mode(req: &JsonRpcRequest) -> Option<String> {
    string_param(req, "marketMode")
        .or_else(|| string_param(req, "market_mode"))
        .map(|value| match value.trim().to_ascii_lowercase().as_str() {
            CUSTOM_MARKET_MODE | "private" => CUSTOM_MARKET_MODE.to_string(),
            _ => BUILTIN_MARKET_MODE.to_string(),
        })
}

/// 函数 `market_source_mode_for_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
///
/// # 返回
/// 返回函数执行结果
fn market_source_mode_for_request(req: &JsonRpcRequest) -> String {
    requested_market_source_mode(req).unwrap_or_else(current_market_source_mode)
}

/// 函数 `source_url_from_request`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
///
/// # 返回
/// 返回函数执行结果
fn source_url_from_request(req: &JsonRpcRequest) -> Option<String> {
    let market_mode = market_source_mode_for_request(req);
    if market_mode == BUILTIN_MARKET_MODE {
        return None;
    }
    string_param(req, "sourceUrl")
        .or_else(|| string_param(req, "source_url"))
        .or_else(current_market_source_url)
}

/// 函数 `current_market_source_url`
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
pub(crate) fn current_market_source_url() -> Option<String> {
    crate::app_settings::get_persisted_app_setting(
        crate::app_settings::APP_SETTING_PLUGIN_MARKET_SOURCE_URL_KEY,
    )
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

/// 函数 `current_market_source_mode`
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
pub(crate) fn current_market_source_mode() -> String {
    if let Some(value) = crate::app_settings::get_persisted_app_setting(
        crate::app_settings::APP_SETTING_PLUGIN_MARKET_MODE_KEY,
    ) {
        return match value.trim().to_ascii_lowercase().as_str() {
            "private" => CUSTOM_MARKET_MODE.to_string(),
            CUSTOM_MARKET_MODE => CUSTOM_MARKET_MODE.to_string(),
            _ => BUILTIN_MARKET_MODE.to_string(),
        };
    }
    if current_market_source_url().is_some() {
        return CUSTOM_MARKET_MODE.to_string();
    }
    BUILTIN_MARKET_MODE.to_string()
}

/// 函数 `fetch_catalog_entries`
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
pub(crate) async fn fetch_catalog_entries(
    source_url: Option<&str>,
) -> Result<Vec<PluginCatalogEntry>, String> {
    if let Some(source_url) = source_url {
        let normalized = source_url.trim();
        if !normalized.is_empty() {
            match super::runtime::fetch_text(normalized).await {
                Ok(text) => {
                    let value: Value = serde_json::from_str(&text)
                        .map_err(|err| format!("parse catalog response failed: {err}"))?;
                    let items = if let Some(items) =
                        value.get("items").and_then(|value| value.as_array())
                    {
                        items.clone()
                    } else if let Some(items) = value.as_array() {
                        items.clone()
                    } else {
                        Vec::new()
                    };
                    let normalized_items = items
                        .into_iter()
                        .filter_map(|item| parse_catalog_entry_value(&item, Some(normalized)).ok())
                        .collect::<Vec<_>>();
                    return Ok(normalized_items);
                }
                Err(err) => {
                    return Err(format!("fetch plugin catalog failed: {err}"));
                }
            }
        }
    }
    Ok(builtin_catalog_entries())
}

/// 函数 `builtin_catalog_entries`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 返回函数执行结果
fn builtin_catalog_entries() -> Vec<PluginCatalogEntry> {
    vec![
        build_builtin_cleanup_plugin(
            "cleanup-banned-accounts",
            "清理封禁账号",
            "一键清理所有状态为 banned 的账号，适合做批量收尾整理。",
            "开始清理封禁账号：",
            "cleanup_banned_accounts()",
            "清理完成，删除 ",
            "所有封禁账号",
            BUILTIN_CLEANUP_TASK_INTERVAL_SECS,
        ),
        build_builtin_cleanup_plugin(
            "cleanup-unavailable-free-accounts",
            "清理不可用免费账号",
            "自动清理状态不可用且属于 free 的账号，适合做定时收尾整理。",
            "开始清理不可用免费账号：",
            "cleanup_unavailable_free_accounts()",
            "清理完成，删除 ",
            "不可用免费账号",
            BUILTIN_UNAVAILABLE_FREE_CLEANUP_TASK_INTERVAL_SECS,
        ),
    ]
}

/// 函数 `build_builtin_cleanup_plugin`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - id: 参数 id
/// - name: 参数 name
/// - description: 参数 description
/// - start_message: 参数 start_message
/// - cleanup_call: 参数 cleanup_call
/// - finish_prefix: 参数 finish_prefix
/// - task_subject: 参数 task_subject
/// - interval_seconds: 参数 interval_seconds
///
/// # 返回
/// 返回函数执行结果
fn build_builtin_cleanup_plugin(
    id: &str,
    name: &str,
    description: &str,
    start_message: &str,
    cleanup_call: &str,
    finish_prefix: &str,
    task_subject: &str,
    interval_seconds: i64,
) -> PluginCatalogEntry {
    PluginCatalogEntry {
        id: id.to_string(),
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: Some(description.to_string()),
        author: Some("CodexManager".to_string()),
        homepage_url: None,
        script_url: None,
        script_body: Some(format!(
            r#"
/// 函数 `run`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - context: 参数 context
///
/// # 返回
/// 无
fn run(context) {{
    log("{}" + context["plugin"]["name"]);
    let result = {};
    log("{}" + result["deleted"].to_string() + " 个账号");
    result
}}
"#,
            start_message, cleanup_call, finish_prefix
        )),
        permissions: vec!["accounts:cleanup".to_string()],
        manifest_version: "1".to_string(),
        category: Some("official".to_string()),
        runtime_kind: "rhai".to_string(),
        tags: vec![
            "账号治理".to_string(),
            "精选".to_string(),
            task_subject.to_string(),
        ],
        tasks: vec![PluginCatalogTask {
            id: "run".to_string(),
            name: "定时自动清理".to_string(),
            description: Some(format!(
                "每 {} 秒自动清理一次{}",
                interval_seconds, task_subject
            )),
            entrypoint: "run".to_string(),
            schedule_kind: "interval".to_string(),
            interval_seconds: Some(interval_seconds),
            enabled: true,
        }],
        source_url: Some(BUILTIN_MARKET_SOURCE_URL.to_string()),
    }
}

/// 函数 `sync_builtin_cleanup_task_schedule`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - crate: 参数 crate
///
/// # 返回
/// 无
pub(crate) fn sync_builtin_cleanup_task_schedule() {
    let Some(storage) = open_storage() else {
        return;
    };
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let Some(install) = storage
        .find_plugin_install("cleanup-banned-accounts")
        .ok()
        .flatten()
    else {
        return;
    };
    if install.source_url.as_deref() != Some(BUILTIN_MARKET_SOURCE_URL) {
        return;
    }
    let Some(task) = storage
        .find_plugin_task("cleanup-banned-accounts::run")
        .ok()
        .flatten()
    else {
        return;
    };
    if task.schedule_kind != "manual" {
        return;
    }
    let Some(entry) = builtin_catalog_entries().into_iter().next() else {
        return;
    };
    let Some(task_entry) = entry.tasks.into_iter().next() else {
        return;
    };
    let next_run_at = if install.status == "enabled" && task.enabled {
        task_entry
            .interval_seconds
            .filter(|value| *value > 0)
            .map(|value| now_ts() + value)
    } else {
        None
    };
    let Ok(task_json) = serde_json::to_string(&task_entry) else {
        return;
    };
    let _ = storage.update_plugin_task_definition(
        &task.id,
        &task_entry.name,
        task_entry.description.as_deref(),
        &task_entry.entrypoint,
        &task_entry.schedule_kind,
        task_entry.interval_seconds,
        task.enabled,
        next_run_at,
        &task_json,
    );
}

/// 函数 `parse_catalog_entry_value`
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
pub(crate) fn parse_catalog_entry_value(
    value: &Value,
    source_url: Option<&str>,
) -> Result<PluginCatalogEntry, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "plugin entry must be an object".to_string())?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing plugin id".to_string())?;
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(id)
        .to_string();
    let version = obj
        .get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("0.0.0")
        .to_string();
    let description = obj
        .get("description")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let author = obj
        .get("author")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let homepage_url = obj
        .get("homepageUrl")
        .or_else(|| obj.get("homepage_url"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let script_url = obj
        .get("scriptUrl")
        .or_else(|| obj.get("script_url"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let script_body = obj
        .get("scriptBody")
        .or_else(|| obj.get("script_body"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let permissions = obj
        .get("permissions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|text| text.trim().to_string()))
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tasks = obj
        .get("tasks")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(parse_catalog_task_value)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let manifest_version = obj
        .get("manifestVersion")
        .or_else(|| obj.get("manifest_version"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("1")
        .to_string();
    let category = obj
        .get("category")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let runtime_kind = obj
        .get("runtimeKind")
        .or_else(|| obj.get("runtime_kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("rhai")
        .to_string();
    let tags = obj
        .get("tags")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|text| text.trim().to_string()))
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(PluginCatalogEntry {
        id: id.to_string(),
        name,
        version,
        description,
        author,
        homepage_url,
        script_url,
        script_body,
        permissions,
        tasks,
        manifest_version,
        category,
        runtime_kind,
        tags,
        source_url: source_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    })
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;

/// 函数 `parse_catalog_task_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - value: 参数 value
///
/// # 返回
/// 返回函数执行结果
fn parse_catalog_task_value(value: &Value) -> Result<PluginCatalogTask, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "task entry must be an object".to_string())?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "missing task id".to_string())?;
    Ok(PluginCatalogTask {
        id: id.to_string(),
        name: obj
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(id)
            .to_string(),
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        entrypoint: obj
            .get("entrypoint")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("run")
            .to_string(),
        schedule_kind: obj
            .get("scheduleKind")
            .or_else(|| obj.get("schedule_kind"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("manual")
            .to_string(),
        interval_seconds: obj
            .get("intervalSeconds")
            .or_else(|| obj.get("interval_seconds"))
            .and_then(Value::as_i64),
        enabled: obj.get("enabled").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// 函数 `build_plugin_tasks`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - entry: 参数 entry
/// - now: 参数 now
///
/// # 返回
/// 返回函数执行结果
fn build_plugin_tasks(entry: &PluginCatalogEntry, now: i64) -> Result<Vec<PluginTask>, String> {
    entry
        .tasks
        .iter()
        .map(|task| {
            let task_json = serde_json::to_string(task)
                .map_err(|err| format!("serialize task manifest failed: {err}"))?;
            let next_run_at = if task.schedule_kind == "manual" {
                None
            } else {
                task.interval_seconds.and_then(|interval| {
                    if interval > 0 {
                        Some(now + interval)
                    } else {
                        None
                    }
                })
            };
            Ok(PluginTask {
                id: format!("{}::{}", entry.id, task.id),
                plugin_id: entry.id.clone(),
                name: task.name.clone(),
                description: task.description.clone(),
                entrypoint: task.entrypoint.clone(),
                schedule_kind: task.schedule_kind.clone(),
                interval_seconds: task.interval_seconds,
                enabled: task.enabled,
                next_run_at,
                last_run_at: None,
                last_status: None,
                last_error: None,
                task_json,
                created_at: now,
                updated_at: now,
            })
        })
        .collect()
}

/// 函数 `install_or_update_plugin`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - req: 参数 req
/// - is_update: 参数 is_update
///
/// # 返回
/// 返回函数执行结果
fn install_or_update_plugin(req: &JsonRpcRequest, is_update: bool) -> Result<Value, String> {
    super::runtime::run_network_future(install_or_update_plugin_async(req, is_update))
}

async fn install_or_update_plugin_async(
    req: &JsonRpcRequest,
    is_update: bool,
) -> Result<Value, String> {
    let entry_value = req
        .params
        .as_ref()
        .and_then(|value| value.get("entry"))
        .cloned();
    let request = req.clone();
    let source_url =
        super::runtime::run_storage(move || Ok(source_url_from_request(&request))).await?;
    let entry = if let Some(value) = entry_value {
        parse_catalog_entry_value(&value, source_url.as_deref())?
    } else {
        let plugin_id = string_param(req, "pluginId")
            .or_else(|| string_param(req, "plugin_id"))
            .ok_or_else(|| "missing pluginId".to_string())?;
        let items = fetch_catalog_entries(source_url.as_deref()).await?;
        items
            .into_iter()
            .find(|item| item.id == plugin_id)
            .ok_or_else(|| format!("plugin not found: {plugin_id}"))?
    };

    let script_body = entry
        .script_body
        .clone()
        .or_else(|| entry.script_url.as_ref().map(|_| String::new()))
        .unwrap_or_default();
    let script_body = if script_body.trim().is_empty() {
        if let Some(script_url) = entry.script_url.as_deref() {
            super::runtime::fetch_text(script_url).await?
        } else {
            return Err(format!("plugin script missing: {}", entry.id));
        }
    } else {
        script_body
    };

    super::runtime::run_storage(move || {
        persist_plugin_install(entry, source_url, script_body, is_update)
    })
    .await
}

fn persist_plugin_install(
    entry: PluginCatalogEntry,
    source_url: Option<String>,
    script_body: String,
    is_update: bool,
) -> Result<Value, String> {
    let existing_install = if is_update {
        let Some(storage) = open_storage() else {
            return Err("storage unavailable".to_string());
        };
        let storage = crate::account::remote_storage::AccountStorage::new(&storage);
        let Some(existing) = storage
            .find_plugin_install(&entry.id)
            .map_err(|err| err.to_string())?
        else {
            return Err(format!("plugin not installed: {}", entry.id));
        };
        Some(existing)
    } else {
        None
    };

    let permissions_json = serde_json::to_string(&entry.permissions)
        .map_err(|err| format!("serialize permissions failed: {err}"))?;
    let manifest_json = serde_json::to_string(&entry)
        .map_err(|err| format!("serialize plugin manifest failed: {err}"))?;
    let installed_at = now_ts();
    let tasks = build_plugin_tasks(&entry, installed_at)?;
    let plugin = PluginInstall {
        plugin_id: entry.id.clone(),
        source_url: entry.source_url.clone().or(source_url),
        name: entry.name.clone(),
        version: entry.version.clone(),
        description: entry.description.clone(),
        author: entry.author.clone(),
        homepage_url: entry.homepage_url.clone(),
        script_url: entry.script_url.clone(),
        script_body,
        permissions_json,
        manifest_json,
        status: existing_install
            .as_ref()
            .map(|plugin| plugin.status.clone())
            .unwrap_or_else(|| "disabled".to_string()),
        installed_at: existing_install
            .as_ref()
            .map(|plugin| plugin.installed_at)
            .unwrap_or(installed_at),
        updated_at: installed_at,
        last_run_at: existing_install
            .as_ref()
            .and_then(|plugin| plugin.last_run_at),
        last_error: existing_install
            .as_ref()
            .and_then(|plugin| plugin.last_error.clone()),
    };

    let Some(storage) = open_storage() else {
        return Err("storage unavailable".to_string());
    };
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    if storage.replace_plugin_install(&plugin, &tasks).is_err() {
        return Err(if is_update {
            "update plugin failed".to_string()
        } else {
            "install plugin failed".to_string()
        });
    }

    let install_summary = to_installed_plugin_summary(&plugin, &tasks);
    Ok(serde_json::json!({
        "plugin": install_summary,
        "tasks": tasks_to_summaries(&plugin, &tasks),
    }))
}

/// 函数 `to_installed_plugin_summary`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - plugin: 参数 plugin
/// - tasks: 参数 tasks
///
/// # 返回
/// 返回函数执行结果
fn to_installed_plugin_summary(
    plugin: &PluginInstall,
    tasks: &[PluginTask],
) -> InstalledPluginSummary {
    let manifest_entry = serde_json::from_str::<Value>(&plugin.manifest_json)
        .ok()
        .and_then(|value| parse_catalog_entry_value(&value, plugin.source_url.as_deref()).ok());
    let manifest_version = manifest_entry
        .as_ref()
        .map(|entry| entry.manifest_version.clone())
        .unwrap_or_else(|| "1".to_string());
    let category = manifest_entry
        .as_ref()
        .and_then(|entry| entry.category.clone());
    let runtime_kind = manifest_entry
        .as_ref()
        .map(|entry| entry.runtime_kind.clone())
        .unwrap_or_else(|| "rhai".to_string());
    let tags = manifest_entry
        .as_ref()
        .map(|entry| entry.tags.clone())
        .unwrap_or_default();
    let task_count = tasks.len() as i64;
    let enabled_task_count = tasks.iter().filter(|task| task.enabled).count() as i64;
    InstalledPluginSummary {
        plugin_id: plugin.plugin_id.clone(),
        source_url: plugin.source_url.clone(),
        name: plugin.name.clone(),
        version: plugin.version.clone(),
        description: plugin.description.clone(),
        author: plugin.author.clone(),
        homepage_url: plugin.homepage_url.clone(),
        script_url: plugin.script_url.clone(),
        permissions: serde_json::from_str::<Vec<String>>(&plugin.permissions_json)
            .unwrap_or_default(),
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

/// 函数 `tasks_to_summaries`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - plugin: 参数 plugin
/// - tasks: 参数 tasks
///
/// # 返回
/// 返回函数执行结果
fn tasks_to_summaries(plugin: &PluginInstall, tasks: &[PluginTask]) -> Vec<PluginTaskSummary> {
    tasks
        .iter()
        .map(|task| PluginTaskSummary {
            id: task.id.clone(),
            plugin_id: plugin.plugin_id.clone(),
            plugin_name: plugin.name.clone(),
            name: task.name.clone(),
            description: task.description.clone(),
            entrypoint: task.entrypoint.clone(),
            schedule_kind: task.schedule_kind.clone(),
            interval_seconds: task.interval_seconds,
            enabled: task.enabled,
            next_run_at: task.next_run_at,
            last_run_at: task.last_run_at,
            last_status: task.last_status.clone(),
            last_error: task.last_error.clone(),
        })
        .collect()
}
