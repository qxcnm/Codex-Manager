"use client";

import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { serviceClient } from "@/lib/api/service-client";
import { computeAvailablePoolRemain } from "@/lib/utils/pool-remain";
import { pickBestRecommendations, pickCurrentAccount } from "@/lib/utils/usage";

export function useDashboardStats() {
  const snapshotQuery = useQuery({
    queryKey: ["startup-snapshot", 120],
    queryFn: () => serviceClient.getStartupSnapshot({ requestLogLimit: 120 }),
    retry: 1,
  });

  const data = snapshotQuery.data;
  const accounts = data?.accounts || [];
  const poolRemain = useMemo(() => computeAvailablePoolRemain(accounts), [accounts]);
  const totalAccounts = accounts.length;
  const availableAccounts = accounts.filter((item) => item.availabilityKind === "available").length;
  const unavailableAccounts = accounts.filter((item) => item.availabilityKind === "unavailable").length;
  const expiredAccounts = accounts.filter((item) => item.availabilityKind === "expired").length;
  const currentAccount = pickCurrentAccount(
    accounts,
    data?.requestLogs || [],
    data?.manualPreferredAccountId
  );
  const recommendations = pickBestRecommendations(accounts);

  return {
    stats: {
      total: totalAccounts,
      available: availableAccounts,
      unavailable: unavailableAccounts,
      expired: expiredAccounts,
      todayTokens: data?.requestLogTodaySummary.todayTokens || 0,
      cachedTokens: data?.requestLogTodaySummary.cachedInputTokens || 0,
      reasoningTokens: data?.requestLogTodaySummary.reasoningOutputTokens || 0,
      todayCost: data?.requestLogTodaySummary.estimatedCost || 0,
      poolRemain: {
        primary: poolRemain.primaryRemainPercent,
        secondary: poolRemain.secondaryRemainPercent,
        primaryKnownCount: poolRemain.primaryKnownCount,
        primaryBucketCount: poolRemain.primaryBucketCount,
        secondaryKnownCount: poolRemain.secondaryKnownCount,
        secondaryBucketCount: poolRemain.secondaryBucketCount,
      },
    },
    currentAccount,
    recommendations,
    requestLogs: data?.requestLogs || [],
    isLoading: snapshotQuery.isLoading,
    isError: snapshotQuery.isError,
    error: snapshotQuery.error,
  };
}
