"use client";

import { useEffect, useMemo, useState } from "react";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Button, buttonVariants } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { accountClient } from "@/lib/api/account-client";
import { appClient } from "@/lib/api/app-client";
import { serviceClient } from "@/lib/api/service-client";
import { useAppStore } from "@/lib/store/useAppStore";
import { useI18n } from "@/lib/i18n/provider";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { findBestMatchingModel } from "@/lib/api/model-catalog";
import {
  type QuotaLimitUnit,
  estimateQuotaLimitUsd,
  formatQuotaLimitUsd,
  formatQuotaLimitValue,
  parseQuotaLimitTokens,
  resolveQuotaLimitUnit,
  sanitizeQuotaLimitValue,
  QUOTA_LIMIT_REFERENCE_PRICE_USD_PER_1K_TOKENS,
} from "@/lib/utils/api-key-quota";
import { toast } from "sonner";
import { useQueryClient, useQuery } from "@tanstack/react-query";
import { Key, Clipboard, ShieldCheck, Info } from "lucide-react";
import type { Account, AggregateApi, ApiKey, ApiKeyOwner, AppUser } from "@/types";

const PROTOCOL_LABELS: Record<string, string> = {
  openai_compat: "通配兼容 (Codex / Claude Code / Gemini CLI)",
  anthropic_native: "通配兼容 (Codex / Claude Code / Gemini CLI)",
  gemini_native: "通配兼容 (Codex / Claude Code / Gemini CLI)",
};

const REASONING_LABELS: Record<string, string> = {
  auto: "跟随请求",
  low: "低 (low)",
  medium: "中 (medium)",
  high: "高 (high)",
  xhigh: "极高 (xhigh)",
};

const SERVICE_TIER_LABELS: Record<string, string> = {
  auto: "跟随请求",
  fast: "Fast",
};

function normalizeEditableServiceTier(value?: string | null): string {
  const normalized = String(value || "").trim().toLowerCase();
  return normalized === "fast" ? "fast" : "";
}

const ROTATION_STRATEGY_LABELS: Record<string, string> = {
  account_rotation: "账号轮转",
  aggregate_api_rotation: "聚合API轮转",
  hybrid_rotation: "混合轮转（账号优先）",
};

const ACCOUNT_PLAN_FILTER_LABELS: Record<string, string> = {
  all: "全部账号",
  free: "Free",
  go: "Go",
  plus: "Plus",
  pro: "Pro",
  team: "Team",
  business: "Business",
  enterprise: "Enterprise",
  edu: "Edu",
  unknown: "未知计划",
};

const ROTATION_ACCOUNT = "account_rotation";
const ROTATION_AGGREGATE_API = "aggregate_api_rotation";
const ROTATION_HYBRID = "hybrid_rotation";
const SOURCE_KIND_OPENAI_ACCOUNT = "openai_account";
const ALL_AGGREGATE_APIS_VALUE = "__all__";

function normalizePlanKey(value?: string | null): string {
  return String(value || "").trim().toLowerCase() || "unknown";
}

function accountMatchesPlanFilter(account: Account, planFilter: string): boolean {
  const normalizedFilter = normalizePlanKey(planFilter);
  if (!normalizedFilter || normalizedFilter === "all") return true;
  return normalizePlanKey(account.planType) === normalizedFilter;
}

function isActiveAggregateApi(api: AggregateApi): boolean {
  return String(api.status || "").trim().toLowerCase() === "active";
}

function aggregateApiDisplayName(api: AggregateApi): string {
  const name = String(api.supplierName || "").trim();
  return name ? `${name} (${api.url})` : api.url;
}

function buildCodexManagerGatewayBaseUrl(addr?: string | null): string {
  const normalized = String(addr || "localhost:48760").trim() || "localhost:48760";
  const withScheme = /^https?:\/\//i.test(normalized)
    ? normalized
    : `http://${normalized}`;
  const withoutTrailingSlash = withScheme.replace(/\/+$/, "");
  return withoutTrailingSlash.endsWith("/v1")
    ? withoutTrailingSlash
    : `${withoutTrailingSlash}/v1`;
}

