"use client";

import { useEffect } from "react";
import { usePathname } from "next/navigation";
import { Header } from "@/components/layout/header";
import { PageKeepAliveViewport } from "@/components/layout/page-keep-alive-viewport";
import { RouteTransitionOverlay } from "@/components/layout/route-transition-overlay";
import { Sidebar } from "@/components/layout/sidebar";
import { useAppStore } from "@/lib/store/useAppStore";
import { normalizeRoutePath } from "@/lib/utils/static-routes";

const TRAY_PREVIEW_PATH = "/tray-preview";
const NARROW_VIEWPORT_QUERY = "(max-width: 639px)";

export function isTrayPreviewPath(pathname: string): boolean {
  return normalizeRoutePath(pathname) === TRAY_PREVIEW_PATH;
}

export function AppFrame({ children }: { children: React.ReactNode }) {
  const pathname = usePathname();
  const isTrayPreview = isTrayPreviewPath(pathname);
  const setSidebarOpen = useAppStore((state) => state.setSidebarOpen);

  useEffect(() => {
    document.documentElement.classList.toggle("tray-preview-mode", isTrayPreview);
    document.body.classList.remove("tray-preview-mode");
    return () => {
      document.documentElement.classList.remove("tray-preview-mode");
      document.body.classList.remove("tray-preview-mode");
    };
  }, [isTrayPreview]);

  useEffect(() => {
    const narrowViewport = window.matchMedia(NARROW_VIEWPORT_QUERY);
    const collapseSidebar = () => {
      if (narrowViewport.matches) {
        setSidebarOpen(false);
      }
    };

    collapseSidebar();
    narrowViewport.addEventListener("change", collapseSidebar);
    return () => {
      narrowViewport.removeEventListener("change", collapseSidebar);
    };
  }, [setSidebarOpen]);

  if (isTrayPreview) {
    return <main className="h-screen overflow-hidden bg-transparent">{children}</main>;
  }

  return (
    <div
      className="console-shell flex h-screen overflow-hidden"
      data-command-center="true"
    >
      <Sidebar />
      <div
        data-slot="app-main-column"
        className="flex min-w-0 flex-1 flex-col overflow-hidden"
      >
        <div
          data-slot="app-main-surface"
          className="flex h-full w-full flex-col"
        >
          <Header />
          <main className="relative min-w-0 flex-1 overflow-y-auto px-4 pb-5 pt-2 no-scrollbar lg:px-6 lg:pb-6 lg:pt-3">
            <RouteTransitionOverlay />
            <PageKeepAliveViewport initialChildren={children} />
          </main>
        </div>
      </div>
    </div>
  );
}
