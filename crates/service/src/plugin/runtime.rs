use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
use codexmanager_core::storage::{
    now_ts, PluginRunLog, PluginRuntimeInstall, PluginTask, PluginTaskExecutionRow,
};
use rhai::{Array, Dynamic, Engine, Map, Scope};
use serde_json::{json, Value};
use std::collections::HashSet;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::account_cleanup::{delete_banned_accounts, delete_unavailable_free_accounts};
use crate::storage_helpers::open_storage;

static PLUGIN_HTTP_CLIENT: OnceLock<Mutex<Option<reqwest::Client>>> = OnceLock::new();
static PLUGIN_SCRIPT_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);
static PLUGIN_TASK_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);
static PLUGIN_STORAGE_WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(16);
#[cfg(test)]
static PLUGIN_HTTP_CLIENT_BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);

fn build_plugin_http_client() -> Result<reqwest::Client, String> {
    #[cfg(test)]
    PLUGIN_HTTP_CLIENT_BUILD_COUNT.fetch_add(1, Ordering::SeqCst);

    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|err| format!("build http client failed: {err}"))
}

fn plugin_http_client() -> Result<reqwest::Client, String> {
    let lock = PLUGIN_HTTP_CLIENT.get_or_init(|| Mutex::new(None));
    let mut cached = crate::lock_utils::lock_recover(lock, "plugin_http_client");
    if let Some(client) = cached.as_ref() {
        return Ok(client.clone());
    }
    let client = build_plugin_http_client()?;
    *cached = Some(client.clone());
    Ok(client)
}

/// The Rhai host ABI and legacy synchronous RPC callers execute on controlled
/// workers. Their network futures share one runtime instead of a blocking HTTP
/// client or a runtime/thread created for every request.
pub(super) fn run_network_future<T: Send>(
    future: impl std::future::Future<Output = Result<T, String>> + Send,
) -> Result<T, String> {
    crate::runtime::service_runtime::run_sync(future)?
}

/// Only short synchronous storage phases cross this boundary. Network waits
/// in catalog/install run directly on the caller's async task.
pub(super) async fn run_storage<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let permit = PLUGIN_STORAGE_WORKERS
        .acquire()
        .await
        .map_err(|_| "plugin storage unavailable".to_string())?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|error| format!("plugin storage worker failed: {error}"))?
}

pub(super) async fn handle_task_run_async(req: &JsonRpcRequest) -> JsonRpcResponse {
    let Ok(permit) = PLUGIN_TASK_WORKERS.try_acquire() else {
        return super::json_response(
            req,
            crate::error_codes::rpc_error_payload("plugin script execution busy".to_string()),
        );
    };
    let request = req.clone();
    let result = crate::runtime::blocking::run("plugin-script", move || {
        let _permit = permit;
        handle_task_run(&request)
    })
    .await;
    result.unwrap_or_else(|error| {
        super::json_response(
            req,
            crate::error_codes::rpc_error_payload(format!("plugin script worker failed: {error}")),
        )
    })
}

#[cfg(test)]
fn plugin_http_client_build_count_for_test() -> usize {
    PLUGIN_HTTP_CLIENT_BUILD_COUNT.load(Ordering::SeqCst)
}

fn task_execution_row_from_task(task: PluginTask) -> PluginTaskExecutionRow {
    PluginTaskExecutionRow {
        id: task.id,
        plugin_id: task.plugin_id,
        name: task.name,
        description: task.description,
        entrypoint: task.entrypoint,
        schedule_kind: task.schedule_kind,
        interval_seconds: task.interval_seconds,
        enabled: task.enabled,
    }
}

/// 函数 `handle_task_run`
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
pub(crate) fn handle_task_run(req: &JsonRpcRequest) -> JsonRpcResponse {
    let Some(task_id) = req
        .params
        .as_ref()
        .and_then(|value| value.get("taskId").or_else(|| value.get("task_id")))
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return super::json_response(
            req,
            crate::error_codes::rpc_error_payload("missing taskId".to_string()),
        );
    };

    match run_plugin_task(
        &task_id,
        req.params
            .as_ref()
            .and_then(|value| value.get("input"))
            .cloned(),
    ) {
        Ok(value) => super::json_response(req, value),
        Err(err) => super::json_response(req, crate::error_codes::rpc_error_payload(err)),
    }
}

/// 函数 `run_plugin_task`
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
pub(crate) fn run_plugin_task(task_id: &str, input: Option<Value>) -> Result<Value, String> {
    let storage = open_storage().ok_or_else(|| "storage unavailable".to_string())?;
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let Some(task) = storage
        .find_plugin_task(task_id)
        .map_err(|err| err.to_string())?
    else {
        return Err(format!("task not found: {task_id}"));
    };
    run_loaded_plugin_task(&storage, task_execution_row_from_task(task), input)
}

