"use client";

import { AlertTriangle, CheckCircle2, Database, RefreshCw, Trash2, Zap } from "lucide-react";
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
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";
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
  return (
    <div className="space-y-5">
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
          <a
            href={buildStaticRouteUrl("/platform-mode")}
            className="inline-flex h-8 w-fit items-center justify-center rounded-lg border border-amber-500/40 bg-background/70 px-3 text-xs font-medium text-foreground transition-colors hover:bg-background"
          >
            {t("去切换为本地网关")}
          </a>
        </div>
      ) : null}

      <Card className="glass-card shadow-sm">
        <CardContent className="space-y-3 pt-0">
          <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_auto_auto] xl:items-center">
            <div className="min-w-0">
              <Input
                placeholder={t("搜索路径、账号或密钥 ID...")}
                className="glass-card h-10 rounded-xl px-3"
                value={search}
                onChange={(event) => onSearchChange(event.target.value)}
              />
            </div>
            <div className="flex shrink-0 items-center gap-1 rounded-xl border border-border/60 bg-muted/30 p-1">
              {["all", "2xx", "4xx", "5xx"].map((item) => (
                <Button
                  key={item}
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => onFilterChange(item as StatusFilter)}
                  className={cn(
                    "h-auto rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-wide transition-all",
                    filter === item
                      ? "bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:bg-background/60 hover:text-foreground",
                  )}
                >
                  {item.toUpperCase()}
                </Button>
              ))}
            </div>
            <div className="flex shrink-0 items-center gap-2 xl:justify-self-end">
              <Button
                variant="outline"
                size="sm"
                className="glass-card h-9 rounded-xl px-3.5"
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

          <div className="grid gap-3 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] xl:items-end">
            <div className="space-y-2">
              <div className="text-[11px] font-medium text-muted-foreground">
                {t("快捷时间")}
              </div>
              <div className="flex flex-wrap items-center gap-1 rounded-xl border border-border/60 bg-muted/30 p-1">
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
                      "h-auto rounded-lg px-3 py-1.5 text-xs font-semibold transition-all",
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
              <div className="space-y-1">
                <div className="text-[11px] font-medium text-muted-foreground">
                  {t("开始时间")}
                </div>
                <Input
                  type="datetime-local"
                  className="glass-card h-10 rounded-xl px-3"
                  value={startTimeInput}
                  onChange={(event) => onStartTimeChange(event.target.value)}
                />
              </div>
              <div className="space-y-1">
                <div className="text-[11px] font-medium text-muted-foreground">
                  {t("结束时间")}
                </div>
                <Input
                  type="datetime-local"
                  className="glass-card h-10 rounded-xl px-3"
                  value={endTimeInput}
                  onChange={(event) => onEndTimeChange(event.target.value)}
                />
              </div>
            </div>

            <div className="text-[11px] text-muted-foreground xl:justify-self-end xl:text-right">
              <div className="font-medium text-foreground">{compactMetaText}</div>
              {hasActiveTimeRange ? (
                <Button
                  type="button"
                  variant="link"
                  className="mt-1 h-auto p-0 text-xs text-primary hover:underline"
                  onClick={onClearTimeRange}
                >
                  {t("清除时间筛选")}
                </Button>
              ) : null}
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
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

      <Card className="glass-card overflow-hidden gap-0 py-0 shadow-sm">
        <CardHeader className="flex min-h-1 items-center border-b border-border/40 bg-[var(--table-section-bg)] py-3">
          <div className="flex w-full flex-col gap-1 xl:flex-row xl:items-center xl:justify-between">
            <div>
              <CardTitle className="text-[15px] font-semibold">
                {t("请求明细 按")}{" "}
                <span className="font-medium text-foreground">{currentFilterLabel}</span>{" "}
                {t("展示")}
              </CardTitle>
            </div>
            <div className="text-xs text-muted-foreground"></div>
          </div>
        </CardHeader>
        <CardContent className="px-0">
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
        </CardContent>
      </Card>

      <div className="flex items-center justify-between px-2">
        <div className="text-xs text-muted-foreground">
          {t("共")} {summary.filteredCount} {t("条匹配日志")}
        </div>
        <div className="flex items-center gap-6">
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
