"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useAppSession } from "@/hooks/useAppSession";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { serviceHealthClient } from "@/lib/api/service-health-client";
import { useAppStore } from "@/lib/store/useAppStore";
import type { ServiceHealthReportInput } from "@/types/service-health";

export function useServiceHealth() {
  const addr = useAppStore((state) => state.serviceStatus.addr);
  const active = useDesktopPageActive("/service-health");
  const { data: session, isServiceReady } = useAppSession({ enabled: active });
  const enabled = active && isServiceReady && Boolean(session);
  const queryClient = useQueryClient();
  // Own reports must never carry over when switching users or services.
  const queryKey = ["service-health", addr, session?.currentUser?.id, session?.role];
  const summary = useQuery({
    queryKey,
    queryFn: () => serviceHealthClient.summary(addr),
    enabled,
    staleTime: 10_000,
    refetchInterval: enabled ? 15_000 : false,
    retry: 1,
  });
  const invalidate = () => queryClient.invalidateQueries({ queryKey });
  const report = useMutation({
    mutationFn: (input: ServiceHealthReportInput) => serviceHealthClient.report(input, addr),
    onSuccess: invalidate,
  });
  const resolve = useMutation({
    mutationFn: (id: string) => serviceHealthClient.resolve(id, addr),
    onSuccess: invalidate,
  });
  return { summary, report, resolve, enabled, isServiceReady };
}