pub(super) fn run_loaded_plugin_task(
    storage: &codexmanager_core::storage::Storage,
    task: PluginTaskExecutionRow,
    input: Option<Value>,
) -> Result<Value, String> {
    let storage = crate::account::remote_storage::AccountStorage::new(storage);
    let Some(plugin) = storage
        .find_plugin_runtime_install(&task.plugin_id)
        .map_err(|err| err.to_string())?
    else {
        return Err(format!("plugin not found: {}", task.plugin_id));
    };
    if plugin.status != "enabled" && task.schedule_kind != "manual" {
        return Err(format!("plugin disabled: {}", plugin.plugin_id));
    }

    let permissions = parse_permissions(&plugin.permissions_json);
    let run_started_at = now_ts();
    let result = execute_plugin_script(&plugin, &task, input.clone(), &permissions, run_started_at);
    let (status, output_json, error_message) = match result {
        Ok(value) => ("ok", Some(value.clone()), None),
        Err(err) => ("error", None, Some(err)),
    };
    let run_finished_at = now_ts();
    let duration_ms = ((run_finished_at - run_started_at).max(0)) * 1000;
    let _ = storage.insert_plugin_run_log(&PluginRunLog {
        id: None,
        plugin_id: plugin.plugin_id.clone(),
        task_id: Some(task.id.clone()),
        run_type: if task.schedule_kind == "manual" {
            "manual".to_string()
        } else {
            "scheduled".to_string()
        },
        status: status.to_string(),
        started_at: run_started_at,
        finished_at: Some(run_finished_at),
        duration_ms: Some(duration_ms),
        output_json: output_json.clone().map(|value| value.to_string()),
        error: error_message.clone(),
    });
    let next_run_at = next_run_time_for_task(&task, run_finished_at);
    let _ = storage.update_plugin_task_schedule(
        &task.id,
        next_run_at,
        Some(run_finished_at),
        Some(status),
        error_message.as_deref(),
    );
    let _ = storage.update_plugin_install_last_run(
        &plugin.plugin_id,
        run_finished_at,
        error_message.as_deref(),
    );
    match error_message {
        Some(err) => Err(err),
        None => Ok(json!({
            "ok": true,
            "pluginId": plugin.plugin_id.clone(),
            "taskId": task.id.clone(),
            "output": output_json,
        })),
    }
}

/// 函数 `fetch_text`
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
pub(crate) async fn fetch_text(url: &str) -> Result<String, String> {
    let client = plugin_http_client()?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("fetch {url} failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "fetch {url} failed with status {}",
            response.status()
        ));
    }
    response
        .text()
        .await
        .map_err(|err| format!("read {url} response failed: {err}"))
}

/// 函数 `execute_plugin_script`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - plugin: 参数 plugin
/// - task: 参数 task
/// - input: 参数 input
/// - permissions: 参数 permissions
/// - run_started_at: 参数 run_started_at
///
/// # 返回
/// 返回函数执行结果
fn execute_plugin_script(
    plugin: &PluginRuntimeInstall,
    task: &PluginTaskExecutionRow,
    input: Option<Value>,
    permissions: &HashSet<String>,
    run_started_at: i64,
) -> Result<Value, String> {
    let _permit = PLUGIN_SCRIPT_WORKERS
        .try_acquire()
        .map_err(|_| "plugin script execution busy".to_string())?;
    let mut engine = Engine::new();
    engine.set_max_operations(50_000);
    engine.on_print(|text| {
        log::info!("plugin print: {}", text);
    });

    let plugin_name = plugin.name.clone();
    let plugin_id = plugin.plugin_id.clone();
    let log_plugin_id = plugin_id.clone();
    engine.register_fn("log", move |message: String| {
        log::info!("plugin log [{}]: {}", log_plugin_id, message);
    });

    if permissions.contains("settings:read") {
        let settings_map = crate::app_settings::list_app_settings_map();
        let get_setting_map = settings_map.clone();
        engine.register_fn("get_setting", move |key: String| -> Dynamic {
            get_setting_map
                .get(key.trim())
                .map(|value| Dynamic::from(value.clone()))
                .unwrap_or(Dynamic::UNIT)
        });
        engine.register_fn("list_settings", move || -> Dynamic {
            dynamic_from_json(json!(settings_map))
        });
    }

    if permissions.contains("network") {
        engine.register_fn("http_get", move |url: String| -> Dynamic {
            match fetch_http_value("GET", &url, None) {
                Ok(value) => dynamic_from_json(value),
                Err(err) => dynamic_from_json(json!({ "ok": false, "error": err })),
            }
        });
        engine.register_fn("http_post", move |url: String, body: String| -> Dynamic {
            match fetch_http_value("POST", &url, Some(body)) {
                Ok(value) => dynamic_from_json(value),
                Err(err) => dynamic_from_json(json!({ "ok": false, "error": err })),
            }
        });
    }

    if permissions.contains("accounts:cleanup") {
        engine.register_fn("cleanup_banned_accounts", move || -> Dynamic {
            match delete_banned_accounts() {
                Ok(value) => dynamic_from_json(json!(value)),
                Err(err) => dynamic_from_json(json!({ "ok": false, "error": err })),
            }
        });
        engine.register_fn("cleanup_unavailable_free_accounts", move || -> Dynamic {
            match delete_unavailable_free_accounts() {
                Ok(value) => dynamic_from_json(json!(value)),
                Err(err) => dynamic_from_json(json!({ "ok": false, "error": err })),
            }
        });
    }

    let ast = engine
        .compile(&plugin.script_body)
        .map_err(|err| format!("compile plugin script failed: {err}"))?;
    let mut scope = Scope::new();
    let context = json!({
        "plugin": {
            "id": plugin_id,
            "name": plugin_name,
            "version": plugin.version.clone(),
            "sourceUrl": plugin.source_url.clone(),
            "permissions": permissions.iter().cloned().collect::<Vec<_>>(),
        },
        "task": {
            "id": task.id.clone(),
            "name": task.name.clone(),
            "description": task.description.clone(),
            "entrypoint": task.entrypoint.clone(),
            "scheduleKind": task.schedule_kind.clone(),
            "intervalSeconds": task.interval_seconds,
            "enabled": task.enabled,
        },
        "input": input,
        "runStartedAt": run_started_at,
    });
    let result = engine
        .call_fn::<Dynamic>(
            &mut scope,
            &ast,
            &task.entrypoint,
            (dynamic_from_json(context),),
        )
        .map_err(|err| format!("plugin task failed: {err}"))?;
    Ok(json_from_dynamic(result))
}

