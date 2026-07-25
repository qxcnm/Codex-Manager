"use client";

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Download,
  Info,
  Play,
  RefreshCw,
  X,
  Trash2,
  ToggleLeft,
  ToggleRight,
} from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import {
  Empty,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { appClient } from "@/lib/api/app-client";
import { pluginClient } from "@/lib/api/plugin-client";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";
import { formatLocalDateTimeFromSeconds } from "@/lib/utils/time";
import {
  InstalledPluginSummary,
  PluginCatalogEntry,
  PluginRunLogSummary,
  PluginTaskSummary,
} from "@/types";
import {
  PageHeader,
  PageWorkspace,
} from "@/components/layout/page-workspace";

type SelectedPluginDetail =
  | { kind: "catalog"; pluginId: string }
  | { kind: "installed"; pluginId: string }
  | null;

type PluginViewFilter = "installed" | "not-installed" | "update";
type TranslateFn = (message: string, values?: Record<string, string | number>) => string;

const MARKET_MODE_OPTIONS = [
  {
    value: "builtin",
    label: "内置精选",
    description: "默认使用官方精选插件，适合开箱即用。",
  },
  {
    value: "custom",
    label: "自定义源",
    description: "接入你自己的远程 JSON 市场源。",
  },
] as const;

const PLUGIN_VIEW_FILTER_OPTIONS: Array<{
  value: PluginViewFilter;
  label: string;
}> = [
  { value: "installed", label: "已安装" },
  { value: "not-installed", label: "未安装" },
  { value: "update", label: "更新" },
];

const EMPTY_PLUGIN_CATALOG_ITEMS: PluginCatalogEntry[] = [];
const EMPTY_INSTALLED_PLUGIN_ITEMS: InstalledPluginSummary[] = [];

/**
 * 函数 `normalizeMarketMode`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizeMarketMode(value: string | null | undefined) {
  return String(value || "")
    .trim()
    .toLowerCase() === "custom"
    ? "custom"
    : "builtin";
}

/**
 * 函数 `formatPermissionLabel`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - permission: 参数 permission
 *
 * # 返回
 * 返回函数执行结果
 */
function formatPermissionLabel(permission: string, t: TranslateFn) {
  switch (permission) {
    case "accounts:cleanup":
      return t("清理封禁账号");
    case "settings:read":
      return t("读取设置");
    case "network":
      return t("网络访问");
    default:
      return permission;
  }
}

/**
 * 函数 `formatMarketCategory`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - category: 参数 category
 *
 * # 返回
 * 返回函数执行结果
 */
function formatMarketCategory(category: string | null | undefined, t: TranslateFn) {
  switch (category) {
    case "official":
      return t("官方精选");
    case "private":
      return t("企业私有");
    case "community":
      return t("社区插件");
    default:
      return category || "";
  }
}

/**
 * 函数 `formatRuntimeKind`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - runtimeKind: 参数 runtimeKind
 *
 * # 返回
 * 返回函数执行结果
 */
function formatRuntimeKind(runtimeKind: string | null | undefined) {
  switch (runtimeKind) {
    case "rhai":
      return "Rhai";
    case "wasm":
      return "WASM";
    default:
      return runtimeKind || "";
  }
}

/**
 * 函数 `compareVersionStrings`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - left: 参数 left
 * - right: 参数 right
 *
 * # 返回
 * 返回函数执行结果
 */
function compareVersionStrings(left: string, right: string) {
  const leftParts = left
    .split(/[^0-9]+/)
    .filter(Boolean)
    .map((item) => Number(item));
  const rightParts = right
    .split(/[^0-9]+/)
    .filter(Boolean)
    .map((item) => Number(item));
  const length = Math.max(leftParts.length, rightParts.length);
  for (let index = 0; index < length; index += 1) {
    const leftValue = leftParts[index] ?? 0;
    const rightValue = rightParts[index] ?? 0;
    if (leftValue !== rightValue) {
      return leftValue - rightValue;
    }
  }
  return left.localeCompare(right, undefined, {
    numeric: true,
    sensitivity: "base",
  });
}

/**
 * 函数 `PermissionBadge`
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
function PermissionBadge({ permission, t }: { permission: string; t: TranslateFn }) {
  return (
    <Badge variant="secondary" className="mr-1.5 mb-1">
      {formatPermissionLabel(permission, t)}
    </Badge>
  );
}

/**
 * 函数 `StatusBadge`
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
function StatusBadge({ status, t }: { status: string; t: TranslateFn }) {
  const normalized = status.toLowerCase();
  const label =
    normalized === "enabled"
      ? t("启用中")
      : normalized === "broken"
        ? t("异常")
        : t("未知");
  const toneClass =
    normalized === "enabled"
      ? "border-emerald-500/20 bg-emerald-500/10 text-emerald-600"
      : normalized === "broken"
        ? "border-red-500/20 bg-red-500/10 text-red-600"
        : "border-amber-500/20 bg-amber-500/10 text-amber-600";
  return <Badge className={toneClass}>{label}</Badge>;
}

/**
 * 函数 `formatDuration`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
function formatDuration(value: number | null): string {
  if (value == null) return "-";
  if (value >= 10_000) return `${Math.round(value / 1000)}s`;
  if (value >= 1000) return `${(value / 1000).toFixed(1).replace(/\.0$/, "")}s`;
  return `${Math.round(value)}ms`;
}

/**
 * 函数 `formatTimestamp`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
function formatTimestamp(value: number | null): string {
  return formatLocalDateTimeFromSeconds(value, "-");
}

/**
 * 函数 `PluginCard`
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
function PluginCard({
  item,
  onOpenDetails,
  onInstall,
  t,
}: {
  item: PluginCatalogEntry;
  onOpenDetails: (entry: PluginCatalogEntry) => void;
  onInstall: (entry: PluginCatalogEntry) => void;
  t: TranslateFn;
}) {
  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader className="space-y-2 pb-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <CardTitle className="text-base">{item.name}</CardTitle>
            <CardDescription className="mt-1 line-clamp-1">
              {item.description || t("暂无描述")}
            </CardDescription>
          </div>
          <Badge variant="secondary">{item.version}</Badge>
        </div>
        <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
          {item.author ? (
            <span>
              {t("作者")}：{item.author}
            </span>
          ) : null}
          <span>
            {t("权限")} {item.permissions.length}
          </span>
          <span>
            {t("任务")} {item.tasks.length}
          </span>
          {item.category ? (
            <Badge variant="outline">
              {formatMarketCategory(item.category, t)}
            </Badge>
          ) : null}
          <Badge variant="outline">{formatRuntimeKind(item.runtimeKind)}</Badge>
        </div>
      </CardHeader>
      <CardContent className="flex items-center justify-between gap-3 pt-0">
        <div className="text-xs text-muted-foreground">
          <span>
            {item.sourceUrl === "builtin://codexmanager"
              ? t("来源：内置精选市场")
              : item.sourceUrl
                ? t("来源：{source}", { source: item.sourceUrl })
                : t("内置市场")}
          </span>
        </div>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenDetails(item)}
          >
            <Info className="mr-1.5 h-4 w-4" />
            {t("详情")}
          </Button>
          <Button size="sm" onClick={() => onInstall(item)} className="gap-2">
            <Download className="h-4 w-4" />
            {t("安装")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

/**
 * 函数 `InstalledPluginCard`
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
function InstalledPluginCard({
  item,
  updateVersion,
  onOpenDetails,
  onUpdate,
  onEnable,
  onDisable,
  t,
}: {
  item: InstalledPluginSummary;
  updateVersion?: string | null;
  onOpenDetails: (item: InstalledPluginSummary) => void;
  onUpdate?: (pluginId: string) => void;
  onEnable: (pluginId: string) => void;
  onDisable: (pluginId: string) => void;
  t: TranslateFn;
}) {
  return (
    <Card className="glass-card mission-panel shadow-sm">
      <CardHeader className="space-y-2 pb-3">
        <div className="flex items-start justify-between gap-3">
          <div className="min-w-0">
            <CardTitle className="text-base">{item.name}</CardTitle>
            <CardDescription className="mt-1 line-clamp-1">
              {item.description || t("暂无描述")}
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="secondary">{item.version}</Badge>
            {updateVersion ? (
              <Badge className="border-primary/20 bg-primary/10 text-primary">
                {t("可更新")} {updateVersion}
              </Badge>
            ) : null}
            <Badge variant="outline">{t("已安装")}</Badge>
            <StatusBadge status={item.status} t={t} />
          </div>
        </div>
        <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
          {item.author ? (
            <span>
              {t("作者")}：{item.author}
            </span>
          ) : null}
          <span>
            {t("权限")} {item.permissions.length}
          </span>
          <span>
            {t("任务")} {item.enabledTaskCount}/{item.taskCount}
          </span>
          {item.category ? (
            <Badge variant="outline">
              {formatMarketCategory(item.category, t)}
            </Badge>
          ) : null}
          <Badge variant="outline">{formatRuntimeKind(item.runtimeKind)}</Badge>
        </div>
      </CardHeader>
      <CardContent className="flex items-center justify-between gap-3 pt-0">
        <div className="text-xs text-muted-foreground">
          <span>
            {item.sourceUrl === "builtin://codexmanager"
              ? t("来源：内置精选市场")
              : item.sourceUrl
                ? t("来源：{source}", { source: item.sourceUrl })
                : t("内置安装")}
          </span>
        </div>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenDetails(item)}
          >
            <Info className="mr-1.5 h-4 w-4" />
            {t("详情")}
          </Button>
          {updateVersion && onUpdate ? (
            <Button
              size="sm"
              onClick={() => onUpdate(item.pluginId)}
              className="gap-2"
            >
              <RefreshCw className="h-4 w-4" />
              {t("更新")}
            </Button>
          ) : item.status === "enabled" ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => onDisable(item.pluginId)}
            >
              <ToggleLeft className="mr-1.5 h-4 w-4" />
              {t("停用")}
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={() => onEnable(item.pluginId)}
            >
              <ToggleRight className="mr-1.5 h-4 w-4" />
              {t("启用")}
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

export default function PluginsPage() {
  const { t } = useI18n();
  const serviceReady = useAppStore((state) => state.serviceStatus.connected);
  const isPageActive = useDesktopPageActive("/plugins/");
  const isActivationReady = useDeferredDesktopActivation(serviceReady);
  usePageTransitionReady("/plugins/", !serviceReady);
  const queryClient = useQueryClient();
  const [marketModeDraft, setMarketModeDraft] = useState<string | null>(
    "builtin",
  );
  const [pluginViewFilter, setPluginViewFilter] =
    useState<PluginViewFilter>("installed");
  const [sourceUrlDraft, setSourceUrlDraft] = useState<string | null>(null);
  const [selectedPlugin, setSelectedPlugin] =
    useState<SelectedPluginDetail>(null);
  const [pendingUninstallPlugin, setPendingUninstallPlugin] =
    useState<InstalledPluginSummary | null>(null);
  const [taskIntervalDrafts, setTaskIntervalDrafts] = useState<
    Record<string, string>
  >({});

  const settingsQuery = useQuery({
    queryKey: ["plugin-settings"],
    queryFn: () => appClient.getSettings(),
    enabled: isPageActive && isActivationReady,
  });
  const marketMode =
    marketModeDraft ??
    normalizeMarketMode(settingsQuery.data?.pluginMarketMode);
  const sourceUrl =
    sourceUrlDraft ?? (settingsQuery.data?.pluginMarketSourceUrl || "");

  const catalogQuery = useQuery({
    queryKey: ["plugin-catalog", marketMode, sourceUrl],
    queryFn: () =>
      pluginClient.getCatalog({
        marketMode,
        sourceUrl: marketMode === "custom" ? sourceUrl || undefined : undefined,
      }),
    enabled: isPageActive && isActivationReady,
  });

  const installedQuery = useQuery({
    queryKey: ["plugin-installed"],
    queryFn: () => pluginClient.listInstalled(),
    enabled: isPageActive && isActivationReady,
  });

  const tasksQuery = useQuery({
    queryKey: ["plugin-tasks"],
    queryFn: () => pluginClient.listTasks(),
    enabled: isPageActive && isActivationReady,
  });

  const logsQuery = useQuery({
    queryKey: ["plugin-logs"],
    queryFn: () => pluginClient.listLogs({ limit: 20 }),
    enabled: isPageActive && isActivationReady,
  });

  const saveSourceMutation = useMutation({
    mutationFn: async () =>
      appClient.setSettings({
        pluginMarketMode: normalizeMarketMode(marketMode),
        pluginMarketSourceUrl: sourceUrl,
      }),
    onSuccess: (settings) => {
      queryClient.setQueryData(["plugin-settings"], settings);
      setMarketModeDraft(null);
      setSourceUrlDraft(null);
      toast.success(t("市场源已保存"));
      void queryClient.invalidateQueries({ queryKey: ["plugin-catalog"] });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : t("保存市场源失败"));
    },
  });

  const installMutation = useMutation({
    mutationFn: (entry: PluginCatalogEntry) => pluginClient.install(entry),
    onSuccess: () => {
      setPluginViewFilter("installed");
      toast.success(t("插件已安装"));
      void queryClient.invalidateQueries({ queryKey: ["plugin-installed"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-logs"] });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : t("安装失败"));
    },
  });

  const updateMutation = useMutation({
    mutationFn: (payload: { pluginId: string; sourceUrl?: string | null }) =>
      pluginClient.update(payload.pluginId, payload.sourceUrl || undefined),
    onSuccess: () => {
      toast.success(t("插件已更新"));
      void queryClient.invalidateQueries({ queryKey: ["plugin-catalog"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-installed"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-logs"] });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : t("更新失败"));
    },
  });

  const toggleMutation = useMutation({
    mutationFn: async (payload: { pluginId: string; enabled: boolean }) =>
      payload.enabled
        ? pluginClient.enable(payload.pluginId)
        : pluginClient.disable(payload.pluginId),
    onSuccess: () => {
      toast.success(t("插件状态已更新"));
      void queryClient.invalidateQueries({ queryKey: ["plugin-installed"] });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : t("更新失败"));
    },
  });

  const uninstallMutation = useMutation({
    mutationFn: (pluginId: string) => pluginClient.uninstall(pluginId),
    onSuccess: () => {
      toast.success(t("插件已卸载"));
      void queryClient.invalidateQueries({ queryKey: ["plugin-installed"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-logs"] });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : t("卸载失败"));
    },
  });

  const runTaskMutation = useMutation({
    mutationFn: (taskId: string) => pluginClient.runTask(taskId),
    onSuccess: () => {
      toast.success(t("任务已执行"));
      void queryClient.invalidateQueries({ queryKey: ["plugin-installed"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-logs"] });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : t("运行失败"));
    },
  });

  const updateTaskMutation = useMutation({
    mutationFn: (payload: { taskId: string; intervalSeconds: number }) =>
      pluginClient.updateTask(payload.taskId, payload.intervalSeconds),
    onSuccess: () => {
      toast.success(t("任务间隔已更新"));
      void queryClient.invalidateQueries({ queryKey: ["plugin-installed"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-tasks"] });
      void queryClient.invalidateQueries({ queryKey: ["plugin-logs"] });
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : t("更新任务失败"));
    },
  });

  const tasksByPluginId = useMemo(() => {
    const map = new Map<string, PluginTaskSummary[]>();
    for (const task of tasksQuery.data || []) {
      const items = map.get(task.pluginId) || [];
      items.push(task);
      map.set(task.pluginId, items);
    }
    return map;
  }, [tasksQuery.data]);

  const logsByPluginId = useMemo(() => {
    const map = new Map<string, PluginRunLogSummary[]>();
    for (const log of logsQuery.data || []) {
      const items = map.get(log.pluginId) || [];
      items.push(log);
      map.set(log.pluginId, items);
    }
    return map;
  }, [logsQuery.data]);

  const catalogItems = catalogQuery.data?.items ?? EMPTY_PLUGIN_CATALOG_ITEMS;
  const installedItems = installedQuery.data ?? EMPTY_INSTALLED_PLUGIN_ITEMS;
  const catalogById = useMemo(
    () => new Map(catalogItems.map((item) => [item.id, item])),
    [catalogItems],
  );
  const installedById = useMemo(
    () => new Map(installedItems.map((item) => [item.pluginId, item])),
    [installedItems],
  );
  const installedPluginIds = useMemo(
    () => new Set(installedItems.map((item) => item.pluginId)),
    [installedItems],
  );
  const updatableVersionByPluginId = useMemo(() => {
    const map = new Map<string, string>();
    for (const item of installedItems) {
      const catalogEntry = catalogById.get(item.pluginId);
      if (
        catalogEntry &&
        compareVersionStrings(catalogEntry.version, item.version) > 0
      ) {
        map.set(item.pluginId, catalogEntry.version);
      }
    }
    return map;
  }, [catalogById, installedItems]);
  const notInstalledCatalogItems = useMemo(
    () => catalogItems.filter((item) => !installedPluginIds.has(item.id)),
    [catalogItems, installedPluginIds],
  );
  const updatableInstalledItems = useMemo(
    () =>
      installedItems.filter((item) => updatableVersionByPluginId.has(item.pluginId)),
    [installedItems, updatableVersionByPluginId],
  );
  const selectedCatalogItem =
    selectedPlugin ? catalogById.get(selectedPlugin.pluginId) || null : null;
  const selectedInstalledItem =
    selectedPlugin ? installedById.get(selectedPlugin.pluginId) || null : null;
  const selectedTasks = selectedPlugin
    ? tasksByPluginId.get(selectedPlugin.pluginId) || []
    : [];
  const selectedLogs = selectedPlugin
    ? logsByPluginId.get(selectedPlugin.pluginId) || []
    : [];
  const selectedDetail = selectedInstalledItem || selectedCatalogItem;
  const selectedUpdateVersion = selectedInstalledItem
    ? updatableVersionByPluginId.get(selectedInstalledItem.pluginId) || null
    : null;

  return (
    <PageWorkspace>
      <PageHeader
        eyebrow={t("Plugin workspace")}
        title={t("插件中心")}
        description={t("内置精选优先，自定义源按需补充，脚本能力继续由 Rhai 承担。")}
        meta={
          <>
            <Badge variant="secondary" className="rounded-md px-2.5">
              {t("已安装")} {installedItems.length}
            </Badge>
            <Badge variant="secondary" className="rounded-md px-2.5">
              {t("可更新")} {updatableInstalledItems.length}
            </Badge>
          </>
        }
      />

      <Card className="glass-card mission-panel shadow-sm">
        <CardHeader>
          <CardTitle>{t("市场层")}</CardTitle>
          <CardDescription>
            {t("只保留内置精选和自定义源两种模式。内置模式完全隔离自定义 URL，自定义模式才显示并加载远程 JSON 市场。")}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-3 md:grid-cols-2">
            {MARKET_MODE_OPTIONS.map((option) => (
              <Button
                key={option.value}
                type="button"
                variant="outline"
                onClick={() =>
                  setMarketModeDraft(normalizeMarketMode(option.value))
                }
                className={cn(
                  "h-auto w-full min-w-0 justify-start whitespace-normal rounded-xl p-4 text-left transition-all",
                  marketMode === option.value
                    ? "border-primary/40 bg-primary/10 shadow-sm"
                    : "border-border/60 bg-background/40 hover:bg-background/70",
                )}
              >
                <div className="flex min-w-0 items-center justify-between gap-2">
                  <div className="min-w-0 font-medium">{t(option.label)}</div>
                  {marketMode === option.value ? <Badge>{t("已选")}</Badge> : null}
                </div>
                <div className="mt-1 text-xs leading-5 text-muted-foreground">
                  {t(option.description)}
                </div>
              </Button>
            ))}
          </div>
          {marketMode === "custom" ? (
            <>
              <div className="flex flex-col gap-3 md:flex-row md:items-center">
                <Input
                  value={sourceUrl}
                  onChange={(event) => setSourceUrlDraft(event.target.value)}
                  placeholder="https://example.com/plugin-market.json"
                  className="md:flex-1"
                />
                <div className="flex gap-2">
                  <Button
                    onClick={() => saveSourceMutation.mutate()}
                    disabled={saveSourceMutation.isPending}
                  >
                    {t("保存")}
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() =>
                      void queryClient.invalidateQueries({
                        queryKey: ["plugin-catalog"],
                      })
                    }
                  >
                    <RefreshCw className="mr-2 h-4 w-4" />
                    {t("刷新")}
                  </Button>
                </div>
              </div>
              <div className="rounded-xl border border-dashed border-border/60 bg-muted/20 p-4 text-xs text-muted-foreground">
                {catalogQuery.data?.sourceUrl
                  ? t("当前使用自定义源：{sourceUrl}", {
                      sourceUrl: catalogQuery.data.sourceUrl,
                    })
                  : t("当前使用自定义源，适合接入你自己的 JSON 市场文件。")}
              </div>
            </>
          ) : (
            <div className="rounded-xl border border-dashed border-border/60 bg-muted/20 p-4 text-xs text-muted-foreground">
              {t("当前使用内置精选市场，默认只显示官方内置脚本插件。")}
            </div>
          )}
        </CardContent>
      </Card>

      <Card className="glass-card mission-panel shadow-sm">
        <CardHeader className="space-y-4">
          <div>
            <CardTitle>{t("插件列表")}</CardTitle>
            <CardDescription>
              {t("一个面板统一查看插件。未安装看当前市场，已安装看本地插件，更新只显示当前市场里有新版本的已安装插件。")}
            </CardDescription>
          </div>
          <div className="flex flex-wrap gap-2">
            {PLUGIN_VIEW_FILTER_OPTIONS.map((option) => {
              const count =
                option.value === "installed"
                  ? installedItems.length
                  : option.value === "update"
                    ? updatableInstalledItems.length
                    : notInstalledCatalogItems.length;
              return (
                <Button
                  key={option.value}
                  type="button"
                  variant="outline"
                  onClick={() => setPluginViewFilter(option.value)}
                  className={cn(
                    "flex h-auto items-center gap-2 rounded-full px-4 py-2 text-sm transition-all",
                    pluginViewFilter === option.value
                      ? "border-primary/40 bg-primary/10 text-primary shadow-sm"
                      : "border-border/60 bg-background/40 text-muted-foreground hover:bg-background/70",
                  )}
                >
                  <span>{t(option.label)}</span>
                  <Badge variant="secondary">{count}</Badge>
                </Button>
              );
            })}
          </div>
        </CardHeader>
        <CardContent>
          {catalogQuery.isLoading || installedQuery.isLoading ? (
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
              {Array.from({ length: 2 }).map((_, index) => (
                <Skeleton key={index} className="h-72 rounded-xl" />
              ))}
            </div>
          ) : pluginViewFilter === "installed" ? (
            installedItems.length > 0 ? (
              <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
                {installedItems.map((item) => (
                  <InstalledPluginCard
                    key={item.pluginId}
                    item={item}
                    t={t}
                    updateVersion={
                      updatableVersionByPluginId.get(item.pluginId) || null
                    }
                    onOpenDetails={(entry) =>
                      setSelectedPlugin({
                        kind: "installed",
                        pluginId: entry.pluginId,
                      })
                    }
                    onUpdate={(pluginId) =>
                      updateMutation.mutate({
                        pluginId,
                        sourceUrl:
                          catalogById.get(pluginId)?.sourceUrl || item.sourceUrl,
                      })
                    }
                    onEnable={(pluginId) =>
                      toggleMutation.mutate({ pluginId, enabled: true })
                    }
                    onDisable={(pluginId) =>
                      toggleMutation.mutate({ pluginId, enabled: false })
                    }
                  />
                ))}
              </div>
            ) : (
              <Empty className="min-h-40 border bg-muted/20">
                <EmptyHeader>
                  <EmptyTitle>{t("还没有安装任何插件")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            )
          ) : pluginViewFilter === "update" ? (
            updatableInstalledItems.length > 0 ? (
              <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
                {updatableInstalledItems.map((item) => (
                  <InstalledPluginCard
                    key={item.pluginId}
                    item={item}
                    t={t}
                    updateVersion={
                      updatableVersionByPluginId.get(item.pluginId) || null
                    }
                    onOpenDetails={(entry) =>
                      setSelectedPlugin({
                        kind: "installed",
                        pluginId: entry.pluginId,
                      })
                    }
                    onUpdate={(pluginId) =>
                      updateMutation.mutate({
                        pluginId,
                        sourceUrl:
                          catalogById.get(pluginId)?.sourceUrl || item.sourceUrl,
                      })
                    }
                    onEnable={(pluginId) =>
                      toggleMutation.mutate({ pluginId, enabled: true })
                    }
                    onDisable={(pluginId) =>
                      toggleMutation.mutate({ pluginId, enabled: false })
                    }
                  />
                ))}
              </div>
            ) : (
              <Empty className="min-h-40 border bg-muted/20">
                <EmptyHeader>
                  <EmptyTitle>{t("当前市场没有可更新插件")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            )
          ) : notInstalledCatalogItems.length > 0 ? (
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
              {notInstalledCatalogItems.map((item) => (
                <PluginCard
                  key={item.id}
                  item={item}
                  t={t}
                  onOpenDetails={(entry) =>
                    setSelectedPlugin({ kind: "catalog", pluginId: entry.id })
                  }
                  onInstall={(entry) => installMutation.mutate(entry)}
                />
              ))}
            </div>
          ) : (
            <Empty className="min-h-40 border bg-muted/20">
              <EmptyHeader>
                <EmptyTitle>
                  {marketMode === "custom" && !catalogQuery.data?.sourceUrl
                    ? t("当前还没有配置自定义源，所以这里不会显示未安装插件。")
                    : t("暂无未安装插件")}
                </EmptyTitle>
              </EmptyHeader>
            </Empty>
          )}
        </CardContent>
      </Card>

      <Dialog
        open={selectedPlugin !== null}
        onOpenChange={(open) => !open && setSelectedPlugin(null)}
      >
        <DialogContent
          showCloseButton={false}
          className="glass-card mission-panel max-h-[85vh] overflow-hidden p-0 sm:max-w-[860px] lg:max-w-[920px]"
        >
          {selectedDetail ? (
            <div className="flex max-h-[85vh] flex-col">
              <div className="shrink-0 bg-muted/20 px-6 pt-6">
                <div className="flex items-start justify-between gap-4">
                  <DialogHeader className="mb-4 min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <DialogTitle className="text-xl">
                        {selectedDetail.name}
                      </DialogTitle>
                      <Badge variant="secondary">
                        {selectedDetail.version}
                      </Badge>
                      {"status" in selectedDetail ? (
                        <StatusBadge status={selectedDetail.status} t={t} />
                      ) : null}
                    </div>
                    <DialogDescription className="break-words text-sm">
                      {selectedDetail.description || t("暂无描述")}
                    </DialogDescription>
                    <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
                      {selectedDetail.author ? (
                        <span>
                          {t("作者")}：{selectedDetail.author}
                        </span>
                      ) : null}
                      {selectedDetail.sourceUrl ? (
                        <span>
                          {t("来源")}：
                          {selectedDetail.sourceUrl === "builtin://codexmanager"
                            ? t("内置精选市场")
                            : selectedDetail.sourceUrl}
                        </span>
                      ) : null}
                      <span>
                        {t("权限")} {selectedDetail.permissions.length}
                      </span>
                      {"taskCount" in selectedDetail ? (
                        <span>
                          {t("任务")} {selectedDetail.enabledTaskCount}/
                          {selectedDetail.taskCount}
                        </span>
                      ) : (
                        <span>
                          {t("任务")} {selectedDetail.tasks.length}
                        </span>
                      )}
                    </div>
                    <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
                      <span>
                        {t("清单版本")} {selectedDetail.manifestVersion}
                      </span>
                      <span>
                        {t("运行时")}{" "}
                        {formatRuntimeKind(selectedDetail.runtimeKind)}
                      </span>
                      {selectedDetail.category ? (
                        <span>
                          {t("分类")}{" "}
                          {formatMarketCategory(selectedDetail.category, t)}
                        </span>
                      ) : null}
                      {selectedDetail.tags.length > 0 ? (
                        <span>
                          {t("标签")} {selectedDetail.tags.join(" / ")}
                        </span>
                      ) : null}
                    </div>
                  </DialogHeader>
                  <DialogClose
                    className={cn(
                      buttonVariants({ variant: "ghost", size: "icon-sm" }),
                      "shrink-0 text-muted-foreground hover:bg-muted hover:text-foreground",
                    )}
                    type="button"
                  >
                    <X className="h-4 w-4" />
                    <span className="sr-only">{t("关闭")}</span>
                  </DialogClose>
                </div>
              </div>

              <div className="max-h-[calc(85vh-154px)] overflow-y-auto px-6 py-6">
                <div className="grid gap-4">
                  <div className="rounded-xl border border-border/60 bg-background/60 p-4">
                    <div className="mb-2 text-sm font-medium">{t("权限")}</div>
                    <div>
                      {selectedDetail.permissions.length > 0 ? (
                        selectedDetail.permissions.map((permission) => (
                          <PermissionBadge
                            key={permission}
                            permission={permission}
                            t={t}
                          />
                        ))
                      ) : (
                        <div className="text-sm text-muted-foreground">
                          {t("无需额外权限")}
                        </div>
                      )}
                    </div>
                  </div>

                  <div className="rounded-xl border border-border/60 bg-background/60 p-4">
                    <div className="mb-2 text-sm font-medium">{t("任务")}</div>
                    <div className="space-y-2">
                      {selectedTasks.length > 0 ? (
                        selectedTasks.map((task) => (
                          <div
                            key={task.id}
                            className="rounded-xl border border-border/60 bg-background p-3 text-sm"
                          >
                            <div className="flex items-start justify-between gap-3">
                              <div className="min-w-0">
                                <div className="font-medium">{task.name}</div>
                                <div className="mt-1 break-words text-xs text-muted-foreground">
                                  {task.scheduleKind === "manual"
                                    ? t("手动")
                                    : t("每 {seconds} 秒", {
                                        seconds: task.intervalSeconds || 0,
                                      })}
                                  {" · "}
                                  {task.entrypoint}
                                </div>
                              </div>
                              <div className="flex items-center gap-2">
                                <Badge variant="outline">
                                  {task.enabled ? t("启用") : t("禁用")}
                                </Badge>
                                {selectedPlugin?.kind === "installed" ? (
                                  <Button
                                    size="sm"
                                    variant="secondary"
                                    onClick={() =>
                                      runTaskMutation.mutate(task.id)
                                    }
                                  >
                                    <Play className="mr-1.5 h-3.5 w-3.5" />
                                    {t("运行")}
                                  </Button>
                                ) : null}
                              </div>
                            </div>
                            {task.description ? (
                              <div className="mt-1 break-words text-xs text-muted-foreground">
                                {task.scheduleKind === "manual"
                                  ? task.description
                                  : t("每 {seconds} 秒自动执行一次。", {
                                      seconds: task.intervalSeconds || 0,
                                    })}
                              </div>
                            ) : null}
                            {task.lastError ? (
                              <div className="mt-1 break-words text-xs text-red-500">
                                {task.lastError}
                              </div>
                            ) : null}
                            {"scheduleKind" in task &&
                            task.scheduleKind !== "manual" ? (
                              <div className="mt-3 grid gap-2 rounded-xl border border-border/60 bg-background/70 p-3">
                                <div className="text-xs font-medium text-muted-foreground">
                                  {t("自动执行间隔")}
                                </div>
                                <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                                  <Input
                                    type="number"
                                    min={1}
                                    step={1}
                                    className="h-9 w-full sm:max-w-[180px]"
                                    value={
                                      taskIntervalDrafts[task.id] ??
                                      String(task.intervalSeconds || 60)
                                    }
                                    onChange={(event) =>
                                      setTaskIntervalDrafts((prev) => ({
                                        ...prev,
                                        [task.id]: event.target.value,
                                      }))
                                    }
                                    disabled={updateTaskMutation.isPending}
                                  />
                                  <span className="text-xs text-muted-foreground">
                                    {t("秒")}
                                  </span>
                                  <Button
                                    size="sm"
                                    variant="outline"
                                    className="sm:ml-auto"
                                    disabled={updateTaskMutation.isPending}
                                    onClick={() => {
                                      const raw =
                                        taskIntervalDrafts[task.id] ??
                                        String(task.intervalSeconds || 60);
                                      const intervalSeconds = Number(raw);
                                      if (
                                        !Number.isFinite(intervalSeconds) ||
                                        intervalSeconds <= 0
                                      ) {
                                        toast.error(t("请输入大于 0 的秒数"));
                                        return;
                                      }
                                      updateTaskMutation.mutate({
                                        taskId: task.id,
                                        intervalSeconds:
                                          Math.floor(intervalSeconds),
                                      });
                                    }}
                                  >
                                    {t("保存")}
                                  </Button>
                                </div>
                                <div className="break-words text-[11px] text-muted-foreground">
                                  {t("当前设置为每 {seconds} 秒自动执行一次。", {
                                    seconds: task.intervalSeconds || 0,
                                  })}
                                </div>
                              </div>
                            ) : null}
                          </div>
                        ))
                      ) : (
                        <div className="text-sm text-muted-foreground">
                          {t("暂无任务")}
                        </div>
                      )}
                    </div>
                  </div>

                  {selectedPlugin?.kind === "installed" ? (
                    <div className="rounded-xl border border-border/60 bg-background/60 p-4">
                      <div className="mb-2 text-sm font-medium">{t("最近运行")}</div>
                      <div className="space-y-2">
                        {selectedLogs.length > 0 ? (
                          selectedLogs.slice(0, 5).map((log) => (
                            <div
                              key={log.id}
                              className={cn(
                                "rounded-xl border p-3 text-xs",
                                log.status === "ok"
                                  ? "border-emerald-500/20 bg-emerald-500/5"
                                  : "border-red-500/20 bg-red-500/5",
                              )}
                            >
                              <div className="flex items-center justify-between gap-2">
                                <div className="font-medium">
                                  {log.taskName || log.taskId || t("未知任务")}
                                </div>
                                <Badge
                                  variant={
                                    log.status === "ok"
                                      ? "secondary"
                                      : "destructive"
                                  }
                                >
                                  {log.status}
                                </Badge>
                              </div>
                              <div className="mt-1 break-words text-muted-foreground">
                                {log.error ||
                                  (log.output
                                    ? JSON.stringify(log.output)
                                    : t("无输出"))}
                              </div>
                              <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                                <span>
                                  {t("执行于 {time}", {
                                    time: formatTimestamp(log.startedAt),
                                  })}
                                </span>
                                <span>
                                  {t("耗时 {duration}", {
                                    duration: formatDuration(log.durationMs),
                                  })}
                                </span>
                              </div>
                            </div>
                          ))
                        ) : (
                          <div className="text-sm text-muted-foreground">
                            {t("暂无日志")}
                          </div>
                        )}
                      </div>
                    </div>
                  ) : null}
                </div>
              </div>

              <DialogFooter className="mx-0 mb-0 rounded-b-xl border-t border-border/60 bg-muted/20 px-6 py-4 sm:items-center sm:justify-end">
                {selectedPlugin?.kind === "catalog" && selectedCatalogItem ? (
                  <Button
                    className="gap-2"
                    onClick={() => {
                      installMutation.mutate(selectedCatalogItem);
                      setSelectedPlugin(null);
                    }}
                  >
                    <Download className="h-4 w-4" />
                    {t("安装")}
                  </Button>
                ) : null}
                {selectedPlugin?.kind === "installed" &&
                selectedInstalledItem ? (
                  <>
                    {selectedUpdateVersion ? (
                      <Button
                        className="gap-2"
                        onClick={() =>
                          updateMutation.mutate({
                            pluginId: selectedInstalledItem.pluginId,
                            sourceUrl:
                              selectedCatalogItem?.sourceUrl ||
                              selectedInstalledItem.sourceUrl,
                          })
                        }
                      >
                        <RefreshCw className="h-4 w-4" />
                        {t("更新到 {version}", {
                          version: selectedUpdateVersion || "-",
                        })}
                      </Button>
                    ) : null}
                    {selectedInstalledItem.status === "enabled" ? (
                      <Button
                        variant="outline"
                        className="gap-2"
                        onClick={() =>
                          toggleMutation.mutate({
                            pluginId: selectedInstalledItem.pluginId,
                            enabled: false,
                          })
                        }
                      >
                        <ToggleLeft className="h-4 w-4" />
                        {t("停用")}
                      </Button>
                    ) : (
                      <Button
                        variant="outline"
                        className="gap-2"
                        onClick={() =>
                          toggleMutation.mutate({
                            pluginId: selectedInstalledItem.pluginId,
                            enabled: true,
                          })
                        }
                      >
                        <ToggleRight className="h-4 w-4" />
                        {t("启用")}
                      </Button>
                    )}
                    <Button
                      variant="destructive"
                      className="gap-2"
                      onClick={() =>
                        setPendingUninstallPlugin(selectedInstalledItem)
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                      {t("卸载")}
                    </Button>
                  </>
                ) : null}
              </DialogFooter>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={pendingUninstallPlugin !== null}
        onOpenChange={(open) => {
          if (!open) {
            setPendingUninstallPlugin(null);
          }
        }}
        title={t("卸载插件")}
        description={
          pendingUninstallPlugin
            ? t("确认卸载插件「{name}」吗？卸载后对应任务和运行记录会一并清理。", {
                name: pendingUninstallPlugin.name,
              })
            : t("确认卸载这个插件吗？")
        }
        confirmText={t("卸载")}
        confirmVariant="destructive"
        onConfirm={() => {
          if (!pendingUninstallPlugin) {
            return;
          }
          uninstallMutation.mutate(pendingUninstallPlugin.pluginId);
          setSelectedPlugin(null);
          setPendingUninstallPlugin(null);
        }}
      />
    </PageWorkspace>
  );
}
