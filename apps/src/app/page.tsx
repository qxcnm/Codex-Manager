"use client";

import {
  useEffect,
  useMemo,
  useState,
  type ReactNode,
  type WheelEvent as ReactWheelEvent,
} from "react";
import {
  Activity,
  AlertTriangle,
  ArrowRight,
  BarChart3,
  KeyRound,
  LineChart,
  Plus,
  Wallet,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { ApiKeyModal } from "@/components/modals/api-key-modal";
import {
  DashboardGatewayStatus,
  DashboardPoolRemaining,
} from "@/components/dashboard/dashboard-gateway-status";
import {
  AdminUsageTrendChart,
  type AdminUsageGranularity,
} from "@/components/dashboard/admin-usage-trend-chart";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useDashboardStats } from "@/hooks/useDashboardStats";
import { useDashboardAdminUsageSummary } from "@/hooks/useDashboardAdminUsageSummary";
import { resolveSessionRole, useAppSession } from "@/hooks/useAppSession";
import { useLocalDayRange } from "@/hooks/useLocalDayRange";
import { useMemberDashboardSummary } from "@/hooks/useMemberDashboardSummary";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useCodexProfileModeStatus } from "@/hooks/useCodexProfileModeStatus";
import {
  estimateChartYAxisWidth,
  formatCompactTokenAmount,
} from "@/lib/dashboard/format";
import type { AppLocale } from "@/lib/i18n/config";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";
import { formatLocalDateTimeFromSeconds } from "@/lib/utils/time";
import {
  Area,
  AreaChart,
  CartesianGrid,
  XAxis,
  YAxis,
} from "recharts";
import type {
  DashboardAdminUsageSummary,
  DashboardDailyUsagePoint,
  DashboardTokenUsage,
  MemberDashboardAlert,
  MemberDashboardKeyUsage,
  MemberDashboardSummary,
} from "@/types";

interface MetricCardProps {
  title: string;
  value: string;
  icon: LucideIcon;
  color: string;
  sub: string;
  detail?: string;
  badge?: string;
  titleClassName?: string;
  valueClassName?: string;
}

type AdminUsageRangePreset = "7d" | "14d" | "30d" | "custom";

interface AdminUsageRangeValue {
  startTs: number | null;
  endTs: number | null;
  startInput: string;
  endInput: string;
}

const SUPPORTED_INTL_LOCALES = ["zh-CN", "en-US", "ru-RU", "ko-KR"] as const;

const INTL_LOCALE_BY_APP_LOCALE: Record<Exclude<AppLocale, "zh-CN">, string> = {
  en: "en-US",
  ru: "ru-RU",
  ko: "ko-KR",
};

function intlLocaleFromAppLocale(locale: string): string {
  if (
    SUPPORTED_INTL_LOCALES.includes(
      locale as (typeof SUPPORTED_INTL_LOCALES)[number],
    )
  ) {
    return locale;
  }
  return INTL_LOCALE_BY_APP_LOCALE[locale as Exclude<AppLocale, "zh-CN">] ?? "zh-CN";
}

function formatDateInputValue(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function formatDateInputValueFromSeconds(value: number): string {
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return "";
  return formatDateInputValue(date);
}

function parseDateInputStartTs(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value.trim());
  if (!match) return null;
  const [, year, month, day] = match;
  const date = new Date(Number(year), Number(month) - 1, Number(day), 0, 0, 0, 0);
  if (Number.isNaN(date.getTime())) return null;
  return Math.floor(date.getTime() / 1000);
}

function parseDateInputEndTs(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value.trim());
  if (!match) return null;
  const [, year, month, day] = match;
  const date = new Date(Number(year), Number(month) - 1, Number(day) + 1, 0, 0, 0, 0);
  if (Number.isNaN(date.getTime())) return null;
  return Math.floor(date.getTime() / 1000);
}

function buildAdminUsagePresetRange(
  preset: Exclude<AdminUsageRangePreset, "custom">,
  localDayStartTs: number,
  localDayEndTs: number,
): AdminUsageRangeValue {
  const days = preset === "14d" ? 14 : preset === "30d" ? 30 : 7;
  const todayStart = new Date(localDayStartTs * 1000);
  const startDate = new Date(
    todayStart.getFullYear(),
    todayStart.getMonth(),
    todayStart.getDate() - (days - 1),
    0,
    0,
    0,
    0,
  );

  return {
    startTs: Math.floor(startDate.getTime() / 1000),
    endTs: localDayEndTs,
    startInput: formatDateInputValue(startDate),
    endInput: formatDateInputValueFromSeconds(Math.max(localDayStartTs, localDayEndTs - 1)),
  };
}

function formatUsd(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(normalized);
}

function formatWalletCredit(micros: number | null | undefined): string {
  const normalized =
    typeof micros === "number" && Number.isFinite(micros) ? micros / 1_000_000 : 0;
  return formatUsd(normalized);
}

function formatDateTime(value: number | null | undefined): string {
  return formatLocalDateTimeFromSeconds(value, "从未使用");
}

function formatShortDate(value: number | null | undefined, locale: AppLocale): string {
  if (!value) return "--";
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return "--";
  return new Intl.DateTimeFormat(intlLocaleFromAppLocale(locale), {
    month: "2-digit",
    day: "2-digit",
  }).format(date);
}

