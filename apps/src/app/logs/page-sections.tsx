"use client";

import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  Clock3,
  Database,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Trash2,
  Zap,
} from "lucide-react";
import { useState } from "react";
import { ShellLink } from "@/components/layout/shell-link";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import { cn } from "@/lib/utils";
import {
  AccountKeyInfoCell,
  ErrorInfoCell,
  ModelEffortCell,
  RequestRouteInfoCell,
} from "./page-cells";
import {
  formatCompactTokenAmount,
  formatDuration,
  formatTableTokenAmount,
  getStatusBadge,
  type StatusFilter,
  type TimeRangePreset,
  type TranslateFn,
  resolveAccountDisplayName,
  resolveDisplayedStatusCode,
  SummaryCard,
} from "./page-helpers";
import type { AggregateApi, ApiKey, RequestLog, RequestLogFilterSummary } from "@/types";

export function RequestLogsTabContent({
  t,
  isDirectAccountMode,
  isAdminMode,
  serviceConnected,
  search,
  filter,
  timePreset,
  startTimeInput,
  endTimeInput,
  compactMetaText,
  hasActiveTimeRange,
  pageSize,
  currentFilterLabel,
  summary,
  logs,
  isLogsLoading,
  currentPage,
  totalPages,
  accountNameMap,
  apiKeyMap,
  aggregateApiMap,
  clearMutationPending,
  onSearchChange,
  onFilterChange,
  onRefresh,
  onOpenClearConfirm,
  onApplyTimePreset,
  onStartTimeChange,
  onEndTimeChange,
  onClearTimeRange,
  onPageSizeChange,
  onPreviousPage,
  onNextPage,
}: {
  t: TranslateFn;
  isDirectAccountMode: boolean;
  isAdminMode: boolean;
  serviceConnected: boolean;
  search: string;
  filter: StatusFilter;
  timePreset: TimeRangePreset;
  startTimeInput: string;
  endTimeInput: string;
  compactMetaText: string;
  hasActiveTimeRange: boolean;
  pageSize: string;
  currentFilterLabel: string;
  summary: RequestLogFilterSummary;
  logs: RequestLog[];
  isLogsLoading: boolean;
  currentPage: number;
  totalPages: number;
  accountNameMap: Map<string, string>;
  apiKeyMap: Map<string, ApiKey>;
  aggregateApiMap: Map<string, AggregateApi>;
  clearMutationPending: boolean;
  onSearchChange: (value: string) => void;
  onFilterChange: (value: StatusFilter) => void;
  onRefresh: () => void;
  onOpenClearConfirm: () => void;
  onApplyTimePreset: (preset: TimeRangePreset) => void;
  onStartTimeChange: (value: string) => void;
  onEndTimeChange: (value: string) => void;
  onClearTimeRange: () => void;
  onPageSizeChange: (value: string | null) => void;
  onPreviousPage: () => void;
  onNextPage: () => void;
}) {
  const [filtersExpanded, setFiltersExpanded] = useState(false);

  return (
    <div className="space-y-4">
      {isDirectAccountMode ? (
        <div className="flex flex-col gap-3 rounded-xl border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm sm:flex-row sm:items-center sm:justify-between">
          <div className="flex min-w-0 items-start gap-3">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-300" />
            <div>
              <div className="font-semibold text-amber-700 dark:text-amber-200">
                {t("账号直连模式不会产生新的 CodexManager 请求日志")}
              </div>
              <div className="mt-1 text-xs text-muted-foreground">
                {t("这里仅展示历史网关请求；如需记录请求，请切换到本地网关模式。")}
              </div>
            </div>
          </div>
          <ShellLink
            href="/platform-mode"
            className="inline-flex h-8 w-fit items-center justify-center rounded-lg border border-amber-500/40 bg-background/70 px-3 text-xs font-medium text-foreground transition-colors hover:bg-background"
          >
            {t("去切换为本地网关")}
          </ShellLink>
        </div>
      ) : null}

      <Card className="glass-card mission-panel overflow-hidden gap-0 py-0 shadow-sm">
        <CardContent className="p-0">
          <div className={cn("grid", filtersExpanded ? "xl:grid-cols-[minmax(0,1fr)_390px]" : "")}>
            <div
              className={cn(
                "space-y-4 p-4",
                filtersExpanded ? "xl:border-r xl:border-border/50" : "",
              )}
            >
              <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                <div className="min-w-0 space-y-1">
                  <div className="text-[11px] font-semibold tracking-[0.16em] text-muted-foreground uppercase">
                    {t("实时网关观测")}
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <div className="text-lg font-semibold tracking-tight">
                      {t("请求日志")}
                    </div>
                    <span className="rounded-full border border-border/60 bg-background/70 px-2.5 py-1 text-[11px] font-medium text-muted-foreground">
                      {compactMetaText}
                    </span>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-9 rounded-xl bg-background/70 px-3.5"
                    onClick={onRefresh}
                  >
                    <RefreshCw className="mr-1.5 h-4 w-4" /> {t("刷新")}
                  </Button>
                  {isAdminMode ? (
                    <Button
                      variant="destructive"
                      size="sm"
                      className="h-9 rounded-xl px-3.5"
                      onClick={onOpenClearConfirm}
                      disabled={clearMutationPending}
                    >
                      <Trash2 className="mr-1.5 h-4 w-4" /> {t("清空日志")}
                    </Button>
                  ) : null}
                </div>
              </div>

              <div className="grid gap-3 2xl:grid-cols-[minmax(320px,1fr)_auto] 2xl:items-center">
                <div className="relative min-w-0">
                  <Search className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    placeholder={t("搜索路径、账号或密钥 ID...")}
                    className="h-11 rounded-xl border-border/70 bg-background/80 pr-3 pl-10 text-sm shadow-none"
                    value={search}
                    onChange={(event) => onSearchChange(event.target.value)}
                  />
                </div>

                <div className="flex flex-wrap items-center gap-2 2xl:justify-end">
                  <span className="inline-flex h-9 items-center rounded-xl border border-border/60 bg-background/70 px-3 text-xs font-medium text-muted-foreground">
                    {currentFilterLabel}
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-9 rounded-xl bg-background/70 px-3"
                    aria-expanded={filtersExpanded}
                    onClick={() => setFiltersExpanded((current) => !current)}
                  >
                    <SlidersHorizontal className="mr-1.5 h-4 w-4" />
                    {filtersExpanded ? t("收起筛选") : t("展开筛选")}
                    <ChevronDown
                      className={cn(
                        "ml-1.5 h-4 w-4 transition-transform",
                        filtersExpanded ? "rotate-180" : "rotate-0",
                      )}
                    />
                  </Button>
                </div>
              </div>

              {filtersExpanded ? (
                <div className="space-y-3 rounded-xl border border-border/50 bg-muted/20 p-3">
                  <div className="grid grid-cols-4 rounded-xl border border-border/60 bg-background/45 p-1 sm:w-fit sm:min-w-[304px]">
                    {[
                      ["all", "ALL"],
                      ["2xx", "2XX"],
                      ["4xx", "4XX"],
                      ["5xx", "5XX"],
                    ].map(([value, label]) => (
                      <Button
                        key={value}
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => onFilterChange(value as StatusFilter)}
                        className={cn(
                          "h-8 rounded-lg px-3 text-xs font-semibold tracking-wide transition-all",
                          filter === value
                            ? "bg-background text-foreground shadow-sm"
                            : "text-muted-foreground hover:bg-background/60 hover:text-foreground",
                        )}
                      >
                        {label}
                      </Button>
                    ))}
                  </div>

                  <div className="grid gap-3 2xl:grid-cols-[minmax(0,1fr)_minmax(320px,0.72fr)] 2xl:items-center">
                    <div className="flex min-w-0 items-center gap-2 rounded-xl border border-border/60 bg-background/45 p-1">
                      <Clock3 className="ml-2 size-3.5 shrink-0 text-muted-foreground" />
                      <div className="flex min-w-0 flex-wrap items-center gap-1">
                        {(
                          [
                            ["all", t("全部时间")],
                            ["30m", t("最近30分钟")],
                            ["2h", t("最近2小时")],
                            ["24h", t("最近24小时")],
                            ["today", t("今天")],
                          ] as Array<[TimeRangePreset, string]>
                        ).map(([value, label]) => (
                          <Button
                            key={value}
                            type="button"
                            variant="ghost"
                            size="sm"
                            onClick={() => onApplyTimePreset(value)}
                            className={cn(
                              "h-8 rounded-lg px-3 text-xs font-semibold transition-all",
                              timePreset === value
                                ? "bg-background text-foreground shadow-sm"
                                : "text-muted-foreground hover:bg-background/60 hover:text-foreground",
                            )}
                          >
                            {label}
                          </Button>
                        ))}
                      </div>
                    </div>

                    <div className="grid gap-2 sm:grid-cols-2">
                      <Input
                        aria-label={t("开始时间")}
                        type="datetime-local"
                        className="h-10 rounded-xl border-border/70 bg-background/80 px-3 shadow-none"
                        value={startTimeInput}
                        onChange={(event) => onStartTimeChange(event.target.value)}
                      />
                      <Input
                        aria-label={t("结束时间")}
                        type="datetime-local"
                        className="h-10 rounded-xl border-border/70 bg-background/80 px-3 shadow-none"
                        value={endTimeInput}
                        onChange={(event) => onEndTimeChange(event.target.value)}
                      />
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border/50 pt-2 text-[11px] text-muted-foreground">
                    <div>
                      {t("当前视图")} · {currentFilterLabel}
                    </div>
                    {hasActiveTimeRange ? (
                      <Button
                        type="button"
                        variant="link"
                        className="h-auto p-0 text-xs text-primary hover:underline"
                        onClick={onClearTimeRange}
                      >
                        {t("清除时间筛选")}
                      </Button>
                    ) : null}
                  </div>
                </div>
              ) : hasActiveTimeRange ? (
                <div className="flex justify-end border-t border-border/50 pt-2">
                  <Button
                    type="button"
                    variant="link"
                    className="h-auto p-0 text-xs text-primary hover:underline"
                    onClick={onClearTimeRange}
                  >
                    {t("清除时间筛选")}
                  </Button>
                </div>
              ) : null}
            </div>

            {filtersExpanded ? (
              <div className="grid gap-3 border-t border-border/50 bg-muted/20 p-4 sm:grid-cols-2 xl:border-t-0">
                <SummaryCard
                  title={t("当前结果")}
                  value={`${summary.filteredCount}`}
                  description={`${t("总日志")} ${summary.totalCount} ${t("条")}${isDirectAccountMode ? ` · ${t("仅网关流量")}` : ""}`}
                  icon={Zap}
                  toneClass="bg-primary/12 text-primary"
                />
                <SummaryCard
                  title={t("2XX 成功")}
                  value={`${summary.successCount}`}
                  description={
                    isDirectAccountMode
                      ? `${t("状态码 200-299")} · ${t("仅网关流量")}`
                      : t("状态码 200-299")
                  }
                  icon={CheckCircle2}
                  toneClass="bg-green-500/12 text-green-500"
                />
                <SummaryCard
                  title={t("异常请求")}
                  value={`${summary.errorCount}`}
                  description={
                    isDirectAccountMode
                      ? `${t("4xx / 5xx 或显式错误")} · ${t("仅网关流量")}`
                      : t("4xx / 5xx 或显式错误")
                  }
                  icon={AlertTriangle}
                  toneClass="bg-red-500/12 text-red-500"
                />
                <SummaryCard
                  title={t("累计Token")}
                  value={formatCompactTokenAmount(summary.totalTokens)}
                  description={
                    isDirectAccountMode
                      ? `${t("当前筛选结果中的总Token")} · ${t("仅网关流量")}`
                      : t("当前筛选结果中的总Token")
                  }
                  icon={Database}
                  toneClass="bg-amber-500/12 text-amber-500"
                />
              </div>
            ) : null}
          </div>
        </CardContent>
      </Card>

      <Card className="glass-card mission-panel overflow-hidden gap-0 py-0 shadow-sm">
        <CardHeader className="flex min-h-1 items-center border-b border-border/40 bg-[var(--table-section-bg)] py-3">
          <div className="flex w-full flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
            <div className="min-w-0">
              <CardTitle className="text-[15px] font-semibold">
                {t("请求明细")}
              </CardTitle>
              <div className="mt-1 truncate text-xs text-muted-foreground">
                {compactMetaText}
              </div>
            </div>
            <div className="flex items-center gap-3">
              <div className="inline-flex w-fit items-center rounded-full border border-border/60 bg-background/70 px-3 py-1 text-xs font-medium text-muted-foreground">
                {currentFilterLabel}
              </div>
              <div className="hidden text-xs text-muted-foreground sm:block">
                {t("共")} {summary.filteredCount} {t("条匹配日志")}
              </div>
            </div>
          </div>
        </CardHeader>
        <CardContent className="px-0">
          <div className="overflow-x-auto">
            <Table className="min-w-[1500px] table-fixed">
              <TableHeader>
                <TableRow>
                  <TableHead className="h-12 w-[150px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("时间")}
                  </TableHead>
                  <TableHead className="w-[240px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("类型 / 方法 / 路径")}
                  </TableHead>
                  <TableHead className="w-[224px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("账号 / 密钥")}
                  </TableHead>
                  <TableHead className="w-[220px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("模型 / 推理 / 等级")}
                  </TableHead>
                  <TableHead className="w-[92px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("状态")}
                  </TableHead>
                  <TableHead className="w-[128px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("用时 / 首响")}
                  </TableHead>
                  <TableHead className="w-[148px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("Token")}
                  </TableHead>
                  <TableHead className="w-[240px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                    {t("错误")}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLogsLoading ? (
                  Array.from({ length: 10 }).map((_, index) => (
                    <TableRow key={index}>
                      <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-40" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                      <TableCell><Skeleton className="h-6 w-12 rounded-full" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-12" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-full" /></TableCell>
                    </TableRow>
                  ))
                ) : logs.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={8}
                      className="h-52 px-4 text-center text-sm text-muted-foreground"
                    >
                      {!serviceConnected
                        ? t("服务未连接，无法获取日志")
                        : isDirectAccountMode
                          ? t("账号直连模式下不会产生请求日志，如需记录请求请切换到本地网关模式。")
                          : t("暂无请求日志")}
                    </TableCell>
                  </TableRow>
                ) : (
                  logs.map((log) => (
                    <TableRow key={log.id} className="group text-xs hover:bg-muted/20">
                      <TableCell className="px-4 py-3 font-mono text-[11px] text-muted-foreground">
                        {formatTsFromSeconds(log.createdAt, t("未知时间"))}
                      </TableCell>
                      <TableCell className="px-4 py-3 align-top">
                        <RequestRouteInfoCell log={log} />
                      </TableCell>
                      <TableCell className="px-4 py-3 align-top">
                        <AccountKeyInfoCell
                          log={log}
                          accountLabel={resolveAccountDisplayName(log, accountNameMap)}
                          accountNameMap={accountNameMap}
                          apiKeyMap={apiKeyMap}
                          aggregateApiMap={aggregateApiMap}
                        />
                      </TableCell>
                      <TableCell className="px-4 py-3 align-top">
                        <ModelEffortCell log={log} />
                      </TableCell>
                      <TableCell className="px-4 py-3 align-top">
                        {getStatusBadge(resolveDisplayedStatusCode(log))}
                      </TableCell>
                      <TableCell className="px-4 py-3 align-top font-mono">
                        <span
                          className="text-xs text-primary"
                          title={t("首响表示从请求开始到首个上游响应片段的耗时")}
                        >
                          {formatDuration(log.durationMs)}/{formatDuration(log.firstResponseMs)}
                        </span>
                      </TableCell>
                      <TableCell className="px-4 py-3 align-top">
                        <div className="flex flex-col gap-0.5 text-[10px] text-muted-foreground">
                          <span>{t("总")} {formatTableTokenAmount(log.totalTokens)}</span>
                          <span>{t("输入")} {formatTableTokenAmount(log.inputTokens)}</span>
                          <span className="opacity-60">
                            {t("缓存")} {formatTableTokenAmount(log.cachedInputTokens)}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell className="px-4 py-3 text-left align-top">
                        <ErrorInfoCell error={log.error} />
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>
        </CardContent>
      </Card>

      <div className="flex flex-col gap-3 px-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="text-xs text-muted-foreground">
          {t("共")} {summary.filteredCount} {t("条匹配日志")}
        </div>
        <div className="flex flex-wrap items-center gap-4 sm:gap-6">
          <div className="flex items-center gap-2">
            <span className="whitespace-nowrap text-xs text-muted-foreground">
              {t("每页显示")}
            </span>
            <Select value={pageSize} onValueChange={onPageSizeChange}>
              <SelectTrigger className="h-8 w-[78px] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {["5", "10", "20", "50", "100", "200"].map((value) => (
                    <SelectItem key={value} value={value}>
                      {value}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
              disabled={currentPage <= 1}
              onClick={onPreviousPage}
            >
              {t("上一页")}
            </Button>
            <div className="min-w-[68px] text-center text-xs font-medium">
              {t("第")} {currentPage} / {totalPages} {t("页")}
            </div>
            <Button
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
              disabled={currentPage >= totalPages}
              onClick={onNextPage}
            >
              {t("下一页")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
