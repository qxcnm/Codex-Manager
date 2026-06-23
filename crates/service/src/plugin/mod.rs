use codexmanager_core::rpc::types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

mod catalog;
mod runtime;
mod scheduler;
mod store;

static PLUGIN_SCHEDULER_STARTED: OnceLock<()> = OnceLock::new();

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
    PLUGIN_SCHEDULER_STARTED.get_or_init(|| {
        catalog::sync_builtin_cleanup_task_schedule();
        let _ = thread::Builder::new()
            .name("plugin-scheduler".to_string())
            .spawn(plugin_scheduler_loop);
    });
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
fn plugin_scheduler_loop() {
    loop {
        if crate::shutdown_requested() {
            break;
        }
        let sleep_secs = scheduler::run_due_tasks_once();
        let start = std::time::Instant::now();
        let delay = Duration::from_secs(sleep_secs);
        while start.elapsed() < delay {
            if crate::shutdown_requested() {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}
