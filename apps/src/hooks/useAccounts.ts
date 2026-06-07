"use client";

import { useEffect, useMemo, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { accountClient } from "@/lib/api/account-client";
import { CODEX_PROFILE_CANDIDATES_QUERY_KEY } from "@/lib/api/codex-profile-client";
import { attachUsagesToAccounts } from "@/lib/api/normalize";
import { serviceClient } from "@/lib/api/service-client";
import {
  buildStartupSnapshotQueryKey,
  STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
  STARTUP_SNAPSHOT_STALE_TIME,
} from "@/lib/api/startup-snapshot";
import { getAppErrorMessage } from "@/lib/api/transport";
import { listenUsageRefreshCompleted } from "@/lib/api/usage-refresh-events";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useLocalDayRange } from "@/hooks/useLocalDayRange";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { Account, AccountListResult, AccountUsage, StartupSnapshot } from "@/types";

type ImportByDirectoryResult = Awaited<ReturnType<typeof accountClient.importByDirectory>>;
type ImportByFileResult = Awaited<ReturnType<typeof accountClient.importByFile>>;
type AccountExportPayload = Parameters<typeof accountClient.export>[0];
type ExportResult = Awaited<ReturnType<typeof accountClient.export>>;
type WarmupPayload = Parameters<typeof accountClient.warmup>[0];
type WarmupResult = Awaited<ReturnType<typeof accountClient.warmup>>;
type RefreshAllRtResult = Awaited<
  ReturnType<typeof accountClient.refreshAllChatgptAuthTokens>
>;
type DeleteAccountsByStatusesResult = Awaited<
  ReturnType<typeof accountClient.deleteByStatuses>
>;
type AccountSortUpdate = { accountId: string; sort: number };

/**
 * 函数 `isAccountRefreshBlocked`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - status: 参数 status
 *
 * # 返回
 * 返回函数执行结果
 */
function isAccountRefreshBlocked(status: string | null | undefined): boolean {
  return String(status || "").trim().toLowerCase() === "disabled";
}

/**
 * 函数 `buildImportSummaryMessage`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - result: 参数 result
 *
 * # 返回
 * 返回函数执行结果
 */
function buildImportSummaryMessage(result: ImportByDirectoryResult, t: (message: string, values?: Record<string, string | number>) => string): string {
  const total = Number(result?.total || 0);
  const created = Number(result?.created || 0);
  const updated = Number(result?.updated || 0);
  const failed = Number(result?.failed || 0);
  return t("导入完成：共{total}，新增{created}，更新{updated}，失败{failed}", {
    total,
    created,
    updated,
    failed,
  });
}

/**
 * 函数 `formatUsageRefreshErrorMessage`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - error: 参数 error
 *
 * # 返回
 * 返回函数执行结果
 */
function formatUsageRefreshErrorMessage(
  error: unknown,
  t: (message: string, values?: Record<string, string | number>) => string,
): string {
  const message = getAppErrorMessage(error);
  if (message.toLowerCase().includes("refresh token failed with status 401")) {
    return t("账号长期未登录，refresh 已过期，已改为不可用状态");
  }
  return message;
}

function getAccountsAutoRefreshIntervalMs(
  enabled: boolean,
  intervalSecs: number,
): number | false {
  if (!enabled) {
    return false;
  }
  if (typeof document !== "undefined" && document.visibilityState !== "visible") {
    return false;
  }
  return Math.max(1, intervalSecs) * 1000;
}

function getUsageListRefreshIntervalMs(
  enabled: boolean,
  intervalSecs: number,
): number | false {
  const intervalMs = getAccountsAutoRefreshIntervalMs(enabled, intervalSecs);
  if (!intervalMs) {
    return false;
  }
  return Math.min(5_000, intervalMs);
}

function buildUsageListFingerprint(usages: AccountUsage[]): string {
  if (usages.length === 0) {
    return "";
  }

  return usages
    .map((usage) =>
      [
        usage.accountId,
        usage.capturedAt ?? "",
        usage.usedPercent ?? "",
        usage.secondaryUsedPercent ?? "",
        usage.resetsAt ?? "",
        usage.secondaryResetsAt ?? "",
        usage.availabilityStatus ?? "",
        usage.creditsJson ?? "",
      ].join(":"),
    )
    .sort()
    .join("|");
}

