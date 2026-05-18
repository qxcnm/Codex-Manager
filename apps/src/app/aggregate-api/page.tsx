"use client";

import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowUp,
  Copy,
  Database,
  Download,
  Eye,
  EyeOff,
  MoreVertical,
  Plus,
  RefreshCw,
  Save,
  Settings2,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { AggregateApiModal } from "@/components/modals/aggregate-api-modal";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { accountClient } from "@/lib/api/account-client";
import { quotaClient } from "@/lib/api/quota-client";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { formatCompactNumber, formatTsFromSeconds } from "@/lib/utils/usage";
import { useAppStore } from "@/lib/store/useAppStore";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import {
  AggregateApi,
  AggregateApiBalanceSnapshot,
  AggregateApiSecretResult,
  AggregateApiSupplierModel,
  ManagedModelSourceModel,
} from "@/types";

type TranslateFn = (key: string, values?: Record<string, string | number>) => string;

const AGGREGATE_API_PROVIDER_LABELS: Record<string, string> = {
  codex: "Codex",
  claude: "Claude",
  gemini: "Gemini",
};

const AGGREGATE_API_PROVIDER_FILTER_LABELS: Record<string, string> = {
  all: "全部类型",
  codex: "Codex",
  claude: "Claude",
  gemini: "Gemini",
};

function parseBalanceSnapshot(api: AggregateApi): AggregateApiBalanceSnapshot | null {
  const raw = String(api.lastBalanceJson || "").trim();
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<AggregateApiBalanceSnapshot>;
    return {
      isValid: parsed.isValid ?? true,
      invalidMessage: parsed.invalidMessage ?? null,
      remaining: typeof parsed.remaining === "number" ? parsed.remaining : null,
      unit: typeof parsed.unit === "string" ? parsed.unit : null,
      planName: typeof parsed.planName === "string" ? parsed.planName : null,
      total: typeof parsed.total === "number" ? parsed.total : null,
      used: typeof parsed.used === "number" ? parsed.used : null,
      extra:
        parsed.extra && typeof parsed.extra === "object"
          ? (parsed.extra as Record<string, unknown>)
          : null,
    };
  } catch {
    return null;
  }
}

function formatBalanceAmount(snapshot: AggregateApiBalanceSnapshot | null) {
  if (!snapshot || typeof snapshot.remaining !== "number") {
    return "-";
  }
  const unit = String(snapshot.unit || "").trim();
  const value = Number.isInteger(snapshot.remaining)
    ? String(snapshot.remaining)
    : snapshot.remaining.toFixed(2);
  if (unit.toUpperCase() === "USD") {
    return `$${value}`;
  }
  return unit ? `${value} ${unit}` : value;
}

function aggregateApiSupplierKey(api: AggregateApi | null) {
  if (!api) return "";
  return String(api.supplierName || "").trim() || String(api.url || "").trim();
}

function sourceModelKey(model: ManagedModelSourceModel | AggregateApiSupplierModel) {
  return [
    "sourceKind" in model ? model.sourceKind : model.supplierKey,
    "sourceId" in model ? model.sourceId : model.providerType,
    model.upstreamModel,
  ].join("::");
}

/**
 * 函数 `getTestBadge`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - api: 参数 api
 *
 * # 返回
 * 返回函数执行结果
 */
function getTestBadge(api: AggregateApi, t: TranslateFn) {
  if (api.lastTestStatus === "success") {
    return (
      <Badge className="border-green-500/20 bg-green-500/10 text-green-500">
        {t("已连通")}
      </Badge>
    );
  }
  if (api.lastTestStatus === "failed") {
    return (
      <Badge className="border-red-500/20 bg-red-500/10 text-red-500">
        {t("失败")}
      </Badge>
    );
  }
  return <Badge variant="secondary">{t("未测试")}</Badge>;
}

