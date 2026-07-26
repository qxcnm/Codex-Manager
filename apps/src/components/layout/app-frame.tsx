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
          data-slot="app-main-scale"
          className="flex h-full w-full origin-top-left flex-col xl:h-[111.111111%] xl:w-[111.111111%] xl:scale-90"
        >
          <Header />
          <main className="relative min-w-0 flex-1 overflow-y-auto px-4 pb-7 pt-3 no-scrollbar lg:px-5 xl:pb-10 xl:pl-[26px] xl:pr-[45px] xl:pt-[3px]">
            <RouteTransitionOverlay />
            <PageKeepAliveViewport initialChildren={children} />
          </main>
        </div>
      </div>
    </div>
  );
}
