"use client";

import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  DollarSign,
  Copy,
  Eye,
  EyeOff,
  ExternalLink,
  Link2,
  MoreVertical,
  Plus,
  Settings2,
  Zap,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { ApiKeyModal } from "@/components/modals/api-key-modal";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useApiKeys } from "@/hooks/useApiKeys";
import {
  isAdminRole,
  resolveSessionRole,
  useAppSession,
} from "@/hooks/useAppSession";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { useI18n } from "@/lib/i18n/provider";
import { accountClient } from "@/lib/api/account-client";
import { appClient } from "@/lib/api/app-client";
import {
  buildGeminiGatewayEndpoint,
  buildOpenAiGatewayEndpoint,
  resolveGatewayOrigin,
} from "@/lib/gateway/endpoints";
import { useAppStore } from "@/lib/store/useAppStore";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import {
  buildCcSwitchProviderImportUrl,
  buildCcSwitchProviderName,
  normalizeCodexManagerGatewayEndpoint,
} from "@/lib/utils/ccswitch";
import {
  estimateQuotaLimitUsd,
  formatQuotaLimitUsd,
} from "@/lib/utils/api-key-quota";
import { formatLocalMinuteFromSeconds } from "@/lib/utils/time";
import { formatCompactNumber } from "@/lib/utils/usage";
import type { ApiKeyOwner, AppUser } from "@/types";

const ROTATION_STRATEGY_LABELS: Record<string, string> = {
  account_rotation: "账号轮转",
  aggregate_api_rotation: "聚合API轮转",
  hybrid_rotation: "混合轮转（账号优先）",
};

function userCanOwnApiKey(user: AppUser): boolean {
  return user.role !== "admin";
}

function appUserLabel(user: AppUser | null | undefined): string {
  if (!user) return "未分配";
  return user.displayName ? `${user.displayName} (${user.username})` : user.username;
}

function resolveApiKeyOwnerLabel(
  owner: ApiKeyOwner | undefined,
  usersById: Map<string, AppUser>,
  distributionEnabled: boolean,
  t: (value: string) => string,
): string {
  if (!owner) return distributionEnabled ? t("未分配") : t("未启用");
  if (owner.ownerKind === "user") {
    return appUserLabel(owner.ownerUserId ? usersById.get(owner.ownerUserId) : null);
  }
  return owner.projectId ? `${t("项目")} ${owner.projectId}` : t("未分配");
}

