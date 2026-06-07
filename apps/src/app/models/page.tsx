"use client";

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Database,
  Download,
  Link2,
  MoreVertical,
  PencilLine,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  Unlink,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Empty,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { ModelCatalogModal } from "@/components/modals/model-catalog-modal";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import {
  isAdminRole,
  resolveSessionRole,
  useAppSession,
} from "@/hooks/useAppSession";
import { useManagedModels } from "@/hooks/useManagedModels";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { accountClient, ModelPriceRuleEntry } from "@/lib/api/account-client";
import { findBestMatchingModel } from "@/lib/api/model-catalog";
import { useI18n } from "@/lib/i18n/provider";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import { ManagedModelSourceMapping, ManagedModelSourceModel } from "@/types";

type ModelFilter = "all" | "api" | "custom" | "edited";
type RoutingSourceKind = "openai_account" | "aggregate_api";

const SOURCE_KEY_SEPARATOR = "|";

function sourceCandidateKey(item: Pick<ManagedModelSourceModel, "sourceKind" | "sourceId" | "upstreamModel">) {
  return [
    item.sourceKind,
    encodeURIComponent(item.sourceId),
    encodeURIComponent(item.upstreamModel),
  ].join(SOURCE_KEY_SEPARATOR);
}

function parseInteger(value: string, fallback: number): number {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function sourceKindLabel(sourceKind: string, t: (value: string) => string): string {
  if (sourceKind === "openai_account") return t("账号池");
  if (sourceKind === "aggregate_api") return t("聚合 API");
  return sourceKind || t("未知来源");
}

function compactSourceId(value: string, maxLength = 28): string {
  const normalized = value.trim();
  if (normalized.length <= maxLength) return normalized;
  const headLength = Math.max(8, Math.floor((maxLength - 3) * 0.55));
  const tailLength = Math.max(6, maxLength - headLength - 3);
  return `${normalized.slice(0, headLength)}...${normalized.slice(-tailLength)}`;
}

function MiniStatBadge({
  label,
  value,
}: {
  label: string;
  value: string;
}) {
  return (
    <div className="inline-flex items-center gap-2 rounded-full border border-border/60 bg-background/45 px-3 py-1.5 text-xs text-muted-foreground">
      <span>{label}</span>
      <span className="font-semibold text-foreground">{value}</span>
    </div>
  );
}

export default function ModelsPage() {
  const { t } = useI18n();
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const isAdminMode = isAdminRole(role);
  const {
    models,
    isLoading,
    isServiceReady,
    refreshRemote,
    pruneStaleRemoteModels,
    saveModel,
    saveModelPriceRule,
    readModelPriceRule,
    deleteModel,
    deleteModels,
    exportCodexCache,
    routing,
    canExportCodexCache,
    isRefreshing,
    isPruningStaleRemote,
    isSaving,
    isDeleting,
    isExporting,
    isRoutingSaving,
    syncSourceModels,
    saveSourceModel,
    saveSourceMapping,
    deleteSourceMapping,
  } = useManagedModels();
  const isPageActive = useDesktopPageActive("/models/");
  usePageTransitionReady("/models/", !isServiceReady || !isLoading);
  const canLoadAdminRoutingSources =
    isServiceReady && isPageActive && isAdminMode && !isSessionLoading;

  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<ModelFilter>("all");
  const [modalOpen, setModalOpen] = useState(false);
  const [editingSlug, setEditingSlug] = useState<string | null>(null);
  const [selectedSlugs, setSelectedSlugs] = useState<string[]>([]);
  const [deleteSlugs, setDeleteSlugs] = useState<string[]>([]);
  const [editingPriceRule, setEditingPriceRule] = useState<ModelPriceRuleEntry | null>(null);
  const [activeModelSlug, setActiveModelSlug] = useState<string>("");
  const [routingDialogOpen, setRoutingDialogOpen] = useState(false);
  const [sourceDraft, setSourceDraft] = useState({
    sourceKind: "aggregate_api" as RoutingSourceKind,
    sourceId: "",
    upstreamModel: "",
    displayName: "",
  });
  const [mappingDraft, setMappingDraft] = useState({
    candidateKey: "",
    priority: "0",
    weight: "1",
    billingModelSlug: "",
  });
  const [candidateSearch, setCandidateSearch] = useState("");
  const [candidateKindFilter, setCandidateKindFilter] = useState<
    "all" | RoutingSourceKind
  >("all");

  const { data: accountList } = useQuery({
    queryKey: ["accounts", "model-routing-sources"],
    queryFn: () => accountClient.list(),
    enabled: canLoadAdminRoutingSources,
    retry: 1,
  });

  const { data: aggregateApis } = useQuery({
    queryKey: ["aggregate-apis", "model-routing-sources"],
    queryFn: () => accountClient.listAggregateApis(),
    enabled: canLoadAdminRoutingSources,
    retry: 1,
  });

  useEffect(() => {
    if (isPageActive) return;
    const frameId = window.requestAnimationFrame(() => {
      setModalOpen(false);
      setEditingSlug(null);
      setSelectedSlugs([]);
      setDeleteSlugs([]);
      setActiveModelSlug("");
      setRoutingDialogOpen(false);
      setCandidateSearch("");
      setCandidateKindFilter("all");
    });
    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [isPageActive]);

  useEffect(() => {
    const frameId = window.requestAnimationFrame(() => {
      const availableSlugs = new Set(models.map((item) => item.slug));
      setSelectedSlugs((current) =>
        current.filter((slug) => availableSlugs.has(slug))
      );
      setDeleteSlugs((current) =>
        current.filter((slug) => availableSlugs.has(slug))
      );
    });
    return () => {
      window.cancelAnimationFrame(frameId);
    };
  }, [models]);

  useEffect(() => {
    const activeModelRemoved =
      activeModelSlug && !models.some((model) => model.slug === activeModelSlug);
    if (models.length === 0 || activeModelRemoved) {
      const frameId = window.requestAnimationFrame(() => {
        setActiveModelSlug("");
        setRoutingDialogOpen(false);
      });
      return () => {
        window.cancelAnimationFrame(frameId);
      };
    }
    return undefined;
  }, [activeModelSlug, models]);

  const editingModel = useMemo(
    () => findBestMatchingModel(models, editingSlug || ""),
    [editingSlug, models]
  );

  useEffect(() => {
    let cancelled = false;
    const slug = editingModel?.slug;
    if (!slug) {
      setEditingPriceRule(null);
      return;
    }
    setEditingPriceRule(null);
    readModelPriceRule(slug)
      .then((result) => {
        if (!cancelled) setEditingPriceRule(result);
      })
      .catch((err) => {
        console.warn("读取模型价格失败", err);
        if (!cancelled) setEditingPriceRule(null);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editingModel]);

  const nextSortIndex = useMemo(
    () => models.reduce((maxValue, item) => Math.max(maxValue, item.sortIndex), -1) + 1,
    [models]
  );

  const stats = useMemo(
    () => ({
      total: models.length,
      apiEnabled: models.filter((item) => item.supportedInApi).length,
      routable: models.filter((item) =>
        routing.mappings.some(
          (mapping) => mapping.platformModelSlug === item.slug && mapping.enabled
        )
      ).length,
      custom: models.filter((item) => item.sourceKind === "custom").length,
      edited: models.filter((item) => item.userEdited).length,
    }),
    [models, routing.mappings]
  );

  const filteredModels = useMemo(() => {
    const keyword = search.trim().toLowerCase();
    return models.filter((model) => {
      const matchesKeyword =
        !keyword ||
        model.slug.toLowerCase().includes(keyword) ||
        model.displayName.toLowerCase().includes(keyword) ||
        String(model.description || "").toLowerCase().includes(keyword);
      if (!matchesKeyword) return false;

      switch (filter) {
        case "api":
          return model.supportedInApi;
        case "custom":
          return model.sourceKind === "custom";
        case "edited":
          return model.userEdited;
        default:
          return true;
      }
    });
  }, [filter, models, search]);

  const visibleSelectedSlugs = useMemo(
    () =>
      filteredModels
        .map((model) => model.slug)
        .filter((slug) => selectedSlugs.includes(slug)),
    [filteredModels, selectedSlugs]
  );

  const currentFilterLabel = useMemo(() => {
    switch (filter) {
      case "api":
        return t("仅 API 可用");
      case "custom":
        return t("仅自定义");
      case "edited":
        return t("仅本地覆写");
      default:
        return t("全部模型");
    }
  }, [filter, t]);

  const sourceNameByKey = useMemo(() => {
    const names = new Map<string, string>();
    for (const account of accountList?.items ?? []) {
      names.set(
        `openai_account:${account.id}`,
        account.label || account.name || account.id
      );
    }
    for (const api of aggregateApis ?? []) {
      names.set(
        `aggregate_api:${api.id}`,
        api.supplierName || api.url || api.id
      );
    }
    return names;
  }, [accountList?.items, aggregateApis]);

  const sourceModelByKey = useMemo(() => {
    return new Map(
      routing.sourceModels.map((item) => [sourceCandidateKey(item), item])
    );
  }, [routing.sourceModels]);

  const mappingsByModel = useMemo(() => {
    const result = new Map<string, ManagedModelSourceMapping[]>();
    for (const mapping of routing.mappings) {
      const current = result.get(mapping.platformModelSlug) ?? [];
      current.push(mapping);
      result.set(mapping.platformModelSlug, current);
    }
    for (const mappings of result.values()) {
      mappings.sort((left, right) => {
        if (left.enabled !== right.enabled) return left.enabled ? -1 : 1;
        if (left.priority !== right.priority) return left.priority - right.priority;
        return left.upstreamModel.localeCompare(right.upstreamModel);
      });
    }
    return result;
  }, [routing.mappings]);

  const routingStatsByModel = useMemo(() => {
    const statsBySlug = new Map<
      string,
      {
        total: number;
        enabled: number;
        sourceKinds: string[];
        lastSyncedAt: number | null;
      }
    >();
    for (const [slug, mappings] of mappingsByModel.entries()) {
      let lastSyncedAt: number | null = null;
      const sourceKinds = new Set<string>();
      for (const mapping of mappings) {
        sourceKinds.add(mapping.sourceKind);
        const sourceModel = sourceModelByKey.get(sourceCandidateKey(mapping));
        if (sourceModel?.lastSyncedAt != null) {
          lastSyncedAt = Math.max(lastSyncedAt ?? 0, sourceModel.lastSyncedAt);
        }
      }
      statsBySlug.set(slug, {
        total: mappings.length,
        enabled: mappings.filter((mapping) => mapping.enabled).length,
        sourceKinds: Array.from(sourceKinds),
        lastSyncedAt,
      });
    }
    return statsBySlug;
  }, [mappingsByModel, sourceModelByKey]);

  const activeModel = useMemo(
    () => findBestMatchingModel(models, activeModelSlug || ""),
    [activeModelSlug, models]
  );

  const activeMappings = useMemo(
    () => (activeModelSlug ? mappingsByModel.get(activeModelSlug) ?? [] : []),
    [activeModelSlug, mappingsByModel]
  );

  const activeMappingKeys = useMemo(
    () => new Set(activeMappings.map((mapping) => sourceCandidateKey(mapping))),
    [activeMappings]
  );

  const sourceCandidates = useMemo(() => {
    const keyword = candidateSearch.trim().toLowerCase();
    return routing.sourceModels
      .filter((sourceModel) => sourceModel.status === "available")
      .filter((sourceModel) => !activeMappingKeys.has(sourceCandidateKey(sourceModel)))
      .filter((sourceModel) =>
        candidateKindFilter === "all"
          ? true
          : sourceModel.sourceKind === candidateKindFilter
      )
      .filter((sourceModel) => {
        if (!keyword) return true;
        const sourceTitle = sourceNameByKey
          .get(`${sourceModel.sourceKind}:${sourceModel.sourceId}`)
          ?.toLowerCase();
        return (
          sourceModel.upstreamModel.toLowerCase().includes(keyword) ||
          sourceModel.sourceId.toLowerCase().includes(keyword) ||
          Boolean(sourceTitle?.includes(keyword))
        );
      })
      .sort((left, right) => {
        const leftExact = left.upstreamModel === activeModelSlug ? 0 : 1;
        const rightExact = right.upstreamModel === activeModelSlug ? 0 : 1;
        if (leftExact !== rightExact) return leftExact - rightExact;
        if (left.sourceKind !== right.sourceKind) {
          return left.sourceKind.localeCompare(right.sourceKind);
        }
        return left.upstreamModel.localeCompare(right.upstreamModel);
      });
  }, [
    activeMappingKeys,
    activeModelSlug,
    candidateKindFilter,
    candidateSearch,
    routing.sourceModels,
    sourceNameByKey,
  ]);

  const formatSourceTitle = (sourceKind: string, sourceId: string): string => {
    const label = sourceNameByKey.get(`${sourceKind}:${sourceId}`);
    return label || compactSourceId(sourceId) || "--";
  };

  const formatSourceDetail = (sourceKind: string, sourceId: string): string => {
    const label = sourceNameByKey.get(`${sourceKind}:${sourceId}`);
    const sourceLabel = sourceKindLabel(sourceKind, t);
    if (!sourceId) return sourceLabel;
    if (!label || label === sourceId) {
      return `${sourceLabel} ID ${compactSourceId(sourceId)}`;
    }
    return `${sourceLabel} ID ${compactSourceId(sourceId)}`;
  };

  const linkSourceModelToActiveModel = (sourceModel: ManagedModelSourceModel) => {
    if (!activeModelSlug) return;
    void saveSourceMapping({
      platformModelSlug: activeModelSlug,
      sourceKind: sourceModel.sourceKind,
      sourceId: sourceModel.sourceId,
      upstreamModel: sourceModel.upstreamModel,
      enabled: true,
      priority: parseInteger(mappingDraft.priority, 0),
      weight: parseInteger(mappingDraft.weight, 1),
      billingModelSlug: mappingDraft.billingModelSlug.trim() || null,
    }).then((result) => {
      if (result) {
        setMappingDraft((current) => ({
          ...current,
          candidateKey: "",
          billingModelSlug: "",
        }));
      }
    });
  };

  const saveManualSourceAndLink = () => {
    if (!activeModelSlug) return;
    const sourceId = sourceDraft.sourceId.trim();
    const upstreamModel = sourceDraft.upstreamModel.trim();
    if (!sourceId || !upstreamModel) return;
    void saveSourceModel({
      sourceKind: sourceDraft.sourceKind,
      sourceId,
      upstreamModel,
      displayName: sourceDraft.displayName.trim() || null,
    }).then((result) => {
      if (!result) return;
      void saveSourceMapping({
        platformModelSlug: activeModelSlug,
        sourceKind: sourceDraft.sourceKind,
        sourceId,
        upstreamModel,
        enabled: true,
        priority: parseInteger(mappingDraft.priority, 0),
        weight: parseInteger(mappingDraft.weight, 1),
        billingModelSlug: mappingDraft.billingModelSlug.trim() || null,
      }).then((mappingResult) => {
        if (mappingResult) {
          setSourceDraft((current) => ({
            ...current,
            upstreamModel: "",
            displayName: "",
          }));
          setMappingDraft((current) => ({
            ...current,
            billingModelSlug: "",
          }));
        }
      });
    });
  };

  const allVisibleSelected =
    filteredModels.length > 0 && visibleSelectedSlugs.length === filteredModels.length;
  const deleteTargetCount = deleteSlugs.length;
  const singleDeleteSlug = deleteTargetCount === 1 ? deleteSlugs[0] : null;

  const toggleSelectSlug = (slug: string) => {
    setSelectedSlugs((current) =>
      current.includes(slug)
        ? current.filter((item) => item !== slug)
        : [...current, slug]
    );
  };

  const toggleSelectAllVisible = () => {
    const visibleSlugs = filteredModels.map((model) => model.slug);
    setSelectedSlugs((current) => {
      if (visibleSlugs.length > 0 && visibleSlugs.every((slug) => current.includes(slug))) {
        return current.filter((slug) => !visibleSlugs.includes(slug));
      }
      return Array.from(new Set([...current, ...visibleSlugs]));
    });
  };

  const openSingleDeleteDialog = (slug: string) => {
    setDeleteSlugs([slug]);
  };

  const openBatchDeleteDialog = () => {
    setDeleteSlugs(selectedSlugs);
  };

  const openRoutingDialog = (slug: string) => {
    setActiveModelSlug(slug);
    setMappingDraft({
      candidateKey: "",
      priority: "0",
      weight: "1",
      billingModelSlug: "",
    });
    setCandidateSearch("");
    setCandidateKindFilter("all");
    setRoutingDialogOpen(true);
  };

  return (
    <>
      <div className="space-y-3 animate-in fade-in duration-500">
        <div className="space-y-2">
          <div className="space-y-2">
            <Badge className="w-fit rounded-full bg-primary/10 px-3 py-1 text-primary">
              {isAdminMode ? t("模型目录") : t("可用模型")}
            </Badge>
            <div className="space-y-1">
              <h1 className="text-3xl font-semibold tracking-tight">
                {isAdminMode ? t("模型管理") : t("可用模型")}
              </h1>
              <p className="max-w-4xl text-sm leading-6 text-muted-foreground">
                {isAdminMode
                  ? t("这里维护本地结构化模型目录。默认绑定模型会优先展示 supportedInApi=true 的模型，而 Codex CLI 仍会拿到完整目录。")
                  : t("查看当前账号可调用的平台模型。成员界面只展示平台模型名，不展示真实上游模型或来源配置。")}
              </p>
            </div>
            <div className="flex flex-wrap gap-2 text-xs">
              {isAdminMode ? (
                <>
                  <Badge variant="secondary" className="rounded-full px-3 py-1">
                    {t("完整目录会同步到 Codex CLI")}
                  </Badge>
                  <Badge variant="secondary" className="rounded-full px-3 py-1">
                    {t("默认绑定优先展示 API 可用模型")}
                  </Badge>
                  <Badge variant="secondary" className="rounded-full px-3 py-1">
                    {t("远端刷新可与本地覆写共存")}
                  </Badge>
                </>
              ) : (
                <>
                  <Badge variant="secondary" className="rounded-full px-3 py-1">
                    {t("仅展示平台模型")}
                  </Badge>
                  <Badge variant="secondary" className="rounded-full px-3 py-1">
                    {t("隐藏真实上游")}
                  </Badge>
                </>
              )}
            </div>
          </div>
        </div>

        <Card className="glass-card shadow-md ">
          <CardHeader className="pb-3">
            <div className="flex flex-col gap-3">
              <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                <div>
                  <CardTitle>{isAdminMode ? t("模型目录明细") : t("可用模型列表")}</CardTitle>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {isAdminMode
                      ? t("按 slug、显示名称或描述快速定位，并结合来源与覆写状态查看当前目录。")
                      : t("按 slug、显示名称或描述快速定位，只展示当前可见的平台模型。")}
                  </p>
                </div>
                {isAdminMode ? (
                  <div className="flex flex-wrap gap-2 lg:justify-end">
                    <Button
                      variant="outline"
                      onClick={() => void refreshRemote()}
                      disabled={isRefreshing || isPruningStaleRemote}
                    >
                      <RefreshCw
                        className={`mr-2 h-4 w-4 ${isRefreshing ? "animate-spin" : ""}`}
                      />
                      {t("远端并入")}
                    </Button>
                    <Button
                      variant="outline"
                      className="border-destructive/40 text-destructive hover:bg-destructive/10 hover:text-destructive"
                      onClick={() => void pruneStaleRemoteModels()}
                      disabled={isRefreshing || isPruningStaleRemote}
                      title={t("仅删除未本地覆写且不再出现在远端目录中的远端模型，不会删除自定义模型。")}
                    >
                      <Trash2
                        className={`mr-2 h-4 w-4 ${isPruningStaleRemote ? "animate-pulse" : ""}`}
                      />
                      {t("清理远端旧模型")}
                    </Button>
                    {canExportCodexCache ? (
                      <Button
                        variant="outline"
                        onClick={() => void exportCodexCache()}
                        disabled={isExporting}
                      >
                        <Download
                          className={`mr-2 h-4 w-4 ${isExporting ? "animate-spin" : ""}`}
                        />
                        {t("导出到本地 Codex 缓存")}
                      </Button>
                    ) : null}
                    <Button
                      variant="outline"
                      onClick={openBatchDeleteDialog}
                      disabled={selectedSlugs.length === 0 || isDeleting}
                    >
                      <Trash2 className="mr-2 h-4 w-4" />
                      {t("批量删除模型")}
                    </Button>
                    <Button
                      onClick={() => {
                        setEditingSlug(null);
                        setModalOpen(true);
                      }}
                    >
                      <Plus className="mr-2 h-4 w-4" />
                      {t("新增自定义模型")}
                    </Button>
                  </div>
                ) : null}
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <MiniStatBadge label={t("模型总数")} value={`${stats.total}`} />
                <MiniStatBadge label={t("API 可用")} value={`${stats.apiEnabled}`} />
                <MiniStatBadge
                  label={isAdminMode ? t("可调用映射") : t("可调用")}
                  value={`${stats.routable}`}
                />
                {isAdminMode ? (
                  <>
                    <MiniStatBadge label={t("自定义模型")} value={`${stats.custom}`} />
                    <MiniStatBadge label={t("本地覆写")} value={`${stats.edited}`} />
                  </>
                ) : null}
                <Badge variant="secondary" className="rounded-full px-3 py-1">
                  {t("当前筛选")} {currentFilterLabel}
                </Badge>
                <Badge variant="secondary" className="rounded-full px-3 py-1">
                  {t("共 {count} 条", { count: filteredModels.length })}
                </Badge>
                {isAdminMode && selectedSlugs.length > 0 ? (
                  <Badge variant="secondary" className="rounded-full px-3 py-1">
                    {t("已选 {count} 项", { count: selectedSlugs.length })}
                  </Badge>
                ) : null}
              </div>
              <div className="grid gap-3 md:grid-cols-2">
                <div className="flex h-10 items-center gap-2 rounded-xl border border-border/60 bg-background/35 px-3">
                  <Search className="h-4 w-4 text-muted-foreground" />
                  <Input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t("搜索 slug、显示名称或描述")}
                    className="h-full bg-transparent px-0 shadow-none focus-visible:ring-0"
                  />
                </div>
                <Select value={filter} onValueChange={(value) => setFilter(value as ModelFilter)}>
                  <SelectTrigger className="h-10 w-full rounded-xl px-3">
                    <SelectValue>{currentFilterLabel}</SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="all">{t("全部模型")}</SelectItem>
                    <SelectItem value="api">{t("仅 API 可用")}</SelectItem>
                    {isAdminMode ? (
                      <>
                        <SelectItem value="custom">{t("仅自定义")}</SelectItem>
                        <SelectItem value="edited">{t("仅本地覆写")}</SelectItem>
                      </>
                    ) : null}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              <div className="text-xs text-muted-foreground">
                {isAdminMode
                  ? t("保存后会自动同步到 `~/.codex/models_cache.json`；如需让 `/model` 立即看到最新模型与说明，仍需重启正在运行中的 Codex 会话。Web 端可通过上方导出按钮下载同名 `models_cache.json`，再手动放入本地 `~/.codex/`；桌面端继续由本地自动同步。")
                  : t("下游请求请使用平台模型 slug；实际来源和真实 upstream model 不在成员界面展示。")}
              </div>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            {!isServiceReady ? (
              <Empty className="min-h-40 border bg-background/35">
                <EmptyHeader>
                  <EmptyTitle>{t("服务未连接，当前无法读取模型目录。")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : isLoading ? (
              <div className="space-y-3">
                {Array.from({ length: 6 }).map((_, index) => (
                  <Skeleton key={`models-skeleton-${index}`} className="h-12 w-full rounded-xl" />
                ))}
              </div>
            ) : filteredModels.length === 0 ? (
              <Empty className="min-h-40 border bg-background/35">
                <EmptyHeader>
                  <EmptyTitle>
                    {isAdminMode
                      ? t("没有匹配的模型。你可以调整筛选条件，或直接新增一个自定义模型。")
                      : t("没有匹配的模型。你可以调整筛选条件。")}
                  </EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              <div className="overflow-x-auto">
                <Table>
                  <TableHeader>
                    <TableRow>
                      {isAdminMode ? (
                      <TableHead className="w-12 text-center">
                        <Checkbox
                          checked={allVisibleSelected}
                          onCheckedChange={toggleSelectAllVisible}
                        />
                      </TableHead>
                      ) : null}
                      <TableHead>{t("模型")}</TableHead>
                      {isAdminMode ? <TableHead>{t("来源")}</TableHead> : null}
                      <TableHead>{t("API")}</TableHead>
                      <TableHead>{isAdminMode ? t("调用状态") : t("状态")}</TableHead>
                      <TableHead>{t("可见性")}</TableHead>
                      <TableHead>{t("推理等级")}</TableHead>
                      <TableHead>{t("更新时间")}</TableHead>
                      {isAdminMode ? (
                      <TableHead className="table-sticky-action-head w-[120px] text-right">
                        {t("操作")}
                      </TableHead>
                      ) : null}
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {filteredModels.map((model) => {
                      const routingStats = routingStatsByModel.get(model.slug) ?? {
                        total: 0,
                        enabled: 0,
                        sourceKinds: [],
                        lastSyncedAt: null,
                      };
                      return (
                      <TableRow key={model.slug}>
                        {isAdminMode ? (
                        <TableCell className="text-center">
                          <Checkbox
                            checked={selectedSlugs.includes(model.slug)}
                            onCheckedChange={() => toggleSelectSlug(model.slug)}
                          />
                        </TableCell>
                        ) : null}
                        <TableCell className="min-w-[280px]">
                          <div className="space-y-1">
                            <div className="flex items-center gap-2">
                              <span className="font-medium">{model.displayName || model.slug}</span>
                              <Badge variant="secondary" className="font-mono text-[11px]">
                                {model.slug}
                              </Badge>
                            </div>
                            <p className="text-xs text-muted-foreground">
                              {model.description || t("未填写描述")}
                            </p>
                          </div>
                        </TableCell>
                        {isAdminMode ? (
                          <TableCell>
                            <div className="flex flex-wrap gap-2">
                              <Badge
                                variant={model.sourceKind === "custom" ? "default" : "secondary"}
                              >
                                {model.sourceKind === "custom" ? t("自定义") : t("远端")}
                              </Badge>
                              {model.userEdited ? (
                                <Badge className="bg-primary/10 text-primary">{t("已覆写")}</Badge>
                              ) : null}
                            </div>
                          </TableCell>
                        ) : null}
                        <TableCell>
                          {model.supportedInApi ? (
                            <Badge className="bg-emerald-500/10 text-emerald-600">
                              {t("可用")}
                            </Badge>
                          ) : (
                            <Badge variant="outline">{t("隐藏")}</Badge>
                          )}
                        </TableCell>
                        <TableCell className="min-w-[170px]">
                          {isAdminMode ? (
                            <div className="space-y-1">
                              <div className="flex flex-wrap gap-1">
                                {routingStats.enabled > 0 ? (
                                  <Badge className="bg-emerald-500/10 text-emerald-600">
                                    {t("已启用 {count} 条", { count: routingStats.enabled })}
                                  </Badge>
                                ) : (
                                  <Badge variant="outline">{t("暂不可调用")}</Badge>
                                )}
                                {routingStats.total > routingStats.enabled ? (
                                  <Badge variant="secondary">
                                    {t("候选 {count} 条", {
                                      count: routingStats.total - routingStats.enabled,
                                    })}
                                  </Badge>
                                ) : null}
                              </div>
                              <div className="text-[11px] text-muted-foreground">
                                {routingStats.sourceKinds.length > 0
                                  ? routingStats.sourceKinds
                                      .map((kind) => sourceKindLabel(kind, t))
                                      .join(" / ")
                                  : t("暂无启用来源")}
                              </div>
                            </div>
                          ) : routingStats.enabled > 0 ? (
                            <Badge className="bg-emerald-500/10 text-emerald-600">
                              {t("可调用")}
                            </Badge>
                          ) : (
                            <Badge variant="outline">{t("暂不可调用")}</Badge>
                          )}
                        </TableCell>
                        <TableCell>
                          {model.visibility === "list" ? (
                            <Badge className="bg-primary/10 text-primary">list</Badge>
                          ) : model.visibility === "hide" ? (
                            <Badge variant="outline">hide</Badge>
                          ) : (
                            <Badge variant="secondary">{t("未设置")}</Badge>
                          )}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {model.supportedReasoningLevels.length > 0
                            ? model.supportedReasoningLevels.map((item) => item.effort).join(" / ")
                            : model.defaultReasoningLevel || t("未配置")}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground">
                          {formatTsFromSeconds(model.updatedAt, t("未同步"))}
                        </TableCell>
                        {isAdminMode ? (
                        <TableCell className="table-sticky-action-cell text-right">
                          <div className="flex justify-end gap-1">
                            <Button
                              variant="ghost"
                              size="icon"
                              aria-label={t("关联来源")}
                              title={t("关联来源")}
                              onClick={() => openRoutingDialog(model.slug)}
                            >
                              <Link2 className="h-4 w-4" />
                            </Button>
                            <DropdownMenu>
                              <DropdownMenuTrigger render={<span />} nativeButton={false}>
                                <Button variant="ghost" size="icon" aria-label={t("模型操作")}>
                                  <MoreVertical className="h-4 w-4" />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                  <DropdownMenuGroup>
                                <DropdownMenuItem onClick={() => openRoutingDialog(model.slug)}>
                                  <Link2 className="h-4 w-4" />
                                  {t("关联来源")}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  onClick={() => {
                                    setEditingSlug(model.slug);
                                    setModalOpen(true);
                                  }}
                                >
                                  <PencilLine className="h-4 w-4" />
                                  {t("编辑模型")}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  variant="destructive"
                                  onClick={() => openSingleDeleteDialog(model.slug)}
                                >
                                  <Trash2 className="h-4 w-4" />
                                  {t("删除模型")}
                                </DropdownMenuItem>
                                </DropdownMenuGroup>
                              </DropdownMenuContent>
                            </DropdownMenu>
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
      </div>

      {isAdminMode ? (
        <Dialog
          open={routingDialogOpen}
          onOpenChange={(open) => {
            setRoutingDialogOpen(open);
            if (!open) {
              setActiveModelSlug("");
            }
          }}
        >
          <DialogContent className="glass-card max-h-[calc(100vh-2rem)] overflow-y-auto p-0 shadow-sm  md:max-w-[980px] xl:max-w-[1180px]">
            <div className="p-5 sm:p-6">
          <DialogHeader className="pb-3 pr-8">
            <div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <div>
                <DialogTitle>{t("关联来源")}</DialogTitle>
                <DialogDescription className="mt-1">
                  {t("下游请求只使用平台模型名；这里为当前模型选择账号池或聚合 API 的真实模型。")}
                </DialogDescription>
              </div>
              {activeModel ? (
                <Badge variant="secondary" className="w-fit rounded-full px-3 py-1 font-mono">
                  {activeModel.slug}
                </Badge>
              ) : null}
            </div>
          </DialogHeader>
          <div className="space-y-4">
            {!isServiceReady ? (
              <Empty className="min-h-40 border bg-background/35">
                <EmptyHeader>
                  <EmptyTitle>{t("服务未连接，当前无法读取模型路由。")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : !activeModel ? (
              <Empty className="min-h-40 border bg-background/35">
                <EmptyHeader>
                  <EmptyTitle>{t("请先从模型目录选择一个模型。")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              <>
                <div className="flex flex-wrap items-center gap-2">
                  <Badge variant="secondary" className="rounded-full px-3 py-1 font-mono">
                    {t("对外模型")} · {activeModel.slug}
                  </Badge>
                  <Badge
                    className={
                      activeMappings.some((mapping) => mapping.enabled)
                        ? "rounded-full bg-emerald-500/10 px-3 py-1 text-emerald-600"
                        : "rounded-full bg-amber-500/10 px-3 py-1 text-amber-700"
                    }
                  >
                    {activeMappings.some((mapping) => mapping.enabled)
                      ? t("可调用")
                      : t("暂不可调用")}
                  </Badge>
                  <Badge variant="secondary" className="rounded-full px-3 py-1">
                    {t("已配置路由 {count} 条", { count: activeMappings.length })}
                  </Badge>
                </div>

                <div className="grid gap-2 md:grid-cols-3">
                  <div className="rounded-xl border border-border/60 bg-background/35 px-4 py-3">
                    <div className="text-[11px] font-medium text-muted-foreground">
                      {t("对外模型名")}
                    </div>
                    <div className="mt-1 truncate font-mono text-sm">{activeModel.slug}</div>
                  </div>
                  <div className="rounded-xl border border-border/60 bg-background/35 px-4 py-3">
                    <div className="text-[11px] font-medium text-muted-foreground">
                      {t("可关联真实模型")}
                    </div>
                    <div className="mt-1 text-sm">
                      {t("{count} 个可选", { count: routing.sourceModels.length })}
                    </div>
                  </div>
                  <div className="rounded-xl border border-border/60 bg-background/35 px-4 py-3">
                    <div className="text-[11px] font-medium text-muted-foreground">
                      {t("已启用来源")}
                    </div>
                    <div className="mt-1 text-sm">
                      {t("{count} 条", {
                        count: activeMappings.filter((mapping) => mapping.enabled).length,
                      })}
                    </div>
                  </div>
                </div>

                <div className="grid gap-4 xl:grid-cols-[minmax(0,0.95fr)_minmax(420px,1.05fr)]">
                  <section className="rounded-xl border border-border/60 bg-background/30">
                    <div className="flex items-center justify-between gap-3 border-b border-border/60 px-4 py-3">
                      <div className="flex items-center gap-2 text-sm font-medium">
                        <Link2 className="h-4 w-4 text-primary" />
                        {t("已关联来源")}
                      </div>
                      <Badge variant="secondary" className="rounded-full">
                        {t("{count} 条", { count: activeMappings.length })}
                      </Badge>
                    </div>
                    {activeMappings.length === 0 ? (
                      <Empty className="min-h-32 border-0 bg-transparent">
                        <EmptyHeader>
                          <EmptyTitle>{t("还没有关联来源。")}</EmptyTitle>
                        </EmptyHeader>
                      </Empty>
                    ) : (
                      <div className="max-h-[520px] space-y-2 overflow-y-auto p-3">
                        {activeMappings.map((mapping) => (
                          <div
                            key={mapping.id}
                            className="rounded-lg border border-border/60 bg-background/40 p-3"
                          >
                            <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                              <div className="min-w-0 space-y-2">
                                <div className="flex flex-wrap items-center gap-2">
                                  <Badge variant="secondary">
                                    {sourceKindLabel(mapping.sourceKind, t)}
                                  </Badge>
                                  <Badge
                                    className={
                                      mapping.enabled
                                        ? "bg-emerald-500/10 text-emerald-600"
                                        : ""
                                    }
                                    variant={mapping.enabled ? "default" : "outline"}
                                  >
                                    {mapping.enabled ? t("启用") : t("禁用")}
                                  </Badge>
                                </div>
                                <div className="truncate text-sm font-medium">
                                  {formatSourceTitle(mapping.sourceKind, mapping.sourceId)}
                                </div>
                                <div
                                  className="truncate text-xs text-muted-foreground"
                                  title={mapping.sourceId}
                                >
                                  {formatSourceDetail(mapping.sourceKind, mapping.sourceId)}
                                </div>
                                <div className="break-all font-mono text-xs">
                                  {mapping.upstreamModel}
                                </div>
                              </div>
                              {isAdminMode ? (
                                <div className="flex shrink-0 gap-2">
                                  <Button
                                    variant="outline"
                                    size="sm"
                                    disabled={isRoutingSaving}
                                    onClick={() =>
                                      void saveSourceMapping({
                                        ...mapping,
                                        enabled: !mapping.enabled,
                                      })
                                    }
                                  >
                                    {mapping.enabled ? t("禁用") : t("启用")}
                                  </Button>
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    aria-label={t("删除映射")}
                                    disabled={isRoutingSaving}
                                    onClick={() =>
                                      void deleteSourceMapping(
                                        mapping.id,
                                        mapping.sourceKind,
                                        mapping.sourceId,
                                        mapping.upstreamModel,
                                      )
                                    }
                                  >
                                    <Unlink className="h-4 w-4" />
                                  </Button>
                                </div>
                              ) : null}
                            </div>
                            <div className="mt-3 flex flex-wrap gap-2 text-xs text-muted-foreground">
                              <span>
                                {t("优先级 {priority} · 权重 {weight}", {
                                  priority: mapping.priority,
                                  weight: mapping.weight,
                                })}
                              </span>
                              <span>{mapping.billingModelSlug || t("按平台模型")}</span>
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </section>

                  <section className="rounded-xl border border-border/60 bg-background/30 p-4">
                    <Tabs defaultValue="quick" className="gap-3">
                      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                        <div className="flex items-center gap-2 text-sm font-medium">
                          <Database className="h-4 w-4 text-primary" />
                          {t("添加来源")}
                        </div>
                        <TabsList className="grid w-full grid-cols-2 sm:w-[240px]">
                          <TabsTrigger value="quick">{t("快捷关联")}</TabsTrigger>
                          <TabsTrigger value="manual">{t("手动添加")}</TabsTrigger>
                        </TabsList>
                      </div>

                      <TabsContent value="quick" className="space-y-3">
                        <div className="flex flex-wrap gap-2">
                          <Button
                            variant="outline"
                            size="sm"
                            disabled={isRoutingSaving}
                            onClick={() =>
                              void syncSourceModels({ sourceKind: "openai_account" })
                            }
                          >
                            <RefreshCw className="mr-2 h-4 w-4" />
                            {t("同步账号池")}
                          </Button>
                          <Button
                            variant="outline"
                            size="sm"
                            disabled={isRoutingSaving}
                            onClick={() =>
                              void syncSourceModels({ sourceKind: "aggregate_api" })
                            }
                          >
                            <RefreshCw className="mr-2 h-4 w-4" />
                            {t("同步聚合 API")}
                          </Button>
                          <Badge variant="secondary" className="rounded-full px-3 py-1">
                            {t("{count} 个可选", { count: sourceCandidates.length })}
                          </Badge>
                        </div>

                        <div className="grid gap-2 sm:grid-cols-3">
                          <Input
                            value={mappingDraft.priority}
                            onChange={(event) =>
                              setMappingDraft((current) => ({
                                ...current,
                                priority: event.target.value,
                              }))
                            }
                            placeholder={t("优先级")}
                            className="h-9 rounded-lg"
                          />
                          <Input
                            value={mappingDraft.weight}
                            onChange={(event) =>
                              setMappingDraft((current) => ({
                                ...current,
                                weight: event.target.value,
                              }))
                            }
                            placeholder={t("权重")}
                            className="h-9 rounded-lg"
                          />
                          <Input
                            value={mappingDraft.billingModelSlug}
                            onChange={(event) =>
                              setMappingDraft((current) => ({
                                ...current,
                                billingModelSlug: event.target.value,
                              }))
                            }
                            placeholder={t("计费模型，可选")}
                            className="h-9 rounded-lg"
                          />
                        </div>

                        <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_160px]">
                          <div className="relative">
                            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                            <Input
                              value={candidateSearch}
                              onChange={(event) => setCandidateSearch(event.target.value)}
                              placeholder={t("搜索真实模型或来源")}
                              className="h-9 rounded-lg pl-9"
                            />
                          </div>
                          <Select
                            value={candidateKindFilter}
                            onValueChange={(value) =>
                              setCandidateKindFilter(
                                (value || "all") as "all" | RoutingSourceKind
                              )
                            }
                          >
                            <SelectTrigger className="h-9 rounded-lg">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                    <SelectGroup>
                              <SelectItem value="all">{t("全部来源")}</SelectItem>
                              <SelectItem value="openai_account">{t("账号池")}</SelectItem>
                              <SelectItem value="aggregate_api">{t("聚合 API")}</SelectItem>
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                        </div>

                        <div className="max-h-[380px] space-y-2 overflow-y-auto pr-1">
                          {sourceCandidates.length === 0 ? (
                            <Empty className="min-h-32 border bg-background/35">
                              <EmptyHeader>
                                <EmptyTitle>{t("暂无可关联候选。")}</EmptyTitle>
                              </EmptyHeader>
                            </Empty>
                          ) : (
                            sourceCandidates.slice(0, 80).map((sourceModel) => (
                              <div
                                key={sourceCandidateKey(sourceModel)}
                                className="rounded-lg border border-border/60 bg-background/40 p-3"
                              >
                                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                                  <div className="min-w-0 space-y-1">
                                    <div className="flex flex-wrap items-center gap-2">
                                      <Badge variant="secondary">
                                        {sourceKindLabel(sourceModel.sourceKind, t)}
                                      </Badge>
                                      {sourceModel.upstreamModel === activeModelSlug ? (
                                        <Badge className="bg-emerald-500/10 text-emerald-600">
                                          {t("同名")}
                                        </Badge>
                                      ) : null}
                                      <span className="break-all font-mono text-xs">
                                        {sourceModel.upstreamModel}
                                      </span>
                                    </div>
                                    <div className="text-xs text-muted-foreground">
                                      {formatSourceTitle(
                                        sourceModel.sourceKind,
                                        sourceModel.sourceId
                                      )}
                                      {" · "}
                                      {sourceModel.discoveryKind === "manual"
                                        ? t("手动")
                                        : t("同步")}
                                      {" · "}
                                      {formatTsFromSeconds(
                                        sourceModel.lastSyncedAt,
                                        t("未同步")
                                      )}
                                    </div>
                                    <div
                                      className="truncate text-[11px] text-muted-foreground/75"
                                      title={sourceModel.sourceId}
                                    >
                                      {formatSourceDetail(
                                        sourceModel.sourceKind,
                                        sourceModel.sourceId
                                      )}
                                    </div>
                                  </div>
                                  <Button
                                    size="sm"
                                    disabled={isRoutingSaving}
                                    onClick={() => linkSourceModelToActiveModel(sourceModel)}
                                  >
                                    <Link2 className="mr-2 h-4 w-4" />
                                    {t("关联")}
                                  </Button>
                                </div>
                              </div>
                            ))
                          )}
                        </div>
                      </TabsContent>

                      <TabsContent value="manual" className="space-y-3">
                        <div className="grid gap-2 sm:grid-cols-2">
                          <Select
                            value={sourceDraft.sourceKind}
                            onValueChange={(value) =>
                              setSourceDraft((current) => ({
                                ...current,
                                sourceKind: (value || "aggregate_api") as RoutingSourceKind,
                              }))
                            }
                          >
                            <SelectTrigger className="h-9 rounded-lg">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                    <SelectGroup>
                              <SelectItem value="aggregate_api">{t("聚合 API")}</SelectItem>
                              <SelectItem value="openai_account">{t("账号池")}</SelectItem>
                              </SelectGroup>
                            </SelectContent>
                          </Select>
                          <Input
                            value={sourceDraft.sourceId}
                            onChange={(event) =>
                              setSourceDraft((current) => ({
                                ...current,
                                sourceId: event.target.value,
                              }))
                            }
                            placeholder={t("来源 ID")}
                            className="h-9 rounded-lg"
                          />
                        </div>
                        <div className="grid gap-2 sm:grid-cols-2">
                          <Input
                            value={sourceDraft.upstreamModel}
                            onChange={(event) =>
                              setSourceDraft((current) => ({
                                ...current,
                                upstreamModel: event.target.value,
                              }))
                            }
                            placeholder={t("真实模型名")}
                            className="h-9 rounded-lg"
                          />
                          <Input
                            value={sourceDraft.displayName}
                            onChange={(event) =>
                              setSourceDraft((current) => ({
                                ...current,
                                displayName: event.target.value,
                              }))
                            }
                            placeholder={t("显示名称，可选")}
                            className="h-9 rounded-lg"
                          />
                        </div>
                        <div className="grid gap-2 sm:grid-cols-3">
                          <Input
                            value={mappingDraft.priority}
                            onChange={(event) =>
                              setMappingDraft((current) => ({
                                ...current,
                                priority: event.target.value,
                              }))
                            }
                            placeholder={t("优先级")}
                            className="h-9 rounded-lg"
                          />
                          <Input
                            value={mappingDraft.weight}
                            onChange={(event) =>
                              setMappingDraft((current) => ({
                                ...current,
                                weight: event.target.value,
                              }))
                            }
                            placeholder={t("权重")}
                            className="h-9 rounded-lg"
                          />
                          <Input
                            value={mappingDraft.billingModelSlug}
                            onChange={(event) =>
                              setMappingDraft((current) => ({
                                ...current,
                                billingModelSlug: event.target.value,
                              }))
                            }
                            placeholder={t("计费模型，可选")}
                            className="h-9 rounded-lg"
                          />
                        </div>
                        <Button
                          className="w-full sm:w-auto"
                          disabled={
                            isRoutingSaving ||
                            !sourceDraft.sourceId.trim() ||
                            !sourceDraft.upstreamModel.trim()
                          }
                          onClick={saveManualSourceAndLink}
                        >
                          <Plus className="mr-2 h-4 w-4" />
                          {t("保存并关联")}
                        </Button>
                      </TabsContent>
                    </Tabs>
                  </section>
                </div>
              </>
            )}
          </div>
            </div>
          </DialogContent>
        </Dialog>
      ) : null}

      {isAdminMode ? (
      <ModelCatalogModal
        open={modalOpen}
        onOpenChange={setModalOpen}
        model={editingModel}
        nextSortIndex={nextSortIndex}
        isSaving={isSaving}
        onSave={saveModel}
        onSavePriceRule={saveModelPriceRule}
        priceRule={editingPriceRule}
      />
      ) : null}

      {isAdminMode ? (
      <ConfirmDialog
        open={deleteTargetCount > 0}
        onOpenChange={(open) => {
          if (!open) {
            setDeleteSlugs([]);
          }
        }}
        title={deleteTargetCount > 1 ? t("批量删除模型") : t("删除模型")}
        description={
          deleteTargetCount > 1
            ? t(
                "确定要删除选中的 {count} 个模型吗？如果后续执行远端刷新，远端模型可能会再次并入本地目录。",
                { count: deleteTargetCount }
              )
            : singleDeleteSlug
              ? t("确定要删除模型 {slug} 吗？如果后续执行远端刷新，远端模型可能会再次并入本地目录。", {
                  slug: singleDeleteSlug,
                })
              : ""
        }
        confirmText={isDeleting ? t("删除中...") : t("删除")}
        confirmVariant="destructive"
        onConfirm={() => {
          if (singleDeleteSlug) {
            void deleteModel(singleDeleteSlug).then((ok) => {
              if (ok) {
                setSelectedSlugs((current) =>
                  current.filter((slug) => slug !== singleDeleteSlug)
                );
                setDeleteSlugs([]);
              }
            });
            return;
          }

          if (deleteTargetCount > 1) {
            void deleteModels(deleteSlugs).then((result) => {
              if (result.deleted.length > 0) {
                setSelectedSlugs((current) =>
                  current.filter((slug) => !result.deleted.includes(slug))
                );
              }
              setDeleteSlugs([]);
            });
          }
        }}
      />
      ) : null}
    </>
  );
}
