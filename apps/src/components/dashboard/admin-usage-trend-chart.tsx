"use client";

import {
  useEffect,
  useMemo,
  useState,
  type WheelEvent as ReactWheelEvent,
} from "react";
import { LoaderCircle, RotateCcw } from "lucide-react";
import {
  CartesianGrid,
  ComposedChart,
  Line,
  XAxis,
  YAxis,
} from "recharts";
import { Button } from "@/components/ui/button";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import {
  estimateChartYAxisWidth,
  formatCompactTokenAmount,
} from "@/lib/dashboard/format";
import type { AppLocale } from "@/lib/i18n/config";
import { useI18n } from "@/lib/i18n/provider";
import type {
  DashboardAdminUsageSummary,
  DashboardTokenUsage,
  DashboardUsageSeriesPoint,
} from "@/types";

export type AdminUsageGranularity = "day" | "hour";
type AdminUsageMetric = "tokens" | "requests";

const MODEL_SERIES_COLORS = [
  "var(--usage-series-1)",
  "var(--usage-series-2)",
  "var(--usage-series-3)",
  "var(--usage-series-4)",
  "var(--usage-series-5)",
  "var(--usage-series-6)",
  "var(--usage-series-7)",
  "var(--usage-series-8)",
] as const;
const MAX_SELECTED_MODELS = 5;

const SUPPORTED_INTL_LOCALES = ["zh-CN", "en-US", "ru-RU", "ko-KR"] as const;
const INTL_LOCALE_BY_APP_LOCALE: Record<Exclude<AppLocale, "zh-CN">, string> = {
  en: "en-US",
  ru: "ru-RU",
  ko: "ko-KR",
};

function intlLocaleFromAppLocale(locale: AppLocale): string {
  if (
    SUPPORTED_INTL_LOCALES.includes(
      locale as (typeof SUPPORTED_INTL_LOCALES)[number],
    )
  ) {
    return locale;
  }
  return INTL_LOCALE_BY_APP_LOCALE[locale as Exclude<AppLocale, "zh-CN">] ?? "zh-CN";
}

function formatBucketLabel(
  value: number,
  granularity: AdminUsageGranularity,
  locale: AppLocale,
): string {
  const date = new Date(value * 1_000);
  if (Number.isNaN(date.getTime())) return "--";
  return new Intl.DateTimeFormat(intlLocaleFromAppLocale(locale), {
    month: "2-digit",
    day: "2-digit",
    ...(granularity === "hour"
      ? { hour: "2-digit", minute: "2-digit", hour12: false }
      : {}),
  }).format(date);
}

function metricValue(usage: DashboardTokenUsage, metric: AdminUsageMetric): number {
  return metric === "requests" ? usage.requestCount : usage.totalTokens;
}

function fallbackSeries(summary: DashboardAdminUsageSummary): DashboardUsageSeriesPoint[] {
  if (summary.seriesUsage.length > 0) {
    return summary.seriesUsage;
  }
  return summary.dailyUsage.map((point) => ({
    bucketStartTs: point.dayStartTs,
    bucketEndTs: point.dayEndTs,
    usage: point.usage,
  }));
}

