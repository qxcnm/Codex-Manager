"use client";

import { useCallback, useEffect, useState, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { AlertCircle, Play, RefreshCw } from "lucide-react";
import { useTheme } from "next-themes";
import { toast } from "sonner";
import { useAppStore } from "@/lib/store/useAppStore";
import { accountClient } from "@/lib/api/account-client";
import { serviceClient } from "@/lib/api/service-client";
import {
  buildStartupSnapshotQueryKey,
  STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
  STARTUP_SNAPSHOT_STALE_TIME,
} from "@/lib/api/startup-snapshot";
import { appClient } from "@/lib/api/app-client";
import { isTauriRuntime } from "@/lib/api/transport";
import { Button } from "@/components/ui/button";
import {
  formatServiceError,
  isExpectedInitializeResult,
  normalizeServiceAddr,
} from "@/lib/utils/service";

const DEFAULT_SERVICE_ADDR = "localhost:48760";
const PRIMARY_PAGE_WARMUP_STALE_TIME = 30_000;
const PRIMARY_PAGE_WARMUP_PAGE_SIZE = 20;
const PRIMARY_PAGE_ROUTES = ["/", "/accounts", "/apikeys", "/logs", "/settings"] as const;
const sleep = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

function routeChunkPath(route: (typeof PRIMARY_PAGE_ROUTES)[number]) {
  if (route === "/") {
    return "/_next/static/chunks/app/page.js";
  }
  return `/_next/static/chunks/app${route}/page.js`;
}

function warmRouteChunkScript(route: (typeof PRIMARY_PAGE_ROUTES)[number]) {
  return new Promise<void>((resolve) => {
    const src = routeChunkPath(route);
    const existing = document.querySelector<HTMLScriptElement>(
      `script[data-codexmanager-route-chunk="${src}"]`
    );
    if (existing) {
      resolve();
      return;
    }

    const script = document.createElement("script");
    script.src = src;
    script.async = true;
    script.dataset.codexmanagerRouteChunk = src;
    script.onload = () => resolve();
    script.onerror = () => resolve();
    document.head.append(script);
  });
}

export function AppBootstrap({ children }: { children: React.ReactNode }) {
  const { setServiceStatus, setAppSettings, serviceStatus } = useAppStore();
  const { setTheme } = useTheme();
  const queryClient = useQueryClient();
  const router = useRouter();
  const [isInitializing, setIsInitializing] = useState(true);
  const hasInitializedOnce = useRef(false);
  const hasWarmedDevRoutes = useRef(false);
  const [error, setError] = useState<string | null>(null);

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
        const initializeResult = await serviceClient.initialize();
        if (!isExpectedInitializeResult(initializeResult)) {
          throw new Error("Port is in use or unexpected service responded (missing server_name)");
        }
        return initializeResult;
      } catch (serviceError: unknown) {
        lastError = serviceError;
        if (attempt < retries) {
          await sleep(300);
        }
      }
    }

    throw lastError || new Error(`服务初始化失败: ${addr}`);
  }, []);

  const startAndInitializeService = useCallback(
    async (addr: string) => {
      await serviceClient.start(addr);
      return initializeService(addr, 2);
    },
    [initializeService]
  );

  const prefetchStartupSnapshot = useCallback(
    async (addr: string) => {
      await queryClient.prefetchQuery({
        queryKey: buildStartupSnapshotQueryKey(
          addr,
          STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT
        ),
        queryFn: () =>
          serviceClient.getStartupSnapshot({
            requestLogLimit: STARTUP_SNAPSHOT_REQUEST_LOG_LIMIT,
          }),
        staleTime: STARTUP_SNAPSHOT_STALE_TIME,
      });
    },
    [queryClient]
  );

  const warmupPrimaryPages = useCallback(
    async (addr: string) => {
      if (!isTauriRuntime()) {
        return;
      }

      for (const route of PRIMARY_PAGE_ROUTES) {
        router.prefetch(route);
      }

      const warmupTasks = [
        queryClient.prefetchQuery({
          queryKey: ["accounts", "list"],
          queryFn: () => accountClient.list(),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["usage", "list"],
          queryFn: () => accountClient.listUsage(),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["gateway", "manual-account", addr || null],
          queryFn: () => serviceClient.getManualPreferredAccountId(),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["apikeys"],
          queryFn: () => accountClient.listApiKeys(),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["apikey-models"],
          queryFn: () => accountClient.listModels(false),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["apikey-usage-stats"],
          queryFn: async () => {
            const stats = await accountClient.listApiKeyUsageStats();
            return stats.reduce<Record<string, number>>((result, item) => {
              const keyId = String(item.keyId || "").trim();
              if (!keyId) return result;
              result[keyId] = Math.max(0, item.totalTokens || 0);
              return result;
            }, {});
          },
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["accounts", "lookup"],
          queryFn: () => accountClient.list(),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["logs", "list", "", "all", 1, PRIMARY_PAGE_WARMUP_PAGE_SIZE],
          queryFn: () =>
            serviceClient.listRequestLogs({
              query: "",
              statusFilter: "all",
              page: 1,
              pageSize: PRIMARY_PAGE_WARMUP_PAGE_SIZE,
            }),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["logs", "summary", "", "all"],
          queryFn: () =>
            serviceClient.getRequestLogSummary({
              query: "",
              statusFilter: "all",
            }),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
        queryClient.prefetchQuery({
          queryKey: ["app-settings-snapshot"],
          queryFn: () => appClient.getSettings(),
          staleTime: PRIMARY_PAGE_WARMUP_STALE_TIME,
        }),
      ];

      await Promise.allSettled(warmupTasks);
    },
    [queryClient, router]
  );

  const warmupDevRouteTransitions = useCallback(() => {
    if (!isTauriRuntime()) {
      return () => {};
    }
    if (process.env.NODE_ENV !== "development") {
      return () => {};
    }
    if (hasWarmedDevRoutes.current || typeof window === "undefined") {
      return () => {};
    }
    hasWarmedDevRoutes.current = true;

    const runtime = globalThis as typeof globalThis & {
      requestIdleCallback?: (
        callback: IdleRequestCallback,
        options?: IdleRequestOptions,
      ) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    const currentPath = window.location.pathname;
    const routes = PRIMARY_PAGE_ROUTES.filter((route) => route !== currentPath);
    const controllers: AbortController[] = [];

    const runWarmup = () => {
      for (const route of routes) {
        router.prefetch(route);
      }

      void Promise.allSettled(
        routes.flatMap((route) => {
          const controller = new AbortController();
          controllers.push(controller);
          return [
            fetch(route, {
              method: "GET",
              credentials: "same-origin",
              cache: "default",
              signal: controller.signal,
              headers: {
                "x-codexmanager-route-warmup": "1",
              },
            }),
            warmRouteChunkScript(route),
          ];
        })
      );
    };

    if (runtime.requestIdleCallback) {
      const idleId = runtime.requestIdleCallback(() => runWarmup(), {
        timeout: 800,
      });
      return () => {
        runtime.cancelIdleCallback?.(idleId);
        for (const controller of controllers) {
          controller.abort();
        }
      };
    }

    const timer = window.setTimeout(runWarmup, 120);
    return () => {
      window.clearTimeout(timer);
      for (const controller of controllers) {
        controller.abort();
      }
    };
  }, [router]);

  const init = useCallback(async () => {
    const desktopRuntime = isTauriRuntime();

    // Only show full screen loading if we haven't initialized once
    if (!hasInitializedOnce.current) {
      setIsInitializing(true);
    }
    setError(null);

    try {
      const settings = await appClient.getSettings();
      const addr = normalizeServiceAddr(settings.serviceAddr || DEFAULT_SERVICE_ADDR);
      
      const currentAppliedTheme = typeof document !== 'undefined' ? document.documentElement.getAttribute('data-theme') : null;
      if (settings.theme && settings.theme !== currentAppliedTheme) {
        setTheme(settings.theme);
      }
      
      setAppSettings(settings);
      
      // CRITICAL: Do not reset status to connected: false if we are already connected
      // This prevents the Header badge from flashing
      if (!serviceStatus.connected || serviceStatus.addr !== addr) {
        setServiceStatus({ addr, connected: false, version: "" });
      }

      try {
        let initializeResult;
        try {
          initializeResult = await initializeService(addr, 1);
        } catch {
          if (!desktopRuntime) {
            throw new Error(`服务未启动或无法访问: ${addr}`);
          }
          initializeResult = await startAndInitializeService(addr);
        }
        setServiceStatus({
          addr,
          connected: true,
          version: initializeResult.version,
        });
        await prefetchStartupSnapshot(addr);
        await warmupPrimaryPages(addr);
        setIsInitializing(false);
        hasInitializedOnce.current = true;
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
    // We remove serviceStatus from dependencies to avoid infinite loop
    // and use hasInitializedOnce ref for stability
  }, [
    initializeService,
    prefetchStartupSnapshot,
    warmupPrimaryPages,
    setAppSettings,
    setServiceStatus,
    setTheme,
    startAndInitializeService,
  ]);

  const handleForceStart = async () => {
    setIsInitializing(true);
    setError(null);
    try {
      const addr = normalizeServiceAddr(serviceStatus.addr || DEFAULT_SERVICE_ADDR);
      const settings = await appClient.setSettings({ serviceAddr: addr });
      
      const currentAppliedTheme = typeof document !== 'undefined' ? document.documentElement.getAttribute('data-theme') : null;
      if (settings.theme && settings.theme !== currentAppliedTheme) {
        setTheme(settings.theme);
      }
      
      setAppSettings(settings);
      const initializeResult = await startAndInitializeService(addr);
      setServiceStatus({
        addr,
        connected: true,
        version: initializeResult.version,
      });
      await prefetchStartupSnapshot(addr);
      await warmupPrimaryPages(addr);
      applyLowTransparency(settings.lowTransparency);
      setIsInitializing(false);
      toast.success("服务已启动");
    } catch (startError: unknown) {
      setServiceStatus({ connected: false, version: "" });
      setError(formatServiceError(startError));
      setIsInitializing(false);
    }
  };

  useEffect(() => {
    void init();
  }, [init]);

  useEffect(() => warmupDevRouteTransitions(), [warmupDevRouteTransitions]);

  const showLoading = isInitializing && !hasInitializedOnce.current;
  const showError = !!error && !hasInitializedOnce.current;

  return (
    <>
      {/* Always keep children mounted to prevent Header/Sidebar remounting 'reload' feel */}
      {children}

      {(showLoading || showError) && (
        <div className="fixed inset-0 z-50 flex flex-col items-center justify-center bg-background">
          <div className="flex w-full max-w-md flex-col items-center gap-6 rounded-3xl glass-card p-10 shadow-2xl animate-in fade-in zoom-in duration-500">
            {showLoading ? (
              <>
                <div className="h-14 w-14 animate-spin rounded-full border-4 border-primary border-t-transparent" />
                <div className="flex flex-col items-center gap-2">
                  <h2 className="text-2xl font-bold tracking-tight">正在准备环境</h2>
                  <p className="px-4 text-center text-sm text-muted-foreground">
                    正在同步本地配置，请稍候...
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
                    无法同步核心服务状态
                  </h2>
                  <p className="max-h-32 overflow-y-auto break-all rounded-lg bg-muted/50 p-3 font-mono text-[10px] text-muted-foreground">
                    {error}
                  </p>
                </div>
                <div className="grid w-full grid-cols-2 gap-3">
                  <Button variant="outline" onClick={() => void init()} className="h-11 gap-2">
                    <RefreshCw className="h-4 w-4" /> 重试
                  </Button>
                  <Button onClick={handleForceStart} className="h-11 gap-2 bg-primary">
                    <Play className="h-4 w-4" /> 强制启动
                  </Button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
    </>
  );
}