function formatShortDateRange(
  startTs: number | null | undefined,
  endTsExclusive: number | null | undefined,
  locale: AppLocale,
): string {
  if (!startTs || !endTsExclusive || endTsExclusive <= startTs) {
    return "--";
  }
  return `${formatShortDate(startTs, locale)} - ${formatShortDate(endTsExclusive - 1, locale)}`;
}

function statusBadgeVariant(status: string): "default" | "secondary" | "destructive" | "outline" {
  const normalized = status.trim().toLowerCase();
  if (normalized === "enabled" || normalized === "active") return "default";
  if (normalized === "disabled" || normalized === "inactive") return "secondary";
  return "outline";
}

function alertTone(alert: MemberDashboardAlert): string {
  if (alert.severity === "critical") {
    return "border-destructive/40 bg-destructive/10 text-destructive";
  }
  if (alert.severity === "warning") {
    return "border-yellow-500/40 bg-yellow-500/10 text-yellow-700 dark:text-yellow-300";
  }
  return "border-blue-500/40 bg-blue-500/10 text-blue-700 dark:text-blue-300";
}

function MetricCard({
  title,
  value,
  icon: Icon,
  color,
  sub,
  detail,
  badge,
  titleClassName,
  valueClassName,
}: MetricCardProps) {
  return (
    <Card
      size="sm"
      className="glass-card console-metric mission-panel overflow-hidden py-0 shadow-sm transition-colors"
    >
      <CardContent className="flex min-h-[52px] items-center justify-between gap-2 px-3 py-2">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-2">
            <CardTitle className={cn("min-w-0 truncate text-xs font-semibold text-muted-foreground", titleClassName)}>
              {title}
            </CardTitle>
            {badge ? (
              <span
                className="inline-flex max-w-[96px] items-center gap-1 truncate rounded-md border border-primary/20 bg-primary/8 px-1.5 py-0.5 text-[10px] leading-none text-primary"
                title={badge}
              >
                <Activity className="h-2.5 w-2.5 shrink-0" />
                <span className="truncate">{badge}</span>
              </span>
            ) : null}
          </div>
          <div
            className={cn("mt-1 truncate font-mono text-xl font-semibold leading-none tracking-normal text-foreground tabular-nums", valueClassName)}
            title={detail ? `${sub} · ${detail}` : sub}
          >
            {value}
          </div>
        </div>
        <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background/65 text-primary shadow-sm">
          <Icon className={cn("h-3 w-3", color)} />
        </div>
      </CardContent>
    </Card>
  );
}

function DashboardInitialSkeleton() {
  return (
    <div className="space-y-8 animate-in fade-in duration-500 ease-out motion-reduce:animate-none">
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {Array.from({ length: 4 }).map((_, index) => (
          <Skeleton key={index} className="h-36 w-full rounded-xl" />
        ))}
      </div>
      <Skeleton className="h-52 w-full rounded-xl" />
      <div className="grid gap-6 md:grid-cols-2">
        <Skeleton className="h-72 w-full rounded-xl" />
        <Skeleton className="h-72 w-full rounded-xl" />
      </div>
    </div>
  );
}

function DirectModeUnavailable({
  active,
  children,
  className,
}: {
  active: boolean;
  children: ReactNode;
  className?: string;
}) {
  const { t } = useI18n();
  if (!active) return <>{children}</>;

  return (
    <div className={cn("relative overflow-hidden rounded-xl", className)}>
      <div className="pointer-events-none select-none opacity-60 blur-[1px] grayscale">
        {children}
      </div>
      <div className="absolute inset-0 z-10 flex items-center justify-center bg-background/45 p-4 backdrop-blur-sm">
        <div className="grid max-w-md justify-items-center gap-3 rounded-2xl border border-amber-500/40 bg-background/80 px-5 py-4 text-center shadow-lg shadow-amber-500/10">
          <div>
            <div className="text-sm font-semibold text-amber-700 dark:text-amber-200">
              {t("账号直连模式下不可用")}
            </div>
            <div className="mt-1 text-xs text-muted-foreground">
              {t("切换到本地网关后可统计请求日志、Token 和费用")}
            </div>
          </div>
          <a
            href={buildStaticRouteUrl("/platform-mode")}
            className="inline-flex h-8 items-center justify-center rounded-lg bg-primary px-3 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
          >
            {t("去切换为本地网关")}
          </a>
        </div>
      </div>
    </div>
  );
}

function sumDashboardTokenUsages(usages: DashboardTokenUsage[]): DashboardTokenUsage {
  return usages.reduce<DashboardTokenUsage>(
    (total, usage) => ({
      inputTokens: total.inputTokens + usage.inputTokens,
      cachedInputTokens: total.cachedInputTokens + usage.cachedInputTokens,
      outputTokens: total.outputTokens + usage.outputTokens,
      reasoningOutputTokens:
        total.reasoningOutputTokens + usage.reasoningOutputTokens,
      totalTokens: total.totalTokens + usage.totalTokens,
      estimatedCostUsd: total.estimatedCostUsd + usage.estimatedCostUsd,
      requestCount: total.requestCount + usage.requestCount,
      successCount: total.successCount + usage.successCount,
      errorCount: total.errorCount + usage.errorCount,
    }),
    {
      inputTokens: 0,
      cachedInputTokens: 0,
      outputTokens: 0,
      reasoningOutputTokens: 0,
      totalTokens: 0,
      estimatedCostUsd: 0,
      requestCount: 0,
      successCount: 0,
      errorCount: 0,
    },
  );
}

