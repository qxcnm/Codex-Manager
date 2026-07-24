"use client";

import { useMemo, useState } from "react";
import { Plus, Trash2 } from "lucide-react";

import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import {
  microusdToUsdPerMillion,
  usdPerMillionToMicrousd,
} from "@/lib/api/managed-models-v2";
import { useI18n } from "@/lib/i18n/provider";
import type { AggregateApi } from "@/types/api-key";
import type {
  ManagedModelV2,
  ManagedModelV2Upsert,
  ModelInstructionsModeV2,
  ModelPriceTierV2,
  ModelRouteSourceKindV2,
  ModelRouteV2,
  ModelVisibilityV2,
} from "@/types/model-v2";

type RouteDraft = {
  key: string;
  id: string;
  sourceKind: ModelRouteSourceKindV2;
  sourceId: string;
  upstreamModel: string;
  enabled: boolean;
  priority: string;
  weight: string;
};

type ModelDraft = {
  slug: string;
  displayName: string;
  description: string;
  provider: string;
  family: string;
  category: string;
  tags: string;
  enabled: boolean;
  supportedInApi: boolean;
  visibility: ModelVisibilityV2;
  sortOrder: string;
  contextWindow: string;
  maxContextWindow: string;
  defaultReasoningEffort: string;
  capabilitiesJson: string;
  inputPrice: string;
  cachedInputPrice: string;
  outputPrice: string;
  longContextThreshold: string;
  longInputPrice: string;
  longCachedInputPrice: string;
  longOutputPrice: string;
  routes: RouteDraft[];
  instructionsMode: ModelInstructionsModeV2;
  instructionsText: string;
};

interface ModelCatalogModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  model?: ManagedModelV2 | null;
  nextSortOrder: number;
  aggregateApis?: AggregateApi[];
  isSaving?: boolean;
  onSave: (input: ManagedModelV2Upsert) => Promise<ManagedModelV2 | null>;
}

const DEFAULT_CAPABILITIES = {
  reasoningEfforts: [],
  serviceTiers: [],
  inputModalities: ["text", "image"],
  supportsParallelToolCalls: false,
  supportsReasoningSummaries: false,
  defaultReasoningSummary: "auto",
  supportsVerbosity: false,
  truncationMode: "tokens",
  truncationLimit: 10000,
  experimentalSupportedTools: [],
};

function aggregateApiDisplayName(
  api: AggregateApi | undefined,
  t: (message: string) => string,
): string {
  return api?.supplierName?.trim() || t("未命名聚合 API");
}

function aggregateRouteSourceLabel(
  sourceId: string,
  aggregateApis: AggregateApi[],
  t: (message: string) => string,
): string {
  const normalizedId = sourceId.trim();
  if (!normalizedId) return t("选择聚合 API");
  const api = aggregateApis.find((item) => item.id === normalizedId);
  return `${t("聚合 API")}：${
    api ? aggregateApiDisplayName(api, t) : t("未知聚合 API")
  }`;
}

function routeDraft(route: ModelRouteV2, index: number): RouteDraft {
  return {
    key: route.id || `route-${index}-${route.sourceKind}-${route.sourceId}`,
    id: route.id,
    sourceKind: route.sourceKind,
    sourceId: route.sourceId,
    upstreamModel: route.upstreamModel,
    enabled: route.enabled,
    priority: String(route.priority),
    weight: String(route.weight),
  };
}

