"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Bug, FolderOpen, TerminalSquare } from "lucide-react";
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
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  getDesktopDiagnostics,
  openDesktopDiagnosticsLogsDir,
  setDesktopDiagnostics,
  type DesktopDiagnosticsSettingsPatch,
} from "@/lib/api/desktop-diagnostics";
import { getAppErrorMessage } from "@/lib/api/transport";

const DESKTOP_DIAGNOSTICS_QUERY_KEY = ["desktop-diagnostics"] as const;

export function DesktopDiagnosticsCard({
  t,
  active = true,
}: {
  t: (value: string) => string;
  active?: boolean;
}) {
  const queryClient = useQueryClient();
  const diagnostics = useQuery({
    queryKey: DESKTOP_DIAGNOSTICS_QUERY_KEY,
    queryFn: getDesktopDiagnostics,
    enabled: active,
  });
  const updateDiagnostics = useMutation({
    mutationFn: (patch: DesktopDiagnosticsSettingsPatch) =>
      setDesktopDiagnostics(patch),
    onSuccess: (snapshot) => {
      queryClient.setQueryData(DESKTOP_DIAGNOSTICS_QUERY_KEY, snapshot);
      toast.success(t("诊断设置已更新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新诊断设置失败")}: ${getAppErrorMessage(error)}`);
    },
  });
  const openLogs = useMutation({
    mutationFn: openDesktopDiagnosticsLogsDir,
    onError: (error: unknown) => {
      toast.error(`${t("打开日志目录失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const snapshot = diagnostics.data;
  const controlsDisabled = diagnostics.isLoading || updateDiagnostics.isPending;

  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader>
        <div className="flex items-center gap-2">
          <Bug className="h-4 w-4 text-primary" />
          <CardTitle className="text-base">{t("桌面诊断")}</CardTitle>
        </div>
        <CardDescription>
          {t("记录桌面启动异常，并控制本地运行日志")}
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-5">
        {diagnostics.isError ? (
          <Alert variant="destructive">
            <AlertTitle>{t("诊断信息读取失败")}</AlertTitle>
            <AlertDescription>
              {getAppErrorMessage(diagnostics.error)}
            </AlertDescription>
          </Alert>
        ) : null}

        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0 space-y-0.5">
            <Label>{t("Debug 模式")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("记录更详细的桌面启动与运行信息，设置在重启后继续生效")}
            </p>
          </div>
          <Switch
            aria-label={t("Debug 模式")}
            checked={snapshot?.debugMode ?? false}
            disabled={controlsDisabled}
            onCheckedChange={(debugMode) =>
              updateDiagnostics.mutate({ debugMode })
            }
          />
        </div>

        {snapshot?.debugModeForced ? (
          <Alert>
            <TerminalSquare className="h-4 w-4" />
            <AlertDescription>
              {t("本次启动已通过 --debug 临时启用 Debug 模式和文件日志")}
            </AlertDescription>
          </Alert>
        ) : null}

        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0 space-y-0.5">
            <Label>{t("本地文件日志")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("关闭后停止写入桌面运行日志；请求日志、Token 与费用统计不受影响")}
            </p>
          </div>
          <Switch
            aria-label={t("本地文件日志")}
            checked={snapshot?.fileLoggingEnabled ?? true}
            disabled={controlsDisabled}
            onCheckedChange={(fileLoggingEnabled) =>
              updateDiagnostics.mutate({ fileLoggingEnabled })
            }
          />
        </div>

        <div className="flex flex-col gap-2 border-t pt-4 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <p className="text-xs text-muted-foreground">
              {t("运行日志单文件最多 512 KB，达到上限后自动覆盖")}
            </p>
            {snapshot?.logDir ? (
              <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
                {snapshot.logDir}
              </p>
            ) : null}
          </div>
          <Button
            variant="outline"
            className="shrink-0 gap-2 self-start sm:self-auto"
            disabled={openLogs.isPending}
            onClick={() => openLogs.mutate()}
          >
            <FolderOpen className="h-4 w-4" />
            {t("打开日志目录")}
          </Button>
        </div>

        {snapshot?.startupError ? (
          <Alert variant="destructive">
            <AlertTitle>{t("最近一次启动失败")}</AlertTitle>
            <AlertDescription>
              <pre className="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-words font-mono text-xs">
                {snapshot.startupError}
              </pre>
            </AlertDescription>
          </Alert>
        ) : null}
      </CardContent>
    </Card>
  );
}
