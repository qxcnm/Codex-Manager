import type { RequestOptions } from "../utils/request";

export type InvokeParams = Record<string, unknown>;

export type WebCommandDescriptor = {
  rpcMethod?: string;
  mapParams?: (params?: InvokeParams) => InvokeParams;
  direct?: (params?: InvokeParams, options?: RequestOptions) => Promise<unknown>;
};

type WebRpcCaller = <T>(
  rpcMethod: string,
  params?: InvokeParams,
  options?: RequestOptions
) => Promise<T>;

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function mapKeyIdToId(params?: InvokeParams): InvokeParams {
  const source = params ?? {};
  const keyId =
    typeof source.keyId === "string" && source.keyId.trim()
      ? source.keyId.trim()
      : undefined;
  if (!keyId) {
    return source;
  }
  return {
    ...source,
    id: keyId,
  };
}

function isSupportedBrowserImportFile(file: File): boolean {
  const normalizedName = String(file.name || "").trim().toLowerCase();
  return normalizedName.endsWith(".json") || normalizedName.endsWith(".txt");
}

async function pickImportFilesFromBrowser(directory: boolean): Promise<unknown> {
  if (typeof document === "undefined") {
    throw new Error("当前环境不支持浏览器文件选择");
  }

  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".json,.txt,application/json,text/plain";
  input.multiple = true;
  if (directory) {
    const directoryInput = input as HTMLInputElement & {
      directory?: boolean;
      webkitdirectory?: boolean;
    };
    directoryInput.directory = true;
    directoryInput.webkitdirectory = true;
  }
  input.style.display = "none";
  document.body.appendChild(input);

  return await new Promise<unknown>((resolve, reject) => {
    let finished = false;

    const cleanup = () => {
      input.removeEventListener("change", handleChange);
      input.removeEventListener("cancel", handleCancel as EventListener);
      input.remove();
    };

    const finish = (value: unknown) => {
      if (finished) return;
      finished = true;
      cleanup();
      resolve(value);
    };

    const fail = (error: unknown) => {
      if (finished) return;
      finished = true;
      cleanup();
      reject(error);
    };

    const handleCancel = () => {
      finish({
        ok: true,
        canceled: true,
      });
    };

    const handleChange = async () => {
      try {
        const files = Array.from(input.files ?? []);
        if (!files.length) {
          handleCancel();
          return;
        }

        const importableFiles = files.filter(isSupportedBrowserImportFile);
        if (!importableFiles.length) {
          fail(
            new Error(
              directory
                ? "所选目录中没有可导入的 .json 或 .txt 文件"
                : "请选择 .json 或 .txt 文件"
            )
          );
          return;
        }

        const fileEntries = await Promise.all(
          importableFiles.map(async (file) => {
            const content = await file.text();
            const relativePath =
              (file as File & { webkitRelativePath?: string }).webkitRelativePath ||
              file.name;
            return {
              content,
              path: relativePath || file.name,
            };
          })
        );
        const nonEmptyEntries = fileEntries.filter(
          (entry) => entry.content.trim().length > 0
        );
        if (!nonEmptyEntries.length) {
          fail(new Error("未在所选文件中找到可导入内容"));
          return;
        }

        const filePaths = nonEmptyEntries.map((entry) => entry.path);
        const contents = nonEmptyEntries.map((entry) => entry.content);
        const directorySourcePath = filePaths[0] || fileEntries[0]?.path || "";
        const directoryPath = directory
          ? directorySourcePath.split("/")[0] || directorySourcePath.split("\\")[0] || ""
          : "";

        finish({
          ok: true,
          canceled: false,
          directoryPath,
          fileCount: importableFiles.length,
          filePaths,
          contents,
        });
      } catch (error) {
        fail(error);
      }
    };

    input.addEventListener("change", handleChange);
    input.addEventListener("cancel", handleCancel as EventListener);
    input.click();
  });
}