function buildDraft(model: ManagedModelV2 | null | undefined, nextSortOrder: number): ModelDraft {
  const longTier = model?.priceTiers.find((tier) => tier.minInputTokens > 0);
  return {
    slug: model?.slug || "",
    displayName: model?.displayName || "",
    description: model?.description || "",
    provider: model?.provider || "",
    family: model?.family || "",
    category: model?.category || "",
    tags: model?.tags.join(", ") || "",
    enabled: model?.enabled ?? true,
    supportedInApi: model?.supportedInApi ?? true,
    visibility: model?.visibility || "list",
    sortOrder: String(model?.sortOrder ?? nextSortOrder),
    contextWindow: model?.contextWindow == null ? "" : String(model.contextWindow),
    maxContextWindow:
      model?.maxContextWindow == null ? "" : String(model.maxContextWindow),
    defaultReasoningEffort: model?.defaultReasoningEffort || "",
    capabilitiesJson: JSON.stringify(model?.capabilities || DEFAULT_CAPABILITIES, null, 2),
    inputPrice: microusdToUsdPerMillion(model?.price.inputMicrousdPer1m ?? null),
    cachedInputPrice: microusdToUsdPerMillion(
      model?.price.cachedInputMicrousdPer1m ?? null,
    ),
    outputPrice: microusdToUsdPerMillion(model?.price.outputMicrousdPer1m ?? null),
    longContextThreshold: longTier ? String(longTier.minInputTokens) : "",
    longInputPrice: microusdToUsdPerMillion(longTier?.inputMicrousdPer1m ?? null),
    longCachedInputPrice: microusdToUsdPerMillion(
      longTier?.cachedInputMicrousdPer1m ?? null,
    ),
    longOutputPrice: microusdToUsdPerMillion(longTier?.outputMicrousdPer1m ?? null),
    routes:
      model?.routes.map(routeDraft) ||
      [
        routeDraft(
          {
            id: "",
            sourceKind: "account_pool",
            sourceId: "default",
            upstreamModel: "",
            enabled: true,
            priority: 0,
            weight: 1,
          },
          0,
        ),
      ],
    instructionsMode: model?.instructionsMode || "passthrough",
    instructionsText: model?.instructionsText || "",
  };
}

