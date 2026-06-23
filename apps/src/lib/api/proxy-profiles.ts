import { invoke, withAddr } from "./transport";
import {
  normalizeProxyProfile,
  normalizeProxyProfileListResult,
  normalizeProxyTestPresetsResult,
  normalizeProxyTestJobState,
  normalizeProxySpeedTestListResult,
  normalizeProxyDiagnosticTestListResult,
  normalizeAccountProxyUrlTestListResult,
  normalizeProxyProfileUrlTestListResult,
} from "./normalize";
import type {
  ProxyProfile,
  ProxyProfileListResult,
  ProxyTestPresetsResult,
  ProxyTestJobState,
  ProxySpeedTestListResult,
  ProxyProfileUrlTestListResult,
  ProxyDiagnosticTestListResult,
} from "@/types";

export const PROXY_PROFILES_QUERY_KEY = ["proxy-profiles"] as const;
export const PROXY_TEST_PRESETS_QUERY_KEY = ["proxy-test-presets"] as const;

export interface ProxyProfileCreatePayload {
  name: string;
  proxyUrl: string;
  enabled?: boolean;
  tagsJson?: string | null;
  notes?: string | null;
}

export interface ProxyProfileUpdatePayload {
  id: string;
  name?: string | null;
  proxyUrl?: string | null;
  enabled?: boolean | null;
  tagsJson?: string | null;
  notes?: string | null;
}

export interface ProxyProfileLatencyTestPayload {
  id: string;
}

import type { CfStyleConfig } from "./account-client";

export interface ProxyProfileCloudflareSpeedTestPayload {
  id: string;
  config?: CfStyleConfig | null;
}

export interface ProxyProfileSpeedTestPayload {

  id: string;
  providerId?: string | null;
  fileSizeId?: string | null;
  diagnosticProviderId?: string | null;
  diagnosticFileSizeId?: string | null;
}

export interface ProxyProfileTestJobPayload {
  jobId: string;
}

export const proxyProfilesClient = {
  async listProxyProfiles(): Promise<ProxyProfileListResult> {
    const result = await invoke<unknown>("service_system_proxy_list", withAddr());
    return normalizeProxyProfileListResult(result);
  },
  async listProxyTestPresets(): Promise<ProxyTestPresetsResult> {
    const result = await invoke<unknown>(
      "service_system_proxy_test_presets",
      withAddr(),
    );
    return normalizeProxyTestPresetsResult(result);
  },
  async createProxyProfile(
    payload: ProxyProfileCreatePayload,
  ): Promise<ProxyProfile> {
    const result = await invoke<unknown>(
      "service_system_proxy_create",
      withAddr({
        name: payload.name,
        proxyUrl: payload.proxyUrl,
        enabled: payload.enabled ?? true,
        tagsJson: payload.tagsJson ?? null,
        notes: payload.notes ?? null,
      }),
    );
    return normalizeProxyProfile(result);
  },
  async updateProxyProfile(
    payload: ProxyProfileUpdatePayload,
  ): Promise<ProxyProfile> {
    const result = await invoke<unknown>(
      "service_system_proxy_update",
      withAddr({
        id: payload.id,
        name: payload.name ?? null,
        proxyUrl: payload.proxyUrl ?? null,
        enabled: payload.enabled ?? null,
        tagsJson: payload.tagsJson ?? null,
        notes: payload.notes ?? null,
      }),
    );
    return normalizeProxyProfile(result);
  },
  async deleteProxyProfile(id: string): Promise<void> {
    await invoke("service_system_proxy_delete", withAddr({ id }));
  },
  async testProxyProfileLatency(
    payload: ProxyProfileLatencyTestPayload,
  ): Promise<ProxyTestJobState> {
    const result = await invoke<unknown>(
      "service_system_proxy_test_latency",
      withAddr({
        id: payload.id,
      }),
    );
    return normalizeProxyTestJobState(result);
  },
  async testProxyProfileSpeed(
    payload: ProxyProfileSpeedTestPayload,
  ): Promise<ProxyTestJobState> {
    const result = await invoke<unknown>(
      "service_system_proxy_speed_test",
      withAddr({
        id: payload.id,
        providerId: payload.providerId ?? null,
        fileSizeId: payload.fileSizeId ?? null,
        diagnosticProviderId: payload.diagnosticProviderId ?? null,
        diagnosticFileSizeId: payload.diagnosticFileSizeId ?? null,
      }),
    );
    return normalizeProxyTestJobState(result);
  },
  async testProxyProfileCloudflareSpeed(
    payload: ProxyProfileCloudflareSpeedTestPayload,
  ): Promise<ProxyTestJobState> {
    const result = await invoke<unknown>(
      "service_system_proxy_cloudflare_speed_test",
      withAddr({
        id: payload.id,
        config: payload.config ?? null,
      }),
    );
    return normalizeProxyTestJobState(result);
  },

  async getProxyTestJob(payload: ProxyProfileTestJobPayload): Promise<ProxyTestJobState> {
    const result = await invoke<unknown>(
      "service_system_proxy_test_job",
      withAddr({
        jobId: payload.jobId,
      }),
    );
    return normalizeProxyTestJobState(result);
  },
  async cancelProxyTestJob(payload: ProxyProfileTestJobPayload): Promise<void> {
    await invoke(
      "service_system_proxy_cancel_test",
      withAddr({
        jobId: payload.jobId,
      }),
    );
  },
  async getProxyProfileSpeedHistory(payload: { id: string; limit?: number }): Promise<ProxySpeedTestListResult> {
    const result = await invoke<unknown>(
      "service_system_proxy_speed_test_history",
      withAddr({
        id: payload.id,
        limit: payload.limit ?? null,
      }),
    );
    return normalizeProxySpeedTestListResult(result);
  },
  async getProxyProfileLatencyHistory(payload: { id: string; limit?: number }): Promise<ProxyProfileUrlTestListResult> {
    const result = await invoke<unknown>(
      "service_system_proxy_latency_test_history",
      withAddr({
        id: payload.id,
        limit: payload.limit ?? null,
      }),
    );
    return normalizeProxyProfileUrlTestListResult(result);
  },
  async getProxyProfileDiagnosticsHistory(payload: { id: string; limit?: number }): Promise<ProxyDiagnosticTestListResult> {
    const result = await invoke<unknown>(
      "service_system_proxy_diagnostics_history",
      withAddr({
        id: payload.id,
        limit: payload.limit ?? null,
      }),
    );
    return normalizeProxyDiagnosticTestListResult(result);
  },
};
