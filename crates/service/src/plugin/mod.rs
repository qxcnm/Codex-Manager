use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;

mod catalog;
pub(crate) mod injected;
mod runtime;
mod scheduler;
pub(crate) mod store;

static PLUGIN_SCHEDULER: Mutex<Option<tokio::task::JoinHandle<()>>> = Mutex::new(None);

/// 函数 `ensure_plugin_scheduler`
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
pub(crate) fn ensure_plugin_scheduler() {
    let mut task = crate::lock_utils::lock_recover(&PLUGIN_SCHEDULER, "plugin_scheduler");
    if crate::shutdown_requested() || task.as_ref().is_some_and(|task| !task.is_finished()) {
        return;
    }
    match crate::runtime::service_runtime::process_runtime() {
        Ok(runtime) => *task = Some(runtime.spawn(plugin_scheduler_loop())),
        Err(error) => log::error!("plugin scheduler unavailable: {error}"),
    }
}

/// 函数 `try_handle`
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
pub(crate) fn try_handle(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let result = match req.method.as_str() {
        "plugin/catalog/list" | "plugin/catalog/refresh" => Some(catalog::handle_catalog_list(req)),
        "plugin/install" => Some(catalog::handle_install(req)),
        "plugin/update" => Some(catalog::handle_update(req)),
        "plugin/uninstall" => Some(catalog::handle_uninstall(req)),
        "plugin/list" => Some(store::handle_list_installed(req)),
        "plugin/enable" => Some(store::handle_enable(req, true)),
        "plugin/disable" => Some(store::handle_enable(req, false)),
        "plugin/tasks/update" => Some(store::handle_task_update(req)),
        "plugin/tasks/list" => Some(store::handle_task_list(req)),
        "plugin/tasks/run" => Some(runtime::handle_task_run(req)),
        "plugin/logs/list" => Some(store::handle_log_list(req)),
        _ => None,
    }?;
    Some(result)
}

/// Network-bearing plugin RPCs are intercepted by the native async dispatcher.
/// Rhai remains a synchronous ABI and runs only on its bounded script workers.
pub(crate) async fn try_handle_async(req: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    match req.method.as_str() {
        "plugin/catalog/list" | "plugin/catalog/refresh" => {
            Some(catalog::handle_catalog_list_async(req).await)
        }
        "plugin/install" => Some(catalog::handle_install_async(req, false).await),
        "plugin/update" => Some(catalog::handle_install_async(req, true).await),
        "plugin/tasks/run" => Some(runtime::handle_task_run_async(req).await),
        _ => None,
    }
}

/// 函数 `json_response`
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
pub(crate) fn json_response(req: &JsonRpcRequest, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        id: req.id.clone(),
        result,
    }
}

/// 函数 `plugin_scheduler_loop`
///
/// 作者: gaohongshun
///
/// 时间: 2026-04-02
///
/// # 参数
/// 无
///
/// # 返回
/// 无
async fn plugin_scheduler_loop() {
    if crate::runtime::blocking::run(
        "plugin-schedule-init",
        catalog::sync_builtin_cleanup_task_schedule,
    )
    .await
    .is_err()
    {
        return;
    }
    while !crate::shutdown_requested() {
        let sleep_secs =
            match crate::runtime::blocking::run("plugin-schedule", scheduler::run_due_tasks_once)
                .await
            {
                Ok(seconds) => seconds,
                Err(_) if crate::shutdown_requested() => return,
                Err(error) => {
                    log::warn!("plugin scheduler failed: {error}");
                    1
                }
            };
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_secs(sleep_secs) {
            if crate::shutdown_requested() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

pub(crate) async fn drain_scheduler() {
    let task = crate::lock_utils::lock_recover(&PLUGIN_SCHEDULER, "plugin_scheduler").take();
    if let Some(task) = task {
        let _ = task.await;
    }
}