function buildAccountListResultFromSnapshot(accounts: Account[]): AccountListResult | undefined {
  if (accounts.length === 0) {
    return undefined;
  }
  return {
    items: accounts,
    total: accounts.length,
    page: 1,
    pageSize: accounts.length,
  };
}

/**
 * 函数 `useAccounts`
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
export function useAccounts() {
  const queryClient = useQueryClient();
  const { t } = useI18n();
  const localDayRange = useLocalDayRange();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const backgroundTasks = useAppStore((state) => state.appSettings.backgroundTasks);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/accounts/");
  const areAccountQueriesEnabled = useDeferredDesktopActivation(
    isServiceReady && isPageActive,
  );
  const usageListRefreshIntervalMs = getUsageListRefreshIntervalMs(
    areAccountQueriesEnabled && backgroundTasks.usagePollingEnabled,
    backgroundTasks.usagePollIntervalSecs,
  );
  const usageListFingerprintRef = useRef<string | null>(null);
  const allowEmptyAccountListRef = useRef(false);
  const startupSnapshotQueryKey = buildStartupSnapshotQueryKey(
    serviceStatus.addr,
    STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
    localDayRange.dayStartTs,
  );
  const startupSnapshotQuery = useQuery({
    queryKey: startupSnapshotQueryKey,
    queryFn: () =>
      serviceClient.getStartupSnapshot({
        requestLogLimit: STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
        dayStartTs: localDayRange.dayStartTs,
        dayEndTs: localDayRange.dayEndTs,
      }),
    enabled: areAccountQueriesEnabled,
    retry: 1,
    staleTime: STARTUP_SNAPSHOT_STALE_TIME,
  });
  const startupSnapshot =
    startupSnapshotQuery.data ||
    queryClient.getQueryData<StartupSnapshot>(startupSnapshotQueryKey);
  const startupAccounts = startupSnapshot?.accounts || [];
  const startupUsages = startupSnapshot?.usageSnapshots || [];
  const hasStartupAccountSnapshot = startupAccounts.length > 0;
  const startupAccountList = useMemo(
    () => buildAccountListResultFromSnapshot(startupAccounts),
    [startupAccounts],
  );

  /**
   * 函数 `ensureServiceReady`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - actionLabel: 参数 actionLabel
   *
   * # 返回
   * 返回函数执行结果
   */
  const ensureServiceReady = (actionLabel: string): boolean => {
    if (isServiceReady) {
      return true;
    }
    toast.info(`${t("服务未连接，暂时无法")} ${t(actionLabel)}`);
    return false;
  };

  // 账号实体列表只在显式账号操作/手动刷新时更新；用量轮询通过 usage/list 合并展示，避免临时空读覆盖账号池。
  const accountsQuery = useQuery({
    queryKey: ["accounts", "list"],
    queryFn: async () => {
      const data = await accountClient.list();
      if (data.items.length > 0) {
        allowEmptyAccountListRef.current = false;
        return data;
      }
      if (allowEmptyAccountListRef.current) {
        allowEmptyAccountListRef.current = false;
        return data;
      }
      if (
        startupAccountList &&
        startupAccountList.items.length > 0
      ) {
        console.warn(
          "account/list returned empty while startup snapshot still has accounts; keeping startup account list",
          {
            startupCount: startupAccountList.items.length,
            startupTotal: startupAccountList.total,
          },
        );
        return startupAccountList;
      }
      return data;
    },
    enabled: areAccountQueriesEnabled,
    retry: 1,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    initialData: () =>
      queryClient.getQueryData<AccountListResult>(["accounts", "list"]) ||
      startupAccountList,
    placeholderData: (previousData): AccountListResult | undefined =>
      previousData || startupAccountList,
  });

  const usagesQuery = useQuery({
    queryKey: ["usage", "list"],
    queryFn: () => accountClient.listUsage(),
    enabled: areAccountQueriesEnabled,
    retry: 1,
    refetchInterval: usageListRefreshIntervalMs,
    refetchIntervalInBackground: false,
    placeholderData: (previousData) =>
      previousData || (startupUsages.length > 0 ? startupUsages : undefined),
  });

  const usageListFingerprint = useMemo(
    () => buildUsageListFingerprint(usagesQuery.data || []),
    [usagesQuery.data],
  );

  useEffect(() => {
    if (!areAccountQueriesEnabled) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | null = null;
    const refreshVisibleUsageData = () => {
      void Promise.all([
        queryClient.refetchQueries({ queryKey: ["usage", "list"], type: "active" }),
        queryClient.invalidateQueries({ queryKey: ["usage-aggregate"] }),
        queryClient.invalidateQueries({ queryKey: ["today-summary"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
        queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY }),
      ]);
    };

    void listenUsageRefreshCompleted(() => {
      refreshVisibleUsageData();
    }).then((cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      unlisten = cleanup;
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [areAccountQueriesEnabled, queryClient]);

  useEffect(() => {
    if (!areAccountQueriesEnabled) {
      usageListFingerprintRef.current = null;
      return;
    }

    if (!usagesQuery.isFetched) {
      return;
    }

    const previousFingerprint = usageListFingerprintRef.current;
    usageListFingerprintRef.current = usageListFingerprint;
    if (previousFingerprint == null || previousFingerprint === usageListFingerprint) {
      return;
    }

    void Promise.all([
      queryClient.invalidateQueries({ queryKey: ["usage-aggregate"] }),
      queryClient.invalidateQueries({ queryKey: ["today-summary"] }),
      queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY }),
    ]);
  }, [
    areAccountQueriesEnabled,
    queryClient,
    usageListFingerprint,
    usagesQuery.isFetched,
  ]);

  const visibleAccountList = accountsQuery.data;

  const accounts = useMemo(() => {
    return attachUsagesToAccounts(
      visibleAccountList?.items || [],
      usagesQuery.data || []
    );
  }, [visibleAccountList?.items, usagesQuery.data]);

  const planTypes = useMemo(() => {
    const map = new Map<string, number>();
    const sortOrder = [
      "free",
      "go",
      "plus",
      "pro",
      "team",
      "business",
      "enterprise",
      "edu",
      "unknown",
    ];
    /**
     * 函数 `getSortIndex`
     *
     * 作者: gaohongshun
     *
     * 时间: 2026-04-02
     *
     * # 参数
     * - value: 参数 value
     *
     * # 返回
     * 返回函数执行结果
     */
    const getSortIndex = (value: string) => {
      const index = sortOrder.indexOf(value);
      return index === -1 ? sortOrder.length : index;
    };

    for (const account of accounts) {
      const planType = String(account.planType || "").trim().toLowerCase() || "unknown";
      map.set(planType, (map.get(planType) || 0) + 1);
    }

    return Array.from(map.entries())
      .sort((left, right) => {
        const sortDiff = getSortIndex(left[0]) - getSortIndex(right[0]);
        if (sortDiff !== 0) {
          return sortDiff;
        }
        return left[0].localeCompare(right[0], "zh-Hans-CN");
      })
      .map(([value, count]) => ({ value, count }));
  }, [accounts]);

  /**
   * 函数 `invalidateUsageData`
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
  const invalidateUsageData = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["usage"] }),
      queryClient.invalidateQueries({ queryKey: ["usage-aggregate"] }),
      queryClient.invalidateQueries({ queryKey: ["today-summary"] }),
      queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      queryClient.invalidateQueries({ queryKey: ["logs"] }),
      queryClient.invalidateQueries({ queryKey: CODEX_PROFILE_CANDIDATES_QUERY_KEY }),
    ]);
  };

  const invalidateAccountData = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["accounts", "list"] }),
      invalidateUsageData(),
    ]);
  };

  const allowExplicitEmptyAccountList = () => {
    allowEmptyAccountListRef.current = true;
  };

  const refreshAccountMutation = useMutation({
    mutationFn: (accountId: string) => accountClient.refreshUsage(accountId),
    onSuccess: () => {
      toast.success(t("账号用量已刷新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("刷新失败")}: ${formatUsageRefreshErrorMessage(error, t)}`);
    },
    onSettled: async () => {
      await invalidateUsageData();
    },
  });

  const refreshAllMutation = useMutation({
    mutationFn: () => accountClient.refreshUsage(),
    onSuccess: () => {
      toast.success(t("账号用量已刷新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("刷新失败")}: ${formatUsageRefreshErrorMessage(error, t)}`);
    },
    onSettled: async () => {
      await invalidateUsageData();
    },
  });

  const refreshAccountRtMutation = useMutation({
    mutationFn: (accountId: string) =>
      accountClient.refreshChatgptAuthTokens(accountId),
    onSuccess: () => {
      toast.success(t("账号 AT/RT 已刷新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("刷新 AT/RT 失败")}: ${getAppErrorMessage(error)}`);
    },
    onSettled: async () => {
      await invalidateAccountData();
    },
  });

  const refreshAllAccountRtMutation = useMutation({
    mutationFn: () => accountClient.refreshAllChatgptAuthTokens(),
    onSuccess: (result: RefreshAllRtResult) => {
      const succeeded = Number(result?.succeeded || 0);
      const failed = Number(result?.failed || 0);
      const skipped = Number(result?.skipped || 0);
      if (failed > 0) {
        const firstFailure = (result?.results || []).find((item) => !item.ok);
        toast.warning(
          firstFailure?.message
            ? t("AT/RT 刷新完成：成功{success}个，失败{failed}个，跳过{skipped}个；首个失败：{message}", {
                success: succeeded,
                failed,
                skipped,
                message: firstFailure.message,
              })
            : t("AT/RT 刷新完成：成功{success}个，失败{failed}个，跳过{skipped}个", {
                success: succeeded,
                failed,
                skipped,
              }),
        );
        return;
      }
      toast.success(
        t("AT/RT 刷新完成：成功{success}个，跳过{skipped}个", {
          success: succeeded,
          skipped,
        }),
      );
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量刷新 AT/RT 失败")}: ${getAppErrorMessage(error)}`);
    },
    onSettled: async () => {
      await invalidateAccountData();
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (accountId: string) => accountClient.delete(accountId),
    onSuccess: async () => {
      allowExplicitEmptyAccountList();
      await invalidateAccountData();
      toast.success(t("账号已删除"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("删除失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const deleteManyMutation = useMutation({
    mutationFn: (accountIds: string[]) => accountClient.deleteMany(accountIds),
    onSuccess: async (_result, accountIds) => {
      allowExplicitEmptyAccountList();
      await invalidateAccountData();
      toast.success(t("已删除 {count} 个账号", { count: accountIds.length }));
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量删除失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const deleteByStatusesMutation = useMutation({
    mutationFn: (statuses: string[]) => accountClient.deleteByStatuses({ statuses }),
    onSuccess: async (result: DeleteAccountsByStatusesResult) => {
      const deleted = Number(result?.deleted || 0);
      if (deleted > 0) {
        allowExplicitEmptyAccountList();
      }
      await invalidateAccountData();
      if (deleted > 0) {
        toast.success(t("已清理 {count} 个账号", { count: deleted }));
      } else {
        toast.success(t("未发现可清理的账号"));
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("清理失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const updateAccountSortMutation = useMutation({
    mutationFn: ({ accountId, sort }: { accountId: string; sort: number }) =>
      accountClient.updateSort(accountId, sort),
    onSuccess: async () => {
      await invalidateAccountData();
      toast.success(t("账号顺序已更新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新顺序失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const reorderAccountsMutation = useMutation({
    mutationFn: async (updates: AccountSortUpdate[]) => {
      for (const update of updates) {
        await accountClient.updateSort(update.accountId, update.sort);
      }
      return updates.length;
    },
    onSuccess: async (count) => {
      await invalidateAccountData();
      toast.success(
        count > 1
          ? t("账号顺序已调整（{count} 项）", { count })
          : t("账号顺序已更新"),
      );
    },
    onError: async (error: unknown) => {
      await invalidateAccountData();
      toast.error(`${t("调整账号顺序失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const updateAccountProfileMutation = useMutation({
    mutationFn: ({
      accountId,
      label,
      note,
      tags,
      sort,
      modelSlugs,
      quotaCapacityPrimaryWindowTokens,
      quotaCapacitySecondaryWindowTokens,
    }: {
      accountId: string;
      label?: string | null;
      note?: string | null;
      tags?: string[] | string | null;
      sort?: number | null;
      modelSlugs?: string[] | null;
      quotaCapacityPrimaryWindowTokens?: number | null;
      quotaCapacitySecondaryWindowTokens?: number | null;
    }) =>
      accountClient.updateProfile(accountId, {
        label,
        note,
        tags,
        sort,
        modelSlugs,
        quotaCapacityPrimaryWindowTokens,
        quotaCapacitySecondaryWindowTokens,
      }),
    onSuccess: async () => {
      await invalidateAccountData();
      toast.success(t("账号信息已更新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新账号信息失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const toggleAccountStatusMutation = useMutation({
    mutationFn: ({
      accountId,
      enabled,
    }: {
      accountId: string;
      enabled: boolean;
      sourceStatus?: string | null;
    }) =>
      enabled
        ? accountClient.enableAccount(accountId)
        : accountClient.disableAccount(accountId),
    onSuccess: async (_result, variables) => {
      await invalidateAccountData();
      const normalizedSourceStatus = String(variables.sourceStatus || "")
        .trim()
        .toLowerCase();
      toast.success(
        variables.enabled
          ? normalizedSourceStatus === "inactive"
            ? t("账号已恢复")
            : t("账号已启用")
          : t("账号已禁用")
      );
    },
    onError: (error: unknown, variables) => {
      const normalizedSourceStatus = String(variables.sourceStatus || "")
        .trim()
        .toLowerCase();
      const actionLabel = variables.enabled
        ? normalizedSourceStatus === "inactive"
          ? t("恢复")
          : t("启用")
        : t("禁用");
      toast.error(
        t("账号{action}失败: {error}", {
          action: actionLabel,
          error: getAppErrorMessage(error),
        })
      );
    },
  });

  const importByDirectoryMutation = useMutation({
    mutationFn: () => accountClient.importByDirectory(),
    onSuccess: async (result: ImportByDirectoryResult) => {
      if (result?.canceled) {
        toast.info(t("已取消导入"));
        return;
      }
      await invalidateAccountData();
      toast.success(buildImportSummaryMessage(result, t));
    },
    onError: (error: unknown) => {
      toast.error(`${t("导入失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const importByFileMutation = useMutation({
    mutationFn: () => accountClient.importByFile(),
    onSuccess: async (result: ImportByFileResult) => {
      if (result?.canceled) {
        toast.info(t("已取消导入"));
        return;
      }
      await invalidateAccountData();
      toast.success(buildImportSummaryMessage(result, t));
    },
    onError: (error: unknown) => {
      toast.error(`${t("导入失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const exportMutation = useMutation({
    mutationFn: (params?: AccountExportPayload) => accountClient.export(params),
    onSuccess: (result: ExportResult) => {
      if (result?.canceled) {
        toast.info(t("已取消导出"));
        return;
      }
      const exported = Number(result?.exported || 0);
      const outputDir = String(result?.outputDir || "").trim();
      const isBrowserDownload = outputDir === "browser-download";
      toast.success(
        isBrowserDownload
          ? t("已导出 {count} 个账号，浏览器将开始下载", { count: exported })
          : outputDir
          ? t("已导出 {count} 个账号到 {outputDir}", {
              count: exported,
              outputDir,
            })
          : t("已导出 {count} 个账号", { count: exported })
      );
    },
    onError: (error: unknown) => {
      toast.error(`${t("导出失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const warmupMutation = useMutation({
    mutationFn: (params?: WarmupPayload) => accountClient.warmup(params),
    onSuccess: async (result: WarmupResult) => {
      await invalidateUsageData();
      const requested = Number(result?.requested || 0);
      const succeeded = Number(result?.succeeded || 0);
      const failed = Number(result?.failed || 0);
      const firstFailedItem = (result?.results || []).find((item) => !item.ok);
      if (requested <= 0) {
        toast.info(t("当前没有可预热的账号"));
        return;
      }
      if (failed <= 0) {
        toast.success(t("预热完成：共{requested}个账号，成功{count}个", {
          requested,
          count: succeeded,
        }));
        return;
      }
      const summary = t("预热完成：成功{success}个，失败{failed}个", {
        success: succeeded,
        failed,
      });
      toast.warning(
        firstFailedItem?.message
          ? `${summary}；${t("首个失败")}: ${firstFailedItem.accountName || firstFailedItem.accountId} - ${firstFailedItem.message}`
          : summary,
      );
    },
    onError: (error: unknown) => {
      toast.error(`${t("账号预热失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const setPreferredMutation = useMutation({
    mutationFn: (accountId: string) => accountClient.setPreferred(accountId),
    onSuccess: async () => {
      await invalidateAccountData();
      toast.success(t("已设为优先账号"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("设置优先账号失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const clearPreferredMutation = useMutation({
    mutationFn: (accountId: string) => accountClient.clearPreferred(accountId),
    onSuccess: async () => {
      await invalidateAccountData();
      toast.success(t("已取消优先账号"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("取消优先账号失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  return {
    accounts,
    planTypes,
    total: visibleAccountList?.total || accounts.length,
    isLoading:
      isServiceReady &&
      !hasStartupAccountSnapshot &&
      (!areAccountQueriesEnabled || accountsQuery.isLoading || usagesQuery.isLoading),
    isServiceReady,
    refreshAccount: (accountId: string) => {
      if (!ensureServiceReady("刷新账号")) return;
      const targetAccountId = accountId.trim();
      if (!targetAccountId) {
        toast.error(t("未找到当前账号，请刷新后重试"));
        return;
      }
      refreshAccountMutation.mutate(targetAccountId);
    },
    refreshAccountRt: (accountId: string) => {
      if (!ensureServiceReady("刷新 AT/RT")) return;
      const targetAccountId = accountId.trim();
      if (!targetAccountId) {
        toast.error(t("未找到当前账号，请刷新后重试"));
        return;
      }
      refreshAccountRtMutation.mutate(targetAccountId);
    },
    refreshAllAccountRt: () => {
      if (!ensureServiceReady("刷新 AT/RT")) return;
      if (!accounts.length) {
        toast.info(t("当前没有可刷新的账号"));
        return;
      }
      refreshAllAccountRtMutation.mutate();
    },
    refreshAllAccounts: () => {
      if (!ensureServiceReady("刷新账号")) return;
      if (!accounts.some((account) => !isAccountRefreshBlocked(account.status))) {
        toast.info(t("当前没有可刷新的账号"));
        return;
      }
      refreshAllMutation.mutate();
    },
    refreshAccountList: async () => {
      if (!ensureServiceReady("刷新账号列表")) return;
      await invalidateAccountData();
      toast.success(t("账号列表已刷新"));
    },
    deleteAccount: (accountId: string) => {
      if (!ensureServiceReady("删除账号")) return;
      deleteMutation.mutate(accountId);
    },
    deleteManyAccounts: (accountIds: string[]) => {
      if (!ensureServiceReady("批量删除账号")) return;
      deleteManyMutation.mutate(accountIds);
    },
    cleanupAccountsByStatuses: async (statuses: string[]) => {
      if (!ensureServiceReady("清理账号")) return;
      await deleteByStatusesMutation.mutateAsync(statuses);
    },
    importByFile: () => {
      if (!ensureServiceReady("导入账号")) return;
      importByFileMutation.mutate();
    },
    importByDirectory: () => {
      if (!ensureServiceReady("导入账号")) return;
      importByDirectoryMutation.mutate();
    },
    exportAccounts: async (params?: AccountExportPayload) => {
      if (!ensureServiceReady("导出账号")) return;
      await exportMutation.mutateAsync(params);
    },
    warmupAccounts: async (params?: WarmupPayload) => {
      if (!ensureServiceReady("账号预热")) return;
      return await warmupMutation.mutateAsync(params);
    },
    setPreferredAccount: (accountId: string) => {
      if (!ensureServiceReady("设置优先账号")) return;
      setPreferredMutation.mutate(accountId);
    },
    clearPreferredAccount: (accountId: string) => {
      if (!ensureServiceReady("取消优先账号")) return;
      clearPreferredMutation.mutate(accountId);
    },
    updateAccountSort: async (accountId: string, sort: number) => {
      if (!ensureServiceReady("更新账号顺序")) return;
      await updateAccountSortMutation.mutateAsync({ accountId, sort });
    },
    reorderAccounts: async (updates: AccountSortUpdate[]) => {
      if (!ensureServiceReady("调整账号顺序")) return;
      if (!updates.length) return;
      await reorderAccountsMutation.mutateAsync(updates);
    },
    updateAccountProfile: async (
      accountId: string,
      params: {
        label?: string | null;
        note?: string | null;
        tags?: string[] | string | null;
        sort?: number | null;
        modelSlugs?: string[] | null;
        quotaCapacityPrimaryWindowTokens?: number | null;
        quotaCapacitySecondaryWindowTokens?: number | null;
      }
    ) => {
      if (!ensureServiceReady("更新账号信息")) return;
      await updateAccountProfileMutation.mutateAsync({ accountId, ...params });
    },
    toggleAccountStatus: (
      accountId: string,
      enabled: boolean,
      sourceStatus?: string | null
    ) => {
      if (!ensureServiceReady(enabled ? "启用账号" : "禁用账号")) return;
      toggleAccountStatusMutation.mutate({ accountId, enabled, sourceStatus });
    },
    isRefreshingAccountId:
      refreshAccountMutation.isPending && typeof refreshAccountMutation.variables === "string"
        ? refreshAccountMutation.variables
        : "",
    isRefreshingRtAccountId:
      refreshAccountRtMutation.isPending &&
      typeof refreshAccountRtMutation.variables === "string"
        ? refreshAccountRtMutation.variables
        : "",
    isRefreshingAllRtAccounts: refreshAllAccountRtMutation.isPending,
    isRefreshingAllAccounts: refreshAllMutation.isPending,
    isExporting: exportMutation.isPending,
    isWarmingUpAccounts: warmupMutation.isPending,
    isDeletingMany: deleteManyMutation.isPending,
    isCleaningAccountsByStatus: deleteByStatusesMutation.isPending,
    isUpdatingPreferred:
      setPreferredMutation.isPending || clearPreferredMutation.isPending,
    isUpdatingSortAccountId:
      updateAccountSortMutation.isPending &&
      updateAccountSortMutation.variables &&
      typeof updateAccountSortMutation.variables === "object" &&
      "accountId" in updateAccountSortMutation.variables
        ? String(
            (updateAccountSortMutation.variables as { accountId?: unknown }).accountId || ""
          )
        : "",
    isReorderingAccounts: reorderAccountsMutation.isPending,
    isUpdatingProfileAccountId:
      updateAccountProfileMutation.isPending &&
      updateAccountProfileMutation.variables &&
      typeof updateAccountProfileMutation.variables === "object" &&
      "accountId" in updateAccountProfileMutation.variables
        ? String(
            (updateAccountProfileMutation.variables as { accountId?: unknown }).accountId || ""
          )
        : "",
    isUpdatingStatusAccountId:
      toggleAccountStatusMutation.isPending &&
      toggleAccountStatusMutation.variables &&
      typeof toggleAccountStatusMutation.variables === "object" &&
      "accountId" in toggleAccountStatusMutation.variables
        ? String(
            (toggleAccountStatusMutation.variables as { accountId?: unknown }).accountId || ""
          )
        : "",
  };
}
