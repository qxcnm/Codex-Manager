"use client";

import { useMemo, useState, useSyncExternalStore } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  CODEX_PROFILE_CANDIDATES_QUERY_KEY,
  CODEX_PROFILE_STATUS_QUERY_KEY,
  codexProfileClient,
} from "@/lib/api/codex-profile-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import {
  buildOpenAiGatewayEndpoint,
  resolveGatewayOrigin,
} from "@/lib/gateway/endpoints";
import { useCodexProfileModeStatus } from "@/hooks/useCodexProfileModeStatus";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useAppStore } from "@/lib/store/useAppStore";
import type {
  CodexProfileHistoryRepairSummary,
  CodexProfileMode,
} from "@/types";

const EMPTY_CANDIDATES = { accounts: [], apiKeys: [] };

export function historyRepairChangeCount(
  summary: CodexProfileHistoryRepairSummary | null,
): number {
  if (!summary) return 0;
  return (
    summary.changedRolloutFileCount +
    summary.updatedSqliteRowCount +
    summary.addedSessionIndexEntryCount
  );
}

export function pickAvailableCandidateId<T extends { id: string }>(
  preferredId: string | null | undefined,
  managedId: string | null | undefined,
  candidates: T[],
): string {
  const ids = new Set(candidates.map((item) => item.id));
  if (preferredId && ids.has(preferredId)) return preferredId;
  if (managedId && ids.has(managedId)) return managedId;
  return candidates[0]?.id || "";
}

export function modeImpact(
  mode: CodexProfileMode | null,
  t: (value: string, params?: Record<string, string | number>) => string,
): string {
  if (mode === "direct_account") {
    return t("当前为账号直连，Codex CLI 直连 OpenAI，CodexManager 无法统计 CLI 请求日志和用量。");
  }
  if (mode === "gateway") {
    return t("当前为本地网关，Codex CLI 经过 CodexManager 转发，请求日志、Token 和费用统计可用。");
  }
  return t("选择账号直连或本地网关后，CodexManager 会接管该 Codex profile 的 auth.json / config.toml。");
}

