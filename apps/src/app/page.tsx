"use client";

import {
  useEffect,
  useMemo,
  useState,
  type ReactNode,
  type WheelEvent as ReactWheelEvent,
} from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Activity,
  AlertTriangle,
  ArrowRight,
  BarChart3,
  BrainCircuit,
  CheckCircle2,
  Clock3,
  Database,
  DollarSign,
  KeyRound,
  LineChart,
  PieChart,
  Plus,
  ShieldCheck,
  Wallet,
  Users,
  XCircle,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { ApiKeyModal } from "@/components/modals/api-key-modal";
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
import { Progress } from "@/components/ui/progress";
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
  formatPercent,
} from "@/lib/dashboard/format";
import { quotaClient } from "@/lib/api/quota-client";
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
  DashboardSourceUsageSummary,
  DashboardTokenUsage,
  DashboardUserUsageSummary,
  MemberDashboardAlert,
  MemberDashboardKeyUsage,
  MemberDashboardSummary,
  ModelInfo,
  RequestLog,
} from "@/types";

interface StatProgressCardProps {
  title: string;
  value: number;
  total: number;
  icon: LucideIcon;
  color: string;
  sub: string;
}

interface MetricCardProps {
  title: string;
  value: string;
  icon: LucideIcon;
  color: string;
  sub: string;
  badge?: string;
}

interface PercentBarProps {
  label: string;
  value: number | null | undefined;
  tone?: "default" | "green" | "blue";
}

interface AccountHighlightCardProps {
  title: string;
  name: string;
  subtitle: string;
  tone?: "green" | "blue";
  progressLabel?: string;
  progressValue?: number | null | undefined;
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

function formatDuration(value: number | null | undefined): string {
  if (value == null) return "-";
  if (value >= 10_000) return `${Math.round(value / 1000)}s`;
  if (value >= 1000) return `${(value / 1000).toFixed(1).replace(/\.0$/, "")}s`;
  return `${Math.round(value)}ms`;
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

function modelPriceSummary(model: ModelInfo): string {
  const raw = model.priceSummary;
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return "按平台价格规则";
  }
  const source = raw as Record<string, unknown>;
  const input = typeof source.inputUsdPer1M === "number" ? source.inputUsdPer1M : null;
  const output = typeof source.outputUsdPer1M === "number" ? source.outputUsdPer1M : null;
  if (input == null || output == null) return "按平台价格规则";
  return `In ${formatUsd(input)}/1M · Out ${formatUsd(output)}/1M`;
}

function PercentBar({ label, value, tone = "default" }: PercentBarProps) {
  const normalized = value == null ? 0 : Math.max(0, Math.min(100, Math.round(value)));
  const colorClass =
    tone === "green"
      ? "bg-green-500"
      : tone === "blue"
        ? "bg-blue-500"
        : "bg-primary";

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between text-[10px]">
        <span className="text-muted-foreground">{label}</span>
        <span className="font-semibold">{formatPercent(value)}</span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted/60">
        <div
          className={cn("h-full rounded-full transition-all", colorClass)}
          style={{ width: `${normalized}%` }}
        />
      </div>
    </div>
  );
}

function quotaTrackClass(tone: "green" | "blue") {
  return tone === "blue" ? "bg-blue-500/20" : "bg-green-500/20";
}

function quotaIndicatorClass(tone: "green" | "blue") {
  return tone === "blue" ? "bg-blue-500" : "bg-green-500";
}

function AccountHighlightCard({
  title,
  name,
  subtitle,
  tone = "green",
  progressLabel,
  progressValue,
}: AccountHighlightCardProps) {
  const iconToneClass =
    tone === "blue"
      ? "bg-blue-500/20 text-blue-500"
      : "bg-green-500/20 text-green-500";

  return (
    <div className="rounded-xl border border-border/40 bg-accent/20 p-4 shadow-sm">
      <div className="flex items-center gap-4">
        <div
          className={cn(
            "flex h-11 w-11 shrink-0 items-center justify-center rounded-xl",
            iconToneClass,
          )}
        >
          <CheckCircle2 className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <p className="text-[11px] font-medium text-muted-foreground">{title}</p>
          <p className="truncate text-sm font-semibold leading-5">{name}</p>
          <p className="truncate text-xs text-muted-foreground">{subtitle}</p>
        </div>
      </div>
      {progressLabel ? (
        <div className="mt-3 border-t border-border/40 pt-3">
          <PercentBar label={progressLabel} value={progressValue} tone={tone} />
        </div>
      ) : null}
    </div>
  );
}