async function exportAccountsViaBrowser(
  postWebRpc: WebRpcCaller,
  params: Record<string, unknown> | null = null,
  options: RequestOptions = {}
): Promise<unknown> {
  if (typeof document === "undefined") {
    throw new Error("当前环境不支持浏览器导出");
  }

  const selectedAccountIds = Array.isArray(params?.selectedAccountIds)
    ? params.selectedAccountIds
        .map((item) => String(item || "").trim())
        .filter(Boolean)
    : [];
  const exportMode =
    typeof params?.exportMode === "string" && params.exportMode.trim()
      ? params.exportMode.trim()
      : "multiple";
  const payload =
    asRecord(
      await postWebRpc<unknown>(
        "account/exportData",
        {
          selectedAccountIds,
          exportMode,
        },
        options
      )
    ) ?? {};
  const files = Array.isArray(payload.files)
    ? payload.files
        .map((item) => asRecord(item))
        .filter((item): item is Record<string, unknown> => item !== null)
    : [];

  for (const item of files) {
    const fileName =
      typeof item.fileName === "string" && item.fileName.trim()
        ? item.fileName.trim()
        : "account.json";
    const content = typeof item.content === "string" ? item.content : "";
    const blob = new Blob([content], {
      type: "application/json;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = fileName;
    anchor.style.display = "none";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  }

  return {
    ok: true,
    canceled: false,
    exported:
      typeof payload.exported === "number" ? payload.exported : files.length,
    outputDir: "browser-download",
  };
}

export function createWebCommandMap(
  postWebRpc: WebRpcCaller
): Record<string, WebCommandDescriptor> {
  return {
    app_settings_get: { rpcMethod: "appSettings/get" },
    app_settings_set: {
      rpcMethod: "appSettings/set",
      mapParams: (params) => asRecord(asRecord(params)?.patch) ?? {},
    },
    service_initialize: { rpcMethod: "initialize" },
    service_startup_snapshot: { rpcMethod: "startup/snapshot" },
    service_account_list: { rpcMethod: "account/list" },
    service_account_delete: { rpcMethod: "account/delete" },
    service_account_delete_many: { rpcMethod: "account/deleteMany" },
    service_account_delete_by_statuses: {
      rpcMethod: "account/deleteByStatuses",
    },
    service_account_delete_unavailable_free: {
      rpcMethod: "account/deleteUnavailableFree",
    },
    service_account_update: { rpcMethod: "account/update" },
    service_account_import: { rpcMethod: "account/import" },
    service_account_import_by_file: {
      direct: () => pickImportFilesFromBrowser(false),
    },
    service_account_import_by_directory: {
      direct: () => pickImportFilesFromBrowser(true),
    },
    service_account_export_by_account_files: {
      direct: (params, options) =>
        exportAccountsViaBrowser(postWebRpc, asRecord(params), options),
    },
    service_account_warmup: { rpcMethod: "account/warmup" },
    service_account_manager_status: { rpcMethod: "accountManager/status" },
    service_account_manager_session_current: {
      rpcMethod: "accountManager/session/current",
    },
    service_account_manager_profile_update: {
      rpcMethod: "accountManager/profile/update",
    },
    service_account_manager_password_change: {
      rpcMethod: "accountManager/password/change",
    },
    service_account_manager_users_list: { rpcMethod: "accountManager/users/list" },
    service_account_manager_user_create: {
      rpcMethod: "accountManager/users/create",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_account_manager_user_update: {
      rpcMethod: "accountManager/users/update",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_account_manager_user_delete: {
      rpcMethod: "accountManager/users/delete",
    },
    service_account_manager_wallet_top_up: {
      rpcMethod: "accountManager/wallet/topUp",
      mapParams: (params) => {
        const source = asRecord(params) ?? {};
        return {
          ownerKind: source.owner_kind ?? source.ownerKind,
          ownerId: source.owner_id ?? source.ownerId,
          amountCreditMicros:
            source.amount_credit_micros ?? source.amountCreditMicros,
          note: source.note,
        };
      },
    },
    service_account_manager_wallet_set_available: {
      rpcMethod: "accountManager/wallet/setAvailable",
      mapParams: (params) => {
        const source = asRecord(params) ?? {};
        return {
          ownerKind: source.owner_kind ?? source.ownerKind,
          ownerId: source.owner_id ?? source.ownerId,
          availableCreditMicros:
            source.available_credit_micros ?? source.availableCreditMicros,
          note: source.note,
        };
      },
    },
    service_account_manager_api_key_owners_list: {
      rpcMethod: "accountManager/apiKeyOwners/list",
    },
    service_account_manager_api_key_owner_set: {
      rpcMethod: "accountManager/apiKeyOwners/set",
      mapParams: (params) => {
        const source = asRecord(params) ?? {};
        return {
          keyId: source.key_id ?? source.keyId,
          ownerKind: source.owner_kind ?? source.ownerKind,
          ownerUserId: source.owner_user_id ?? source.ownerUserId,
          projectId: source.project_id ?? source.projectId,
        };
      },
    },
    service_model_groups_list: { rpcMethod: "modelGroups/list" },
    service_model_group_save: { rpcMethod: "modelGroups/save" },
    service_model_group_delete: { rpcMethod: "modelGroups/delete" },
    service_model_group_models_set: { rpcMethod: "modelGroups/setModels" },
    service_model_group_users_set: { rpcMethod: "modelGroups/setUsers" },
    service_dashboard_admin_usage_summary: {
      rpcMethod: "dashboard/adminUsageSummary",
      mapParams: (params) => {
        const source = asRecord(params) ?? {};
        return {
          startTs: source.start_ts ?? source.startTs,
          endTs: source.end_ts ?? source.endTs,
        };
      },
    },
    service_dashboard_member_summary: {
      rpcMethod: "dashboard/memberSummary",
      mapParams: (params) => {
        const source = asRecord(params) ?? {};
        return {
          userId: source.user_id ?? source.userId,
          dayStartTs: source.day_start_ts ?? source.dayStartTs,
          dayEndTs: source.day_end_ts ?? source.dayEndTs,
        };
      },
    },
    service_usage_read: { rpcMethod: "account/usage/read" },
    service_usage_list: { rpcMethod: "account/usage/list" },
    service_usage_refresh: { rpcMethod: "account/usage/refresh" },
    service_usage_quota_reset: { rpcMethod: "account/usage/resetQuota" },
    service_usage_aggregate: { rpcMethod: "account/usage/aggregate" },
    service_quota_overview: { rpcMethod: "quota/overview" },
    service_quota_model_usage: { rpcMethod: "quota/modelUsage" },
    service_quota_api_key_usage: { rpcMethod: "quota/apiKeyUsage" },
    service_quota_source_list: { rpcMethod: "quota/sourceList" },
    service_quota_model_pools: { rpcMethod: "quota/modelPools" },
    service_quota_system_pool: { rpcMethod: "quota/systemPool" },
    service_quota_capacity_config: { rpcMethod: "quota/capacityConfig" },
    service_quota_billing_rules: { rpcMethod: "quota/billingRules" },
    service_quota_billing_rule_upsert: {
      rpcMethod: "quota/billingRule/upsert",
      mapParams: (params) => ({
        id: typeof params?.id === "string" ? params.id : null,
        name: typeof params?.name === "string" ? params.name : "",
        status: typeof params?.status === "string" ? params.status : null,
        priority: typeof params?.priority === "number" ? params.priority : null,
        multiplierMillis:
          typeof params?.multiplierMillis === "number"
            ? params.multiplierMillis
            : typeof params?.multiplier_millis === "number"
              ? params.multiplier_millis
              : 1000,
        modelPattern:
          typeof params?.modelPattern === "string"
            ? params.modelPattern
            : typeof params?.model_pattern === "string"
              ? params.model_pattern
              : null,
        serviceTier:
          typeof params?.serviceTier === "string"
            ? params.serviceTier
            : typeof params?.service_tier === "string"
              ? params.service_tier
              : null,
        userId:
          typeof params?.userId === "string"
            ? params.userId
            : typeof params?.user_id === "string"
              ? params.user_id
              : null,
        apiKeyId:
          typeof params?.apiKeyId === "string"
            ? params.apiKeyId
            : typeof params?.api_key_id === "string"
              ? params.api_key_id
              : null,
        startsAt:
          typeof params?.startsAt === "number"
            ? params.startsAt
            : typeof params?.starts_at === "number"
              ? params.starts_at
              : null,
        endsAt:
          typeof params?.endsAt === "number"
            ? params.endsAt
            : typeof params?.ends_at === "number"
              ? params.ends_at
              : null,
      }),
    },
    service_quota_billing_rule_delete: {
      rpcMethod: "quota/billingRule/delete",
      mapParams: (params) => ({
        id: typeof params?.id === "string" ? params.id : "",
      }),
    },
    service_quota_source_models_set: {
      rpcMethod: "quota/sourceModels/set",
      mapParams: (params) => ({
        sourceKind:
          typeof params?.sourceKind === "string"
            ? params.sourceKind
            : typeof params?.source_kind === "string"
              ? params.source_kind
              : "",
        sourceId:
          typeof params?.sourceId === "string"
            ? params.sourceId
            : typeof params?.source_id === "string"
              ? params.source_id
              : "",
        modelSlugs: Array.isArray(params?.modelSlugs)
          ? params.modelSlugs
          : Array.isArray(params?.model_slugs)
            ? params.model_slugs
            : [],
      }),
    },
    service_quota_capacity_template_update: {
      rpcMethod: "quota/capacityTemplate/update",
    },
    service_quota_account_capacity_override_update: {
      rpcMethod: "quota/accountCapacityOverride/update",
    },
    service_quota_refresh_sources: {
      rpcMethod: "quota/refreshSources",
      mapParams: (params) => ({
        kinds: Array.isArray(params?.kinds) ? params.kinds : [],
        sourceIds: Array.isArray(params?.sourceIds)
          ? params.sourceIds
          : Array.isArray(params?.source_ids)
            ? params.source_ids
            : [],
      }),
    },
    service_aggregate_api_list: { rpcMethod: "aggregateApi/list" },
    service_aggregate_api_create: { rpcMethod: "aggregateApi/create" },
    service_aggregate_api_update: { rpcMethod: "aggregateApi/update" },
    service_aggregate_api_delete: { rpcMethod: "aggregateApi/delete" },
    service_aggregate_api_read_secret: { rpcMethod: "aggregateApi/readSecret" },
    service_aggregate_api_test_connection: {
      rpcMethod: "aggregateApi/testConnection",
    },
    service_aggregate_api_refresh_balance: {
      rpcMethod: "aggregateApi/refreshBalance",
    },
    service_aggregate_api_supplier_models_list: {
      rpcMethod: "aggregateApi/supplierModels/list",
    },
    service_aggregate_api_supplier_model_save: {
      rpcMethod: "aggregateApi/supplierModels/save",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_aggregate_api_supplier_model_delete: {
      rpcMethod: "aggregateApi/supplierModels/delete",
    },
    service_aggregate_api_supplier_models_import: {
      rpcMethod: "aggregateApi/sourceModels/importSupplier",
    },
    service_login_start: {
      rpcMethod: "account/login/start",
      mapParams: (params) => ({
        ...(params ?? {}),
        type:
          typeof params?.loginType === "string" && params.loginType.trim()
            ? params.loginType
            : "chatgpt",
        openBrowser: false,
      }),
    },
    service_login_status: { rpcMethod: "account/login/status" },
    service_login_complete: { rpcMethod: "account/login/complete" },
    service_login_chatgpt_auth_tokens: {
      rpcMethod: "account/login/start",
      mapParams: (params) => ({
        ...(params ?? {}),
        type: "chatgptAuthTokens",
      }),
    },
    service_account_read: { rpcMethod: "account/read" },
    service_account_logout: { rpcMethod: "account/logout" },
    service_chatgpt_auth_tokens_refresh: {
      rpcMethod: "account/chatgptAuthTokens/refresh",
    },
    service_chatgpt_auth_tokens_refresh_all: {
      rpcMethod: "account/chatgptAuthTokens/refreshAll",
    },
    service_apikey_list: { rpcMethod: "apikey/list" },
    service_apikey_create: { rpcMethod: "apikey/create" },
    service_apikey_usage_stats: { rpcMethod: "apikey/usageStats" },
    service_apikey_delete: {
      rpcMethod: "apikey/delete",
      mapParams: mapKeyIdToId,
    },
    service_apikey_update_model: {
      rpcMethod: "apikey/updateModel",
      mapParams: mapKeyIdToId,
    },
    service_apikey_disable: {
      rpcMethod: "apikey/disable",
      mapParams: mapKeyIdToId,
    },
    service_apikey_enable: {
      rpcMethod: "apikey/enable",
      mapParams: mapKeyIdToId,
    },
    service_apikey_models: { rpcMethod: "apikey/models" },
    service_model_catalog_list: { rpcMethod: "apikey/modelCatalogList" },
    service_model_catalog_save: {
      rpcMethod: "apikey/modelCatalogSave",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_model_catalog_delete: { rpcMethod: "apikey/modelCatalogDelete" },
    service_model_routing: { rpcMethod: "apikey/modelRouting" },
    service_model_source_sync: {
      rpcMethod: "apikey/modelSourceSync",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_model_source_model_save: {
      rpcMethod: "apikey/modelSourceModelSave",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_model_source_mapping_save: {
      rpcMethod: "apikey/modelSourceMappingSave",
      mapParams: (params) => asRecord(asRecord(params)?.payload) ?? {},
    },
    service_model_source_mapping_delete: {
      rpcMethod: "apikey/modelSourceMappingDelete",
    },
    service_apikey_read_secret: {
      rpcMethod: "apikey/readSecret",
      mapParams: mapKeyIdToId,
    },
    service_gateway_transport_get: { rpcMethod: "gateway/transport/get" },
    service_gateway_transport_set: { rpcMethod: "gateway/transport/set" },
    service_gateway_upstream_proxy_get: { rpcMethod: "gateway/upstreamProxy/get" },
    service_gateway_upstream_proxy_set: { rpcMethod: "gateway/upstreamProxy/set" },
    service_gateway_route_strategy_get: { rpcMethod: "gateway/routeStrategy/get" },
    service_gateway_route_strategy_set: { rpcMethod: "gateway/routeStrategy/set" },
    service_gateway_manual_account_get: { rpcMethod: "gateway/manualAccount/get" },
    service_gateway_manual_account_set: { rpcMethod: "gateway/manualAccount/set" },
    service_gateway_manual_account_clear: {
      rpcMethod: "gateway/manualAccount/clear",
    },
    service_gateway_background_tasks_get: {
      rpcMethod: "gateway/backgroundTasks/get",
    },
    service_gateway_background_tasks_set: {
      rpcMethod: "gateway/backgroundTasks/set",
    },
    service_gateway_concurrency_recommend_get: {
      rpcMethod: "gateway/concurrencyRecommendation/get",
    },
    service_gateway_codex_latest_version_get: {
      rpcMethod: "gateway/codexLatestVersion/get",
    },
    service_requestlog_list: { rpcMethod: "requestlog/list" },
    service_requestlog_error_list: { rpcMethod: "requestlog/error_list" },
    service_requestlog_error_clear: { rpcMethod: "requestlog/error_clear" },
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
    open_in_browser: {
      direct: async (params) => {
        const url = typeof params?.url === "string" ? params.url.trim() : "";
        if (!url) {
          throw new Error("缺少浏览器跳转地址");
        }
        if (typeof window === "undefined") {
          throw new Error("当前环境不支持打开浏览器");
        }
        window.open(url, "_blank", "noopener,noreferrer");
        return { ok: true };
      },
    },
    open_external_url: {
      direct: async (params) => {
        const url = typeof params?.url === "string" ? params.url.trim() : "";
        if (!url) {
          throw new Error("缺少外部跳转地址");
        }
        if (typeof window === "undefined") {
          throw new Error("当前环境不支持打开外部链接");
        }
        window.location.href = url;
        return { ok: true };
      },
    },
    open_in_file_manager: {
      direct: async () => {
        throw new Error("当前环境不支持打开本地目录");
      },
    },
    app_show_main_window: {
      direct: async () => {
        if (typeof window !== "undefined") {
          window.location.href = "/";
        }
        return { ok: true };
      },
    },
    app_update_open_logs_dir: {
      direct: async () => {
        throw new Error("当前环境不支持打开更新日志目录");
      },
    },
  };
}
