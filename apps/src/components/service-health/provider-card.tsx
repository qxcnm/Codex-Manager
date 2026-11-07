"use client";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { useI18n } from "@/lib/i18n/provider";
import type { ServiceHealthProviderSummary } from "@/types/service-health";
import { PROVIDER_LABELS, REGION_LABELS } from "./health-labels";

export function HealthProviderCard({ provider }: { provider: ServiceHealthProviderSummary }) {
  const { t, locale } = useI18n();
  const number = (n: number) => n.toLocaleString(locale);
  const latency = (n: number | null) => n == null ? "—" : `${number(Math.round(n))} ms`;
  const metrics = [
    ["请求样本", number(provider.sampleCount)],
    ["平均首响应", latency(provider.avgFirstResponseMs)],
    ["P95 首响应", latency(provider.p95FirstResponseMs)],
    ["反馈用户", number(provider.reporterCount)],
  ];
  const errors = [
    ["请求受限", provider.rateLimitedCount], ["上游错误", provider.upstreamErrorCount],
    ["本地错误", provider.localErrorCount], ["网络错误", provider.networkErrorCount],
    ["未分类错误", provider.unknownErrorCount],
  ] as const;
  const status = provider.status === "healthy" ? "样本正常" : provider.status === "degraded" ? "观察到异常" : "样本不足";
  return (
    <Card className="glass-card min-w-0" data-testid={`health-provider-${provider.provider}`}>
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <CardTitle>{t(PROVIDER_LABELS[provider.provider])}</CardTitle>
          <Badge variant={provider.status === "degraded" ? "destructive" : "secondary"}>{t(status)}</Badge>
        </div>
        <CardDescription>{t("本服务最近 60 分钟的观测")}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div>
          <div className="text-3xl font-semibold tabular-nums tracking-tight">{provider.availabilityPercent == null ? "—" : `${provider.availabilityPercent.toLocaleString(locale, { maximumFractionDigits: 1 })}%`}</div>
          <p className="mt-1 text-xs text-muted-foreground">{t("样本成功率")}</p>
        </div>
        <dl className="grid grid-cols-2 gap-3">
          {metrics.map(([label, value]) => <div key={label}><dt className="text-xs text-muted-foreground">{t(label)}</dt><dd className="mt-1 text-sm font-medium tabular-nums">{value}</dd></div>)}
        </dl>
        <Separator />
        <dl className="flex flex-wrap gap-x-4 gap-y-2">
          {errors.map(([label, count]) => <div key={label} className="flex items-center gap-1.5 text-xs"><dt className="text-muted-foreground">{t(label)}</dt><dd className="font-medium tabular-nums">{number(count)}</dd></div>)}
        </dl>
        <p className="text-xs text-muted-foreground">{t("有效反馈 {count} 条", { count: provider.activeReportCount })}{provider.regions.length > 0 ? ` · ${provider.regions.map((region) => t(REGION_LABELS[region])).join(" / ")}` : ""}</p>
      </CardContent>
    </Card>
  );
}