export default function AggregateApiPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const isPageActive = useDesktopPageActive("/aggregate-api/");
  const isQueryEnabled = useDeferredDesktopActivation(
    isServiceReady && isPageActive,
  );
  const [modalOpen, setModalOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [providerFilter, setProviderFilter] = useState("all");
  const [revealedSecrets, setRevealedSecrets] = useState<
    Record<string, AggregateApiSecretResult>
  >({});
  const [loadingSecretId, setLoadingSecretId] = useState<string | null>(null);
  const [testingApiId, setTestingApiId] = useState<string | null>(null);
  const [testingAll, setTestingAll] = useState(false);
  const [refreshingBalanceId, setRefreshingBalanceId] = useState<string | null>(null);
  const [refreshingBalances, setRefreshingBalances] = useState(false);
  const [togglingApiId, setTogglingApiId] = useState<string | null>(null);
  const [statusOverrides, setStatusOverrides] = useState<Record<string, boolean>>(
    {},
  );
  const [modelPoolApiId, setModelPoolApiId] = useState<string | null>(null);
  const [sourceModelDraft, setSourceModelDraft] = useState({
    upstreamModel: "",
    displayName: "",
  });
  const [supplierModelDraft, setSupplierModelDraft] = useState({
    upstreamModel: "",
    displayName: "",
  });

  const { data: aggregateApis = [], isLoading } = useQuery({
    queryKey: ["aggregate-apis"],
    queryFn: () => accountClient.listAggregateApis(),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const { data: quotaModelPools } = useQuery({
    queryKey: ["quota", "model-pools"],
    queryFn: () => quotaClient.modelPools(),
    enabled: isQueryEnabled,
    retry: 1,
  });

  const { data: modelRouting, isLoading: modelRoutingLoading } = useQuery({
    queryKey: ["model-routing"],
    queryFn: () => accountClient.listManagedModelRouting(),
    enabled: isQueryEnabled && Boolean(modelPoolApiId),
    retry: 1,
  });

  usePageTransitionReady("/aggregate-api/", !isServiceReady || !isLoading);

  useEffect(() => {
    if (isPageActive) return;
    setModalOpen(false);
    setEditingId(null);
    setDeleteId(null);
    setModelPoolApiId(null);
  }, [isPageActive]);

  useEffect(() => {
    setSourceModelDraft({ upstreamModel: "", displayName: "" });
    setSupplierModelDraft({ upstreamModel: "", displayName: "" });
  }, [modelPoolApiId]);

  useEffect(() => {
    setStatusOverrides((current) => {
      const serverStatusMap = new Map(
        aggregateApis.map((item) => [
          item.id,
          String(item.status || "").trim().toLowerCase() !== "disabled",
        ]),
      );
      let changed = false;
      const next: Record<string, boolean> = {};

      Object.entries(current).forEach(([id, enabled]) => {
        const serverEnabled = serverStatusMap.get(id);
        if (serverEnabled == null) {
          changed = true;
          return;
        }
        if (serverEnabled !== enabled) {
          next[id] = enabled;
          return;
        }
        changed = true;
      });

      return changed ? next : current;
    });
  }, [aggregateApis]);

  const editingApi = useMemo(
    () => aggregateApis.find((item) => item.id === editingId) || null,
    [aggregateApis, editingId],
  );

  const modelPoolApi = useMemo(
    () => aggregateApis.find((item) => item.id === modelPoolApiId) || null,
    [aggregateApis, modelPoolApiId],
  );
  const modelPoolSupplierKey = useMemo(
    () => aggregateApiSupplierKey(modelPoolApi),
    [modelPoolApi],
  );
  const modelPoolProviderType = modelPoolApi?.providerType || "codex";
  const modelPoolApiActive = modelPoolApi?.status === "active";

  const { data: supplierModels = [], isLoading: supplierModelsLoading } = useQuery({
    queryKey: [
      "aggregate-api",
      "supplier-models",
      modelPoolSupplierKey,
      modelPoolProviderType,
    ],
    queryFn: () =>
      accountClient.listAggregateApiSupplierModels({
        supplierKey: modelPoolSupplierKey,
        providerType: modelPoolProviderType,
      }),
    enabled: isQueryEnabled && Boolean(modelPoolApiId) && Boolean(modelPoolSupplierKey),
    retry: 1,
  });

  const sourceModels = useMemo(
    () =>
      (modelRouting?.sourceModels || [])
        .filter(
          (item) =>
            item.sourceKind === "aggregate_api" && item.sourceId === modelPoolApiId,
        )
        .sort((a, b) => a.upstreamModel.localeCompare(b.upstreamModel)),
    [modelRouting?.sourceModels, modelPoolApiId],
  );

  const sourceModelKeys = useMemo(
    () => new Set(sourceModels.map((item) => item.upstreamModel)),
    [sourceModels],
  );

  const filteredAggregateApis = useMemo(() => {
    if (providerFilter === "all") {
      return aggregateApis;
    }
    return aggregateApis.filter((api) => api.providerType === providerFilter);
  }, [aggregateApis, providerFilter]);

  const defaultCreateSort = useMemo(() => {
    const maxSort = aggregateApis.reduce(
      (max, api) => Math.max(max, Number(api.sort) || 0),
      0,
    );
    return maxSort + 5;
  }, [aggregateApis]);

  const aggregateQuotaById = useMemo(() => {
    const map = new Map<
      string,
      { model: string | null; tokens: number | null; models: Set<string> }
    >();
    for (const item of quotaModelPools?.items ?? []) {
      for (const source of item.sources) {
        if (source.sourceKind !== "aggregate_api") continue;
        const current =
          map.get(source.sourceId) ||
          { model: null, tokens: null, models: new Set<string>() };
        current.models.add(item.model);
        if (current.tokens == null && source.remainingTokens != null) {
          current.tokens = source.remainingTokens;
          current.model = item.model;
        }
        map.set(source.sourceId, current);
      }
    }
    return map;
  }, [quotaModelPools]);

  /**
   * 函数 `renderTestStatus`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - api: 参数 api
   *
   * # 返回
   * 返回函数执行结果
   */
  const renderTestStatus = (api: AggregateApi) => {
    const badge = getTestBadge(api, t);
    if (api.lastTestStatus !== "failed" || !api.lastTestError) {
      return badge;
    }

    return (
      <Tooltip>
        <TooltipTrigger render={<div />} className="inline-flex cursor-help">
          {badge}
        </TooltipTrigger>
        <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
          {api.lastTestError}
        </TooltipContent>
      </Tooltip>
    );
  };

  const renderBalanceStatus = (api: AggregateApi) => {
    if (!api.balanceQueryEnabled) {
      return <Badge variant="secondary">{t("未启用")}</Badge>;
    }

    const snapshot = parseBalanceSnapshot(api);
    if (api.lastBalanceStatus === "success" && snapshot) {
      const badge = (
        <Badge className="border-emerald-500/20 bg-emerald-500/10 text-emerald-600">
          {formatBalanceAmount(snapshot)}
        </Badge>
      );
      const details = [
        snapshot.planName ? `${t("套餐")}: ${snapshot.planName}` : null,
        typeof snapshot.used === "number"
          ? `${t("已用")}: ${snapshot.used.toFixed(2)}`
          : null,
        typeof snapshot.total === "number"
          ? `${t("总额")}: ${snapshot.total.toFixed(2)}`
          : null,
      ].filter(Boolean);

      if (details.length === 0) {
        return badge;
      }
      return (
        <Tooltip>
          <TooltipTrigger render={<div />} className="inline-flex cursor-help">
            {badge}
          </TooltipTrigger>
          <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
            {details.join("\n")}
          </TooltipContent>
        </Tooltip>
      );
    }

    if (api.lastBalanceStatus === "failed") {
      const badge = (
        <Badge className="border-red-500/20 bg-red-500/10 text-red-500">
          {t("查询失败")}
        </Badge>
      );
      if (!api.lastBalanceError) {
        return badge;
      }
      return (
        <Tooltip>
          <TooltipTrigger render={<div />} className="inline-flex cursor-help">
            {badge}
          </TooltipTrigger>
          <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
            {api.lastBalanceError}
          </TooltipContent>
        </Tooltip>
      );
    }

    return <Badge variant="secondary">{t("未查询")}</Badge>;
  };

  const testMutation = useMutation({
    mutationFn: (apiId: string) =>
      accountClient.testAggregateApiConnection(apiId),
    onMutate: async (apiId) => {
      setTestingApiId(apiId);
    },
    onSuccess: async (result) => {
      if (result.ok) {
        toast.success(t("已连通"));
        return;
      }
      toast.error(
        t("连通性测试失败: {reason}", {
          reason: result.message || result.statusCode || t("未返回具体错误信息"),
        }),
      );
    },
    onSettled: async (_result, _error, apiId) => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      setTestingApiId((current) => (current === apiId ? null : current));
    },
    onError: (error: unknown) => {
      toast.error(`${t("测试")} ${t("失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const testAllMutation = useMutation({
    mutationFn: async (apiIds: string[]) => {
      const results = await Promise.allSettled(
        apiIds.map((id) => accountClient.testAggregateApiConnection(id))
      );
      return results;
    },
    onMutate: async () => {
      setTestingAll(true);
    },
    onSuccess: async (results) => {
      const successCount = results.filter(
        (r) => r.status === "fulfilled" && r.value.ok
      ).length;
      const failCount = results.length - successCount;

      if (failCount === 0) {
        toast.success(t("全部测试完成，{count} 个连通", { count: successCount }));
      } else {
        toast.warning(
          t("测试完成：{success} 个连通，{fail} 个失败", {
            success: successCount,
            fail: failCount,
          })
        );
      }
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      setTestingAll(false);
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量测试失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const refreshBalanceMutation = useMutation({
    mutationFn: (apiId: string) => accountClient.refreshAggregateApiBalance(apiId),
    onMutate: async (apiId) => {
      setRefreshingBalanceId(apiId);
    },
    onSuccess: async (result) => {
      if (result.ok) {
        toast.success(t("余额已刷新"));
        return;
      }
      toast.warning(
        t("余额查询失败 {reason}", {
          reason: result.message || t("未返回具体错误信息"),
        }),
      );
    },
    onSettled: async (_result, _error, apiId) => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      setRefreshingBalanceId((current) => (current === apiId ? null : current));
    },
    onError: (error: unknown) => {
      toast.error(`${t("余额查询失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const refreshAllBalancesMutation = useMutation({
    mutationFn: async (apiIds: string[]) => {
      const results = await Promise.allSettled(
        apiIds.map((id) => accountClient.refreshAggregateApiBalance(id))
      );
      return results;
    },
    onMutate: async () => {
      setRefreshingBalances(true);
    },
    onSuccess: async (results) => {
      const successCount = results.filter(
        (r) => r.status === "fulfilled" && r.value.ok
      ).length;
      const failCount = results.length - successCount;
      if (failCount === 0) {
        toast.success(t("余额刷新完成：{count} 个成功", { count: successCount }));
      } else {
        toast.warning(
          t("余额刷新完成：{success} 个成功，{fail} 个失败", {
            success: successCount,
            fail: failCount,
          })
        );
      }
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      setRefreshingBalances(false);
    },
    onError: (error: unknown) => {
      toast.error(`${t("批量刷新余额失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (apiId: string) => accountClient.deleteAggregateApi(apiId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      await queryClient.invalidateQueries({ queryKey: ["apikeys"] });
      await queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] });
      toast.success(`${t("聚合API")} ${t("删除")}`);
    },
    onError: (error: unknown) => {
      toast.error(`${t("删除")} ${t("失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const prioritizeMutation = useMutation({
    mutationFn: async (api: AggregateApi) => {
      const currentMinSort = aggregateApis.reduce(
        (min, item) => Math.min(min, Number(item.sort) || 0),
        Number(api.sort) || 0,
      );
      const nextSort =
        (Number(api.sort) || 0) <= currentMinSort ? currentMinSort : currentMinSort - 5;

      if ((Number(api.sort) || 0) === nextSort) {
        return false;
      }

      await accountClient.updateAggregateApi(api.id, {
        providerType: api.providerType,
        supplierName: api.supplierName || "",
        sort: nextSort,
        url: api.url,
        key: null,
      });
      return true;
    },
    onSuccess: async (changed) => {
      if (!changed) {
        toast.info(t("设为优先"));
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] });
      toast.success(t("设为优先"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("设为优先")} ${t("失败")}: ${error instanceof Error ? error.message : String(error)}`);
    },
  });

  const toggleStatusMutation = useMutation({
    mutationFn: async ({
      api,
      enabled,
    }: {
      api: AggregateApi;
      enabled: boolean;
    }) => {
      await accountClient.updateAggregateApi(api.id, {
        supplierName: api.supplierName || api.url,
        status: enabled ? "active" : "disabled",
      });
      return enabled;
    },
    onMutate: async ({ api, enabled }) => {
      await queryClient.cancelQueries({ queryKey: ["aggregate-apis"] });
      const previousAggregateApis =
        queryClient.getQueryData<AggregateApi[]>(["aggregate-apis"]) || [];
      setStatusOverrides((current) => ({
        ...current,
        [api.id]: enabled,
      }));
      queryClient.setQueryData<AggregateApi[]>(["aggregate-apis"], (current) =>
        (current || []).map((item) =>
          item.id === api.id
            ? {
                ...item,
                status: enabled ? "active" : "disabled",
              }
            : item,
        ),
      );
      setTogglingApiId(api.id);
      return {
        previousAggregateApis,
      };
    },
    onSuccess: async (_result, variables) => {
      setStatusOverrides((current) => ({
        ...current,
        [variables.api.id]: variables.enabled,
      }));
      queryClient.setQueryData<AggregateApi[]>(["aggregate-apis"], (current) =>
        (current || []).map((item) =>
          item.id === variables.api.id
            ? {
                ...item,
                status: variables.enabled ? "active" : "disabled",
              }
            : item,
        ),
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["aggregate-apis"] }),
        queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      ]);
      toast.success(t("状态已更新"));
    },
    onError: (error: unknown, _variables, context) => {
      if (context?.previousAggregateApis) {
        queryClient.setQueryData(
          ["aggregate-apis"],
          context.previousAggregateApis,
        );
      }
      setStatusOverrides((current) => {
        const next = { ...current };
        if (_variables?.api?.id) {
          delete next[_variables.api.id];
        }
        return next;
      });
      toast.error(
        `${t("更新状态失败")}: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
    onSettled: async (_result, _error, variables) => {
      setTogglingApiId((current) =>
        current === variables.api.id ? null : current,
      );
    },
  });

  const syncModelPoolMutation = useMutation({
    mutationFn: (apiId: string) =>
      accountClient.syncManagedModelSourceModels({
        sourceKind: "aggregate_api",
        sourceId: apiId,
      }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["model-routing"] });
      toast.success(t("模型池已同步"));
    },
    onError: (error: unknown) => {
      toast.error(
        `${t("同步失败")}: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  const saveSourceModelMutation = useMutation({
    mutationFn: (params: {
      apiId: string;
      upstreamModel: string;
      displayName?: string | null;
    }) =>
      accountClient.saveManagedModelSourceModel({
        sourceKind: "aggregate_api",
        sourceId: params.apiId,
        upstreamModel: params.upstreamModel,
        displayName: params.displayName,
      }),
    onSuccess: async () => {
      setSourceModelDraft({ upstreamModel: "", displayName: "" });
      await queryClient.invalidateQueries({ queryKey: ["model-routing"] });
      toast.success(t("来源模型已添加"));
    },
    onError: (error: unknown) => {
      toast.error(
        `${t("保存失败")}: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  const saveSupplierModelMutation = useMutation({
    mutationFn: (params: {
      supplierKey: string;
      providerType: string;
      upstreamModel: string;
      displayName?: string | null;
    }) => accountClient.saveAggregateApiSupplierModel(params),
    onSuccess: async () => {
      setSupplierModelDraft({ upstreamModel: "", displayName: "" });
      await queryClient.invalidateQueries({
        queryKey: ["aggregate-api", "supplier-models"],
      });
      toast.success(t("供应商模型已保存"));
    },
    onError: (error: unknown) => {
      toast.error(
        `${t("保存失败")}: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  const importSupplierModelsMutation = useMutation({
    mutationFn: (params: {
      apiId: string;
      supplierKey: string;
      providerType: string;
    }) => accountClient.importAggregateApiSupplierModels(params),
    onSuccess: async (result) => {
      await queryClient.invalidateQueries({ queryKey: ["model-routing"] });
      toast.success(t("已导入 {count} 个模型", { count: result.imported }));
    },
    onError: (error: unknown) => {
      toast.error(
        `${t("导入失败")}: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  const deleteSupplierModelMutation = useMutation({
    mutationFn: (params: {
      supplierKey: string;
      providerType: string;
      upstreamModel: string;
    }) => accountClient.deleteAggregateApiSupplierModel(params),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: ["aggregate-api", "supplier-models"],
      });
      toast.success(t("供应商模型已删除"));
    },
    onError: (error: unknown) => {
      toast.error(
        `${t("删除失败")}: ${error instanceof Error ? error.message : String(error)}`,
      );
    },
  });

  /**
   * 函数 `openCreateModal`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * 无
   *
   * # 返回
   * 返回函数执行结果
   */
  const openCreateModal = () => {
    setEditingId(null);
    setModalOpen(true);
  };

  /**
   * 函数 `openEditModal`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const openEditModal = (apiId: string) => {
    setEditingId(apiId);
    setModalOpen(true);
  };

  /**
   * 函数 `ensureSecretLoaded`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const ensureSecretLoaded = async (apiId: string) => {
    if (revealedSecrets[apiId]) {
      return revealedSecrets[apiId];
    }
    setLoadingSecretId(apiId);
    try {
      const secretResult = await accountClient.readAggregateApiSecret(apiId);
      const authType = String(secretResult.authType || "").trim().toLowerCase();
      if (authType === "userpass") {
        if (!secretResult.username || !secretResult.password) {
          throw new Error(t("后端未返回账号密码明文"));
        }
      } else if (!secretResult.key) {
        throw new Error(t("后端未返回密钥明文"));
      }
      setRevealedSecrets((current) => ({ ...current, [apiId]: secretResult }));
      return secretResult;
    } finally {
      setLoadingSecretId(null);
    }
  };

  /**
   * 函数 `toggleSecret`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const toggleSecret = async (apiId: string) => {
    if (revealedSecrets[apiId]) {
      setRevealedSecrets((current) => {
        const next = { ...current };
        delete next[apiId];
        return next;
      });
      return;
    }
    try {
      await ensureSecretLoaded(apiId);
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  /**
   * 函数 `copySecret`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - apiId: 参数 apiId
   *
   * # 返回
   * 返回函数执行结果
   */
  const copySecret = async (
    apiId: string,
    target: "key" | "username" | "password"
  ) => {
    try {
      const secret = await ensureSecretLoaded(apiId);
      const authType = String(secret.authType || "").trim().toLowerCase();
      const value =
        target === "username"
          ? secret.username
          : target === "password"
            ? secret.password
            : secret.key;
      if (authType === "userpass") {
        if (!value) {
          throw new Error(t("账号密码字段为空"));
        }
      } else if (!value) {
        throw new Error(t("密钥为空"));
      }
      await copyTextToClipboard(value);
      toast.success(t("已复制到剪贴板"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const secretPreview = (secret: AggregateApiSecretResult) => {
    const authType = String(secret.authType || "").trim().toLowerCase();
    if (authType === "userpass") {
      return `${secret.username || ""}:${secret.password || ""}`;
    }
    return secret.key || "";
  };

  const sourceModelDraftUpstream = sourceModelDraft.upstreamModel.trim();
  const supplierModelDraftUpstream = supplierModelDraft.upstreamModel.trim();

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      {!isServiceReady ? (
        <Card className="glass-card shadow-sm">
          <CardContent className="pt-6 text-sm text-muted-foreground">
            {t("服务未连接")}
          </CardContent>
        </Card>
      ) : null}

      <div>
        <div>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("管理上游聚合地址与密钥，并测试连通性")}
          </p>
        </div>
      </div>

      <div className="space-y-4">
        <Card className="glass-card shadow-sm">
          <CardContent className="px-4 ">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-2">
                <span className="text-sm text-muted-foreground">{t("查询")}</span>
                <Select
                  value={providerFilter}
                  onValueChange={(value) => setProviderFilter(value || "all")}
                >
                  <SelectTrigger className="w-[160px]">
                    <SelectValue>
                      {(value) =>
                        t(
                          AGGREGATE_API_PROVIDER_FILTER_LABELS[
                            String(value || "")
                          ] || "全部类型",
                        )
                      }
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="all">{t("全部类型")}</SelectItem>
                    <SelectItem value="codex">Codex</SelectItem>
                    <SelectItem value="claude">Claude</SelectItem>
                    <SelectItem value="gemini">Gemini</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              <div className="flex items-center gap-3">
                <div className="text-xs text-muted-foreground">
                  {t("共")} {filteredAggregateApis.length} {t("条")}
                </div>
                <Button
                  variant="outline"
                  className="h-10 gap-2"
                  onClick={() => {
                    const apiIds = filteredAggregateApis.map((api) => api.id);
                    if (apiIds.length === 0) {
                      toast.info(t("暂无可测试的聚合 API"));
                      return;
                    }
                    testAllMutation.mutate(apiIds);
                  }}
                  disabled={!isServiceReady || testingAll || filteredAggregateApis.length === 0}
                >
                  <RefreshCw className={testingAll ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                  {t("测试全部")}
                </Button>
                <Button
                  variant="outline"
                  className="h-10 gap-2"
                  onClick={() => {
                    const apiIds = filteredAggregateApis
                      .filter((api) => api.balanceQueryEnabled)
                      .map((api) => api.id);
                    if (apiIds.length === 0) {
                      toast.info(t("暂无已启用余额检测的聚合 API"));
                      return;
                    }
                    refreshAllBalancesMutation.mutate(apiIds);
                  }}
                  disabled={
                    !isServiceReady ||
                    refreshingBalances ||
                    filteredAggregateApis.every((api) => !api.balanceQueryEnabled)
                  }
                >
                  <RefreshCw className={refreshingBalances ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                  {t("刷新余额")}
                </Button>
                <Button
                  className="h-10 gap-2 shadow-sm shadow-primary/20"
                  onClick={openCreateModal}
                  disabled={!isServiceReady}
                >
                  <Plus className="h-4 w-4" /> {t("新建聚合 API")}
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card className="glass-card overflow-hidden py-0 shadow-sm">
          <CardContent className="p-0">
            <Table className="w-full table-fixed">
              <TableHeader>
                <TableRow>
                  <TableHead className="max-w-[220px]">{t("供应商 / URL")}</TableHead>
                  <TableHead className="w-[84px] text-center">{t("类型")}</TableHead>
                  <TableHead className="w-[148px]">{t("密钥")}</TableHead>
                  <TableHead className="w-[64px] text-center">{t("顺序")}</TableHead>
                  <TableHead className="w-[130px]">{t("测试连通性")}</TableHead>
                  <TableHead className="w-[150px]">{t("余额")}</TableHead>
                  <TableHead className="w-[112px] text-right pr-4">{t("状态")}</TableHead>
                  <TableHead className="table-sticky-action-head w-[144px] text-center">
                    {t("操作")}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {isLoading ? (
                  Array.from({ length: 3 }).map((_, index) => (
                    <TableRow key={index}>
                      <TableCell>
                        <Skeleton className="h-4 w-24" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-6 w-12 rounded-full" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-4 w-28" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="mx-auto h-4 w-12" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-6 w-20 rounded-full" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-6 w-24 rounded-full" />
                      </TableCell>
                      <TableCell>
                        <Skeleton className="h-6 w-16 rounded-full" />
                      </TableCell>
                      <TableCell className="table-sticky-action-cell text-center">
                        <Skeleton className="mx-auto h-8 w-8" />
                      </TableCell>
                    </TableRow>
                  ))
                ) : filteredAggregateApis.length === 0 ? (
                  <TableRow>
                    <TableCell colSpan={8} className="h-48 text-center">
                      <div className="flex flex-col items-center justify-center gap-2 text-muted-foreground">
                        <ShieldCheck className="h-8 w-8 opacity-20" />
                        <p>
                          {providerFilter === "all"
                            ? t("暂无聚合 API，点击右上角新建")
                            : t("暂无 {provider} 聚合 API", {
                                provider:
                                  AGGREGATE_API_PROVIDER_LABELS[
                                    providerFilter
                                  ] || providerFilter,
                              })}
                        </p>
                      </div>
                    </TableCell>
                  </TableRow>
                ) : (
                  filteredAggregateApis.map((api) => {
                    const revealed = revealedSecrets[api.id];
                    const serverEnabled =
                      String(api.status || "")
                        .trim()
                        .toLowerCase() !== "disabled";
                    const isEnabled = statusOverrides[api.id] ?? serverEnabled;
                    const createdTimeText = formatTsFromSeconds(
                      api.createdAt,
                      t("未知时间"),
                    );
                    const quotaInfo = aggregateQuotaById.get(api.id);
                    const assignedModels = api.modelSlugs.length
                      ? api.modelSlugs
                      : quotaInfo
                        ? Array.from(quotaInfo.models).slice(0, 2)
                        : [];

                    return (
                      <TableRow key={api.id} className="group">
                        <TableCell className="overflow-hidden">
                          <Tooltip>
                            <TooltipTrigger
                              render={<div />}
                              className="block cursor-help text-left"
                            >
                              <div className="grid gap-0.5 overflow-hidden">
                                <span className="block truncate text-xs font-medium text-foreground">
                                  {api.supplierName || "-"}
                                </span>
                                <span className="block truncate font-mono text-[11px] text-muted-foreground">
                                  {api.url}
                                </span>
                                {api.modelOverride ? (
                                  <span className="block truncate font-mono text-[10px] text-muted-foreground/80">
                                    model: {api.modelOverride}
                                  </span>
                                ) : null}
                                {assignedModels.length ? (
                                  <span className="block truncate text-[10px] text-muted-foreground/80">
                                    {t("额度模型")}: {assignedModels.join(", ")}
                                    {!api.modelSlugs.length && quotaInfo && quotaInfo.models.size > 2
                                      ? ` +${quotaInfo.models.size - 2}`
                                      : ""}
                                  </span>
                                ) : (
                                  <span className="block truncate text-[10px] text-muted-foreground/80">
                                    {t("额度模型")}: {t("全部 API 模型")}
                                  </span>
                                )}
                              </div>
                            </TooltipTrigger>
                            <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                              <div className="grid gap-1">
                                <div className="text-[11px] font-medium">
                                  {api.supplierName || "-"}
                                </div>
                                <div className="break-all text-xs">
                                  {api.url}
                                </div>
                                {api.modelOverride ? (
                                  <div className="break-all font-mono text-[11px] opacity-80">
                                    model: {api.modelOverride}
                                  </div>
                                ) : null}
                                <div className="break-all text-[11px] opacity-80">
                                  {t("额度模型")}:{" "}
                                  {api.modelSlugs.length
                                    ? api.modelSlugs.join(", ")
                                    : t("全部 API 可用模型")}
                                </div>
                                <div className="text-[11px] opacity-80">
                                  {t("创建时间")}: {createdTimeText}
                                </div>
                              </div>
                            </TooltipContent>
                          </Tooltip>
                        </TableCell>
                        <TableCell className="text-center">
                          <div className="flex justify-center">
                            <Badge
                              variant="secondary"
                              className="w-fit text-[10px] font-normal"
                            >
                              {AGGREGATE_API_PROVIDER_LABELS[
                                api.providerType
                              ] || api.providerType}
                            </Badge>
                          </div>
                        </TableCell>
                        <TableCell className="overflow-hidden">
                          <div className="flex min-w-0 items-center gap-1.5 overflow-hidden">
                            <Tooltip>
                              <TooltipTrigger
                                render={<div />}
                                className="block min-w-0 cursor-help"
                              >
                                <code className="block min-w-0 flex-1 truncate rounded border border-primary/5 bg-muted/50 px-2 py-1 font-mono text-[10px] leading-4 text-primary">
                                  {revealed
                                    ? secretPreview(revealed)
                                    : loadingSecretId === api.id
                                      ? t("读取中...")
                                      : api.id}
                                </code>
                              </TooltipTrigger>
                              <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                                {revealed ? secretPreview(revealed) : api.id}
                              </TooltipContent>
                            </Tooltip>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-7 w-7 text-muted-foreground hover:text-primary"
                              disabled={!isServiceReady}
                              onClick={() => void toggleSecret(api.id)}
                            >
                              {revealed ? (
                                <EyeOff className="h-3.5 w-3.5" />
                              ) : (
                                <Eye className="h-3.5 w-3.5" />
                              )}
                            </Button>
                            {String(api.authType || "")
                              .trim()
                              .toLowerCase() === "userpass" ? (
                              <DropdownMenu>
                                <DropdownMenuTrigger>
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    className="h-7 w-7 text-muted-foreground hover:text-primary"
                                    render={<span />}
                                    nativeButton={false}
                                    disabled={!isServiceReady}
                                  >
                                    <Copy className="h-3.5 w-3.5" />
                                  </Button>
                                </DropdownMenuTrigger>
                                <DropdownMenuContent align="end">
                                  <DropdownMenuGroup>
                                  <DropdownMenuItem
                                    onClick={() => void copySecret(api.id, "username")}
                                  >
                                    {t("复制用户名")}
                                  </DropdownMenuItem>
                                  <DropdownMenuItem
                                    onClick={() => void copySecret(api.id, "password")}
                                  >
                                    {t("复制密码")}
                                  </DropdownMenuItem>
                                  </DropdownMenuGroup>
                                </DropdownMenuContent>
                              </DropdownMenu>
                            ) : (
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-7 w-7 text-muted-foreground hover:text-primary"
                                disabled={!isServiceReady}
                                onClick={() => void copySecret(api.id, "key")}
                              >
                                <Copy className="h-3.5 w-3.5" />
                              </Button>
                            )}
                          </div>
                        </TableCell>
                        <TableCell className="text-center font-mono text-xs text-muted-foreground">
                          {api.sort}
                        </TableCell>
                        <TableCell className="whitespace-nowrap align-middle">
                          <div className="flex flex-col items-start gap-1">
                            <div className="flex items-center gap-2">
                              {renderTestStatus(api)}
                              <Button
                                variant="outline"
                                size="sm"
                                className="h-7 gap-1.5 px-2 text-xs"
                                disabled={
                                  !isServiceReady || testingApiId === api.id
                                }
                                onClick={() => testMutation.mutate(api.id)}
                              >
                                <RefreshCw
                                  className={
                                    testingApiId === api.id
                                      ? "h-3.5 w-3.5 animate-spin"
                                      : "h-3.5 w-3.5"
                                  }
                                />
                                {t("测试")}
                              </Button>
                            </div>
                          </div>
                          {api.lastTestAt ? (
                            <p className="mt-1 text-[10px] text-muted-foreground">
                              {formatTsFromSeconds(api.lastTestAt, t("未知时间"))}
                            </p>
                          ) : null}
                          {api.lastTestStatus === "failed" && api.lastTestError ? (
                            <Tooltip>
                              <TooltipTrigger
                                render={<div />}
                                className="mt-1 block max-w-full cursor-help text-left"
                              >
                                <p className="max-w-[220px] truncate text-[10px] text-red-500/90">
                                  {api.lastTestError}
                                </p>
                              </TooltipTrigger>
                              <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                                {api.lastTestError}
                              </TooltipContent>
                            </Tooltip>
                          ) : null}
                        </TableCell>
                        <TableCell className="whitespace-nowrap align-middle">
                          <div className="flex items-center gap-2">
                            {renderBalanceStatus(api)}
                            <Tooltip>
                              <TooltipTrigger render={<span />} className="inline-flex">
                                <Button
                                  variant="outline"
                                  size="icon"
                                  className="h-7 w-7"
                                  disabled={
                                    !isServiceReady ||
                                    !api.balanceQueryEnabled ||
                                    refreshingBalanceId === api.id ||
                                    refreshingBalances
                                  }
                                  onClick={() =>
                                    refreshBalanceMutation.mutate(api.id)
                                  }
                                >
                                  <RefreshCw
                                    className={
                                      refreshingBalanceId === api.id
                                        ? "h-3.5 w-3.5 animate-spin"
                                        : "h-3.5 w-3.5"
                                    }
                                  />
                                </Button>
                              </TooltipTrigger>
                              <TooltipContent>{t("刷新余额")}</TooltipContent>
                            </Tooltip>
                          </div>
                          {api.lastBalanceAt ? (
                            <p className="mt-1 text-[10px] text-muted-foreground">
                              {formatTsFromSeconds(api.lastBalanceAt, t("未知时间"))}
                            </p>
                          ) : null}
                          {quotaInfo?.tokens != null ? (
                            <p className="mt-1 max-w-[180px] truncate text-[10px] text-muted-foreground">
                              {t("折算")}{" "}
                              {formatCompactNumber(quotaInfo.tokens, "0.00", 2, true)}{" "}
                              token
                              {quotaInfo.model ? ` · ${quotaInfo.model}` : ""}
                            </p>
                          ) : null}
                          {api.lastBalanceStatus === "failed" && api.lastBalanceError ? (
                            <Tooltip>
                              <TooltipTrigger
                                render={<div />}
                                className="mt-1 block max-w-full cursor-help text-left"
                              >
                                <p className="max-w-[180px] truncate text-[10px] text-red-500/90">
                                  {api.lastBalanceError}
                                </p>
                              </TooltipTrigger>
                              <TooltipContent className="max-w-sm whitespace-pre-wrap break-words">
                                {api.lastBalanceError}
                              </TooltipContent>
                            </Tooltip>
                          ) : null}
                        </TableCell>
                        <TableCell className="align-middle pr-4">
                          <div className="flex items-center justify-end gap-2">
                            <Switch
                              className="scale-75"
                              checked={isEnabled}
                              disabled={
                                !isServiceReady || togglingApiId === api.id
                              }
                              onCheckedChange={(enabled) =>
                                toggleStatusMutation.mutate({ api, enabled })
                              }
                            />
                            <span className="text-[10px] font-medium text-muted-foreground">
                              {isEnabled ? t("启用") : t("禁用")}
                            </span>
                          </div>
                        </TableCell>
                        <TableCell className="table-sticky-action-cell">
                          <div className="table-action-cell gap-1">
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                              disabled={!isServiceReady}
                              onClick={() => setModelPoolApiId(api.id)}
                              title={t("模型池")}
                            >
                              <Database className="h-4 w-4" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                              disabled={!isServiceReady}
                              onClick={() => openEditModal(api.id)}
                              title={t("编辑配置")}
                            >
                              <Settings2 className="h-4 w-4" />
                            </Button>
                            <DropdownMenu>
                              <DropdownMenuTrigger>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-8 w-8"
                                  render={<span />}
                                  nativeButton={false}
                                  disabled={!isServiceReady}
                                >
                                  <MoreVertical className="h-4 w-4" />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end">
                                  <DropdownMenuGroup>
                                <DropdownMenuItem
                                  className="gap-2"
                                  disabled={!isServiceReady}
                                  onClick={() => openEditModal(api.id)}
                                >
                                  {t("编辑聚合 API")}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  className="gap-2"
                                  disabled={
                                    !isServiceReady || prioritizeMutation.isPending
                                  }
                                  onClick={() => prioritizeMutation.mutate(api)}
                                >
                                  <ArrowUp className="h-4 w-4" /> {t("设为优先")}
                                </DropdownMenuItem>
                                <DropdownMenuItem
                                  className="gap-2 text-red-500"
                                  disabled={!isServiceReady}
                                  onClick={() => setDeleteId(api.id)}
                                >
                                  <Trash2 className="h-4 w-4" /> {t("删除聚合 API")}
                                </DropdownMenuItem>
                                </DropdownMenuGroup>
                              </DropdownMenuContent>
                            </DropdownMenu>
                          </div>
                        </TableCell>
                      </TableRow>
                    );
                  })
                )}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      </div>

      <AggregateApiModal
        open={modalOpen}
        onOpenChange={setModalOpen}
        aggregateApi={editingApi}
        defaultSort={defaultCreateSort}
      />

      <Dialog
        open={Boolean(modelPoolApiId)}
        onOpenChange={(open) => {
          if (!open) setModelPoolApiId(null);
        }}
      >
        {modelPoolApi ? (
          <DialogContent className="max-h-[86vh] max-w-5xl overflow-hidden p-0">
            <DialogHeader className="border-b px-6 py-5">
              <DialogTitle>{t("模型池配置")}</DialogTitle>
              <DialogDescription>
                {modelPoolApi.supplierName || modelPoolApi.url} ·{" "}
                {AGGREGATE_API_PROVIDER_LABELS[modelPoolProviderType] ||
                  modelPoolProviderType}
              </DialogDescription>
            </DialogHeader>

            <div className="max-h-[calc(86vh-92px)] overflow-y-auto px-6 py-5">
              <div className="grid gap-3 md:grid-cols-3">
                <div className="rounded-lg border bg-muted/20 p-3">
                  <p className="text-xs text-muted-foreground">{t("供应商标识")}</p>
                  <p className="mt-1 truncate font-mono text-xs">
                    {modelPoolSupplierKey}
                  </p>
                </div>
                <div className="rounded-lg border bg-muted/20 p-3">
                  <p className="text-xs text-muted-foreground">{t("当前模型池")}</p>
                  <p className="mt-1 text-lg font-semibold">{sourceModels.length}</p>
                </div>
                <div className="rounded-lg border bg-muted/20 p-3">
                  <p className="text-xs text-muted-foreground">{t("供应商模板")}</p>
                  <p className="mt-1 text-lg font-semibold">{supplierModels.length}</p>
                </div>
              </div>

              <Tabs defaultValue="pool" className="mt-5 gap-4">
                <TabsList className="grid w-full grid-cols-2 sm:w-[280px]">
                  <TabsTrigger value="pool">{t("当前模型池")}</TabsTrigger>
                  <TabsTrigger value="templates">{t("供应商模板")}</TabsTrigger>
                </TabsList>

                <TabsContent value="pool" className="space-y-4">
                  <div className="flex flex-wrap items-center gap-2">
                    <Button
                      variant="outline"
                      className="gap-2"
                      disabled={
                        !isServiceReady ||
                        !modelPoolApiActive ||
                        syncModelPoolMutation.isPending ||
                        !modelPoolApiId
                      }
                      onClick={() => {
                        if (modelPoolApiId) syncModelPoolMutation.mutate(modelPoolApiId);
                      }}
                    >
                      <RefreshCw
                        className={
                          syncModelPoolMutation.isPending
                            ? "h-4 w-4 animate-spin"
                            : "h-4 w-4"
                        }
                      />
                      {t("同步 /models")}
                    </Button>
                    <Button
                      variant="outline"
                      className="gap-2"
                      disabled={
                        !isServiceReady ||
                        !modelPoolApiActive ||
                        importSupplierModelsMutation.isPending ||
                        supplierModels.length === 0 ||
                        !modelPoolApiId
                      }
                      onClick={() => {
                        if (!modelPoolApiId) return;
                        importSupplierModelsMutation.mutate({
                          apiId: modelPoolApiId,
                          supplierKey: modelPoolSupplierKey,
                          providerType: modelPoolProviderType,
                        });
                      }}
                    >
                      <Download className="h-4 w-4" />
                      {t("从供应商模板导入")}
                    </Button>
                  </div>
                  {!modelPoolApiActive ? (
                    <p className="text-xs text-muted-foreground">
                      {t("当前聚合 API 未启用，启用后才能同步或导入模型。")}
                    </p>
                  ) : null}

                  <div className="grid gap-3 rounded-lg border bg-muted/10 p-3 md:grid-cols-[1fr_1fr_auto]">
                    <div className="grid gap-1.5">
                      <Label htmlFor="aggregate-source-model">
                        {t("上游模型名")}
                      </Label>
                      <Input
                        id="aggregate-source-model"
                        value={sourceModelDraft.upstreamModel}
                        placeholder="gpt-4o"
                        onChange={(event) =>
                          setSourceModelDraft((current) => ({
                            ...current,
                            upstreamModel: event.target.value,
                          }))
                        }
                      />
                    </div>
                    <div className="grid gap-1.5">
                      <Label htmlFor="aggregate-source-display">
                        {t("显示名称")}
                      </Label>
                      <Input
                        id="aggregate-source-display"
                        value={sourceModelDraft.displayName}
                        placeholder={sourceModelDraftUpstream || t("可选")}
                        onChange={(event) =>
                          setSourceModelDraft((current) => ({
                            ...current,
                            displayName: event.target.value,
                          }))
                        }
                      />
                    </div>
                    <div className="flex items-end">
                      <Button
                        className="gap-2"
                        disabled={
                          !isServiceReady ||
                          !modelPoolApiId ||
                          !sourceModelDraftUpstream ||
                          saveSourceModelMutation.isPending
                        }
                        onClick={() => {
                          if (!modelPoolApiId || !sourceModelDraftUpstream) return;
                          saveSourceModelMutation.mutate({
                            apiId: modelPoolApiId,
                            upstreamModel: sourceModelDraftUpstream,
                            displayName: sourceModelDraft.displayName.trim() || null,
                          });
                        }}
                      >
                        <Save className="h-4 w-4" />
                        {t("添加到模型池")}
                      </Button>
                    </div>
                  </div>

                  <div className="rounded-lg border">
                    <div className="grid grid-cols-[1fr_120px_130px] border-b bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
                      <span>{t("上游模型")}</span>
                      <span>{t("来源")}</span>
                      <span className="text-right">{t("最近同步")}</span>
                    </div>
                    <div className="max-h-[300px] overflow-y-auto">
                      {modelRoutingLoading ? (
                        <div className="space-y-2 p-3">
                          <Skeleton className="h-9 w-full" />
                          <Skeleton className="h-9 w-full" />
                        </div>
                      ) : sourceModels.length === 0 ? (
                        <div className="p-6 text-center text-sm text-muted-foreground">
                          {t("暂无模型，先同步 /models 或手动添加")}
                        </div>
                      ) : (
                        sourceModels.map((model) => (
                          <div
                            key={sourceModelKey(model)}
                            className="grid grid-cols-[1fr_120px_130px] items-center gap-3 border-b px-3 py-2 last:border-b-0"
                          >
                            <div className="min-w-0">
                              <p className="truncate font-mono text-xs">
                                {model.upstreamModel}
                              </p>
                              {model.displayName ? (
                                <p className="truncate text-xs text-muted-foreground">
                                  {model.displayName}
                                </p>
                              ) : null}
                            </div>
                            <Badge variant="secondary" className="w-fit">
                              {model.discoveryKind === "template"
                                ? t("模板")
                                : model.discoveryKind === "manual"
                                  ? t("手动")
                                  : t("同步")}
                            </Badge>
                            <span className="truncate text-right text-xs text-muted-foreground">
                              {model.lastSyncedAt
                                ? formatTsFromSeconds(model.lastSyncedAt, t("未知时间"))
                                : "-"}
                            </span>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                </TabsContent>

                <TabsContent value="templates" className="space-y-4">
                  <div className="grid gap-3 rounded-lg border bg-muted/10 p-3 md:grid-cols-[1fr_1fr_auto]">
                    <div className="grid gap-1.5">
                      <Label htmlFor="supplier-template-model">
                        {t("供应商模型名")}
                      </Label>
                      <Input
                        id="supplier-template-model"
                        value={supplierModelDraft.upstreamModel}
                        placeholder="gpt-4o"
                        onChange={(event) =>
                          setSupplierModelDraft((current) => ({
                            ...current,
                            upstreamModel: event.target.value,
                          }))
                        }
                      />
                    </div>
                    <div className="grid gap-1.5">
                      <Label htmlFor="supplier-template-display">
                        {t("显示名称")}
                      </Label>
                      <Input
                        id="supplier-template-display"
                        value={supplierModelDraft.displayName}
                        placeholder={supplierModelDraftUpstream || t("可选")}
                        onChange={(event) =>
                          setSupplierModelDraft((current) => ({
                            ...current,
                            displayName: event.target.value,
                          }))
                        }
                      />
                    </div>
                    <div className="flex items-end">
                      <Button
                        className="gap-2"
                        disabled={
                          !isServiceReady ||
                          !modelPoolSupplierKey ||
                          !supplierModelDraftUpstream ||
                          saveSupplierModelMutation.isPending
                        }
                        onClick={() => {
                          if (!modelPoolSupplierKey || !supplierModelDraftUpstream) {
                            return;
                          }
                          saveSupplierModelMutation.mutate({
                            supplierKey: modelPoolSupplierKey,
                            providerType: modelPoolProviderType,
                            upstreamModel: supplierModelDraftUpstream,
                            displayName: supplierModelDraft.displayName.trim() || null,
                          });
                        }}
                      >
                        <Save className="h-4 w-4" />
                        {t("保存模板")}
                      </Button>
                    </div>
                  </div>

                  <div className="rounded-lg border">
                    <div className="grid grid-cols-[1fr_100px_116px] border-b bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
                      <span>{t("供应商模型")}</span>
                      <span>{t("状态")}</span>
                      <span className="text-right">{t("操作")}</span>
                    </div>
                    <div className="max-h-[300px] overflow-y-auto">
                      {supplierModelsLoading ? (
                        <div className="space-y-2 p-3">
                          <Skeleton className="h-9 w-full" />
                          <Skeleton className="h-9 w-full" />
                        </div>
                      ) : supplierModels.length === 0 ? (
                        <div className="p-6 text-center text-sm text-muted-foreground">
                          {t("暂无供应商模板，可先手动维护模型名")}
                        </div>
                      ) : (
                        supplierModels.map((model) => {
                          const inPool = sourceModelKeys.has(model.upstreamModel);
                          return (
                            <div
                              key={sourceModelKey(model)}
                              className="grid grid-cols-[1fr_100px_116px] items-center gap-3 border-b px-3 py-2 last:border-b-0"
                            >
                              <div className="min-w-0">
                                <p className="truncate font-mono text-xs">
                                  {model.upstreamModel}
                                </p>
                                {model.displayName ? (
                                  <p className="truncate text-xs text-muted-foreground">
                                    {model.displayName}
                                  </p>
                                ) : null}
                              </div>
                              <Badge
                                variant={model.status === "available" ? "secondary" : "outline"}
                                className="w-fit"
                              >
                                {model.status === "available" ? t("可用") : t("禁用")}
                              </Badge>
                              <div className="flex justify-end gap-1">
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-8 w-8"
                                  disabled={
                                    !isServiceReady ||
                                    inPool ||
                                    !modelPoolApiId ||
                                    saveSourceModelMutation.isPending
                                  }
                                  title={inPool ? t("已在模型池") : t("导入到当前模型池")}
                                  onClick={() => {
                                    if (!modelPoolApiId) return;
                                    saveSourceModelMutation.mutate({
                                      apiId: modelPoolApiId,
                                      upstreamModel: model.upstreamModel,
                                      displayName: model.displayName,
                                    });
                                  }}
                                >
                                  <Download className="h-4 w-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-8 w-8 text-muted-foreground hover:text-red-500"
                                  disabled={
                                    !isServiceReady ||
                                    deleteSupplierModelMutation.isPending
                                  }
                                  title={t("删除模板")}
                                  onClick={() =>
                                    deleteSupplierModelMutation.mutate({
                                      supplierKey: model.supplierKey,
                                      providerType: model.providerType,
                                      upstreamModel: model.upstreamModel,
                                    })
                                  }
                                >
                                  <Trash2 className="h-4 w-4" />
                                </Button>
                              </div>
                            </div>
                          );
                        })
                      )}
                    </div>
                  </div>
                </TabsContent>
              </Tabs>
            </div>
          </DialogContent>
        ) : null}
      </Dialog>

      <ConfirmDialog
        open={Boolean(deleteId)}
        onOpenChange={(open) => !open && setDeleteId(null)}
        title={t("删除聚合 API")}
        description={t("删除聚合 API")}
        confirmText={t("删除")}
        cancelText={t("取消")}
        onConfirm={() => {
          if (!deleteId) return;
          deleteMutation.mutate(deleteId);
          setDeleteId(null);
        }}
      />
    </div>
  );
}