export function AdminUsageTrendChart({
  summary,
  granularity,
  onGranularityChange,
  hourlyAvailable,
  isRefreshing,
}: {
  summary: DashboardAdminUsageSummary;
  granularity: AdminUsageGranularity;
  onGranularityChange: (granularity: AdminUsageGranularity) => void;
  hourlyAvailable: boolean;
  isRefreshing: boolean;
}) {
  const { t, locale } = useI18n();
  const [metric, setMetric] = useState<AdminUsageMetric>("tokens");
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [showTotal, setShowTotal] = useState(false);
  const [zoomWindow, setZoomWindow] = useState<{
    startIndex: number;
    endIndex: number;
  } | null>(null);

  const availableModelNames = useMemo(
    () => summary.modelUsage.map((series) => series.model),
    [summary.modelUsage],
  );
  const activeModels = useMemo(() => {
    const available = new Set(availableModelNames);
    const retained = selectedModels.filter((model) => available.has(model));
    return retained.length > 0
      ? retained.slice(0, MAX_SELECTED_MODELS)
      : availableModelNames.slice(0, Math.min(3, MAX_SELECTED_MODELS));
  }, [availableModelNames, selectedModels]);
  const activeModelSet = useMemo(() => new Set(activeModels), [activeModels]);
  const modelDefinitions = useMemo(
    () =>
      activeModels.map((model) => {
        const stableIndex = Math.max(0, availableModelNames.indexOf(model));
        return {
          model,
          key: `model${stableIndex}`,
          color: MODEL_SERIES_COLORS[stableIndex % MODEL_SERIES_COLORS.length],
        };
      }),
    [activeModels, availableModelNames],
  );
  const chartConfig = useMemo(() => {
    const config: ChartConfig = {
      total: {
        label: t("全部模型"),
        color: "var(--usage-total-line)",
      },
    };
    for (const definition of modelDefinitions) {
      config[definition.key] = {
        label: definition.model,
        color: definition.color,
      };
    }
    return config;
  }, [modelDefinitions, t]);

  const chartData = useMemo(() => {
    const points = fallbackSeries(summary);
    const modelPointMaps = new Map(
      summary.modelUsage.map((series) => [
        series.model,
        new Map(series.points.map((point) => [point.bucketStartTs, point.usage])),
      ]),
    );
    return points.map((point) => {
      const row: Record<string, number | string> = {
        bucketStartTs: point.bucketStartTs,
        label: formatBucketLabel(point.bucketStartTs, granularity, locale),
        total: metricValue(point.usage, metric),
      };
      for (const definition of modelDefinitions) {
        const usage = modelPointMaps
          .get(definition.model)
          ?.get(point.bucketStartTs);
        row[definition.key] = usage ? metricValue(usage, metric) : 0;
      }
      return row;
    });
  }, [granularity, locale, metric, modelDefinitions, summary]);

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
  const visibleEndIndex =
    normalizedZoomWindow?.endIndex ?? Math.max(0, chartData.length - 1);
  const visibleChartData = useMemo(
    () => chartData.slice(visibleStartIndex, visibleEndIndex + 1),
    [chartData, visibleEndIndex, visibleStartIndex],
  );
  const hasZoomWindow =
    chartData.length > 1 &&
    (visibleStartIndex > 0 || visibleEndIndex < chartData.length - 1);

  useEffect(() => {
    let active = true;
    queueMicrotask(() => {
      if (active) setZoomWindow(null);
    });
    return () => {
      active = false;
    };
  }, [summary.rangeEndTs, summary.rangeStartTs, summary.seriesBucketSeconds]);

  const formatMetric = (value: number) =>
    metric === "requests"
      ? new Intl.NumberFormat(intlLocaleFromAppLocale(locale), {
          notation: "compact",
          maximumFractionDigits: 1,
        }).format(Math.max(0, value))
      : formatCompactTokenAmount(value);
  const yAxisWidth = estimateChartYAxisWidth(
    [
      0,
      ...visibleChartData.flatMap((row) => [
        ...(showTotal ? [Number(row.total)] : []),
        ...modelDefinitions.map((definition) => Number(row[definition.key] ?? 0)),
      ]),
    ],
    formatMetric,
  );

  const handleWheelZoom = (event: ReactWheelEvent<HTMLDivElement>) => {
    if (chartData.length <= 2) return;
    event.preventDefault();
    const currentCount = visibleEndIndex - visibleStartIndex + 1;
    const minCount = Math.min(granularity === "hour" ? 8 : 3, chartData.length);
    const step = Math.max(1, Math.round(currentCount * 0.2));
    const nextCount =
      event.deltaY < 0
        ? Math.max(minCount, currentCount - step)
        : Math.min(chartData.length, currentCount + step);
    if (nextCount === currentCount) return;

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
    setZoomWindow({ startIndex: nextStartIndex, endIndex: nextEndIndex });
  };

  const toggleModel = (model: string) => {
    if (activeModelSet.has(model)) {
      if (activeModels.length <= 1) return;
      setSelectedModels(activeModels.filter((item) => item !== model));
      return;
    }
    if (activeModels.length >= MAX_SELECTED_MODELS) return;
    setSelectedModels([...activeModels, model]);
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex flex-wrap items-center gap-2">
          <div
            className="inline-flex rounded-md border border-border/70 bg-background/40 p-0.5"
            role="group"
            aria-label={t("时间粒度")}
          >
            {(["day", "hour"] as const).map((value) => (
              <Button
                key={value}
                type="button"
                size="sm"
                variant={granularity === value ? "default" : "ghost"}
                className="h-7 px-2.5 text-xs"
                aria-pressed={granularity === value}
                disabled={value === "hour" && !hourlyAvailable}
                onClick={() => {
                  setZoomWindow(null);
                  onGranularityChange(value);
                }}
              >
                {value === "day" ? t("按天") : t("按小时")}
              </Button>
            ))}
          </div>
          <div
            className="inline-flex rounded-md border border-border/70 bg-background/40 p-0.5"
            role="group"
            aria-label={t("指标")}
          >
            {(["tokens", "requests"] as const).map((value) => (
              <Button
                key={value}
                type="button"
                size="sm"
                variant={metric === value ? "default" : "ghost"}
                className="h-7 px-2.5 text-xs"
                aria-pressed={metric === value}
                onClick={() => setMetric(value)}
              >
                {value === "tokens" ? t("Token") : t("请求数")}
              </Button>
            ))}
          </div>
          {hasZoomWindow ? (
            <Button
              type="button"
              size="sm"
              variant="outline"
              className="h-8 gap-1.5 text-xs"
              onClick={() => setZoomWindow(null)}
            >
              <RotateCcw className="size-3.5" />
              {t("重置缩放")}
            </Button>
          ) : null}
        </div>
        <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
          {isRefreshing ? (
            <span className="inline-flex items-center gap-1.5 text-primary">
              <LoaderCircle className="size-3 animate-spin" />
              {t("正在更新曲线")}
            </span>
          ) : null}
          <span>
            {hourlyAvailable
              ? t("滚轮缩放时间区间，点击模型切换曲线")
              : t("小时曲线最多支持 31 天区间")}
          </span>
        </div>
      </div>

      {availableModelNames.length > 0 ? (
        <div className="flex flex-wrap items-center gap-1.5" aria-label={t("模型曲线")}>
          <Button
            type="button"
            size="sm"
            variant={showTotal ? "secondary" : "outline"}
            className="h-7 gap-1.5 px-2 text-xs"
            aria-pressed={showTotal}
            onClick={() => setShowTotal((value) => !value)}
          >
            <span
              className="h-0.5 w-3 shrink-0 rounded-full bg-(--usage-total-line)"
              aria-hidden="true"
            />
            {t("全部模型")}
          </Button>
          {availableModelNames.map((model, index) => {
            const selectedIndex = activeModels.indexOf(model);
            const isSelected = selectedIndex >= 0;
            const disabled = !isSelected && activeModels.length >= MAX_SELECTED_MODELS;
            const color = MODEL_SERIES_COLORS[index % MODEL_SERIES_COLORS.length];
            return (
              <Button
                key={model}
                type="button"
                size="sm"
                variant={isSelected ? "secondary" : "outline"}
                className="h-7 max-w-full gap-1.5 px-2 text-xs"
                aria-pressed={isSelected}
                disabled={disabled}
                onClick={() => toggleModel(model)}
              >
                <span
                  className="size-2 shrink-0 rounded-full"
                  style={{ backgroundColor: color }}
                  aria-hidden="true"
                />
                <span className="truncate">{model}</span>
              </Button>
            );
          })}
        </div>
      ) : null}

      <div
        className="mission-panel rounded-lg border border-primary/20 bg-background/30 shadow-[inset_0_1px_0_rgb(255_255_255/0.06)]"
        onWheel={handleWheelZoom}
      >
        {chartData.length === 0 ? (
          <div className="flex h-64 items-center justify-center text-sm text-muted-foreground">
            {t("暂无模型用量数据")}
          </div>
        ) : (
          <ChartContainer
            config={chartConfig}
            className="h-72 w-full rounded-md bg-transparent p-3"
            initialDimension={{ width: 720, height: 288 }}
            aria-label={t("模型用量趋势图")}
          >
            <ComposedChart
              accessibilityLayer
              data={visibleChartData}
              margin={{ top: 18, right: 14, left: 10, bottom: 4 }}
            >
              <CartesianGrid
                vertical={false}
                stroke="rgb(var(--primary-rgb) / 0.16)"
                strokeDasharray="4 8"
              />
              <XAxis
                dataKey="label"
                tickLine={false}
                axisLine={false}
                tickMargin={10}
                minTickGap={granularity === "hour" ? 36 : 18}
              />
              <YAxis
                tickLine={false}
                axisLine={false}
                tickMargin={10}
                width={yAxisWidth}
                tickFormatter={(value) => formatMetric(Number(value))}
              />
              <ChartTooltip
                cursor={{ stroke: "var(--border)", strokeWidth: 1 }}
                content={
                  <ChartTooltipContent
                    indicator="line"
                    labelFormatter={(value) => value}
                    formatter={(value, name) =>
                      Number(value) === 0 && String(name) !== "total" ? null : (
                        <div className="flex min-w-40 items-center justify-between gap-4">
                          <span className="truncate text-muted-foreground">
                            {String(name) === "total" ? t("全部模型") : String(name)}
                          </span>
                          <span className="font-mono font-medium text-foreground">
                            {formatMetric(Number(value))}
                          </span>
                        </div>
                      )
                    }
                  />
                }
              />
              {showTotal ? (
                <Line
                  dataKey="total"
                  name="total"
                  type="linear"
                  stroke="var(--color-total)"
                  strokeWidth={1.5}
                  strokeDasharray="7 5"
                  dot={false}
                  activeDot={{ r: 4, strokeWidth: 2 }}
                />
              ) : null}
              {modelDefinitions.map((definition) => (
                <Line
                  key={definition.model}
                  dataKey={definition.key}
                  name={definition.model}
                  type="linear"
                  stroke={`var(--color-${definition.key})`}
                  strokeWidth={2}
                  dot={false}
                  activeDot={{ r: 4, strokeWidth: 2 }}
                  connectNulls
                />
              ))}
            </ComposedChart>
          </ChartContainer>
        )}
      </div>
    </div>
  );
}
