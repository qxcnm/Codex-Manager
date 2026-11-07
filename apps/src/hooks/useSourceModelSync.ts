"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { modelDiscoveryClient } from "@/lib/api/model-discovery-client";
import {
  buildAggregateApiListQueryKey,
  buildApiKeyListQueryKey,
  buildManagedModelListQueryKey,
  buildManagedModelSelectorQueryKey,
  normalizeQueryServiceAddress,
} from "@/lib/api/account-query-keys";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { sourceModelSyncMessage } from "@/lib/source-model-sync-messages";
import type {
  ModelDiscoveryList,
  ModelDiscoverySourceKind,
  ModelDiscoveryTask,
} from "@/types/model-discovery";

interface ModelSyncSource {
  id: string;
  name: string;
}

export function useSourceModelSync({
  sourceKind,
  serviceAddr,
  enabled,
}: {
  sourceKind: ModelDiscoverySourceKind;
  serviceAddr: string | null;
  enabled: boolean;
}) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const addr = normalizeQueryServiceAddress(serviceAddr);
  const queryKey = ["model-discovery", "list", addr] as const;
  const [selection, setSelection] = useState<{
    source: ModelSyncSource;
    addr: string | null;
  } | null>(null);
  useEffect(() => {
    if (!selection || (enabled && selection.addr === addr)) return;
    const frame = window.requestAnimationFrame(() => setSelection(null));
    return () => window.cancelAnimationFrame(frame);
  }, [addr, enabled, selection]);
  const query = useQuery({
    queryKey,
    queryFn: () => modelDiscoveryClient.list(addr),
    enabled,
    staleTime: 5_000,
    refetchInterval: enabled ? 5_000 : false,
    refetchIntervalInBackground: false,
    retry: 1,
  });
  const tasks = useMemo(
    () => new Map(
      (query.data?.items ?? [])
        .filter((task) => task.sourceKind === sourceKind)
        .map((task) => [task.sourceId, task]),
    ),
    [query.data, sourceKind],
  );
  const successFingerprint = [...tasks.values()]
    .filter((task) => task.lastSuccessAt !== null)
    .map((task) => `${task.sourceId}:${task.lastSuccessAt}`)
    .sort()
    .join("|");
  const observedSuccess = useRef("");
  useEffect(() => {
    if (!enabled || !successFingerprint) return;
    const fingerprint = `${addr}:${sourceKind}:${successFingerprint}`;
    if (observedSuccess.current === fingerprint) return;
    observedSuccess.current = fingerprint;
    // A background run can add models while another page's cached data is retained.
    for (const key of [
      buildManagedModelListQueryKey(addr, true),
      buildManagedModelListQueryKey(addr, false),
      buildManagedModelSelectorQueryKey(addr),
      buildAggregateApiListQueryKey(addr),
      buildApiKeyListQueryKey(addr),
      ["startup-snapshot", addr],
    ]) {
      void queryClient.invalidateQueries({ queryKey: key });
    }
  }, [addr, enabled, queryClient, sourceKind, successFingerprint]);

  const mutation = useMutation({
    mutationFn: ({ sourceId, enabled: taskEnabled, intervalMinutes, requestAddr }: {
      sourceId: string;
      enabled: boolean;
      intervalMinutes?: number;
      requestAddr: string | null;
    }) => modelDiscoveryClient.update({
      sourceKind,
      sourceId,
      enabled: taskEnabled,
      ...(intervalMinutes === undefined ? {} : { intervalMinutes }),
    }, requestAddr),
    onSuccess: (task: ModelDiscoveryTask, variables) => {
      const updatedKey = ["model-discovery", "list", variables.requestAddr] as const;
      queryClient.setQueryData<ModelDiscoveryList>(updatedKey, (current) => ({
        items: [
          ...(current?.items ?? []).filter((item) =>
            item.sourceKind !== task.sourceKind || item.sourceId !== task.sourceId),
          task,
        ],
      }));
      toast.success(task.enabled ? t("后台同步设置已保存") : t("后台同步已停止"));
      void queryClient.invalidateQueries({ queryKey: updatedKey });
    },
    onError: (error: unknown) => {
      toast.error(t("更新后台同步失败：{error}", { error: sourceModelSyncMessage(getAppErrorMessage(error), t) }));
    },
  });

  const selectedSource = selection?.addr === addr ? selection.source : null;
  return {
    sourceKind,
    tasks,
    selectedSource,
    task: selectedSource ? tasks.get(selectedSource.id) : undefined,
    open: enabled && Boolean(selectedSource),
    isLoading: query.isPending,
    hasData: Boolean(query.data),
    error: query.error ? getAppErrorMessage(query.error) : null,
    isSaving: mutation.isPending,
    available: enabled,
    openSource: (source: ModelSyncSource) => {
      if (enabled) setSelection({ source, addr });
    },
    onOpenChange: (open: boolean) => {
      if (!open) setSelection(null);
    },
    refresh: () => { void query.refetch(); },
    save: (taskEnabled: boolean, intervalMinutes?: number) => {
      if (!enabled || !selectedSource || mutation.isPending || !query.data) return;
      mutation.mutate({ sourceId: selectedSource.id, enabled: taskEnabled, intervalMinutes, requestAddr: addr });
    },
  };
}

export type SourceModelSyncController = ReturnType<typeof useSourceModelSync>;
