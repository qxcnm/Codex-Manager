"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Download, ExternalLink } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { useI18n } from "@/lib/i18n/provider";
import { appClient } from "@/lib/api/app-client";
import type {
  UpdateCheckResult,
  UpdatePrepareResult,
} from "@/lib/api/app-updates";
import { getAppErrorMessage } from "@/lib/api/transport";
import { reportClientError, reportClientWarning } from "@/lib/client-logger";

export const AUTO_UPDATE_CHECK_INTERVAL_MS = 7 * 60 * 60 * 1_000;
export const AUTO_UPDATE_INITIAL_DELAY_MS = 5_000;
export const AUTO_UPDATE_IDLE_TIMEOUT_MS = 30_000;
export const AUTO_UPDATE_LAST_CHECK_AT_KEY =
  "codexmanager.update.lastAutomaticCheckAt";

let automaticCheckInFlight: Promise<UpdateCheckResult> | null = null;

function readLastAutomaticCheckAt(): number | null {
  try {
    const value = Number.parseInt(
      window.localStorage.getItem(AUTO_UPDATE_LAST_CHECK_AT_KEY) || "",
      10,
    );
    return Number.isFinite(value) && value > 0 ? value : null;
  } catch {
    return null;
  }
}

function recordAutomaticCheckCompleted(at: number) {
  try {
    window.localStorage.setItem(AUTO_UPDATE_LAST_CHECK_AT_KEY, String(at));
  } catch {
    // Storage can be unavailable in restricted embedded runtimes. The in-memory timer still works.
  }
}

function initialAutomaticCheckDelay(now: number): number {
  const lastCheckAt = readLastAutomaticCheckAt();
  if (!lastCheckAt || lastCheckAt > now) {
    return AUTO_UPDATE_INITIAL_DELAY_MS;
  }

  const remaining = AUTO_UPDATE_CHECK_INTERVAL_MS - (now - lastCheckAt);
  return remaining > 0
    ? Math.max(AUTO_UPDATE_INITIAL_DELAY_MS, remaining)
    : AUTO_UPDATE_INITIAL_DELAY_MS;
}

function checkForUpdate(): Promise<UpdateCheckResult> {
  if (!automaticCheckInFlight) {
    automaticCheckInFlight = appClient.checkUpdate().finally(() => {
      automaticCheckInFlight = null;
    });
  }
  return automaticCheckInFlight;
}

function buildReleaseUrl(summary: UpdateCheckResult): string {
  if (!summary.repo) {
    return "https://github.com/qxcnm/Codex-Manager/releases";
  }
  const tag =
    summary.releaseTag ||
    (summary.latestVersion ? `v${summary.latestVersion}` : "");
  return tag
    ? `https://github.com/${summary.repo}/releases/tag/${tag}`
    : `https://github.com/${summary.repo}/releases`;
}

