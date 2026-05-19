"use client";

import { useCallback, useEffect, useState, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { usePathname } from "next/navigation";
import { AlertCircle, Play, RefreshCw } from "lucide-react";
import { useTheme } from "next-themes";
import { toast } from "sonner";
import { useAppStore } from "@/lib/store/useAppStore";
import { serviceClient } from "@/lib/api/service-client";
import {
  buildStartupSnapshotQueryKey,
  STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
  STARTUP_SNAPSHOT_STALE_TIME,
} from "@/lib/api/startup-snapshot";
import { appClient } from "@/lib/api/app-client";
import { loadRuntimeCapabilities } from "@/lib/api/transport";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { CodexCliOnboardingDialog } from "@/components/layout/codex-cli-onboarding-dialog";
import { applyAppearancePreset } from "@/lib/appearance";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useLocalDayRange } from "@/hooks/useLocalDayRange";
import { isTrayPreviewPath } from "@/components/layout/app-frame";
import {
  formatServiceError,
  isServicePortConflictError,
  isExpectedInitializeResult,
  normalizeServiceAddr,
} from "@/lib/utils/service";
import { useI18n } from "@/lib/i18n/provider";
import {
  getCanonicalStaticRouteUrl,
  normalizeRoutePath,
} from "@/lib/utils/static-routes";
import { withTimeout } from "@/lib/utils/timeout";

const DEFAULT_SERVICE_ADDR = "localhost:48760";
const STARTUP_WARMUP_LABEL = "[startup warmup]";
const STARTUP_STEP_TIMEOUT_MS = 15_000;
const CODEX_CLI_GUIDE_SESSION_DISMISSED_KEY =
  "codexmanager.codexCliGuide.sessionDismissed";
const UNSUPPORTED_RUNTIME_AUTO_RETRY_LIMIT = 8;
const UNSUPPORTED_RUNTIME_AUTO_RETRY_DELAY_MS = 1500;

function readCodexCliGuideSessionDismissed() {
  if (typeof window === "undefined") {
    return false;
  }

  try {
    return (
      window.sessionStorage.getItem(CODEX_CLI_GUIDE_SESSION_DISMISSED_KEY) === "1"
    );
  } catch {
    return false;
  }
}

function writeCodexCliGuideSessionDismissed(dismissed: boolean) {
  if (typeof window === "undefined") {
    return;
  }

  try {
    if (dismissed) {
      window.sessionStorage.setItem(CODEX_CLI_GUIDE_SESSION_DISMISSED_KEY, "1");
      return;
    }
    window.sessionStorage.removeItem(CODEX_CLI_GUIDE_SESSION_DISMISSED_KEY);
  } catch {
    // Some embedded/web runtimes can deny storage; in-memory state still handles the current render.
  }
}
/**
 * 函数 `sleep`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - ms: 参数 ms
 *
 * # 返回
 * 返回函数执行结果
 */
const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

function readPortFromServiceAddr(addr: string): string {
  const normalized = normalizeServiceAddr(addr || DEFAULT_SERVICE_ADDR);
  return normalized.split(":").pop() || "48760";
}

function isValidServicePort(value: string): boolean {
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) {
    return false;
  }
  const port = Number.parseInt(trimmed, 10);
  return Number.isInteger(port) && port >= 1 && port <= 65535;
}

/**
 * 函数 `AppBootstrap`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - params: 参数 params
 *
 * # 返回
 * 返回函数执行结果
 */