function DailyTokenLineChart({
  points,
  className,
  zoomWindow,
  onZoomWindowChange,
}: {
  points: DashboardDailyUsagePoint[];
  className?: string;
  zoomWindow?: { startIndex: number; endIndex: number } | null;
  onZoomWindowChange?: (next: { startIndex: number; endIndex: number } | null) => void;
}) {
  const { t, locale } = useI18n();
  const chartConfig = {
    totalTokens: {
      label: t("Token"),
      color: "var(--primary)",
    },
  } satisfies ChartConfig;
  const chartData = points.map((item) => ({
    date: formatShortDate(item.dayStartTs, locale),
    totalTokens: item.usage.totalTokens,
    estimatedCostUsd: item.usage.estimatedCostUsd,
    requestCount: item.usage.requestCount,
  }));
  const normalizedZoomWindow = useMemo(() => {
    if (chartData.length === 0) return null;
    const startIndex = Math.max(
      0,
      Math.min(zoomWindow?.startIndex ?? 0, chartData.length - 1),
    );
    const endIndex = Math.max(
      startIndex,
      Math.min(zoomWindow?.endIndex ?? chartData.length - 1, chartData.length - 1),
    );
    return { startIndex, endIndex };
  }, [chartData.length, zoomWindow?.endIndex, zoomWindow?.startIndex]);
  const visibleStartIndex = normalizedZoomWindow?.startIndex ?? 0;
  const visibleEndIndex = normalizedZoomWindow?.endIndex ?? Math.max(0, chartData.length - 1);
  const visibleChartData = useMemo(
    () => chartData.slice(visibleStartIndex, visibleEndIndex + 1),
    [chartData, visibleEndIndex, visibleStartIndex],
  );

  const handleWheelZoom = (event: ReactWheelEvent<HTMLDivElement>) => {
    if (!onZoomWindowChange || chartData.length <= 2) {
      return;
    }
    event.preventDefault();

    const currentCount = visibleEndIndex - visibleStartIndex + 1;
    const minCount = Math.min(3, chartData.length);
    const step = Math.max(1, Math.round(currentCount * 0.2));
    const nextCount =
      event.deltaY < 0
        ? Math.max(minCount, currentCount - step)
        : Math.min(chartData.length, currentCount + step);
    if (nextCount === currentCount) {
      return;
    }

    const bounds = event.currentTarget.getBoundingClientRect();
    const ratio =
      bounds.width > 0
        ? Math.min(Math.max((event.clientX - bounds.left) / bounds.width, 0), 1)
        : 0.5;
    const focalIndex = visibleStartIndex + Math.round((currentCount - 1) * ratio);

    let nextStartIndex = focalIndex - Math.floor((nextCount - 1) * ratio);
    let nextEndIndex = nextStartIndex + nextCount - 1;

    if (nextStartIndex < 0) {
      nextStartIndex = 0;
      nextEndIndex = nextCount - 1;
    }
    if (nextEndIndex > chartData.length - 1) {
      nextEndIndex = chartData.length - 1;
      nextStartIndex = Math.max(0, nextEndIndex - nextCount + 1);
    }

    onZoomWindowChange({
      startIndex: nextStartIndex,
      endIndex: nextEndIndex,
    });
  };
  const yAxisWidth = estimateChartYAxisWidth(
    [0, ...visibleChartData.map((item) => item.totalTokens)],
    formatCompactTokenAmount,
  );

  return (
    <div
      className="mission-panel rounded-lg border border-primary/20 bg-background/30 shadow-[inset_0_1px_0_rgb(255_255_255/0.06)]"
      onWheel={handleWheelZoom}
      title={t("在图表区域使用鼠标滚轮缩放时间区间")}
    >
      <ChartContainer
        config={chartConfig}
        className={cn("h-64 w-full rounded-md bg-transparent p-3", className)}
        initialDimension={{ width: 720, height: 256 }}
      >
        <AreaChart
          accessibilityLayer
          data={visibleChartData}
          margin={{ top: 18, right: 14, left: 10, bottom: 4 }}
        >
          <defs>
            <linearGradient id="fillTotalTokens" x1="0" y1="0" x2="0" y2="1">
              <stop
                offset="5%"
                stopColor="var(--color-totalTokens)"
                stopOpacity={0.32}
              />
              <stop
                offset="95%"
                stopColor="var(--color-totalTokens)"
                stopOpacity={0.03}
              />
            </linearGradient>
          </defs>
          <CartesianGrid vertical={false} stroke="rgb(var(--primary-rgb) / 0.16)" strokeDasharray="4 8" />
          <XAxis
            dataKey="date"
            tickLine={false}
            axisLine={false}
            tickMargin={10}
            minTickGap={18}
          />
          <YAxis
            tickLine={false}
            axisLine={false}
            tickMargin={10}
            width={yAxisWidth}
            tickFormatter={(value) => formatCompactTokenAmount(Number(value))}
          />
          <ChartTooltip
            cursor={false}
            content={
              <ChartTooltipContent
                indicator="line"
                labelFormatter={(value) => value}
                formatter={(value, name, item) => {
                  const row = item.payload as {
                    estimatedCostUsd?: number;
                    requestCount?: number;
                  };
                  return (
                    <div className="grid min-w-36 gap-1">
                      <div className="flex items-center justify-between gap-3">
                        <span className="text-muted-foreground">{String(name)}</span>
                        <span className="font-mono font-medium text-foreground">
                          {formatCompactTokenAmount(Number(value))}
                        </span>
                      </div>
                      <div className="flex items-center justify-between gap-3 text-muted-foreground">
                        <span>Cost</span>
                        <span>{formatUsd(row.estimatedCostUsd)}</span>
                      </div>
                      <div className="flex items-center justify-between gap-3 text-muted-foreground">
                        <span>Requests</span>
                        <span>{row.requestCount ?? 0}</span>
                      </div>
                    </div>
                  );
                }}
              />
            }
          />
          <Area
            dataKey="totalTokens"
            type="monotone"
            fill="url(#fillTotalTokens)"
            stroke="var(--color-totalTokens)"
            strokeWidth={2.5}
            dot={{ r: 4, strokeWidth: 2, fill: "var(--background)" }}
            activeDot={{ r: 6, strokeWidth: 2 }}
          />
        </AreaChart>
      </ChartContainer>
    </div>
  );
}