/// 函数 `fetch_http_value`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - method: 参数 method
/// - url: 参数 url
/// - body: 参数 body
///
/// # 返回
/// 返回函数执行结果
fn fetch_http_value(method: &str, url: &str, body: Option<String>) -> Result<Value, String> {
    run_network_future(fetch_http_value_async(method, url, body))
}

async fn fetch_http_value_async(
    method: &str,
    url: &str,
    body: Option<String>,
) -> Result<Value, String> {
    let client = plugin_http_client()?;
    let request = match method {
        "POST" => client.post(url),
        _ => client.get(url),
    };
    let request = if let Some(body) = body {
        request.body(body)
    } else {
        request
    };
    let response = request
        .send()
        .await
        .map_err(|err| format!("http {method} {url} failed: {err}"))?;
    let status = response.status().as_u16();
    let body_text = response
        .text()
        .await
        .map_err(|err| format!("read {url} response failed: {err}"))?;
    Ok(json!({
        "ok": true,
        "status": status,
        "body": body_text,
    }))
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
fn parse_permissions(raw: &str) -> HashSet<String> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 函数 `next_run_time_for_task`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// - task: 参数 task
/// - finished_at: 参数 finished_at
///
/// # 返回
/// 返回函数执行结果
fn next_run_time_for_task(task: &PluginTaskExecutionRow, finished_at: i64) -> Option<i64> {
    if task.schedule_kind == "manual" {
        return None;
    }
    task.interval_seconds
        .filter(|value| *value > 0)
        .map(|interval| finished_at + interval)
}

/// 函数 `dynamic_from_json`
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
fn dynamic_from_json(value: Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(value) => Dynamic::from(value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Dynamic::from(value)
            } else if let Some(value) = number.as_u64() {
                Dynamic::from(value as i64)
            } else if let Some(value) = number.as_f64() {
                Dynamic::from(value)
            } else {
                Dynamic::UNIT
            }
        }
        Value::String(value) => Dynamic::from(value),
        Value::Array(items) => {
            let mut array = Array::new();
            for item in items {
                array.push(dynamic_from_json(item));
            }
            Dynamic::from(array)
        }
        Value::Object(items) => {
            let mut map = Map::new();
            for (key, value) in items {
                map.insert(key.into(), dynamic_from_json(value));
            }
            Dynamic::from(map)
        }
    }
}

/// 函数 `json_from_dynamic`
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
fn json_from_dynamic(value: Dynamic) -> Value {
    if value.is_unit() {
        return Value::Null;
    }
    if let Some(value) = value.clone().try_cast::<bool>() {
        return Value::Bool(value);
    }
    if let Some(value) = value.clone().try_cast::<i64>() {
        return Value::Number(value.into());
    }
    if let Some(value) = value.clone().try_cast::<f64>() {
        return serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if let Some(value) = value.clone().try_cast::<String>() {
        return Value::String(value);
    }
    if let Some(array) = value.clone().try_cast::<Array>() {
        return Value::Array(array.into_iter().map(json_from_dynamic).collect());
    }
    if let Some(map) = value.try_cast::<Map>() {
        let mut out = serde_json::Map::new();
        for (key, value) in map {
            out.insert(key.into(), json_from_dynamic(value));
        }
        return Value::Object(out);
    }
    Value::Null
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
