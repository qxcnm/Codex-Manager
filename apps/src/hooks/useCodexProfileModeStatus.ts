"use client";

import { useQuery } from "@tanstack/react-query";
import {
  CODEX_PROFILE_STATUS_QUERY_KEY,
  codexProfileClient,
} from "@/lib/api/codex-profile-client";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useAppStore } from "@/lib/store/useAppStore";
import type { CodexProfileMode } from "@/types";

export const CODEX_PROFILE_MODE_LABELS: Record<CodexProfileMode, string> = {
  missing: "未发现配置",
  unmanaged: "未托管",
  direct_account: "账号直连",
  gateway: "本地网关",
  managed_unknown: "托管状态未知",
};

interface UseCodexProfileModeStatusOptions {
  enabled?: boolean;
  refetchIntervalMs?: number | false;
}

export function useCodexProfileModeStatus(
  options: UseCodexProfileModeStatusOptions = {},
) {
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isQueryEnabled = (options.enabled ?? true) && isServiceReady;
  const statusQuery = useQuery({
    queryKey: CODEX_PROFILE_STATUS_QUERY_KEY,
    queryFn: () => codexProfileClient.get(),
    enabled: isQueryEnabled,
    retry: 1,
    staleTime: 5_000,
    refetchInterval: isQueryEnabled ? options.refetchIntervalMs ?? false : false,
    refetchIntervalInBackground: false,
  });
  const status = statusQuery.data;
  const mode = status?.mode ?? null;

  return {
    ...statusQuery,
    status,
    mode,
    modeLabel: mode ? CODEX_PROFILE_MODE_LABELS[mode] : "未知",
    isServiceReady,
    isDirectAccountMode: mode === "direct_account",
    isGatewayMode: mode === "gateway",
  };
}
