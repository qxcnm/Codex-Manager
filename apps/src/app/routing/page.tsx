"use client";

import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { BrainCircuit, Gauge, Layers3, Repeat2, Route, Save, ShieldCheck, SlidersHorizontal, Sparkles } from "lucide-react";
import { toast } from "sonner";
import { PageHeader, PageWorkspace, WorkPanel } from "@/components/layout/page-workspace";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { appClient } from "@/lib/api/app-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import type { AppSettings } from "@/types";

function routeStrategyLabel(value: string): string {
  if (value === "balanced") return "均衡轮询";
  return "顺序优先";
}

function numberInputValue(value: number | null | undefined): string {
  return String(Number.isFinite(value) ? value : 0);
}

export default function SmartRoutingPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/routing/");
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const settingsQuery = useQuery({
    queryKey: ["app-settings", "smart-routing"],
    queryFn: appClient.getSettings,
    enabled: isServiceReady && isPageActive,
  });
  const [saving, setSaving] = useState(false);
  const snapshot = settingsQuery.data;

  usePageTransitionReady("/routing/", !isServiceReady || !settingsQuery.isLoading);

  const patchSettings = async (patch: Partial<AppSettings>) => {
    if (!snapshot) return;
    setSaving(true);
    try {
      await appClient.setSettings(patch);
      await queryClient.invalidateQueries({ queryKey: ["app-settings"] });
      await queryClient.invalidateQueries({ queryKey: ["app-settings", "smart-routing"] });
      toast.success(t("已保存"));
    } catch (error) {
      toast.error(getAppErrorMessage(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <PageWorkspace>
      <PageHeader
        eyebrow="Smart Router"
        title={t("智能路由口")}
        description={t("统一管理智能别名、批次轮询、额度保护和模型映射入口。")}
        actions={<Button variant="outline" disabled={!isServiceReady || settingsQuery.isFetching} onClick={() => settingsQuery.refetch()}><Route className="mr-2 h-4 w-4" />{t("刷新")}</Button>}
      />

      {!isServiceReady ? (
        <Alert>
          <ShieldCheck className="h-4 w-4" />
          <AlertTitle>{t("服务未连接")}</AlertTitle>
          <AlertDescription>{t("连接 CodexManager 服务后才能调整智能路由。")}</AlertDescription>
        </Alert>
      ) : null}

      <div className="grid gap-3 md:grid-cols-4">
        <WorkPanel><CardContent className="flex items-center justify-between p-4"><div><p className="text-xs text-muted-foreground">{t("当前策略")}</p><p className="mt-1 font-semibold">{snapshot ? t(routeStrategyLabel(snapshot.routeStrategy)) : "-"}</p></div><SlidersHorizontal className="h-5 w-5 text-primary" /></CardContent></WorkPanel>
        <WorkPanel><CardContent className="flex items-center justify-between p-4"><div><p className="text-xs text-muted-foreground">{t("批次轮询")}</p><p className="mt-1 font-semibold">{snapshot?.accountBatchRotation.enabled ? t("已开启") : t("未开启")}</p></div><Repeat2 className="h-5 w-5 text-emerald-500" /></CardContent></WorkPanel>
        <WorkPanel><CardContent className="flex items-center justify-between p-4"><div><p className="text-xs text-muted-foreground">{t("额度保护")}</p><p className="mt-1 font-semibold">{snapshot?.quotaGuard.enabled ? t("已开启") : t("默认关闭")}</p></div><Gauge className="h-5 w-5 text-amber-500" /></CardContent></WorkPanel>
        <WorkPanel><CardContent className="flex items-center justify-between p-4"><div><p className="text-xs text-muted-foreground">{t("智能别名")}</p><p className="mt-1 font-mono text-sm">smart / coding / fast / cheap</p></div><BrainCircuit className="h-5 w-5 text-violet-500" /></CardContent></WorkPanel>
      </div>

      <div className="grid gap-4 xl:grid-cols-[1.1fr_0.9fr]">
        <WorkPanel>
          <CardContent className="space-y-5 p-4">
            <div>
              <h2 className="flex items-center gap-2 font-semibold"><Sparkles className="h-4 w-4 text-primary" />{t("智能选择")}</h2>
              <p className="mt-1 text-xs text-muted-foreground">{t("前台可请求 smart、coding、fast、cheap，后台按健康、额度、能力和批次策略选路。")}</p>
            </div>
            <div className="grid gap-2">
              <Label>{t("账号选路策略")}</Label>
              <Select value={snapshot?.routeStrategy || "ordered"} disabled={!snapshot || saving} onValueChange={(value) => void patchSettings({ routeStrategy: value || "ordered" })}>
                <SelectTrigger className="max-w-sm"><SelectValue /></SelectTrigger>
                <SelectContent><SelectGroup><SelectItem value="ordered">{t("顺序优先 (Ordered)")}</SelectItem><SelectItem value="balanced">{t("均衡轮询 (Balanced)")}</SelectItem></SelectGroup></SelectContent>
              </Select>
            </div>
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border/70 p-3">
              <div><Label>{t("线程感知账号分配")}</Label><p className="mt-1 text-xs text-muted-foreground">{t("新线程优先分配到当前承载更少线程的账号，已有线程保持粘性。")}</p></div>
              <Switch checked={Boolean(snapshot?.threadAwareAccountDistributionEnabled)} disabled={!snapshot || saving} onCheckedChange={(checked) => void patchSettings({ threadAwareAccountDistributionEnabled: checked })} />
            </div>
            <div className="rounded-lg border border-violet-500/20 bg-violet-500/5 p-3">
              <div className="mb-2 flex items-center gap-2 text-xs font-semibold text-violet-500"><Layers3 className="h-3.5 w-3.5" />{t("别名能力口")}</div>
              <div className="flex flex-wrap gap-2">{["smart", "coding", "fast", "cheap"].map((item) => <Badge key={item} variant="outline" className="font-mono">{item}</Badge>)}</div>
            </div>
          </CardContent>
        </WorkPanel>

        <WorkPanel>
          <CardContent className="space-y-5 p-4">
            <div>
              <h2 className="flex items-center gap-2 font-semibold"><Repeat2 className="h-4 w-4 text-emerald-500" />{t("分批正向循环")}</h2>
              <p className="mt-1 text-xs text-muted-foreground">{t("按你设定的批次运行：当前批次不可用后才切下一批，不额外打乱。")}</p>
            </div>
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border/70 p-3">
              <div><Label>{t("启用批次轮询")}</Label><p className="mt-1 text-xs text-muted-foreground">{t("适合 5 小时限制账号池正向循环。")}</p></div>
              <Switch checked={Boolean(snapshot?.accountBatchRotation.enabled)} disabled={!snapshot || saving} onCheckedChange={(enabled) => snapshot && void patchSettings({ accountBatchRotation: { ...snapshot.accountBatchRotation, enabled } })} />
            </div>
            <div className="grid gap-3 sm:grid-cols-3">
              <div className="grid gap-2"><Label>{t("每批账号数量")}</Label><Input type="number" min={1} value={numberInputValue(snapshot?.accountBatchRotation.batchSize)} disabled={!snapshot || saving || !snapshot.accountBatchRotation.enabled} onChange={(event) => { const value = Number.parseInt(event.target.value, 10); if (snapshot && Number.isFinite(value) && value > 0) void patchSettings({ accountBatchRotation: { ...snapshot.accountBatchRotation, batchSize: value } }); }} /></div>
              <div className="grid gap-2"><Label>{t("无重置时间时的恢复窗口（分钟）")}</Label><Input type="number" min={1} value={numberInputValue(snapshot?.accountBatchRotation.fallbackWindowMinutes)} disabled={!snapshot || saving || !snapshot.accountBatchRotation.enabled} onChange={(event) => { const value = Number.parseInt(event.target.value, 10); if (snapshot && Number.isFinite(value) && value > 0) void patchSettings({ accountBatchRotation: { ...snapshot.accountBatchRotation, fallbackWindowMinutes: value } }); }} /></div>
              <div className="grid gap-2"><Label>{t("单次请求最多尝试")}</Label><Input type="number" min={1} max={64} value={numberInputValue(snapshot?.accountBatchRotation.maxAttemptsPerRequest)} disabled={!snapshot || saving} onChange={(event) => { const value = Number.parseInt(event.target.value, 10); if (snapshot && Number.isFinite(value) && value > 0) void patchSettings({ accountBatchRotation: { ...snapshot.accountBatchRotation, maxAttemptsPerRequest: value } }); }} /></div>
            </div>
          </CardContent>
        </WorkPanel>
      </div>

      <WorkPanel>
        <CardContent className="space-y-5 p-4">
          <div>
            <h2 className="flex items-center gap-2 font-semibold"><Gauge className="h-4 w-4 text-amber-500" />{t("额度保护")}</h2>
            <p className="mt-1 text-xs text-muted-foreground">{t("默认关闭；开启后低于保留百分比的账号会被跳过。")}</p>
          </div>
          <div className="grid gap-4 md:grid-cols-3">
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border/70 p-3"><Label>{t("启用额度保护")}</Label><Switch checked={Boolean(snapshot?.quotaGuard.enabled)} disabled={!snapshot || saving} onCheckedChange={(enabled) => snapshot && void patchSettings({ quotaGuard: { ...snapshot.quotaGuard, enabled } })} /></div>
            <div className="grid gap-2"><Label>{t("5 小时窗口保留 (%)")}</Label><Input type="number" min={0} max={100} value={numberInputValue(snapshot?.quotaGuard.primaryMinRemainingPercent)} disabled={!snapshot || saving || !snapshot.quotaGuard.enabled} onChange={(event) => { const value = Number.parseInt(event.target.value, 10); if (snapshot && Number.isFinite(value)) void patchSettings({ quotaGuard: { ...snapshot.quotaGuard, primaryMinRemainingPercent: value } }); }} /></div>
            <div className="grid gap-2"><Label>{t("周窗口保留 (%)")}</Label><Input type="number" min={0} max={100} value={numberInputValue(snapshot?.quotaGuard.secondaryMinRemainingPercent)} disabled={!snapshot || saving || !snapshot.quotaGuard.enabled} onChange={(event) => { const value = Number.parseInt(event.target.value, 10); if (snapshot && Number.isFinite(value)) void patchSettings({ quotaGuard: { ...snapshot.quotaGuard, secondaryMinRemainingPercent: value } }); }} /></div>
          </div>
          <div className="flex justify-end"><Button variant="outline" disabled={saving || settingsQuery.isFetching} onClick={() => settingsQuery.refetch()}><Save className="mr-2 h-4 w-4" />{t("刷新配置")}</Button></div>
        </CardContent>
      </WorkPanel>
    </PageWorkspace>
  );
}
