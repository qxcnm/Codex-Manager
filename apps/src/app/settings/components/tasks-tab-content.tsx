import { Settings as SettingsIcon } from "lucide-react";
import { AppSettings, BackgroundTaskSettings } from "@/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  CUSTOM_WORKER_MODE_VALUE,
  type WorkerPreset,
  WORKER_PRESETS,
  stringifyNumber,
  formatRuntimeTimeZoneLabel,
} from "@/app/settings/settings-page-helpers";

type BackgroundDraft = Record<string, string>;

export function TasksTabContent({
  t,
  snapshot,
  backgroundTaskDraft,
  setBackgroundTaskDraft,
  updateBackgroundTasks,
  saveBackgroundTaskField,
  saveBackgroundTaskTextField,
  activeWorkerModeValue,
  activeWorkerPreset,
  activeWorkerSummary,
  deriveConcurrencyRecommendationPending,
  applyWorkerPreset,
  deriveConcurrencyRecommendation,
  workerAdvancedDialogOpen,
  setWorkerAdvancedDialogOpen,
  saveAccountMaxInflightField,
  onInvalidWarmupCron,
}: {
  t: (value: string, params?: Record<string, string | number>) => string;
  snapshot: AppSettings;
  backgroundTaskDraft: BackgroundDraft;
  setBackgroundTaskDraft: React.Dispatch<React.SetStateAction<BackgroundDraft>>;
  updateBackgroundTasks: (patch: Partial<BackgroundTaskSettings>) => void;
  saveBackgroundTaskField: (
    key: keyof BackgroundTaskSettings,
    minimum: number,
  ) => void;
  saveBackgroundTaskTextField: (key: "warmupCronExpression") => void;
  activeWorkerModeValue: string;
  activeWorkerPreset: WorkerPreset | null;
  activeWorkerSummary: string;
  deriveConcurrencyRecommendationPending: boolean;
  applyWorkerPreset: (preset: WorkerPreset) => void;
  deriveConcurrencyRecommendation: () => void;
  workerAdvancedDialogOpen: boolean;
  setWorkerAdvancedDialogOpen: React.Dispatch<React.SetStateAction<boolean>>;
  saveAccountMaxInflightField: (minimum: number) => void;
  onInvalidWarmupCron: () => void;
}) {
  return (
    <>
      <Card className="glass-card shadow-sm">
        <CardHeader>
          <CardTitle className="text-base">{t("后台任务线程")}</CardTitle>
          <CardDescription>{t("管理自动轮询和保活任务；")}</CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          {[
            {
              label: "用量轮询线程",
              enabledKey: "usagePollingEnabled",
              intervalKey: "usagePollIntervalSecs",
            },
            {
              label: "网关保活线程",
              enabledKey: "gatewayKeepaliveEnabled",
              intervalKey: "gatewayKeepaliveIntervalSecs",
            },
            {
              label: "令牌刷新轮询",
              enabledKey: "tokenRefreshPollingEnabled",
              intervalKey: "tokenRefreshPollIntervalSecs",
            },
          ].map((task) => (
            <div
              key={task.enabledKey}
              className="flex items-center justify-between gap-4 rounded-lg bg-accent/20 p-3"
            >
              <div className="flex items-center gap-3">
                <Switch
                  checked={
                    snapshot.backgroundTasks[
                      task.enabledKey as keyof BackgroundTaskSettings
                    ] as boolean
                  }
                  onCheckedChange={(value) =>
                    updateBackgroundTasks({
                      [task.enabledKey]: value,
                    } as Partial<BackgroundTaskSettings>)
                  }
                />
                <Label>{t(task.label)}</Label>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">{t("间隔(秒)")}</span>
                <Input
                  className="h-8 w-20"
                  type="number"
                  value={
                    backgroundTaskDraft[task.intervalKey] ||
                    stringifyNumber(
                      snapshot.backgroundTasks[
                        task.intervalKey as keyof BackgroundTaskSettings
                      ] as number,
                    )
                  }
                  onChange={(event) =>
                    setBackgroundTaskDraft((current) => ({
                      ...current,
                      [task.intervalKey]: event.target.value,
                    }))
                  }
                  onBlur={() =>
                    saveBackgroundTaskField(
                      task.intervalKey as keyof BackgroundTaskSettings,
                      1,
                    )
                  }
                />
              </div>
            </div>
          ))}
          <div className="grid gap-3 rounded-lg bg-accent/20 p-3 lg:grid-cols-[minmax(180px,240px)_minmax(180px,1fr)] lg:items-end">
            <div className="flex items-center gap-3 lg:self-center">
              <Switch
                checked={snapshot.backgroundTasks.warmupCronEnabled}
                onCheckedChange={(value) => {
                  const expression = String(
                    backgroundTaskDraft.warmupCronExpression ??
                      snapshot.backgroundTasks.warmupCronExpression,
                  ).trim();
                  if (value && !expression) {
                    onInvalidWarmupCron();
                    return;
                  }
                  updateBackgroundTasks({
                    warmupCronEnabled: value,
                    ...(value ? { warmupCronExpression: expression } : {}),
                  });
                }}
              />
              <Label>{t("定时账号预热")}</Label>
            </div>
            <div className="grid gap-1.5">
              <Label>{t("Cron 表达式")}</Label>
              <Input
                className="h-8 font-mono"
                value={
                  backgroundTaskDraft.warmupCronExpression ??
                  snapshot.backgroundTasks.warmupCronExpression
                }
                onChange={(event) =>
                  setBackgroundTaskDraft((current) => ({
                    ...current,
                    warmupCronExpression: event.target.value,
                  }))
                }
                onBlur={() => saveBackgroundTaskTextField("warmupCronExpression")}
                placeholder="0 0 * * *|5 5 * * *|10 10 * * *"
              />
            </div>
            <div className="text-xs text-muted-foreground lg:col-span-2">
              <span>
                {t("计划按服务端时区 {timeZone} 执行。多个计划用 | 分隔。", {
                  timeZone: formatRuntimeTimeZoneLabel(
                    snapshot.runtimeTimeZone,
                    t("服务端本地时区"),
                  ),
                })}
              </span>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card className="glass-card shadow-sm">
        <CardHeader>
          <div className="flex items-center gap-2">
            <SettingsIcon className="h-4 w-4 text-primary" />
            <CardTitle className="text-base">{t("运行模式")}</CardTitle>
          </div>
          <CardDescription>
            {t(
              "普通用户选择一个模式即可，系统会自动按档位调整并发。需要更细的控制时，再打开高级参数。",
            )}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Card size="sm">
            <CardContent>
              <div className="grid gap-4 lg:grid-cols-[minmax(280px,380px)_minmax(0,1fr)] lg:items-end">
                <div className="space-y-2">
                  <Label>{t("运行模式")}</Label>
                  <Select
                    value={activeWorkerModeValue}
                    onValueChange={(value) => {
                      const selectedPreset = WORKER_PRESETS.find(
                        (preset) => preset.key === value,
                      );
                      if (!selectedPreset) return;
                      if (selectedPreset.key === "recommended") {
                        deriveConcurrencyRecommendation();
                        return;
                      }
                      applyWorkerPreset(selectedPreset);
                    }}
                  >
                    <SelectTrigger
                      className="h-10 w-full bg-background/80"
                      disabled={deriveConcurrencyRecommendationPending}
                    >
                      <SelectValue placeholder={t("选择运行模式")}>
                        {(value) => {
                          const selectedPreset = WORKER_PRESETS.find(
                            (preset) => preset.key === String(value || "").trim(),
                          );
                          return selectedPreset
                            ? t(selectedPreset.simpleLabel)
                            : t("自定义（来自高级参数）");
                        }}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {WORKER_PRESETS.map((preset) => (
                          <SelectItem key={preset.key} value={preset.key}>
                            {t(preset.simpleLabel)}
                          </SelectItem>
                        ))}
                        {!activeWorkerPreset ? (
                          <SelectItem value={CUSTOM_WORKER_MODE_VALUE} disabled>
                            {t("自定义（来自高级参数）")}
                          </SelectItem>
                        ) : null}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>

                <div className="flex min-h-10 flex-wrap items-center gap-2 lg:justify-start lg:self-end">
                  <span className="text-sm font-medium">{t("当前档位")}</span>
                  <Badge
                    variant={activeWorkerPreset ? "default" : "secondary"}
                    className="h-5 px-2"
                  >
                    {activeWorkerPreset ? t(activeWorkerPreset.simpleLabel) : t("自定义")}
                  </Badge>
                </div>
              </div>

              <Separator className="my-4" />
              <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                <p className="text-xs leading-6 text-muted-foreground">
                  {activeWorkerSummary}
                </p>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-8 w-fit gap-2 px-2"
                  onClick={() => setWorkerAdvancedDialogOpen(true)}
                >
                  <SettingsIcon className="h-4 w-4" />
                  {t("高级参数")}
                </Button>
              </div>
            </CardContent>
          </Card>
        </CardContent>
      </Card>

      <Dialog
        open={workerAdvancedDialogOpen}
        onOpenChange={setWorkerAdvancedDialogOpen}
      >
        <DialogContent className="glass-card sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("高级参数")}</DialogTitle>
            <DialogDescription>
              {t("只有在你明确知道这些参数含义时再调整。改动会直接影响并发和资源占用。")}
            </DialogDescription>
          </DialogHeader>
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            {[
              {
                label: "后台巡检并发",
                helper: "控制用量刷新、后台轮询这些任务同时跑多少个。",
                key: "usageRefreshWorkers",
              },
              {
                label: "普通请求自动并发",
                helper: "普通 HTTP 请求的自动并发倍率，越大越快，也越吃资源。",
                key: "httpWorkerFactor",
              },
              {
                label: "普通请求最低保底",
                helper: "普通 HTTP 请求至少保留多少个处理线程，防止太冷清。",
                key: "httpWorkerMin",
              },
              {
                label: "流式请求自动并发",
                helper: "流式请求的自动并发倍率，流式响应多时会更明显。",
                key: "httpStreamWorkerFactor",
              },
              {
                label: "流式请求最低保底",
                helper: "流式请求至少保留多少个处理线程，保证长连接不卡住。",
                key: "httpStreamWorkerMin",
              },
              {
                label: "单账号并发上限",
                helper: "同一账号同时能处理多少个请求。满了以后会优先换下一个账号；填 0 表示关闭上限。",
                key: "accountMaxInflight",
              },
            ].map((worker) => (
              <div key={worker.key} className="grid gap-1.5">
                <Label className="text-xs">{t(worker.label)}</Label>
                <p className="text-[11px] leading-5 text-muted-foreground">{t(worker.helper)}</p>
                <Input
                  type="number"
                  min={worker.key === "accountMaxInflight" ? 0 : 1}
                  className="h-9"
                  value={
                    backgroundTaskDraft[worker.key] ??
                    stringifyNumber(
                      worker.key === "accountMaxInflight"
                        ? snapshot.accountMaxInflight
                        : (snapshot.backgroundTasks[worker.key as keyof BackgroundTaskSettings] as number),
                    )
                  }
                  onChange={(event) =>
                    setBackgroundTaskDraft((current) => ({
                      ...current,
                      [worker.key]: event.target.value,
                    }))
                  }
                  onBlur={() =>
                    worker.key === "accountMaxInflight"
                      ? saveAccountMaxInflightField(0)
                      : saveBackgroundTaskField(worker.key as keyof BackgroundTaskSettings, 1)
                  }
                />
              </div>
            ))}
          </div>
          <DialogFooter className="gap-2 sm:gap-2">
            <Button type="button" variant="ghost" onClick={() => setWorkerAdvancedDialogOpen(false)}>
              {t("关闭")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
