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
  getAllowedTopLevelRouteSections,
  getTopLevelRouteLabel,
  type TopLevelRoutePath,
} from "@/lib/app-shell/top-level-routes";
import { resolveSessionRole, useAppSession } from "@/hooks/useAppSession";
import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
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
      "group/nav relative flex min-h-8 items-center gap-3 overflow-hidden rounded-md px-3 py-1 text-sm font-medium transition-colors hover:bg-primary/6 hover:text-foreground xl:min-h-9",
      !isSidebarOpen && "justify-center px-0",
      isActive
        ? "bg-primary/10 text-primary hover:text-primary"
        : "text-foreground/75",
    )}
  >
    {isActive ? <span className="absolute inset-y-2 left-0 w-[3px] rounded-full bg-primary" /> : null}
    <item.icon className="size-[18px] shrink-0" />
    {isSidebarOpen && (
      <span className="min-w-0 truncate">{itemName}</span>
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
  const navScrollRef = useRef<HTMLDivElement>(null);
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
    const sections = getAllowedTopLevelRouteSections(routeAccess);
    const showGroups = sections.length > 5;

    return sections.map((section, index) => {
      const items: SidebarNavItem[] = section.routes.flatMap((route) => {
        const item = NAV_ITEM_BY_PATH.get(route.path);
        return item ? [{ href: route.path, icon: item.icon }] : [];
      });
      if (items.length === 0) return null;

      return (
        <div
          key={section.id}
          className={cn(
            "grid gap-0.5",
            showGroups && index > 0 && "mt-1.5 border-t border-border/60 pt-1.5",
          )}
        >
          {showGroups && isSidebarOpen && items.length > 1 ? (
            <div className="px-3 text-[11px] font-semibold text-muted-foreground">
              {t(section.label)}
            </div>
          ) : null}
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
    });
  }, [currentShellPath, handleNavigate, isSidebarOpen, routeAccess, t]);

  useEffect(() => {
    navScrollRef.current
      ?.querySelector('[aria-current="page"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [renderedItems]);

  return (
    <div
      data-slot="app-sidebar"
      className={cn(
        "relative z-20 flex shrink-0 flex-col glass-sidebar",
        isSidebarOpen ? "w-[220px] xl:w-[248px]" : "w-[60px] xl:w-[72px]"
      )}
    >
      <div
        aria-hidden="true"
        data-slot="app-sidebar-motion-edge"
        className={cn(
          "pointer-events-none absolute inset-y-0 left-0 z-20 w-px bg-border/70 transition-transform duration-300 ease-out will-change-transform motion-reduce:transition-none",
          isSidebarOpen
            ? "translate-x-[calc(220px-1px)] xl:translate-x-[calc(248px-1px)]"
            : "translate-x-[calc(60px-1px)] xl:translate-x-[calc(72px-1px)]",
        )}
      />
      <div
        className={cn(
          "flex h-[50px] shrink-0 items-center border-b border-border/55",
          isSidebarOpen ? "px-3.5" : "px-2 xl:px-2.5"
        )}
      >
        <Button
          type="button"
          variant="ghost"
          onClick={openCodexCliGuide}
          title={brandTitle}
          aria-label={brandTitle}
          className={cn(
            "flex h-auto w-full items-center gap-2.5 overflow-hidden rounded-md px-0 py-1.5 transition-colors hover:bg-primary/5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40",
            isSidebarOpen ? "justify-start text-left" : "justify-center"
          )}
        >
          <div className="flex size-9 shrink-0 items-center justify-center overflow-hidden rounded-md border border-primary/20 bg-card text-primary">
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
              <span className="truncate text-sm font-semibold text-foreground">CodexManager</span>
              <span className="truncate text-xs text-muted-foreground">
                {t("账号池 · 路由管理")}
              </span>
            </div>
          )}
        </Button>
      </div>

      <div
        ref={navScrollRef}
        data-slot="app-sidebar-scroll"
        className="sidebar-scrollbar min-h-0 flex-1 overflow-y-auto overscroll-contain py-2"
      >
        <nav className="px-2.5" aria-label="CodexManager">
          {renderedItems}
        </nav>
      </div>

      <div
        className={cn(
          "shrink-0 border-t border-border/55 p-2.5",
          !isSidebarOpen && "flex justify-center",
        )}
      >
        <Button
          variant="ghost"
          size="icon"
          className={cn(
            "h-9 rounded-md border border-transparent text-muted-foreground hover:border-primary/20 hover:text-primary",
            isSidebarOpen
              ? "w-full justify-start gap-3 px-3"
              : "w-9 justify-center px-0",
          )}
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