function AdminUsageAnalyticsCard({
  summary,
  isLoading,
  isRefreshing,
  isError,
  rangePreset,
  rangeStartInput,
  rangeEndInput,
  onRangePresetChange,
  onRangeStartInputChange,
  onRangeEndInputChange,
  onApplyCustomRange,
  isCustomRangeInvalid,
  granularity,
  onGranularityChange,
}: {
  summary: DashboardAdminUsageSummary | undefined;
  isLoading: boolean;
  isRefreshing: boolean;
  isError: boolean;
  rangePreset: AdminUsageRangePreset;
  rangeStartInput: string;
  rangeEndInput: string;
  onRangePresetChange: (preset: AdminUsageRangePreset) => void;
  onRangeStartInputChange: (value: string) => void;
  onRangeEndInputChange: (value: string) => void;
  onApplyCustomRange: () => void;
  isCustomRangeInvalid: boolean;
  granularity: AdminUsageGranularity;
  onGranularityChange: (granularity: AdminUsageGranularity) => void;
}) {
  const { t, locale } = useI18n();
  const [zoomWindow, setZoomWindow] = useState<{
    startIndex: number;
    endIndex: number;
  } | null>(null);

  useEffect(() => {
    let active = true;
    if (!summary?.dailyUsage.length) {
      queueMicrotask(() => {
        if (active) setZoomWindow(null);
      });
      return () => {
        active = false;
      };
    }
    const nextZoomWindow = {
      startIndex: 0,
      endIndex: summary.dailyUsage.length - 1,
    };
    queueMicrotask(() => {
      if (active) setZoomWindow(nextZoomWindow);
    });
    return () => {
      active = false;
    };
  }, [summary?.dailyUsage.length, summary?.rangeEndTs, summary?.rangeStartTs]);

  if (isLoading) {
    return <Skeleton className="h-[420px] w-full rounded-xl" />;
  }
  if (isError) {
    return (
      <Card className="glass-card mission-panel shadow-sm">
        <CardContent>
          <Alert variant="destructive">
            <AlertTriangle />
            <AlertTitle>{t("管理员用量分析读取失败")}</AlertTitle>
            <AlertDescription>{t("请稍后重试或检查核心服务状态。")}</AlertDescription>
          </Alert>
        </CardContent>
      </Card>
    );
  }
  if (!summary) {
    return (
      <Card className="glass-card mission-panel shadow-sm">
        <CardContent>
          <Empty className="min-h-40 border bg-muted/20">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <LineChart />
              </EmptyMedia>
              <EmptyTitle>{t("管理员用量分析暂不可用")}</EmptyTitle>
              <EmptyDescription>{t("核心服务连接后会自动刷新。")}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        </CardContent>
      </Card>
    );
  }

  const isTodayOnlyRange =
    summary.rangeStartTs === summary.todayStartTs &&
    summary.rangeEndTs === summary.todayEndTs;
  const rangeUsage = isTodayOnlyRange
    ? summary.todayUsage
    : sumDashboardTokenUsages(summary.dailyUsage.map((item) => item.usage));
  const hasZoomWindow =
    summary.modelUsage.length === 0 &&
    summary.dailyUsage.length > 1 &&
    zoomWindow != null &&
    (zoomWindow.startIndex > 0 ||
      zoomWindow.endIndex < summary.dailyUsage.length - 1);
  const rangeBadgeLabel = isTodayOnlyRange ? t("今日") : t("所选区间");

  return (
    <Card
      id="admin-usage-analytics"
      className="dashboard-analytics-card dashboard-primary-panel glass-card mission-panel scroll-mt-4 overflow-hidden shadow-sm"
    >
      <CardHeader className="flex flex-col gap-4">
        <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div>
            <CardTitle className="flex items-center gap-2 text-base font-semibold">
              <LineChart className="h-4 w-4 text-primary" />
              {t("管理员用量分析")}
            </CardTitle>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("按模型查看 token 和请求趋势，支持小时粒度")}
            </p>
            <div className="mt-2 text-[11px] text-muted-foreground">
              {t("当前区间")} {formatShortDateRange(summary.rangeStartTs, summary.rangeEndTs, locale)}
              {" · "}
              {t("图表区域支持鼠标滚轮缩放")}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-2 xl:justify-end">
            <div className="flex flex-wrap items-center gap-2">
              <Select
                value={rangePreset}
                onValueChange={(value) =>
                  onRangePresetChange(value as AdminUsageRangePreset)
                }
              >
                <SelectTrigger className="w-[132px] bg-background/40">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    <SelectItem value="7d">{t("最近 7 天")}</SelectItem>
                    <SelectItem value="14d">{t("最近 14 天")}</SelectItem>
                    <SelectItem value="30d">{t("最近 30 天")}</SelectItem>
                    <SelectItem value="custom">{t("自定义区间")}</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
              <Input
                type="date"
                className="w-[144px] bg-background/40 text-xs"
                value={rangeStartInput}
                disabled={rangePreset !== "custom"}
                onChange={(event) => onRangeStartInputChange(event.target.value)}
              />
              <Input
                type="date"
                className="w-[144px] bg-background/40 text-xs"
                value={rangeEndInput}
                disabled={rangePreset !== "custom"}
                onChange={(event) => onRangeEndInputChange(event.target.value)}
              />
              <Button
                size="sm"
                variant="outline"
                disabled={rangePreset !== "custom" || isCustomRangeInvalid}
                onClick={onApplyCustomRange}
              >
                {t("应用")}
              </Button>
              {hasZoomWindow ? (
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    setZoomWindow({
                      startIndex: 0,
                      endIndex: summary.dailyUsage.length - 1,
                    })
                  }
                >
                  {t("重置缩放")}
                </Button>
              ) : null}
            </div>
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        {summary.seriesUsage.length > 0 ? (
          <AdminUsageTrendChart
            summary={summary}
            granularity={granularity}
            onGranularityChange={onGranularityChange}
            hourlyAvailable={
              summary.rangeEndTs - summary.rangeStartTs <= 31 * 86_400
            }
            isRefreshing={isRefreshing}
          />
        ) : (
          <DailyTokenLineChart
            points={summary.dailyUsage}
            zoomWindow={zoomWindow}
            onZoomWindowChange={setZoomWindow}
          />
        )}
        <div className="grid gap-3 text-xs sm:grid-cols-2 xl:grid-cols-4">
          <div className="mission-panel rounded-md border border-primary/20 bg-primary/10 px-3 py-2">
            <div className="text-muted-foreground">{rangeBadgeLabel}</div>
            <div className="mt-1 font-mono font-semibold text-primary">
              {formatCompactTokenAmount(rangeUsage.totalTokens)}
            </div>
            <div className="text-muted-foreground">
              {formatUsd(rangeUsage.estimatedCostUsd)}
            </div>
          </div>
          <div className="mission-panel rounded-md border border-primary/20 bg-primary/10 px-3 py-2">
            <div className="text-muted-foreground">
              {isTodayOnlyRange ? t("今日请求") : t("区间请求")}
            </div>
            <div className="mt-1 font-mono font-semibold">
              {rangeUsage.requestCount} · {t("成功")} {rangeUsage.successCount}
            </div>
          </div>
          <div className="mission-panel rounded-md border border-primary/20 bg-primary/10 px-3 py-2">
            <div className="text-muted-foreground">
              {isTodayOnlyRange ? t("输入 / 输出") : t("区间输入 / 输出")}
            </div>
            <div className="mt-1 font-mono font-semibold">
              {formatCompactTokenAmount(rangeUsage.inputTokens - rangeUsage.cachedInputTokens)} /{" "}
              {formatCompactTokenAmount(rangeUsage.outputTokens)}
            </div>
          </div>
          <div className="mission-panel rounded-md border border-primary/20 bg-primary/10 px-3 py-2">
            <div className="text-muted-foreground">
              {isTodayOnlyRange ? t("缓存 / 推理") : t("区间缓存 / 推理")}
            </div>
            <div className="mt-1 font-mono font-semibold">
              {formatCompactTokenAmount(rangeUsage.cachedInputTokens)} /{" "}
              {formatCompactTokenAmount(rangeUsage.reasoningOutputTokens)}
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