function StatProgressCard({
  title,
  value,
  total,
  icon: Icon,
  color,
  sub,
}: StatProgressCardProps) {
  const { t } = useI18n();
  const percentage = total > 0 ? Math.min(Math.round((value / total) * 100), 100) : 0;

  return (
    <Card className="glass-card overflow-hidden shadow-sm transition-colors">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        <Icon className={cn("h-4 w-4", color)} />
      </CardHeader>
      <CardContent className="space-y-3">
        <div>
          <div className="text-2xl font-bold">{value}</div>
          <p className="mt-1 text-[10px] text-muted-foreground">{sub}</p>
        </div>
        <div className="space-y-1">
          <div className="flex items-center justify-between text-[10px]">
            <span className="text-muted-foreground">{t("占比")}</span>
            <span className="font-mono font-medium">{percentage}%</span>
          </div>
          <Progress value={percentage} className="h-1.5" />
        </div>
      </CardContent>
    </Card>
  );
}

function MetricCard({ title, value, icon: Icon, color, sub, badge }: MetricCardProps) {
  return (
    <Card className="glass-card overflow-hidden shadow-sm transition-colors">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        <Icon className={cn("h-4 w-4", color)} />
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">{value}</div>
        <p className="mt-1 text-[10px] text-muted-foreground">{sub}</p>
        {badge ? (
          <div className="mt-4 flex w-fit items-center gap-2 rounded-full bg-blue-500/10 px-2 py-0.5 text-[10px] text-blue-600 dark:text-blue-400">
            <Activity className="h-3 w-3" />
            {badge}
          </div>
        ) : null}
      </CardContent>
    </Card>
  );
}

