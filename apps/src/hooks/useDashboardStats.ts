"use client";

import { useEffect, useRef } from "react";
import { useQuery } from "@tanstack/react-query";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useLocalDayRange } from "@/hooks/useLocalDayRange";
import {
  buildStartupSnapshotQueryKey,
  hasStartupSnapshotSignal,
  STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
  STARTUP_SNAPSHOT_STALE_TIME,
  STARTUP_SNAPSHOT_WARMUP_INTERVAL_MS,
  STARTUP_SNAPSHOT_WARMUP_TIMEOUT_MS,
} from "@/lib/api/startup-snapshot";
import { serviceClient } from "@/lib/api/service-client";
import { useAppStore } from "@/lib/store/useAppStore";
import { pickBestRecommendations, pickCurrentAccount } from "@/lib/utils/usage";

interface UseDashboardStatsOptions {
  forceActive?: boolean;
  requestLogLimit?: number;
  includeAccountHints?: boolean;
  includeApiModels?: boolean;
  includeApiKeys?: boolean;
  includeAccounts?: boolean;
  includeUsageSnapshots?: boolean;
  includeAccountRuntime?: boolean;
  includeAccountDetails?: boolean;
}

/**
 * 函数 `useDashboardStats`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * 无
 *
 * # 返回
 * 返回函数执行结果
 */
export function useDashboardStats(options: UseDashboardStatsOptions = {}) {
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const localDayRange = useLocalDayRange();
  const isServiceReady = serviceStatus.connected;
  const defaultPageActive = useDesktopPageActive("/");
  const isPageActive = options.forceActive ?? defaultPageActive;
  const requestLogLimit =
    options.requestLogLimit ?? STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT;
  const includeAccountHints = options.includeAccountHints ?? true;
  const includeApiModels = options.includeApiModels ?? true;
  const includeApiKeys = options.includeApiKeys ?? true;
  const includeAccounts = options.includeAccounts ?? true;
  const includeUsageSnapshots = options.includeUsageSnapshots ?? true;
  const includeAccountRuntime = options.includeAccountRuntime ?? true;
  const includeAccountDetails = options.includeAccountDetails ?? true;
  const isSnapshotQueryEnabled = useDeferredDesktopActivation(
    isServiceReady && isPageActive,
  );
  const warmupStartedAtRef = useRef<number | null>(null);

  useEffect(() => {
    if (!isServiceReady || !isPageActive) {
      warmupStartedAtRef.current = null;
      return;
    }
    warmupStartedAtRef.current = Date.now();
  }, [isPageActive, isServiceReady, serviceStatus.addr]);

  const snapshotQuery = useQuery({
    queryKey: buildStartupSnapshotQueryKey(
      serviceStatus.addr,
      requestLogLimit,
      localDayRange.dayStartTs,
      localDayRange.dayEndTs,
      includeApiModels,
      includeApiKeys,
      includeAccounts,
      includeUsageSnapshots,
      includeAccountRuntime,
      includeAccountDetails,
    ),
    queryFn: () =>
      serviceClient.getStartupSnapshot({
        requestLogLimit,
        dayStartTs: localDayRange.dayStartTs,
        dayEndTs: localDayRange.dayEndTs,
        includeApiModels,
        includeApiKeys,
        includeAccounts,
        includeUsageSnapshots,
        includeAccountRuntime,
        includeAccountDetails,
      }),
    enabled: isSnapshotQueryEnabled,
    retry: 1,
    staleTime: STARTUP_SNAPSHOT_STALE_TIME,
    refetchInterval: (query) => {
      if (!isServiceReady || !isPageActive) return false;
      if (typeof document !== "undefined" && document.visibilityState !== "visible") {
        return false;
      }
      const startedAt = warmupStartedAtRef.current;
      if (startedAt != null) {
        if (Date.now() - startedAt >= STARTUP_SNAPSHOT_WARMUP_TIMEOUT_MS) {
          warmupStartedAtRef.current = null;
        } else {
          const snapshot = query.state.data;
          if (
            snapshot &&
            snapshot.accounts.length > 0 &&
            !hasStartupSnapshotSignal(snapshot)
          ) {
            return STARTUP_SNAPSHOT_WARMUP_INTERVAL_MS;
          }
        }
      }

      return STARTUP_SNAPSHOT_STALE_TIME;
    },
    refetchIntervalInBackground: false,
  });

  const data = snapshotQuery.data;
  const accounts = data?.accounts || [];
  const hasStartupSignal = hasStartupSnapshotSignal(data);
  const shouldWarmupPoll =
    isPageActive &&
    isServiceReady &&
    accounts.length > 0 &&
    !hasStartupSignal &&
    snapshotQuery.isFetching;
  const hasSnapshotData = Boolean(data);
  const accountSummary = data?.accountSummary;
  const totalAccounts = accountSummary?.accountCount ?? accounts.length;
  const availableAccounts =
    accountSummary?.availableCount ?? accounts.filter((item) => item.isAvailable).length;
  const unavailableAccounts = Math.max(totalAccounts - availableAccounts, 0);
  const currentAccount = includeAccountHints
    ? pickCurrentAccount(accounts, data?.requestLogs || [])
    : null;
  const recommendations = includeAccountHints
    ? pickBestRecommendations(accounts)
    : { primaryPick: null, secondaryPick: null };

  return {
    stats: {
      apiKeyCount: data?.apiKeys.length || 0,
      total: totalAccounts,
      available: availableAccounts,
      unavailable: unavailableAccounts,
      todayTokens: data?.requestLogTodaySummary.todayTokens || 0,
      cachedTokens: data?.requestLogTodaySummary.cachedInputTokens || 0,
      reasoningTokens: data?.requestLogTodaySummary.reasoningOutputTokens || 0,
      todayCost: data?.requestLogTodaySummary.estimatedCost || 0,
      poolRemain: {
        primary: data?.usageAggregateSummary.primaryRemainPercent ?? null,
        secondary: data?.usageAggregateSummary.secondaryRemainPercent ?? null,
        primaryKnownCount: data?.usageAggregateSummary.primaryKnownCount ?? 0,
        primaryBucketCount: data?.usageAggregateSummary.primaryBucketCount ?? 0,
        secondaryKnownCount: data?.usageAggregateSummary.secondaryKnownCount ?? 0,
        secondaryBucketCount: data?.usageAggregateSummary.secondaryBucketCount ?? 0,
      },
    },
    accounts,
    currentAccount,
    recommendations,
    requestLogs: data?.requestLogs || [],
    isLoading:
      (!isServiceReady && !hasSnapshotData) ||
      (!isSnapshotQueryEnabled && !data) ||
      snapshotQuery.isPending ||
      shouldWarmupPoll,
    isSyncingSnapshot: shouldWarmupPoll,
    isServiceReady,
    isError: snapshotQuery.isError,
    error: snapshotQuery.error,
  };
}
