import { invoke, withAddr } from "./transport";
import {
  GatewayConcurrencyRecommendation,
  readGatewayManualAccountId,
  GatewayRouteStrategySettings,
  GatewayTransportSettings,
  GatewayUpstreamProxySettings,
  ServiceListenConfig,
  readGatewayConcurrencyRecommendation,
  readGatewayRouteStrategySettings,
  readGatewayTransportSettings,
  readGatewayUpstreamProxySettings,
  readServiceListenConfig,
} from "./gateway-settings";
import {
  normalizeAppSettings,
  normalizeBackgroundTasks,
  normalizeGatewayErrorLogListResult,
  normalizeRequestLogFilterSummary,
  normalizeRequestLogListResult,
  normalizeStartupSnapshot,
  normalizeTodaySummary,
} from "./normalize";
import {
  BackgroundTaskSettings,
  GatewayErrorLogListResult,
  RequestLogFilterSummary,
  RequestLogListResult,
  RequestLogTodaySummary,
  ServiceInitializationResult,
  StartupSnapshot,
} from "../../types";
import { readInitializeResult } from "@/lib/utils/service";

export const serviceClient = {
  start: (addr?: string) => invoke("service_start", { addr }),
  stop: () => invoke("service_stop"),
  async initialize(addr?: string): Promise<ServiceInitializationResult> {
    const result = await invoke<unknown>(
      "service_initialize",
      addr ? { addr } : withAddr()
    );
    return readInitializeResult(result);
  },
  async getStartupSnapshot(
    params?: {
      requestLogLimit?: number;
      dayStartTs?: number;
      dayEndTs?: number;
    }
  ): Promise<StartupSnapshot> {
    const result = await invoke<unknown>(
      "service_startup_snapshot",
      withAddr(params)
    );
    return normalizeStartupSnapshot(result);
  },
  syncCodexModelsCache: (params: {
    userAgent: string;
    models: Array<Record<string, unknown>>;
    codexHome?: string | null;
    etag?: string | null;
    fetchedAt?: string;
  }) =>
    invoke<unknown>("service_sync_codex_models_cache", {
      userAgent: params.userAgent,
      models: params.models,
      codexHome: params.codexHome || null,
      etag: params.etag || null,
      fetchedAt: params.fetchedAt || new Date().toISOString(),
    }),
  syncCodexProviderConfig: (params: {
    baseUrl: string;
    providerId?: string | null;
    providerName?: string | null;
    envKey?: string | null;
    codexHome?: string | null;
  }) =>
    invoke<unknown>("service_sync_codex_provider_config", {
      baseUrl: params.baseUrl,
      providerId: params.providerId || "codexmanager",
      providerName: params.providerName || "CodexManager Local",
      envKey: params.envKey || "CODEXMANAGER_API_KEY",
      codexHome: params.codexHome || null,
    }),

  async getGatewayTransport(): Promise<GatewayTransportSettings> {
    const result = await invoke<unknown>("service_gateway_transport_get", withAddr());
    return readGatewayTransportSettings(result);
  },
  async setGatewayTransport(
    settings: Record<string, unknown>
  ): Promise<GatewayTransportSettings> {
    const result = await invoke<unknown>(
      "service_gateway_transport_set",
      withAddr(settings)
    );
    return readGatewayTransportSettings(result);
  },
  async getUpstreamProxy(): Promise<GatewayUpstreamProxySettings> {
    const result = await invoke<unknown>("service_gateway_upstream_proxy_get", withAddr());
    return readGatewayUpstreamProxySettings(result);
  },
  async setUpstreamProxy(proxyUrl: string): Promise<GatewayUpstreamProxySettings> {
    const result = await invoke<unknown>(
      "service_gateway_upstream_proxy_set",
      withAddr({ proxyUrl })
    );
    return readGatewayUpstreamProxySettings(result);
  },
  async getRouteStrategy(): Promise<GatewayRouteStrategySettings> {
    const result = await invoke<unknown>("service_gateway_route_strategy_get", withAddr());
    return readGatewayRouteStrategySettings(result);
  },
  async setRouteStrategy(strategy: string): Promise<GatewayRouteStrategySettings> {
    const result = await invoke<unknown>(
      "service_gateway_route_strategy_set",
      withAddr({ strategy })
    );
    return readGatewayRouteStrategySettings(result);
  },
  async getManualPreferredAccountId(): Promise<string> {
    const result = await invoke<unknown>("service_gateway_manual_account_get", withAddr());
    return readGatewayManualAccountId(result);
  },
  setManualPreferredAccount: (accountId: string) =>
    invoke("service_gateway_manual_account_set", withAddr({ accountId })),
  clearManualPreferredAccount: () =>
    invoke("service_gateway_manual_account_clear", withAddr()),

  getBackgroundTasks: () =>
    invoke<unknown>("service_gateway_background_tasks_get", withAddr()).then(
      normalizeBackgroundTasks
    ),
  setBackgroundTasks: (settings: BackgroundTaskSettings) =>
    invoke<unknown>(
      "service_gateway_background_tasks_set",
      withAddr({ ...(settings as unknown as Record<string, unknown>) })
    ).then(normalizeBackgroundTasks),
  async getConcurrencyRecommendation(): Promise<GatewayConcurrencyRecommendation> {
    const result = await invoke<unknown>(
      "service_gateway_concurrency_recommend_get",
      withAddr()
    );
    return readGatewayConcurrencyRecommendation(result);
  },

  async listRequestLogs(params?: {
    query?: string;
    statusFilter?: string;
    page?: number;
    pageSize?: number;
    startTs?: number | null;
    endTs?: number | null;
  }): Promise<RequestLogListResult> {
    const result = await invoke<unknown>(
      "service_requestlog_list",
      withAddr({
        query: params?.query || "",
        statusFilter: params?.statusFilter || "all",
        page: params?.page ?? 1,
        pageSize: params?.pageSize ?? 20,
        startTs: params?.startTs ?? null,
        endTs: params?.endTs ?? null,
      })
    );
    return normalizeRequestLogListResult(result);
  },
  async getRequestLogSummary(params?: {
    query?: string;
    statusFilter?: string;
    startTs?: number | null;
    endTs?: number | null;
  }): Promise<RequestLogFilterSummary> {
    const result = await invoke<unknown>(
      "service_requestlog_summary",
      withAddr({
        query: params?.query || "",
        statusFilter: params?.statusFilter || "all",
        startTs: params?.startTs ?? null,
        endTs: params?.endTs ?? null,
      })
    );
    return normalizeRequestLogFilterSummary(result);
  },
  async listGatewayErrorLogs(params?: {
    page?: number;
    pageSize?: number;
    stageFilter?: string;
  }): Promise<GatewayErrorLogListResult> {
    const result = await invoke<unknown>(
      "service_requestlog_error_list",
      withAddr({
        page: params?.page ?? 1,
        pageSize: params?.pageSize ?? 10,
        stageFilter: params?.stageFilter || "all",
      })
    );
    return normalizeGatewayErrorLogListResult(result);
  },
  clearGatewayErrorLogs: () =>
    invoke("service_requestlog_error_clear", withAddr()),
  clearRequestLogs: () => invoke("service_requestlog_clear", withAddr()),
  async getTodaySummary(params?: {
    dayStartTs?: number;
    dayEndTs?: number;
  }): Promise<RequestLogTodaySummary> {
    const result = await invoke<unknown>(
      "service_requestlog_today_summary",
      withAddr(params)
    );
    return normalizeTodaySummary(result);
  },

  async getListenConfig(): Promise<ServiceListenConfig> {
    const result = await invoke<unknown>("service_listen_config_get", withAddr());
    return readServiceListenConfig(result);
  },
  async setListenConfig(mode: string): Promise<ServiceListenConfig> {
    const result = await invoke<unknown>(
      "service_listen_config_set",
      withAddr({ mode })
    );
    return readServiceListenConfig(result);
  },

  getEnvOverrides: async () => {
    const result = await invoke<unknown>("app_settings_get");
    return normalizeAppSettings(result).envOverrides;
  },
};
