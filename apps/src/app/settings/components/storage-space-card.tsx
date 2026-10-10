"use client";

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Database } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import {
  getStorageSpaceUsage,
  reclaimStorageSpace,
  type StorageSpaceUsage,
} from "@/lib/api/storage-space";
import { getAppErrorMessage } from "@/lib/api/transport";

const STORAGE_SPACE_QUERY_KEY = ["storage-space-usage"] as const;

type TranslateFn = (
  message: string,
  values?: Record<string, string | number>,
) => string;

function formatBytes(bytes: number | null): string {
  if (bytes === null || !Number.isFinite(bytes) || bytes < 0) return "-";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

function isBusy(usage: StorageSpaceUsage | undefined): boolean {
  return Boolean(
    usage &&
      (usage.reclaimRunning || usage.rebuildRunning || usage.purgePending),
  );
}

export function StorageSpaceCard({
  t,
  active = true,
}: {
  t: TranslateFn;
  active?: boolean;
}) {
  const queryClient = useQueryClient();
  const [rebuildConfirmOpen, setRebuildConfirmOpen] = useState(false);
  const usageQuery = useQuery({
    queryKey: STORAGE_SPACE_QUERY_KEY,
    queryFn: getStorageSpaceUsage,
    enabled: active,
    refetchInterval: (query) => (isBusy(query.state.data) ? 3000 : false),
  });
  const reclaim = useMutation({
    mutationFn: (rebuild: boolean) => reclaimStorageSpace(rebuild),
    onSuccess: (result) => {
      toast.success(
        result.mode === "rebuild"
          ? t("数据库整理已开始")
          : t("已开始在后台回收空间"),
      );
      void queryClient.invalidateQueries({ queryKey: STORAGE_SPACE_QUERY_KEY });
    },
    onError: (error: unknown) => {
      toast.error(`${t("回收失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const usage = usageQuery.data;
  const incremental = usage?.autoVacuum === "incremental";
  const modeLabel =
    usage?.autoVacuum === "incremental"
      ? t("增量回收")
      : usage?.autoVacuum === "full"
        ? t("完整回收")
        : t("未启用");
  const busy = isBusy(usage) || reclaim.isPending;

  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-center gap-2">
          <Database className="h-4 w-4 text-primary" />
          <CardTitle className="text-base">{t("数据库空间")}</CardTitle>
        </div>
        <CardDescription>
          {t(
            "清空或过期清理日志后，数据库文件不会自动变小；释放出的空间会被新数据复用，也可以在这里回收给系统。",
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {usageQuery.isError ? (
          <Alert variant="destructive">
            <AlertTitle>{t("读取数据库空间失败")}</AlertTitle>
            <AlertDescription>
              {getAppErrorMessage(usageQuery.error)}
            </AlertDescription>
          </Alert>
        ) : null}

        {usage?.backend === "remote" ? (
          <p className="text-xs text-muted-foreground">
            {t("远程数据库模式下不支持此操作")}
          </p>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-3 text-xs sm:grid-cols-5">
              <div>
                <div className="text-muted-foreground">{t("已用空间")}</div>
                <div className="mt-0.5 font-mono">
                  {usage ? formatBytes(usage.usedBytes) : "-"}
                </div>
              </div>
              <div>
                <div className="text-muted-foreground">{t("可回收空间")}</div>
                <div className="mt-0.5 font-mono">
                  {usage ? formatBytes(usage.reclaimableBytes) : "-"}
                </div>
              </div>
              <div>
                <div className="text-muted-foreground">{t("数据库文件")}</div>
                <div className="mt-0.5 font-mono">
                  {usage ? formatBytes(usage.fileBytes) : "-"}
                </div>
              </div>
              <div>
                <div className="text-muted-foreground">{t("WAL 文件")}</div>
                <div className="mt-0.5 font-mono">
                  {usage ? formatBytes(usage.walBytes) : "-"}
                </div>
              </div>
              <div>
                <div className="text-muted-foreground">
                  {t("自动回收模式")}
                </div>
                <div className="mt-0.5">{usage ? modeLabel : "-"}</div>
              </div>
            </div>

            {usage?.purgePending ? (
              <p className="text-xs text-muted-foreground">
                {t("正在后台分批清理已清空的请求内容")}
              </p>
            ) : null}
            {usage?.rebuildRunning ? (
              <p className="text-xs text-amber-600 dark:text-amber-400">
                {t("数据库整理中，请勿关闭应用")}
              </p>
            ) : null}
            {usage?.lastError ? (
              <p className="text-xs text-destructive">
                {t("上次回收失败：{error}", { error: usage.lastError })}
              </p>
            ) : null}

            <div className="flex flex-wrap items-center gap-2">
              {incremental ? (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={!usage || busy || usage.reclaimableBytes <= 0}
                  onClick={() => reclaim.mutate(false)}
                >
                  {usage?.reclaimRunning ? t("回收中...") : t("立即回收")}
                </Button>
              ) : usage && usage.autoVacuum === "none" ? (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => setRebuildConfirmOpen(true)}
                >
                  {t("启用增量回收并整理数据库")}
                </Button>
              ) : null}
            </div>
          </>
        )}
      </CardContent>
      <ConfirmDialog
        open={rebuildConfirmOpen}
        onOpenChange={setRebuildConfirmOpen}
        title={t("整理数据库")}
        description={t(
          "当前数据库创建于旧版本，未启用增量回收。整理会重写整个数据库文件：数据目录和系统临时目录各需要约等于已用空间的空闲磁盘，耗时取决于数据库大小，期间网关写入日志可能短暂等待，建议在低峰时执行。完成后，之后释放的空间都可以在后台逐步回收。",
        )}
        confirmText={t("开始整理")}
        onConfirm={() => reclaim.mutate(true)}
      />
    </Card>
  );
}
