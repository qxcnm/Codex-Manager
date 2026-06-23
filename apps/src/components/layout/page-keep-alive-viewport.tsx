"use client";

import {
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useState,
  type ComponentType,
  type LazyExoticComponent,
  type ReactNode,
} from "react";
import { Loader2 } from "lucide-react";
import { usePathname } from "next/navigation";
import {
  type TopLevelRoutePath,
  type TopLevelRouteAccessContext,
  getAllowedTopLevelRoutes,
  getFirstAllowedTopLevelRoutePath,
  getTopLevelRouteLabel,
  isTopLevelRouteAllowedForRole,
  toTopLevelRoutePath,
} from "@/lib/app-shell/top-level-routes";
import { resolveSessionRole, useAppSession } from "@/hooks/useAppSession";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";
import { Card, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";

const ROOT_ROUTE_PATH = "/";

const LAZY_PAGE_COMPONENTS: Record<
  Exclude<TopLevelRoutePath, typeof ROOT_ROUTE_PATH>,
  LazyExoticComponent<ComponentType>
> = {
  "/accounts": lazy(() => import("@/app/accounts/page")),
  "/account-manager": lazy(() => import("@/app/account-manager/page")),
  "/aggregate-api": lazy(() => import("@/app/aggregate-api/page")),
  "/apikeys": lazy(() => import("@/app/apikeys/page")),
  "/platform-mode": lazy(() => import("@/app/platform-mode/page")),
  "/models": lazy(() => import("@/app/models/page")),
  "/model-groups": lazy(() => import("@/app/model-groups/page")),
  "/plugins": lazy(() => import("@/app/plugins/page")),
  "/logs": lazy(() => import("@/app/logs/page")),
  "/settings": lazy(() => import("@/app/settings/page")),
  "/proxy-settings": lazy(() => import("@/app/proxy-settings/page")),
  "/author": lazy(() => import("@/app/author/page")),
};

const ROOT_PAGE_COMPONENT = lazy(() => import("@/app/page"));

function PagePanelFallback({ title }: { title: string }) {
  const { t } = useI18n();
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);

  return (
    <div
      className={cn(
        "fixed inset-y-0 right-0 z-40 overflow-hidden bg-background/70",
        isSidebarOpen ? "left-56" : "left-16",
      )}
    >
      <div className="relative flex h-full w-full items-start justify-center px-8 pt-[31vh]">
        <Card className="w-full max-w-xl border-border/70 bg-card/95 shadow-sm">
          <CardContent className="flex flex-col items-center gap-5 px-8 py-8 text-center">
            <div className="flex size-14 items-center justify-center rounded-full border bg-muted text-primary">
              <Loader2 className="size-7 animate-spin" />
            </div>
            <div className="flex flex-col gap-2">
              <p className="text-xl font-semibold tracking-tight text-foreground">{t(title)}</p>
              <p className="text-sm text-muted-foreground">
                {t("正在恢复页面内容，请稍候...")}
              </p>
            </div>
            <div className="flex w-full max-w-sm flex-col gap-2">
              <Skeleton className="h-2 w-full rounded-full" />
              <Skeleton className="mx-auto h-2 w-2/3 rounded-full" />
            </div>
            <p className="text-xs text-muted-foreground">
              {t("页面缓存已命中，正在恢复视图与数据状态")}
            </p>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function LazyPagePanel({
  path,
  access,
}: {
  path: TopLevelRoutePath;
  access: TopLevelRouteAccessContext;
}) {
  const LazyPage = path === ROOT_ROUTE_PATH ? ROOT_PAGE_COMPONENT : LAZY_PAGE_COMPONENTS[path];

  return (
    <Suspense fallback={<PagePanelFallback title={getTopLevelRouteLabel(path, access)} />}>
      <LazyPage />
    </Suspense>
  );
}

export function PageKeepAliveViewport({
  initialChildren,
}: {
  initialChildren: ReactNode;
}) {
  const { t } = useI18n();
  const pathname = usePathname();
  const [normalizedInitialPath] = useState<TopLevelRoutePath>(() =>
    toTopLevelRoutePath(pathname),
  );
  const currentShellPath = useAppStore((state) => state.currentShellPath);
  const openShellTabs = useAppStore((state) => state.openShellTabs);
  const syncShellPathFromLocation = useAppStore(
    (state) => state.syncShellPathFromLocation,
  );
  const pruneShellTabs = useAppStore((state) => state.pruneShellTabs);
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const routeAccess = useMemo(
    () => ({ role, mode: session?.mode ?? null }),
    [role, session?.mode],
  );

  useEffect(() => {
    syncShellPathFromLocation(normalizedInitialPath);
  }, [normalizedInitialPath, syncShellPathFromLocation]);

  useEffect(() => {
    const handlePopState = () => {
      syncShellPathFromLocation(window.location.pathname);
    };

    window.addEventListener("popstate", handlePopState);
    return () => {
      window.removeEventListener("popstate", handlePopState);
    };
  }, [syncShellPathFromLocation]);

  useEffect(() => {
    document.title = `${t(getTopLevelRouteLabel(currentShellPath, routeAccess))} - CodexManager`;
  }, [currentShellPath, routeAccess, t]);

  useEffect(() => {
    if (isSessionLoading) return;
    const allowedPaths = getAllowedTopLevelRoutes(routeAccess).map((route) => route.path);
    pruneShellTabs(allowedPaths, getFirstAllowedTopLevelRoutePath(routeAccess));
  }, [isSessionLoading, pruneShellTabs, routeAccess]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="relative min-h-0 flex-1">
        {openShellTabs.map((path) => {
          if (!isTopLevelRouteAllowedForRole(path, routeAccess)) {
            return null;
          }
          const isActive = path === currentShellPath;
          const isInitialPanel = path === normalizedInitialPath;

          return (
            <section
              key={path}
              aria-hidden={!isActive}
              data-shell-path={path}
              className={cn(
                "relative min-h-[calc(100vh-11rem)]",
                isActive ? "block" : "hidden",
              )}
            >
              {isInitialPanel ? initialChildren : <LazyPagePanel path={path} access={routeAccess} />}
            </section>
          );
        })}
      </div>
    </div>
  );
}
