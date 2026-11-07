"use client";

import { useId, useState } from "react";
import { AlertCircle, Loader2, Play, RefreshCw, Square } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter,
  DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { useI18n } from "@/lib/i18n/provider";
import { sourceModelSyncMessage } from "@/lib/source-model-sync-messages";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import type { SourceModelSyncController } from "@/hooks/useSourceModelSync";
import type { ModelDiscoveryTask } from "@/types/model-discovery";

function taskStatus(task?: ModelDiscoveryTask): string {
  if (task?.running) return task.enabled ? "正在同步" : "正在停止";
  if (!task?.enabled) return "未开启";
  if (task.lastError) return "等待重试";
  return "已开启";
}

export function SourceModelSyncControl({
  task, loading, error, disabled, onClick,
}: {
  task?: ModelDiscoveryTask;
  loading?: boolean;
  error?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  const { t } = useI18n();
  const status = loading ? t("加载中...") : error ? t("状态读取失败") : t(taskStatus(task));
  return (
    <Button
      type="button"
      variant={task?.enabled ? "secondary" : "outline"}
      size="xs"
      disabled={disabled}
      onClick={onClick}
      aria-label={t("后台模型同步：{status}", { status })}
    >
      {task?.running ? <Loader2 data-icon="inline-start" className="animate-spin" /> : <RefreshCw data-icon="inline-start" />}
      {t("后台同步：{status}", { status })}
    </Button>
  );
}

export function SourceModelSyncDialog({ controller }: { controller: SourceModelSyncController }) {
  const { t } = useI18n();
  return (
    <Dialog open={controller.open} onOpenChange={controller.onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{t("后台模型同步")}</DialogTitle>
          <DialogDescription>{controller.selectedSource?.name}</DialogDescription>
        </DialogHeader>
        {controller.selectedSource ? (
          <SourceModelSyncForm
            key={`${controller.sourceKind}:${controller.selectedSource.id}`}
            controller={controller}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function SourceModelSyncForm({ controller }: { controller: SourceModelSyncController }) {
  const { t } = useI18n();
  const inputId = useId();
  const task = controller.task;
  const savedInterval = task?.intervalMinutes ?? 60;
  const [draft, setDraft] = useState({ saved: savedInterval, value: String(savedInterval) });
  const intervalText = draft.saved === savedInterval ? draft.value : String(savedInterval);
  const interval = Number(intervalText);
  const validInterval = intervalText.trim() !== "" && Number.isInteger(interval) && interval >= 5 && interval <= 10080;
  const disabled = !controller.available || controller.isSaving || !controller.hasData;
  const changed = validInterval && interval !== savedInterval;
  const displayTime = (value?: number | null) => formatTsFromSeconds(value ?? null, t("暂无"));

  return (
    <div className="flex min-h-0 flex-col gap-4 overflow-y-auto">
      <p className="text-sm text-muted-foreground">
        {t("启用后立即拉取并关联模型，之后按间隔在服务后台运行。默认关闭，服务重启后保留设置。")}
      </p>
      <p className="text-xs text-muted-foreground">
        {t("只添加新模型和缺失路由，不修改已有模型设置，也不删除旧模型。停止后不再导入。")}
        {controller.sourceKind === "openai_account" ? ` ${t("使用这个账号获取模型列表；新增路由仍由账号池选择账号处理请求。")}` : ""}
      </p>
      {controller.error ? (
        <Alert variant="destructive">
          <AlertCircle />
          <AlertTitle>{t("读取后台同步状态失败")}</AlertTitle>
          <AlertDescription>{sourceModelSyncMessage(controller.error, t)}</AlertDescription>
          <Button type="button" size="sm" variant="outline" onClick={controller.refresh}>{t("重试")}</Button>
        </Alert>
      ) : controller.isLoading ? (
        <Skeleton className="h-24 w-full" />
      ) : (
        <div className="flex flex-col gap-3" aria-live="polite">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant={task?.lastError ? "destructive" : task?.enabled ? "default" : "secondary"}>{t(taskStatus(task))}</Badge>
            {task?.lastFinishedAt ? <span className="text-xs text-muted-foreground">{t("最近完成：{time}", { time: displayTime(task.lastFinishedAt) })}</span> : null}
          </div>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
            <dt className="text-muted-foreground">{t("最近开始")}</dt><dd>{displayTime(task?.lastStartedAt)}</dd>
            <dt className="text-muted-foreground">{t("最近成功")}</dt><dd>{displayTime(task?.lastSuccessAt)}</dd>
            <dt className="text-muted-foreground">{t("下次同步")}</dt><dd>{task?.enabled ? displayTime(task.nextRunAt) : t("未安排")}</dd>
            <dt className="text-muted-foreground">{t("最近同步结果")}</dt>
            <dd>{task?.lastFinishedAt ? t("发现 {found}，新增模型 {created}，新增路由 {routes}", { found: task.discoveredCount, created: task.createdModelCount, routes: task.addedRouteCount }) : t("尚未同步")}</dd>
          </dl>
          {task?.lastError ? <Alert variant="destructive"><AlertCircle /><AlertTitle>{t("最近同步失败")}</AlertTitle><AlertDescription className="break-words">{sourceModelSyncMessage(task.lastError, t)}</AlertDescription></Alert> : null}
          {task?.lastWarning ? <Alert><AlertTitle>{t("同步提示")}</AlertTitle><AlertDescription className="break-words">{sourceModelSyncMessage(task.lastWarning, t)}</AlertDescription></Alert> : null}
        </div>
      )}
      <div className="flex flex-col gap-2">
        <Label htmlFor={inputId}>{t("同步间隔（分钟）")}</Label>
        <Input
          id={inputId}
          type="number"
          min={5}
          max={10080}
          step={1}
          value={intervalText}
          disabled={disabled}
          aria-invalid={!validInterval}
          aria-describedby={`${inputId}-help`}
          onChange={(event) => setDraft({ saved: savedInterval, value: event.target.value })}
        />
        <p id={`${inputId}-help`} className="text-xs text-muted-foreground">{t("5–10080 分钟，默认 60 分钟。")}</p>
      </div>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={() => controller.onOpenChange(false)}>{t("关闭")}</Button>
        {task?.enabled ? (
          <>
            <Button type="button" variant="outline" disabled={disabled || !changed} onClick={() => controller.save(true, interval)}>{t("保存同步间隔")}</Button>
            <Button type="button" variant="destructive" disabled={disabled} onClick={() => controller.save(false)}>
              {controller.isSaving ? <Loader2 data-icon="inline-start" className="animate-spin" /> : <Square data-icon="inline-start" />}{t("停止后台同步")}
            </Button>
          </>
        ) : (
          <Button type="button" disabled={disabled || !validInterval || Boolean(task?.running)} onClick={() => controller.save(true, interval)}>
            {controller.isSaving ? <Loader2 data-icon="inline-start" className="animate-spin" /> : <Play data-icon="inline-start" />}{t("启动后台同步")}
          </Button>
        )}
      </DialogFooter>
    </div>
  );
}
