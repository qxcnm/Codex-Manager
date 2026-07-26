"use client";

import { AlertTriangle, Check, ArrowRight, PieChart } from "lucide-react";
import { buttonVariants } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
  className,
  valueClassName,
}: {
  label: string;
  value: string;
  tone?: string;
  className?: string;
  valueClassName?: string;
}) {
  return (
    <div
      className={cn(
        "flex min-h-[76px] flex-col justify-center border-t border-border/55 px-5 first:border-t-0 sm:border-l sm:border-t-0 sm:first:border-l-0 xl:min-h-[108px] xl:px-8",
        className,
      )}
    >
      <span className="truncate text-xs text-muted-foreground xl:text-sm">{label}</span>
      <span
        className={cn(
          "mt-1 truncate font-mono text-[22px] font-semibold leading-none xl:text-[30px]",
          tone,
          valueClassName,
        )}
      >
        {value}
      </span>
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
    return <Skeleton className="h-[170px] rounded-xl xl:h-[232px] xl:rounded-2xl" />;
  }

  const title = directMode
    ? t("当前为账号直连模式")
    : connected
      ? t("网关运行正常")
      : t("正在等待网关连接");
  const description = directMode
    ? t("CodexManager 无法统计 CLI 请求日志和用量。")
    : connected
      ? t("近期请求路由稳定，账号池可正常参与调度。")
      : t("正在等待服务连接。");
  const actionHref = directMode ? "/platform-mode" : "/logs";
  const actionLabel = directMode ? t("去切换为本地网关") : t("查看异常请求");

  return (
    <Card className="dashboard-primary-panel routing-command-card glass-card overflow-hidden rounded-xl border-border/60 py-0 xl:rounded-2xl">
      <CardContent className="p-0">
        <div className="flex min-h-[91px] flex-col gap-4 px-5 py-4.5 lg:flex-row lg:items-center lg:justify-between xl:min-h-[123px] xl:gap-6 xl:px-8 xl:py-7">
          <div className="flex min-w-0 items-center gap-4 xl:gap-6">
            <div
              className={cn(
                "flex h-10 w-10 shrink-0 items-center justify-center rounded-full border-2 bg-background/75 shadow-[0_8px_24px_-18px_currentColor] xl:h-[54px] xl:w-[54px] xl:border-[3px]",
                connected && !directMode
                  ? "border-emerald-500 text-emerald-600"
                  : "border-amber-500/45 text-amber-600",
              )}
            >
              {connected && !directMode ? (
                <Check className="h-5 w-5 stroke-[2.5] xl:h-7 xl:w-7" />
              ) : (
                <AlertTriangle className="h-5 w-5 xl:h-7 xl:w-7" />
              )}
            </div>
            <div className="min-w-0">
              <h2 className="text-[18px] font-semibold leading-tight tracking-[-0.02em] text-foreground xl:text-[24px]">
                {title}
              </h2>
              <p className="mt-1 max-w-2xl text-[12px] leading-5 text-muted-foreground xl:mt-2 xl:text-[15px] xl:leading-6">
                {description}
              </p>
            </div>
          </div>
          <a
            href={buildStaticRouteUrl(actionHref)}
            className={cn(
              buttonVariants({ size: "lg" }),
              "command-center-primary-action h-10 min-w-[136px] shrink-0 rounded-lg px-5 text-sm xl:h-[52px] xl:min-w-[172px] xl:rounded-xl xl:px-6 xl:text-base",
            )}
          >
            {actionLabel}
            <ArrowRight className="ml-1.5 h-4 w-4" />
          </a>
        </div>

        <div className="grid border-t border-border/55 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-7">
          <StatusMetric
            label={t("服务连接")}
            value={connected ? t("正常") : t("离线")}
            tone={connected ? "text-emerald-600" : "text-rose-600"}
          />
          <StatusMetric label={t("账号")} value={String(stats.total)} />
          <StatusMetric label={t("可用")} value={String(stats.available)} tone="text-emerald-600" />
          <StatusMetric
            label={t("异常")}
            value={String(stats.unavailable)}
            tone={stats.unavailable > 0 ? "text-rose-600" : "text-foreground"}
          />
          <StatusMetric
            className="lg:col-span-2"
            label={t("今日/缓存/推理 用量")}
            value={`${formatCompactTokenAmount(stats.todayTokens)} / ${formatCompactTokenAmount(stats.cachedTokens)} / ${formatCompactTokenAmount(stats.reasoningTokens)}`}
            valueClassName="text-[15px] tracking-[-0.035em] sm:text-[17px] lg:text-[clamp(14px,1.18vw,21px)] xl:text-[clamp(16px,1.18vw,23px)]"
          />
          <StatusMetric label={t("预计费用")} value={formatUsd(stats.todayCost)} />
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
      <div className="mb-1.5 flex items-center justify-between gap-3 text-xs xl:mb-2 xl:text-sm">
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
      <div className="mt-1.5 truncate font-mono text-[10px] text-muted-foreground xl:mt-2 xl:text-xs">
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
    return <Skeleton className="h-[92px] rounded-xl xl:h-[122px] xl:rounded-2xl" />;
  }

  return (
    <Card className="dashboard-pool-remaining dashboard-primary-panel glass-card overflow-hidden rounded-xl border-border/60 py-0 xl:rounded-2xl">
      <CardContent className="grid gap-5 px-5 py-4 md:grid-cols-[210px_minmax(0,1fr)] md:items-center xl:min-h-[120px] xl:grid-cols-[220px_minmax(0,1fr)_minmax(0,1fr)] xl:gap-7 xl:px-8 xl:py-5">
        <div className="flex min-w-0 items-center gap-3 xl:gap-4">
          <PieChart className="h-5 w-5 shrink-0 text-emerald-600 xl:h-6 xl:w-6" />
          <span className="truncate text-sm font-semibold text-foreground xl:text-lg">
            {t("账号池剩余")}
          </span>
          <Badge
            variant="outline"
            className="h-6 shrink-0 rounded-md border-emerald-500/25 bg-emerald-500/8 px-2 text-[10px] font-semibold text-emerald-700 xl:h-7 xl:text-xs"
          >
            POOL
          </Badge>
        </div>
        <div className="grid min-w-0 gap-5 sm:grid-cols-2 md:col-span-1 xl:col-span-2 xl:gap-7">
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
