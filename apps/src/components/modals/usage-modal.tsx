"use client";

import {
  Calendar,
  Clock,
  Database,
  Gift,
  KeyRound,
  ShieldAlert,
  type LucideIcon,
  RefreshCw,
  Zap,
} from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button, buttonVariants } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";
import {
  formatAccountSubscriptionPlanLabel,
  formatAccountSubscriptionStatusLabel,
} from "@/app/accounts/accounts-page-helpers";
import {
  formatTsFromSeconds,
  getExtraUsageDisplayRows,
  getResetEntitlementDisplayRows,
  getUsageDisplayBuckets,
  isPrimaryWindowOnlyUsage,
  isSecondaryWindowOnlyUsage,
} from "@/lib/utils/usage";
import { Account } from "@/types";
import { useI18n } from "@/lib/i18n/provider";

interface UsageModalProps {
  account: Account | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onRefresh: (id: string) => void;
  onRefreshRt: (id: string) => void;
  isRefreshing: boolean;
  isRefreshingRt: boolean;
}

interface UsageDetailRowProps {
  label: string;
  remainPercent: number | null;
  resetsAt: number | null | undefined;
  icon: LucideIcon;
  tone: "green" | "blue" | "amber";
  caption?: string;
  emptyText?: string;
  emptyResetText?: string;
}

/**
 * 函数 `UsageDetailRow`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - params: 参数 params
 *
 * # 返回
 * 返回函数执行结果
 */
function UsageDetailRow({
  label,
  remainPercent,
  resetsAt,
  icon: Icon,
  tone,
  caption,
  emptyText = "--",
  emptyResetText = "未知",
}: UsageDetailRowProps) {
  const { t } = useI18n();
  const value = remainPercent ?? 0;
  const toneClasses = {
    blue: {
      icon: "bg-blue-500/10 text-blue-500",
      track: "bg-blue-500/20",
      indicator: "bg-blue-500",
    },
    green: {
      icon: "bg-green-500/10 text-green-500",
      track: "bg-green-500/20",
      indicator: "bg-green-500",
    },
    amber: {
      icon: "bg-amber-500/10 text-amber-500",
      track: "bg-amber-500/20",
      indicator: "bg-amber-500",
    },
  } as const;
  const palette = toneClasses[tone];

  return (
    <Card size="sm">
      <CardContent className="grid gap-2">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <div className={cn("rounded-lg p-1.5", palette.icon)}>
            <Icon className="h-3.5 w-3.5" />
          </div>
          <div className="min-w-0 space-y-0.5">
            <span className="block truncate font-medium">{label}</span>
            {caption ? (
              <span className="block text-[10px] text-muted-foreground">{caption}</span>
            ) : null}
          </div>
        </div>
        <div className="shrink-0 text-right">
          <span className="text-base font-semibold">
            {remainPercent == null ? emptyText : `${value}%`}
          </span>
          <span className="ml-1 text-xs text-muted-foreground">
            {remainPercent == null ? "" : t("剩余")}
          </span>
        </div>
      </div>

      <Progress
        value={value}
        trackClassName={palette.track}
        indicatorClassName={palette.indicator}
      />

      <div className="flex items-center justify-between gap-3 text-[10px] text-muted-foreground">
        <span className="shrink-0">
          {t("已使用")} {remainPercent == null ? "--" : `${Math.max(0, 100 - value)}%`}
        </span>
        <span className="flex min-w-0 items-center justify-end gap-1 text-right">
          <Clock className="h-2.5 w-2.5" />
          {t("重置时间:")} {formatTsFromSeconds(resetsAt, t(emptyResetText))}
        </span>
      </div>
      </CardContent>
    </Card>
  );
}