interface ApiKeyModalProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  apiKey?: ApiKey | null;
  appUsers?: AppUser[];
  apiKeyOwner?: ApiKeyOwner | null;
  distributionEnabled?: boolean;
  isAdminMode?: boolean;
  showMemberOwnership?: boolean;
  onOwnerSaved?: () => Promise<void> | void;
}

function userCanOwnApiKey(user: AppUser): boolean {
  return user.role !== "admin";
}

function appUserLabel(user: AppUser | null | undefined): string {
  if (!user) return "选择可分发成员";
  return user.displayName ? `${user.displayName} (${user.username})` : user.username;
}

/**
 * 函数 `ApiKeyModal`
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
export function ApiKeyModal({
  open,
  onOpenChange,
  apiKey,
  appUsers = [],
  apiKeyOwner,
  distributionEnabled = false,
  isAdminMode = true,
  showMemberOwnership = isAdminMode,
  onOwnerSaved,
}: ApiKeyModalProps) {
  const { t } = useI18n();
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { canAccessManagementRpc, isDesktopRuntime } = useRuntimeCapabilities();
  const [name, setName] = useState("");
  const [protocolType, setProtocolType] = useState("openai_compat");
  const [modelSlug, setModelSlug] = useState("");
  const [reasoningEffort, setReasoningEffort] = useState("");
  const [serviceTier, setServiceTier] = useState("");
  const [rotationStrategy, setRotationStrategy] = useState(ROTATION_ACCOUNT);
  const [aggregateApiId, setAggregateApiId] = useState("");
  const [accountPlanFilter, setAccountPlanFilter] = useState("all");
  const [quotaLimitValue, setQuotaLimitValue] = useState("");
  const [quotaLimitUnit, setQuotaLimitUnit] = useState<QuotaLimitUnit>("k");
  const [upstreamBaseUrl, setUpstreamBaseUrl] = useState("");
  const [customKey, setCustomKey] = useState("");
  const [ownerUserId, setOwnerUserId] = useState("");
  const [generatedKey, setGeneratedKey] = useState("");
  const [syncCodexProviderConfig, setSyncCodexProviderConfig] = useState(false);

  const [isLoading, setIsLoading] = useState(false);
  const queryClient = useQueryClient();
  const isServiceReady = canAccessManagementRpc && serviceStatus.connected;
  const memberOwnershipEnabled = isAdminMode && showMemberOwnership;
  const usesAccountPlanFilter =
    rotationStrategy === ROTATION_ACCOUNT ||
    rotationStrategy === ROTATION_HYBRID;
  const usesAggregateRouting =
    rotationStrategy === ROTATION_AGGREGATE_API ||
    rotationStrategy === ROTATION_HYBRID;
  const usesAggregateApiSelector = rotationStrategy === ROTATION_AGGREGATE_API;
  const billableUsers = useMemo(
    () => appUsers.filter((user) => userCanOwnApiKey(user)),
    [appUsers],
  );
  const billableUsersById = useMemo(
    () => new Map(billableUsers.map((user) => [user.id, user])),
    [billableUsers],
  );
  const unavailableMessage = canAccessManagementRpc
    ? t("服务未连接，平台密钥与模型配置暂不可编辑；连接恢复后可继续操作。")
    : t("当前运行环境暂不支持平台密钥管理。");

  const { data: models } = useQuery({
    queryKey: ["apikey-models"],
    queryFn: async () => {
      const cached = await accountClient.listModels(false);
      if (cached.models.length > 0) {
        return cached;
      }
      try {
        return await accountClient.listModels(true);
      } catch {
        return cached;
      }
    },
    enabled: open && isServiceReady,
  });

  const { data: aggregateApis } = useQuery({
    queryKey: ["aggregate-apis"],
    queryFn: () => accountClient.listAggregateApis(),
    enabled: open && isServiceReady && isAdminMode && usesAggregateRouting,
  });

  const { data: accountList } = useQuery({
    queryKey: ["accounts", "api-key-modal"],
    queryFn: () => accountClient.list(),
    enabled: open && isServiceReady && isAdminMode && usesAccountPlanFilter,
  });

  const { data: modelRouting } = useQuery({
    queryKey: ["model-routing"],
    queryFn: () => accountClient.listManagedModelRouting(),
    enabled: open && isServiceReady && isAdminMode,
  });

  const selectedModelInfo = useMemo(
    () => findBestMatchingModel(models?.models || [], modelSlug),
    [modelSlug, models?.models],
  );

  const aggregateApiItems = aggregateApis || [];
  const activeAggregateApis = useMemo(
    () => aggregateApiItems.filter(isActiveAggregateApi),
    [aggregateApiItems],
  );
  const aggregateApiSelectItems = useMemo(() => {
    const selectedId = String(aggregateApiId || "").trim();
    if (!selectedId) return activeAggregateApis;
    const selected = aggregateApiItems.find((api) => api.id === selectedId);
    if (!selected || activeAggregateApis.some((api) => api.id === selectedId)) {
      return activeAggregateApis;
    }
    return [selected, ...activeAggregateApis];
  }, [activeAggregateApis, aggregateApiId, aggregateApiItems]);

  const eligibleAccountIds = useMemo(() => {
    const ids = new Set<string>();
    for (const account of accountList?.items || []) {
      if (
        String(account.status || "").trim().toLowerCase() === "active" &&
        account.hasToken &&
        accountMatchesPlanFilter(account, accountPlanFilter)
      ) {
        ids.add(account.id);
      }
    }
    return ids;
  }, [accountList?.items, accountPlanFilter]);

  const aggregateSourceIds = useMemo(() => {
    const ids = new Set<string>();
    if (!usesAggregateRouting) return ids;
    const selectedId = String(aggregateApiId || "").trim();
    if (usesAggregateApiSelector && selectedId) {
      ids.add(selectedId);
      return ids;
    }
    for (const api of activeAggregateApis) {
      ids.add(api.id);
    }
    return ids;
  }, [
    activeAggregateApis,
    aggregateApiId,
    usesAggregateApiSelector,
    usesAggregateRouting,
  ]);

  const routeSupportedModelSlugs = useMemo(() => {
    if (!isAdminMode) return null;
    if (usesAccountPlanFilter && !modelRouting) return null;
    const slugs = new Set<string>();
    const hasAccountList = Boolean(accountList);
    const hasAggregateApiList = Boolean(aggregateApis);

    if (usesAccountPlanFilter) {
      for (const mapping of modelRouting?.mappings || []) {
        if (!mapping.enabled || mapping.sourceKind !== SOURCE_KIND_OPENAI_ACCOUNT) {
          continue;
        }
        if (!hasAccountList || eligibleAccountIds.has(mapping.sourceId)) {
          slugs.add(mapping.platformModelSlug);
        }
      }
      if (hasAccountList) {
        for (const account of accountList?.items || []) {
          if (!eligibleAccountIds.has(account.id)) continue;
          for (const slug of account.modelSlugs || []) {
            const normalized = String(slug || "").trim();
            if (normalized) slugs.add(normalized);
          }
        }
      }
    }

    if (usesAggregateRouting) {
      if (hasAggregateApiList) {
        for (const api of aggregateApiItems) {
          if (!aggregateSourceIds.has(api.id)) continue;
          for (const slug of api.modelSlugs || []) {
            const normalized = String(slug || "").trim();
            if (normalized) slugs.add(normalized);
          }
        }
      }
    }

    return slugs;
  }, [
    accountList,
    aggregateApiItems,
    aggregateApis,
    aggregateSourceIds,
    eligibleAccountIds,
    isAdminMode,
    modelRouting,
    usesAccountPlanFilter,
    usesAggregateRouting,
  ]);

  const visibleModels = useMemo(() => {
    const catalog = models?.models || [];
    const selectedSlug = String(modelSlug || "").trim();
    const baseModels = catalog.filter((model) => {
      if (
        routeSupportedModelSlugs &&
        !routeSupportedModelSlugs.has(model.slug) &&
        model.slug !== selectedSlug
      ) {
        return false;
      }
      if (model.supportedInApi) {
        return true;
      }
      return Boolean(selectedSlug) && model.slug === selectedModelInfo?.slug;
    });
    if (selectedModelInfo && selectedModelInfo.slug !== selectedSlug) {
      return [
        {
          ...selectedModelInfo,
          slug: selectedSlug,
          displayName: selectedModelInfo.displayName || selectedSlug,
        },
        ...baseModels,
      ];
    }
    return baseModels;
  }, [modelSlug, models?.models, routeSupportedModelSlugs, selectedModelInfo]);

  const modelLabelMap = Object.fromEntries(
    visibleModels.map((model) => [model.slug, model.displayName || model.slug]),
  );

  const quotaLimitTokenPreview = useMemo(
    () => parseQuotaLimitTokens(quotaLimitValue, quotaLimitUnit),
    [quotaLimitUnit, quotaLimitValue],
  );
  const quotaLimitUsdPreview = estimateQuotaLimitUsd(quotaLimitTokenPreview);

  useEffect(() => {
    if (!open) return;

    if (!apiKey) {
      setName("");
      setProtocolType("openai_compat");
      setModelSlug("");
      setReasoningEffort("");
      setServiceTier("");
      setRotationStrategy(ROTATION_ACCOUNT);
      setAggregateApiId("");
      setAccountPlanFilter("all");
      setQuotaLimitValue("");
      setQuotaLimitUnit("k");
      setUpstreamBaseUrl("");
      setCustomKey("");
      setOwnerUserId(
        memberOwnershipEnabled && distributionEnabled ? billableUsers[0]?.id || "" : "",
      );
      setGeneratedKey("");
      setSyncCodexProviderConfig(false);
      return;
    }

    setName(apiKey.name || "");
    setProtocolType("openai_compat");
    setModelSlug(apiKey.modelSlug || "");
    setReasoningEffort(apiKey.reasoningEffort || "");
    setServiceTier(normalizeEditableServiceTier(apiKey.serviceTier));
    setRotationStrategy(apiKey.rotationStrategy || ROTATION_ACCOUNT);
    setAggregateApiId(apiKey.aggregateApiId || "");
    setAccountPlanFilter(apiKey.accountPlanFilter || "all");
    const resolvedQuotaUnit = resolveQuotaLimitUnit(apiKey.quotaLimitTokens);
    setQuotaLimitUnit(resolvedQuotaUnit);
    setQuotaLimitValue(
      formatQuotaLimitValue(apiKey.quotaLimitTokens, resolvedQuotaUnit),
    );
    setGeneratedKey("");
    setCustomKey("");
    setUpstreamBaseUrl(apiKey.upstreamBaseUrl || "");
    setSyncCodexProviderConfig(false);
    setOwnerUserId(
      memberOwnershipEnabled && apiKeyOwner?.ownerKind === "user"
        ? apiKeyOwner.ownerUserId || ""
        : "",
    );
  }, [
    apiKey,
    apiKeyOwner,
    billableUsers,
    distributionEnabled,
    isDesktopRuntime,
    memberOwnershipEnabled,
    open,
  ]);

  useEffect(() => {
    if (!usesAggregateApiSelector && aggregateApiId) {
      setAggregateApiId("");
    }
  }, [aggregateApiId, usesAggregateApiSelector]);

  const handleQuotaLimitUnitChange = (unit: QuotaLimitUnit) => {
    const currentTokens = parseQuotaLimitTokens(quotaLimitValue, quotaLimitUnit);
    setQuotaLimitUnit(unit);
    if (currentTokens !== null) {
      setQuotaLimitValue(formatQuotaLimitValue(currentTokens, unit));
    }
  };

  /**
   * 函数 `handleSave`
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
  const handleSave = async () => {
    if (!isServiceReady) {
      toast.info(
        canAccessManagementRpc
          ? t("服务未连接，暂时无法保存平台密钥")
          : t("当前运行环境暂不支持平台密钥管理"),
      );
      return;
    }
    setIsLoading(true);
    try {
      const normalizedOwnerUserId =
        memberOwnershipEnabled && ownerUserId && ownerUserId !== "__none__"
          ? ownerUserId
          : "";
      if (memberOwnershipEnabled && distributionEnabled && !normalizedOwnerUserId) {
        throw new Error(t("请选择平台 Key 归属成员"));
      }
      const params = {
        name: name || null,
        modelSlug: !modelSlug || modelSlug === "auto" ? null : modelSlug,
        reasoningEffort:
          !reasoningEffort || reasoningEffort === "auto"
            ? null
            : reasoningEffort,
        serviceTier:
          !serviceTier || serviceTier === "auto" ? null : serviceTier,
        protocolType,
        upstreamBaseUrl: upstreamBaseUrl || null,
        staticHeadersJson: null,
        rotationStrategy: isAdminMode ? rotationStrategy : ROTATION_ACCOUNT,
        aggregateApiId:
          isAdminMode && usesAggregateApiSelector && aggregateApiId
            ? aggregateApiId
            : null,
        accountPlanFilter:
          isAdminMode && usesAccountPlanFilter && accountPlanFilter !== "all"
            ? accountPlanFilter
            : null,
        quotaLimitTokens: quotaLimitTokenPreview,
        customKey: !apiKey?.id && customKey.trim() ? customKey.trim() : null,
      };

      let savedKeyId = apiKey?.id || "";
      if (apiKey?.id) {
        await accountClient.updateApiKey(apiKey.id, params);
        savedKeyId = apiKey.id;
        toast.success(t("密钥配置已更新"));
      } else {
        const result = await accountClient.createApiKey(params);
        savedKeyId = result.id;
        setGeneratedKey(result.key);
        toast.success(t("平台密钥已创建"));
      }
      if (memberOwnershipEnabled && savedKeyId && normalizedOwnerUserId) {
        await appClient.setApiKeyOwner({
          keyId: savedKeyId,
          ownerKind: "user",
          ownerUserId: normalizedOwnerUserId,
          projectId: null,
        });
        await onOwnerSaved?.();
      }

      if (isDesktopRuntime && syncCodexProviderConfig) {
        await serviceClient.syncCodexProviderConfig({
          baseUrl: buildCodexManagerGatewayBaseUrl(serviceStatus.addr),
          providerId: "codexmanager",
          providerName: "CodexManager Local",
          envKey: "CODEXMANAGER_API_KEY",
        });
        toast.success(t("已更新本机 Codex CLI custom provider 配置"));
      }

      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
        queryClient.invalidateQueries({ queryKey: ["apikey-models"] }),
        queryClient.invalidateQueries({
          queryKey: ["account-manager", "api-key-owners"],
        }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
        queryClient.invalidateQueries({ queryKey: ["dashboard", "member-summary"] }),
      ]);
      if (apiKey?.id) onOpenChange(false);
    } catch (err: unknown) {
      toast.error(
        `操作失败: ${err instanceof Error ? err.message : String(err)}`,
      );
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * 函数 `copyKey`
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
  const copyKey = async () => {
    try {
      await copyTextToClipboard(generatedKey);
      toast.success(t("密钥已复制"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[calc(100%-2rem)] max-w-[calc(100%-2rem)] sm:max-w-[680px] md:max-w-[760px] max-h-[90vh] overflow-y-auto glass-card">
        <DialogHeader>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-full bg-primary/10">
              <Key className="h-5 w-5 text-primary" />
            </div>
            <DialogTitle>
              {apiKey?.id ? t("编辑平台密钥") : t("创建平台密钥")}
            </DialogTitle>
          </div>
          <DialogDescription>
            {t("配置网关访问凭据，您可以绑定特定模型、推理等级或自定义上游。")}
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-5 py-4">
          {!isServiceReady ? (
            <Alert>
              <Info />
              <AlertDescription>{unavailableMessage}</AlertDescription>
            </Alert>
          ) : null}
          <div className="grid grid-cols-2 gap-4 items-start">
            <div className="grid gap-2 content-start">
              <Label htmlFor="name">{t("密钥名称 (可选)")}</Label>
              <Input
                id="name"
                placeholder={t("例如：主机房 / 测试")}
                value={name}
                disabled={!isServiceReady}
                onChange={(e) => setName(e.target.value)}
              />
            </div>
            {isAdminMode ? (
            <>
            <div className="grid gap-2 content-start">
              <Label>{t("轮转策略")}</Label>
              <Select
                value={rotationStrategy}
                onValueChange={(val) => {
                  if (!val) return;
                  setRotationStrategy(val);
                }}
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                  <SelectValue>
                    {(value) =>
                      t(ROTATION_STRATEGY_LABELS[String(value || "")] || "账号轮转")
                    }
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                    <SelectGroup>
                  <SelectItem value="account_rotation">{t("账号轮转")}</SelectItem>
                  <SelectItem value="aggregate_api_rotation">
                    {t("聚合API轮转")}
                  </SelectItem>
                  <SelectItem value="hybrid_rotation">
                    {t("混合轮转（账号优先）")}
                  </SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <p className="col-span-2 -mt-1 text-[11px] text-muted-foreground">
              {t(
                "账号轮转只走账号池；聚合API轮转只走聚合API；混合轮转先走账号池，账号耗尽后使用聚合API兜底。",
              )}
            </p>
            </>
            ) : null}
          </div>

          {!apiKey?.id ? (
            <div className="grid gap-2">
              <Label htmlFor="customKey">{t("自定义 API Key (可选)")}</Label>
              <Input
                id="customKey"
                type="password"
                autoComplete="off"
                spellCheck={false}
                placeholder={t("留空则自动生成")}
                value={customKey}
                disabled={!isServiceReady}
                onChange={(e) => setCustomKey(e.target.value)}
              />
              <p className="text-[11px] text-muted-foreground">
                {t(
                  "用于复用固定 OPENAI_API_KEY；填写后将按该值创建平台密钥，留空则继续随机生成。",
                )}
              </p>
            </div>
          ) : null}

          {isAdminMode && usesAccountPlanFilter ? (
            <div className="grid gap-2">
              <Label>{t("账号组筛选")}</Label>
              <Select
                value={accountPlanFilter}
                onValueChange={(val) => val && setAccountPlanFilter(val)}
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                  <SelectValue>
                    {(value) =>
                      t(
                        ACCOUNT_PLAN_FILTER_LABELS[String(value || "")] ||
                          "全部账号",
                      )
                    }
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                    <SelectGroup>
                  {Object.entries(ACCOUNT_PLAN_FILTER_LABELS).map(
                    ([value, label]) => (
                      <SelectItem key={value} value={value}>
                        {t(label)}
                      </SelectItem>
                    ),
                  )}
                  </SelectGroup>
                </SelectContent>
              </Select>
              <p className="text-[11px] text-muted-foreground">
                {t(
                  "仅对账号轮转和混合轮转生效，可限制这把平台密钥只从指定账号计划类型中选路由账号。",
                )}
              </p>
            </div>
          ) : null}

          {isAdminMode && usesAggregateApiSelector ? (
            <div className="grid gap-2">
              <Label>{t("聚合 API 来源")}</Label>
              <Select
                value={aggregateApiId || ALL_AGGREGATE_APIS_VALUE}
                onValueChange={(val) =>
                  setAggregateApiId(
                    val === ALL_AGGREGATE_APIS_VALUE ? "" : String(val || ""),
                  )
                }
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                  <SelectValue>
                    {(value) => {
                      const selectedId = String(value || "");
                      if (!selectedId || selectedId === ALL_AGGREGATE_APIS_VALUE) {
                        return t("全部启用聚合 API");
                      }
                      const selected = aggregateApiSelectItems.find(
                        (api) => api.id === selectedId,
                      );
                      return selected ? aggregateApiDisplayName(selected) : selectedId;
                    }}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                  <SelectGroup>
                    <SelectItem value={ALL_AGGREGATE_APIS_VALUE}>
                      {t("全部启用聚合 API")}
                    </SelectItem>
                    {aggregateApiSelectItems.map((api) => (
                      <SelectItem key={api.id} value={api.id}>
                        {aggregateApiDisplayName(api)}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
              <p className="text-[11px] text-muted-foreground">
                {t("选择具体来源后，这把平台密钥只使用该聚合 API，模型列表也只显示该来源可路由模型。")}
              </p>
            </div>
          ) : null}

          {memberOwnershipEnabled ? (
            <div className="grid gap-2">
              <Label>{t("归属成员")}</Label>
              <Select
                value={ownerUserId || "__none__"}
                onValueChange={(val) =>
                  setOwnerUserId(val === "__none__" ? "" : String(val || ""))
                }
                disabled={!isServiceReady || billableUsers.length === 0}
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={t("选择可分发成员")}>
                    {(value) => {
                      const id = String(value || "");
                      if (!id || id === "__none__") return t("未分配");
                      return appUserLabel(billableUsersById.get(id));
                    }}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                  <SelectGroup>
                    <SelectItem value="__none__">{t("未分配")}</SelectItem>
                    {billableUsers.map((user) => (
                      <SelectItem key={user.id} value={user.id}>
                        {appUserLabel(user)}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
              <p className="text-[11px] text-muted-foreground">
                {distributionEnabled
                  ? t("额度分发开启时，平台 Key 必须归属到一个成员钱包。")
                  : t("未开启额度分发时可先不分配，开启后再补齐归属。")}
              </p>
            </div>
          ) : null}

          <div className="grid gap-2">
            <Label htmlFor="quotaLimitTokens">{t("总额度限制 (Token，可选)")}</Label>
            <div className="grid grid-cols-[minmax(0,1fr)_92px] gap-2">
              <Input
                id="quotaLimitTokens"
                inputMode="decimal"
                min={0}
                placeholder={t("不填表示不限制")}
                value={quotaLimitValue}
                disabled={!isServiceReady}
                onChange={(e) =>
                  setQuotaLimitValue(sanitizeQuotaLimitValue(e.target.value))
                }
              />
              <Select
                value={quotaLimitUnit}
                onValueChange={(value) =>
                  handleQuotaLimitUnitChange(value as QuotaLimitUnit)
                }
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent align="end">
                  <SelectGroup>
                    <SelectItem value="k">{t("K")}</SelectItem>
                    <SelectItem value="m">{t("M")}</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
            </div>
            <p className="text-[11px] text-muted-foreground">
              {quotaLimitTokenPreview === null
                ? t(
                    "达到上限后，这把平台密钥的新请求会被拒绝；已在途请求会按完成后的真实用量继续统计。",
                  )
                : `${t("折算")} ${quotaLimitTokenPreview.toLocaleString(
                    "zh-CN",
                  )} Token ≈ ${formatQuotaLimitUsd(quotaLimitUsdPreview)} (${t(
                    "按",
                  )} $${QUOTA_LIMIT_REFERENCE_PRICE_USD_PER_1K_TOKENS.toFixed(
                    2,
                  )} / 1K Token ${t("参考估算")})`}
            </p>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="grid gap-2 content-start">
              <Label>{t("协议类型")}</Label>
              <Select
                value={protocolType}
                onValueChange={(val) => val && setProtocolType(val)}
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                  <SelectValue>
                    {(value) =>
                      t(
                        PROTOCOL_LABELS[String(value || "")] ||
                          "通配兼容 (Codex / Claude Code / Gemini CLI)",
                      )
                    }
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                    <SelectGroup>
                  <SelectItem value="openai_compat">
                    {t("通配兼容 (Codex / Claude Code / Gemini CLI)")}
                  </SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
              <p className="min-h-[32px] text-[11px] text-muted-foreground">
                {t("默认按路径通配：")}<code>/v1/messages*</code> {t("走 Claude 语义，")}<code>/v1beta/models/*:generateContent</code> {t("这类路径走 Gemini 语义，其它标准路径走 Codex / OpenAI 语义。")}
              </p>
            </div>
            <div className="grid gap-2 content-start">
              <Label>{t("绑定模型 (可选)")}</Label>
              <Select
                value={modelSlug}
                onValueChange={(val) => val && setModelSlug(val)}
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={t("跟随请求")}>
                    {(value) => {
                      const nextValue = String(value || "").trim();
                      if (!nextValue || nextValue === "auto") return t("跟随请求");
                      const resolvedModel = findBestMatchingModel(
                        models?.models || [],
                        nextValue,
                      );
                      return resolvedModel?.displayName || modelLabelMap[nextValue] || nextValue;
                    }}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                    <SelectGroup>
                  <SelectItem value="auto">{t("跟随请求")}</SelectItem>
                  {visibleModels.map((model) => (
                    <SelectItem key={model.slug} value={model.slug}>
                      {model.displayName || model.slug}
                    </SelectItem>
                  ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
              <p className="text-[11px] text-muted-foreground">
                {t("选择“跟随请求”时，会使用请求体里的实际模型；请求日志展示的是最终生效模型。")}
              </p>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-4">
            <div className="grid gap-2 content-start">
              <Label>{t("推理等级 (可选)")}</Label>
              <Select
                value={reasoningEffort}
                onValueChange={(val) => val && setReasoningEffort(val)}
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                    <SelectValue placeholder={t("跟随请求等级")}>
                    {(value) => {
                      const nextValue = String(value || "").trim();
                      if (!nextValue) return t("跟随请求等级");
                      return t(REASONING_LABELS[nextValue] || nextValue);
                    }}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                    <SelectGroup>
                  <SelectItem value="auto">{t("跟随请求")}</SelectItem>
                  <SelectItem value="low">{t("低 (low)")}</SelectItem>
                  <SelectItem value="medium">{t("中 (medium)")}</SelectItem>
                  <SelectItem value="high">{t("高 (high)")}</SelectItem>
                  <SelectItem value="xhigh">{t("极高 (xhigh)")}</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
              <p className="min-h-[32px] text-[11px] text-muted-foreground">
                {t("会覆盖请求里的 reasoning effort。")}
              </p>
            </div>
            <div className="grid gap-2 content-start">
              <Label>{t("服务等级 (可选)")}</Label>
              <Select
                value={serviceTier}
                onValueChange={(val) => val && setServiceTier(val)}
                disabled={!isServiceReady}
              >
                <SelectTrigger className="w-full">
                    <SelectValue placeholder={t("跟随请求")}>
                    {(value) => {
                      const nextValue = String(value || "").trim();
                      if (!nextValue) return t("跟随请求");
                      return t(SERVICE_TIER_LABELS[nextValue] || nextValue);
                    }}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent align="start">
                    <SelectGroup>
                  <SelectItem value="auto">{t("跟随请求")}</SelectItem>
                  <SelectItem value="fast">Fast</SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
              <p className="text-[11px] text-muted-foreground">
                {t("Fast 会映射为上游 priority；未设置时跟随请求。")}
              </p>
            </div>
          </div>

          {isDesktopRuntime ? (
            <div className="flex items-start gap-3 rounded-md border border-border/70 bg-muted/30 p-3">
              <Checkbox
                id="syncCodexProviderConfig"
                checked={syncCodexProviderConfig}
                disabled={!isServiceReady}
                onCheckedChange={(checked) =>
                  setSyncCodexProviderConfig(checked === true)
                }
              />
              <div className="grid gap-1">
                <Label htmlFor="syncCodexProviderConfig" className="text-sm">
                  {t("同步本机 Codex CLI custom provider")}
                </Label>
                <p className="text-[11px] leading-5 text-muted-foreground">
                  {t(
                    "仅更新 `~/.codex/config.toml` 里的 `[model_providers.codexmanager]`，不会修改默认模型、model_provider、review_model，也不会写入 `auth.json`。",
                  )}
                </p>
              </div>
            </div>
          ) : null}

          {generatedKey && (
            <div className="space-y-2 pt-4 border-t">
              <Label className="text-xs text-primary flex items-center gap-1.5">
                <ShieldCheck className="h-3.5 w-3.5" /> {t("平台密钥已生成")}
              </Label>
              <div className="flex gap-2">
                <Input
                  value={generatedKey}
                  readOnly
                  className="font-mono text-sm bg-primary/5"
                />
                <Button
                  variant="outline"
                  onClick={() => void copyKey()}
                  disabled={!generatedKey}
                >
                  <Clipboard className="h-4 w-4" />
                </Button>
              </div>
            </div>
          )}
        </div>

        <DialogFooter>
          <DialogClose
            className={buttonVariants({ variant: "ghost" })}
            type="button"
          >
            {generatedKey ? t("关闭") : t("取消")}
          </DialogClose>
          {!generatedKey && (
            <Button
              onClick={handleSave}
              disabled={!isServiceReady || isLoading}
            >
              {isLoading ? t("保存中...") : t("完成")}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
