"use client";

import {
  Cable,
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
  UserRound,
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
  useState,
  type MouseEvent,
} from "react";

const NAV_ITEM_BY_PATH = new Map<TopLevelRoutePath, { icon: LucideIcon }>([
  ["/", { icon: LayoutDashboard }],
  ["/accounts", { icon: Users }],
  ["/account-manager", { icon: UserCog }],
  ["/aggregate-api", { icon: Database }],
  ["/apikeys", { icon: Key }],
  ["/platform-mode", { icon: Cable }],
  ["/models", { icon: Boxes }],
  ["/model-groups", { icon: Route }],
  ["/plugins", { icon: Puzzle }],
  ["/logs", { icon: FileText }],
  ["/settings", { icon: Settings }],
  ["/author", { icon: UserRound }],
]);

type SidebarNavItem = {
  href: TopLevelRoutePath;
  icon: LucideIcon;
};

type RenderedSidebarSection = {
  id: string;
  label: string;
  items: SidebarNavItem[];
};

const NavItem = memo(({
  item,
  isActive,
  isSidebarOpen,
  onNavigate,
  itemName,
  index,
}: {
  item: SidebarNavItem,
  isActive: boolean,
  isSidebarOpen: boolean,
  onNavigate: (href: string, event: MouseEvent<HTMLAnchorElement>) => void,
  itemName: string,
  index: number,
}) => (
  <a
    href={buildStaticRouteUrl(item.href)}
    onClick={(event) => onNavigate(item.href, event)}
    aria-current={isActive ? "page" : undefined}
    aria-label={itemName}
    title={itemName}
    className={cn(
      "group/nav relative flex min-h-10 items-center gap-3 overflow-hidden rounded-md border border-transparent px-3 py-2 text-[13px] transition-colors duration-200 hover:border-primary/20 hover:bg-primary/5 hover:text-primary",
      !isSidebarOpen && "justify-center px-0",
      isActive
        ? "border-primary/25 bg-primary/10 text-primary shadow-[inset_3px_0_0_rgb(var(--primary-rgb)/0.8),0_10px_22px_-20px_rgb(var(--primary-rgb)/0.34)]"
        : "text-muted-foreground",
    )}
  >
    {isActive ? (
      <>
        <span className="absolute inset-y-1 left-0 w-0.5 rounded-full bg-primary" />
        <span className="absolute inset-x-3 top-0 h-px bg-gradient-to-r from-primary/35 via-primary/10 to-transparent" />
      </>
    ) : null}
    <div className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border/70 bg-background/35">
      <item.icon className="h-3.5 w-3.5" />
    </div>
    {isSidebarOpen && (
      <>
        <span className="truncate font-medium">{itemName}</span>
      </>
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
    const sections: RenderedSidebarSection[] = getAllowedTopLevelRouteSections(
      routeAccess,
    ).map((section) => ({
      id: section.id,
      label: section.label,
      items: section.routes.flatMap((route) => {
        const item = NAV_ITEM_BY_PATH.get(route.path);
        if (!item) return [];
        return [{ href: route.path, icon: item.icon }];
      }),
    }));
    const itemIndexes = new Map(
      sections
        .flatMap((section) => section.items)
        .map((item, index) => [item.href, index] as const),
    );

    return sections.map((section, sectionIndex) => (
      <div
        key={section.id}
        className={cn(
          "space-y-1",
          sectionIndex > 0 && "mt-4 border-t border-border/70 pt-4",
        )}
      >
        {isSidebarOpen ? (
          <div className="animate-in px-3 pb-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground/70 fade-in slide-in-from-left-1 duration-200 motion-reduce:animate-none">
            {t(section.label)}
          </div>
        ) : null}
        <div className="grid gap-1">
          {section.items.map((item) => {
            const itemName = t(getTopLevelRouteLabel(item.href, routeAccess));
            return (
              <NavItem
                key={item.href}
                item={item}
                itemName={itemName}
                isActive={item.href === currentShellPath}
                isSidebarOpen={isSidebarOpen}
                onNavigate={handleNavigate}
                index={itemIndexes.get(item.href) ?? 0}
              />
            );
          })}
        </div>
      </div>
    ));
  }, [currentShellPath, handleNavigate, isSidebarOpen, routeAccess, t]);

  return (
    <div
      data-slot="app-sidebar"
      className={cn(
        "relative z-20 flex shrink-0 flex-col glass-sidebar",
        isSidebarOpen ? "w-60" : "w-16"
      )}
    >
      <div
        aria-hidden="true"
        data-slot="app-sidebar-motion-edge"
        className={cn(
          "pointer-events-none absolute inset-y-0 left-0 z-20 w-px bg-gradient-to-b from-transparent via-primary/55 to-transparent transition-transform duration-200 ease-out will-change-transform motion-reduce:transition-none",
          isSidebarOpen
            ? "translate-x-[calc(15rem-1px)]"
            : "translate-x-[calc(4rem-1px)]",
        )}
      />
      <div
        className={cn(
          "flex h-[76px] items-center border-b border-border/70 shrink-0",
          isSidebarOpen ? "px-3" : "px-2"
        )}
      >
        <Button
          type="button"
          variant="ghost"
          onClick={openCodexCliGuide}
          title={brandTitle}
          aria-label={brandTitle}
          className={cn(
            "flex h-auto w-full items-center gap-2 overflow-hidden rounded-md border border-border/70 bg-background/65 px-2 py-2 transition-colors duration-200 hover:border-primary/25 hover:bg-accent/35 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40",
            isSidebarOpen ? "text-left" : "justify-center"
          )}
        >
          <div className="flex h-10 w-10 shrink-0 items-center justify-center overflow-hidden rounded-md border border-primary/20 bg-white text-primary shadow-sm">
            {logoFailed ? (
              <span className="text-sm font-bold">CM</span>
            ) : (
              <img
                src="/logo.png"
                alt="CodexManager"
                className="h-full w-full object-cover"
                onError={() => setLogoFailed(true)}
              />
            )}
          </div>
          {isSidebarOpen && (
            <div className="flex flex-col overflow-hidden animate-in fade-in slide-in-from-left-1 duration-200 motion-reduce:animate-none">
              <span className="truncate text-sm font-semibold text-foreground">CodexManager</span>
              <span className="truncate font-mono text-[10px] uppercase text-primary/70">
                Admin Console
              </span>
            </div>
          )}
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto py-4 no-scrollbar">
        <nav className="px-2">
          {renderedItems}
        </nav>
      </div>

      <div className="border-t border-border/70 p-2 shrink-0">
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
