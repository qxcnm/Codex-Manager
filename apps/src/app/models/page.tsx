"use client";

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Boxes,
  CircleDollarSign,
  Database,
  Download,
  EyeOff,
  FileJson,
  GitBranch,
  PencilLine,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";

import { PageHeader, MetricCard, PageWorkspace } from "@/components/layout/page-workspace";
import { BatchModelStateDropdown } from "@/components/models/batch-model-state-dropdown";
import {
  ModelStateDropdown,
  type ModelStateTarget,
} from "@/components/models/model-state-dropdown";
import { BatchModelRoutesModal } from "@/components/modals/batch-model-routes-modal";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { ModelCatalogModal } from "@/components/modals/model-catalog-modal";
import { ModelImportModal } from "@/components/modals/model-import-modal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { isAdminRole, resolveSessionRole, useAppSession } from "@/hooks/useAppSession";
import { useManagedModels } from "@/hooks/useManagedModels";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { accountClient } from "@/lib/api/account-client";
import { microusdToUsdPerMillion } from "@/lib/api/managed-models-v2";
import { useI18n } from "@/lib/i18n/provider";
import type {
  ManagedModelBatchRouteAssignmentV2,
  ManagedModelV2,
  ModelInstructionsModeV2,
  ModelPriceStatusV2,
  ModelRouteSourceKindV2,
} from "@/types/model-v2";

type ModelFilter =
  | "all"
  | "enabled"
  | "builtin"
  | "custom"
  | "price_missing"
  | "route_missing"
  | "hidden";

const MODEL_FILTER_LABELS: Record<ModelFilter, string> = {
  all: "全部模型",
  enabled: "已启用",
  builtin: "内置模型",
  custom: "自定义模型",
  price_missing: "价格缺失",
  route_missing: "路由缺失",
  hidden: "已隐藏",
};

function modelFilterLabel(
  filter: ModelFilter,
  t: (message: string) => string,
): string {
  return t(MODEL_FILTER_LABELS[filter] || MODEL_FILTER_LABELS.all);
}

function enabledRouteCount(model: ManagedModelV2): number {
  return model.routes.filter((route) => route.enabled).length;
}

function modelMatchesFilter(model: ManagedModelV2, filter: ModelFilter): boolean {
  if (model.visibility === "hide") return filter === "hidden";
  if (filter === "hidden") return false;
  if (filter === "enabled") return model.enabled;
  if (filter === "builtin") return model.origin === "builtin";
  if (filter === "custom") return model.origin === "custom";
  if (filter === "price_missing") return model.price.priceStatus === "missing";
  if (filter === "route_missing") return enabledRouteCount(model) === 0;
  return true;
}

const BUILTIN_MODEL_DESCRIPTION_KEYS: Record<string, string> = {
  "gpt-5.6-sol": "最新的前沿智能体编程模型。",
  "gpt-5.6-terra": "适合日常工作的均衡型智能体编程模型。",
  "gpt-5.6-luna": "快速且经济的智能体编程模型。",
  "gpt-5.5": "适合复杂编程、研究和真实工作场景的前沿模型。",
  "gpt-5.4": "适合日常编程的强大模型。",
  "gpt-5.4-mini": "适合简单编程任务的小型、快速且高性价比模型。",
  "gpt-5.2": "针对专业工作和长时间运行智能体优化的模型。",
  "gpt-image-2": "先进的图像生成和编辑模型。",
  "codex-auto-review": "用于 Codex 自动审批审查的模型。",
};

function modelDescription(
  model: ManagedModelV2,
  t: (message: string) => string,
): string | null {
  const builtinDescription = BUILTIN_MODEL_DESCRIPTION_KEYS[model.slug];
  return model.origin === "builtin" && builtinDescription
    ? t(builtinDescription)
    : model.description;
}

function priceStatusLabel(
  status: ModelPriceStatusV2,
  t: (message: string) => string,
): string {
  if (status === "official") return t("官方价格");
  if (status === "estimated") return t("估算价格");
  if (status === "custom") return t("自定义价格");
  return t("价格缺失");
}

function instructionsModeLabel(
  mode: ModelInstructionsModeV2,
  t: (message: string) => string,
): string {
  if (mode === "fallback") return t("兜底");
  if (mode === "override") return t("覆盖");
  return t("透传");
}

function routeSourceLabel(
  sourceKind: ModelRouteSourceKindV2,
  sourceId: string,
  t: (message: string) => string,
): string {
  if (sourceKind === "account_pool") return `${t("账号池")}:${t("默认")}`;
  return `${t("聚合 API")}:${sourceId}`;
}

