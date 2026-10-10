use crate::storage_helpers::open_storage;

use super::runtime::run_loaded_plugin_task;
use super::store::rearm_enabled_interval_tasks_for_plugin;

const DEFAULT_PLUGIN_SCHEDULER_INTERVAL_SECS: u64 = 5;

/// 函数 `run_due_tasks_once`
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
pub(crate) fn run_due_tasks_once() -> u64 {
    let Some(storage) = open_storage() else {
        return DEFAULT_PLUGIN_SCHEDULER_INTERVAL_SECS;
    };
    let storage = crate::account::remote_storage::AccountStorage::new(&storage);
    let now = codexmanager_core::storage::now_ts();
    if rearm_enabled_interval_tasks_for_plugin(&storage, None, now).is_err() {
        log::warn!("repair plugin task schedules failed");
    }
    let tasks = match storage.list_due_plugin_tasks(now, 100) {
        Ok(items) => items,
        Err(err) => {
            log::warn!("list due plugin tasks failed: {err}");
            return DEFAULT_PLUGIN_SCHEDULER_INTERVAL_SECS;
        }
    };
    for task in tasks {
        let _ = run_loaded_plugin_task(&storage, task, None);
    }

    let next_run_at = match storage.next_enabled_plugin_task_run_at() {
        Ok(value) => value,
        Err(err) => {
            log::warn!("read next plugin task schedule failed: {err}");
            return DEFAULT_PLUGIN_SCHEDULER_INTERVAL_SECS;
        }
    };
    next_run_at
        .map(|value| {
            if value <= now {
                1
            } else {
                (value - now) as u64
            }
        })
        .unwrap_or(DEFAULT_PLUGIN_SCHEDULER_INTERVAL_SECS)
        .clamp(1, DEFAULT_PLUGIN_SCHEDULER_INTERVAL_SECS)
}