function AdminDashboard() {
  const { stats, isLoading, isServiceReady } = useDashboardStats({
    requestLogLimit: 0,
    includeAccountHints: false,
    includeApiModels: false,
    includeApiKeys: false,
    includeAccounts: true,
    includeUsageSnapshots: true,
    includeAccountRuntime: true,
    includeAccountDetails: false,
  });
  const { isDirectAccountMode } = useCodexProfileModeStatus({
    enabled: true,
    refetchIntervalMs: 10_000,
  });
  const localDayRange = useLocalDayRange();
  const [adminUsageRangePreset, setAdminUsageRangePreset] =
    useState<AdminUsageRangePreset>("7d");
  const [adminUsageRangeStartInput, setAdminUsageRangeStartInput] = useState("");
  const [adminUsageRangeEndInput, setAdminUsageRangeEndInput] = useState("");
  const [adminUsageRangeParams, setAdminUsageRangeParams] =
    useState<AdminUsageRangeValue>({
      startTs: null,
      endTs: null,
      startInput: "",
      endInput: "",
    });
  const [adminUsageGranularity, setAdminUsageGranularity] =
    useState<AdminUsageGranularity>("day");

  useEffect(() => {
    if (adminUsageRangePreset === "custom") {
      return;
    }
    let active = true;
    const nextRange = buildAdminUsagePresetRange(
      adminUsageRangePreset,
      localDayRange.dayStartTs,
      localDayRange.dayEndTs,
    );
    queueMicrotask(() => {
      if (!active) return;
      setAdminUsageRangeStartInput(nextRange.startInput);
      setAdminUsageRangeEndInput(nextRange.endInput);
      setAdminUsageRangeParams(nextRange);
    });
    return () => {
      active = false;
    };
  }, [
    adminUsageRangePreset,
    localDayRange.dayEndTs,
    localDayRange.dayStartTs,
  ]);

  const {
    data: adminUsageSummary,
    isLoading: isAdminUsageLoading,
    isFetching: isAdminUsageFetching,
    isError: isAdminUsageError,
  } = useDashboardAdminUsageSummary(
    {
      startTs: adminUsageRangeParams.startTs,
      endTs: adminUsageRangeParams.endTs,
      includeBreakdowns: false,
      includeSeries: true,
      seriesBucketSeconds: adminUsageGranularity === "hour" ? 3_600 : 86_400,
    },
    true,
  );
  usePageTransitionReady("/", !isServiceReady || !isLoading);

  const isCustomAdminUsageRangeInvalid =
    adminUsageRangePreset === "custom" &&
    (() => {
      const startTs = parseDateInputStartTs(adminUsageRangeStartInput);
      const endTs = parseDateInputEndTs(adminUsageRangeEndInput);
      return startTs == null || endTs == null || endTs <= startTs;
    })();

  return (
    <div className="space-y-5 animate-in fade-in duration-500 xl:space-y-7">
      <DashboardGatewayStatus
        connected={isServiceReady}
        directMode={isDirectAccountMode}
        stats={{
          total: stats.total,
          available: stats.available,
          unavailable: stats.unavailable,
          todayTokens: adminUsageSummary?.todayUsage.totalTokens ?? stats.todayTokens,
          cachedTokens:
            adminUsageSummary?.todayUsage.cachedInputTokens ?? stats.cachedTokens,
          reasoningTokens:
            adminUsageSummary?.todayUsage.reasoningOutputTokens ?? stats.reasoningTokens,
          todayCost: adminUsageSummary?.todayUsage.estimatedCostUsd ?? stats.todayCost,
        }}
        isLoading={isLoading}
      />

      <DashboardPoolRemaining
        primary={stats.poolRemain.primary}
        secondary={stats.poolRemain.secondary}
        primaryKnownCount={stats.poolRemain.primaryKnownCount}
        primaryBucketCount={stats.poolRemain.primaryBucketCount}
        secondaryKnownCount={stats.poolRemain.secondaryKnownCount}
        secondaryBucketCount={stats.poolRemain.secondaryBucketCount}
        isLoading={isLoading}
      />

      <DirectModeUnavailable active={isDirectAccountMode}>
        <AdminUsageAnalyticsCard
          summary={adminUsageSummary}
          isLoading={isLoading || isAdminUsageLoading}
          isRefreshing={isAdminUsageFetching && !isAdminUsageLoading}
          isError={isAdminUsageError}
          rangePreset={adminUsageRangePreset}
          rangeStartInput={adminUsageRangeStartInput}
          rangeEndInput={adminUsageRangeEndInput}
          onRangePresetChange={(preset) => {
            setAdminUsageRangePreset(preset);
            if (preset === "custom") {
              return;
            }
            const nextRange = buildAdminUsagePresetRange(
              preset,
              localDayRange.dayStartTs,
              localDayRange.dayEndTs,
            );
            setAdminUsageRangeStartInput(nextRange.startInput);
            setAdminUsageRangeEndInput(nextRange.endInput);
            setAdminUsageRangeParams(nextRange);
          }}
          onRangeStartInputChange={setAdminUsageRangeStartInput}
          onRangeEndInputChange={setAdminUsageRangeEndInput}
          onApplyCustomRange={() => {
            const startTs = parseDateInputStartTs(adminUsageRangeStartInput);
            const endTs = parseDateInputEndTs(adminUsageRangeEndInput);
            if (startTs == null || endTs == null || endTs <= startTs) {
              return;
            }
            if (endTs - startTs > 31 * 86_400) {
              setAdminUsageGranularity("day");
            }
            setAdminUsageRangeParams({
              startTs,
              endTs,
              startInput: adminUsageRangeStartInput,
              endInput: adminUsageRangeEndInput,
            });
          }}
          isCustomRangeInvalid={isCustomAdminUsageRangeInvalid}
          granularity={adminUsageGranularity}
          onGranularityChange={setAdminUsageGranularity}
        />
      </DirectModeUnavailable>

    </div>
  );
}

