import { invoke, withAddr } from "./transport";
import type { RequestOptions } from "@/lib/utils/request";
import {
  GatewayConcurrencyRecommendation,
  readGatewayManualAccountId,
  GatewayRouteStrategySettings,
  GatewayTransportSettings,
  GatewayTransportSettingsPatch,
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
  normalizeRequestLogFilterSummary,
  normalizeRequestLogListResult,
  normalizeRequestLogListWithSummaryResult,
  normalizeStartupSnapshot,
  normalizeTodaySummary,
} from "./normalize";
import {
  BackgroundTaskSettings,
  RequestLogFilterSummary,
  RequestLogListResult,
  RequestLogListWithSummaryResult,
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
      includeApiModels?: boolean;
      includeApiKeys?: boolean;
      includeAccounts?: boolean;
      includeUsageSnapshots?: boolean;
      includeAccountRuntime?: boolean;
      includeAccountDetails?: boolean;
    }
  ): Promise<StartupSnapshot> {
    const result = await invoke<unknown>(
      "service_startup_snapshot",
      withAddr(params)
    );
    return normalizeStartupSnapshot(result);
  },
  async getGatewayTransport(): Promise<GatewayTransportSettings> {
    const result = await invoke<unknown>("service_gateway_transport_get", withAddr());
    return readGatewayTransportSettings(result);
  },
  async setGatewayTransport(
    settings: GatewayTransportSettingsPatch
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
  }, options?: RequestOptions): Promise<RequestLogListResult> {
    const result = await invoke<unknown>(
      "service_requestlog_list",
      withAddr({
        query: params?.query || "",
        statusFilter: params?.statusFilter || "all",
        page: params?.page ?? 1,
        pageSize: params?.pageSize ?? 20,
        startTs: params?.startTs ?? null,
        endTs: params?.endTs ?? null,
      }),
      options
    );
    return normalizeRequestLogListResult(result);
  },
  async listRequestLogsWithSummary(params?: {
    query?: string;
    statusFilter?: string;
    page?: number;
    pageSize?: number;
    startTs?: number | null;
    endTs?: number | null;
  }, options?: RequestOptions): Promise<RequestLogListWithSummaryResult> {
    const result = await invoke<unknown>(
      "service_requestlog_list_with_summary",
      withAddr({
        query: params?.query || "",
        statusFilter: params?.statusFilter || "all",
        page: params?.page ?? 1,
        pageSize: params?.pageSize ?? 20,
        startTs: params?.startTs ?? null,
        endTs: params?.endTs ?? null,
      }),
      options
    );
    return normalizeRequestLogListWithSummaryResult(result);
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
