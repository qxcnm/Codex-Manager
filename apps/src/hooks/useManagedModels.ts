"use client";

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { managedModelsV2Client } from "@/lib/api/managed-models-v2";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import type {
  ManagedModelBatchRouteAssignmentV2,
  ManagedModelBatchRouteResultV2,
  ManagedModelBatchStateV2Update,
  ManagedModelImportPreviewV2Result,
  ManagedModelImportV2Params,
  ManagedModelListV2Result,
  ManagedModelV2,
  ManagedModelV2Upsert,
  ModelCatalogV2Stats,
  ModelRouteV2,
  ModelVisibilityV2,
} from "@/types/model-v2";

export const MANAGED_MODELS_V2_QUERY_KEY = ["managed-models-v2"] as const;

const EMPTY_STATS: ModelCatalogV2Stats = {
  total: 0,
  enabled: 0,
  builtin: 0,
  custom: 0,
  priceMissing: 0,
  missingRoute: 0,
};

type BatchDeleteManagedModelsResult = {
  hidden: string[];
  deleted: string[];
  failed: Array<{ slug: string; reason: string }>;
};

type UpdateManagedModelStateInput = {
  model: ManagedModelV2;
  enabled: boolean;
  visibility: ModelVisibilityV2;
};

function routeAssignmentKey(sourceKind: string, sourceId: string): string {
  return `${sourceKind}\u0000${sourceId}`;
}

function buildCatalogStats(items: ManagedModelV2[]): ModelCatalogV2Stats {
  return {
    total: items.length,
    enabled: items.filter((model) => model.enabled).length,
    builtin: items.filter((model) => model.origin === "builtin").length,
    custom: items.filter((model) => model.origin === "custom").length,
    priceMissing: items.filter(
      (model) => model.price.priceStatus === "missing",
    ).length,
    missingRoute: items.filter(
      (model) => !model.routes.some((route) => route.enabled),
    ).length,
  };
}

