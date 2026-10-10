"use client";

import { AlertTriangle, Check, ArrowRight, PieChart } from "lucide-react";
import { buttonVariants } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import {
  formatCompactTokenAmount,
  formatPercent,
} from "@/lib/dashboard/format";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";

interface DashboardGatewayStatusProps {
  connected: boolean;
  directMode: boolean;
  stats: {
    total: number;
    available: number;
    unavailable: number;
    todayTokens: number;
    cachedTokens: number;
    reasoningTokens: number;
    todayCost: number;
  };
  isLoading: boolean;
}

interface DashboardPoolRemainingProps {
  primary: number | null;
  secondary: number | null;
  primaryKnownCount: number;
  primaryBucketCount: number;
  secondaryKnownCount: number;
  secondaryBucketCount: number;
  isLoading: boolean;
}

function formatUsd(value: number): string {
  return `$${Math.max(0, value || 0).toFixed(2)}`;
}

function StatusMetric({
  label,
  value,
  tone = "text-foreground",
  detail,
}: {
  label: string;
  value: string;
  tone?: string;
  detail?: string;
}) {
  return (
    <div className="flex min-w-0 flex-col justify-center gap-0.5">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <span
        className={cn(
          "truncate font-mono text-lg font-semibold leading-none tabular-nums",
          tone,
        )}
      >
        {value}
      </span>
      {detail ? (
        <span className="truncate text-xs text-muted-foreground" title={detail}>
          {detail}
        </span>
      ) : null}
    </div>
  );
}

export function DashboardGatewayStatus({
  connected,
  directMode,
  stats,
  isLoading,
}: DashboardGatewayStatusProps) {
  const { t } = useI18n();

  if (isLoading) {
    return <Skeleton className="h-[116px] rounded-lg xl:h-[124px]" />;
  }

  const title = connected ? t("网关运行正常") : t("正在等待网关连接");
  const description = directMode
    ? t("本机 Codex 的直连请求不会经过网关；下方仍展示 CodexManager 已记录的网关流量。")
    : connected
      ? t("近期请求路由稳定，账号池可正常参与调度。")
      : t("正在等待服务连接。");
  const actionHref = "/logs";
  const actionLabel = directMode ? t("打开请求日志") : t("查看异常请求");

  return (
    <Card className="dashboard-primary-panel routing-command-card glass-card overflow-hidden py-0">
      <CardContent className="p-0">
        <div className="flex min-h-[52px] flex-col gap-2 px-4 py-1.5 md:flex-row md:items-center md:justify-between xl:px-5">
          <div className="flex min-w-0 items-center gap-2.5">
            <div
              className={cn(
                "flex h-8 w-8 shrink-0 items-center justify-center rounded-full border-2 bg-background/75 shadow-[0_8px_24px_-18px_currentColor]",
                connected
                  ? "border-emerald-500 text-emerald-600"
                  : "border-amber-500/45 text-amber-600",
              )}
            >
              {connected ? (
                <Check className="h-4 w-4 stroke-[2.5]" />
              ) : (
                <AlertTriangle className="h-4 w-4" />
              )}
            </div>
            <div className="min-w-0">
              <h2 className="text-lg font-semibold leading-tight text-foreground">
                {title}
              </h2>
              <p className="mt-0.5 max-w-2xl text-xs leading-5 text-muted-foreground">
                {description}
              </p>
            </div>
          </div>
          <a
            href={buildStaticRouteUrl(actionHref)}
            className={cn(
              buttonVariants({ size: "lg" }),
              "command-center-primary-action h-8 min-w-[124px] shrink-0 rounded-md px-3 text-sm",
            )}
          >
            {actionLabel}
            <ArrowRight className="ml-1 h-3.5 w-3.5" />
          </a>
        </div>

        <div className="grid grid-cols-2 gap-x-4 gap-y-2 border-t border-border/55 px-4 py-2 md:grid-cols-4 xl:px-5">
          <StatusMetric
            label={t("可用账号")}
            value={`${stats.available} / ${stats.total}`}
            tone="text-emerald-600"
          />
          <StatusMetric
            label={t("异常")}
            value={String(stats.unavailable)}
            tone={stats.unavailable > 0 ? "text-rose-600" : "text-foreground"}
          />
          <StatusMetric
            label={`${t("今日")} Token`}
            value={formatCompactTokenAmount(stats.todayTokens)}
            detail={`${t("缓存")} ${formatCompactTokenAmount(stats.cachedTokens)} · ${t("推理")} ${formatCompactTokenAmount(stats.reasoningTokens)}`}
          />
          <StatusMetric
            label={t("预计费用")}
            value={formatUsd(stats.todayCost)}
          />
        </div>
      </CardContent>
    </Card>
  );
}

function PoolBucket({
  label,
  value,
  knownCount,
  bucketCount,
  tone,
}: {
  label: string;
  value: number | null;
  knownCount: number;
  bucketCount: number;
  tone: "emerald" | "blue";
}) {
  const normalizedValue = value == null ? 0 : Math.max(0, Math.min(100, value));
  const isEmerald = tone === "emerald";

  return (
    <div className="min-w-0">
      <div className="mb-1 flex items-center justify-between gap-3 text-xs xl:text-sm">
        <span className="font-medium text-muted-foreground">{label}</span>
        <span
          className={cn(
            "font-mono font-semibold",
            isEmerald ? "text-emerald-600" : "text-blue-600",
          )}
        >
          {formatPercent(value)}
        </span>
      </div>
      <Progress
        value={normalizedValue}
        className="gap-0"
        trackClassName={cn(
          "h-1.5 xl:h-2",
          isEmerald ? "bg-emerald-500/18" : "bg-blue-500/18",
        )}
        indicatorClassName={isEmerald ? "bg-emerald-500" : "bg-blue-500"}
      />
      <div className="mt-1 truncate font-mono text-[10px] text-muted-foreground xl:text-xs">
        {knownCount}/{bucketCount}
      </div>
    </div>
  );
}

export function DashboardPoolRemaining({
  primary,
  secondary,
  primaryKnownCount,
  primaryBucketCount,
  secondaryKnownCount,
  secondaryBucketCount,
  isLoading,
}: DashboardPoolRemainingProps) {
  const { t } = useI18n();

  if (isLoading) {
    return <Skeleton className="h-[60px] rounded-lg" />;
  }

  return (
    <Card className="dashboard-pool-remaining dashboard-primary-panel glass-card overflow-hidden py-0">
      <CardContent className="grid gap-2 px-4 py-1.5 md:grid-cols-[160px_minmax(0,1fr)] md:items-center xl:grid-cols-[180px_minmax(0,1fr)_minmax(0,1fr)]">
        <div className="flex min-w-0 items-center gap-3">
          <PieChart className="h-5 w-5 shrink-0 text-emerald-600 xl:h-6 xl:w-6" />
          <span className="truncate text-sm font-semibold text-foreground">
            {t("账号池剩余")}
          </span>
        </div>
        <div className="grid min-w-0 gap-3 sm:grid-cols-2 md:col-span-1 xl:col-span-2">
          <PoolBucket
            label={t("5小时内")}
            value={primary}
            knownCount={primaryKnownCount}
            bucketCount={primaryBucketCount}
            tone="emerald"
          />
          <PoolBucket
            label={t("7天内")}
            value={secondary}
            knownCount={secondaryKnownCount}
            bucketCount={secondaryBucketCount}
            tone="blue"
          />
        </div>
      </CardContent>
    </Card>
  );
}
