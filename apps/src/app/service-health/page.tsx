"use client";

import { useState } from "react";
import { HeartPulse, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { PageHeader, PageWorkspace } from "@/components/layout/page-workspace";
import { HealthProviderCard } from "@/components/service-health/provider-card";
import { HealthReportDialog } from "@/components/service-health/report-dialog";
import { PROVIDER_LABELS, REGION_LABELS, SYMPTOM_LABELS } from "@/components/service-health/health-labels";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { useServiceHealth } from "@/hooks/useServiceHealth";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";

export default function ServiceHealthPage() {
  const { t, locale } = useI18n();
  const { summary, report, resolve, enabled, isServiceReady } = useServiceHealth();
  const [reportOpen, setReportOpen] = useState(false);
  const data = summary.data;
  const time = (ts: number) => new Date(ts * 1000).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  const onResolve = async (id: string) => {
    try { await resolve.mutateAsync(id); toast.success(t("已标记恢复")); }
    catch (error) { toast.error(getAppErrorMessage(error)); }
  };
  return (
    <PageWorkspace>
      <PageHeader title={t("服务健康")} description={t("查看本服务所有用户的请求概况与异常反馈。")}
        actions={<>
          <Button variant="outline" disabled={!enabled || summary.isFetching} onClick={() => void summary.refetch()}><RefreshCw data-icon="inline-start" />{t("刷新")}</Button>
          <Button disabled={!enabled} onClick={() => setReportOpen(true)}><HeartPulse data-icon="inline-start" />{t("报告异常")}</Button>
        </>}
        meta={<><Badge variant="outline">{t("本服务内共享")}</Badge><Badge variant="secondary">{t("最近 60 分钟")}</Badge></>}
      />
      <Alert>
        <AlertTitle>{t("共享统计，保护请求隐私")}</AlertTitle>
        <AlertDescription>{t("仅展示汇总数字，不公开账号、密钥、请求内容或原始错误。单个服务的样本不能代表官方或全球状态。")}</AlertDescription>
      </Alert>
      {!isServiceReady ? <Alert><AlertTitle>{t("服务未连接")}</AlertTitle><AlertDescription>{t("连接服务后查看健康统计。")}</AlertDescription></Alert> : summary.isError ? (
        <Alert variant="destructive"><AlertTitle>{t("健康统计暂不可用")}</AlertTitle><AlertDescription>{getAppErrorMessage(summary.error)}</AlertDescription></Alert>
      ) : !data ? <div className="grid gap-3 md:grid-cols-2"><Skeleton className="h-72" /><Skeleton className="h-72" /></div> : (
        <>
          <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
            <span>{t("{start}–{end} · {count} 条请求样本", { start: time(data.windowStart), end: time(data.windowEnd), count: data.sampleCount })}</span>
            <span>{t("更新于 {time}", { time: time(data.updatedAt) })}{data.sampled ? ` · ${t("已达采样上限（{count} 条）", { count: data.sampleLimit })}` : ""}</span>
          </div>
          {data.sampleCount === 0 && <Empty><EmptyHeader><EmptyMedia variant="icon"><HeartPulse /></EmptyMedia><EmptyTitle>{t("最近 60 分钟暂无请求样本")}</EmptyTitle><EmptyDescription>{t("产生请求后自动更新；没有样本不代表服务正常。")}</EmptyDescription></EmptyHeader></Empty>}
          <div className="grid gap-3 md:grid-cols-2">
            {data.providers.filter((provider) => provider.provider !== "unknown" || provider.sampleCount > 0 || provider.activeReportCount > 0).map((provider) => <HealthProviderCard key={provider.provider} provider={provider} />)}
          </div>
          <Card className="glass-card">
            <CardHeader><CardTitle>{t("我的异常报告")}</CardTitle><CardDescription>{t("仅你能撤销自己的报告；重复反馈不会增加用户数。")}</CardDescription></CardHeader>
            <CardContent>
              {data.myReports.length === 0 ? <p className="text-sm text-muted-foreground">{t("暂无有效报告")}</p> : <ul className="flex flex-col gap-3">
                {data.myReports.map((item) => <li key={item.id} className="flex flex-wrap items-center justify-between gap-3">
                  <div className="flex min-w-0 flex-col gap-1"><span className="text-sm font-medium">{t(PROVIDER_LABELS[item.provider])} · {t(SYMPTOM_LABELS[item.symptom])}</span><span className="text-xs text-muted-foreground">{t(REGION_LABELS[item.region])} · {t("{time} 自动失效", { time: time(item.expiresAt) })}</span></div>
                  <Button size="sm" variant="outline" disabled={resolve.isPending || !enabled} onClick={() => void onResolve(item.id)}>{t("标记恢复")}</Button>
                </li>)}
              </ul>}
            </CardContent>
          </Card>
        </>
      )}
      <HealthReportDialog key={String(reportOpen)} open={reportOpen} onOpenChange={setReportOpen} onSubmit={report.mutateAsync} pending={report.isPending} />
    </PageWorkspace>
  );
}