export function useManagedModels() {
  const queryClient = useQueryClient();
  const { t } = useI18n();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/models/");
  const isQueryEnabled = useDeferredDesktopActivation(
    isServiceReady && isPageActive,
  );

  const ensureServiceReady = (actionLabel: string): boolean => {
    if (isServiceReady) return true;
    toast.info(`${t("服务未连接，暂时无法")} ${t(actionLabel)}`);
    return false;
  };

  const query = useQuery({
    queryKey: MANAGED_MODELS_V2_QUERY_KEY,
    queryFn: () => managedModelsV2Client.list(true),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const invalidateConsumers = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["model-groups"] }),
      queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
    ]);
  };

  const reloadCatalog = async (): Promise<ManagedModelListV2Result> => {
    const result = await query.refetch();
    if (result.error) throw result.error;
    if (!result.data) throw new Error(t("读取模型失败"));
    return result.data;
  };

  const refreshCatalogAfterCommittedMutation = async (): Promise<void> => {
    try {
      await reloadCatalog();
      await invalidateConsumers();
    } catch (error) {
      toast.error(`${t("读取模型失败")}: ${getAppErrorMessage(error)}`);
    }
  };

  const applyCommittedDeletesToCache = (
    hiddenSlugs: string[],
    deletedSlugs: string[],
  ): void => {
    const hidden = new Set(hiddenSlugs);
    const deleted = new Set(deletedSlugs);
    queryClient.setQueryData<ManagedModelListV2Result>(
      MANAGED_MODELS_V2_QUERY_KEY,
      (current) => {
        if (!current) return current;
        const items = current.items
          .filter((model) => !deleted.has(model.slug))
          .map((model) =>
            hidden.has(model.slug)
              ? {
                  ...model,
                  enabled: false,
                  visibility: "hide" as const,
                  userEdited: true,
                }
              : model,
          );
        return { items, stats: buildCatalogStats(items) };
      },
    );
  };

  const applyCommittedModelsToCache = (
    savedModels: ManagedModelV2[],
  ): void => {
    if (savedModels.length === 0) return;
    const savedBySlug = new Map(
      savedModels.map((model) => [model.slug, model] as const),
    );
    queryClient.setQueryData<ManagedModelListV2Result>(
      MANAGED_MODELS_V2_QUERY_KEY,
      (current) => {
        if (!current) return current;
        const items = current.items.map((model) =>
          savedBySlug.get(model.slug) ?? model,
        );
        return { items, stats: buildCatalogStats(items) };
      },
    );
  };

  const saveMutation = useMutation({
    mutationFn: (input: ManagedModelV2Upsert) =>
      managedModelsV2Client.upsert(input),
    onSuccess: async () => {
      await reloadCatalog();
      await invalidateConsumers();
      toast.success(t("模型已保存"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("保存模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const modelStateMutation = useMutation({
    mutationFn: ({
      model,
      enabled,
      visibility,
    }: UpdateManagedModelStateInput) =>
      managedModelsV2Client.updateState({
        slug: model.slug,
        enabled,
        visibility,
      }),
    onSuccess: (savedModel, input) => {
      applyCommittedModelsToCache([savedModel]);
      if (input.visibility === "hide") {
        toast.success(
          input.enabled
            ? t("模型 {slug} 已隐藏但保持启用", { slug: savedModel.slug })
            : t("模型 {slug} 已隐藏并禁用", { slug: savedModel.slug }),
        );
      } else if (input.enabled) {
        toast.success(
          input.model.visibility === "hide"
            ? t("模型 {slug} 已恢复并启用", { slug: savedModel.slug })
            : t("模型 {slug} 已启用并显示", { slug: savedModel.slug }),
        );
      } else {
        toast.success(
          input.model.visibility === "hide"
            ? t("模型 {slug} 已恢复显示但保持禁用", {
                slug: savedModel.slug,
              })
            : t("模型 {slug} 已禁用但保留显示", { slug: savedModel.slug }),
        );
      }
      void refreshCatalogAfterCommittedMutation();
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新模型状态失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const batchModelStateMutation = useMutation({
    mutationFn: async ({
      slugs,
      enabled,
      visibility,
    }: ManagedModelBatchStateV2Update): Promise<ManagedModelV2[]> => {
      const normalizedSlugs = Array.from(
        new Set(slugs.map((slug) => slug.trim()).filter(Boolean)),
      );
      if (normalizedSlugs.length === 0) return [];
      return managedModelsV2Client.updateStates({
        slugs: normalizedSlugs,
        enabled,
        visibility,
      });
    },
    onSuccess: (updated) => {
      if (updated.length > 0) {
        applyCommittedModelsToCache(updated);
      }
      if (updated.length > 0) {
        toast.success(
          t("已更新 {count} 个模型的状态", {
            count: updated.length,
          }),
        );
      }
      if (updated.length > 0) {
        void refreshCatalogAfterCommittedMutation();
      }
    },
    onError: (error: unknown) => {
      toast.error(
        `${t("批量更新模型状态失败")}: ${getAppErrorMessage(error)}`,
      );
    },
  });

  const deleteMutation = useMutation({
    mutationFn: async (slug: string) => {
      const catalog =
        queryClient.getQueryData<ManagedModelListV2Result>(
          MANAGED_MODELS_V2_QUERY_KEY,
        ) ?? query.data;
      const isBuiltin =
        catalog?.items.find((model) => model.slug === slug)?.origin ===
        "builtin";
      await managedModelsV2Client.delete(slug);
      return { isBuiltin, slug };
    },
    onSuccess: ({ isBuiltin, slug }) => {
      applyCommittedDeletesToCache(
        isBuiltin ? [slug] : [],
        isBuiltin ? [] : [slug],
      );
      toast.success(
        isBuiltin
          ? t("已隐藏内置模型 {slug}", { slug })
          : t("已删除自定义模型 {slug}", { slug }),
      );
      void refreshCatalogAfterCommittedMutation();
    },
    onError: (error: unknown) => {
      toast.error(`${t("删除模型失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const batchDeleteMutation = useMutation({
    mutationFn: async (slugs: string[]): Promise<BatchDeleteManagedModelsResult> => {
      const normalizedSlugs = Array.from(
        new Set(slugs.map((slug) => slug.trim()).filter(Boolean)),
      );
      const catalog =
        queryClient.getQueryData<ManagedModelListV2Result>(
          MANAGED_MODELS_V2_QUERY_KEY,
        ) ?? query.data;
      const hidden: string[] = [];
      const deleted: string[] = [];
      const failed: Array<{ slug: string; reason: string }> = [];
      for (const slug of normalizedSlugs) {
        try {
          await managedModelsV2Client.delete(slug);
          if (
            catalog?.items.find((model) => model.slug === slug)?.origin ===
            "builtin"
          ) {
            hidden.push(slug);
          } else {
            deleted.push(slug);
          }
        } catch (error) {
          failed.push({ slug, reason: getAppErrorMessage(error) });
        }
      }
      return { hidden, deleted, failed };
    },
    onSuccess: (result) => {
      const processedCount = result.hidden.length + result.deleted.length;
      if (processedCount > 0) {
        applyCommittedDeletesToCache(result.hidden, result.deleted);
      }
      if (processedCount > 0 && result.failed.length === 0) {
        if (result.hidden.length > 0 && result.deleted.length > 0) {
          toast.success(
            t("已隐藏 {hidden} 个内置模型，并删除 {deleted} 个自定义模型", {
              hidden: result.hidden.length,
              deleted: result.deleted.length,
            }),
          );
        } else if (result.hidden.length > 0) {
          toast.success(
            t("已隐藏 {count} 个内置模型", {
              count: result.hidden.length,
            }),
          );
        } else {
          toast.success(
            t("已删除 {count} 个自定义模型", {
              count: result.deleted.length,
            }),
          );
        }
      } else if (processedCount > 0) {
        toast.warning(
          t("批量处理完成：隐藏{hidden}个，删除{deleted}个，失败{failed}个", {
            hidden: result.hidden.length,
            deleted: result.deleted.length,
            failed: result.failed.length,
          }),
        );
      } else if (result.failed.length > 0) {
        toast.error(
          `${t("批量删除失败")}: ${result.failed[0].slug} - ${result.failed[0].reason}`,
        );
      }
      if (processedCount > 0) {
        void refreshCatalogAfterCommittedMutation();
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量删除失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const batchAssignRoutesMutation = useMutation({
    mutationFn: async (
      input: ManagedModelBatchRouteAssignmentV2,
    ): Promise<ManagedModelBatchRouteResultV2> => {
      const catalog =
        queryClient.getQueryData<ManagedModelListV2Result>(
          MANAGED_MODELS_V2_QUERY_KEY,
        ) ?? query.data;
      const normalizedSlugs = Array.from(
        new Set(input.slugs.map((slug) => slug.trim()).filter(Boolean)),
      );
      const templates = input.routes.map((route) => ({
        ...route,
        sourceId:
          route.sourceKind === "account_pool" ? "default" : route.sourceId.trim(),
      }));
      if (normalizedSlugs.length === 0 || templates.length === 0) {
        throw new Error(t("请选择模型并至少配置一条路由"));
      }

      const updated: string[] = [];
      const failed: Array<{ slug: string; reason: string }> = [];
      for (const slug of normalizedSlugs) {
        const model = catalog?.items.find((item) => item.slug === slug);
        if (!model) {
          failed.push({ slug, reason: t("模型不存在") });
          continue;
        }

        const existingRoutes = new Map<string, ModelRouteV2>();
        for (const route of model.routes) {
          const key = routeAssignmentKey(route.sourceKind, route.sourceId);
          if (!existingRoutes.has(key)) existingRoutes.set(key, route);
        }
        const assignmentKeys = new Set(
          templates.map((route) =>
            routeAssignmentKey(route.sourceKind, route.sourceId),
          ),
        );
        const assignedRoutes: ModelRouteV2[] = templates.map((route) => {
          const existing = existingRoutes.get(
            routeAssignmentKey(route.sourceKind, route.sourceId),
          );
          return {
            id: existing?.id || "",
            sourceKind: route.sourceKind,
            sourceId: route.sourceId,
            upstreamModel: model.slug,
            enabled: true,
            priority: route.priority,
            weight: route.weight,
          };
        });
        const routes =
          input.mode === "replace"
            ? assignedRoutes
            : [
                ...model.routes.filter(
                  (route) =>
                    !assignmentKeys.has(
                      routeAssignmentKey(route.sourceKind, route.sourceId),
                    ),
                ),
                ...assignedRoutes,
              ];

        try {
          await managedModelsV2Client.upsert({
            previousSlug: model.slug,
            model: { ...model, routes },
          });
          updated.push(slug);
        } catch (error) {
          failed.push({ slug, reason: getAppErrorMessage(error) });
        }
      }
      return { updated, failed };
    },
    onSuccess: async (result) => {
      await reloadCatalog();
      await invalidateConsumers();
      if (result.updated.length > 0 && result.failed.length === 0) {
        toast.success(
          t("已为 {count} 个模型分配路由", { count: result.updated.length }),
        );
      } else if (result.updated.length > 0) {
        toast.warning(
          t("批量分配完成：成功{success}个，失败{failed}个", {
            success: result.updated.length,
            failed: result.failed.length,
          }),
        );
      } else if (result.failed.length > 0) {
        toast.error(
          `${t("批量分配路由失败")}: ${result.failed[0].slug} - ${result.failed[0].reason}`,
        );
      }
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量分配路由失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const previewImportMutation = useMutation({
    mutationFn: (input: ManagedModelImportV2Params) =>
      managedModelsV2Client.previewImport(input),
    onError: (error: unknown) => {
      toast.error(`${t("导入预览失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const commitImportMutation = useMutation({
    mutationFn: (input: ManagedModelImportV2Params) =>
      managedModelsV2Client.commitImport(input),
    onSuccess: async (result) => {
      await reloadCatalog();
      await invalidateConsumers();
      toast.success(t("已导入 {count} 个模型", { count: result.committed }));
    },
    onError: (error: unknown) => {
      toast.error(`${t("导入提交失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  return {
    models: query.data?.items || [],
    catalog: query.data || { items: [], stats: EMPTY_STATS },
    stats: query.data?.stats || EMPTY_STATS,
    isLoading: isServiceReady && (!isQueryEnabled || query.isLoading),
    isServiceReady,
    refreshLocal: async () => {
      if (!ensureServiceReady("读取模型")) return null;
      try {
        const result = await reloadCatalog();
        toast.success(t("模型目录已重新读取"));
        return result;
      } catch (error) {
        toast.error(`${t("读取模型失败")}: ${getAppErrorMessage(error)}`);
        return null;
      }
    },
    saveModel: async (input: ManagedModelV2Upsert) => {
      if (!ensureServiceReady("保存模型")) return null;
      return saveMutation.mutateAsync(input);
    },
    updateModelState: async (input: UpdateManagedModelStateInput) => {
      if (!ensureServiceReady("更新模型状态")) return false;
      try {
        await modelStateMutation.mutateAsync(input);
        return true;
      } catch {
        return false;
      }
    },
    updateModelStates: async (input: ManagedModelBatchStateV2Update) => {
      if (!ensureServiceReady("批量更新模型状态")) {
        return [];
      }
      return batchModelStateMutation.mutateAsync(input);
    },
    deleteModel: async (slug: string) => {
      if (!ensureServiceReady("删除模型")) return false;
      await deleteMutation.mutateAsync(slug);
      return true;
    },
    deleteModels: async (slugs: string[]) => {
      if (!ensureServiceReady("批量删除模型")) {
        return { hidden: [], deleted: [], failed: [] };
      }
      return batchDeleteMutation.mutateAsync(slugs);
    },
    assignModelRoutes: async (input: ManagedModelBatchRouteAssignmentV2) => {
      if (!ensureServiceReady("批量分配模型路由")) {
        return null;
      }
      return batchAssignRoutesMutation.mutateAsync(input);
    },
    previewImport: async (
      input: ManagedModelImportV2Params,
    ): Promise<ManagedModelImportPreviewV2Result | null> => {
      if (!ensureServiceReady("导入模型")) return null;
      return previewImportMutation.mutateAsync(input);
    },
    commitImport: async (
      input: ManagedModelImportV2Params,
    ): Promise<ManagedModelImportPreviewV2Result | null> => {
      if (!ensureServiceReady("导入模型")) return null;
      return commitImportMutation.mutateAsync(input);
    },
    isRefreshing: query.isRefetching,
    isSaving: saveMutation.isPending,
    isUpdatingModelState:
      modelStateMutation.isPending || batchModelStateMutation.isPending,
    isBatchUpdatingModelState: batchModelStateMutation.isPending,
    updatingModelStateSlug: modelStateMutation.isPending
      ? (modelStateMutation.variables?.model.slug ?? null)
      : null,
    isDeleting: deleteMutation.isPending || batchDeleteMutation.isPending,
    isAssigningRoutes: batchAssignRoutesMutation.isPending,
    isImporting:
      previewImportMutation.isPending || commitImportMutation.isPending,
  };
}
