import { Repeat2, TimerReset } from "lucide-react";
import type { AppSettings } from "@/types";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";

export function AccountStabilityCard({
  t,
  snapshot,
  updateSettings,
}: {
  t: (value: string) => string;
  snapshot: AppSettings;
  updateSettings: {
    mutate: (patch: Partial<AppSettings>) => void;
  };
}) {
  const config = snapshot.accountBatchRotation;
  const status = snapshot.accountBatchRotationStatus;
  const patchConfig = (patch: Partial<typeof config>) =>
    updateSettings.mutate({ accountBatchRotation: { ...config, ...patch } });
  const resetText = status.earliestResetAt
    ? new Date(status.earliestResetAt * 1000).toLocaleString()
    : t("等待真实额度重置时间");

  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-start justify-between gap-4">
          <div className="space-y-1">
            <CardTitle className="flex items-center gap-2 text-base">
              <Repeat2 className="h-4 w-4 text-primary" />
              {t("账号稳定 · 分批正向循环")}
            </CardTitle>
            <CardDescription>
              {t("把账号池固定分组；当前组全部限额或冷却后才切换下一组，末组结束后从首组检查恢复。")}
            </CardDescription>
          </div>
          <Switch
            checked={config.enabled}
            onCheckedChange={(enabled) => patchConfig({ enabled })}
          />
        </div>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="grid gap-4 md:grid-cols-3">
          <div className="grid gap-2">
            <Label>{t("每批账号数量")}</Label>
            <Input
              type="number"
              min={1}
              max={10000}
              value={config.batchSize}
              disabled={!config.enabled}
              onChange={(event) => {
                const value = Number.parseInt(event.target.value, 10);
                if (Number.isFinite(value) && value > 0) patchConfig({ batchSize: value });
              }}
            />
            <p className="text-[10px] text-muted-foreground">
              {t("例如填 5：账号 1–5、6–10、11–15 依次成组。")}
            </p>
          </div>
          <div className="grid gap-2">
            <Label>{t("无重置时间时的恢复窗口（分钟）")}</Label>
            <Input
              type="number"
              min={1}
              max={43200}
              value={config.fallbackWindowMinutes}
              disabled={!config.enabled}
              onChange={(event) => {
                const value = Number.parseInt(event.target.value, 10);
                if (Number.isFinite(value) && value > 0) {
                  patchConfig({ fallbackWindowMinutes: value });
                }
              }}
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("单次请求最多尝试")}</Label>
            <Input
              type="number"
              min={1}
              max={64}
              value={config.maxAttemptsPerRequest}
              onChange={(event) => {
                const value = Number.parseInt(event.target.value, 10);
                if (Number.isFinite(value) && value > 0) {
                  patchConfig({ maxAttemptsPerRequest: value });
                }
              }}
            />
            <p className="text-[10px] text-muted-foreground">
              {t("限制换连接、重试和切号的总次数，避免异常时扫完整个号池。")}
            </p>
          </div>
        </div>

        <div className="grid gap-3 border-t pt-5 sm:grid-cols-3">
          <div className="rounded-lg border border-border/60 bg-muted/20 p-3">
            <p className="text-[10px] text-muted-foreground">{t("当前批次")}</p>
            <p className="mt-1 text-lg font-semibold">
              {status.totalBatches ? `${status.currentBatch} / ${status.totalBatches}` : "—"}
            </p>
          </div>
          <div className="rounded-lg border border-border/60 bg-muted/20 p-3">
            <p className="text-[10px] text-muted-foreground">{t("本批可用")}</p>
            <p className="mt-1 text-lg font-semibold">
              {status.currentBatchAvailable} / {status.currentBatchAccounts}
            </p>
          </div>
          <div className="rounded-lg border border-border/60 bg-muted/20 p-3">
            <p className="flex items-center gap-1 text-[10px] text-muted-foreground">
              <TimerReset className="h-3 w-3" /> {t("最早恢复")}
            </p>
            <p className="mt-1 truncate text-sm font-medium" title={resetText}>{resetText}</p>
          </div>
        </div>
        <p className="text-[10px] text-muted-foreground">
          {t("默认关闭，不影响现有选路。分批只限定候选范围，批内仍使用健康度、额度、并发和会话粘性策略。")}
        </p>
      </CardContent>
    </Card>
  );
}
