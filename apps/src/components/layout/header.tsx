"use client";

import { useEffect, useState } from "react";
import { Gauge, LogOut, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { useAppStore } from "@/lib/store/useAppStore";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { DisclaimerTicker } from "@/components/layout/disclaimer-ticker";
import { LanguageSwitcher } from "@/components/layout/language-switcher";
import { serviceClient } from "@/lib/api/service-client";
import { appClient } from "@/lib/api/app-client";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import {
  formatServiceError,
  isExpectedInitializeResult,
  normalizeServiceAddr,
} from "@/lib/utils/service";
import { getTopLevelRouteLabel } from "@/lib/app-shell/top-level-routes";
import { resolveSessionRole, useAppSession } from "@/hooks/useAppSession";

const DEFAULT_SERVICE_ADDR = "localhost:48760";

/**
 * 函数 `Header`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * 无
 *
 * # 返回
 * 返回函数执行结果
 */
export function Header() {
  const appSettings = useAppStore((state) => state.appSettings);
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const currentShellPath = useAppStore((state) => state.currentShellPath);
  const setServiceStatus = useAppStore((state) => state.setServiceStatus);
  const setAppSettings = useAppStore((state) => state.setAppSettings);
  const { t } = useI18n();
  const [isToggling, setIsToggling] = useState(false);
  const [portInput, setPortInput] = useState("48760");
  const { canManageService, isDesktopRuntime, mode } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const routeAccess = { role, mode: session?.mode ?? null, isDesktopRuntime };

  useEffect(() => {
    const current = String(serviceStatus.addr || DEFAULT_SERVICE_ADDR);
    const [, port = current] = current.split(":");
    setPortInput(port || "48760");
  }, [serviceStatus.addr]);

  /**
   * 函数 `getPageTitle`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const getPageTitle = () => {
    return t(getTopLevelRouteLabel(currentShellPath, routeAccess));
  };

  const canLogoutWebSession =
    mode === "web-gateway" &&
    (appSettings.webAuthMode !== "none" || !serviceStatus.connected);

  /**
   * 函数 `persistServiceAddr`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - nextAddr: 参数 nextAddr
   *
   * # 返回
   * 返回函数执行结果
   */
  const persistServiceAddr = async (nextAddr: string) => {
    const normalized = normalizeServiceAddr(nextAddr);
    const settings = await appClient.setSettings({ serviceAddr: normalized });
    setAppSettings(settings);
    setServiceStatus({ addr: normalized });
    return normalized;
  };

  /**
   * 函数 `handleToggleService`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - enabled: 参数 enabled
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleToggleService = async (enabled: boolean) => {
    setIsToggling(true);
    try {
      const nextAddr = await persistServiceAddr(serviceStatus.addr || `localhost:${portInput}`);
      if (enabled) {
        await serviceClient.start(nextAddr);
        const initResult = await serviceClient.initialize(nextAddr);
        if (!isExpectedInitializeResult(initResult)) {
          throw new Error("Port is in use or unexpected service responded (invalid initialize response)");
        }
        setServiceStatus({
          connected: true,
          version: initResult.version,
          addr: nextAddr,
        });
        toast.success(t("服务已启动"));
      } else {
        await serviceClient.stop();
        setServiceStatus({ connected: false, version: "" });
        toast.info(t("服务已停止"));
      }
    } catch (error: unknown) {
      toast.error(`${t("操作失败")}: ${formatServiceError(error)}`);
    } finally {
      setIsToggling(false);
    }
  };

  /**
   * 函数 `handlePortBlur`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const handlePortBlur = async () => {
    try {
      const nextAddr = await persistServiceAddr(`localhost:${portInput}`);
      setServiceStatus({ addr: nextAddr });
    } catch (error: unknown) {
      toast.error(`${t("保存失败")}: ${formatServiceError(error)}`);
    }
  };

  const handleLogout = () => {
    if (typeof window === "undefined") return;
    window.location.assign("/__logout");
  };

  return (
    <>
      <header className="sticky top-0 z-30 flex min-h-[50px] items-center justify-between gap-2 glass-header px-3 sm:gap-3 sm:px-4 lg:px-6">
        <div className="header-title-group flex min-w-0 flex-1 items-center overflow-hidden">
          <h1 className="header-page-title min-w-0 truncate text-sm font-medium text-muted-foreground">
            {getPageTitle()}
          </h1>
        </div>

        <div className="header-action-cluster ml-auto flex max-w-full min-w-0 shrink-0 items-center gap-1.5 sm:gap-2">
          <span
            className="flex size-8 items-center justify-center sm:hidden"
            title={serviceStatus.connected ? t("服务已连接") : t("服务未连接")}
            aria-label={serviceStatus.connected ? t("服务已连接") : t("服务未连接")}
          >
            <span className={serviceStatus.connected ? "size-2 rounded-full bg-emerald-500" : "size-2 rounded-full bg-rose-500"} />
          </span>
          <div className="header-service-strip hidden h-9 min-w-0 items-center rounded-md border border-border/60 bg-background/60 px-1 sm:flex">
            <Badge
              variant="secondary"
              className="header-service-badge h-8 shrink-0 rounded-md border-0 bg-transparent px-2 text-xs font-medium text-foreground shadow-none"
            >
              <span className={serviceStatus.connected ? "mr-2 size-2 rounded-full bg-emerald-500" : "mr-2 size-2 rounded-full bg-rose-500"} />
              <span className="header-service-status-label">
                {serviceStatus.connected ? t("服务已连接") : t("服务未连接")}
              </span>
              {serviceStatus.version ? (
                <span className="header-service-version ml-2 border-l border-border/70 pl-2 font-mono text-xs text-muted-foreground">
                  v{serviceStatus.version}
                </span>
              ) : null}
            </Badge>

            {canManageService ? (
              <div className="header-service-port flex h-7 shrink-0 items-center gap-1.5 border-l border-border/60 px-2">
                <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                  <Gauge className="size-3.5 text-primary" />
                  <span className="header-service-port-label hidden lg:inline">{t("端口")}</span>
                </span>
                <Input
                  className="h-7 w-12 border-0 bg-transparent p-0 font-mono text-xs text-foreground focus-visible:ring-0"
                  placeholder="48760"
                  value={portInput}
                  onChange={(event) => {
                    const nextPort = event.target.value.replace(/[^\d]/g, "");
                    setPortInput(nextPort);
                    if (nextPort) setServiceStatus({ addr: `localhost:${nextPort}` });
                  }}
                  onBlur={() => void handlePortBlur()}
                />
                <Switch
                  checked={serviceStatus.connected}
                  disabled={isToggling}
                  onCheckedChange={handleToggleService}
                  className="scale-90"
                />
              </div>
            ) : null}
          </div>

          <Button
            variant="ghost"
            size="icon"
            className="header-service-refresh hidden size-9 text-muted-foreground hover:text-foreground sm:inline-flex"
            onClick={() => window.location.reload()}
            title={t("刷新数据")}
            aria-label={t("刷新数据")}
          >
            <RefreshCw />
          </Button>

          <DisclaimerTicker compact />
          <LanguageSwitcher
            compact
            className="header-language-switcher"
            triggerClassName="w-[124px] min-w-[124px] gap-2 px-2.5"
          />

          {canLogoutWebSession ? (
            <Button
              variant="ghost"
              size="sm"
              className="h-9 gap-2 rounded-md px-2.5 text-muted-foreground hover:bg-destructive/10 hover:text-destructive xl:px-3"
              onClick={handleLogout}
              title={t("退出登录")}
              aria-label={t("退出登录")}
            >
              <LogOut className="h-3.5 w-3.5" />
              <span className="hidden text-xs sm:inline">{t("退出登录")}</span>
            </Button>
          ) : null}
        </div>
      </header>
    </>
  );
}
