"use client";

import { useEffect, useState } from "react";
import { Cable, Gauge, LogOut } from "lucide-react";
import { toast } from "sonner";
import { useAppStore } from "@/lib/store/useAppStore";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
  const routeAccess = { role, mode: session?.mode ?? null };

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
          version: "",
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
      <header className="sticky top-0 z-30 flex min-h-16 items-center gap-4 glass-header px-4 xl:px-5">
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-gradient-to-r from-transparent via-primary/30 to-transparent" />
        <div className="flex min-w-0 flex-1 items-center gap-3 overflow-hidden">
          <span className="h-8 w-px shrink-0 bg-gradient-to-b from-cyan-400/20 via-primary/70 to-violet-500/20" />
          <div className="min-w-0">
            <p className="hidden items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-primary/75 sm:flex">
              <Cable className="h-3 w-3" />
              OpenRuntime · Control Plane
            </p>
            <h1 className="truncate text-lg font-semibold text-foreground">{getPageTitle()}</h1>
          </div>
          <Badge
            variant={serviceStatus.connected ? "default" : "secondary"}
            className="h-7 shrink-0 rounded-md border-primary/20 bg-primary/10 px-2.5 font-mono text-[11px] text-primary shadow-sm"
          >
            <span
              className={`mr-1.5 h-1.5 w-1.5 rounded-full ${
                serviceStatus.connected ? "bg-emerald-500" : "bg-rose-500"
              }`}
            />
            {serviceStatus.connected ? "Runtime Online" : "Runtime Offline"}
          </Badge>
          {serviceStatus.version ? (
            <span className="hidden font-mono text-xs text-muted-foreground 2xl:inline">v{serviceStatus.version}</span>
          ) : null}
        </div>

        <div className="ml-auto flex min-w-0 shrink-0 items-center gap-2 justify-self-end">
          <LanguageSwitcher compact triggerClassName="w-[112px] 2xl:w-[124px]" />

          {canManageService ? (
            <div className="flex h-9 items-center gap-2 rounded-md border border-border/60 bg-background/55 px-2.5 shadow-[inset_0_1px_0_rgb(255_255_255/0.16)]">
              <span className="flex items-center gap-1.5 font-mono text-[10px] font-medium uppercase text-muted-foreground">
                <Gauge className="h-3.5 w-3.5 text-primary" />
                <span className="hidden 2xl:inline">{t("监听端口")}</span>
              </span>
              <Input
                className="h-7 w-14 border-0 bg-transparent p-0 font-mono text-xs text-primary focus-visible:ring-0"
                placeholder="48760"
                value={portInput}
                onChange={(event) => {
                  const nextPort = event.target.value.replace(/[^\d]/g, "");
                  setPortInput(nextPort);
                  if (nextPort) {
                    setServiceStatus({ addr: `localhost:${nextPort}` });
                  }
                }}
                onBlur={() => void handlePortBlur()}
              />
              <div className="mx-1 h-4 w-px bg-primary/25" />
              <Switch
                checked={serviceStatus.connected}
                disabled={isToggling}
                onCheckedChange={handleToggleService}
                className="scale-90"
              />
            </div>
          ) : null}

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