export function AppBootstrap({ children }: { children: React.ReactNode }) {
  const {
    setServiceStatus,
    setAppSettings,
    setRuntimeCapabilities,
    closeCodexCliGuide,
    serviceStatus,
    appSettings,
    isCodexCliGuideOpen,
    runtimeCapabilities,
  } = useAppStore();
  const { setTheme } = useTheme();
  const { t } = useI18n();
  const localDayRange = useLocalDayRange();
  const queryClient = useQueryClient();
  const pathname = usePathname();
  const isTrayPreview = isTrayPreviewPath(pathname);
  const { canManageService, isDesktopRuntime, isUnsupportedWebRuntime } =
    useRuntimeCapabilities();
  const [isInitializing, setIsInitializing] = useState(true);
  const hasInitializedOnce = useRef(false);
  const hasBootstrappedOnce = useRef(false);
  const unsupportedRuntimeRetryCountRef = useRef(0);
  const serviceStatusRef = useRef(serviceStatus);
  const runtimeCapabilitiesRef = useRef(runtimeCapabilities);
  const [error, setError] = useState<string | null>(null);
  const [recoveryPort, setRecoveryPort] = useState(() =>
    readPortFromServiceAddr(DEFAULT_SERVICE_ADDR),
  );
  const [isRecoveringPort, setIsRecoveringPort] = useState(false);
  const [guideSessionDismissed, setGuideSessionDismissedState] = useState(
    readCodexCliGuideSessionDismissed
  );
  const supportsLocalServiceStart = canManageService;

  const dismissCodexCliGuideForSession = useCallback(() => {
    setGuideSessionDismissedState(true);
    writeCodexCliGuideSessionDismissed(true);
  }, []);

  useEffect(() => {
    serviceStatusRef.current = serviceStatus;
  }, [serviceStatus]);

  useEffect(() => {
    runtimeCapabilitiesRef.current = runtimeCapabilities;
  }, [runtimeCapabilities]);

  /**
   * 函数 `applyLowTransparency`
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
  const applyLowTransparency = (enabled: boolean) => {
    if (enabled) {
      document.body.classList.add("low-transparency");
    } else {
      document.body.classList.remove("low-transparency");
    }
  };

  const initializeService = useCallback(async (addr: string, retries = 0) => {
    let lastError: unknown = null;

    for (let attempt = 0; attempt <= retries; attempt += 1) {
      try {
        const initializeResult = await withTimeout(
          serviceClient.initialize(addr),
          STARTUP_STEP_TIMEOUT_MS,
          `Service initialization timed out after ${STARTUP_STEP_TIMEOUT_MS / 1000}s (${addr})`,
        );
        if (!isExpectedInitializeResult(initializeResult)) {
          throw new Error("Port is in use or unexpected service responded (invalid initialize response)");
        }
        return initializeResult;
      } catch (serviceError: unknown) {
        lastError = serviceError;
        if (attempt < retries) {
          await sleep(300);
        }
      }
    }

    throw lastError || new Error(t("服务初始化失败: {addr}", { addr }));
  }, [t]);

  const startAndInitializeService = useCallback(
    async (addr: string) => {
      await withTimeout(
        serviceClient.start(addr),
        STARTUP_STEP_TIMEOUT_MS,
        `Service start timed out after ${STARTUP_STEP_TIMEOUT_MS / 1000}s (${addr})`,
      );
      return initializeService(addr, 0);
    },
    [initializeService]
  );

  const prefetchStartupSnapshot = useCallback(
    async (addr: string) => {
      await queryClient.prefetchQuery({
        queryKey: buildStartupSnapshotQueryKey(
          addr,
          STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
          localDayRange.dayStartTs,
        ),
        queryFn: () =>
          serviceClient.getStartupSnapshot({
            requestLogLimit: STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
            dayStartTs: localDayRange.dayStartTs,
            dayEndTs: localDayRange.dayEndTs,
          }),
        staleTime: STARTUP_SNAPSHOT_STALE_TIME,
      });
    },
    [localDayRange.dayEndTs, localDayRange.dayStartTs, queryClient]
  );

  const shouldBlockOnInitialDashboardSnapshot = useCallback(
    (desktopRuntime: boolean) =>
      desktopRuntime &&
      !hasInitializedOnce.current &&
      normalizeRoutePath(pathname) === "/",
    [pathname],
  );

  const applyConnectedServiceState = useCallback(
    (
      addr: string,
      version: string,
      lowTransparency: boolean,
      options?: { blockOnDashboardSnapshot?: boolean },
    ) => {
      setServiceStatus({
        addr,
        connected: true,
        version,
      });
      applyLowTransparency(lowTransparency);
      setIsInitializing(false);
      hasInitializedOnce.current = true;

      if (options?.blockOnDashboardSnapshot) {
        void withTimeout(
          prefetchStartupSnapshot(addr),
          STARTUP_STEP_TIMEOUT_MS,
          `Startup dashboard snapshot prefetch timed out after ${STARTUP_STEP_TIMEOUT_MS / 1000}s`,
        ).catch((warmupError) => {
          console.warn(
            `${STARTUP_WARMUP_LABEL} initial dashboard snapshot prefetch failed`,
            warmupError,
          );
        });
      }
    },
    [prefetchStartupSnapshot, setServiceStatus],
  );

  const init = useCallback(async () => {
    // Only show full screen loading if we haven't initialized once
    if (!hasInitializedOnce.current) {
      setIsInitializing(true);
    }
    setError(null);

    try {
      const detectedRuntimeCapabilities = await withTimeout(
        loadRuntimeCapabilities(
          runtimeCapabilitiesRef.current?.mode === "unsupported-web"
        ),
        STARTUP_STEP_TIMEOUT_MS,
        `Runtime detection timed out after ${STARTUP_STEP_TIMEOUT_MS / 1000}s`,
      );
      setRuntimeCapabilities(detectedRuntimeCapabilities);
      const desktopRuntime = detectedRuntimeCapabilities.mode === "desktop-tauri";
      const shouldBlockOnDashboardSnapshot =
        shouldBlockOnInitialDashboardSnapshot(desktopRuntime);

      if (detectedRuntimeCapabilities.mode === "unsupported-web") {
        if (!hasInitializedOnce.current) {
          setServiceStatus({ connected: false, version: "" });
          setError(
            detectedRuntimeCapabilities.unsupportedReason ||
              t("当前 Web 运行方式不受支持")
          );
        }
        setIsInitializing(false);
        return;
      }
      unsupportedRuntimeRetryCountRef.current = 0;

      const settings = await withTimeout(
        appClient.getSettings(),
        STARTUP_STEP_TIMEOUT_MS,
        `Loading app settings timed out after ${STARTUP_STEP_TIMEOUT_MS / 1000}s`,
      );
      const addr = normalizeServiceAddr(settings.serviceAddr || DEFAULT_SERVICE_ADDR);
      setRecoveryPort(readPortFromServiceAddr(addr));
      const currentServiceStatus = serviceStatusRef.current;
      
      const currentAppliedTheme = typeof document !== 'undefined' ? document.documentElement.getAttribute('data-theme') : null;
      if (settings.theme && settings.theme !== currentAppliedTheme) {
        setTheme(settings.theme);
      }
      applyAppearancePreset(settings.appearancePreset);
      
      setAppSettings(settings);
      
      // CRITICAL: Do not reset status to connected: false if we are already connected
      // This prevents the Header badge from flashing
      if (!currentServiceStatus.connected || currentServiceStatus.addr !== addr) {
        setServiceStatus({ addr, connected: false, version: "" });
      }

      try {
        try {
          await initializeService(addr, 0);
        } catch (initializeError) {
          if (!desktopRuntime) {
            throw initializeError;
          }
          await startAndInitializeService(addr);
        }
        await applyConnectedServiceState(
          addr,
          "",
          settings.lowTransparency,
          { blockOnDashboardSnapshot: shouldBlockOnDashboardSnapshot },
        );
      } catch (serviceError: unknown) {
        if (!hasInitializedOnce.current) {
           setServiceStatus({ addr, connected: false, version: "" });
           setError(formatServiceError(serviceError));
        }
        setIsInitializing(false);
      }
    } catch (appError: unknown) {
      if (!hasInitializedOnce.current) {
        setError(appError instanceof Error ? appError.message : String(appError));
      }
      setIsInitializing(false);
    }
    // 使用 ref 读取最新 serviceStatus，避免把初始化流程绑定到状态抖动上
  }, [
    applyConnectedServiceState,
    initializeService,
    setAppSettings,
    setRuntimeCapabilities,
    setServiceStatus,
    setTheme,
    startAndInitializeService,
    shouldBlockOnInitialDashboardSnapshot,
    t,
  ]);

  /**
   * 函数 `handleForceStart`
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
  const handleForceStart = async () => {
    if (!supportsLocalServiceStart) {
      void init();
      return;
    }

    setIsInitializing(true);
    setError(null);
    try {
      const addr = normalizeServiceAddr(serviceStatus.addr || DEFAULT_SERVICE_ADDR);
      const settings = await appClient.setSettings({ serviceAddr: addr });
      
      const currentAppliedTheme = typeof document !== 'undefined' ? document.documentElement.getAttribute('data-theme') : null;
      if (settings.theme && settings.theme !== currentAppliedTheme) {
        setTheme(settings.theme);
      }
      applyAppearancePreset(settings.appearancePreset);
      
      setAppSettings(settings);
      await startAndInitializeService(addr);
      await applyConnectedServiceState(
        addr,
        "",
        settings.lowTransparency,
        {
          blockOnDashboardSnapshot:
            shouldBlockOnInitialDashboardSnapshot(true),
        },
      );
      toast.success(t("服务已连接"));
    } catch (startError: unknown) {
      setServiceStatus({ connected: false, version: "" });
      setError(formatServiceError(startError));
      setIsInitializing(false);
    }
  };

  const handleRecoverWithPort = async () => {
    if (!supportsLocalServiceStart) {
      return;
    }
    const nextPort = recoveryPort.trim();
    if (!isValidServicePort(nextPort)) {
      toast.error(t("请输入 1-65535 之间的端口"));
      return;
    }

    const nextAddr = normalizeServiceAddr(`localhost:${nextPort}`);
    setIsRecoveringPort(true);
    setIsInitializing(true);
    setError(null);
    try {
      const settings = await appClient.setSettings({ serviceAddr: nextAddr });
      const currentAppliedTheme =
        typeof document !== "undefined"
          ? document.documentElement.getAttribute("data-theme")
          : null;
      if (settings.theme && settings.theme !== currentAppliedTheme) {
        setTheme(settings.theme);
      }
      applyAppearancePreset(settings.appearancePreset);
      setAppSettings(settings);
      setServiceStatus({ addr: nextAddr, connected: false, version: "" });
      await startAndInitializeService(nextAddr);
      await applyConnectedServiceState(
        nextAddr,
        "",
        settings.lowTransparency,
        {
          blockOnDashboardSnapshot:
            shouldBlockOnInitialDashboardSnapshot(true),
        },
      );
      toast.success(t("服务已连接"));
    } catch (recoverError: unknown) {
      setServiceStatus({ addr: nextAddr, connected: false, version: "" });
      setError(formatServiceError(recoverError));
      setIsInitializing(false);
    } finally {
      setIsRecoveringPort(false);
    }
  };

  const handleGuideOpenChange = useCallback((open: boolean) => {
    if (open) {
      return;
    }
    if (isCodexCliGuideOpen) {
      closeCodexCliGuide();
      return;
    }
    dismissCodexCliGuideForSession();
  }, [closeCodexCliGuide, dismissCodexCliGuideForSession, isCodexCliGuideOpen]);

  const handleGuideAcknowledge = useCallback(
    async (dismissPermanently: boolean) => {
      if (dismissPermanently) {
        try {
          const settings = await appClient.setSettings({
            codexCliGuideDismissed: true,
          });
          setAppSettings(settings);
          toast.success(t("后续将不再显示这份引导"));
        } catch (guideError: unknown) {
          const message =
            guideError instanceof Error ? guideError.message : String(guideError);
          toast.error(t("保存引导状态失败: {message}", { message }));
          throw guideError;
        }
      }

      closeCodexCliGuide();
      dismissCodexCliGuideForSession();
    },
    [closeCodexCliGuide, dismissCodexCliGuideForSession, setAppSettings, t]
  );

  useEffect(() => {
    if (hasBootstrappedOnce.current) {
      return;
    }
    hasBootstrappedOnce.current = true;
    void init();
  }, [init]);

  useEffect(() => {
    if (
      hasInitializedOnce.current ||
      isInitializing ||
      !error ||
      !isUnsupportedWebRuntime ||
      typeof window === "undefined" ||
      unsupportedRuntimeRetryCountRef.current >=
        UNSUPPORTED_RUNTIME_AUTO_RETRY_LIMIT
    ) {
      return;
    }

    const retryId = window.setTimeout(() => {
      unsupportedRuntimeRetryCountRef.current += 1;
      void init();
    }, UNSUPPORTED_RUNTIME_AUTO_RETRY_DELAY_MS);
    return () => window.clearTimeout(retryId);
  }, [error, init, isInitializing, isUnsupportedWebRuntime]);

  useEffect(() => {
    if (isDesktopRuntime || typeof window === "undefined") {
      return;
    }

    const canonicalUrl = getCanonicalStaticRouteUrl();
    if (!canonicalUrl) {
      return;
    }

    window.history.replaceState(window.history.state, "", canonicalUrl);
  }, [isDesktopRuntime, pathname]);

  const showLoading = isInitializing && !hasInitializedOnce.current;
  const showError = !!error && !hasInitializedOnce.current;
  const showPortRecovery =
    showError &&
    supportsLocalServiceStart &&
    !isUnsupportedWebRuntime &&
    isServicePortConflictError(error);
  const showCodexGuide =
    !isTrayPreview &&
    (isCodexCliGuideOpen ||
      (serviceStatus.connected &&
        !showLoading &&
        !showError &&
        !isUnsupportedWebRuntime &&
        !guideSessionDismissed &&
        !appSettings.codexCliGuideDismissed));
  return (
    <>
      {/* Always keep children mounted to prevent Header/Sidebar remounting 'reload' feel */}
      {children}

      <CodexCliOnboardingDialog
        open={showCodexGuide}
        onOpenChange={handleGuideOpenChange}
        onAcknowledge={handleGuideAcknowledge}
      />

      {!isTrayPreview && (showLoading || showError) && (
        <div className="fixed inset-0 z-50 flex flex-col items-center justify-center bg-background">
          <div className="flex w-full max-w-md flex-col items-center gap-6 rounded-xl glass-card p-10 shadow-sm animate-in fade-in zoom-in duration-500">
            {showLoading ? (
              <>
                <div className="h-14 w-14 animate-spin rounded-full border-4 border-primary border-t-transparent" />
                <div className="flex flex-col items-center gap-2">
                  <h2 className="text-2xl font-bold tracking-tight">{t("正在准备环境")}</h2>
                  <p className="px-4 text-center text-sm text-muted-foreground">
                    {t("正在同步本地配置，请稍候...")}
                  </p>
                </div>
              </>
            ) : (
              <>
                <div className="flex h-14 w-14 items-center justify-center rounded-full bg-destructive/10">
                  <AlertCircle className="h-8 w-8 text-destructive" />
                </div>
                <div className="flex flex-col items-center gap-2 text-center">
                  <h2 className="text-xl font-bold tracking-tight text-destructive">
                    {isUnsupportedWebRuntime
                      ? t("当前 Web 运行方式不受支持")
                      : t("无法同步核心服务状态")}
                  </h2>
                  {isUnsupportedWebRuntime ? (
                    <p className="px-4 text-center text-sm text-muted-foreground">
                      {t(
                        "请通过 `codexmanager-web` 打开页面，或在反向代理中同时提供 `/api/runtime` 与 `/api/rpc`。",
                      )}
                    </p>
                  ) : null}
                  <p className="max-h-32 overflow-y-auto break-all rounded-lg bg-muted/50 p-3 font-mono text-[10px] text-muted-foreground">
                    {error}
                  </p>
                </div>
                {showPortRecovery ? (
                  <div className="w-full rounded-xl border border-border/70 bg-muted/30 p-3">
                    <div className="mb-2 text-left text-sm font-medium">
                      {t("端口被占用，换一个端口重新启动")}
                    </div>
                    <div className="flex gap-2">
                      <Input
                        value={recoveryPort}
                        inputMode="numeric"
                        pattern="[0-9]*"
                        aria-label={t("新的监听端口")}
                        onChange={(event) => setRecoveryPort(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            void handleRecoverWithPort();
                          }
                        }}
                        className="h-10"
                      />
                      <Button
                        type="button"
                        onClick={() => void handleRecoverWithPort()}
                        disabled={isRecoveringPort}
                        className="h-10 shrink-0"
                      >
                        {isRecoveringPort ? t("启动中...") : t("使用新端口启动")}
                      </Button>
                    </div>
                    <p className="mt-2 text-left text-xs text-muted-foreground">
                      {t("保存后会同步更新本地服务地址，CLI 的 base_url 也需要改成同一个端口。")}
                    </p>
                  </div>
                ) : null}
                <div
                  className={`grid w-full gap-3 ${supportsLocalServiceStart ? "grid-cols-2" : "grid-cols-1"}`}
                >
                  <Button variant="outline" onClick={() => void init()} className="h-11 gap-2">
                    <RefreshCw className="h-4 w-4" /> {t("重试")}
                  </Button>
                  {supportsLocalServiceStart ? (
                    <Button onClick={handleForceStart} className="h-11 gap-2 bg-primary">
                      <Play className="h-4 w-4" /> {t("强制启动")}
                    </Button>
                  ) : null}
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </>
  );
}