function DashboardInitialSkeleton() {
  return (
    <div className="space-y-6 animate-in fade-in duration-700">
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

function userUsageName(item: DashboardUserUsageSummary): string {
  return item.displayName || item.username || item.userId;
}

function sourceUsageName(item: DashboardSourceUsageSummary): string {
  return item.name || item.sourceId;
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
      className="rounded-xl"
      onWheel={handleWheelZoom}
      title={t("在图表区域使用鼠标滚轮缩放时间区间")}
    >
      <ChartContainer
        config={chartConfig}
        className={cn("h-64 w-full rounded-xl bg-background/30 p-3", className)}
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
          <CartesianGrid vertical={false} strokeDasharray="4 8" />
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
            strokeWidth={3}
            dot={{ r: 4, strokeWidth: 2, fill: "var(--background)" }}
            activeDot={{ r: 6, strokeWidth: 2 }}
          />
        </AreaChart>
      </ChartContainer>
    </div>
  );
}

function UsageRankList<T extends { todayUsage: DashboardTokenUsage; rangeUsage: DashboardTokenUsage }>({
  title,
  items,
  labelForItem,
  emptyText,
  usageForItem = (item) => item.todayUsage,
}: {
  title: string;
  items: T[];
  labelForItem: (item: T) => string;
  emptyText: string;
  usageForItem?: (item: T) => DashboardTokenUsage;
}) {
  return (
    <div className="min-w-0">
      <div className="mb-2 text-xs font-semibold text-muted-foreground">{title}</div>
      {items.length === 0 ? (
        <Empty className="min-h-20 border bg-muted/20 p-3">
          <EmptyHeader>
            <EmptyTitle>{emptyText}</EmptyTitle>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="space-y-2">
          {items.slice(0, 5).map((item, index) => {
            const usage = usageForItem(item);
            return (
              <div
                key={`${labelForItem(item)}-${index}`}
                className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-lg bg-background/30 px-3 py-2 text-xs"
              >
                <div className="min-w-0">
                  <div className="truncate font-medium">{labelForItem(item)}</div>
                  <div className="truncate text-muted-foreground">
                    {usage.requestCount} req · {formatUsd(usage.estimatedCostUsd)}
                  </div>
                </div>
                <div className="shrink-0 text-right font-semibold">
                  {formatCompactTokenAmount(usage.totalTokens)}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function AdminUsageAnalyticsCard({
  summary,
  isLoading,
  isError,
  rangePreset,
  rangeStartInput,
  rangeEndInput,
  onRangePresetChange,
  onRangeStartInputChange,
  onRangeEndInputChange,
  onApplyCustomRange,
  isCustomRangeInvalid,
}: {
  summary: DashboardAdminUsageSummary | undefined;
  isLoading: boolean;
  isError: boolean;
  rangePreset: AdminUsageRangePreset;
  rangeStartInput: string;
  rangeEndInput: string;
  onRangePresetChange: (preset: AdminUsageRangePreset) => void;
  onRangeStartInputChange: (value: string) => void;
  onRangeEndInputChange: (value: string) => void;
  onApplyCustomRange: () => void;
  isCustomRangeInvalid: boolean;
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
      <Card className="glass-card shadow-sm">
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
      <Card className="glass-card shadow-sm">
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

  const memberItems = summary.users.filter(
    (item) =>
      item.role !== "admin" ||
      item.todayUsage.totalTokens > 0 ||
      item.rangeUsage.totalTokens > 0,
  );
  const activeOpenAiAccounts = summary.openaiAccounts.filter(
    (item) => item.todayUsage.totalTokens > 0 || item.rangeUsage.totalTokens > 0,
  );
  const activeAggregateApis = summary.aggregateApis.filter(
    (item) => item.todayUsage.totalTokens > 0 || item.rangeUsage.totalTokens > 0,
  );
  const isTodayOnlyRange =
    summary.rangeStartTs === summary.todayStartTs &&
    summary.rangeEndTs === summary.todayEndTs;
  const rangeUsage = isTodayOnlyRange
    ? summary.todayUsage
    : sumDashboardTokenUsages(summary.dailyUsage.map((item) => item.usage));
  const listUsageForItem = <T extends {
    todayUsage: DashboardTokenUsage;
    rangeUsage: DashboardTokenUsage;
  }>(
    item: T,
  ) => (isTodayOnlyRange ? item.todayUsage : item.rangeUsage);
  const hasZoomWindow =
    summary.dailyUsage.length > 1 &&
    zoomWindow != null &&
    (zoomWindow.startIndex > 0 ||
      zoomWindow.endIndex < summary.dailyUsage.length - 1);
  const rangeBadgeLabel = isTodayOnlyRange ? t("今日") : t("所选区间");

  return (
    <Card className="glass-card overflow-hidden shadow-sm">
      <CardHeader className="flex flex-col gap-4">
        <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
          <div>
            <CardTitle className="flex items-center gap-2 text-base font-semibold">
              <LineChart className="h-4 w-4 text-primary" />
              {t("管理员用量分析")}
            </CardTitle>
            <p className="mt-1 text-xs text-muted-foreground">
              {t("按天、成员、OpenAI 账号和聚合 API 汇总 token 消耗")}
            </p>
            <div className="mt-2 text-[11px] text-muted-foreground">
              {t("当前区间")} {formatShortDateRange(summary.rangeStartTs, summary.rangeEndTs, locale)}
              {" · "}
              {t("图表区域支持鼠标滚轮缩放")}
            </div>
          </div>
          <div className="flex flex-col items-stretch gap-3 xl:items-end">
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
            <div className="rounded-lg bg-primary/10 px-3 py-2 text-right text-xs">
              <div className="text-muted-foreground">{rangeBadgeLabel}</div>
              <div className="font-semibold text-primary">
                {formatCompactTokenAmount(rangeUsage.totalTokens)}
              </div>
              <div className="text-muted-foreground">{formatUsd(rangeUsage.estimatedCostUsd)}</div>
            </div>
          </div>
        </div>
      </CardHeader>
      <CardContent className="grid gap-5 xl:grid-cols-[minmax(0,1.35fr)_minmax(320px,0.9fr)]">
        <div className="space-y-3">
          <DailyTokenLineChart
            points={summary.dailyUsage}
            zoomWindow={zoomWindow}
            onZoomWindowChange={setZoomWindow}
          />
          <div className="grid gap-3 text-xs sm:grid-cols-3">
            <div className="rounded-lg bg-background/30 px-3 py-2">
              <div className="text-muted-foreground">
                {isTodayOnlyRange ? t("今日请求") : t("区间请求")}
              </div>
              <div className="mt-1 font-semibold">
                {rangeUsage.requestCount} · {t("成功")} {rangeUsage.successCount}
              </div>
            </div>
            <div className="rounded-lg bg-background/30 px-3 py-2">
              <div className="text-muted-foreground">
                {isTodayOnlyRange ? t("输入 / 输出") : t("区间输入 / 输出")}
              </div>
              <div className="mt-1 font-semibold">
                {formatCompactTokenAmount(rangeUsage.inputTokens - rangeUsage.cachedInputTokens)} /{" "}
                {formatCompactTokenAmount(rangeUsage.outputTokens)}
              </div>
            </div>
            <div className="rounded-lg bg-background/30 px-3 py-2">
              <div className="text-muted-foreground">
                {isTodayOnlyRange ? t("缓存 / 推理") : t("区间缓存 / 推理")}
              </div>
              <div className="mt-1 font-semibold">
                {formatCompactTokenAmount(rangeUsage.cachedInputTokens)} /{" "}
                {formatCompactTokenAmount(rangeUsage.reasoningOutputTokens)}
              </div>
            </div>
          </div>
        </div>
        <div className="grid gap-4">
          <UsageRankList
            title={isTodayOnlyRange ? t("成员今日消耗") : t("成员区间消耗")}
            items={memberItems}
            labelForItem={userUsageName}
            emptyText={t("暂无成员消耗")}
            usageForItem={listUsageForItem}
          />
          <UsageRankList
            title={isTodayOnlyRange ? t("OpenAI 账号今日消耗") : t("OpenAI 账号区间消耗")}
            items={activeOpenAiAccounts}
            labelForItem={sourceUsageName}
            emptyText={t("暂无 OpenAI 账号消耗")}
            usageForItem={listUsageForItem}
          />
          <UsageRankList
            title={isTodayOnlyRange ? t("聚合 API 今日消耗") : t("聚合 API 区间消耗")}
            items={activeAggregateApis}
            labelForItem={sourceUsageName}
            emptyText={t("暂无聚合 API 消耗")}
            usageForItem={listUsageForItem}
          />
        </div>
      </CardContent>
    </Card>
  );
}

function AdminDashboard() {
  const { t } = useI18n();
  const { stats, currentAccount, recommendations, requestLogs, isLoading, isServiceReady } =
    useDashboardStats();
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
    isError: isAdminUsageError,
  } = useDashboardAdminUsageSummary(
    {
      startTs: adminUsageRangeParams.startTs,
      endTs: adminUsageRangeParams.endTs,
    },
    true,
  );
  const { data: quotaModelPools, isLoading: isQuotaModelPoolsLoading } = useQuery({
    queryKey: ["quota", "model-pools"],
    queryFn: () => quotaClient.modelPools(),
    enabled: isServiceReady,
    retry: 1,
  });
  usePageTransitionReady("/", !isServiceReady || !isLoading);

  const poolPrimary = stats.poolRemain?.primary ?? 0;
  const poolSecondary = stats.poolRemain?.secondary ?? 0;
  const allModelPoolItems = quotaModelPools?.items ?? [];
  const modelPoolItems = allModelPoolItems.slice(0, 8);
  const isCustomAdminUsageRangeInvalid =
    adminUsageRangePreset === "custom" &&
    (() => {
      const startTs = parseDateInputStartTs(adminUsageRangeStartInput);
      const endTs = parseDateInputEndTs(adminUsageRangeEndInput);
      return startTs == null || endTs == null || endTs <= startTs;
    })();

  return (
    <div className="space-y-6 animate-in fade-in duration-700">
      {isDirectAccountMode ? (
        <Alert className="border-amber-500/30 bg-amber-500/10">
          <AlertTriangle className="size-4" />
          <AlertTitle>{t("当前为账号直连模式")}</AlertTitle>
          <AlertDescription className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <span>
              {t("CodexManager 无法统计 CLI 请求日志和用量。")}
            </span>
            <a
              href={buildStaticRouteUrl("/platform-mode")}
              className="inline-flex h-7 w-fit items-center justify-center rounded-md border border-amber-500/40 bg-background/70 px-2.5 text-xs font-medium text-foreground transition-colors hover:bg-background"
            >
              {t("去切换为本地网关")}
            </a>
          </AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {isLoading ? (
          Array.from({ length: 4 }).map((_, index) => (
            <Skeleton key={index} className="h-36 w-full rounded-xl" />
          ))
        ) : (
          <>
            <MetricCard
              title={t("总账号数")}
              value={String(stats.total)}
              icon={Users}
              color="text-blue-500"
              sub={t("池中所有配置账号")}
              badge={
                isDirectAccountMode
                  ? t("账号直连模式下不可用")
                  : `${t("最近日志")} ${requestLogs.length} ${t("条")}`
              }
            />

            <StatProgressCard
              title={t("可用账号")}
              value={stats.available}
              total={stats.total}
              icon={CheckCircle2}
              color="text-green-500"
              sub={t("当前健康可调用的账号")}
            />

            <StatProgressCard
              title={t("不可用账号")}
              value={stats.unavailable}
              total={stats.total}
              icon={XCircle}
              color="text-red-500"
              sub={t("额度耗尽或授权失效")}
            />

            <Card className="overflow-hidden bg-primary/10 shadow-sm transition-colors">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium text-primary">{t("账号池剩余")}</CardTitle>
                <PieChart className="h-4 w-4 text-primary" />
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-[10px]">
                    <span className="text-muted-foreground">{t("5小时内")}</span>
                    <span className="font-bold">{formatPercent(stats.poolRemain?.primary)}</span>
                  </div>
                  <Progress
                    value={poolPrimary}
                    trackClassName={quotaTrackClass("green")}
                    indicatorClassName={quotaIndicatorClass("green")}
                  />
                </div>
                <div className="space-y-1.5">
                  <div className="flex items-center justify-between text-[10px]">
                    <span className="text-muted-foreground">{t("7天内")}</span>
                    <span className="font-bold">{formatPercent(stats.poolRemain?.secondary)}</span>
                  </div>
                  <Progress
                    value={poolSecondary}
                    trackClassName={quotaTrackClass("blue")}
                    indicatorClassName={quotaIndicatorClass("blue")}
                  />
                </div>
              </CardContent>
            </Card>
          </>
        )}
      </div>

      <DirectModeUnavailable active={isDirectAccountMode}>
        <AdminUsageAnalyticsCard
          summary={adminUsageSummary}
          isLoading={isLoading || isAdminUsageLoading}
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
            setAdminUsageRangeParams({
              startTs,
              endTs,
              startInput: adminUsageRangeStartInput,
              endInput: adminUsageRangeEndInput,
            });
          }}
          isCustomRangeInvalid={isCustomAdminUsageRangeInvalid}
        />
      </DirectModeUnavailable>

      <Card className="glass-card overflow-hidden shadow-sm">
        <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
          <div>
            <CardTitle className="text-sm font-medium">{t("模型额度池概览")}</CardTitle>
            <p className="mt-1 text-[10px] text-muted-foreground">
              {t("按模型管理中的排序权重展示")}
            </p>
          </div>
          <a
            href={buildStaticRouteUrl("/models")}
            className="inline-flex h-8 items-center gap-1.5 rounded-md border border-border/60 bg-background/40 px-3 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground"
          >
            {t("查看全部")}
            <ArrowRight className="h-3.5 w-3.5" />
          </a>
        </CardHeader>
        <CardContent>
          {isLoading || isQuotaModelPoolsLoading ? (
            <Skeleton className="h-24 w-full rounded-xl" />
          ) : modelPoolItems.length === 0 ? (
            <Empty className="min-h-28 border bg-background/35">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Database />
                </EmptyMedia>
                <EmptyTitle>{t("暂无可估算的模型额度池")}</EmptyTitle>
              </EmptyHeader>
            </Empty>
          ) : (
            <div className="space-y-3">
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                {modelPoolItems.map((item) => (
                  <div
                    key={item.model}
                    className="rounded-xl border border-border/50 bg-background/35 p-3"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="truncate font-mono text-sm font-semibold">
                          {item.model}
                        </div>
                        <div className="mt-1 text-[10px] text-muted-foreground">
                          {item.sourceCount} {t("个来源")}
                        </div>
                      </div>
                      <div className="shrink-0 text-right text-sm font-bold">
                        {item.totalRemainingTokens == null
                          ? "--"
                          : formatCompactTokenAmount(item.totalRemainingTokens)}
                      </div>
                    </div>
                    <div className="mt-3 grid gap-1 text-[10px] text-muted-foreground">
                      <div className="flex justify-between gap-2">
                        <span>{t("聚合 API")}</span>
                        <span className="font-medium text-foreground/70">
                          {item.aggregateRemainingTokens == null
                            ? "--"
                            : formatCompactTokenAmount(item.aggregateRemainingTokens)}
                        </span>
                      </div>
                      <div className="flex justify-between gap-2">
                        <span>{t("账号池")}</span>
                        <span className="font-medium text-foreground/70">
                          {item.accountEstimatedRemainingTokens == null
                            ? "--"
                            : formatCompactTokenAmount(item.accountEstimatedRemainingTokens)}
                        </span>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
              {allModelPoolItems.length > modelPoolItems.length ? (
                <div className="text-[11px] text-muted-foreground">
                  {t("已按排序权重展示前 {visible} 个，共 {total} 个模型；完整列表在模型管理页。", {
                    visible: modelPoolItems.length,
                    total: allModelPoolItems.length,
                  })}
                </div>
              ) : null}
            </div>
          )}
        </CardContent>
      </Card>

      <DirectModeUnavailable active={isDirectAccountMode}>
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          {[
            {
              title: t("今日Token"),
              value: formatCompactTokenAmount(stats.todayTokens),
              icon: Zap,
              color: "text-yellow-500",
              sub: t("输入 + 输出合计"),
            },
            {
              title: t("缓存Token"),
              value: formatCompactTokenAmount(stats.cachedTokens),
              icon: Database,
              color: "text-indigo-500",
              sub: t("上下文缓存命中"),
            },
            {
              title: t("推理Token"),
              value: formatCompactTokenAmount(stats.reasoningTokens),
              icon: BrainCircuit,
              color: "text-purple-500",
              sub: t("大模型思考过程"),
            },
            {
              title: t("预计费用"),
              value: formatUsd(stats.todayCost),
              icon: DollarSign,
              color: "text-emerald-500",
              sub: t("按官价估算"),
            },
          ].map((card) =>
            isLoading ? (
              <Skeleton key={card.title} className="h-32 w-full rounded-xl" />
            ) : (
              <MetricCard key={card.title} {...card} />
            ),
          )}
        </div>
      </DirectModeUnavailable>

      <div className="grid gap-6 md:grid-cols-2">
        <DirectModeUnavailable active={isDirectAccountMode}>
          <Card className="glass-card min-h-[300px] shadow-sm">
            <CardHeader className="flex flex-row items-center justify-between">
              <CardTitle className="text-base font-semibold">{t("当前活跃账号")}</CardTitle>
            </CardHeader>
            <CardContent className="flex min-h-[200px] flex-col justify-start">
              {isLoading ? (
                <div className="space-y-4">
                  <Skeleton className="h-28 w-full rounded-xl" />
                  <div className="grid grid-cols-2 gap-4">
                    <Skeleton className="h-32 w-full rounded-xl" />
                    <Skeleton className="h-32 w-full rounded-xl" />
                  </div>
                </div>
              ) : currentAccount ? (
                <div className="space-y-4">
                  <AccountHighlightCard
                    title={t("当前活跃账号")}
                    name={currentAccount.name}
                    subtitle={currentAccount.id}
                    tone="green"
                  />
                  <div className="grid grid-cols-2 gap-4 text-sm">
                    <div className="space-y-3 rounded-xl bg-muted/30 p-4">
                      <p className="text-xs text-muted-foreground">{t("5小时剩余")}</p>
                      <p className="text-lg font-bold">
                        {formatPercent(currentAccount.primaryRemainPercent)}
                      </p>
                      <PercentBar
                        label={t("剩余额度")}
                        value={currentAccount.primaryRemainPercent}
                        tone="green"
                      />
                    </div>
                    <div className="space-y-3 rounded-xl bg-muted/30 p-4">
                      <p className="text-xs text-muted-foreground">{t("7天剩余")}</p>
                      <p className="text-lg font-bold">
                        {formatPercent(currentAccount.secondaryRemainPercent)}
                      </p>
                      <PercentBar
                        label={t("剩余额度")}
                        value={currentAccount.secondaryRemainPercent}
                        tone="blue"
                      />
                    </div>
                  </div>
                </div>
              ) : (
                <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
                  <div className="rounded-full bg-accent/30 p-4 animate-pulse">
                    <Activity className="h-8 w-8 opacity-20" />
                  </div>
                  <p>{isServiceReady ? t("暂无可识别的活跃账号") : t("正在等待服务连接")}</p>
                </div>
              )}
            </CardContent>
          </Card>
        </DirectModeUnavailable>

        <Card className="glass-card min-h-[300px] shadow-sm">
          <CardHeader>
            <CardTitle className="text-base font-semibold">{t("智能推荐")}</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <p className="text-xs text-muted-foreground">
              {t("基于当前配额，系统会优先推荐剩余额度更高且仍可参与路由的账号。")}
            </p>
            {isLoading ? (
              <div className="space-y-4">
                <Skeleton className="h-28 w-full rounded-xl" />
                <Skeleton className="h-28 w-full rounded-xl" />
              </div>
            ) : recommendations.primaryPick || recommendations.secondaryPick ? (
              <>
                {recommendations.primaryPick ? (
                  <AccountHighlightCard
                    title={t("5小时优先账号")}
                    name={recommendations.primaryPick.name}
                    subtitle={recommendations.primaryPick.id}
                    tone="green"
                    progressLabel={t("剩余额度")}
                    progressValue={recommendations.primaryPick.primaryRemainPercent}
                  />
                ) : null}
                {recommendations.secondaryPick ? (
                  <AccountHighlightCard
                    title={t("7天优先账号")}
                    name={recommendations.secondaryPick.name}
                    subtitle={recommendations.secondaryPick.id}
                    tone="blue"
                    progressLabel={t("剩余额度")}
                    progressValue={recommendations.secondaryPick.secondaryRemainPercent}
                  />
                ) : null}
              </>
            ) : (
              <div className="rounded-xl bg-accent/20 p-4 text-sm text-muted-foreground">
                {isServiceReady ? t("当前没有可推荐的可用账号。") : t("正在等待服务连接。")}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
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
      <Card className="glass-card shadow-sm">
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

  const topModel = summary.availableModels[0];
  const successRate =
    summary.usageToday.successRate == null
      ? "--"
      : `${Math.round(summary.usageToday.successRate * 100)}%`;

  return (
    <div className="space-y-6 animate-in fade-in duration-700">
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
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
        <MetricCard
          title={t("可用模型")}
          value={String(summary.availableModels.length)}
          icon={ShieldCheck}
          color="text-purple-500"
          sub={topModel ? topModel.displayName || topModel.slug : t("暂无可用模型")}
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

      <div className="grid gap-6 xl:grid-cols-12">
        <MemberAvailableModelsCard summary={summary} className="xl:col-span-6" />
        <MemberRecentLogsCard logs={summary.recentLogs} className="xl:col-span-6" />
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
            className={cn("rounded-xl border px-3 py-2.5 text-sm", alertTone(alert))}
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
    <Card className={cn("glass-card shadow-sm", className)}>
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
          <div className="flex min-h-[180px] flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-border/60 bg-background/35 text-center text-sm text-muted-foreground">
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
    <Card className={cn("glass-card shadow-sm", className)}>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base font-semibold">
          <LineChart className="h-4 w-4 text-primary" />
          {t("用量趋势")}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="flex h-40 items-end gap-2 rounded-xl bg-background/30 px-3 py-4">
          {summary.usageTrend7d.map((item) => {
            const height = Math.max(6, Math.round((item.totalTokens / maxTokens) * 112));
            return (
              <div
                key={item.dayStartTs}
                className="flex min-w-0 flex-1 flex-col items-center justify-end gap-2"
              >
                <div
                  className="w-full rounded-t-md bg-primary/75 transition-all"
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

        <div className="grid gap-4 md:grid-cols-2">
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
          <TopUsageList
            title={t("Top Key")}
            icon={KeyRound}
            emptyText={t("暂无 Key 用量")}
            items={summary.topKeys
              .filter((item) => item.todayTokens > 0 || item.totalTokens > 0)
              .slice(0, 4)
              .map((item) => ({
                key: item.keyId,
                label: item.name || item.keyId,
                value: formatCompactTokenAmount(item.todayTokens || item.totalTokens),
                sub: item.modelSlug || "auto",
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

function MemberAvailableModelsCard({
  summary,
  className,
}: {
  summary: MemberDashboardSummary;
  className?: string;
}) {
  const { t } = useI18n();
  return (
    <Card className={cn("glass-card shadow-sm", className)}>
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3">
        <div>
          <CardTitle className="text-base font-semibold">{t("可用模型")}</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">
            {t("按模型管理排序展示前 8 个")}
          </p>
        </div>
        <a
          href={buildStaticRouteUrl("/models/")}
          className="inline-flex h-7 items-center gap-1 rounded-md border border-border/60 bg-background/40 px-2.5 text-[0.8rem] font-medium text-muted-foreground transition-colors hover:text-foreground"
        >
          {t("查看全部")}
          <ArrowRight className="h-3.5 w-3.5" />
        </a>
      </CardHeader>
      <CardContent>
        {summary.availableModels.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border/60 bg-background/35 px-4 py-5 text-sm text-muted-foreground">
            {t("暂无可用模型")}
          </div>
        ) : (
          <div className="divide-y divide-border/40">
            {summary.availableModels.slice(0, 8).map((model) => (
              <div key={model.slug} className="grid gap-2 py-3 sm:grid-cols-[1fr_auto]">
                <div className="min-w-0">
                  <div className="truncate font-mono text-sm font-semibold">
                    {model.displayName || model.slug}
                  </div>
                  <div className="mt-1 truncate text-xs text-muted-foreground">
                    {model.slug}
                  </div>
                </div>
                <div className="text-left text-xs text-muted-foreground sm:text-right">
                  <div>{t(modelPriceSummary(model))}</div>
                  <div className="mt-1">
                    {model.contextWindow
                      ? `${formatCompactTokenAmount(model.contextWindow)} context`
                      : "context --"}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function MemberRecentLogsCard({
  logs,
  className,
}: {
  logs: RequestLog[];
  className?: string;
}) {
  const { t } = useI18n();
  return (
    <Card className={cn("glass-card shadow-sm", className)}>
      <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-3">
        <div>
          <CardTitle className="text-base font-semibold">{t("近期请求")}</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">{t("最近 8 条个人 Key 请求")}</p>
        </div>
        <a
          href={buildStaticRouteUrl("/logs/")}
          className="inline-flex h-7 items-center gap-1 rounded-md border border-border/60 bg-background/40 px-2.5 text-[0.8rem] font-medium text-muted-foreground transition-colors hover:text-foreground"
        >
          {t("查看全部")}
          <ArrowRight className="h-3.5 w-3.5" />
        </a>
      </CardHeader>
      <CardContent>
        {logs.length === 0 ? (
          <div className="flex min-h-[180px] flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-border/60 bg-background/35 text-sm text-muted-foreground">
            <Clock3 className="h-8 w-8 opacity-30" />
            <span>{t("暂无请求日志")}</span>
          </div>
        ) : (
          <div className="divide-y divide-border/40">
            {logs.map((log) => (
              <div
                key={log.id}
                className="grid gap-2 py-3 sm:grid-cols-[minmax(0,1fr)_auto]"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <Badge
                      variant={
                        log.statusCode && log.statusCode >= 400
                          ? "destructive"
                          : "secondary"
                      }
                    >
                      {log.statusCode || "-"}
                    </Badge>
                    <span className="truncate font-mono text-sm font-semibold">
                      {log.model || "unknown"}
                    </span>
                  </div>
                  <div className="mt-1 truncate text-xs text-muted-foreground">
                    {t(formatDateTime(log.createdAt))}
                  </div>
                </div>
                <div className="grid grid-cols-3 gap-3 text-xs text-muted-foreground sm:text-right">
                  <span>{formatCompactTokenAmount(log.totalTokens)}</span>
                  <span>{formatDuration(log.durationMs)}</span>
                  <span>{formatUsd(log.estimatedCostUsd)}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
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
