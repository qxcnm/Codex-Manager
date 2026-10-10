use super::remote_storage::AccountStorage;
use crate::storage_helpers::{seaorm_block_on, seaorm_enabled};
use codexmanager_core::storage::*;
use codexmanager_storage_seaorm::PluginsRepository;
use std::{collections::HashMap, ops::Deref};
fn error(message: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure((), Some(message))
}
// Keep the full dual-backend adapter surface available for SeaORM runtime selection.
#[allow(dead_code)]
impl AccountStorage<'_> {
    pub(crate) fn upsert_plugin_install(&self, plugin: &PluginInstall) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().upsert_plugin_install(plugin);
        }
        let plugin = plugin.clone();
        seaorm_block_on(move |s| async move {
            PluginsRepository::upsert_plugin_install(s.connection(), &plugin)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn replace_plugin_install(
        &self,
        plugin: &PluginInstall,
        tasks: &[PluginTask],
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().replace_plugin_install(plugin, tasks);
        }
        let plugin = plugin.clone();
        let tasks = tasks.to_vec();
        seaorm_block_on(move |s| async move {
            PluginsRepository::replace_plugin_install(s.connection(), &plugin, &tasks)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_plugin_installs(&self) -> rusqlite::Result<Vec<PluginInstall>> {
        if !seaorm_enabled() {
            return self.deref().list_plugin_installs();
        }
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_plugin_installs(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_plugin_install(
        &self,
        plugin_id: &str,
    ) -> rusqlite::Result<Option<PluginInstall>> {
        if !seaorm_enabled() {
            return self.deref().find_plugin_install(plugin_id);
        }
        let plugin_id = plugin_id.to_owned();
        seaorm_block_on(move |s| async move {
            PluginsRepository::find_plugin_install(s.connection(), &plugin_id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_plugin_runtime_install(
        &self,
        plugin_id: &str,
    ) -> rusqlite::Result<Option<PluginRuntimeInstall>> {
        if !seaorm_enabled() {
            return self.deref().find_plugin_runtime_install(plugin_id);
        }
        let plugin_id = plugin_id.to_owned();
        seaorm_block_on(move |s| async move {
            PluginsRepository::find_plugin_runtime_install(s.connection(), &plugin_id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_plugin_install_summaries(
        &self,
    ) -> rusqlite::Result<Vec<PluginInstallListSummary>> {
        if !seaorm_enabled() {
            return self.deref().list_plugin_install_summaries();
        }
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_plugin_install_summaries(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn plugin_install_names_for_plugins(
        &self,
        plugin_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        if !seaorm_enabled() {
            return self.deref().plugin_install_names_for_plugins(plugin_ids);
        }
        let plugin_ids = plugin_ids.to_vec();
        seaorm_block_on(move |s| async move {
            PluginsRepository::plugin_install_names_for_plugins(s.connection(), &plugin_ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn plugin_task_names_for_tasks(
        &self,
        task_ids: &[String],
    ) -> rusqlite::Result<HashMap<String, String>> {
        if !seaorm_enabled() {
            return self.deref().plugin_task_names_for_tasks(task_ids);
        }
        let task_ids = task_ids.to_vec();
        seaorm_block_on(move |s| async move {
            PluginsRepository::plugin_task_names_for_tasks(s.connection(), &task_ids)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_plugin_tasks(
        &self,
        plugin_id: Option<&str>,
    ) -> rusqlite::Result<Vec<PluginTask>> {
        if !seaorm_enabled() {
            return self.deref().list_plugin_tasks(plugin_id);
        }
        let plugin_id = plugin_id.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_plugin_tasks(s.connection(), plugin_id.as_deref())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn find_plugin_task(&self, task_id: &str) -> rusqlite::Result<Option<PluginTask>> {
        if !seaorm_enabled() {
            return self.deref().find_plugin_task(task_id);
        }
        let task_id = task_id.to_owned();
        seaorm_block_on(move |s| async move {
            PluginsRepository::find_plugin_task(s.connection(), &task_id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_plugin_task_summaries(
        &self,
        plugin_id: Option<&str>,
    ) -> rusqlite::Result<Vec<PluginTaskListSummary>> {
        if !seaorm_enabled() {
            return self.deref().list_plugin_task_summaries(plugin_id);
        }
        let plugin_id = plugin_id.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_plugin_task_summaries(s.connection(), plugin_id.as_deref())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn plugin_task_counts_by_plugin(
        &self,
    ) -> rusqlite::Result<HashMap<String, PluginTaskCount>> {
        if !seaorm_enabled() {
            return self.deref().plugin_task_counts_by_plugin();
        }
        seaorm_block_on(move |s| async move {
            PluginsRepository::plugin_task_counts_by_plugin(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn delete_plugin_install(&self, plugin_id: &str) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().delete_plugin_install(plugin_id);
        }
        let plugin_id = plugin_id.to_owned();
        seaorm_block_on(move |s| async move {
            PluginsRepository::delete_plugin_install(s.connection(), &plugin_id)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_plugin_tasks_needing_schedule_repair(
        &self,
        plugin_id: Option<&str>,
    ) -> rusqlite::Result<Vec<PluginTaskScheduleRepairRow>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_plugin_tasks_needing_schedule_repair(plugin_id);
        }
        let plugin_id = plugin_id.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_plugin_tasks_needing_schedule_repair(
                s.connection(),
                plugin_id.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn repair_plugin_task_schedules(
        &self,
        plugin_id: Option<&str>,
        now: i64,
    ) -> rusqlite::Result<usize> {
        if !seaorm_enabled() {
            return self.deref().repair_plugin_task_schedules(plugin_id, now);
        }
        let plugin_id = plugin_id.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::repair_plugin_task_schedules(
                s.connection(),
                plugin_id.as_deref(),
                now,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn update_plugin_install_status(
        &self,
        plugin_id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .update_plugin_install_status(plugin_id, status, last_error);
        }
        let plugin_id = plugin_id.to_owned();
        let status = status.to_owned();
        let last_error = last_error.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::update_plugin_install_status(
                s.connection(),
                &plugin_id,
                &status,
                last_error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn update_plugin_install_last_run(
        &self,
        plugin_id: &str,
        last_run_at: i64,
        last_error: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self
                .deref()
                .update_plugin_install_last_run(plugin_id, last_run_at, last_error);
        }
        let plugin_id = plugin_id.to_owned();
        let last_error = last_error.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::update_plugin_install_last_run(
                s.connection(),
                &plugin_id,
                last_run_at,
                last_error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn set_plugin_task_enabled(
        &self,
        task_id: &str,
        enabled: bool,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().set_plugin_task_enabled(task_id, enabled);
        }
        let task_id = task_id.to_owned();
        seaorm_block_on(move |s| async move {
            PluginsRepository::set_plugin_task_enabled(s.connection(), &task_id, enabled)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn update_plugin_task_definition(
        &self,
        task_id: &str,
        name: &str,
        description: Option<&str>,
        entrypoint: &str,
        schedule_kind: &str,
        interval_seconds: Option<i64>,
        enabled: bool,
        next_run_at: Option<i64>,
        task_json: &str,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_plugin_task_definition(
                task_id,
                name,
                description,
                entrypoint,
                schedule_kind,
                interval_seconds,
                enabled,
                next_run_at,
                task_json,
            );
        }
        let task_id = task_id.to_owned();
        let name = name.to_owned();
        let description = description.map(ToOwned::to_owned);
        let entrypoint = entrypoint.to_owned();
        let schedule_kind = schedule_kind.to_owned();
        let task_json = task_json.to_owned();
        seaorm_block_on(move |s| async move {
            PluginsRepository::update_plugin_task_definition(
                s.connection(),
                &task_id,
                &name,
                description.as_deref(),
                &entrypoint,
                &schedule_kind,
                interval_seconds,
                enabled,
                next_run_at,
                &task_json,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn update_plugin_task_schedule(
        &self,
        task_id: &str,
        next_run_at: Option<i64>,
        last_run_at: Option<i64>,
        last_status: Option<&str>,
        last_error: Option<&str>,
    ) -> rusqlite::Result<()> {
        if !seaorm_enabled() {
            return self.deref().update_plugin_task_schedule(
                task_id,
                next_run_at,
                last_run_at,
                last_status,
                last_error,
            );
        }
        let task_id = task_id.to_owned();
        let last_status = last_status.map(ToOwned::to_owned);
        let last_error = last_error.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::update_plugin_task_schedule(
                s.connection(),
                &task_id,
                next_run_at,
                last_run_at,
                last_status.as_deref(),
                last_error.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_due_plugin_tasks(
        &self,
        now: i64,
        limit: i64,
    ) -> rusqlite::Result<Vec<PluginTaskExecutionRow>> {
        if !seaorm_enabled() {
            return self.deref().list_due_plugin_tasks(now, limit);
        }
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_due_plugin_tasks(s.connection(), now, limit)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn next_enabled_plugin_task_run_at(&self) -> rusqlite::Result<Option<i64>> {
        if !seaorm_enabled() {
            return self.deref().next_enabled_plugin_task_run_at();
        }
        seaorm_block_on(move |s| async move {
            PluginsRepository::next_enabled_plugin_task_run_at(s.connection())
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn insert_plugin_run_log(&self, log: &PluginRunLog) -> rusqlite::Result<i64> {
        if !seaorm_enabled() {
            return self.deref().insert_plugin_run_log(log);
        }
        let log = log.clone();
        seaorm_block_on(move |s| async move {
            PluginsRepository::insert_plugin_run_log(s.connection(), &log)
                .await
                .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_plugin_run_logs(
        &self,
        plugin_id: Option<&str>,
        task_id: Option<&str>,
        limit: i64,
    ) -> rusqlite::Result<Vec<PluginRunLog>> {
        if !seaorm_enabled() {
            return self.deref().list_plugin_run_logs(plugin_id, task_id, limit);
        }
        let plugin_id = plugin_id.map(ToOwned::to_owned);
        let task_id = task_id.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_plugin_run_logs(
                s.connection(),
                plugin_id.as_deref(),
                task_id.as_deref(),
                limit,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
    pub(crate) fn list_plugin_run_log_summaries(
        &self,
        plugin_id: Option<&str>,
        task_id: Option<&str>,
        limit: i64,
    ) -> rusqlite::Result<Vec<PluginRunLogListSummary>> {
        if !seaorm_enabled() {
            return self
                .deref()
                .list_plugin_run_log_summaries(plugin_id, task_id, limit);
        }
        let plugin_id = plugin_id.map(ToOwned::to_owned);
        let task_id = task_id.map(ToOwned::to_owned);
        seaorm_block_on(move |s| async move {
            PluginsRepository::list_plugin_run_log_summaries(
                s.connection(),
                plugin_id.as_deref(),
                task_id.as_deref(),
                limit,
            )
            .await
            .map_err(|e| e.to_string())
        })
        .map_err(error)
    }
}
