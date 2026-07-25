"use client";

import {
  useEffect,
  useMemo,
  useState,
  type WheelEvent as ReactWheelEvent,
} from "react";
import { Check, LoaderCircle, RotateCcw } from "lucide-react";
import {
  Brush,
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
  const [hoveredModel, setHoveredModel] = useState<string | null>(null);
  const [zoomWindow, setZoomWindow] = useState<{
    startIndex: number;
    endIndex: number;
  } | null>(null);

  const rankedModelSeries = useMemo(
    () =>
      [...summary.modelUsage].sort((left, right) => {
        const valueDifference =
          metricValue(right.usage, metric) - metricValue(left.usage, metric);
        return valueDifference !== 0
          ? valueDifference
          : left.model.localeCompare(right.model);
      }),
    [metric, summary.modelUsage],
  );
  const availableModelNames = useMemo(
    () => rankedModelSeries.map((series) => series.model),
    [rankedModelSeries],
  );
  const stableModelIndexByName = useMemo(
    () =>
      new Map(
        summary.modelUsage.map((series, index) => [series.model, index] as const),
      ),
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
        const stableIndex = stableModelIndexByName.get(model) ?? 0;
        return {
          model,
          key: `model${stableIndex}`,
          color: MODEL_SERIES_COLORS[stableIndex % MODEL_SERIES_COLORS.length],
        };
      }),
    [activeModels, stableModelIndexByName],
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
  const visibleRangeLabel =
    visibleChartData.length > 0
      ? `${String(visibleChartData[0]?.label ?? "")} – ${String(
          visibleChartData[visibleChartData.length - 1]?.label ?? "",
        )}`
      : "";

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
  const totalMetricForRange = fallbackSeries(summary).reduce(
    (total, point) => total + metricValue(point.usage, metric),
    0,
  );

  return (
    <div className="space-y-3">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
        <div className="flex flex-wrap items-center gap-2">
          <div
            className="inline-flex rounded-md border border-border/70 bg-background/40 p-0.5"
            role="group"
            aria-label={t("时间粒度")}
            title={
              hourlyAvailable ? undefined : t("小时曲线最多支持 31 天区间")
            }
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
        {isRefreshing ? (
          <span className="inline-flex items-center gap-1.5 text-[11px] text-primary">
            <LoaderCircle className="size-3 animate-spin" />
            {t("正在更新曲线")}
          </span>
        ) : null}
      </div>

      {availableModelNames.length > 0 ? (
        <div className="mission-panel space-y-2.5 rounded-lg border border-primary/15 bg-background/25 p-2.5 shadow-[inset_0_1px_0_rgb(255_255_255/0.05)]">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <div className="rounded-md bg-primary/8 px-2 py-1 text-[11px] font-medium text-foreground">
              {t("模型曲线")} · {t("已选 {selected}/{max}", {
                selected: activeModels.length,
                max: MAX_SELECTED_MODELS,
              })}
            </div>
            {selectedModels.length > 0 ? (
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="h-7 gap-1.5 border-primary/25 bg-background/45 px-2 text-xs shadow-sm"
                onClick={() => setSelectedModels([])}
              >
                <RotateCcw className="size-3" />
                {t("恢复默认")}
              </Button>
            ) : null}
            <span className="text-[11px] text-muted-foreground">
              {hourlyAvailable
                ? t("拖动底部时间滑块调整范围，滚轮可快速缩放")
                : t("小时曲线最多支持 31 天区间")}
            </span>
          </div>
          <div
            className="flex max-w-full flex-nowrap items-center gap-2 overflow-x-auto pb-1 sm:flex-wrap sm:overflow-visible"
            aria-label={t("模型曲线")}
          >
            <Button
              type="button"
              size="sm"
              variant="outline"
              className={
                showTotal
                  ? "h-9 shrink-0 gap-1.5 border-primary/50 bg-primary/10 px-2.5 text-xs text-foreground shadow-sm"
                  : "h-9 shrink-0 gap-1.5 border-border/60 bg-background/30 px-2.5 text-xs text-muted-foreground"
              }
              aria-pressed={showTotal}
              onClick={() => setShowTotal((value) => !value)}
            >
              <span className="flex size-4 shrink-0 items-center justify-center rounded border border-(--usage-total-line) bg-background/70">
                {showTotal ? (
                  <Check className="size-3 text-(--usage-total-line)" />
                ) : (
                  <span className="size-1.5 rounded-full bg-(--usage-total-line)" />
                )}
              </span>
              {t("全部模型")}
            </Button>
            {rankedModelSeries.map((series) => {
              const model = series.model;
              const isSelected = activeModelSet.has(model);
              const disabled =
                !isSelected && activeModels.length >= MAX_SELECTED_MODELS;
              const stableIndex = stableModelIndexByName.get(model) ?? 0;
              const color =
                MODEL_SERIES_COLORS[stableIndex % MODEL_SERIES_COLORS.length];
              const value = metricValue(series.usage, metric);
              const share =
                totalMetricForRange > 0 ? (value / totalMetricForRange) * 100 : 0;
              return (
                <Button
                  key={model}
                  type="button"
                  size="sm"
                  variant="outline"
                  className={
                    isSelected
                      ? "h-9 max-w-[19rem] shrink-0 gap-1.5 bg-background/70 px-2.5 text-xs text-foreground shadow-sm"
                      : "h-9 max-w-[19rem] shrink-0 gap-1.5 border-border/60 bg-background/25 px-2.5 text-xs text-muted-foreground opacity-75 hover:opacity-100"
                  }
                  style={
                    isSelected
                      ? {
                          borderColor: color,
                          background: `color-mix(in srgb, ${color} 11%, var(--background))`,
                          boxShadow: `inset 0 0 0 1px color-mix(in srgb, ${color} 28%, transparent), 0 3px 10px rgb(15 23 42 / 0.08)`,
                        }
                      : undefined
                  }
                  aria-label={`${model}: ${formatMetric(value)}, ${share.toFixed(1)}%`}
                  aria-pressed={isSelected}
                  disabled={disabled}
                  title={`${model} · ${formatMetric(value)} · ${share.toFixed(1)}%`}
                  onClick={() => toggleModel(model)}
                  onMouseEnter={() => isSelected && setHoveredModel(model)}
                  onMouseLeave={() => setHoveredModel(null)}
                  onFocus={() => isSelected && setHoveredModel(model)}
                  onBlur={() => setHoveredModel(null)}
                >
                  <span
                    className="flex size-4 shrink-0 items-center justify-center rounded border bg-background/75"
                    style={{ borderColor: color }}
                    aria-hidden="true"
                  >
                    {isSelected ? (
                      <Check className="size-3" style={{ color }} />
                    ) : (
                      <span
                        className="size-1.5 rounded-full"
                        style={{ backgroundColor: color }}
                      />
                    )}
                  </span>
                  <span className="max-w-32 truncate">{model}</span>
                  <span
                    className={
                      isSelected
                        ? "font-mono text-[10px] text-foreground/70"
                        : "font-mono text-[10px] text-muted-foreground"
                    }
                  >
                    {formatMetric(value)} · {share.toFixed(0)}%
                  </span>
                </Button>
              );
            })}
          </div>
          {activeModels.length >= MAX_SELECTED_MODELS ? (
            <p className="text-[11px] text-muted-foreground">
              {t("最多同时比较 {count} 个模型", {
                count: MAX_SELECTED_MODELS,
              })}
            </p>
          ) : null}
        </div>
      ) : null}

      <div
        className="mission-panel overflow-hidden rounded-lg border border-primary/20 bg-gradient-to-b from-background/45 to-background/20 shadow-[inset_0_1px_0_rgb(255_255_255/0.06)]"
        onWheel={handleWheelZoom}
      >
        {chartData.length === 0 ? (
          <div className="flex h-64 items-center justify-center text-sm text-muted-foreground">
            {t("暂无模型用量数据")}
          </div>
        ) : (
          <ChartContainer
            config={chartConfig}
            className="h-80 w-full rounded-md bg-transparent p-3"
            initialDimension={{ width: 720, height: 320 }}
            aria-label={t("模型用量趋势图")}
          >
            <ComposedChart
              accessibilityLayer
              data={chartData}
              margin={{ top: 18, right: 14, left: 10, bottom: 8 }}
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
                itemSorter={(item) => -Number(item.value ?? 0)}
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
                  type="monotone"
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
                  type="monotone"
                  stroke={`var(--color-${definition.key})`}
                  strokeWidth={2.25}
                  opacity={
                    hoveredModel == null || hoveredModel === definition.model
                      ? 1
                      : 0.18
                  }
                  dot={false}
                  activeDot={{ r: 4, strokeWidth: 2 }}
                  connectNulls
                />
              ))}
              <Brush
                dataKey="label"
                height={26}
                travellerWidth={10}
                startIndex={visibleStartIndex}
                endIndex={visibleEndIndex}
                stroke="rgb(var(--primary-rgb) / 0.55)"
                fill="var(--card)"
                onChange={(nextWindow) => {
                  if (
                    typeof nextWindow.startIndex === "number" &&
                    typeof nextWindow.endIndex === "number"
                  ) {
                    setZoomWindow({
                      startIndex: nextWindow.startIndex,
                      endIndex: nextWindow.endIndex,
                    });
                  }
                }}
              />
            </ComposedChart>
          </ChartContainer>
        )}
      </div>
      {visibleRangeLabel ? (
        <div className="text-right text-[11px] text-muted-foreground">
          {t("当前可视区间")}: {visibleRangeLabel}
        </div>
      ) : null}
    </div>
  );
}
