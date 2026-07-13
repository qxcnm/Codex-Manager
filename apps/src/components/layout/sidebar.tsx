"use client";

import {
  LayoutDashboard,
  Users,
  UserCog,
  Key,
  Boxes,
  Database,
  Puzzle,
  FileText,
  Route,
  Settings,
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
  getAllowedTopLevelRouteSections,
  getTopLevelRouteLabel,
  type TopLevelRoutePath,
} from "@/lib/app-shell/top-level-routes";
import { resolveSessionRole, useAppSession } from "@/hooks/useAppSession";
import {
  memo,
  useCallback,
  useMemo,
  type MouseEvent,
} from "react";

const NAV_ITEM_BY_PATH = new Map<TopLevelRoutePath, { icon: LucideIcon }>([
  ["/", { icon: LayoutDashboard }],
  ["/accounts", { icon: Users }],
  ["/account-manager", { icon: UserCog }],
  ["/aggregate-api", { icon: Database }],
  ["/apikeys", { icon: Key }],
  ["/models", { icon: Boxes }],
  ["/model-groups", { icon: Route }],
  ["/plugins", { icon: Puzzle }],
  ["/logs", { icon: FileText }],
  ["/settings", { icon: Settings }],
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
      "flex items-center gap-3 rounded-lg px-3 py-2 transition-all duration-200 hover:bg-accent hover:text-accent-foreground",
      !isSidebarOpen && "justify-center px-0",
      isActive ? "bg-accent text-accent-foreground" : "text-muted-foreground"
    )}
  >
    <item.icon className="h-4 w-4 shrink-0" />
    {isSidebarOpen && <span className="text-sm truncate">{itemName}</span>}
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
  const {
    isSidebarOpen,
    toggleSidebar,
    openCodexCliGuide,
    currentShellPath,
    navigateShellPath,
  } = useAppStore();
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const brandTitle = isSidebarOpen ? t("重新打开 Codex CLI 引导") : "CodexManager";
  const toggleTitle = isSidebarOpen ? t("收起侧边栏") : t("展开侧边栏");
  const routeAccess = useMemo(
    () => ({ role, mode: session?.mode ?? null }),
    [role, session?.mode],
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
    const sections = getAllowedTopLevelRouteSections(routeAccess);
    return sections.map((section, sectionIndex) => (
      <div
        key={section.id}
        className={cn(
          "space-y-1",
          sectionIndex > 0 && "mt-3 border-t border-border/50 pt-3",
        )}
      >
        {isSidebarOpen ? (
          <div className="px-3 pb-1 text-[11px] font-semibold uppercase text-muted-foreground/70">
            {t(section.label)}
          </div>
        ) : null}
        <div className="grid gap-1">
          {section.routes.map((route) => {
            const item = NAV_ITEM_BY_PATH.get(route.path);
            if (!item) return null;
            const navItem = { href: route.path, icon: item.icon };
            const itemName = t(getTopLevelRouteLabel(route.path, routeAccess));
            return (
              <NavItem
                key={route.path}
                item={navItem}
                itemName={itemName}
                isActive={route.path === currentShellPath}
                isSidebarOpen={isSidebarOpen}
                onNavigate={handleNavigate}
              />
            );
          })}
        </div>
      </div>
    ));
  }, [currentShellPath, handleNavigate, isSidebarOpen, routeAccess, t]);

  return (
    <div
      className={cn(
        "relative z-20 flex shrink-0 flex-col glass-sidebar transition-[width] duration-300 ease-in-out",
        isSidebarOpen ? "w-56" : "w-16"
      )}
    >
      <div
        className={cn(
          "flex h-16 items-center border-b shrink-0",
          isSidebarOpen ? "px-4" : "px-2"
        )}
      >
        <Button
          type="button"
          variant="ghost"
          onClick={openCodexCliGuide}
          title={brandTitle}
          aria-label={brandTitle}
          className={cn(
            "flex h-auto w-full items-center gap-2 overflow-hidden rounded-xl px-2 py-1.5 transition-colors duration-200 hover:bg-accent/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/60",
            isSidebarOpen ? "text-left" : "justify-center"
          )}
        >
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <span className="text-sm font-bold">CM</span>
          </div>
          {isSidebarOpen && (
            <div className="flex flex-col overflow-hidden animate-in fade-in duration-300">
              <span className="text-sm font-bold truncate">CodexManager</span>
              <span className="text-xs text-muted-foreground truncate opacity-70">{t("账号池 · 用量管理")}</span>
            </div>
          )}
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto py-4">
        <nav className="px-2">
          {renderedItems}
        </nav>
      </div>

      <div className="border-t p-2 shrink-0">
        <Button
          variant="ghost"
          size="icon"
          className="w-full justify-start gap-3 px-3 h-10"
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
