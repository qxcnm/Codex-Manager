"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { accountClient } from "@/lib/api/account-client";
import {
  buildAccountLookupQueryKey,
  buildApiKeyLookupQueryKey,
} from "@/lib/api/account-query-keys";
import {
  buildStartupSnapshotQueryKey,
  STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
} from "@/lib/api/startup-snapshot";
import { serviceClient } from "@/lib/api/service-client";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import {
  isAdminRole,
  resolveSessionRole,
  useAppSession,
} from "@/hooks/useAppSession";
import { useLocalDayRange } from "@/hooks/useLocalDayRange";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useCodexProfileModeStatus } from "@/hooks/useCodexProfileModeStatus";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { DASHBOARD_ADMIN_USAGE_QUERY_KEY } from "@/hooks/useDashboardAdminUsageSummary";
import { MEMBER_DASHBOARD_SUMMARY_QUERY_KEY } from "@/hooks/useMemberDashboardSummary";
import { RequestLogsTabContent } from "./page-sections";
import { RequestDetailModal } from "./request-detail-modal";
import {
  buildFixedTimePreset,
  LogsPageSkeleton,
  type StatusFilter,
  type TimeRangePreset,
  fromDateTimeLocalValue,
} from "./page-helpers";
import { buildSummaryPlaceholder } from "./page-cells";
import { AccountListResult, ApiKey, RequestLog, RequestLogListWithSummaryResult, StartupSnapshot } from "@/types";

const LOG_SEARCH_DEBOUNCE_MS = 300;
const LOG_REFRESH_ACTIVE_MS = 5_000;
const LOG_REFRESH_FILTERED_MS = 10_000;
const LOG_REFRESH_EMPTY_MS = 15_000;

function getLogRefreshIntervalMs(
  result: RequestLogListWithSummaryResult | undefined,
  hasActiveFilter: boolean,
): number {
  if (result && result.total === 0) {
    return LOG_REFRESH_EMPTY_MS;
  }
  return hasActiveFilter ? LOG_REFRESH_FILTERED_MS : LOG_REFRESH_ACTIVE_MS;
}