/**
 * 函数 `formatUsd`
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
function formatUsd(value: number): string {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(Math.max(0, value));
}

/**
 * 函数 `formatCompactTokenAmount`
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
function formatCompactTokenAmount(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value)
      ? Math.max(0, value)
      : 0;
  if (normalized < 1000) {
    return normalized.toLocaleString("zh-CN", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
  }
  return formatCompactNumber(normalized, "0.00", 2, true);
}

/**
 * 函数 `ApiKeyStatCard`
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
function ApiKeyStatCard({
  title,
  value,
  icon: Icon,
  color,
  sub,
}: {
  title: string;
  value: string;
  icon: typeof Zap;
  color: string;
  sub: string;
}) {
  return (
    <Card className="glass-card overflow-hidden shadow-sm transition-colors">
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{title}</CardTitle>
        <Icon className={color} />
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">{value}</div>
        <p className="mt-1 text-[10px] text-muted-foreground">{sub}</p>
      </CardContent>
    </Card>
  );
}

export default function ApiKeysPage() {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const { isDesktopRuntime, mode } = useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const isAdminMode = isAdminRole(role);
  const showMemberOwnership = isAdminMode && session?.mode === "accounts";
  const serviceAddr = useAppStore((state) => state.serviceStatus.addr);
  const {
    apiKeys,
    isLoading,
    isModelsLoading,
    isServiceReady,
    deleteApiKey,
    toggleApiKeyStatus,
    readApiKeySecret,
    isToggling,
  } = useApiKeys();
  const isPageActive = useDesktopPageActive("/apikeys/");
  const isUsageQueryEnabled = useDeferredDesktopActivation(isServiceReady);
  usePageTransitionReady(
    "/apikeys/",
    !isServiceReady || (!isLoading && !isModelsLoading),
  );
  const [revealedSecrets, setRevealedSecrets] = useState<
    Record<string, string>
  >({});
  const [loadingSecretId, setLoadingSecretId] = useState<string | null>(null);
  const [apiKeyModalOpen, setApiKeyModalOpen] = useState(false);
  const [editingKeyId, setEditingKeyId] = useState<string | null>(null);
  const [deleteKeyId, setDeleteKeyId] = useState<string | null>(null);
  const [ccSwitchImportingId, setCcSwitchImportingId] = useState<string | null>(
    null,
  );
  const [browserOrigin, setBrowserOrigin] = useState("");
  const { data: accountManagerStatus } = useQuery({
    queryKey: ["account-manager", "status"],
    queryFn: () => appClient.getAccountManagerStatus(),
    enabled: isUsageQueryEnabled && isPageActive && showMemberOwnership,
    retry: 1,
  });
  const { data: appUsers = [] } = useQuery<AppUser[]>({
    queryKey: ["account-manager", "users"],
    queryFn: () => appClient.listAppUsers(),
    enabled: isUsageQueryEnabled && isPageActive && showMemberOwnership,
    retry: 1,
  });
  const { data: apiKeyOwners = [] } = useQuery<ApiKeyOwner[]>({
    queryKey: ["account-manager", "api-key-owners"],
    queryFn: () => appClient.listApiKeyOwners(),
    enabled: isUsageQueryEnabled && isPageActive && showMemberOwnership,
    retry: 1,
  });
  const billableAppUsers = useMemo(
    () => appUsers.filter((user) => userCanOwnApiKey(user)),
    [appUsers],
  );
  const appUsersById = useMemo(
    () => new Map(appUsers.map((user) => [user.id, user])),
    [appUsers],
  );
  const ownerByKeyId = useMemo(
    () => new Map(apiKeyOwners.map((owner) => [owner.keyId, owner])),
    [apiKeyOwners],
  );
  const distributionEnabled =
    showMemberOwnership && Boolean(accountManagerStatus?.distributionEnabled);
  const gatewayOrigin = useMemo(
    () =>
      resolveGatewayOrigin({
        browserOrigin,
        runtimeMode: mode,
        serviceAddr,
      }),
    [browserOrigin, mode, serviceAddr],
  );
  const openAiEndpoint = useMemo(
    () => buildOpenAiGatewayEndpoint(gatewayOrigin),
    [gatewayOrigin],
  );
  const nativeProtocolEndpoint = useMemo(
    () => buildGeminiGatewayEndpoint(gatewayOrigin),
    [gatewayOrigin],
  );

  useEffect(() => {
    if (mode !== "web-gateway" || typeof window === "undefined") {
      setBrowserOrigin("");
      return;
    }
    setBrowserOrigin(window.location.origin);
  }, [mode]);

  useEffect(() => {
    if (isPageActive) {
      return;
    }
    setApiKeyModalOpen(false);
    setEditingKeyId(null);
    setDeleteKeyId(null);
    setCcSwitchImportingId(null);
  }, [isPageActive]);

  const editingApiKey = useMemo(
    () => apiKeys.find((item) => item.id === editingKeyId) || null,
    [apiKeys, editingKeyId],
  );
  const handleOwnerSaved = async () => {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: ["account-manager", "api-key-owners"],
      }),
      queryClient.invalidateQueries({
        queryKey: ["account-manager", "users"],
      }),
      queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
    ]);
  };
  const { data: usageOverview, isPending: isUsageOverviewLoading } = useQuery({
    queryKey: ["apikey-usage-overview", serviceAddr || null],
    queryFn: async () => {
      const stats = await accountClient.listApiKeyUsageStats();
      const usageByKey = stats.reduce<Record<string, number>>(
        (result, item) => {
          const keyId = String(item.keyId || "").trim();
          if (!keyId) return result;
          result[keyId] = Math.max(0, item.totalTokens || 0);
          return result;
        },
        {},
      );
      const costByKey = stats.reduce<Record<string, number>>((result, item) => {
        const keyId = String(item.keyId || "").trim();
        if (!keyId) return result;
        result[keyId] = Math.max(0, item.estimatedCostUsd || 0);
        return result;
      }, {});

      const totalTokens = Object.values(usageByKey).reduce(
        (sum, value) => sum + value,
        0,
      );
      const totalCostUsd = stats.reduce(
        (sum, item) => sum + Math.max(0, item.estimatedCostUsd || 0),
        0,
      );
      return {
        usageByKey,
        costByKey,
        totalTokens,
        totalCostUsd,
      };
    },
    enabled: isUsageQueryEnabled && isPageActive,
    retry: 1,
  });
  const usageByKey = usageOverview?.usageByKey || {};
  const costByKey = usageOverview?.costByKey || {};
  const showOverviewLoading =
    isServiceReady && isPageActive && isUsageOverviewLoading;

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
    setEditingKeyId(null);
    setApiKeyModalOpen(true);
  };

  /**
   * 函数 `openEditModal`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - id: 参数 id
   *
   * # 返回
   * 返回函数执行结果
   */
  const openEditModal = (id: string) => {
    setEditingKeyId(id);
    setApiKeyModalOpen(true);
  };

  /**
   * 函数 `ensureSecretLoaded`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - id: 参数 id
   *
   * # 返回
   * 返回函数执行结果
   */
  const ensureSecretLoaded = async (id: string) => {
    if (revealedSecrets[id]) {
      return revealedSecrets[id];
    }
    setLoadingSecretId(id);
    try {
      const secret = await readApiKeySecret(id);
      if (!secret) {
        throw new Error("后端未返回密钥明文");
      }
      setRevealedSecrets((current) => ({ ...current, [id]: secret }));
      return secret;
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
   * - id: 参数 id
   *
   * # 返回
   * 返回函数执行结果
   */
  const toggleSecret = async (id: string) => {
    if (revealedSecrets[id]) {
      setRevealedSecrets((current) => {
        const nextState = { ...current };
        delete nextState[id];
        return nextState;
      });
      return;
    }

    try {
      await ensureSecretLoaded(id);
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  /**
   * 函数 `copyToClipboard`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - id: 参数 id
   *
   * # 返回
   * 返回函数执行结果
   */
  const copyToClipboard = async (id: string) => {
    try {
      const secret = await ensureSecretLoaded(id);
      await copyTextToClipboard(secret);
      toast.success(t("已复制到剪贴板"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const copyEndpoint = async (endpoint: string) => {
    try {
      await copyTextToClipboard(endpoint);
      toast.success(t("端点已复制"));
    } catch (error: unknown) {
      toast.error(error instanceof Error ? error.message : String(error));
    }
  };

  const openCcSwitchImportUrl = async (url: string) => {
    await appClient.openExternalUrl(url);
  };

  const importToCcSwitch = async (key: (typeof apiKeys)[number]) => {
    setCcSwitchImportingId(key.id);
    try {
      const secret = await ensureSecretLoaded(key.id);
      const importUrl = buildCcSwitchProviderImportUrl({
        app: "codex",
        name: buildCcSwitchProviderName(key.name, key.id),
        endpoint: normalizeCodexManagerGatewayEndpoint(serviceAddr, {
          preferPublicOrigin: mode === "web-gateway",
          publicOrigin:
            typeof window === "undefined" ? null : window.location.origin,
        }),
        apiKey: secret,
        model: key.model || key.modelSlug || null,
        notes: "Imported from CodexManager",
        enabled: true,
      });
      await openCcSwitchImportUrl(importUrl);
      toast.success(t("已唤起 ccswitch，请在确认窗口完成导入"));
    } catch (error: unknown) {
      toast.error(
        `${t("唤起 ccswitch 失败")}: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    } finally {
      setCcSwitchImportingId(null);
    }
  };

  /**
   * 函数 `handleDelete`
   *
   * 作者: gaohongshun
   *
   * 时间: 2026-04-02
   *
   * # 参数
   * - id: 参数 id
   *
   * # 返回
   * 返回函数执行结果
   */
  const handleDelete = (id: string) => {
    setDeleteKeyId(id);
  };

  return (
    <div className="space-y-6 animate-in fade-in duration-500">
      {!isServiceReady ? (
        <Card className="glass-card shadow-sm">
          <CardContent className="pt-6 text-sm text-muted-foreground">
            {t("服务未连接")}
          </CardContent>
        </Card>
      ) : null}
      <div className="flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
        <div className="min-w-0">
          <h2 className="text-xl font-bold tracking-tight">{t("平台密钥")}</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("创建和管理网关调用所需的访问令牌")}
          </p>
        </div>
        <div className="flex w-full flex-wrap items-center gap-2 sm:w-auto sm:justify-end lg:shrink-0">
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  variant="outline"
                  className="glass-card h-10 gap-2 rounded-xl px-3 shadow-sm"
                  render={<span />}
                  nativeButton={false}
                />
              }
              nativeButton={false}
              disabled={!gatewayOrigin}
              aria-label={t("复制端点")}
            >
              <Link2 className="h-4 w-4" />
              <span>{t("网关端点")}</span>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="end"
              className="w-[min(92vw,390px)] rounded-xl border border-border/70 bg-popover/95 p-2 shadow-sm"
            >
                                  <DropdownMenuGroup>
                <DropdownMenuLabel className="px-2 py-1 text-[11px] uppercase text-muted-foreground/80">
                  {t("复制调用地址")}
                </DropdownMenuLabel>
                <DropdownMenuItem
                  className="h-auto cursor-pointer items-start gap-3 rounded-lg p-3"
                  onClick={() => void copyEndpoint(openAiEndpoint)}
                >
                  <Copy className="mt-0.5 h-4 w-4 text-primary" />
                  <span className="min-w-0 flex-1">
                    <span className="block text-xs font-semibold">
                      {t("复制 OpenAI / Codex 端点")}
                    </span>
                    <code
                      className="mt-1 block truncate font-mono text-[11px] text-muted-foreground"
                      title={openAiEndpoint}
                    >
                      {openAiEndpoint}
                    </code>
                  </span>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className="h-auto cursor-pointer items-start gap-3 rounded-lg p-3"
                  onClick={() => void copyEndpoint(nativeProtocolEndpoint)}
                >
                  <Copy className="mt-0.5 h-4 w-4 text-primary" />
                  <span className="min-w-0 flex-1">
                    <span className="block text-xs font-semibold">
                      {t("复制 Claude Code / Gemini CLI 端点")}
                    </span>
                    <code
                      className="mt-1 block truncate font-mono text-[11px] text-muted-foreground"
                      title={nativeProtocolEndpoint}
                    >
                      {nativeProtocolEndpoint}
                    </code>
                  </span>
                </DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button
            className="h-10 gap-2 shadow-sm shadow-primary/20"
            onClick={openCreateModal}
            disabled={!isServiceReady}
          >
            <Plus className="h-4 w-4" /> {t("创建密钥")}
          </Button>
        </div>
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        {isLoading || showOverviewLoading ? (
          <>
            <Skeleton className="h-32 w-full rounded-xl" />
            <Skeleton className="h-32 w-full rounded-xl" />
          </>
        ) : (
          <>
            <ApiKeyStatCard
              title={t("总使用 Token")}
              value={formatCompactTokenAmount(usageOverview?.totalTokens || 0)}
              icon={Zap}
              color="h-4 w-4 text-amber-500"
              sub={isAdminMode ? t("按全部平台密钥累计") : t("按我的平台密钥累计")}
            />
            <ApiKeyStatCard
              title={t("总费用")}
              value={formatUsd(usageOverview?.totalCostUsd || 0)}
              icon={DollarSign}
              color="h-4 w-4 text-emerald-500"
              sub={isAdminMode ? t("按全部平台密钥累计") : t("按我的平台密钥累计")}
            />
          </>
        )}
      </div>

      <Card className="glass-card overflow-hidden py-0 shadow-sm">
        <CardContent className="p-0">
          <Table className="min-w-[1160px]">
            <TableHeader>
              <TableRow>
                <TableHead>{t("密钥 / ID")}</TableHead>
                <TableHead>{t("名称")}</TableHead>
                {showMemberOwnership ? <TableHead>{t("归属成员")}</TableHead> : null}
                <TableHead>{t("协议")}</TableHead>
                <TableHead>{t("轮转策略")}</TableHead>
                <TableHead>{t("绑定模型")}</TableHead>
                <TableHead>{t("最近调用")}</TableHead>
                <TableHead>{t("Token / 金额")}</TableHead>
                <TableHead>{t("状态")}</TableHead>
                <TableHead className="table-sticky-action-head w-[144px] text-center">
                  {t("操作")}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                Array.from({ length: 3 }).map((_, index) => (
                    <TableRow key={index}>
                      <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                      {showMemberOwnership ? (
                        <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                      ) : null}
                      <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-28" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-28" /></TableCell>
                      <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                      <TableCell><Skeleton className="h-6 w-16 rounded-full" /></TableCell>
                      <TableCell className="table-sticky-action-cell text-center">
                        <Skeleton className="mx-auto h-8 w-8" />
                      </TableCell>
                    </TableRow>
                ))
              ) : apiKeys.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={showMemberOwnership ? 10 : 9} className="h-48 text-center">
                    <div className="flex flex-col items-center justify-center gap-2 text-muted-foreground">
                      <Plus className="h-8 w-8 opacity-20" />
                      <p>{t("创建密钥")}</p>
                    </div>
                  </TableCell>
                </TableRow>
              ) : (
                apiKeys.map((key) => {
                  const revealed = revealedSecrets[key.id];
                  const isEnabled = String(key.status).toLowerCase() !== "disabled";
                  const usedTokens = usageByKey[key.id] ?? 0;
                  const usedCostUsd = costByKey[key.id] ?? 0;
                  const quotaLimitTokens =
                    typeof key.quotaLimitTokens === "number" &&
                    Number.isFinite(key.quotaLimitTokens) &&
                    key.quotaLimitTokens > 0
                      ? key.quotaLimitTokens
                      : null;
                  const quotaRemaining =
                    quotaLimitTokens === null
                      ? null
                      : Math.max(0, quotaLimitTokens - usedTokens);
                  const isQuotaExhausted =
                    quotaLimitTokens !== null && usedTokens >= quotaLimitTokens;
                  const quotaLimitUsd =
                    quotaLimitTokens === null
                      ? null
                      : estimateQuotaLimitUsd(quotaLimitTokens);
                  const keyOwner = ownerByKeyId.get(key.id);

                  return (
                    <TableRow key={key.id} className="group">
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <code
                            className="inline-block max-w-[180px] truncate whitespace-nowrap rounded border border-primary/5 bg-muted/50 px-2 py-1 font-mono text-[10px] leading-4 text-primary"
                            title={revealed || key.id}
                          >
                            {revealed
                              ? revealed
                              : loadingSecretId === key.id
                                ? t("读取中...")
                                : key.id}
                          </code>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground hover:text-primary"
                            disabled={!isServiceReady}
                            onClick={() => void toggleSecret(key.id)}
                          >
                            {revealed ? (
                              <EyeOff className="h-3.5 w-3.5" />
                            ) : (
                              <Eye className="h-3.5 w-3.5" />
                            )}
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-7 w-7 text-muted-foreground hover:text-primary"
                            disabled={!isServiceReady}
                            onClick={() => void copyToClipboard(key.id)}
                          >
                            <Copy className="h-3.5 w-3.5" />
                          </Button>
                        </div>
                      </TableCell>
                      <TableCell className="text-sm font-semibold">{key.name || t("未命名")}</TableCell>
                      {showMemberOwnership ? (
                      <TableCell>
                        <Badge
                          variant={
                            keyOwner
                              ? "outline"
                              : distributionEnabled
                                ? "destructive"
                                : "secondary"
                          }
                          className="max-w-[180px] truncate text-[10px] font-normal"
                          title={resolveApiKeyOwnerLabel(
                            keyOwner,
                            appUsersById,
                            distributionEnabled,
                            t,
                          )}
                        >
                          {resolveApiKeyOwnerLabel(
                            keyOwner,
                            appUsersById,
                            distributionEnabled,
                            t,
                          )}
                        </Badge>
                      </TableCell>
                      ) : null}
                      <TableCell>
                        <Badge variant="outline" className="bg-accent/20 text-[10px] font-normal capitalize">
                          {key.protocol.replace(/_/g, " ")}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <Badge variant="secondary" className="text-[10px] font-normal">
                          {t(
                            ROTATION_STRATEGY_LABELS[key.rotationStrategy] ||
                              key.rotationStrategy,
                          )}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-xs font-medium text-muted-foreground">
                        {key.model ? (
                          key.model
                        ) : (
                          <span title={t("跟随请求表示使用请求体里的实际 model；请求日志展示的是最终生效模型。")}>
                            {t("跟随请求")}
                          </span>
                        )}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {formatLocalMinuteFromSeconds(key.lastUsedAt, t("从未调用"))}
                      </TableCell>
                      <TableCell className="font-mono text-xs">
                        <div className="space-y-1">
                          <div
                            className={
                              isQuotaExhausted
                                ? "font-semibold text-red-500"
                                : "text-foreground"
                            }
                          >
                            {formatCompactTokenAmount(usedTokens)}
                            {quotaLimitTokens !== null ? (
                              <span className="text-muted-foreground">
                                {" "}
                                / {formatCompactTokenAmount(quotaLimitTokens)}
                              </span>
                            ) : null}
                          </div>
                          <div className="text-[10px] font-normal text-muted-foreground">
                            {quotaLimitTokens === null
                              ? `${t("已花费")} ${formatQuotaLimitUsd(
                                  usedCostUsd,
                                )} · ${t("不限额")}`
                              : isQuotaExhausted
                                ? `${t("已达上限")} · ${formatQuotaLimitUsd(
                                    usedCostUsd,
                                  )} / ${formatQuotaLimitUsd(quotaLimitUsd)}`
                                : `${t("剩余")} ${formatCompactTokenAmount(
                                    quotaRemaining,
                                  )} · ${formatQuotaLimitUsd(
                                    usedCostUsd,
                                  )} / ${formatQuotaLimitUsd(quotaLimitUsd)}`}
                          </div>
                        </div>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <Switch
                            className="scale-75"
                            checked={isEnabled}
                            disabled={!isServiceReady || isToggling}
                            onCheckedChange={(enabled) =>
                              toggleApiKeyStatus({ id: key.id, enabled })
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
                            onClick={() => openEditModal(key.id)}
                            title={t("编辑配置")}
                          >
                            <Settings2 className="h-4 w-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                            disabled={
                              !isServiceReady ||
                              ccSwitchImportingId === key.id ||
                              loadingSecretId === key.id
                            }
                            onClick={() => void importToCcSwitch(key)}
                            title={t("导入 ccswitch")}
                            aria-label={t("导入 ccswitch")}
                          >
                            <ExternalLink className="h-4 w-4" />
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
                                onClick={() => openEditModal(key.id)}
                              >
                                {t("设置模型与推理")}
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                className="gap-2"
                                disabled={!isServiceReady || ccSwitchImportingId === key.id}
                                onClick={() => void importToCcSwitch(key)}
                              >
                                <ExternalLink className="h-4 w-4" /> {t("导入 ccswitch")}
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                className="gap-2 text-red-500"
                                disabled={!isServiceReady}
                                onClick={() => handleDelete(key.id)}
                              >
                                <Trash2 className="h-4 w-4" /> {t("删除密钥")}
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

      <ApiKeyModal
        open={apiKeyModalOpen}
        onOpenChange={setApiKeyModalOpen}
        apiKey={editingApiKey}
        appUsers={billableAppUsers}
        apiKeyOwner={
          showMemberOwnership && editingApiKey
            ? (ownerByKeyId.get(editingApiKey.id) ?? null)
            : null
        }
        distributionEnabled={distributionEnabled}
        isAdminMode={isAdminMode}
        showMemberOwnership={showMemberOwnership}
        onOwnerSaved={handleOwnerSaved}
      />
      <ConfirmDialog
        open={Boolean(deleteKeyId)}
        onOpenChange={(open) => {
          if (!open) {
            setDeleteKeyId(null);
          }
        }}
        title={t("删除密钥")}
        description={`${t("删除密钥")} ${apiKeys.find((item) => item.id === deleteKeyId)?.name || ""}`}
        confirmText={t("删除")}
        confirmVariant="destructive"
        onConfirm={() => {
          if (!deleteKeyId) return;
          deleteApiKey(deleteKeyId);
        }}
      />
    </div>
  );
}