function PriceBadge({ model }: { model: ManagedModelV2 }) {
  const { t } = useI18n();
  if (model.price.priceStatus === "missing") {
    return <Badge variant="destructive">{t("价格缺失")}</Badge>;
  }
  const input = microusdToUsdPerMillion(model.price.inputMicrousdPer1m);
  const cached = microusdToUsdPerMillion(model.price.cachedInputMicrousdPer1m);
  const output = microusdToUsdPerMillion(model.price.outputMicrousdPer1m);
  return (
    <div className="space-y-1">
      <Badge variant="secondary">{priceStatusLabel(model.price.priceStatus, t)}</Badge>
      <div className="font-mono text-[10px] text-muted-foreground">
        {input} / {cached} / {output}
      </div>
    </div>
  );
}

export default function ModelsPage() {
  const { t } = useI18n();
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const isAdminMode = isAdminRole(role);
  const isPageActive = useDesktopPageActive("/models/");
  const {
    models,
    stats,
    isLoading,
    isServiceReady,
    refreshLocal,
    saveModel,
    updateModelState,
    updateModelStates,
    deleteModel,
    deleteModels,
    assignModelRoutes,
    previewImport,
    commitImport,
    exportCodexCache,
    canExportCodexCache,
    isRefreshing,
    isSaving,
    isUpdatingModelState,
    isBatchUpdatingModelState,
    updatingModelStateSlug,
    isDeleting,
    isAssigningRoutes,
    isImporting,
    isExporting,
  } = useManagedModels();
  usePageTransitionReady("/models/", !isServiceReady || !isLoading);
  const isModelOperationPending =
    isLoading ||
    isRefreshing ||
    isSaving ||
    isDeleting ||
    isAssigningRoutes ||
    isImporting ||
    isUpdatingModelState;

  const { data: aggregateApis = [] } = useQuery({
    queryKey: ["aggregate-apis"],
    queryFn: () => accountClient.listAggregateApis(),
    enabled:
      isServiceReady && isPageActive && isAdminMode && !isSessionLoading,
    staleTime: 60_000,
    retry: 1,
  });

  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<ModelFilter>("all");
  const [editorOpen, setEditorOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [batchRoutesOpen, setBatchRoutesOpen] = useState(false);
  const [editingSlug, setEditingSlug] = useState<string | null>(null);
  const [selectedSlugs, setSelectedSlugs] = useState<string[]>([]);
  const [deleteSlugs, setDeleteSlugs] = useState<string[]>([]);

  useEffect(() => {
    if (isPageActive) return;
    const frameId = window.requestAnimationFrame(() => {
      setEditorOpen(false);
      setImportOpen(false);
      setBatchRoutesOpen(false);
      setEditingSlug(null);
      setSelectedSlugs([]);
      setDeleteSlugs([]);
    });
    return () => window.cancelAnimationFrame(frameId);
  }, [isPageActive]);

  useEffect(() => {
    const availableSlugs = new Set(models.map((model) => model.slug));
    const frameId = window.requestAnimationFrame(() => {
      setSelectedSlugs((current) =>
        current.filter((slug) => availableSlugs.has(slug)),
      );
    });
    return () => window.cancelAnimationFrame(frameId);
  }, [models]);

  const filteredModels = useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    return models.filter((model) => {
      if (!modelMatchesFilter(model, filter)) return false;
      if (!needle) return true;
      return [
        model.slug,
        model.displayName,
        model.description || "",
        model.provider || "",
        model.family || "",
        ...model.tags,
      ].some((value) => value.toLocaleLowerCase().includes(needle));
    });
  }, [filter, models, search]);

  const editingModel = useMemo(
    () => models.find((model) => model.slug === editingSlug) || null,
    [editingSlug, models],
  );
  const nextSortOrder = useMemo(
    () => models.reduce((largest, model) => Math.max(largest, model.sortOrder), 0) + 10,
    [models],
  );
  const selectedVisibleCount = filteredModels.filter((model) =>
    selectedSlugs.includes(model.slug),
  ).length;
  const allVisibleSelected =
    filteredModels.length > 0 && selectedVisibleCount === filteredModels.length;

  const openNewModel = () => {
    setEditingSlug(null);
    setEditorOpen(true);
  };

  const openEditor = (slug: string) => {
    setEditingSlug(slug);
    setEditorOpen(true);
  };

  const updateSelectedModelStates = async (target: ModelStateTarget) => {
    const targets = [...selectedSlugs];
    try {
      const result = await updateModelStates({ slugs: targets, ...target });
      const processed = new Set(result.map((model) => model.slug));
      setSelectedSlugs((current) =>
        current.filter((slug) => !processed.has(slug)),
      );
    } catch {
      // The mutation already reports the normalized error and keeps selections.
    }
  };

  const confirmDeleteDescription = useMemo(() => {
    if (deleteSlugs.length === 0) return "";
    const builtinCount = deleteSlugs.filter(
      (slug) => models.find((model) => model.slug === slug)?.origin === "builtin",
    ).length;
    if (deleteSlugs.length === 1) {
      const model = models.find((item) => item.slug === deleteSlugs[0]);
      return model?.origin === "builtin"
        ? t("内置模型 {slug} 将被隐藏并禁用，数据不会删除。", {
            slug: model.slug,
          })
        : t("确定要永久删除自定义模型 {slug} 吗？", { slug: deleteSlugs[0] });
    }
    return t(
      "将处理 {count} 个模型：{builtin} 个内置模型会被隐藏并禁用，其余自定义模型会被删除。",
      { count: deleteSlugs.length, builtin: builtinCount },
    );
  }, [deleteSlugs, models, t]);

  return (
    <>
      <PageWorkspace>
        <PageHeader
          title={isAdminMode ? t("模型管理") : t("可用模型")}
          description={t("本地模型目录是唯一运行时真相源；价格、路由和指令策略会原子保存。")}
          actions={
            <>
              <Button
                size="sm"
                variant="outline"
                disabled={!isServiceReady || isModelOperationPending}
                onClick={() => void refreshLocal()}
              >
                <RefreshCw className={`mr-1.5 h-4 w-4 ${isRefreshing ? "animate-spin" : ""}`} />
                {t("重新读取")}
              </Button>
              {isAdminMode ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={isModelOperationPending}
                  onClick={() => setImportOpen(true)}
                >
                  <FileJson className="mr-1.5 h-4 w-4" />
                  {t("从本地 JSON 导入")}
                </Button>
              ) : null}
              <Button
                size="sm"
                variant="outline"
                disabled={!canExportCodexCache || isExporting}
                onClick={() => void exportCodexCache()}
              >
                <Download className="mr-1.5 h-4 w-4" />
                {isExporting ? t("导出中...") : t("导出到本地 Codex 缓存")}
              </Button>
              {isAdminMode ? (
                <Button
                  size="sm"
                  disabled={!isServiceReady || isModelOperationPending}
                  onClick={openNewModel}
                >
                  <Plus className="mr-1.5 h-4 w-4" />
                  {t("新增自定义模型")}
                </Button>
              ) : null}
            </>
          }
        />

        <section className="grid grid-cols-2 gap-2 md:grid-cols-3 xl:grid-cols-6">
          <MetricCard title={t("总数")} value={stats.total} icon={Database} tone="blue" />
          <MetricCard title={t("已启用")} value={stats.enabled} icon={Boxes} tone="emerald" />
          <MetricCard title={t("内置模型")} value={stats.builtin} icon={Database} tone="violet" />
          <MetricCard title={t("自定义模型")} value={stats.custom} icon={Plus} tone="slate" />
          <MetricCard title={t("价格缺失")} value={stats.priceMissing} icon={CircleDollarSign} tone="amber" />
          <MetricCard title={t("路由缺失")} value={stats.missingRoute} icon={GitBranch} tone="rose" />
        </section>

        <Card className="glass-card overflow-hidden py-0">
          <CardHeader className="border-b border-border/50 px-4 py-3">
            <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
              <div>
                <CardTitle>{t("模型目录明细")}</CardTitle>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t("显示来源、启用状态、价格状态、指令模式和路由状态。")}
                  {isAdminMode ? (
                    <span className="mt-0.5 block text-primary/80">
                      {t("请先勾选一个或多个模型，再使用批量分配路由。")}
                    </span>
                  ) : null}
                </p>
              </div>
              <div className="flex flex-1 flex-wrap items-center justify-end gap-2">
                <div className="relative min-w-[220px] flex-1 lg:max-w-[320px]">
                  <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t("搜索模型")}
                    className="h-9 pl-9"
                  />
                </div>
                <Select
                  value={filter}
                  onValueChange={(value) => setFilter((value || "all") as ModelFilter)}
                >
                  <SelectTrigger
                    aria-label={t("筛选模型")}
                    className="h-9 w-[160px]"
                  >
                    <SelectValue>
                      {(value) =>
                        modelFilterLabel((value || "all") as ModelFilter, t)
                      }
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent><SelectGroup>
                    {(Object.keys(MODEL_FILTER_LABELS) as ModelFilter[]).map((option) => (
                      <SelectItem key={option} value={option}>
                        {modelFilterLabel(option, t)}
                      </SelectItem>
                    ))}
                  </SelectGroup></SelectContent>
                </Select>
                {isAdminMode ? (
                  <>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={
                        isModelOperationPending || selectedSlugs.length === 0
                      }
                      onClick={() => setBatchRoutesOpen(true)}
                    >
                      <GitBranch className="mr-1.5 h-4 w-4" />
                      {t("批量分配路由")} ({selectedSlugs.length})
                    </Button>
                    <BatchModelStateDropdown
                      selectedCount={selectedSlugs.length}
                      disabled={
                        isModelOperationPending || selectedSlugs.length === 0
                      }
                      isUpdating={isBatchUpdatingModelState}
                      onStateChange={updateSelectedModelStates}
                    />
                    <Button
                      size="sm"
                      variant="destructive"
                      disabled={
                        isModelOperationPending || selectedSlugs.length === 0
                      }
                      onClick={() => setDeleteSlugs(selectedSlugs)}
                    >
                      <Trash2 className="mr-1.5 h-4 w-4" />
                      {t("批量删除模型")} ({selectedSlugs.length})
                    </Button>
                  </>
                ) : null}
              </div>
            </div>
          </CardHeader>

          <CardContent className="p-0">
            {!isServiceReady ? (
              <Empty className="min-h-64">
                <EmptyHeader><EmptyTitle>{t("服务未连接，模型目录暂不可用。")}</EmptyTitle></EmptyHeader>
              </Empty>
            ) : isLoading ? (
              <div className="space-y-2 p-4">
                {Array.from({ length: 6 }).map((_, index) => (
                  <Skeleton key={index} className="h-12 w-full" />
                ))}
              </div>
            ) : filteredModels.length === 0 ? (
              <Empty className="min-h-64">
                <EmptyHeader><EmptyTitle>{t("没有符合条件的模型。")}</EmptyTitle></EmptyHeader>
              </Empty>
            ) : (
              <div className="overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      {isAdminMode ? (
                        <TableHead className="w-10">
                          <Checkbox
                            aria-label={t("选择全部模型")}
                            disabled={isModelOperationPending}
                            checked={allVisibleSelected}
                            onCheckedChange={(checked) => {
                              const visibleSlugs = filteredModels.map((model) => model.slug);
                              setSelectedSlugs((current) =>
                                checked === true
                                  ? Array.from(new Set([...current, ...visibleSlugs]))
                                  : current.filter((slug) => !visibleSlugs.includes(slug)),
                              );
                            }}
                          />
                        </TableHead>
                      ) : null}
                      <TableHead>{t("模型")}</TableHead>
                      <TableHead>{t("来源")}</TableHead>
                      <TableHead className="min-w-[132px]">{t("状态")}</TableHead>
                      <TableHead>{t("价格")}</TableHead>
                      <TableHead>{t("指令")}</TableHead>
                      <TableHead>{t("路由")}</TableHead>
                      {isAdminMode ? <TableHead className="table-sticky-action-head w-24 text-right">{t("操作")}</TableHead> : null}
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {filteredModels.map((model) => {
                      const routeCount = enabledRouteCount(model);
                      const description = modelDescription(model, t);
                      return (
                        <TableRow key={model.id || model.slug}>
                          {isAdminMode ? (
                            <TableCell>
                              <Checkbox
                                aria-label={t("选择模型 {slug}", { slug: model.slug })}
                                disabled={isModelOperationPending}
                                checked={selectedSlugs.includes(model.slug)}
                                onCheckedChange={(checked) =>
                                  setSelectedSlugs((current) =>
                                    checked === true
                                      ? Array.from(new Set([...current, model.slug]))
                                      : current.filter((slug) => slug !== model.slug),
                                  )
                                }
                              />
                            </TableCell>
                          ) : null}
                          <TableCell className="min-w-[240px]">
                            <div className="font-medium">{model.displayName}</div>
                            <div className="font-mono text-xs text-muted-foreground">{model.slug}</div>
                            {description ? <div className="mt-1 max-w-[360px] truncate text-xs text-muted-foreground">{description}</div> : null}
                          </TableCell>
                          <TableCell>
                            <div className="flex flex-wrap gap-1">
                              <Badge variant={model.origin === "builtin" ? "secondary" : "outline"}>{model.origin === "builtin" ? t("内置") : t("自定义")}</Badge>
                              {model.visibility === "hide" ? <Badge variant="outline"><EyeOff className="mr-1 h-3 w-3" />{t("隐藏")}</Badge> : null}
                            </div>
                          </TableCell>
                          <TableCell>
                            {isAdminMode ? (
                              <ModelStateDropdown
                                model={model}
                                disabled={!isServiceReady || isModelOperationPending}
                                isUpdating={
                                  isUpdatingModelState &&
                                  updatingModelStateSlug === model.slug
                                }
                                onStateChange={(target) =>
                                  void updateModelState({
                                    model,
                                    ...target,
                                  })
                                }
                              />
                            ) : (
                              <Badge variant={model.enabled ? "default" : "outline"}>{model.enabled ? t("已启用") : t("已禁用")}</Badge>
                            )}
                          </TableCell>
                          <TableCell><PriceBadge model={model} /></TableCell>
                          <TableCell><Badge variant="outline">{instructionsModeLabel(model.instructionsMode, t)}</Badge></TableCell>
                          <TableCell>
                            {routeCount > 0 ? (
                              <div className="space-y-1">
                                <Badge variant="secondary">{t("{count} 条路由", { count: routeCount })}</Badge>
                                <div className="max-w-[220px] truncate text-xs text-muted-foreground">
                                  {model.routes.filter((route) => route.enabled).map((route) => routeSourceLabel(route.sourceKind, route.sourceId, t)).join("，")}
                                </div>
                              </div>
                            ) : (
                              <Badge variant="destructive">{t("路由缺失")}</Badge>
                            )}
                          </TableCell>
                          {isAdminMode ? (
                            <TableCell className="table-sticky-action-cell">
                              <div className="flex justify-end gap-1">
                                <Button type="button" variant="ghost" size="icon" disabled={isModelOperationPending} aria-label={t("编辑模型 {slug}", { slug: model.slug })} onClick={() => openEditor(model.slug)}>
                                  <PencilLine className="h-4 w-4" />
                                </Button>
                                <Button type="button" variant="ghost" size="icon" disabled={isModelOperationPending} aria-label={model.origin === "builtin" ? t("隐藏模型 {slug}", { slug: model.slug }) : t("删除模型 {slug}", { slug: model.slug })} onClick={() => setDeleteSlugs([model.slug])}>
                                  <Trash2 className="h-4 w-4" />
                                </Button>
                              </div>
                            </TableCell>
                          ) : null}
                        </TableRow>
                      );
                    })}
                  </TableBody>
                </Table>
              </div>
            )}
          </CardContent>
        </Card>
      </PageWorkspace>

      {isAdminMode && editorOpen ? (
        <ModelCatalogModal
          open
          onOpenChange={setEditorOpen}
          model={editingModel}
          nextSortOrder={nextSortOrder}
          aggregateApis={aggregateApis}
          isSaving={isSaving}
          onSave={saveModel}
        />
      ) : null}

      {isAdminMode && batchRoutesOpen ? (
        <BatchModelRoutesModal
          open
          onOpenChange={setBatchRoutesOpen}
          selectedSlugs={selectedSlugs}
          aggregateApis={aggregateApis}
          isSaving={isAssigningRoutes}
          onApply={async (input: ManagedModelBatchRouteAssignmentV2) => {
            const result = await assignModelRoutes(input);
            if (result && result.failed.length === 0) setSelectedSlugs([]);
            return result;
          }}
        />
      ) : null}

      {isAdminMode ? (
        <ModelImportModal
          open={importOpen}
          onOpenChange={setImportOpen}
          isWorking={isImporting}
          onPreview={previewImport}
          onCommit={commitImport}
        />
      ) : null}

      {isAdminMode ? (
        <ConfirmDialog
          open={deleteSlugs.length > 0}
          onOpenChange={(open) => {
            if (!open) setDeleteSlugs([]);
          }}
          title={deleteSlugs.length > 1 ? t("批量删除模型") : t("删除模型")}
          description={confirmDeleteDescription}
          confirmText={isDeleting ? t("处理中...") : t("删除")}
          confirmVariant="destructive"
          onConfirm={async () => {
            const targets = [...deleteSlugs];
            if (targets.length === 1) {
              const succeeded = await deleteModel(targets[0]);
              if (succeeded) {
                setSelectedSlugs((current) => current.filter((slug) => slug !== targets[0]));
              }
              return succeeded;
            }
            const result = await deleteModels(targets);
            const processed = new Set([...result.hidden, ...result.deleted]);
            setSelectedSlugs((current) =>
              current.filter((slug) => !processed.has(slug)),
            );
            if (result.failed.length > 0) {
              setDeleteSlugs(result.failed.map((item) => item.slug));
              return false;
            }
            return processed.size > 0;
          }}
        />
      ) : null}
    </>
  );
}
