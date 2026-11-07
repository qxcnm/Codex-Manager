"use client";

import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  Clock3,
  Database,
  List,
  Table2,
  Info,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Trash2,
  Zap,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { RequestLogTable } from "./request-log-table";
import { RequestLogTimeline } from "./request-log-timeline";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import {
  formatCompactTokenAmount,
  type StatusFilter,
  type TimeRangePreset,
  type TranslateFn,
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
  onOpenDetail,
  onApplyTimePreset,
  onStartTimeChange,
  onEndTimeChange,
  onClearTimeRange,
  onPageSizeChange,
  onFirstPage,
  onPreviousPage,
  onNextPage,
  onJumpPage,
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
  onOpenDetail: (log: RequestLog) => void;
  onApplyTimePreset: (preset: TimeRangePreset) => void;
  onStartTimeChange: (value: string) => void;
  onEndTimeChange: (value: string) => void;
  onClearTimeRange: () => void;
  onPageSizeChange: (value: string | null) => void;
  onFirstPage: () => void;
  onPreviousPage: () => void;
  onNextPage: () => void;
  onJumpPage: (page: number) => void;
}) {
  const [view, setView] = useState<"timeline" | "table">("timeline");
  const [filtersExpanded, setFiltersExpanded] = useState(false);
  const [jumpPageInput, setJumpPageInput] = useState(String(currentPage));

  useEffect(() => {
    setJumpPageInput(String(currentPage));
  }, [currentPage]);

  const submitJumpPage = () => {
    const parsedPage = jumpPageInput.trim() ? Number(jumpPageInput) : currentPage;
    const targetPage = Number.isInteger(parsedPage)
      ? Math.min(totalPages, Math.max(1, parsedPage))
      : currentPage;
    setJumpPageInput(String(targetPage));
    onJumpPage(targetPage);
  };

  return (
    <div className="space-y-3">
      {isDirectAccountMode ? (
        <div className="flex gap-3 rounded-xl border border-primary/25 bg-primary/8 px-4 py-3 text-sm">
          <div className="flex min-w-0 items-start gap-3">
            <Info className="mt-0.5 size-4 shrink-0 text-primary" />
            <div>
              <div className="font-semibold text-foreground">
                {t("本机 Codex 的账号直连请求不会写入此日志")}
              </div>
              <div className="mt-1 text-xs text-muted-foreground">
                {t("下方仍展示 CodexManager 已记录的网关请求，包括其他客户端通过平台密钥产生的流量。")}
              </div>
            </div>
          </div>
        </div>
      ) : null}

      <Card className="glass-card mission-panel overflow-hidden gap-0 py-0 shadow-none!">
        <CardContent className="p-0">
          <div className="min-w-0">
            <div
              className="space-y-4 p-4 sm:px-5"
            >
              <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                <div className="min-w-0 space-y-1">
                  <div className="text-xs text-muted-foreground">
                    {t("实时网关观测")}
                  </div>
                  <div className="flex flex-wrap items-center gap-2">
                    <div className="text-lg font-semibold tracking-tight">
                      {t("请求日志")}
                    </div>
                    <span className="text-[11px] leading-5 text-muted-foreground">
                      {compactMetaText}
                    </span>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    className="h-9 rounded-lg bg-background/50 px-3"
                    onClick={onRefresh}
                  >
                    <RefreshCw className="mr-1.5 h-4 w-4" /> {t("刷新")}
                  </Button>
                  {isAdminMode ? (
                    <Button
                      variant="outline"
                      size="sm"
                      className="h-9 rounded-lg border-destructive/20 bg-background/50 px-3 text-destructive hover:bg-destructive/10 hover:text-destructive"
                      onClick={onOpenClearConfirm}
                      disabled={clearMutationPending}
                    >
                      <Trash2 className="mr-1.5 h-4 w-4" /> {t("清空日志")}
                    </Button>
                  ) : null}
                </div>
              </div>

              <div className="grid gap-3 lg:grid-cols-[minmax(220px,1fr)_auto] lg:items-center">
                <div className="relative min-w-0">
                  <Search className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    placeholder={t("搜索路径、账号或密钥 ID...")}
                    className="h-10 rounded-lg border-border/60 bg-background/50 pr-3 pl-10 text-sm shadow-none"
                    aria-label={t("搜索路径、账号或密钥 ID...")}
                    value={search}
                    onChange={(event) => onSearchChange(event.target.value)}
                  />
                </div>

                <div className="flex flex-wrap items-center gap-2 lg:justify-end">
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
                        aria-pressed={filter === value}
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
                            aria-pressed={timePreset === value}
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
              <div className="grid gap-2 border-t border-border/50 bg-muted/15 p-3 sm:grid-cols-2 xl:grid-cols-4">
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

      <Card className="glass-card mission-panel overflow-hidden gap-0 py-0 shadow-none!">
        <CardHeader className="flex min-h-1 items-center border-b border-border/40 bg-[var(--table-section-bg)] py-3">
          <div className="flex w-full flex-wrap items-center justify-between gap-3">
            <div className="min-w-0">
              <CardTitle className="text-[15px] font-semibold">
                {view === "timeline" ? t("请求时间线") : t("请求明细")}
              </CardTitle>
              <div className="mt-1 text-xs leading-5 text-muted-foreground">
                {t("共")} {summary.filteredCount} {t("条匹配日志")}
              </div>
            </div>
            <ToggleGroup
              aria-label={t("日志视图")}
              value={[view]}
              onValueChange={(values) => {
                const nextView = values[0];
                if (nextView === "timeline" || nextView === "table") setView(nextView);
              }}
              size="sm"
              spacing={0}
              className="shrink-0 rounded-lg border border-border/60 bg-muted/35 p-1"
            >
              <ToggleGroupItem value="timeline" aria-label={t("时间线视图")} className="gap-1.5 rounded-md! px-3 aria-pressed:bg-background aria-pressed:shadow-sm">
                <List className="size-3.5" />{t("时间线")}
              </ToggleGroupItem>
              <ToggleGroupItem value="table" aria-label={t("表格视图")} className="gap-1.5 rounded-md! px-3 aria-pressed:bg-background aria-pressed:shadow-sm">
                <Table2 className="size-3.5" />{t("表格")}
              </ToggleGroupItem>
            </ToggleGroup>
          </div>
        </CardHeader>
        <CardContent className="px-0">
          {view === "timeline" ? (
            <RequestLogTimeline
              t={t} logs={logs} isLogsLoading={isLogsLoading}
              serviceConnected={serviceConnected} isDirectAccountMode={isDirectAccountMode}
              accountNameMap={accountNameMap} apiKeyMap={apiKeyMap}
              aggregateApiMap={aggregateApiMap} onOpenDetail={onOpenDetail}
            />
          ) : (
            <RequestLogTable
              t={t} logs={logs} isLogsLoading={isLogsLoading}
              serviceConnected={serviceConnected} isDirectAccountMode={isDirectAccountMode}
              accountNameMap={accountNameMap} apiKeyMap={apiKeyMap}
              aggregateApiMap={aggregateApiMap} onOpenDetail={onOpenDetail}
            />
          )}
        </CardContent>
      </Card>

      <div className="flex flex-wrap items-center justify-between gap-3 px-1 pb-2">
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
              onClick={onFirstPage}
            >
              {t("首页")}
            </Button>
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
          <form
            className="flex items-center gap-2"
            noValidate
            onSubmit={(event) => {
              event.preventDefault();
              submitJumpPage();
            }}
          >
            <label
              className="whitespace-nowrap text-xs text-muted-foreground"
              htmlFor="request-log-jump-page"
            >
              {t("跳至")}
            </label>
            <Input
              id="request-log-jump-page"
              type="number"
              inputMode="numeric"
              min={1}
              max={totalPages}
              step={1}
              className="h-8 w-[68px] text-center text-xs"
              value={jumpPageInput}
              onChange={(event) => setJumpPageInput(event.target.value)}
              onBlur={() => {
                if (!jumpPageInput) {
                  setJumpPageInput(String(currentPage));
                }
              }}
            />
            <span className="text-xs text-muted-foreground">{t("页")}</span>
            <Button
              type="submit"
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
            >
              {t("确定")}
            </Button>
          </form>
        </div>
      </div>
    </div>
  );
}
