"use client";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { ThemeProvider } from "next-themes";
import { useState } from "react";
import { I18nProvider } from "@/lib/i18n/provider";

/**
 * 函数 `Providers`
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
export function Providers({ children }: { children: React.ReactNode }) {
  const [queryClient] = useState(() => new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 60_000,
        // Keep inactive route/query snapshots bounded during long desktop sessions.
        gcTime: 600_000,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
        refetchOnMount: false,
      },
    },
  }));

  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider 
        attribute="data-theme" 
        defaultTheme="tech" 
        enableSystem={false}
        disableTransitionOnChange
        themes={["tech", "dark", "dark-one", "business", "mint", "sunset", "grape", "ocean", "forest", "rose", "slate", "aurora"]}
      >
        <I18nProvider>
          <TooltipProvider>
            {children}
            <Toaster 
              position="top-right" 
              richColors 
              expand={false} 
              visibleToasts={3}
              closeButton
              duration={4000}
              theme="system"
              toastOptions={{
                closeButton: true,
              }}
            />
          </TooltipProvider>
        </I18nProvider>
      </ThemeProvider>
    </QueryClientProvider>
  );
}