export function AutomaticUpdateChecker() {
  const { t } = useI18n();
  const activeRef = useRef(false);
  const [updateCheck, setUpdateCheck] = useState<UpdateCheckResult | null>(null);
  const [preparedUpdate, setPreparedUpdate] =
    useState<UpdatePrepareResult | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [isPreparing, setIsPreparing] = useState(false);
  const [isApplying, setIsApplying] = useState(false);

  const runCheck = useCallback(async () => {
    try {
      const summary = await checkForUpdate();
      if (!activeRef.current) return;

      if (!summary.hasUpdate) {
        recordAutomaticCheckCompleted(Date.now());
        return;
      }
      await appClient.showMainWindow().catch((error: unknown) => {
        reportClientWarning("update_show_main_window_failed", error);
      });
      if (!activeRef.current) return;

      recordAutomaticCheckCompleted(Date.now());
      setUpdateCheck(summary);
      setPreparedUpdate((current) =>
        current?.latestVersion === summary.latestVersion ? current : null,
      );
      setDialogOpen(true);
    } catch (error: unknown) {
      // Automatic checks are deliberately silent. The next scheduled check can retry.
      reportClientWarning("automatic_update_check_failed", error);
    }
  }, []);

  useEffect(() => {
    activeRef.current = true;
    let disposed = false;
    let timeoutId: number | null = null;
    let idleCallbackId: number | null = null;

    const scheduleCheck = (delayMs: number) => {
      timeoutId = window.setTimeout(() => {
        timeoutId = null;

        const checkWhenIdle = () => {
          idleCallbackId = null;
          if (disposed) return;

          void runCheck().finally(() => {
            if (!disposed) {
              scheduleCheck(AUTO_UPDATE_CHECK_INTERVAL_MS);
            }
          });
        };

        if (typeof window.requestIdleCallback === "function") {
          idleCallbackId = window.requestIdleCallback(checkWhenIdle, {
            timeout: AUTO_UPDATE_IDLE_TIMEOUT_MS,
          });
          return;
        }

        checkWhenIdle();
      }, delayMs);
    };

    scheduleCheck(initialAutomaticCheckDelay(Date.now()));

    return () => {
      activeRef.current = false;
      disposed = true;
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId);
      }
      if (
        idleCallbackId !== null &&
        typeof window.cancelIdleCallback === "function"
      ) {
        window.cancelIdleCallback(idleCallbackId);
      }
    };
  }, [runCheck]);

  const prepareUpdate = async () => {
    setIsPreparing(true);
    try {
      const summary = await appClient.prepareUpdate();
      setPreparedUpdate(summary);
      toast.success(
        summary.isPortable
          ? `${t("更新已下载完成，确认后即可替换到")} ${summary.latestVersion || t("新版本")}`
          : `${t("更新包已下载完成，确认后开始替换到")} ${summary.latestVersion || t("新版本")}`,
      );
    } catch (error: unknown) {
      reportClientError("update_prepare_failed", error);
      toast.error(`${t("下载更新失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setIsPreparing(false);
    }
  };

  const applyUpdate = async () => {
    if (!preparedUpdate) return;
    setIsApplying(true);
    try {
      const result = preparedUpdate.isPortable
        ? await appClient.applyUpdatePortable()
        : await appClient.launchInstaller();
      setDialogOpen(false);
      toast.success(
        result.message.trim() ||
          (preparedUpdate.isPortable
            ? t("即将重启并替换更新")
            : t("已开始替换更新流程")),
      );
    } catch (error: unknown) {
      reportClientError("update_apply_failed", error);
      toast.error(`${t("替换更新")}${t("失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setIsApplying(false);
    }
  };

  const openReleasePage = async () => {
    if (!updateCheck) return;
    try {
      await appClient.openInBrowser(buildReleaseUrl(updateCheck));
      setDialogOpen(false);
    } catch (error: unknown) {
      reportClientWarning("update_release_page_open_failed", error);
      toast.error(`${t("打开发布页失败")}: ${getAppErrorMessage(error)}`);
    }
  };

  if (!updateCheck) {
    return null;
  }

  const busy = isPreparing || isApplying;
  return (
    <Dialog
      open={dialogOpen}
      onOpenChange={(open) => {
        if (!busy) setDialogOpen(open);
      }}
    >
      <DialogContent
        showCloseButton={false}
        className="glass-card mission-panel p-6 sm:max-w-[480px]"
      >
        <DialogHeader>
          <DialogTitle>
            {preparedUpdate ? t("替换更新") : t("发现新版本")}
          </DialogTitle>
          <DialogDescription>
            {preparedUpdate
              ? preparedUpdate.isPortable
                ? t("更新包已下载完成。确认后将重启应用并替换当前程序。")
                : t("更新包已下载完成。确认后会开始替换流程。")
              : `${t("当前版本")} ${updateCheck.currentVersion || t("未知")}，${t("发现新版本")} ${
                  updateCheck.latestVersion || updateCheck.releaseTag || t("可用")
                }。`}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3 text-sm">
          <Card size="sm">
            <CardContent>
              <div className="flex items-center justify-between gap-4">
                <span className="text-muted-foreground">{t("当前版本")}</span>
                <span className="font-medium">
                  {updateCheck.currentVersion || t("未知")}
                </span>
              </div>
              <Separator className="my-2" />
              <div className="flex items-center justify-between gap-4">
                <span className="text-muted-foreground">{t("目标版本")}</span>
                <span className="font-medium">
                  {preparedUpdate?.latestVersion ||
                    updateCheck.latestVersion ||
                    updateCheck.releaseTag ||
                    t("未知")}
                </span>
              </div>
            </CardContent>
          </Card>
          <Alert>
            <AlertDescription className="text-xs leading-5">
              {preparedUpdate
                ? t("更新包已准备完成，是否立即替换更新？")
                : t("检测到新版本，是否现在更新？")}
            </AlertDescription>
          </Alert>
        </div>

        <DialogFooter className="gap-2 sm:gap-2">
          <Button
            variant="outline"
            className="update-dialog-later-button"
            disabled={busy}
            onClick={() => setDialogOpen(false)}
          >
            {t("稍后")}
          </Button>
          {preparedUpdate ? (
            <Button className="gap-2" disabled={isApplying} onClick={applyUpdate}>
              <Download className="h-4 w-4" />
              {isApplying ? t("正在替换更新...") : t("替换更新")}
            </Button>
          ) : updateCheck.canPrepare ? (
            <Button className="gap-2" disabled={isPreparing} onClick={prepareUpdate}>
              <Download className="h-4 w-4" />
              {isPreparing ? t("正在下载更新...") : t("下载更新")}
            </Button>
          ) : (
            <Button className="gap-2" onClick={openReleasePage}>
              <ExternalLink className="h-4 w-4" />
              {t("打开发布页")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
