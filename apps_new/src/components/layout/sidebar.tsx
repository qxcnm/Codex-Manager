"use client";

import { usePathname } from "next/navigation";
import { 
  LayoutDashboard, 
  Users, 
  Key, 
  FileText, 
  Settings, 
  ChevronLeft, 
  ChevronRight
} from "lucide-react";
import { cn } from "@/lib/utils";
import { normalizeAppPath, toStaticRouteHref } from "@/lib/utils/navigation";
import { Button } from "@/components/ui/button";
import { useAppStore } from "@/lib/store/useAppStore";
import { memo, useMemo } from "react";

const NAV_ITEMS = [
  { name: "仪表盘", href: "/", icon: LayoutDashboard },
  { name: "账号管理", href: "/accounts", icon: Users },
  { name: "平台密钥", href: "/apikeys", icon: Key },
  { name: "请求日志", href: "/logs", icon: FileText },
  { name: "设置", href: "/settings", icon: Settings },
];

const NavItem = memo(({ item, isActive, isSidebarOpen }: { item: typeof NAV_ITEMS[0], isActive: boolean, isSidebarOpen: boolean }) => (
  <a
    href={toStaticRouteHref(item.href)}
    className={cn(
      "flex items-center gap-3 rounded-lg px-3 py-2 transition-all duration-200 hover:bg-accent hover:text-accent-foreground",
      isActive ? "bg-accent text-accent-foreground" : "text-muted-foreground"
    )}
  >
    <item.icon className="h-4 w-4 shrink-0" />
    {isSidebarOpen && <span className="text-sm truncate">{item.name}</span>}
  </a>
));

NavItem.displayName = "NavItem";

export function Sidebar() {
  const pathname = normalizeAppPath(usePathname());
  const { isSidebarOpen, toggleSidebar } = useAppStore();

  const renderedItems = useMemo(() => 
    NAV_ITEMS.map((item) => (
      <NavItem 
        key={item.href} 
        item={item} 
        isActive={pathname === item.href} 
        isSidebarOpen={isSidebarOpen} 
      />
    )),
    [pathname, isSidebarOpen]
  );

  return (
    <div
      className={cn(
        "relative flex flex-col glass-sidebar transition-all duration-300 ease-in-out",
        isSidebarOpen ? "w-64" : "w-16"
      )}
    >
      <div className="flex h-16 items-center px-4 border-b shrink-0">
        <div className="flex items-center gap-2 overflow-hidden">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <span className="text-sm font-bold">CM</span>
          </div>
          {isSidebarOpen && (
            <div className="flex flex-col overflow-hidden animate-in fade-in duration-300">
              <span className="text-sm font-bold truncate">CodexManager</span>
              <span className="text-xs text-muted-foreground truncate opacity-70">账号池 · 用量管理</span>
            </div>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-y-auto py-4">
        <nav className="grid gap-1 px-2">
          {renderedItems}
        </nav>
      </div>

      <div className="border-t p-2 shrink-0">
        <Button
          variant="ghost"
          size="icon"
          className="w-full justify-start gap-3 px-3 h-10"
          onClick={toggleSidebar}
        >
          {isSidebarOpen ? (
            <>
              <ChevronLeft className="h-4 w-4 shrink-0" />
              <span className="text-sm">收起侧边栏</span>
            </>
          ) : (
            <ChevronRight className="h-4 w-4 shrink-0" />
          )}
        </Button>
      </div>
    </div>
  );
}
