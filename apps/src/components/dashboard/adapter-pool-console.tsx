"use client";

import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  ArrowRight,
  Bot,
  Database,
  Plus,
  Power,
  Radar,
  RefreshCw,
  Route,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useQueries, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { accountClient } from "@/lib/api/account-client";
import {
  readAggregateAutoProbe,
  writeAggregateAutoProbe,
} from "@/lib/aggregate-key-import";
import { dashboardClient } from "@/lib/api/dashboard-client";
import {
  cancelAdapterPoolProbe,
  probeAdapterPool,
  readAdapterAutoProbe,
  refreshAdapterPool,
  resumeAdapterPoolProbe,
  subscribeAdapterAutoProbe,
  writeAdapterAutoProbe,
  type AdapterProbePoolId,
  type AdapterProbeProgress,
  type AdapterProbeResult,
} from "@/lib/adapter-probe";
import { listGrokCredentials, setGrokCredentialEnabled } from "@/lib/api/grok-client";
import { listKiroCredentials, setKiroCredentialEnabled } from "@/lib/api/kiro-client";
import { attachUsagesToAccounts } from "@/lib/api/normalize";
import { getAppErrorMessage } from "@/lib/api/transport";
import { useI18n } from "@/lib/i18n/provider";
import {
  getAccountRecoveryGuidance,
  type TranslateFn,
} from "@/app/accounts/accounts-page-helpers";
import { useAppStore } from "@/lib/store/useAppStore";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";

type AdapterId = AdapterProbePoolId | "aggregate";
type PoolStatus = "available" | "limited" | "disabled" | "error" | "unknown";

interface AdapterPoolItem {
  id: string;
  label: string;
  status: PoolStatus;
  detail: string;
  active: boolean;
  models: string[];
  probedAt: number | null;
  resetCardCount: number;
  resetCardManualActivation: boolean;
  usageTokens: number | null;
  usageCostUsd: number | null;
  importedAt: number | null;
  guidanceBadge: string;
  guidanceDetail: string;
  guidanceAction: "none" | "probe" | "refresh_usage" | "refresh_credentials" | "wait" | "remove";
}

interface AdapterPoolSnapshot {
  enabled: boolean;
  fallbackEnabled?: boolean;
  fallbackKeyCount?: number;
  total: number;
  available: number;
  limited: number;
  disabled: number;
  error: number;
  models: number;
  resetCards: number;
  items: AdapterPoolItem[];
}

/**
 * 首页唯一识别的平台描述格式。未来后端提供 Adapter Registry 后，只需把
 * 这个数组的来源替换为接口，资源池卡片和详情无需再增加平台判断。
 */
export interface AdapterPoolDescriptor {
  id: AdapterId;
  title: string;
  description: string;
  href: string;
  icon: LucideIcon;
  supportsResetCards?: boolean;
  queryKey: readonly string[];
  load: (t: TranslateFn) => Promise<AdapterPoolSnapshot>;
  fallback?: (context: AdapterPoolFallbackContext) => Pick<AdapterPoolSnapshot, "total" | "available">;
  setPoolEnabled?: (snapshot: AdapterPoolSnapshot, enabled: boolean) => Promise<void>;
  setFallbackEnabled?: (enabled: boolean) => Promise<number>;
  probePool: (
    snapshot: AdapterPoolSnapshot,
    onProgress?: (progress: AdapterProbeProgress) => void,
  ) => Promise<AdapterProbeResult>;
  refreshPool: (snapshot: AdapterPoolSnapshot) => Promise<{ requested: number; succeeded: number; failed: number }>;
}

interface AdapterPoolFallbackContext {
  codexTotal: number;
  codexAvailable: number;
}

function normalizeStatus(value: string | null | undefined): PoolStatus {
  const status = String(value || "").trim().toLowerCase();
  if (["active", "available", "ok"].includes(status)) return "available";
  if (["limited", "cooldown", "low_quota"].includes(status)) return "limited";
  if (["disabled", "inactive"].includes(status)) return "disabled";
  if (["banned", "unavailable", "error"].includes(status)) return "error";
  return "unknown";
}

function countModels(items: AdapterPoolItem[]): number {
  return new Set(items.flatMap((item) => item.models)).size;
}

function formatCompactTokens(value: number): string {
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1).replace(/\.0$/, "")}B`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1).replace(/\.0$/, "")}K`;
  return String(Math.max(0, Math.round(value)));
}

function normalizeEpochMilliseconds(value: number | null | undefined): number | null {
  if (!value || !Number.isFinite(value) || value <= 0) return null;
  return value < 10_000_000_000 ? value * 1000 : value;
}