export default function UsageModal({
  account,
  open,
  onOpenChange,
  onRefresh,
  onRefreshRt,
  isRefreshing,
  isRefreshingRt,
}: UsageModalProps) {
  const { t } = useI18n();
  if (!account) return null;
  const subscriptionStatusLabel = formatAccountSubscriptionStatusLabel(account, t);
  const subscriptionPlanLabel = formatAccountSubscriptionPlanLabel(account, t);
  const primaryWindowOnly = isPrimaryWindowOnlyUsage(account.usage);
  const secondaryWindowOnly = isSecondaryWindowOnlyUsage(account.usage);
  const usageBuckets = getUsageDisplayBuckets(account.usage);
  const extraUsageRows = getExtraUsageDisplayRows(account.usage);
  const resetEntitlements =
    account.resetEntitlements?.length > 0
      ? account.resetEntitlements
      : getResetEntitlementDisplayRows(account.usage);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="glass-card grid-rows-[auto_minmax(0,1fr)_auto] p-6"
        style={{
          height: "min(680px, calc(100vh - 2rem))",
          maxWidth: "none",
          width: "min(700px, calc(100vw - 2rem))",
        }}
      >
        <DialogHeader>
          <div className="mb-2 flex items-center gap-3">
            <div className="rounded-full bg-primary/10 p-2 text-primary">
              <Database className="h-5 w-5" />
            </div>
            <DialogTitle>{t("用量详情")}</DialogTitle>
          </div>
          <DialogDescription className="font-medium text-foreground/80">
            {t("账号:")} {account.name} ({account.id.slice(0, 8)}...)
          </DialogDescription>
        </DialogHeader>

        <div className="grid min-h-0 gap-4 overflow-y-auto py-4 pr-1">
          {!account.hasToken ? (
            <Alert variant="destructive">
              <ShieldAlert className="h-4 w-4" />
              <AlertTitle>{t("缺少授权 Token")}</AlertTitle>
              <AlertDescription>
                {t("该账号只有用量快照，当前不能参与模型刷新或网关转发。请重新登录或刷新 AT/RT 后再使用。")}
              </AlertDescription>
            </Alert>
          ) : null}

          <Card size="sm">
            <CardHeader>
              <CardTitle>{t("套餐信息")}</CardTitle>
              <CardDescription>
                {t("这里展示账号套餐接口同步回来的套餐状态与时间信息。")}
              </CardDescription>
            </CardHeader>

            <CardContent>
              <div className="grid gap-3 sm:grid-cols-2">
                <Card size="sm">
                  <CardContent>
                    <div className="text-[10px] text-muted-foreground">{t("订阅状态")}</div>
                    <div className="text-sm font-semibold">{subscriptionStatusLabel}</div>
                  </CardContent>
                </Card>
                <Card size="sm">
                  <CardContent>
                    <div className="text-[10px] text-muted-foreground">{t("订阅方案")}</div>
                    <div className="text-sm font-semibold">{subscriptionPlanLabel}</div>
                  </CardContent>
                </Card>
                <Card size="sm">
                  <CardContent>
                    <div className="text-[10px] text-muted-foreground">{t("到期时间")}</div>
                    <div className="text-sm font-semibold">
                      {formatTsFromSeconds(account.subscriptionExpiresAt, t("未知"))}
                    </div>
                  </CardContent>
                </Card>
                <Card size="sm">
                  <CardContent>
                    <div className="text-[10px] text-muted-foreground">{t("续费时间")}</div>
                    <div className="text-sm font-semibold">
                      {formatTsFromSeconds(account.subscriptionRenewsAt, t("未知"))}
                    </div>
                  </CardContent>
                </Card>
              </div>
            </CardContent>
          </Card>

          <Card size="sm">
            <CardHeader>
              <CardTitle>{t("额度窗口")}</CardTitle>
              <CardDescription>
                {t("标准 5 小时、7 天周期，以及像 Code Review / Spark 这类专属额度都会在这里按单列依次显示。")}
              </CardDescription>
            </CardHeader>

            <CardContent>
              <div className="grid gap-3 sm:grid-cols-2">
              <UsageDetailRow
                label={t("5小时额度")}
                remainPercent={usageBuckets.primaryRemainPercent}
                resetsAt={usageBuckets.primaryResetsAt}
                icon={Clock}
                tone="green"
                caption={t("标准模型窗口")}
                emptyText={secondaryWindowOnly ? t("未提供") : "--"}
                emptyResetText={secondaryWindowOnly ? t("未提供") : t("未知")}
              />

              <UsageDetailRow
                label={t("7天周期额度")}
                remainPercent={usageBuckets.secondaryRemainPercent}
                resetsAt={usageBuckets.secondaryResetsAt}
                icon={Calendar}
                tone="blue"
                caption={t("长周期窗口")}
                emptyText={primaryWindowOnly ? t("未提供") : "--"}
                emptyResetText={primaryWindowOnly ? t("未提供") : t("未知")}
              />

              {extraUsageRows.map((item) => (
                <UsageDetailRow
                  key={item.id}
                  label={`${t(item.label, item.labelValues)}${item.labelSuffix ? t(item.labelSuffix) : ""}`}
                  remainPercent={item.remainPercent}
                  resetsAt={item.resetsAt}
                  icon={Zap}
                  tone="amber"
                  caption={t(item.windowLabel, item.windowLabelValues)}
                  emptyText="--"
                  emptyResetText={t("未知")}
                />
              ))}
              </div>
            </CardContent>
          </Card>

          {resetEntitlements.length > 0 ? (
            <Card size="sm">
              <CardHeader>
                <CardTitle>{t("重置权益")}</CardTitle>
                <CardDescription>
                  {t("这里只展示识别到的重置卡；实际调用仍按你设置的批次循环运行。")}
                </CardDescription>
              </CardHeader>
              <CardContent>
                <div className="grid gap-3 sm:grid-cols-2">
                  {resetEntitlements.map((item) => (
                    <Card key={item.id} size="sm" className="border-amber-500/20 bg-amber-500/5">
                      <CardContent className="grid gap-2">
                        <div className="flex items-start justify-between gap-3">
                          <div className="flex min-w-0 items-center gap-2">
                            <div className="rounded-lg bg-amber-500/10 p-1.5 text-amber-500">
                              <Gift className="h-3.5 w-3.5" />
                            </div>
                            <div className="min-w-0">
                              <div className="truncate font-medium">{t(item.label)}</div>
                              <div className="text-[10px] text-muted-foreground">
                                {item.manualActivationRequired
                                  ? t("需要手动激活")
                                  : t("可用权益")}
                              </div>
                            </div>
                          </div>
                          <div className="shrink-0 text-right">
                            <div className="text-base font-semibold">
                              {item.count == null ? "--" : item.count}
                            </div>
                            <div className="text-[10px] text-muted-foreground">{t("张")}</div>
                          </div>
                        </div>
                        <div className="grid gap-1 text-[10px] text-muted-foreground">
                          <div>
                            {t("过期时间")}: {formatTsFromSeconds(item.expiresAt, t("未知"))}
                          </div>
                          {item.activatesAt ? (
                            <div>
                              {t("生效时间")}: {formatTsFromSeconds(item.activatesAt, t("未知"))}
                            </div>
                          ) : null}
                          {item.appliesTo.length > 0 ? (
                            <div className="break-all">
                              {t("适用范围")}: {item.appliesTo.join(", ")}
                            </div>
                          ) : null}
                        </div>
                      </CardContent>
                    </Card>
                  ))}
                </div>
              </CardContent>
            </Card>
          ) : null}

          <div className="text-center">
            <p className="text-[10px] italic text-muted-foreground">
              {t("数据捕获于:")} {formatTsFromSeconds(account.lastRefreshAt, t("未知时间"))}
            </p>
          </div>
        </div>

        <DialogFooter className="-mx-6 -mb-6 min-h-16 px-10 py-3 sm:items-center sm:justify-between">
          <DialogClose
            className={buttonVariants({ variant: "ghost" })}
            type="button"
          >
            {t("关闭")}
          </DialogClose>
          <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
            <Button
              variant="outline"
              onClick={() => onRefreshRt(account.id)}
              disabled={isRefreshingRt}
              className="gap-2"
            >
              <KeyRound
                className={cn("h-4 w-4", isRefreshingRt && "animate-pulse")}
              />
              {isRefreshingRt ? t("AT/RT 刷新中...") : t("刷新 AT/RT")}
            </Button>
            <Button
              onClick={() => onRefresh(account.id)}
              disabled={isRefreshing}
              className="gap-2"
            >
              <RefreshCw
                className={cn("h-4 w-4", isRefreshing && "animate-spin")}
              />
              {isRefreshing ? t("正在刷新...") : t("立即刷新")}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
