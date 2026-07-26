"use client";

import Image from "next/image";
import {
  Cable,
  House,
  Users,
  UserCog,
  Key,
  Boxes,
  Database,
  Puzzle,
  WandSparkles,
  FileText,
  FolderKanban,
  Route,
  Settings,
  UserRound,
  Globe,
  ChevronLeft,
  ChevronRight,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { buildStaticRouteUrl } from "@/lib/utils/static-routes";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/lib/store/useAppStore";
import { useI18n } from "@/lib/i18n/provider";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import {
  getAllowedTopLevelRoutes,
  getTopLevelRouteLabel,
  type TopLevelRoutePath,
} from "@/lib/app-shell/top-level-routes";
import { resolveSessionRole, useAppSession } from "@/hooks/useAppSession";
import {
  memo,
  useCallback,
  useMemo,
  useState,
  type MouseEvent,
} from "react";

const NAV_ITEM_BY_PATH = new Map<TopLevelRoutePath, { icon: LucideIcon }>([
  ["/", { icon: House }],
  ["/accounts", { icon: Users }],
  ["/account-manager", { icon: UserCog }],
  ["/aggregate-api", { icon: Database }],
  ["/apikeys", { icon: Key }],
  ["/platform-mode", { icon: Cable }],
  ["/projects", { icon: FolderKanban }],
  ["/models", { icon: Boxes }],
  ["/model-groups", { icon: Route }],
  ["/plugins", { icon: Puzzle }],
  ["/skills", { icon: WandSparkles }],
  ["/logs", { icon: FileText }],
  ["/settings", { icon: Settings }],
  ["/proxy-settings", { icon: Globe }],
  ["/author", { icon: UserRound }],
]);

type SidebarNavItem = {
  href: TopLevelRoutePath;
  icon: LucideIcon;
};

const NavItem = memo(({
  item,
  isActive,
  isSidebarOpen,
  onNavigate,
  itemName,
}: {
  item: SidebarNavItem,
  isActive: boolean,
  isSidebarOpen: boolean,
  onNavigate: (href: string, event: MouseEvent<HTMLAnchorElement>) => void,
  itemName: string,
}) => (
  <a
    href={buildStaticRouteUrl(item.href)}
    onClick={(event) => onNavigate(item.href, event)}
    aria-current={isActive ? "page" : undefined}
    aria-label={itemName}
    title={itemName}
    className={cn(
      "group/nav relative flex min-h-11 items-center gap-3.5 overflow-hidden rounded-xl px-4 py-2 text-sm transition-[background-color,color,box-shadow] duration-300 ease-out hover:bg-primary/6 hover:text-primary xl:min-h-[58px] xl:gap-5 xl:rounded-2xl xl:px-5 xl:py-3 xl:text-base [@media(max-height:800px)]:min-h-10 [@media(max-height:800px)]:gap-3 [@media(max-height:800px)]:rounded-xl [@media(max-height:800px)]:px-4 [@media(max-height:800px)]:py-1.5 [@media(max-height:800px)]:text-sm",
      !isSidebarOpen && "justify-center px-0",
      isActive
        ? "bg-primary/10 text-primary shadow-[inset_0_1px_0_rgb(255_255_255/0.5),0_14px_28px_-24px_rgb(var(--primary-rgb)/0.55)]"
        : "text-muted-foreground",
    )}
  >
    {isActive ? (
      <>
        <span className="absolute inset-y-3 left-0 w-[3px] rounded-full bg-primary" />
      </>
    ) : null}
    <item.icon className="h-[18px] w-[18px] shrink-0 xl:h-[22px] xl:w-[22px]" />
    {isSidebarOpen && (
      <span className="truncate font-medium">{itemName}</span>
    )}
  </a>
));

NavItem.displayName = "NavItem";

/**
 * 函数 `Sidebar`
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
export function Sidebar() {
  const { t } = useI18n();
  const [logoFailed, setLogoFailed] = useState(false);
  const isSidebarOpen = useAppStore((state) => state.isSidebarOpen);
  const currentShellPath = useAppStore((state) => state.currentShellPath);
  const toggleSidebar = useAppStore((state) => state.toggleSidebar);
  const openCodexCliGuide = useAppStore((state) => state.openCodexCliGuide);
  const navigateShellPath = useAppStore((state) => state.navigateShellPath);
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const brandTitle = isSidebarOpen ? t("重新打开 Codex 引导") : "CodexManager";
  const toggleTitle = isSidebarOpen ? t("收起侧边栏") : t("展开侧边栏");
  const routeAccess = useMemo(
    () => ({ role, mode: session?.mode ?? null, isDesktopRuntime }),
    [isDesktopRuntime, role, session?.mode],
  );

  const handleNavigate = useCallback(
    (href: string, event: MouseEvent<HTMLAnchorElement>) => {
      if (
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey
      ) {
        return;
      }

      if (href === currentShellPath) {
        event.preventDefault();
        return;
      }

      event.preventDefault();
      navigateShellPath(href);
    },
    [currentShellPath, navigateShellPath],
  );

  const renderedItems = useMemo(() => {
    const items: SidebarNavItem[] = getAllowedTopLevelRoutes(routeAccess).flatMap(
      (route) => {
        const item = NAV_ITEM_BY_PATH.get(route.path);
        if (!item) return [];
        return [{ href: route.path, icon: item.icon }];
      },
    );

    return (
      <div className="grid gap-1.5">
        {items.map((item) => {
          const itemName = t(getTopLevelRouteLabel(item.href, routeAccess));
          return (
            <NavItem
              key={item.href}
              item={item}
              itemName={itemName}
              isActive={item.href === currentShellPath}
              isSidebarOpen={isSidebarOpen}
              onNavigate={handleNavigate}
            />
          );
        })}
      </div>
    );
  }, [currentShellPath, handleNavigate, isSidebarOpen, routeAccess, t]);

  return (
    <div
      data-slot="app-sidebar"
      className={cn(
        "relative z-20 flex shrink-0 flex-col glass-sidebar",
        isSidebarOpen ? "w-[220px] xl:w-[280px]" : "w-[60px] xl:w-[72px]"
      )}
    >
      <div
        aria-hidden="true"
        data-slot="app-sidebar-motion-edge"
        className={cn(
          "pointer-events-none absolute inset-y-0 left-0 z-20 w-px bg-border/70 transition-transform duration-300 ease-out will-change-transform motion-reduce:transition-none",
          isSidebarOpen
            ? "translate-x-[calc(220px-1px)] xl:translate-x-[calc(280px-1px)]"
            : "translate-x-[calc(60px-1px)] xl:translate-x-[calc(72px-1px)]",
        )}
      />
      <div
        className={cn(
          "flex h-[68px] items-center border-b border-border/55 shrink-0 xl:h-[96px] [@media(max-height:800px)]:h-[68px]",
          isSidebarOpen ? "px-4 xl:px-6" : "px-2 xl:px-2.5"
        )}
      >
        <Button
          type="button"
          variant="ghost"
          onClick={openCodexCliGuide}
          title={brandTitle}
          aria-label={brandTitle}
          className={cn(
            "flex h-auto w-full items-center gap-2.5 overflow-hidden rounded-xl px-0 py-1.5 transition-colors duration-200 hover:bg-primary/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 xl:gap-3.5 xl:py-2",
            isSidebarOpen ? "justify-start text-left" : "justify-center"
          )}
        >
          <div className="flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-[10px] border border-primary/20 bg-card text-primary shadow-[0_12px_24px_-18px_rgb(var(--primary-rgb)/0.8)] xl:h-12 xl:w-12 xl:rounded-xl [@media(max-height:800px)]:h-9 [@media(max-height:800px)]:w-9 [@media(max-height:800px)]:rounded-[10px]">
            {logoFailed ? (
              <span className="text-sm font-bold">CM</span>
            ) : (
              <Image
                src="/logo.png"
                alt="CodexManager"
                width={48}
                height={48}
                className="h-full w-full object-cover"
                onError={() => setLogoFailed(true)}
              />
            )}
          </div>
          {isSidebarOpen && (
            <div className="flex flex-col overflow-hidden animate-in fade-in slide-in-from-left-1 duration-200 motion-reduce:animate-none">
              <span className="truncate text-sm font-semibold tracking-[-0.02em] text-foreground xl:text-[17px]">CodexManager</span>
              <span className="truncate text-xs text-muted-foreground xl:mt-0.5 xl:text-sm">
                {t("账号池 · 路由管理")}
              </span>
            </div>
          )}
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto py-3.5 no-scrollbar xl:py-5 [@media(max-height:800px)]:py-3">
        <nav className="px-2.5 xl:px-3.5">
          {renderedItems}
        </nav>
      </div>

      <div className="border-t border-border/55 p-2.5 shrink-0">
        <Button
          variant="ghost"
          size="icon"
          className="h-9 w-full justify-start gap-3 rounded-md border border-transparent px-3 text-muted-foreground hover:border-primary/20 hover:text-primary"
          title={toggleTitle}
          aria-label={toggleTitle}
          onClick={toggleSidebar}
        >
          {isSidebarOpen ? (
            <>
              <ChevronLeft className="h-4 w-4 shrink-0" />
              <span className="text-sm">{t("收起侧边栏")}</span>
            </>
          ) : (
            <ChevronRight className="h-4 w-4 shrink-0" />
          )}
        </Button>
      </div>
    </div>
  );
}