function formatPoolAge(value: number | null): string {
  const importedAt = normalizeEpochMilliseconds(value);
  if (!importedAt) return "--";
  const minutes = Math.max(0, Math.floor((Date.now() - importedAt) / 60_000));
  const days = Math.floor(minutes / 1_440);
  const hours = Math.floor((minutes % 1_440) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes % 60}m`;
  return `${minutes}m`;
}

function formatImportedAt(value: number | null): string {
  const importedAt = normalizeEpochMilliseconds(value);
  if (!importedAt) return "--";
  return new Date(importedAt).toLocaleString();
}

function formatEstimatedCost(tokens: number, value: number, missingLabel = "价格未配置"): string {
  if (tokens > 0 && value <= 0) return missingLabel;
  return `$${value < 0.01 && value > 0 ? value.toFixed(4) : value.toFixed(2)}`;
}

let usageBreakdownCache:
  | { expiresAt: number; promise: ReturnType<typeof dashboardClient.getAdminUsageSummary> }
  | null = null;

function loadUsageBreakdown() {
  const now = Date.now();
  if (usageBreakdownCache && usageBreakdownCache.expiresAt > now) {
    return usageBreakdownCache.promise;
  }
  const promise = dashboardClient.getAdminUsageSummary({ includeBreakdowns: true });
  usageBreakdownCache = { expiresAt: now + 10_000, promise };
  void promise.catch(() => {
    if (usageBreakdownCache?.promise === promise) usageBreakdownCache = null;
  });
  return promise;
}

async function probeAggregateSnapshot(
  snapshot: AdapterPoolSnapshot,
  onProgress?: (progress: AdapterProbeProgress) => void,
): Promise<AdapterProbeResult> {
  const targets = snapshot.items.filter((item) => item.active);
  let completed = 0;
  let succeeded = 0;
  let failed = 0;
  const emit = () => onProgress?.({
    id: "aggregate-inline",
    requested: targets.length,
    completed,
    succeeded,
    failed,
  });
  emit();
  await Promise.all(
    targets.map(async (item) => {
      try {
        const result = await accountClient.testAggregateApiConnection(item.id);
        if (result.ok) {
          succeeded += 1;
          await accountClient.syncManagedModelSourceModels({
            sourceKind: "aggregate_api",
            sourceId: item.id,
          });
        } else {
          failed += 1;
        }
      } catch {
        failed += 1;
      } finally {
        completed += 1;
        emit();
      }
    }),
  );
  return { requested: targets.length, succeeded, failed, cancelled: 0 };
}

const ADAPTER_POOL_DESCRIPTORS: readonly AdapterPoolDescriptor[] = [
  {
    id: "codex",
    title: "GPT / Codex 池",
    description: "OpenAI/Codex 账号池，按批次循环参与统一网关调用。",
    href: "/accounts",
    icon: Bot,
    supportsResetCards: true,
    queryKey: ["resource-pools", "codex-accounts"],
    fallback: ({ codexTotal, codexAvailable }) => ({
      total: codexTotal,
      available: codexAvailable,
    }),
    load: async (t) => {
      const [accounts, usages, catalog, usageSummary] = await Promise.all([
        accountClient.list(),
        accountClient.listUsage(),
        accountClient.listModels(false),
        loadUsageBreakdown().catch(() => null),
      ]);
      const records = attachUsagesToAccounts(accounts.items, usages);
      const usageByAccount = new Map(
        (usageSummary?.openaiAccounts ?? []).map((item) => [item.sourceId, item.rangeUsage]),
      );
      const items: AdapterPoolItem[] = records.map((account) => {
        const guidance = getAccountRecoveryGuidance(account, t);
        const normalizedPlan = String(account.planType || "").trim().toLowerCase();
        const discoveredModels = account.lastRefreshAt
          ? catalog.models
              .filter((model) => {
                if (!model.supportedInApi) return false;
                if (model.availableInPlans.length === 0) return true;
                if (!normalizedPlan) return false;
                return model.availableInPlans.some(
                  (plan) => String(plan).trim().toLowerCase() === normalizedPlan,
                );
              })
              .map((model) => model.slug)
          : [];
        return {
        id: account.id,
        label: account.name || account.id.slice(0, 8),
        status:
          guidance.tone === "ready"
            ? "available"
            : guidance.action === "wait"
              ? "limited"
              : guidance.tone === "danger"
                ? "error"
                : account.status === "disabled"
                  ? "disabled"
                  : "unknown",
        detail: account.availabilityText || account.status || "未知",
        active: account.status !== "disabled",
        models: account.modelSlugs.length > 0 ? account.modelSlugs : discoveredModels,
        probedAt: account.lastRefreshAt,
        resetCardCount: account.resetEntitlements.reduce(
          (sum, item) => sum + (item.count ?? 1),
          0,
        ),
        resetCardManualActivation: account.resetEntitlements.some(
          (item) => item.manualActivationRequired,
        ),
        usageTokens: usageByAccount.get(account.id)?.totalTokens ?? 0,
        usageCostUsd: usageByAccount.get(account.id)?.estimatedCostUsd ?? 0,
        importedAt: account.createdAt || null,
        guidanceBadge: guidance.badge,
        guidanceDetail: guidance.detail,
        guidanceAction: guidance.action,
      };
      });
      return {
        enabled: records.some((item) => item.status !== "disabled"),
        total: records.length,
        available: items.filter((item) => item.status === "available").length,
        limited: items.filter((item) => item.status === "limited").length,
        disabled: records.filter((item) => item.status === "disabled").length,
        error: items.filter((item) => item.status === "error").length,
        models: countModels(items),
        resetCards: items.reduce((sum, item) => sum + item.resetCardCount, 0),
        items,
      };
    },
    setPoolEnabled: async (snapshot, enabled) => {
      await Promise.all(
        snapshot.items.map((item) =>
          enabled
            ? accountClient.enableAccount(item.id)
            : accountClient.disableAccount(item.id),
        ),
      );
    },
    probePool: (snapshot, onProgress) =>
      probeAdapterPool(
        "codex",
        snapshot.items.filter((item) => item.active).map((item) => item.id),
        onProgress,
      ),
    refreshPool: (snapshot) =>
      refreshAdapterPool(
        "codex",
        snapshot.items.filter((item) => item.active).map((item) => item.id),
      ),
  },
  {
    id: "kiro",
    title: "Kiro 池",
    description: "Kiro 凭据池，可独立启停并参与 OpenAI 格式统一路由。",
    href: "/kiro",
    icon: Zap,
    queryKey: ["resource-pools", "kiro"],
    load: async (t) => {
      const [records, usageSummary] = await Promise.all([
        listKiroCredentials(),
        loadUsageBreakdown().catch(() => null),
      ]);
      const usageByCredential = new Map(
        (usageSummary?.kiroCredentials ?? []).map((item) => [item.sourceId, item.rangeUsage]),
      );
      const items: AdapterPoolItem[] = records.map((item) => ({
        id: item.id,
        label: item.email || item.id.slice(0, 8),
        status: item.cooldownUntil
          ? "limited"
          : item.status !== "active"
            ? normalizeStatus(item.status)
            : item.modelProbeCheckedAt == null
              ? "unknown"
              : item.availableModels.length > 0
                ? "available"
                : "error",
        detail: item.apiRegion || item.authRegion || item.subscription || item.status,
        active: item.status === "active",
        models: item.availableModels,
        probedAt: item.modelProbeCheckedAt,
        resetCardCount: 0,
        resetCardManualActivation: false,
        usageTokens: usageByCredential.get(item.id)?.totalTokens ?? 0,
        usageCostUsd: usageByCredential.get(item.id)?.estimatedCostUsd ?? 0,
        importedAt: item.createdAt || null,
        guidanceBadge: item.cooldownUntil
          ? t("等待恢复")
          : item.status === "active" && item.availableModels.length > 0
            ? t("正在服役")
            : item.status === "disabled"
              ? t("已停用")
              : item.modelProbeCheckedAt == null
                ? t("等待准入")
                : t("需要处理"),
        guidanceDetail: item.cooldownUntil
          ? t("限流冷却中，不要删除；恢复后会重新参与调用。")
          : item.status === "active" && item.availableModels.length > 0
            ? t("当前凭据可以参与统一网关调用。")
            : item.status === "disabled"
              ? t("手动关闭状态，需要时重新启用。")
              : t("进入 Kiro 管理页刷新凭据或重新探测。"),
        guidanceAction: item.cooldownUntil
          ? "wait"
          : item.status === "active" && item.availableModels.length > 0
            ? "none"
            : item.status === "disabled"
              ? "none"
              : item.modelProbeCheckedAt == null
                ? "probe"
                : "refresh_credentials",
      }));
      return {
        enabled: records.some((item) => item.status === "active"),
        total: records.length,
        available: records.filter(
          (item) => item.status === "active" && item.availableModels.length > 0,
        ).length,
        limited: records.filter((item) => item.cooldownUntil != null).length,
        disabled: records.filter((item) => item.status === "disabled").length,
        error: records.filter(
          (item) =>
            (item.status !== "active" && item.status !== "disabled") ||
            (item.status === "active" &&
              item.modelProbeCheckedAt != null &&
              item.availableModels.length === 0),
        ).length,
        models: countModels(items),
        resetCards: 0,
        items,
      };
    },
    setPoolEnabled: async (snapshot, enabled) => {
      await Promise.all(
        snapshot.items.map((item) => setKiroCredentialEnabled(item.id, enabled)),
      );
    },
    probePool: (snapshot, onProgress) =>
      probeAdapterPool(
        "kiro",
        snapshot.items.filter((item) => item.active).map((item) => item.id),
        onProgress,
      ),
    refreshPool: (snapshot) =>
      refreshAdapterPool(
        "kiro",
        snapshot.items.filter((item) => item.active).map((item) => item.id),
      ),
  },
  {
    id: "grok",
    title: "Grok 池",
    description: "Grok Web 账号池，后续可加入同一套批次和平台开关。",
    href: "/grok",
    icon: Activity,
    queryKey: ["resource-pools", "grok"],
    load: async (t) => {
      const [records, usageSummary] = await Promise.all([
        listGrokCredentials(),
        loadUsageBreakdown().catch(() => null),
      ]);
      const usageByCredential = new Map(
        (usageSummary?.grokCredentials ?? []).map((item) => [item.sourceId, item.rangeUsage]),
      );
      const items: AdapterPoolItem[] = records.map((item) => ({
        id: item.id,
        label: item.accountMasked || item.id.slice(0, 8),
        status: item.cooldownUntil ? "limited" : normalizeStatus(item.status),
        detail: item.webTier || item.status,
        active: item.status === "active",
        models: item.availableModels,
        probedAt: item.availableModels.length > 0 ? item.updatedAt : null,
        resetCardCount: 0,
        resetCardManualActivation: false,
        usageTokens: usageByCredential.get(item.id)?.totalTokens ?? 0,
        usageCostUsd: usageByCredential.get(item.id)?.estimatedCostUsd ?? 0,
        importedAt: item.createdAt || null,
        guidanceBadge: item.cooldownUntil
          ? t("等待恢复")
          : item.status === "active"
            ? t("正在服役")
            : item.status === "disabled"
              ? t("已停用")
              : t("需要处理"),
        guidanceDetail: item.cooldownUntil
          ? t("限流冷却中，不要删除；恢复后会自动回池。")
          : item.status === "active"
            ? t("当前凭据可以参与调用。")
            : item.status === "disabled"
              ? t("手动关闭状态，需要时重新启用。")
              : t("进入 Grok 管理页刷新或重新导入凭据。"),
        guidanceAction: item.cooldownUntil
          ? "wait"
          : item.status === "active" || item.status === "disabled"
            ? "none"
            : "refresh_credentials",
      }));
      return {
        enabled: records.some((item) => item.status === "active"),
        total: records.length,
        available: records.filter((item) => item.status === "active").length,
        limited: records.filter((item) => item.cooldownUntil != null).length,
        disabled: records.filter((item) => item.status === "disabled").length,
        error: records.filter(
          (item) => item.status !== "active" && item.status !== "disabled",
        ).length,
        models: countModels(items),
        resetCards: 0,
        items,
      };
    },
    setPoolEnabled: async (snapshot, enabled) => {
      await Promise.all(
        snapshot.items.map((item) => setGrokCredentialEnabled(item.id, enabled)),
      );
    },
    probePool: (snapshot, onProgress) =>
      probeAdapterPool(
        "grok",
        snapshot.items.filter((item) => item.active).map((item) => item.id),
        onProgress,
      ),
    refreshPool: (snapshot) =>
      refreshAdapterPool(
        "grok",
        snapshot.items.filter((item) => item.active).map((item) => item.id),
      ),
  },
  {
    id: "aggregate",
    title: "中转站池",
    description: "OpenAI 兼容中转站与第三方接口池，可独立启停并作为账号池备用线路。",
    href: "/aggregate-api",
    icon: Database,
    queryKey: ["resource-pools", "aggregate-apis"],
    load: async (t) => {
      const [records, routing, usageSummary, apiKeys] = await Promise.all([
        accountClient.listAggregateApis(),
        accountClient.listManagedModelRouting(),
        loadUsageBreakdown().catch(() => null),
        accountClient.listApiKeys(),
      ]);
      const modelsByApi = new Map<string, string[]>();
      for (const model of routing.sourceModels) {
        if (model.sourceKind !== "aggregate_api" || model.status === "disabled") continue;
        const current = modelsByApi.get(model.sourceId) ?? [];
        current.push(model.upstreamModel);
        modelsByApi.set(model.sourceId, current);
      }
      const usageByApi = new Map(
        (usageSummary?.aggregateApis ?? []).map((item) => [item.sourceId, item.rangeUsage]),
      );
      const items: AdapterPoolItem[] = records.map((item) => {
        const active = item.status === "active";
        const probeSucceeded = item.lastTestStatus === "success";
        const probeFailed = item.lastTestStatus === "failed";
        return {
          id: item.id,
          label: item.supplierName || item.url,
          status: !active
            ? "disabled"
            : probeSucceeded
              ? "available"
              : probeFailed
                ? "error"
                : "unknown",
          detail: item.lastTestError || item.url,
          active,
          models: modelsByApi.get(item.id) ?? item.modelSlugs,
          probedAt: item.lastTestAt,
          resetCardCount: 0,
          resetCardManualActivation: false,
          usageTokens: usageByApi.get(item.id)?.totalTokens ?? 0,
          usageCostUsd: usageByApi.get(item.id)?.estimatedCostUsd ?? 0,
          importedAt: item.createdAt,
          guidanceBadge: !active
            ? t("已停用")
            : probeSucceeded
              ? t("渠道可用")
              : probeFailed
                ? t("需要处理")
                : t("等待准入"),
          guidanceDetail: !active
            ? t("手动关闭状态，需要时重新启用。")
            : probeSucceeded
              ? t("已通过连通探测，可被平台 Key 选择或作为混合路由备用。")
              : probeFailed
                ? t("连通探测失败，请检查地址、KEY 或上游状态后重新探测。")
                : t("点击立即探测，成功后同步可用模型。"),
          guidanceAction: active && !probeSucceeded ? "probe" : "none",
        };
      });
      return {
        enabled: records.some((item) => item.status === "active"),
        fallbackEnabled: apiKeys.some(
          (key) => key.status === "active" && key.rotationStrategy === "hybrid_rotation",
        ),
        fallbackKeyCount: apiKeys.filter(
          (key) =>
            key.status === "active" &&
            (key.rotationStrategy === "account_rotation" ||
              key.rotationStrategy === "hybrid_rotation"),
        ).length,
        total: records.length,
        available: items.filter((item) => item.status === "available").length,
        limited: 0,
        disabled: items.filter((item) => item.status === "disabled").length,
        error: items.filter((item) => item.status === "error").length,
        models: countModels(items),
        resetCards: 0,
        items,
      };
    },
    setPoolEnabled: async (snapshot, enabled) => {
      await Promise.all(
        snapshot.items.map((item) =>
          accountClient.updateAggregateApi(item.id, {
            supplierName: item.label,
            status: enabled ? "active" : "disabled",
          }),
        ),
      );
    },
    setFallbackEnabled: async (enabled) => {
      const apiKeys = await accountClient.listApiKeys();
      const targets = apiKeys.filter(
        (key) =>
          key.status === "active" &&
          (key.rotationStrategy === "account_rotation" ||
            key.rotationStrategy === "hybrid_rotation"),
      );
      await Promise.all(
        targets.map((key) =>
          accountClient.updateApiKey(key.id, {
            name: key.name || null,
            modelSlug: key.modelSlug || null,
            reasoningEffort: key.reasoningEffort || null,
            serviceTier: key.serviceTier || null,
            protocolType: key.protocol || "openai_compat",
            upstreamBaseUrl: key.upstreamBaseUrl || null,
            staticHeadersJson: key.staticHeadersJson || null,
            rotationStrategy: enabled ? "hybrid_rotation" : "account_rotation",
            aggregateApiId: null,
            accountPlanFilter: key.accountPlanFilter,
            quotaLimitTokens: key.quotaLimitTokens,
            allowedModels: key.allowedModels,
            allowedPlatforms: key.allowedPlatforms,
            modelVisibility: key.modelVisibility,
            expiresAt: key.expiresAt,
            concurrencyLimit: key.concurrencyLimit,
          }),
        ),
      );
      return targets.length;
    },
    probePool: probeAggregateSnapshot,
    refreshPool: async (snapshot) => {
      const result = await probeAggregateSnapshot(snapshot);
      return {
        requested: result.requested,
        succeeded: result.succeeded,
        failed: result.failed,
      };
    },
  },
];

interface AdapterPoolViewModel extends AdapterPoolSnapshot {
  descriptor: AdapterPoolDescriptor;
}

function statusTone(status: PoolStatus): string {
  switch (status) {
    case "available":
      return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
    case "limited":
      return "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
    case "disabled":
      return "border-muted-foreground/20 bg-muted/50 text-muted-foreground";
    case "error":
      return "border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-300";
    default:
      return "border-blue-500/25 bg-blue-500/10 text-blue-700 dark:text-blue-300";
  }
}

function PoolOverview({
  pool,
  expanded,
  onExpand,
  onEnabledChange,
  onProbe,
  onRefresh,
  onAutoProbeChange,
  onFallbackChange,
  autoProbe,
  busy,
  probing,
  probeProgress,
  refreshing,
  fallbackBusy,
  serviceReady,
}: {
  pool: AdapterPoolViewModel;
  expanded: boolean;
  onExpand: () => void;
  onEnabledChange: (enabled: boolean) => void;
  onProbe: () => void;
  onRefresh: () => void;
  onAutoProbeChange: (enabled: boolean) => void;
  onFallbackChange: (enabled: boolean) => void;
  autoProbe: boolean;
  busy: boolean;
  probing: boolean;
  probeProgress: AdapterProbeProgress | null;
  refreshing: boolean;
  fallbackBusy: boolean;
  serviceReady: boolean;
}) {
  const { t } = useI18n();
  const { descriptor } = pool;
  const Icon = descriptor.icon;
  return (
    <Card className="glass-card mission-panel resource-pool-gilded overflow-hidden py-0 shadow-sm">
      <CardContent className="p-0">
        <button type="button" onClick={onExpand} className="block w-full p-4 text-left transition-colors hover:bg-accent/40">
          <div className="flex items-start justify-between gap-3">
            <div className="flex min-w-0 items-center gap-3">
              <div className="flex size-11 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary"><Icon className="size-5" /></div>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="font-semibold">{descriptor.title}</h3>
                  <Badge variant={pool.enabled ? "secondary" : "outline"} className="text-[10px]">{pool.enabled ? t("参与调用") : t("暂停调用")}</Badge>
                </div>
                <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{t(descriptor.description)}</p>
              </div>
            </div>
            <ArrowRight className={cn("mt-1 size-4 shrink-0 text-muted-foreground transition-transform", expanded && "rotate-90")} />
          </div>
          <div className={cn("mt-4 grid grid-cols-3 gap-2 text-center", descriptor.supportsResetCards ? "sm:grid-cols-6" : "sm:grid-cols-5")}>
            {[
              [t("总数"), pool.total],
              [t("可用"), pool.available],
              [t("限额"), pool.limited],
              [t("异常"), pool.error],
              [t("模型"), pool.models],
              ...(descriptor.supportsResetCards ? [[t("重置卡"), pool.resetCards]] : []),
            ].map(([label, value]) => (
              <div key={String(label)} className="rounded-md border border-border/60 bg-background/50 px-2 py-2"><div className="font-mono text-base font-semibold">{value}</div><div className="text-[10px] text-muted-foreground">{label}</div></div>
            ))}
          </div>
        </button>
        <div className="grid gap-2 border-t border-border/60 px-4 py-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <button
              type="button"
              onClick={onProbe}
              disabled={busy || !serviceReady || pool.total === 0}
              className="inline-flex h-8 items-center rounded-md border border-border/70 bg-background/55 px-3 text-xs font-medium transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Radar className={cn("mr-1.5 size-3.5", probing && "animate-pulse")} />
              {probing
                ? `${t("取消探测")} · ${probeProgress?.completed ?? 0}/${probeProgress?.requested ?? pool.total}`
                : t("立即探测")}
            </button>
            <button
              type="button"
              onClick={onRefresh}
              disabled={refreshing || probing || busy || !serviceReady || pool.total === 0}
              className="inline-flex h-8 items-center rounded-md border border-border/70 bg-background/55 px-3 text-xs font-medium transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            >
              <RefreshCw className={cn("mr-1.5 size-3.5", refreshing && "animate-spin")} />
              {refreshing ? t("正在刷新…") : t("刷新状态")}
            </button>
            <label className="ml-auto flex items-center gap-2 text-xs text-muted-foreground">
              <span>{t("导入后自动探测")}</span>
              <Switch
                checked={autoProbe}
                disabled={!serviceReady}
                onCheckedChange={onAutoProbeChange}
                aria-label={`${descriptor.title} ${t("自动探测")}`}
              />
            </label>
          </div>
          <div className="flex items-center justify-between gap-3 border-t border-border/50 pt-2">
            <div className="text-xs text-muted-foreground">
              {busy
                ? t("正在更新账号状态…")
                : serviceReady
                  ? t("控制整个资源池是否参与调用")
                  : t("服务未连接，暂时无法操作")}
            </div>
            {pool.total === 0 ? (
              <span className="rounded-md border border-dashed border-border px-2 py-1 text-[10px] text-muted-foreground">
                {t("先导入凭据")}
              </span>
            ) : (
              <Switch
                checked={pool.enabled}
                disabled={busy || probing || refreshing || !serviceReady || !descriptor.setPoolEnabled}
                onCheckedChange={onEnabledChange}
                aria-label={`${descriptor.title} ${t("总开关")}`}
              />
            )}
          </div>
          {descriptor.setFallbackEnabled ? (
            <div className="flex items-center justify-between gap-3 border-t border-border/50 pt-2">
              <div>
                <div className="text-xs font-medium">{t("账号池不可用时启用中转站兜底")}</div>
                <div className="mt-0.5 text-[10px] text-muted-foreground">
                  {pool.fallbackEnabled
                    ? t("账号池始终优先，全部不可用后才切换中转站")
                    : t("已关闭兜底，中转站不会接管账号池失败的请求")}
                </div>
              </div>
              <Switch
                checked={pool.fallbackEnabled ?? false}
                disabled={
                  fallbackBusy ||
                  busy ||
                  !serviceReady ||
                  (pool.fallbackKeyCount ?? 0) === 0
                }
                onCheckedChange={onFallbackChange}
                aria-label={t("中转站兜底开关")}
              />
            </div>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}

function PoolDetails({ pool }: { pool: AdapterPoolViewModel }) {
  const { t } = useI18n();
  const navigateShellPath = useAppStore((state) => state.navigateShellPath);
  return (
    <Card className="glass-card mission-panel resource-pool-gilded resource-pool-detail overflow-hidden py-0 shadow-sm">
      <CardHeader className="border-b border-border/60 px-4 py-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <CardTitle className="text-sm">{pool.descriptor.title} · {t("池子详情")}</CardTitle>
          <button type="button" onClick={() => navigateShellPath(pool.descriptor.href)} className="inline-flex h-8 items-center rounded-md border border-border/70 px-3 text-xs font-medium hover:bg-accent">{t("进入高级管理")}</button>
        </div>
      </CardHeader>
      <CardContent className="p-4">
        {pool.items.length === 0 ? <div className="rounded-lg border border-dashed border-border/70 py-8 text-center text-sm text-muted-foreground">{t("暂无资源")}</div> : (
          <div className="grid grid-cols-3 gap-2 sm:grid-cols-5 md:grid-cols-7 xl:grid-cols-10">
            {pool.items.map((item, index) => (
              <button type="button" onClick={() => navigateShellPath(pool.descriptor.href)} key={item.id} className={cn("aspect-square rounded-lg border p-2 text-left transition-colors hover:-translate-y-0.5 hover:shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring", statusTone(item.status), item.active ? "opacity-100" : "opacity-55")} title={`${item.label} · ${item.guidanceBadge} · ${item.guidanceDetail} · ${t("导入时间")} ${formatImportedAt(item.importedAt)} · ${t("入池时长")} ${formatPoolAge(item.importedAt)}${item.usageTokens != null && item.usageCostUsd != null ? ` · ${t("近7天")} ${item.usageTokens.toLocaleString()} tokens · ${formatEstimatedCost(item.usageTokens, item.usageCostUsd, t("价格未配置"))}` : ""} · ${t("点击进入管理")}`}>
                <div className="flex h-full flex-col justify-between"><div className="flex items-center justify-between gap-1"><span className="font-mono text-[11px] font-semibold">{String(index + 1).padStart(2, "0")}</span><span className="rounded-full border border-current/20 px-1 text-[8px] font-semibold">{item.guidanceBadge}</span></div><div className="min-w-0"><div className="truncate text-[11px] font-medium">{item.label}</div><div className="line-clamp-2 text-[9px] leading-tight opacity-85">{item.guidanceDetail}</div></div><div className="grid gap-0.5"><div className="truncate font-mono text-[9px] opacity-80">{t("入池时长")} {formatPoolAge(item.importedAt)}</div>{item.usageTokens != null && item.usageCostUsd != null ? <div className="truncate font-mono text-[9px] font-semibold">{t("近7天")} {formatCompactTokens(item.usageTokens)} · {formatEstimatedCost(item.usageTokens, item.usageCostUsd, t("价格未配置"))}</div> : null}<div className="truncate text-[9px] opacity-80">{item.models[0] || (item.probedAt ? t("已探测账号状态") : t("未探测"))}</div>{item.resetCardCount > 0 ? <div className="truncate text-[9px] font-semibold text-amber-700 dark:text-amber-200">{t("重置卡")} ×{item.resetCardCount}{item.resetCardManualActivation ? ` · ${t("待激活")}` : ""}</div> : null}</div></div>
              </button>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

export function AdapterPoolConsole({
  codexTotal,
  codexAvailable,
  serviceReady,
}: {
  codexTotal: number;
  codexAvailable: number;
  serviceReady: boolean;
}) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const navigateShellPath = useAppStore((state) => state.navigateShellPath);
  const [expandedId, setExpandedId] = useState<AdapterId | null>(
    ADAPTER_POOL_DESCRIPTORS[0]?.id ?? null,
  );
  const [busyPoolId, setBusyPoolId] = useState<AdapterId | null>(null);
  const [probingPoolId, setProbingPoolId] = useState<AdapterId | null>(null);
  const [probeProgress, setProbeProgress] = useState<AdapterProbeProgress | null>(null);
  const [refreshingPoolId, setRefreshingPoolId] = useState<AdapterId | null>(null);
  const [fallbackBusy, setFallbackBusy] = useState(false);
  const [autoProbeByPool, setAutoProbeByPool] = useState<Record<AdapterId, boolean>>({
    codex: true,
    kiro: true,
    grok: true,
    aggregate: true,
  });

  useEffect(() => {
    setAutoProbeByPool({
      codex: readAdapterAutoProbe("codex"),
      kiro: readAdapterAutoProbe("kiro"),
      grok: readAdapterAutoProbe("grok"),
      aggregate: readAggregateAutoProbe(),
    });
    return subscribeAdapterAutoProbe((poolId, enabled) => {
      setAutoProbeByPool((current) => ({ ...current, [poolId]: enabled }));
    });
  }, []);

  useEffect(() => {
    if (!serviceReady) return;
    let mounted = true;
    void (async () => {
      for (const descriptor of ADAPTER_POOL_DESCRIPTORS) {
        if (!mounted) return;
        if (descriptor.id === "aggregate") continue;
        const result = await resumeAdapterPoolProbe(
          descriptor.id,
          (progress) => {
            if (!mounted) return;
            setProbingPoolId(descriptor.id);
            setProbeProgress(progress);
          },
          () => mounted,
        ).catch(() => null);
        if (!result) continue;
        if (mounted) {
          await queryClient.refetchQueries({
            queryKey: descriptor.queryKey,
            type: "active",
          });
          setProbingPoolId(null);
          setProbeProgress(null);
        }
        return;
      }
    })();
    return () => {
      mounted = false;
    };
  }, [queryClient, serviceReady]);
  const results = useQueries({
    queries: ADAPTER_POOL_DESCRIPTORS.map((descriptor) => ({
      queryKey: descriptor.queryKey,
      queryFn: () => descriptor.load(t),
      enabled: serviceReady,
      staleTime: 10_000,
    })),
  });
  const pools = useMemo<AdapterPoolViewModel[]>(
    () =>
      ADAPTER_POOL_DESCRIPTORS.map((descriptor, index) => {
        const snapshot = results[index]?.data;
        const fallback = descriptor.fallback?.({ codexTotal, codexAvailable });
        return {
          descriptor,
          enabled: snapshot?.enabled ?? false,
          fallbackEnabled: snapshot?.fallbackEnabled ?? false,
          fallbackKeyCount: snapshot?.fallbackKeyCount ?? 0,
          total: snapshot?.total ?? fallback?.total ?? 0,
          available: snapshot?.available ?? fallback?.available ?? 0,
          limited: snapshot?.limited ?? 0,
          disabled: snapshot?.disabled ?? 0,
          error: snapshot?.error ?? 0,
          models: snapshot?.models ?? 0,
          resetCards: snapshot?.resetCards ?? 0,
          items: snapshot?.items ?? [],
        };
      }),
    [codexAvailable, codexTotal, results],
  );
  const expanded = pools.find((pool) => pool.descriptor.id === expandedId) ?? null;

  const togglePool = async (pool: AdapterPoolViewModel, enabled: boolean) => {
    if (!pool.descriptor.setPoolEnabled || busyPoolId) return;
    setBusyPoolId(pool.descriptor.id);
    try {
      await pool.descriptor.setPoolEnabled(pool, enabled);
      await queryClient.invalidateQueries({ queryKey: pool.descriptor.queryKey });
      toast.success(
        enabled
          ? `${pool.descriptor.title} ${t("已启用，将参与调用")}`
          : `${pool.descriptor.title} ${t("已停用，不再参与调用")}`,
      );
    } catch (error) {
      await queryClient.invalidateQueries({ queryKey: pool.descriptor.queryKey });
      toast.error(`${t("操作失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setBusyPoolId(null);
    }
  };

  const toggleFallback = async (pool: AdapterPoolViewModel, enabled: boolean) => {
    if (!pool.descriptor.setFallbackEnabled || fallbackBusy || busyPoolId) return;
    setFallbackBusy(true);
    try {
      const updated = await pool.descriptor.setFallbackEnabled(enabled);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: pool.descriptor.queryKey }),
        queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
      ]);
      toast.success(
        enabled
          ? `${t("中转站兜底已开启")} · ${updated} ${t("个平台 Key")}`
          : `${t("中转站兜底已关闭")} · ${updated} ${t("个平台 Key")}`,
      );
    } catch (error) {
      toast.error(`${t("操作失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setFallbackBusy(false);
    }
  };

  const probePool = async (pool: AdapterPoolViewModel) => {
    if (probingPoolId === pool.descriptor.id) {
      if (pool.descriptor.id === "aggregate") {
        toast.info(t("中转站探测正在进行，请等待完成"));
        return;
      }
      try {
        const requested = await cancelAdapterPoolProbe(pool.descriptor.id);
        if (requested) toast.info(`${pool.descriptor.title} ${t("正在取消探测")}`);
      } catch (error) {
        toast.error(`${t("取消失败")}: ${getAppErrorMessage(error)}`);
      }
      return;
    }
    if (probingPoolId || refreshingPoolId || busyPoolId) return;
    const activeCount = pool.items.filter((item) => item.active).length;
    if (activeCount === 0) {
      toast.warning(t("当前资源池没有启用中的凭据"));
      return;
    }
    setProbingPoolId(pool.descriptor.id);
    try {
      setProbeProgress({ id: "", requested: activeCount, completed: 0, succeeded: 0, failed: 0 });
      const result = await pool.descriptor.probePool(pool, (progress) => {
        setProbeProgress(progress);
        void queryClient.refetchQueries({
          queryKey: pool.descriptor.queryKey,
          type: "active",
        });
      });
      await queryClient.refetchQueries({ queryKey: pool.descriptor.queryKey, type: "active" });
      if ((result.cancelled ?? 0) > 0) {
        toast.info(
          `${pool.descriptor.title} ${t("探测已取消")}: ${result.succeeded} ${t("成功")}, ${result.failed} ${t("失败")}, ${result.cancelled} ${t("未执行")}`,
        );
      } else if (result.failed > 0) {
        toast.warning(
          `${pool.descriptor.title} ${t("探测完成")}: ${result.succeeded} ${t("成功")}, ${result.failed} ${t("失败")}`,
        );
      } else {
        toast.success(`${pool.descriptor.title} ${t("探测完成")}: ${result.succeeded} ${t("条可用凭据")}`);
      }
    } catch (error) {
      toast.error(`${t("探测失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setProbingPoolId(null);
      setProbeProgress(null);
    }
  };

  const refreshPool = async (pool: AdapterPoolViewModel) => {
    if (refreshingPoolId || probingPoolId || busyPoolId) return;
    const activeCount = pool.items.filter((item) => item.active).length;
    if (activeCount === 0) {
      toast.warning(t("当前资源池没有启用中的凭据"));
      return;
    }
    setRefreshingPoolId(pool.descriptor.id);
    try {
      const result = await pool.descriptor.refreshPool(pool);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: pool.descriptor.queryKey }),
        queryClient.invalidateQueries({ queryKey: ["accounts"] }),
        queryClient.invalidateQueries({ queryKey: ["usage"] }),
        queryClient.invalidateQueries({ queryKey: [pool.descriptor.id] }),
      ]);
      if (result.failed > 0) {
        toast.warning(`${pool.descriptor.title} ${t("刷新完成")}: ${result.succeeded} ${t("成功")}, ${result.failed} ${t("失败")}`);
      } else {
        toast.success(`${pool.descriptor.title} ${t("刷新完成")}: ${result.succeeded} ${t("条凭据")}`);
      }
    } catch (error) {
      toast.error(`${t("刷新失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setRefreshingPoolId(null);
    }
  };

  const setAutoProbe = (poolId: AdapterId, enabled: boolean) => {
    if (poolId === "aggregate") writeAggregateAutoProbe(enabled);
    else writeAdapterAutoProbe(poolId, enabled);
    setAutoProbeByPool((current) => ({ ...current, [poolId]: enabled }));
    toast.success(enabled ? t("导入后将自动探测") : t("已关闭导入后自动探测"));
  };

  return (
    <section className="resource-pool-frame space-y-4 rounded-xl p-3 sm:p-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div><div className="flex items-center gap-2 text-xs font-semibold text-primary"><Power className="size-3.5" />{t("资源池总控")}</div><h2 className="mt-1 text-xl font-semibold">{t("统一资源池")}</h2><p className="mt-1 max-w-2xl text-sm text-muted-foreground">{t("先看平台池，再展开账号方块；表格管理保留在高级页面。")}</p></div>
        <div className="flex flex-wrap items-center gap-2"><button type="button" onClick={() => navigateShellPath("/import")} className="inline-flex h-8 items-center rounded-md border border-amber-500/30 bg-amber-500/10 px-3 text-xs font-medium text-amber-700 transition-colors hover:bg-amber-500/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:text-amber-200"><Plus className="mr-1.5 h-3.5 w-3.5" />{t("导入凭据")}</button><button type="button" onClick={() => navigateShellPath("/routing")} className="inline-flex h-8 items-center rounded-md border border-violet-500/30 bg-violet-500/10 px-3 text-xs font-medium text-violet-700 transition-colors hover:bg-violet-500/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:text-violet-200"><Route className="mr-1.5 h-3.5 w-3.5" />{t("智能调度")}</button><span className="text-[11px] text-muted-foreground">{t("当前策略：批次轮询优先")}</span></div>
      </div>
      <div className="grid gap-3 md:grid-cols-2 2xl:grid-cols-4">
        {pools.map((pool) => (
          <PoolOverview
            key={pool.descriptor.id}
            pool={pool}
            expanded={expandedId === pool.descriptor.id}
            onExpand={() => setExpandedId((current) => current === pool.descriptor.id ? null : pool.descriptor.id)}
            onEnabledChange={(enabled) => void togglePool(pool, enabled)}
            onProbe={() => void probePool(pool)}
            onRefresh={() => void refreshPool(pool)}
            onAutoProbeChange={(enabled) => setAutoProbe(pool.descriptor.id, enabled)}
            onFallbackChange={(enabled) => void toggleFallback(pool, enabled)}
            autoProbe={autoProbeByPool[pool.descriptor.id]}
            busy={busyPoolId === pool.descriptor.id}
            probing={probingPoolId === pool.descriptor.id}
            probeProgress={probingPoolId === pool.descriptor.id ? probeProgress : null}
            refreshing={refreshingPoolId === pool.descriptor.id}
            fallbackBusy={fallbackBusy}
            serviceReady={serviceReady}
          />
        ))}
      </div>
      {expanded ? <PoolDetails pool={expanded} /> : null}
    </section>
  );
}
