"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database } from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { accountClient } from "@/lib/api/account-client";
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
import { RequestLogsTabContent } from "./page-sections";
import {
  buildFixedTimePreset,
  LogsPageSkeleton,
  type LogsTab,
  type StatusFilter,
  type TimeRangePreset,
  fromDateTimeLocalValue,
} from "./page-helpers";
import { buildSummaryPlaceholder } from "./page-cells";
import { AccountListResult, ApiKey, RequestLogListResult, StartupSnapshot } from "@/types";

function LogsPageContent() {
  const { t } = useI18n();
  const localDayRange = useLocalDayRange();
  const searchParams = useSearchParams();
  const { serviceStatus } = useAppStore();
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
  const [search, setSearch] = useState(routeQuery);
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [timePreset, setTimePreset] = useState<TimeRangePreset>("all");
  const [startTimeInput, setStartTimeInput] = useState("");
  const [endTimeInput, setEndTimeInput] = useState("");
  const [pageSize, setPageSize] = useState("10");
  const [page, setPage] = useState(1);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const [activeTab, setActiveTab] = useState<LogsTab>("requests");
  const pageSizeNumber = Number(pageSize) || 10;
  const startTs = useMemo(
    () => fromDateTimeLocalValue(startTimeInput),
    [startTimeInput],
  );
  const endTs = useMemo(() => fromDateTimeLocalValue(endTimeInput), [endTimeInput]);
  const hasActiveTimeRange = startTs != null || endTs != null;
  const startupSnapshot = queryClient.getQueryData<StartupSnapshot>(
    buildStartupSnapshotQueryKey(
      serviceStatus.addr,
      STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
      localDayRange.dayStartTs,
    )
  );
  const startupAccounts = startupSnapshot?.accounts || [];
  const startupApiKeys = startupSnapshot?.apiKeys || [];
  const startupRequestLogs = startupSnapshot?.requestLogs || [];
  const canUseStartupLogsPlaceholder =
    !routeQuery.trim() &&
    !search.trim() &&
    filter === "all" &&
    page === 1 &&
    !hasActiveTimeRange;
  const hasStartupLogsSnapshot =
    canUseStartupLogsPlaceholder && startupRequestLogs.length > 0;

  const { data: accountsResult } = useQuery({
    queryKey: ["accounts", "lookup"],
    queryFn: () => accountClient.list(),
    enabled: areLogQueriesEnabled && isPageActive && isAdminMode,
    staleTime: 60_000,
    retry: 1,
    placeholderData: (previousData): AccountListResult | undefined =>
      previousData ||
      (startupAccounts.length > 0
        ? {
            items: startupAccounts,
            total: startupAccounts.length,
            page: 1,
            pageSize: startupAccounts.length,
          }
        : undefined),
  });

  const { data: apiKeysResult } = useQuery({
    queryKey: ["apikeys", "lookup"],
    queryFn: () => accountClient.listApiKeys(),
    enabled: areLogQueriesEnabled && isPageActive,
    staleTime: 60_000,
    retry: 1,
    placeholderData: (previousData): ApiKey[] | undefined =>
      previousData || (startupApiKeys.length > 0 ? startupApiKeys : undefined),
  });

  const { data: aggregateApisResult } = useQuery({
    queryKey: ["aggregate-apis", "lookup"],
    queryFn: () => accountClient.listAggregateApis(),
    enabled: areLogQueriesEnabled && isPageActive && isAdminMode,
    staleTime: 60_000,
    retry: 1,
  });

  const { data: logsResult, isLoading, isError: isLogsError } = useQuery({
    queryKey: ["logs", "list", search, filter, startTs, endTs, page, pageSizeNumber],
    queryFn: () =>
      serviceClient.listRequestLogs({
        query: search,
        statusFilter: filter,
        startTs,
        endTs,
        page,
        pageSize: pageSizeNumber,
      }),
    enabled: areLogQueriesEnabled && isPageActive,
    refetchInterval: 5000,
    retry: 1,
    placeholderData: (previousData): RequestLogListResult | undefined =>
      previousData ||
      (hasStartupLogsSnapshot
        ? {
            items: startupRequestLogs,
            total: startupRequestLogs.length,
            page: 1,
            pageSize: pageSizeNumber,
          }
        : undefined),
  });

  const { data: summaryResult, isError: isSummaryError } = useQuery({
    queryKey: ["logs", "summary", search, filter, startTs, endTs],
    queryFn: () =>
      serviceClient.getRequestLogSummary({
        query: search,
        statusFilter: filter,
        startTs,
        endTs,
      }),
    enabled: areLogQueriesEnabled && isPageActive,
    refetchInterval: 5000,
    retry: 1,
    placeholderData: (previousData) =>
      previousData ||
      (canUseStartupLogsPlaceholder
        ? buildSummaryPlaceholder(startupRequestLogs)
        : undefined),
  });

  const clearMutation = useMutation({
    mutationFn: () => serviceClient.clearRequestLogs(),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["logs"] }),
        queryClient.invalidateQueries({ queryKey: ["today-summary"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      ]);
      toast.success(t("日志已清空"));
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
      (!isLogsLoading &&
        (Boolean(summaryResult) || isLogsError || isSummaryError)),
  );
  const currentPage = logsResult?.page || page;
  const summary = summaryResult || {
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

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    const frameId = window.requestAnimationFrame(() => {
      setSearch((current) => (current === routeQuery ? current : routeQuery));
      setPage(1);
    });
    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [routeQuery]);

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
    serviceStatus.connected ? t("5 秒刷新") : t("服务未连接")
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
    <div className="animate-in space-y-5 fade-in duration-500">
      <Tabs
        value={activeTab}
        onValueChange={(value) => {
          if (value === "requests") {
            setActiveTab("requests");
          }
        }}
        className="w-full"
      >
        <TabsList className="glass-card flex h-11 w-full justify-start overflow-x-auto rounded-xl p-1 no-scrollbar lg:w-fit">
          <TabsTrigger value="requests" className="gap-2 px-5 shrink-0">
            <Database className="h-4 w-4" /> {t("请求日志")}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="requests" className="space-y-5">
          <RequestLogsTabContent
            t={t}
            isDirectAccountMode={isDirectAccountMode}
            isAdminMode={isAdminMode}
            serviceConnected={serviceStatus.connected}
            search={search}
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
              setSearch(value);
              setPage(1);
            }}
            onFilterChange={(value) => {
              setFilter(value);
              setPage(1);
            }}
            onRefresh={() => {
              void queryClient.invalidateQueries({ queryKey: ["logs"] });
            }}
            onOpenClearConfirm={() => setClearConfirmOpen(true)}
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
            onPreviousPage={() => setPage(Math.max(1, currentPage - 1))}
            onNextPage={() => setPage(Math.min(totalPages, currentPage + 1))}
          />
        </TabsContent>

      </Tabs>

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
