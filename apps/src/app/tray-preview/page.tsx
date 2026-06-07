"use client";

import {
  ArrowUpRight,
  CheckCircle2,
  CircleOff,
  DollarSign,
  Loader2,
  Server,
  Users,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useDashboardStats } from "@/hooks/useDashboardStats";
import {
  formatCompactTokenAmount,
  formatPercent,
} from "@/lib/dashboard/format";
import { appClient } from "@/lib/api/app-client";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";

const TRAY_PREVIEW_REQUEST_LOG_LIMIT = 24;

interface MetricTileProps {
  label: string;
  value: string | number;
  icon: LucideIcon;
  tone: string;
}

interface QuotaLineProps {
  label: string;
  value: number | null | undefined;
  tone: "green" | "blue";
}

function MetricTile({ label, value, icon: Icon, tone }: MetricTileProps) {
  return (
    <div className="rounded-xl border border-border/50 bg-card/65 px-3 py-2.5 shadow-inner">
      <div className="mb-1.5 flex items-center justify-between gap-2">
        <span className="truncate text-[11px] font-medium text-muted-foreground">
          {label}
        </span>
        <Icon className={cn("h-3.5 w-3.5 shrink-0", tone)} />
      </div>
      <div className="truncate text-[20px] font-semibold leading-6 tracking-normal text-foreground">
        {value}
      </div>
    </div>
  );
}

function QuotaLine({ label, value, tone }: QuotaLineProps) {
  const normalized = value == null ? 0 : Math.max(0, Math.min(100, Math.round(value)));
  const barTone = tone === "green" ? "bg-emerald-500" : "bg-blue-500";

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-3 text-[11px]">
        <span className="text-muted-foreground">{label}</span>
        <span className="font-semibold text-foreground">
          {formatPercent(value)}
        </span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-muted/60">
        <div className={cn("h-full rounded-full", barTone)} style={{ width: `${normalized}%` }} />
      </div>
    </div>
  );
}

export default function TrayPreviewPage() {
  const { t } = useI18n();
  const {
    stats,
    currentAccount,
    isLoading,
    isServiceReady,
    isError,
    error,
  } = useDashboardStats({
    forceActive: true,
    requestLogLimit: TRAY_PREVIEW_REQUEST_LOG_LIMIT,
  });

  const openMainWindow = async () => {
    await appClient.showMainWindow();
  };

  const statusText = isServiceReady ? t("本地服务已连接") : t("等待本地服务");

  return (
    <div className="h-screen overflow-hidden bg-transparent font-sans text-foreground">
      <section className="relative h-full overflow-hidden rounded-[18px] border border-border/60 bg-card/90 shadow-2xl">
        <div className="flex h-full flex-col p-4">
          <header className="mb-3 flex items-start justify-between gap-3">
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span
                  className={cn(
                    "h-2.5 w-2.5 rounded-full",
                    isServiceReady ? "bg-emerald-500" : "bg-amber-500",
                  )}
                />
                <p className="truncate text-[12px] font-medium text-muted-foreground">
                  {statusText}
                </p>
              </div>
              <h1 className="mt-1 truncate text-[17px] font-semibold tracking-normal">
                CodexManager
              </h1>
            </div>
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-8 shrink-0 rounded-full px-2.5 text-[12px] text-foreground/80 hover:bg-accent/70"
              onClick={() => void openMainWindow()}
            >
              <ArrowUpRight className="h-3.5 w-3.5" />
              {t("打开")}
            </Button>
          </header>

          {isLoading ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-muted-foreground">
              <Loader2 className="h-5 w-5 animate-spin" />
              <p className="text-[12px]">{t("正在同步状态")}</p>
            </div>
          ) : isError ? (
            <div className="flex flex-1 flex-col justify-center gap-3 rounded-xl border border-red-500/[0.15] bg-red-500/[0.08] p-4 text-[12px] text-red-700 dark:text-red-200">
              <div className="flex items-center gap-2 font-semibold">
                <CircleOff className="h-4 w-4" />
                {t("状态读取失败")}
              </div>
              <p className="max-h-[4.5em] overflow-hidden break-all text-red-700/80 dark:text-red-200/80">
                {error instanceof Error ? error.message : String(error || "")}
              </p>
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col gap-3">
              <div className="grid grid-cols-2 gap-2">
                <MetricTile
                  label={t("总账号数")}
                  value={stats.total}
                  icon={Users}
                  tone="text-blue-500"
                />
                <MetricTile
                  label={t("可用账号")}
                  value={stats.available}
                  icon={CheckCircle2}
                  tone="text-emerald-500"
                />
                <MetricTile
                  label={t("今日Token")}
                  value={formatCompactTokenAmount(stats.todayTokens)}
                  icon={Zap}
                  tone="text-amber-500"
                />
                <MetricTile
                  label={t("预计费用")}
                  value={`$${Number(stats.todayCost || 0).toFixed(2)}`}
                  icon={DollarSign}
                  tone="text-emerald-600"
                />
              </div>

              <div className="rounded-xl border border-border/50 bg-card/60 p-3 shadow-inner">
                <div className="mb-3 flex items-center justify-between gap-3">
                  <div className="min-w-0">
                    <p className="text-[11px] font-medium text-muted-foreground">
                      {t("当前活跃账号")}
                    </p>
                    <p className="break-all text-[13px] font-semibold leading-snug [overflow-wrap:anywhere]">
                      {currentAccount?.name || t("暂无可识别的活跃账号")}
                    </p>
                  </div>
                  <Server className="h-4 w-4 shrink-0 text-muted-foreground" />
                </div>
                <div className="space-y-3">
                  <QuotaLine
                    label={t("5小时剩余")}
                    value={currentAccount?.primaryRemainPercent ?? stats.poolRemain?.primary}
                    tone="green"
                  />
                  <QuotaLine
                    label={t("7天剩余")}
                    value={currentAccount?.secondaryRemainPercent ?? stats.poolRemain?.secondary}
                    tone="blue"
                  />
                </div>
              </div>

            </div>
          )}
        </div>
      </section>
    </div>
  );
}