export function usePlatformModePageState(
  t: (value: string, params?: Record<string, string | number>) => string,
) {
  const queryClient = useQueryClient();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { mode, canAccessManagementRpc } = useRuntimeCapabilities();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/platform-mode/");
  const [codexHomeDraft, setCodexHomeDraft] = useState<string | null>(null);
  const [selectedAccountIdDraft, setSelectedAccountIdDraft] = useState<string | null>(null);
  const [selectedApiKeyIdDraft, setSelectedApiKeyIdDraft] = useState<string | null>(null);
  const [gatewayBaseUrlDraft, setGatewayBaseUrlDraft] = useState<string | null>(null);
  const browserOrigin = useSyncExternalStore(
    () => () => undefined,
    () =>
      mode === "web-gateway" && typeof window !== "undefined"
        ? window.location.origin
        : "",
    () => "",
  );

  const defaultGatewayBaseUrl = useMemo(() => {
    const origin = resolveGatewayOrigin({
      browserOrigin,
      runtimeMode: mode,
      serviceAddr: serviceStatus.addr,
    });
    return buildOpenAiGatewayEndpoint(origin);
  }, [browserOrigin, mode, serviceStatus.addr]);

  const statusQuery = useCodexProfileModeStatus();
  const candidatesQuery = useQuery({
    queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY,
    queryFn: () => codexProfileClient.listCandidates(),
    enabled: isServiceReady,
    retry: 1,
    staleTime: 0,
    refetchInterval: isServiceReady && isPageActive ? 5_000 : false,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: true,
  });

  const status = statusQuery.status;
  const candidates = candidatesQuery.data || EMPTY_CANDIDATES;
  const codexHomeInput = codexHomeDraft ?? status?.codexHome ?? "";
  const selectedAccountId = pickAvailableCandidateId(
    selectedAccountIdDraft,
    status?.selectedAccountId,
    candidates.accounts,
  );
  const selectedApiKeyId = pickAvailableCandidateId(
    selectedApiKeyIdDraft,
    status?.selectedApiKeyId,
    candidates.apiKeys,
  );
  const gatewayBaseUrl =
    gatewayBaseUrlDraft ?? status?.gatewayBaseUrl ?? defaultGatewayBaseUrl;
  const isDirectActive = status?.mode === "direct_account";
  const isGatewayActive = status?.mode === "gateway";
  const activeAccountValue = status?.selectedAccountId
    ? candidates.accounts.find((item) => item.id === status.selectedAccountId)?.label ||
      status.selectedAccountId
    : "-";
  const activeKeyValue = status?.selectedApiKeyId
    ? candidates.apiKeys.find((item) => item.id === status.selectedApiKeyId)?.name ||
      status.selectedApiKeyId
    : "-";

  const refreshAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_STATUS_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY }),
    ]);
  };

  const showHistoryRepairToast = (summary: CodexProfileHistoryRepairSummary | null) => {
    if (!summary) return;
    if (summary.warnings.length > 0) {
      toast.warning(`${t("历史修复完成但有警告")}：${summary.warnings[0]}`);
      return;
    }
    if (historyRepairChangeCount(summary) > 0) {
      toast.success(t("历史会话可见性已修复"));
    }
  };

  const saveConfigMutation = useMutation({
    mutationFn: () => codexProfileClient.setConfig(codexHomeInput),
    onSuccess: async (nextStatus) => {
      setCodexHomeDraft(nextStatus.codexHome);
      await refreshAll();
      toast.success(t("Codex profile 路径已保存"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("保存失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const applyDirectMutation = useMutation({
    mutationFn: () =>
      codexProfileClient.applyDirectAccount({
        accountId: selectedAccountId,
        codexHome: codexHomeInput,
      }),
    onSuccess: async (nextStatus) => {
      await refreshAll();
      toast.success(t("已切换到账号直连"));
      showHistoryRepairToast(nextStatus.historyRepair);
    },
    onError: (error: unknown) => {
      toast.error(`${t("切换失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const applyGatewayMutation = useMutation({
    mutationFn: () =>
      codexProfileClient.applyGateway({
        apiKeyId: selectedApiKeyId,
        codexHome: codexHomeInput,
        baseUrl: gatewayBaseUrl,
      }),
    onSuccess: async (nextStatus) => {
      await refreshAll();
      toast.success(t("已切换到本地网关"));
      showHistoryRepairToast(nextStatus.historyRepair);
    },
    onError: (error: unknown) => {
      toast.error(`${t("切换失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const restoreMutation = useMutation({
    mutationFn: () => codexProfileClient.restore(codexHomeInput),
    onSuccess: async () => {
      await refreshAll();
      toast.success(t("已恢复接管前的 Codex 配置"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("恢复失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const repairHistoryMutation = useMutation({
    mutationFn: () => codexProfileClient.repairHistory(codexHomeInput),
    onSuccess: async (summary) => {
      await refreshAll();
      showHistoryRepairToast(summary);
      if (summary.warnings.length === 0 && historyRepairChangeCount(summary) === 0) {
        toast.success(t("历史会话已与当前模式一致"));
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("修复失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const pruneHistoryBackupsMutation = useMutation({
    mutationFn: () => codexProfileClient.pruneHistoryBackups(codexHomeInput),
    onSuccess: async (result) => {
      await refreshAll();
      if (result.warnings.length > 0) {
        toast.warning(`${t("清理完成但有警告")}：${result.warnings[0]}`);
        return;
      }
      toast.success(
        t("已清理 {count} 份历史备份，释放 {bytes}", {
          count: result.removedCount,
          bytes: result.removedBytes,
        }),
      );
    },
    onError: (error: unknown) => {
      toast.error(`${t("清理失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const isMutating =
    saveConfigMutation.isPending ||
    applyDirectMutation.isPending ||
    applyGatewayMutation.isPending ||
    restoreMutation.isPending ||
    repairHistoryMutation.isPending ||
    pruneHistoryBackupsMutation.isPending;

  const latestHistoryRepair =
    repairHistoryMutation.data ||
    applyDirectMutation.data?.historyRepair ||
    applyGatewayMutation.data?.historyRepair ||
    status?.historyRepair ||
    null;

  return {
    serviceStatus,
    mode,
    isServiceReady,
    statusQuery,
    candidatesQuery,
    status,
    candidates,
    codexHomeInput,
    selectedAccountId,
    selectedApiKeyId,
    gatewayBaseUrl,
    defaultGatewayBaseUrl,
    isDirectActive,
    isGatewayActive,
    activeAccountValue,
    activeKeyValue,
    setCodexHomeDraft,
    setSelectedAccountIdDraft,
    setSelectedApiKeyIdDraft,
    setGatewayBaseUrlDraft,
    refreshAll,
    saveConfigMutation,
    applyDirectMutation,
    applyGatewayMutation,
    restoreMutation,
    repairHistoryMutation,
    pruneHistoryBackupsMutation,
    isMutating,
    latestHistoryRepair,
  };
}
