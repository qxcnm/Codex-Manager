"use client";

import { useEffect, useState } from "react";
import { CalendarDays, Gauge, LogOut, RefreshCw } from "lucide-react";
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
import { isAdminRole, resolveSessionRole, useAppSession } from "@/hooks/useAppSession";

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
  const { locale, t } = useI18n();
  const [isToggling, setIsToggling] = useState(false);
  const [portInput, setPortInput] = useState("48760");
  const { canManageService, isDesktopRuntime, mode } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const routeAccess = { role, mode: session?.mode ?? null, isDesktopRuntime };
  const isCommandCenter = currentShellPath === "/" && isAdminRole(role);

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
    if (currentShellPath === "/" && isAdminRole(role)) {
      return t("仪表盘");
    }
    return t(getTopLevelRouteLabel(currentShellPath, routeAccess));
  };

  const currentDate = new Intl.DateTimeFormat(
    locale === "en" ? "en-US" : locale === "ru" ? "ru-RU" : locale === "ko" ? "ko-KR" : "zh-CN",
    { year: "numeric", month: "long", day: "numeric" },
  ).format(new Date());

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
      <header className="sticky top-0 z-30 flex min-h-[68px] items-center justify-between gap-3 glass-header px-5 xl:min-h-[96px] xl:gap-4 xl:pl-9 xl:pr-[45px]">
        <div className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-gradient-to-r from-transparent via-primary/30 to-transparent" />
        <div className="flex min-w-0 items-center gap-4 xl:gap-5">
          <h1 className="truncate text-[21px] font-semibold tracking-[-0.015em] text-foreground xl:text-[27px]">
            {getPageTitle()}
          </h1>
          <span className="hidden items-center gap-2 text-sm text-muted-foreground md:flex xl:text-[15px]" suppressHydrationWarning>
            {!isCommandCenter ? <CalendarDays className="h-3.5 w-3.5" /> : null}
            {currentDate}
          </span>
        </div>

        <div className="ml-auto flex min-w-0 shrink-0 items-center gap-2">
          <div className={`hidden h-10 items-center rounded-full border border-border/55 bg-background/70 px-1.5 shadow-[0_14px_30px_-22px_rgb(15_23_42/0.45)] backdrop-blur-2xl sm:flex xl:h-12 xl:px-2 ${isCommandCenter ? "min-w-[320px] justify-center xl:min-w-[430px]" : ""}`}>
            <Badge
              variant="secondary"
              className="h-8 shrink-0 rounded-full border-0 bg-transparent px-2.5 text-xs font-medium text-foreground shadow-none xl:h-9 xl:px-3.5 xl:text-sm"
            >
              <span className={serviceStatus.connected ? "mr-2 h-2 w-2 rounded-full bg-emerald-500" : "mr-2 h-2 w-2 rounded-full bg-rose-500"} />
              {serviceStatus.connected ? t("服务已连接") : t("服务未连接")}
              {serviceStatus.version ? (
                <span className="ml-2 border-l border-border/70 pl-2 font-mono text-[10px] text-muted-foreground xl:text-xs">
                  v{serviceStatus.version}
                </span>
              ) : null}
            </Badge>

            {canManageService ? (
              <div className="flex h-7 items-center gap-2 border-l border-border/60 px-3 xl:h-8 xl:gap-2.5 xl:px-4">
                <span className="flex items-center gap-1.5 text-xs text-muted-foreground xl:text-sm">
                <Gauge className="h-3.5 w-3.5 text-primary" />
                  <span className="hidden lg:inline">{t("端口")}</span>
                </span>
                <Input
                  className="h-7 w-12 border-0 bg-transparent p-0 font-mono text-xs text-foreground focus-visible:ring-0 xl:h-8 xl:w-14 xl:text-sm"
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

            <Button
              variant="ghost"
              size="sm"
              className="h-8 gap-1.5 rounded-full border-l border-border/60 px-3 text-xs text-muted-foreground hover:bg-primary/5 hover:text-foreground xl:h-9 xl:gap-2 xl:px-4 xl:text-sm"
              onClick={() => window.location.reload()}
              title={t("刷新数据")}
            >
              <RefreshCw className="h-3.5 w-3.5" />
              <span className="hidden lg:inline">{t("刚刚更新")}</span>
            </Button>
          </div>

          <DisclaimerTicker compact />
          <LanguageSwitcher compact triggerClassName="w-[106px]" />

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
