"use client";

import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  accountClient,
  ManagedModelPayload,
  ManagedModelSourceMappingPayload,
  ManagedModelSourceModelPayload,
  ManagedModelSourceSyncPayload,
  ModelPriceRuleUpsertPayload,
} from "@/lib/api/account-client";
import { serviceClient } from "@/lib/api/service-client";
import {
  buildCodexModelsCachePayload,
  serializeManagedModelCatalogForCodexCache,
} from "@/lib/api/model-catalog";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { ManagedModelCatalog, ManagedModelRouting } from "@/types";

const MANAGED_MODEL_QUERY_KEY = ["managed-model-catalog"];
const MANAGED_MODEL_ROUTING_QUERY_KEY = ["managed-model-routing"];

type BatchDeleteManagedModelsResult = {
  deleted: string[];
  failed: Array<{ slug: string; reason: string }>;
};

export function useManagedModels() {
  const queryClient = useQueryClient();
  const { t } = useI18n();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const {
    canAccessManagementRpc,
    isDesktopRuntime,
    canUseBrowserDownloadExport,
  } = useRuntimeCapabilities();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/models/");
  const isQueryEnabled = useDeferredDesktopActivation(isServiceReady && isPageActive);
  const codexUserAgentRef = useRef("");
  const syncedCatalogFingerprintRef = useRef("");

  const ensureServiceReady = (actionLabel: string): boolean => {
    if (isServiceReady) {
      return true;
    }
    toast.info(`${t("服务未连接，暂时无法")} ${t(actionLabel)}`);
    return false;
  };

  const resolveCodexUserAgent = async (): Promise<string> => {
    const cachedUserAgent = codexUserAgentRef.current.trim();
    if (cachedUserAgent.includes("codex_cli_rs/")) {
      return cachedUserAgent;
    }

    const initializeResult = await serviceClient.initialize(serviceStatus.addr);
    const userAgent = String(initializeResult.userAgent || "").trim();
    if (!userAgent.includes("codex_cli_rs/")) {
      throw new Error(t("当前服务未返回可用的 Codex CLI 标识"));
    }

    codexUserAgentRef.current = userAgent;
    return userAgent;
  };

  const triggerBrowserDownload = (fileName: string, content: string): void => {
    if (typeof document === "undefined") {
      throw new Error(t("当前环境不支持浏览器导出"));
    }

    const blob = new Blob([content], {
      type: "application/json;charset=utf-8",
    });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = fileName;
    anchor.style.display = "none";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 0);
  };

  const syncCatalogToCodexCache = async (
    catalog: ManagedModelCatalog | null | undefined,
    options?: { force?: boolean },
  ): Promise<string | null> => {
    if (!catalog) {
      return t("模型目录为空");
    }

    if (!isDesktopRuntime) {
      return null;
    }

    if (!isServiceReady) {
      return t("服务未连接");
    }

    const models = serializeManagedModelCatalogForCodexCache(catalog.items || []);
    if (models.length === 0) {
      return t("模型目录为空");
    }

    const fingerprint = JSON.stringify(models);
    if (!options?.force && syncedCatalogFingerprintRef.current === fingerprint) {
      return null;
    }

    try {
      const userAgent = await resolveCodexUserAgent();
      await serviceClient.syncCodexModelsCache({
        userAgent,
        models,
      });
      syncedCatalogFingerprintRef.current = fingerprint;
      return null;
    } catch (error) {
      return getAppErrorMessage(error);
    }
  };

  const reloadManagedCatalog = async (): Promise<ManagedModelCatalog> => {
    const catalog = await accountClient.listManagedModels(false);
    queryClient.setQueryData(MANAGED_MODEL_QUERY_KEY, catalog);
    return catalog;
  };

  const reloadManagedRouting = async (): Promise<ManagedModelRouting> => {
    const routing = await accountClient.listManagedModelRouting();
    queryClient.setQueryData(MANAGED_MODEL_ROUTING_QUERY_KEY, routing);
    return routing;
  };

  const query = useQuery({
    queryKey: MANAGED_MODEL_QUERY_KEY,
    queryFn: () => accountClient.listManagedModels(false),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const routingQuery = useQuery({
    queryKey: MANAGED_MODEL_ROUTING_QUERY_KEY,
    queryFn: () => accountClient.listManagedModelRouting(),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const invalidateAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: MANAGED_MODEL_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: MANAGED_MODEL_ROUTING_QUERY_KEY }),
      queryClient.invalidateQueries({ queryKey: ["apikey-models"] }),
      queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
    ]);
  };

  const refreshMutation = useMutation({
    mutationFn: (refreshRemote: boolean) => accountClient.listManagedModels(refreshRemote),
    onSuccess: async (catalog) => {
      queryClient.setQueryData(MANAGED_MODEL_QUERY_KEY, catalog);
      const cacheSyncError = await syncCatalogToCodexCache(catalog);
      await invalidateAll();
      if (cacheSyncError) {
        toast.error(`${t("模型目录已刷新，但同步 Codex 模型缓存失败")}: ${cacheSyncError}`);
      } else {
        toast.success(t("模型目录已刷新"));
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("刷新模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const pruneStaleRemoteMutation = useMutation({
    mutationFn: () => accountClient.pruneStaleRemoteManagedModels(),
    onSuccess: async (catalog) => {
      queryClient.setQueryData(MANAGED_MODEL_QUERY_KEY, catalog);
      const cacheSyncError = await syncCatalogToCodexCache(catalog);
      await invalidateAll();
      if (cacheSyncError) {
        toast.error(`${t("远端旧模型已清理，但同步 Codex 模型缓存失败")}: ${cacheSyncError}`);
      } else {
        toast.success(t("远端旧模型已清理"));
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("清理远端旧模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const saveMutation = useMutation({
    mutationFn: (params: ManagedModelPayload) => accountClient.saveManagedModel(params),
    onSuccess: async () => {
      const catalog = await reloadManagedCatalog();
      const cacheSyncError = await syncCatalogToCodexCache(catalog);
      await invalidateAll();
      if (cacheSyncError) {
        toast.error(`${t("模型已保存，但同步 Codex 模型缓存失败")}: ${cacheSyncError}`);
      } else {
        toast.success(t("模型已保存"));
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("保存模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (slug: string) => accountClient.deleteManagedModel(slug),
    onSuccess: async () => {
      const catalog = await reloadManagedCatalog();
      const cacheSyncError = await syncCatalogToCodexCache(catalog);
      await invalidateAll();
      if (cacheSyncError) {
        toast.error(`${t("模型已删除，但同步 Codex 模型缓存失败")}: ${cacheSyncError}`);
      } else {
        toast.success(t("模型已删除"));
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("删除模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const batchDeleteMutation = useMutation({
    mutationFn: async (slugs: string[]): Promise<BatchDeleteManagedModelsResult> => {
      const normalizedSlugs = Array.from(
        new Set(
          slugs
            .map((slug) => String(slug || "").trim())
            .filter(Boolean)
        )
      );
      const deleted: string[] = [];
      const failed: Array<{ slug: string; reason: string }> = [];

      for (const slug of normalizedSlugs) {
        try {
          await accountClient.deleteManagedModel(slug);
          deleted.push(slug);
        } catch (error) {
          failed.push({
            slug,
            reason: getAppErrorMessage(error),
          });
        }
      }

      return {
        deleted,
        failed,
      };
    },
    onSuccess: async (result) => {
      const catalog = await reloadManagedCatalog();
      const cacheSyncError = await syncCatalogToCodexCache(catalog);
      await invalidateAll();

      if (cacheSyncError) {
        toast.error(`${t("模型已删除，但同步 Codex 模型缓存失败")}: ${cacheSyncError}`);
      } else if (result.deleted.length > 0 && result.failed.length === 0) {
        toast.success(t("已删除 {count} 个模型", { count: result.deleted.length }));
      } else if (result.deleted.length > 0) {
        toast.warning(
          t("批量删除完成：成功{success}个，失败{failed}个", {
            success: result.deleted.length,
            failed: result.failed.length,
          })
        );
      } else if (result.failed.length > 0) {
        const firstFailed = result.failed[0];
        toast.error(
          `${t("批量删除失败")}: ${firstFailed.slug} - ${firstFailed.reason}`
        );
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量删除失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const exportMutation = useMutation({
    mutationFn: async () => {
      if (!isServiceReady) {
        throw new Error(t("服务未连接"));
      }

      const catalog = query.data ?? (await reloadManagedCatalog());
      const models = catalog.items || [];
      if (!models.length) {
        throw new Error(t("模型目录为空"));
      }

      if (isDesktopRuntime) {
        const cacheSyncError = await syncCatalogToCodexCache(catalog, { force: true });
        if (cacheSyncError) {
          throw new Error(cacheSyncError);
        }
        return { mode: "desktop" as const };
      }

      if (!canUseBrowserDownloadExport) {
        throw new Error(t("当前环境不支持导出 Codex 缓存"));
      }

      const userAgent = await resolveCodexUserAgent();
      const payload = buildCodexModelsCachePayload(models, userAgent);
      triggerBrowserDownload("models_cache.json", `${JSON.stringify(payload, null, 2)}\n`);
      return { mode: "browser" as const };
    },
    onSuccess: (result) => {
      toast.success(
        result?.mode === "browser"
          ? t("Codex 缓存已下载，请保存到 `~/.codex/models_cache.json`")
          : t("已导出到本地 Codex 缓存")
      );
    },
    onError: (error) => {
      toast.error(`${t("导出失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const sourceSyncMutation = useMutation({
    mutationFn: (params: ManagedModelSourceSyncPayload) =>
      accountClient.syncManagedModelSourceModels(params),
    onSuccess: async (routing) => {
      queryClient.setQueryData(MANAGED_MODEL_ROUTING_QUERY_KEY, routing);
      await invalidateAll();
      toast.success(t("来源模型已同步"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("同步来源模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const sourceModelMutation = useMutation({
    mutationFn: (params: ManagedModelSourceModelPayload) =>
      accountClient.saveManagedModelSourceModel(params),
    onSuccess: async () => {
      await reloadManagedRouting();
      await invalidateAll();
      toast.success(t("来源模型已保存"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("保存来源模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const sourceMappingMutation = useMutation({
    mutationFn: (params: ManagedModelSourceMappingPayload) =>
      accountClient.saveManagedModelSourceMapping(params),
    onSuccess: async () => {
      await reloadManagedRouting();
      await invalidateAll();
      toast.success(t("模型映射已保存"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("保存模型映射失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const sourceMappingDeleteMutation = useMutation({
    mutationFn: (params: {
      id: string;
      sourceKind: string;
      sourceId: string;
      upstreamModel: string;
    }) => accountClient.deleteManagedModelSourceMapping(params),
    onSuccess: async () => {
      await reloadManagedRouting();
      await invalidateAll();
      toast.success(t("模型映射已删除"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("删除模型映射失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  useEffect(() => {
    codexUserAgentRef.current = "";
    syncedCatalogFingerprintRef.current = "";
  }, [serviceStatus.addr]);

  useEffect(() => {
    if (!isDesktopRuntime || !isServiceReady || !query.data || query.dataUpdatedAt === 0) {
      return;
    }

    void syncCatalogToCodexCache(query.data).then((errorMessage) => {
      if (errorMessage) {
        console.warn("sync codex models cache failed", errorMessage);
      }
    });
  }, [
    isDesktopRuntime,
    isServiceReady,
    query.data,
    query.dataUpdatedAt,
  ]);

  return {
    models: query.data?.items || [],
    catalog: query.data || { items: [] },
    routing: routingQuery.data || { sourceModels: [], mappings: [] },
    isLoading: isServiceReady && (!isQueryEnabled || query.isLoading),
    isRoutingLoading: isServiceReady && (!isQueryEnabled || routingQuery.isLoading),
    isServiceReady,
    refreshRemote: async () => {
      if (!ensureServiceReady("刷新模型")) return null;
      return refreshMutation.mutateAsync(true);
    },
    refreshLocal: async () => {
      if (!ensureServiceReady("读取模型")) return null;
      return refreshMutation.mutateAsync(false);
    },
    pruneStaleRemoteModels: async () => {
      if (!ensureServiceReady("清理远端旧模型")) return null;
      return pruneStaleRemoteMutation.mutateAsync();
    },
    saveModel: async (params: ManagedModelPayload) => {
      if (!ensureServiceReady("保存模型")) return null;
      return saveMutation.mutateAsync(params);
    },
    saveModelPriceRule: async (params: ModelPriceRuleUpsertPayload) => {
      if (!ensureServiceReady("保存模型价格")) {
        throw new Error(t("服务未就绪，无法保存模型价格"));
      }
      await accountClient.upsertModelPriceRule(params);
    },
    readModelPriceRule: async (modelPattern: string) => {
      if (!ensureServiceReady("读取模型价格")) return null;
      return accountClient.readModelPriceRule(modelPattern);
    },
    deleteModel: async (slug: string) => {
      if (!ensureServiceReady("删除模型")) return false;
      await deleteMutation.mutateAsync(slug);
      return true;
    },
    deleteModels: async (slugs: string[]) => {
      if (!ensureServiceReady("批量删除模型")) {
        return { deleted: [], failed: [] };
      }
      return batchDeleteMutation.mutateAsync(slugs);
    },
    exportCodexCache: async () => {
      if (!ensureServiceReady("导出模型目录")) return false;
      await exportMutation.mutateAsync();
      return true;
    },
    syncSourceModels: async (params: ManagedModelSourceSyncPayload) => {
      if (!ensureServiceReady("同步来源模型")) return null;
      return sourceSyncMutation.mutateAsync(params);
    },
    saveSourceModel: async (params: ManagedModelSourceModelPayload) => {
      if (!ensureServiceReady("保存来源模型")) return null;
      return sourceModelMutation.mutateAsync(params);
    },
    saveSourceMapping: async (params: ManagedModelSourceMappingPayload) => {
      if (!ensureServiceReady("保存模型映射")) return null;
      return sourceMappingMutation.mutateAsync(params);
    },
    deleteSourceMapping: async (
      mappingId: string,
      sourceKind: string,
      sourceId: string,
      upstreamModel: string,
    ) => {
      if (!ensureServiceReady("删除模型映射")) return false;
      await sourceMappingDeleteMutation.mutateAsync({
        id: mappingId,
        sourceKind,
        sourceId,
        upstreamModel,
      });
      return true;
    },
    isRefreshing: refreshMutation.isPending,
    isPruningStaleRemote: pruneStaleRemoteMutation.isPending,
    isSaving: saveMutation.isPending,
    isDeleting: deleteMutation.isPending || batchDeleteMutation.isPending,
    isExporting: exportMutation.isPending,
    isRoutingSaving:
      sourceSyncMutation.isPending ||
      sourceModelMutation.isPending ||
      sourceMappingMutation.isPending ||
      sourceMappingDeleteMutation.isPending,
    canExportCodexCache:
      !isDesktopRuntime && isServiceReady && Boolean(query.data?.items?.length),
  };
}