function MemberDashboard() {
  const { t } = useI18n();
  const [apiKeyModalOpen, setApiKeyModalOpen] = useState(false);
  const {
    data: summary,
    isLoading,
    isServiceReady,
    isError,
  } = useMemberDashboardSummary(true);
  usePageTransitionReady("/", !isServiceReady || !isLoading);

  if (isLoading) {
    return <DashboardInitialSkeleton />;
  }

  if (isError || !summary) {
    return (
      <Card className="glass-card mission-panel shadow-sm">
        <CardContent className="flex min-h-[220px] flex-col items-center justify-center gap-3 text-center">
          <AlertTriangle className="h-8 w-8 text-yellow-500" />
          <div className="text-base font-semibold">{t("个人仪表盘暂不可用")}</div>
          <p className="max-w-md text-sm text-muted-foreground">
            {isServiceReady ? t("请稍后重试或检查登录状态。") : t("正在等待服务连接。")}
          </p>
        </CardContent>
      </Card>
    );
  }

  const successRate =
    summary.usageToday.successRate == null
      ? "--"
      : `${Math.round(summary.usageToday.successRate * 100)}%`;

  return (
    <div className="space-y-6 animate-in fade-in duration-700">
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        <MetricCard
          title={t("钱包余额")}
          value={
            summary.distributionEnabled
              ? formatWalletCredit(summary.wallet?.availableCreditMicros)
              : t("未启用扣费")
          }
          icon={Wallet}
          color="text-emerald-500"
          sub={
            summary.distributionEnabled
              ? t("当前账号可用余额")
              : t("额度分发未启用")
          }
          badge={
            summary.wallet?.status
              ? `${t("状态")} ${summary.wallet.status}`
              : summary.distributionEnabled
                ? t("无钱包")
                : undefined
          }
        />
        <MetricCard
          title={t("今日用量")}
          value={formatCompactTokenAmount(summary.usageToday.totalTokens)}
          icon={Zap}
          color="text-yellow-500"
          sub={`${formatUsd(summary.usageToday.estimatedCostUsd)} · ${t("成功率")} ${successRate}`}
        />
        <MetricCard
          title={t("我的平台密钥")}
          value={`${summary.apiKeySummary.enabledCount}/${summary.apiKeySummary.totalCount}`}
          icon={KeyRound}
          color="text-blue-500"
          sub={`${t("启用 / 全部")} · ${t("最近")} ${t(formatDateTime(summary.apiKeySummary.lastUsedAt))}`}
        />
      </div>

      <MemberAlerts alerts={summary.alerts} onCreateKey={() => setApiKeyModalOpen(true)} />

      <div className="grid gap-6 xl:grid-cols-12">
        <MemberKeyUsageCard
          summary={summary}
          onCreateKey={() => setApiKeyModalOpen(true)}
          className="xl:col-span-7"
        />
        <MemberUsageTrendCard summary={summary} className="xl:col-span-5" />
      </div>

      <ApiKeyModal
        open={apiKeyModalOpen}
        onOpenChange={setApiKeyModalOpen}
        isAdminMode={false}
      />
    </div>
  );
}

