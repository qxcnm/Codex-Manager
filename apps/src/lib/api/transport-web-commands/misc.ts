import type { WebCommandDescriptor } from "./shared";
import { openExternalUrlDirect, openInBrowserDirect, showMainWindowDirect, unsupportedOpenInFileManager, unsupportedOpenUpdateLogsDir } from "./browser-direct";

export function createMiscWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_initialize: { rpcMethod: "initialize" },
    service_startup_snapshot: { rpcMethod: "startup/snapshot" },
    app_settings_get: { rpcMethod: "appSettings/get" },
    app_settings_set: { rpcMethod: "appSettings/set", mapParams: (params) => params && typeof params.patch === "object" && params.patch !== null ? (params.patch as Record<string, unknown>) : {} },
    service_requestlog_list: { rpcMethod: "requestlog/list" },
    service_requestlog_summary: { rpcMethod: "requestlog/summary" },
    service_requestlog_clear: { rpcMethod: "requestlog/clear" },
    service_requestlog_today_summary: { rpcMethod: "requestlog/today_summary" },
    service_plugin_catalog_list: { rpcMethod: "plugin/catalog/list" },
    service_plugin_catalog_refresh: { rpcMethod: "plugin/catalog/refresh" },
    service_plugin_install: { rpcMethod: "plugin/install" },
    service_plugin_update: { rpcMethod: "plugin/update" },
    service_plugin_uninstall: { rpcMethod: "plugin/uninstall" },
    service_plugin_list: { rpcMethod: "plugin/list" },
    service_plugin_enable: { rpcMethod: "plugin/enable" },
    service_plugin_disable: { rpcMethod: "plugin/disable" },
    service_plugin_tasks_update: { rpcMethod: "plugin/tasks/update" },
    service_plugin_tasks_list: { rpcMethod: "plugin/tasks/list" },
    service_plugin_tasks_run: { rpcMethod: "plugin/tasks/run" },
    service_plugin_logs_list: { rpcMethod: "plugin/logs/list" },
    service_listen_config_get: { rpcMethod: "service/listenConfig/get" },
    service_listen_config_set: { rpcMethod: "service/listenConfig/set" },
    open_in_browser: { direct: openInBrowserDirect },
    open_external_url: { direct: openExternalUrlDirect },
    open_in_file_manager: { direct: unsupportedOpenInFileManager },
    app_show_main_window: { direct: showMainWindowDirect },
    app_update_open_logs_dir: { direct: unsupportedOpenUpdateLogsDir },
  };
}