function optionalPositiveInteger(value: string, label: string): number | null {
  const normalized = value.trim();
  if (!normalized) return null;
  const parsed = Number(normalized);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${label} 必须是正整数`);
  }
  return parsed;
}

function integer(value: string, label: string, minimum?: number): number {
  const parsed = Number(value.trim());
  if (!Number.isSafeInteger(parsed) || (minimum != null && parsed < minimum)) {
    throw new Error(`${label} 必须是${minimum === 1 ? "正" : ""}整数`);
  }
  return parsed;
}

function parseCapabilities(value: string): Record<string, unknown> {
  const parsed = JSON.parse(value || "{}");
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("关键能力 JSON 必须是对象");
  }
  return parsed as Record<string, unknown>;
}

function buildPrice(
  draft: ModelDraft,
  model: ManagedModelV2 | null | undefined,
): Pick<ManagedModelV2, "price" | "priceTiers"> {
  const baseValues = [draft.inputPrice, draft.cachedInputPrice, draft.outputPrice];
  const hasBasePrice = baseValues.some((value) => value.trim() !== "");
  if (!hasBasePrice) {
    if (
      [
        draft.longContextThreshold,
        draft.longInputPrice,
        draft.longCachedInputPrice,
        draft.longOutputPrice,
      ].some((value) => value.trim() !== "")
    ) {
      throw new Error("配置长上下文价格前必须先填写基础三价");
    }
    return {
      price: {
        priceStatus: "missing",
        priceSource: null,
        inputMicrousdPer1m: null,
        cachedInputMicrousdPer1m: null,
        outputMicrousdPer1m: null,
      },
      priceTiers: [],
    };
  }
  if (baseValues.some((value) => value.trim() === "")) {
    throw new Error("输入、缓存输入和输出价格必须同时填写");
  }

  const baseTier: ModelPriceTierV2 = {
    minInputTokens: 0,
    inputMicrousdPer1m: usdPerMillionToMicrousd(draft.inputPrice),
    cachedInputMicrousdPer1m: usdPerMillionToMicrousd(draft.cachedInputPrice),
    outputMicrousdPer1m: usdPerMillionToMicrousd(draft.outputPrice),
  };
  const priceTiers = [baseTier];
  const longValues = [
    draft.longContextThreshold,
    draft.longInputPrice,
    draft.longCachedInputPrice,
    draft.longOutputPrice,
  ];
  if (longValues.some((value) => value.trim() !== "")) {
    if (longValues.some((value) => value.trim() === "")) {
      throw new Error("长上下文阈值和三项价格必须完整填写");
    }
    priceTiers.push({
      minInputTokens: integer(draft.longContextThreshold, "长上下文阈值", 1),
      inputMicrousdPer1m: usdPerMillionToMicrousd(draft.longInputPrice),
      cachedInputMicrousdPer1m: usdPerMillionToMicrousd(
        draft.longCachedInputPrice,
      ),
      outputMicrousdPer1m: usdPerMillionToMicrousd(draft.longOutputPrice),
    });
  }

  const unchanged =
    model != null &&
    model.price.inputMicrousdPer1m === baseTier.inputMicrousdPer1m &&
    model.price.cachedInputMicrousdPer1m === baseTier.cachedInputMicrousdPer1m &&
    model.price.outputMicrousdPer1m === baseTier.outputMicrousdPer1m &&
    JSON.stringify(model.priceTiers) === JSON.stringify(priceTiers);
  return {
    price: {
      priceStatus: unchanged ? model.price.priceStatus : "custom",
      priceSource: unchanged ? model.price.priceSource : "local-ui",
      inputMicrousdPer1m: baseTier.inputMicrousdPer1m,
      cachedInputMicrousdPer1m: baseTier.cachedInputMicrousdPer1m,
      outputMicrousdPer1m: baseTier.outputMicrousdPer1m,
    },
    priceTiers,
  };
}

export function ModelCatalogModal({
  open,
  onOpenChange,
  model,
  nextSortOrder,
  aggregateApis = [],
  isSaving = false,
  onSave,
}: ModelCatalogModalProps) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<ModelDraft>(() =>
    buildDraft(model, nextSortOrder),
  );
  const [error, setError] = useState<string | null>(null);

  const title = useMemo(
    () => (model ? t("编辑模型") : t("新增自定义模型")),
    [model, t],
  );

  const updateDraft = <K extends keyof ModelDraft>(key: K, value: ModelDraft[K]) => {
    setDraft((current) => ({ ...current, [key]: value }));
  };

  const updateRoute = <K extends keyof RouteDraft>(
    index: number,
    key: K,
    value: RouteDraft[K],
  ) => {
    setDraft((current) => ({
      ...current,
      routes: current.routes.map((route, routeIndex) =>
        routeIndex === index ? { ...route, [key]: value } : route,
      ),
    }));
  };

  const addRoute = (sourceKind: ModelRouteSourceKindV2) => {
    const sourceId =
      sourceKind === "account_pool" ? "default" : aggregateApis[0]?.id || "";
    setDraft((current) => ({
      ...current,
      routes: [
        ...current.routes,
        {
          key: `new-${Date.now()}-${current.routes.length}`,
          id: "",
          sourceKind,
          sourceId,
          upstreamModel: current.slug.trim(),
          enabled: true,
          priority: "0",
          weight: "1",
        },
      ],
    }));
  };

  const handleSave = async () => {
    try {
      setError(null);
      const slug = draft.slug.trim();
      if (!slug) throw new Error("模型 slug 不能为空");
      const price = buildPrice(draft, model);
      if (
        price.price.priceStatus === "missing" &&
        (model?.permissionGroupIds.length || 0) > 0
      ) {
        throw new Error("缺失价格的模型不能保留在计费权限组中");
      }
      const routes: ModelRouteV2[] = draft.routes.map((route) => {
        const sourceId =
          route.sourceKind === "account_pool" ? "default" : route.sourceId.trim();
        if (!sourceId || !route.upstreamModel.trim()) {
          throw new Error("每条路由都必须填写来源和上游模型");
        }
        return {
          id: route.id,
          sourceKind: route.sourceKind,
          sourceId,
          upstreamModel: route.upstreamModel.trim(),
          enabled: route.enabled,
          priority: integer(route.priority, "路由优先级"),
          weight: integer(route.weight, "路由权重", 1),
        };
      });
      if (
        draft.instructionsMode === "override" &&
        !draft.instructionsText.trim()
      ) {
        throw new Error("override 模式必须填写 instructions text");
      }

      const nextModel: ManagedModelV2 = {
        id: model?.id || "",
        slug,
        displayName: draft.displayName.trim() || slug,
        description: draft.description.trim() || null,
        provider: draft.provider.trim() || null,
        family: draft.family.trim() || null,
        category: draft.category.trim() || null,
        tags: draft.tags
          .split(",")
          .map((tag) => tag.trim())
          .filter(Boolean),
        origin: model?.origin || "custom",
        enabled: draft.enabled,
        supportedInApi: draft.supportedInApi,
        visibility: draft.visibility,
        sortOrder: integer(draft.sortOrder, "排序"),
        contextWindow: optionalPositiveInteger(draft.contextWindow, "上下文窗口"),
        maxContextWindow: optionalPositiveInteger(
          draft.maxContextWindow,
          "最大上下文窗口",
        ),
        defaultReasoningEffort: draft.defaultReasoningEffort.trim() || null,
        capabilities: parseCapabilities(draft.capabilitiesJson),
        instructionsMode: draft.instructionsMode,
        instructionsText: draft.instructionsText.trim() || null,
        builtinRevision: model?.builtinRevision || null,
        userEdited: model?.userEdited || false,
        ...price,
        routes,
        permissionGroupIds:
          price.price.priceStatus === "missing"
            ? []
            : model?.permissionGroupIds || [],
        createdAt: model?.createdAt || 0,
        updatedAt: model?.updatedAt || 0,
      };
      const saved = await onSave({
        previousSlug: model?.slug || null,
        model: nextModel,
      });
      if (saved) onOpenChange(false);
    } catch (saveError) {
      setError(saveError instanceof Error ? saveError.message : String(saveError));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card p-0 sm:max-w-[960px] xl:max-w-[1180px]">
        <div className="max-h-[84vh] overflow-y-auto p-5">
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription>
              {t("模型、价格、路由和指令策略将通过一次原子保存提交。")}
            </DialogDescription>
          </DialogHeader>

          <Tabs defaultValue="basic" className="mt-5">
            <TabsList className="grid w-full grid-cols-4">
              <TabsTrigger value="basic">{t("基本信息")}</TabsTrigger>
              <TabsTrigger value="price">{t("价格")}</TabsTrigger>
              <TabsTrigger value="routes">{t("路由")}</TabsTrigger>
              <TabsTrigger value="instructions">{t("指令策略")}</TabsTrigger>
            </TabsList>

            <TabsContent value="basic" className="mt-4 space-y-4">
              <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="model-slug">{t("模型标识（Slug）")}</Label>
                  <Input
                    id="model-slug"
                    value={draft.slug}
                    disabled={model?.origin === "builtin"}
                    onChange={(event) => {
                      const slug = event.target.value;
                      setDraft((current) => ({
                        ...current,
                        slug,
                        routes: current.routes.map((route) =>
                          route.upstreamModel.trim()
                            ? route
                            : { ...route, upstreamModel: slug },
                        ),
                      }));
                    }}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="model-display-name">{t("显示名称")}</Label>
                  <Input
                    id="model-display-name"
                    value={draft.displayName}
                    onChange={(event) => updateDraft("displayName", event.target.value)}
                  />
                </div>
                <div className="space-y-2 md:col-span-2">
                  <Label htmlFor="model-description">{t("描述")}</Label>
                  <Textarea
                    id="model-description"
                    rows={2}
                    value={draft.description}
                    onChange={(event) => updateDraft("description", event.target.value)}
                  />
                </div>
              </div>

              <div className="grid gap-4 md:grid-cols-3">
                {([
                  ["provider", "提供方"],
                  ["family", "模型系列"],
                  ["category", "模型分类"],
                ] as const).map(([field, label]) => (
                  <div key={field} className="space-y-2">
                    <Label htmlFor={`model-${field}`}>{t(label)}</Label>
                    <Input
                      id={`model-${field}`}
                      value={draft[field]}
                      onChange={(event) => updateDraft(field, event.target.value)}
                    />
                  </div>
                ))}
                <div className="space-y-2 md:col-span-3">
                  <Label htmlFor="model-tags">{t("标签")}</Label>
                  <Input
                    id="model-tags"
                    value={draft.tags}
                    onChange={(event) => updateDraft("tags", event.target.value)}
                    placeholder={t("例如：编程, 推理")}
                  />
                </div>
              </div>

              <div className="grid gap-4 md:grid-cols-4">
                <div className="space-y-2">
                  <Label htmlFor="model-sort-order">{t("排序")}</Label>
                  <Input
                    id="model-sort-order"
                    type="number"
                    value={draft.sortOrder}
                    onChange={(event) => updateDraft("sortOrder", event.target.value)}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="model-context-window">{t("上下文窗口")}</Label>
                  <Input
                    id="model-context-window"
                    type="number"
                    min="1"
                    value={draft.contextWindow}
                    onChange={(event) => updateDraft("contextWindow", event.target.value)}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="model-max-context-window">{t("最大上下文窗口")}</Label>
                  <Input
                    id="model-max-context-window"
                    type="number"
                    min="1"
                    value={draft.maxContextWindow}
                    onChange={(event) =>
                      updateDraft("maxContextWindow", event.target.value)
                    }
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="model-reasoning-effort">{t("默认推理强度")}</Label>
                  <Input
                    id="model-reasoning-effort"
                    value={draft.defaultReasoningEffort}
                    onChange={(event) =>
                      updateDraft("defaultReasoningEffort", event.target.value)
                    }
                  />
                </div>
              </div>

              <div className="grid gap-3 md:grid-cols-3">
                <Card size="sm">
                  <CardContent className="flex items-center justify-between gap-3">
                    <Label htmlFor="model-enabled">{t("启用模型")}</Label>
                    <Switch
                      id="model-enabled"
                      checked={draft.enabled}
                      onCheckedChange={(checked) => updateDraft("enabled", checked)}
                    />
                  </CardContent>
                </Card>
                <Card size="sm">
                  <CardContent className="flex items-center justify-between gap-3">
                    <Label htmlFor="model-supported-api">{t("可用于 API")}</Label>
                    <Switch
                      id="model-supported-api"
                      checked={draft.supportedInApi}
                      onCheckedChange={(checked) =>
                        updateDraft("supportedInApi", checked)
                      }
                    />
                  </CardContent>
                </Card>
                <div className="space-y-2">
                  <Label htmlFor="model-visibility">{t("可见性")}</Label>
                  <Select
                    value={draft.visibility}
                    onValueChange={(value) =>
                      updateDraft("visibility", (value || "list") as ModelVisibilityV2)
                    }
                  >
                    <SelectTrigger id="model-visibility" aria-label={t("可见性")}>
                      <SelectValue>
                        {(value) => value === "hide" ? t("隐藏") : t("列表显示")}
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        <SelectItem value="list">{t("列表显示")}</SelectItem>
                        <SelectItem value="hide">{t("隐藏")}</SelectItem>
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </div>
              </div>

              <div className="space-y-2">
                <Label htmlFor="model-capabilities">{t("关键能力 JSON")}</Label>
                <Textarea
                  id="model-capabilities"
                  rows={10}
                  className="font-mono text-xs"
                  value={draft.capabilitiesJson}
                  onChange={(event) =>
                    updateDraft("capabilitiesJson", event.target.value)
                  }
                />
              </div>
            </TabsContent>

            <TabsContent value="price" className="mt-4 space-y-4">
              <Card size="sm">
                <CardHeader><CardTitle>{t("基础价格（美元 / 百万令牌）")}</CardTitle></CardHeader>
                <CardContent className="grid gap-4 md:grid-cols-3">
                  <div className="space-y-2">
                    <Label htmlFor="price-input">{t("输入价格")}</Label>
                    <Input id="price-input" inputMode="decimal" value={draft.inputPrice} onChange={(event) => updateDraft("inputPrice", event.target.value)} placeholder={t("留空表示价格缺失")} />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="price-cached">{t("缓存输入价格")}</Label>
                    <Input id="price-cached" inputMode="decimal" value={draft.cachedInputPrice} onChange={(event) => updateDraft("cachedInputPrice", event.target.value)} />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="price-output">{t("输出价格")}</Label>
                    <Input id="price-output" inputMode="decimal" value={draft.outputPrice} onChange={(event) => updateDraft("outputPrice", event.target.value)} />
                  </div>
                </CardContent>
              </Card>

              <Card size="sm">
                <CardHeader><CardTitle>{t("可选长上下文阶梯价")}</CardTitle></CardHeader>
                <CardContent className="grid gap-4 md:grid-cols-4">
                  <div className="space-y-2">
                    <Label htmlFor="price-long-threshold">{t("输入令牌阈值")}</Label>
                    <Input id="price-long-threshold" type="number" min="1" value={draft.longContextThreshold} onChange={(event) => updateDraft("longContextThreshold", event.target.value)} />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="price-long-input">{t("输入价格")}</Label>
                    <Input id="price-long-input" inputMode="decimal" value={draft.longInputPrice} onChange={(event) => updateDraft("longInputPrice", event.target.value)} />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="price-long-cached">{t("缓存输入价格")}</Label>
                    <Input id="price-long-cached" inputMode="decimal" value={draft.longCachedInputPrice} onChange={(event) => updateDraft("longCachedInputPrice", event.target.value)} />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="price-long-output">{t("输出价格")}</Label>
                    <Input id="price-long-output" inputMode="decimal" value={draft.longOutputPrice} onChange={(event) => updateDraft("longOutputPrice", event.target.value)} />
                  </div>
                </CardContent>
              </Card>
              <p className="text-xs text-muted-foreground">
                {t("价格按十进制字符串无损转换为整数 micro-USD；三个基础价格必须同时存在或同时留空。")}
              </p>
            </TabsContent>

            <TabsContent value="routes" className="mt-4 space-y-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-sm text-muted-foreground">
                  {t("上游模型名始终手填；这里不会访问供应商 `/models`。")}
                </p>
                <div className="flex gap-2">
                  <Button type="button" size="sm" variant="outline" onClick={() => addRoute("account_pool")}>
                    <Plus className="mr-1.5 h-4 w-4" />{t("添加账号池路由")}
                  </Button>
                  <Button type="button" size="sm" variant="outline" onClick={() => addRoute("aggregate_api")}>
                    <Plus className="mr-1.5 h-4 w-4" />{t("添加聚合路由")}
                  </Button>
                </div>
              </div>

              {draft.routes.length === 0 ? (
                <div className="rounded-lg border border-dashed p-8 text-center text-sm text-muted-foreground">
                  {t("当前模型没有 route，启用后将显示 missing route。")}
                </div>
              ) : (
                draft.routes.map((route, index) => (
                  <Card key={route.key} size="sm">
                    <CardContent className="space-y-3">
                      <div className="grid gap-3 md:grid-cols-[minmax(150px,0.8fr)_minmax(180px,1fr)_minmax(180px,1fr)]">
                        <div className="min-w-0 space-y-2">
                          <Label className="leading-5" htmlFor={`route-kind-${index}`}>{t("来源类型")}</Label>
                          <Select
                            value={route.sourceKind}
                            onValueChange={(value) => {
                              const sourceKind = (value || "account_pool") as ModelRouteSourceKindV2;
                              updateRoute(index, "sourceKind", sourceKind);
                              updateRoute(index, "sourceId", sourceKind === "account_pool" ? "default" : "");
                            }}
                          >
                            <SelectTrigger id={`route-kind-${index}`} className="w-full min-w-0" aria-label={t("来源类型")}>
                              <SelectValue>
                                {(value) => value === "aggregate_api" ? t("聚合 API") : t("账号池")}
                              </SelectValue>
                            </SelectTrigger>
                            <SelectContent><SelectGroup>
                              <SelectItem value="account_pool">{t("账号池")}</SelectItem>
                              <SelectItem value="aggregate_api">{t("聚合 API")}</SelectItem>
                            </SelectGroup></SelectContent>
                          </Select>
                        </div>
                        <div className="min-w-0 space-y-2">
                          <Label className="leading-5" htmlFor={`route-source-${index}`}>
                            {t("来源")}
                          </Label>
                          {route.sourceKind === "aggregate_api" && aggregateApis.length > 0 ? (
                            <Select
                              value={route.sourceId}
                              onValueChange={(value) =>
                                updateRoute(index, "sourceId", value || "")
                              }
                            >
                              <SelectTrigger
                                id={`route-source-${index}`}
                                className="w-full min-w-0"
                                aria-label={t("来源")}
                              >
                                <SelectValue placeholder={t("选择聚合 API")}>
                                  {(value) =>
                                    aggregateRouteSourceLabel(
                                      String(value || ""),
                                      aggregateApis,
                                      t,
                                    )
                                  }
                                </SelectValue>
                              </SelectTrigger>
                              <SelectContent><SelectGroup>
                                {aggregateApis.map((api) => (
                                  <SelectItem key={api.id} value={api.id}>
                                    {t("聚合 API")}：{aggregateApiDisplayName(api, t)}
                                  </SelectItem>
                                ))}
                              </SelectGroup></SelectContent>
                            </Select>
                          ) : (
                            <Input
                              id={`route-source-${index}`}
                              value={
                                route.sourceKind === "account_pool"
                                  ? t("默认账号池")
                                  : route.sourceId
                              }
                              disabled={route.sourceKind === "account_pool"}
                              onChange={(event) =>
                                updateRoute(index, "sourceId", event.target.value)
                              }
                            />
                          )}
                        </div>
                        <div className="min-w-0 space-y-2">
                          <Label className="leading-5" htmlFor={`route-model-${index}`}>{t("上游模型")}</Label>
                          <Input id={`route-model-${index}`} value={route.upstreamModel} onChange={(event) => updateRoute(index, "upstreamModel", event.target.value)} />
                        </div>
                      </div>
                      <div className="grid items-end gap-3 md:grid-cols-[120px_120px_1fr_auto]">
                        <div className="space-y-2">
                          <Label htmlFor={`route-priority-${index}`}>{t("优先级")}</Label>
                          <Input id={`route-priority-${index}`} type="number" value={route.priority} onChange={(event) => updateRoute(index, "priority", event.target.value)} />
                        </div>
                        <div className="space-y-2">
                          <Label htmlFor={`route-weight-${index}`}>{t("权重")}</Label>
                          <Input id={`route-weight-${index}`} type="number" min="1" value={route.weight} onChange={(event) => updateRoute(index, "weight", event.target.value)} />
                        </div>
                        <div className="flex h-9 items-center gap-2">
                          <Switch id={`route-enabled-${index}`} aria-label={t("启用路由")} checked={route.enabled} onCheckedChange={(checked) => updateRoute(index, "enabled", checked)} />
                          <Label htmlFor={`route-enabled-${index}`}>{t("启用路由")}</Label>
                        </div>
                        <Button type="button" variant="ghost" size="icon" aria-label={t("删除路由")} onClick={() => updateDraft("routes", draft.routes.filter((_, routeIndex) => routeIndex !== index))}>
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </CardContent>
                  </Card>
                ))
              )}
            </TabsContent>

            <TabsContent value="instructions" className="mt-4 space-y-4">
              <div className="space-y-2">
                <Label htmlFor="model-instructions-mode">{t("指令模式")}</Label>
                <Select value={draft.instructionsMode} onValueChange={(value) => updateDraft("instructionsMode", (value || "passthrough") as ModelInstructionsModeV2)}>
                  <SelectTrigger id="model-instructions-mode" aria-label={t("指令模式")}>
                    <SelectValue>
                      {(value) => value === "fallback" ? t("兜底") : value === "override" ? t("覆盖") : t("透传")}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent><SelectGroup>
                    <SelectItem value="passthrough">{t("透传")}</SelectItem>
                    <SelectItem value="fallback">{t("兜底")}</SelectItem>
                    <SelectItem value="override">{t("覆盖")}</SelectItem>
                  </SelectGroup></SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <Label htmlFor="model-instructions-text">{t("指令文本")}</Label>
                <Textarea id="model-instructions-text" rows={14} value={draft.instructionsText} disabled={draft.instructionsMode === "passthrough"} onChange={(event) => updateDraft("instructionsText", event.target.value)} />
              </div>
              <p className="text-xs text-muted-foreground">
                {draft.instructionsMode === "passthrough"
                  ? t("客户端 instructions 原样传递，模型文本不参与请求。")
                  : draft.instructionsMode === "fallback"
                    ? t("仅当所有客户端 instruction channel 都为空时使用模型文本。")
                    : t("模型文本替换顶层及连续 leading system/developer instructions；文本不能为空。")}
              </p>
            </TabsContent>
          </Tabs>

          {error ? <p className="mt-4 text-sm text-destructive">{error}</p> : null}
        </div>

        <div className="border-t border-border/50 px-5 py-3">
          <DialogFooter>
            <DialogClose className={buttonVariants({ variant: "ghost" })} type="button">
              {t("取消")}
            </DialogClose>
            <Button type="button" disabled={isSaving} onClick={() => void handleSave()}>
              {isSaving ? t("保存中...") : t("保存模型")}
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}