function LogsPageContent() {
  const { t } = useI18n();
  const localDayRange = useLocalDayRange();
  const searchParams = useSearchParams();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const serviceAddr = serviceStatus.addr || null;
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const isAdminMode = isAdminRole(role);
  const isPageActive = useDesktopPageActive("/logs/");
  const { isDirectAccountMode } = useCodexProfileModeStatus({
    enabled: isAdminMode && isPageActive,
    refetchIntervalMs: 10_000,
  });
  const queryClient = useQueryClient();
  const areLogQueriesEnabled = useDeferredDesktopActivation(serviceStatus.connected);
  const routeQuery = searchParams.get("query") || "";
  const [searchInput, setSearchInput] = useState(routeQuery);
  const [search, setSearch] = useState(routeQuery);
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [timePreset, setTimePreset] = useState<TimeRangePreset>("all");
  const [startTimeInput, setStartTimeInput] = useState("");
  const [endTimeInput, setEndTimeInput] = useState("");
  const [pageSize, setPageSize] = useState("10");
  const [page, setPage] = useState(1);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const [detailLog, setDetailLog] = useState<RequestLog | null>(null);
  const pageSizeNumber = Number(pageSize) || 10;
  const startTs = useMemo(
    () => fromDateTimeLocalValue(startTimeInput),
    [startTimeInput],
  );
  const endTs = useMemo(() => fromDateTimeLocalValue(endTimeInput), [endTimeInput]);
  const hasActiveTimeRange = startTs != null || endTs != null;
  const hasActiveLogFilter =
    Boolean(search.trim()) || filter !== "all" || hasActiveTimeRange;
  const startupSnapshot = queryClient.getQueryData<StartupSnapshot>(
    buildStartupSnapshotQueryKey(
      serviceAddr,
      STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
      localDayRange.dayStartTs,
      localDayRange.dayEndTs,
    )
  );
  const startupAccounts = startupSnapshot?.accounts || [];
  const startupApiKeys = startupSnapshot?.apiKeys || [];
  const startupRequestLogs = startupSnapshot?.requestLogs || [];
  const canUseStartupLogsPlaceholder =
    !routeQuery.trim() &&
    !searchInput.trim() &&
    !search.trim() &&
    filter === "all" &&
    page === 1 &&
    !hasActiveTimeRange;
  const hasStartupLogsSnapshot =
    canUseStartupLogsPlaceholder && startupRequestLogs.length > 0;

  const { data: accountsResult } = useQuery({
    queryKey: buildAccountLookupQueryKey(serviceAddr),
    queryFn: () => accountClient.list(serviceAddr),
    enabled: areLogQueriesEnabled && isPageActive && isAdminMode,
    staleTime: 60_000,
    retry: 1,
    placeholderData: (): AccountListResult | undefined =>
      startupAccounts.length > 0
        ? {
            items: startupAccounts,
            total: startupAccounts.length,
            page: 1,
            pageSize: startupAccounts.length,
          }
        : undefined,
  });

  const { data: apiKeysResult } = useQuery({
    queryKey: buildApiKeyLookupQueryKey(serviceAddr),
    queryFn: () => accountClient.listApiKeys(serviceAddr),
    enabled: areLogQueriesEnabled && isPageActive,
    staleTime: 60_000,
    retry: 1,
    placeholderData: (): ApiKey[] | undefined =>
      startupApiKeys.length > 0 ? startupApiKeys : undefined,
  });

  const { data: aggregateApisResult } = useQuery({
    queryKey: ["aggregate-apis", "lookup", serviceAddr],
    queryFn: () => accountClient.listAggregateApis(serviceAddr),
    enabled: areLogQueriesEnabled && isPageActive && isAdminMode,
    staleTime: 60_000,
    retry: 1,
  });

  const { data: logsResult, isLoading, isError: isLogsError } = useQuery({
    queryKey: [
      "logs",
      "list-with-summary",
      serviceAddr,
      search,
      filter,
      startTs,
      endTs,
      page,
      pageSizeNumber,
    ],
    queryFn: ({ signal }) =>
      serviceClient.listRequestLogsWithSummary(
        {
          query: search,
          statusFilter: filter,
          startTs,
          endTs,
          page,
          pageSize: pageSizeNumber,
        },
        { signal },
        serviceAddr,
      ),
    enabled: areLogQueriesEnabled && isPageActive,
    refetchInterval: (query) => {
      if (!areLogQueriesEnabled || !isPageActive) {
        return false;
      }
      if (typeof document !== "undefined" && document.visibilityState !== "visible") {
        return false;
      }
      return getLogRefreshIntervalMs(
        query.state.data as RequestLogListWithSummaryResult | undefined,
        hasActiveLogFilter,
      );
    },
    refetchIntervalInBackground: false,
    retry: 1,
    gcTime: 60_000,
    placeholderData: (
      previousData,
      previousQuery,
    ): RequestLogListWithSummaryResult | undefined => {
      if (previousQuery?.queryKey[2] === serviceAddr && previousData) {
        return previousData;
      }
      return hasStartupLogsSnapshot
        ? {
            items: startupRequestLogs,
            total: startupRequestLogs.length,
            page: 1,
            pageSize: pageSizeNumber,
            summary: buildSummaryPlaceholder(startupRequestLogs),
          }
        : undefined;
    },
  });

  const clearMutation = useMutation({
    mutationFn: () => serviceClient.clearRequestLogs(serviceAddr),
    onSuccess: async () => {
      queryClient.setQueriesData<RequestLogListWithSummaryResult>(
        { queryKey: ["logs", "list-with-summary", serviceAddr] },
        (previousData) =>
          previousData
            ? {
                ...previousData,
                items: [],
                total: 0,
                page: 1,
                summary: {
                  ...previousData.summary,
                  totalCount: 0,
                  filteredCount: 0,
                  successCount: 0,
                  errorCount: 0,
                  totalTokens: 0,
                  totalCostUsd: 0,
                },
              }
            : previousData,
      );
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ["logs", "list-with-summary", serviceAddr],
        }),
        queryClient.invalidateQueries({ queryKey: ["today-summary"] }),
        queryClient.invalidateQueries({ queryKey: DASHBOARD_ADMIN_USAGE_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: MEMBER_DASHBOARD_SUMMARY_QUERY_KEY }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      ]);
      toast.success(t("日志已清空"), {
        description: t(
          "磁盘空间不会立即变小，可在“设置 → 网关 → 数据库空间”中查看并回收。",
        ),
      });
    },
    onError: (error: unknown) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  const accountNameMap = useMemo(() => {
    return new Map(
      (accountsResult?.items || []).map((account) => [
        account.id,
        account.label || account.name || account.id,
      ]),
    );
  }, [accountsResult?.items]);

  const apiKeyMap = useMemo(() => {
    return new Map((apiKeysResult || []).map((apiKey) => [apiKey.id, apiKey]));
  }, [apiKeysResult]);

  const aggregateApiMap = useMemo(() => {
    return new Map(
      (aggregateApisResult || []).map((aggregateApi) => [
        aggregateApi.id,
        aggregateApi,
      ]),
    );
  }, [aggregateApisResult]);

  const logs = logsResult?.items || [];
  const isLogsLoading =
    serviceStatus.connected &&
    !hasStartupLogsSnapshot &&
    (!areLogQueriesEnabled || isLoading);
  usePageTransitionReady(
    "/logs/",
    !serviceStatus.connected ||
      (!isLogsLoading && (Boolean(logsResult?.summary) || isLogsError)),
  );
  const currentPage = logsResult?.page || page;
  const summary = logsResult?.summary || {
    totalCount: logsResult?.total || 0,
    filteredCount: logsResult?.total || 0,
    successCount: 0,
    errorCount: 0,
    totalTokens: 0,
    totalCostUsd: 0,
  };
  const totalPages = Math.max(
    1,
    Math.ceil((logsResult?.total || 0) / pageSizeNumber),
  );
  const currentRefreshIntervalMs = getLogRefreshIntervalMs(
    logsResult,
    hasActiveLogFilter,
  );

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    const frameId = window.requestAnimationFrame(() => {
      setSearchInput((current) => (current === routeQuery ? current : routeQuery));
      setSearch((current) => (current === routeQuery ? current : routeQuery));
      setPage(1);
    });
    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [routeQuery]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    const timeoutId = window.setTimeout(() => {
      setSearch((current) => (current === searchInput ? current : searchInput));
      setPage(1);
    }, LOG_SEARCH_DEBOUNCE_MS);
    return () => {
      window.clearTimeout(timeoutId);
    };
  }, [searchInput]);

  useEffect(() => {
    if (isPageActive) {
      return;
    }
    if (typeof window === "undefined") {
      return;
    }
    const frameId = window.requestAnimationFrame(() => {
      setClearConfirmOpen(false);
    });
    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [isPageActive]);

  useEffect(() => {
    if (timePreset !== "today") {
      return;
    }
    const frameId = window.requestAnimationFrame(() => {
      const todayRange = buildFixedTimePreset(
        "today",
        localDayRange.dayStartTs,
        localDayRange.dayEndTs,
      );
      setStartTimeInput((current) =>
        current === todayRange.startInput ? current : todayRange.startInput,
      );
      setEndTimeInput((current) =>
        current === todayRange.endInput ? current : todayRange.endInput,
      );
    });
    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [localDayRange.dayEndTs, localDayRange.dayStartTs, timePreset]);

  const currentFilterLabel =
    filter === "all"
      ? t("全部状态")
      : filter === "2xx"
        ? t("成功请求")
        : filter === "4xx"
          ? t("客户端错误")
          : t("服务端错误");
  const currentTimeRangeLabel =
    timePreset === "30m"
      ? t("最近30分钟")
      : timePreset === "2h"
        ? t("最近2小时")
        : timePreset === "24h"
          ? t("最近24小时")
          : timePreset === "today"
            ? t("今天")
            : hasActiveTimeRange
              ? t("自定义时间")
              : t("全部时间");
  const compactMetaText = `${summary.filteredCount}/${summary.totalCount} ${t("条")} · ${currentFilterLabel} · ${currentTimeRangeLabel} · ${
    serviceStatus.connected
      ? currentRefreshIntervalMs === LOG_REFRESH_ACTIVE_MS
        ? t("5 秒刷新")
        : currentRefreshIntervalMs === LOG_REFRESH_FILTERED_MS
          ? t("10 秒刷新")
          : t("15 秒刷新")
      : t("服务未连接")
  }`;

  const applyTimePreset = (preset: TimeRangePreset) => {
    setTimePreset(preset);
    setPage(1);
    if (preset === "all") {
      setStartTimeInput("");
      setEndTimeInput("");
      return;
    }
    if (preset === "custom") {
      return;
    }
    const nextRange = buildFixedTimePreset(
      preset,
      localDayRange.dayStartTs,
      localDayRange.dayEndTs,
    );
    setStartTimeInput(nextRange.startInput);
    setEndTimeInput(nextRange.endInput);
  };

  return (
    <div className="animate-in space-y-3 fade-in duration-500">
      <RequestLogsTabContent
        t={t}
        isDirectAccountMode={isDirectAccountMode}
        isAdminMode={isAdminMode}
        serviceConnected={serviceStatus.connected}
        search={searchInput}
        filter={filter}
        timePreset={timePreset}
        startTimeInput={startTimeInput}
        endTimeInput={endTimeInput}
        compactMetaText={compactMetaText}
        hasActiveTimeRange={hasActiveTimeRange}
        pageSize={pageSize}
        currentFilterLabel={currentFilterLabel}
        summary={summary}
        logs={logs}
        isLogsLoading={isLogsLoading}
        currentPage={currentPage}
        totalPages={totalPages}
        accountNameMap={accountNameMap}
        apiKeyMap={apiKeyMap}
        aggregateApiMap={aggregateApiMap}
        clearMutationPending={clearMutation.isPending}
        onSearchChange={(value) => {
          setSearchInput(value);
          setPage(1);
        }}
        onFilterChange={(value) => {
          setFilter(value);
          setPage(1);
        }}
        onRefresh={() => {
          void queryClient.invalidateQueries({
            queryKey: ["logs", "list-with-summary", serviceAddr],
          });
        }}
        onOpenClearConfirm={() => setClearConfirmOpen(true)}
        onOpenDetail={setDetailLog}
        onApplyTimePreset={applyTimePreset}
        onStartTimeChange={(value) => {
          setTimePreset("custom");
          setStartTimeInput(value);
          setPage(1);
        }}
        onEndTimeChange={(value) => {
          setTimePreset("custom");
          setEndTimeInput(value);
          setPage(1);
        }}
        onClearTimeRange={() => applyTimePreset("all")}
        onPageSizeChange={(value) => {
          setPageSize(value || "10");
          setPage(1);
        }}
        onFirstPage={() => setPage(1)}
        onPreviousPage={() => setPage(Math.max(1, currentPage - 1))}
        onNextPage={() => setPage(Math.min(totalPages, currentPage + 1))}
        onJumpPage={setPage}
      />
      {isAdminMode ? (
        <ConfirmDialog
          open={clearConfirmOpen}
          onOpenChange={setClearConfirmOpen}
          title={t("清空请求日志")}
          description={t("确定清空全部请求日志吗？该操作不可恢复。")}
          confirmText={t("清空")}
          confirmVariant="destructive"
          onConfirm={() => clearMutation.mutate()}
        />
      ) : null}
      <RequestDetailModal
        open={detailLog !== null}
        onOpenChange={(open) => {
          if (!open) setDetailLog(null);
        }}
        log={detailLog}
        serviceAddr={serviceAddr}
      />
    </div>
  );
}

export default function LogsPage() {
  return (
    <Suspense fallback={<LogsPageSkeleton />}>
      <LogsPageContent />
    </Suspense>
  );
}