function MemberAlerts({
  alerts,
  onCreateKey,
}: {
  alerts: MemberDashboardAlert[];
  onCreateKey: () => void;
}) {
  const { t } = useI18n();
  if (alerts.length === 0) return null;
  return (
    <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      {alerts.map((alert) => {
        const action =
          alert.kind === "no_api_key" ? (
            <Button size="xs" variant="outline" onClick={onCreateKey}>
              <Plus className="h-3 w-3" />
              {alert.actionLabel ? t(alert.actionLabel) : t("创建 Key")}
            </Button>
          ) : alert.actionHref ? (
            <a
              href={buildStaticRouteUrl(alert.actionHref)}
              className="inline-flex h-6 items-center gap-1 rounded-md border border-border/60 bg-background/40 px-2 text-xs font-medium text-foreground transition-colors hover:bg-muted"
            >
              {alert.actionLabel ? t(alert.actionLabel) : t("查看")}
              <ArrowRight className="h-3 w-3" />
            </a>
          ) : null;
        return (
          <div
            key={alert.kind}
            className={cn("mission-panel rounded-lg border px-3 py-2.5 text-sm", alertTone(alert))}
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="font-semibold">{t(alert.title)}</div>
                <div className="mt-0.5 text-xs opacity-80">{t(alert.message)}</div>
              </div>
              {action}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function MemberKeyUsageCard({
  summary,
  onCreateKey,
  className,
}: {
  summary: MemberDashboardSummary;
  onCreateKey: () => void;
  className?: string;
}) {
  const { t } = useI18n();
  return (
    <Card className={cn("glass-card mission-panel shadow-sm", className)}>
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3">
        <div>
          <CardTitle className="text-base font-semibold">{t("我的平台 Key")}</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            {summary.apiKeySummary.totalCount} {t("个 Key")} · {summary.apiKeySummary.enabledCount} {t("个启用")}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button size="sm" onClick={onCreateKey}>
            <Plus className="h-3.5 w-3.5" />
            {t("创建 Key")}
          </Button>
          <a
            href={buildStaticRouteUrl("/apikeys/")}
            className="inline-flex h-7 items-center gap-1 rounded-md border border-border/60 bg-background/40 px-2.5 text-[0.8rem] font-medium text-muted-foreground transition-colors hover:text-foreground"
          >
            {t("查看全部")}
            <ArrowRight className="h-3.5 w-3.5" />
          </a>
        </div>
      </CardHeader>
      <CardContent>
        {summary.topKeys.length === 0 ? (
          <div className="mission-panel flex min-h-[180px] flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-border/60 bg-background/35 text-center text-sm text-muted-foreground">
            <KeyRound className="h-8 w-8 opacity-30" />
            <span>{t("暂无平台 Key")}</span>
          </div>
        ) : (
          <>
            <div className="hidden md:block">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t("名称")}</TableHead>
                    <TableHead>{t("模型")}</TableHead>
                    <TableHead>{t("今日")}</TableHead>
                    <TableHead>{t("费用")}</TableHead>
                    <TableHead>{t("最近使用")}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {summary.topKeys.map((item) => (
                    <TableRow key={item.keyId} className="border-border/40">
                      <TableCell>
                        <div className="flex min-w-0 items-center gap-2">
                          <Badge variant={statusBadgeVariant(item.status)}>
                            {item.status}
                          </Badge>
                          <span className="max-w-[180px] truncate font-medium">
                            {item.name || item.keyId}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell className="max-w-[180px] truncate font-mono text-xs">
                        {item.modelSlug || "auto"}
                      </TableCell>
                      <TableCell>{formatCompactTokenAmount(item.todayTokens)}</TableCell>
                      <TableCell>{formatUsd(item.todayCostUsd)}</TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {t(formatDateTime(item.lastUsedAt))}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            <div className="divide-y divide-border/40 md:hidden">
              {summary.topKeys.map((item) => (
                <MemberKeyCompactRow key={item.keyId} item={item} />
              ))}
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}

function MemberKeyCompactRow({ item }: { item: MemberDashboardKeyUsage }) {
  const { t } = useI18n();

  return (
    <div className="py-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold">{item.name || item.keyId}</div>
          <div className="mt-1 truncate font-mono text-xs text-muted-foreground">
            {item.modelSlug || "auto"}
          </div>
        </div>
        <Badge variant={statusBadgeVariant(item.status)}>{item.status}</Badge>
      </div>
      <div className="mt-2 grid grid-cols-3 gap-2 text-xs">
        <span className="text-muted-foreground">{formatCompactTokenAmount(item.todayTokens)}</span>
        <span className="text-muted-foreground">{formatUsd(item.todayCostUsd)}</span>
        <span className="truncate text-muted-foreground">{t(formatDateTime(item.lastUsedAt))}</span>
      </div>
    </div>
  );
}

function MemberUsageTrendCard({
  summary,
  className,
}: {
  summary: MemberDashboardSummary;
  className?: string;
}) {
  const { t, locale } = useI18n();
  const maxTokens = useMemo(
    () => Math.max(1, ...summary.usageTrend7d.map((item) => item.totalTokens)),
    [summary.usageTrend7d],
  );
  return (
    <Card className={cn("glass-card mission-panel shadow-sm", className)}>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base font-semibold">
          <LineChart className="h-4 w-4 text-primary" />
          {t("用量趋势")}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="mission-panel flex h-40 items-end gap-2 rounded-lg border border-primary/20 bg-background/25 px-3 py-4">
          {summary.usageTrend7d.map((item) => {
            const height = Math.max(6, Math.round((item.totalTokens / maxTokens) * 112));
            return (
              <div
                key={item.dayStartTs}
                className="flex min-w-0 flex-1 flex-col items-center justify-end gap-2"
              >
                <div
                  className="w-full rounded-t-md bg-primary/75 shadow-sm transition-all"
                  style={{ height }}
                  title={`${formatShortDate(item.dayStartTs, locale)} ${formatCompactTokenAmount(item.totalTokens)}`}
                />
                <div className="text-[10px] text-muted-foreground">
                  {formatShortDate(item.dayStartTs, locale)}
                </div>
              </div>
            );
          })}
        </div>

        <div className="max-w-md">
          <TopUsageList
            title={t("Top 模型")}
            icon={BarChart3}
            emptyText={t("暂无模型用量")}
            items={summary.topModels.map((item) => ({
              key: item.model,
              label: item.model,
              value: formatCompactTokenAmount(item.totalTokens),
              sub: formatUsd(item.estimatedCostUsd),
            }))}
          />
        </div>
      </CardContent>
    </Card>
  );
}

function TopUsageList({
  title,
  icon: Icon,
  emptyText,
  items,
}: {
  title: string;
  icon: LucideIcon;
  emptyText: string;
  items: Array<{ key: string; label: string; value: string; sub: string }>;
}) {
  return (
    <div>
      <div className="mb-2 flex items-center gap-2 text-xs font-semibold text-muted-foreground">
        <Icon className="h-3.5 w-3.5" />
        {title}
      </div>
      {items.length === 0 ? (
        <div className="rounded-lg bg-muted/25 px-3 py-2 text-xs text-muted-foreground">
          {emptyText}
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((item) => (
            <div key={item.key} className="flex items-center justify-between gap-3 text-xs">
              <div className="min-w-0">
                <div className="truncate font-medium">{item.label}</div>
                <div className="truncate text-muted-foreground">{item.sub}</div>
              </div>
              <div className="shrink-0 font-semibold">{item.value}</div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default function DashboardPage() {
  const { data: session, isLoading } = useAppSession();
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const role = resolveSessionRole(session, isLoading, isDesktopRuntime);

  if (isLoading && !session) {
    return <DashboardInitialSkeleton />;
  }

  if (role === "member") {
    return <MemberDashboard />;
  }

  return <AdminDashboard />;
}
