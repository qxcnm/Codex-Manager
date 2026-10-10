"use client";

import { useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useAccounts } from "@/hooks/useAccounts";
import { useAccountResetWarmup } from "@/hooks/useAccountResetWarmup";
import {
  isAdminRole,
  resolveSessionRole,
  useAppSession,
} from "@/hooks/useAppSession";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { proxyProfilesClient } from "@/lib/api/proxy-profiles";
import {
  type AccountProxySettings,
  type AccountProxySource,
} from "@/lib/api/account-client";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";
import {
  buildAccountsByMovedOrder,
  buildAccountsBySizeOrder,
  buildAccountOrderUpdates,
  type AccountEditorState,
  type AccountMoveDirection,
  type AccountMovePlacement,
  type DeleteDialogState,
  getAccountStatusActionType,
  normalizeAccountPlanKey,
  normalizeTagsDraft,
  type StatusFilter,
} from "@/app/accounts/accounts-page-helpers";
import { AccountsPageView } from "@/app/accounts/accounts-page-view";
import { AggregateApiModelAssociationModal } from "@/components/modals/aggregate-api-model-association-modal";
import { isBannedAccount, isLimitedAccount } from "@/lib/utils/usage";
import { accountClient } from "@/lib/api/account-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import type { Account, AccountFetchedModel, ProxyProfile } from "@/types";

type CleanupStatus =
  | "unavailable"
  | "banned"
  | "limited"
  | "disabled"
  | "inactive"
  | "unknown";

const CLEANUP_STATUSES: CleanupStatus[] = [
  "unavailable",
  "banned",
  "limited",
  "disabled",
  "inactive",
  "unknown",
];

function normalizeCleanupStatus(status: string): CleanupStatus | null {
  const normalized = String(status || "").trim().toLowerCase();
  return CLEANUP_STATUSES.includes(normalized as CleanupStatus)
    ? (normalized as CleanupStatus)
    : null;
}

function canBulkEnableAccount(account: Account): boolean {
  return getAccountStatusActionType(account) === "enable";
}

function canBulkDisableAccount(account: Account): boolean {
  return getAccountStatusActionType(account) === "disable";
}

interface AccountsPageContentProps {
  serviceAddr: string;
}

export default function AccountsPage() {
  const serviceAddr = useAppStore((state) => state.serviceStatus.addr);
  return <AccountsPageContent key={serviceAddr || "default"} serviceAddr={serviceAddr} />;
}

function AccountsPageContent({ serviceAddr }: AccountsPageContentProps) {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const { isDesktopRuntime, canUseBrowserDownloadExport } =
    useRuntimeCapabilities();
  const { data: session, isLoading: isSessionLoading } = useAppSession();
  const role = resolveSessionRole(session, isSessionLoading, isDesktopRuntime);
  const canTestAccounts =
    isDesktopRuntime || (!isSessionLoading && isAdminRole(role));
  const canManageAccountModels = canTestAccounts;
  const {
    accounts,
    planTypes,
    isLoading,
    isServiceReady,
    refreshAccount,
    refreshAccountRt,
    refreshAllAccountRt,
    refreshAllAccounts,
    refreshAccountList,
    refreshAccountsSilently,
    deleteAccount,
    deleteManyAccounts,
    cleanupAccountsByStatuses,
    importByFile,
    importByDirectory,
    exportAccounts,
    warmupAccounts,
    isRefreshingAccountId,
    isRefreshingAllAccounts,
    isExporting,
    isWarmingUpAccounts,
    isRefreshingRtAccountId,
    isRefreshingAllRtAccounts,
    isDeletingMany,
    isCleaningAccountsByStatus,
    setPreferredAccount,
    clearPreferredAccount,
    isUpdatingPreferred,
    getAccountProxySettings,
    setAccountProxySettings,
    clearAccountProxySettings,
    testAccountProxySettings,
    isSavingAccountProxy,
    isClearingAccountProxy,
    isTestingAccountProxy,
    reorderAccounts,
    isReorderingAccounts,
    updateAccountProfile,
    isUpdatingProfileAccountId,
    toggleAccountStatus,
    toggleManyAccountStatuses,
    isUpdatingStatusAccountId,
    isUpdatingManyStatuses,
  } = useAccounts();
  const { isUpdatingResetWarmup, setAccountResetWarmupEnabled } =
    useAccountResetWarmup(serviceAddr, isServiceReady);
  const isPageActive = useDesktopPageActive("/accounts/");
  usePageTransitionReady("/accounts/", !isServiceReady || !isLoading);

  const [search, setSearch] = useState("");
  const [planFilter, setPlanFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState("20");
  const [page, setPage] = useState(1);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [addAccountModalOpen, setAddAccountModalOpen] = useState(false);
  const [usageModalOpen, setUsageModalOpen] = useState(false);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [exportModeDraft, setExportModeDraft] = useState<"single" | "multiple">(
    "multiple",
  );
  const [selectedAccountId, setSelectedAccountId] = useState("");
  const [labelDraft, setLabelDraft] = useState("");
  const [groupNameDraft, setGroupNameDraft] = useState("");
  const [tagsDraft, setTagsDraft] = useState("");
  const [noteDraft, setNoteDraft] = useState("");
  const [sortDraft, setSortDraft] = useState("");
  const [forceEnabledDraft, setForceEnabledDraft] = useState(false);
  const [quotaPrimaryDraft, setQuotaPrimaryDraft] = useState("");
  const [quotaSecondaryDraft, setQuotaSecondaryDraft] = useState("");
  const [proxyDialogAccount, setProxyDialogAccount] = useState<Account | null>(null);
  const [proxySettings, setProxySettings] = useState<AccountProxySettings | null>(null);
  const [proxyProfiles, setProxyProfiles] = useState<ProxyProfile[]>([]);
  const [isProxySettingsLoading, setIsProxySettingsLoading] = useState(false);
  const [proxyEnabledDraft, setProxyEnabledDraft] = useState(false);
  const [proxySourceDraft, setProxySourceDraft] =
    useState<AccountProxySource>("custom");
  const [proxyProfileIdDraft, setProxyProfileIdDraft] = useState("");
  const [proxyUrlDraft, setProxyUrlDraft] = useState("");
  const [accountTestAccountId, setAccountTestAccountId] = useState<string | null>(
    null,
  );
  const [accountTestAccountSnapshot, setAccountTestAccountSnapshot] =
    useState<Account | null>(null);
  const [modelAssociationAccount, setModelAssociationAccount] =
    useState<Account | null>(null);
  const [modelAssociationItems, setModelAssociationItems] = useState<
    AccountFetchedModel[]
  >([]);
  const [fetchingModelsAccountId, setFetchingModelsAccountId] = useState<
    string | null
  >(null);
  const [isAssociatingModels, setIsAssociatingModels] = useState(false);
  // 从最新账号列表派生弹窗里的账号，测试结束后状态徽章可自动刷新；
  // 列表短暂重取时回退到快照，避免弹窗闪烁关闭。
  const accountTestAccount = useMemo(
    () =>
      accounts.find((account) => account.id === accountTestAccountId) ??
      accountTestAccountSnapshot,
    [accounts, accountTestAccountId, accountTestAccountSnapshot],
  );

  const [accountEditorState, setAccountEditorState] =
    useState<AccountEditorState | null>(null);
  const [deleteDialogState, setDeleteDialogState] =
    useState<DeleteDialogState>(null);
  const [cleanupDialogOpen, setCleanupDialogOpen] = useState(false);
  const [cleanupStatusDraft, setCleanupStatusDraft] = useState<CleanupStatus[]>([
    "unavailable",
    "banned",
  ]);

  const importFileActionLabel = isDesktopRuntime
    ? t("按文件导入")
    : t("选择文件导入");
  const importDirectoryActionLabel = isDesktopRuntime
    ? t("按文件夹导入")
    : t("选择目录导入");
  const exportActionLabel =
    !isDesktopRuntime && canUseBrowserDownloadExport
      ? t("导出到浏览器")
      : t("导出账号");
  const exportActionShortcut = isExporting
    ? "..."
    : !isDesktopRuntime && canUseBrowserDownloadExport
      ? "DL"
      : "ZIP";

  const filteredAccounts = useMemo(() => {
    return accounts.filter((account) => {
      const matchSearch =
        !search ||
        account.name.toLowerCase().includes(search.toLowerCase()) ||
        account.groupName.toLowerCase().includes(search.toLowerCase()) ||
        account.id.toLowerCase().includes(search.toLowerCase());
      const matchPlan =
        planFilter === "all" || normalizeAccountPlanKey(account) === planFilter;
      const matchStatus =
        statusFilter === "all" ||
        (statusFilter === "available" && account.isAvailable) ||
        (statusFilter === "low_quota" && account.isLowQuota) ||
        (statusFilter === "limited" && isLimitedAccount(account)) ||
        (statusFilter === "banned" && isBannedAccount(account));
      return matchSearch && matchPlan && matchStatus;
    });
  }, [accounts, planFilter, search, statusFilter]);

  const statusFilterOptions = useMemo(
    () => [
      { id: "all" as const, label: `${t("全部")} (${accounts.length})` },
      {
        id: "available" as const,
        label: `${t("可用")} (${accounts.filter((account) => account.isAvailable).length})`,
      },
      {
        id: "low_quota" as const,
        label: `${t("低配额")} (${accounts.filter((account) => account.isLowQuota).length})`,
      },
      {
        id: "limited" as const,
        label: `${t("限流")} (${accounts.filter((account) => isLimitedAccount(account)).length})`,
      },
      {
        id: "banned" as const,
        label: `${t("封禁")} (${accounts.filter((account) => isBannedAccount(account)).length})`,
      },
    ],
    [accounts, t],
  );

  const cleanupStatusCounts = useMemo(() => {
    const counts = new Map<CleanupStatus, number>(
      CLEANUP_STATUSES.map((status) => [status, 0] as const),
    );
    for (const account of accounts) {
      const status = normalizeCleanupStatus(account.status);
      if (status) {
        counts.set(status, (counts.get(status) || 0) + 1);
      }
    }
    return counts;
  }, [accounts]);

  const cleanupStatusOptions = useMemo(
    () =>
      [
        {
          id: "unavailable" as const,
          label: t("不可用"),
          description: t("AT/RT 过期、用量接口 401/403 等不可用账号"),
        },
        {
          id: "banned" as const,
          label: t("封禁"),
          description: t("账号或工作区被停用的账号"),
        },
        {
          id: "limited" as const,
          label: t("用量限制"),
          description: t("明确触发 usage_limit_reached 的账号，不包含低额度账号"),
        },
        {
          id: "disabled" as const,
          label: t("禁用"),
          description: t("手动禁用的账号"),
        },
        {
          id: "inactive" as const,
          label: t("停用"),
          description: t("手动停用或旧版本标记的账号"),
        },
        {
          id: "unknown" as const,
          label: t("未知"),
          description: t("状态字段为 unknown 的账号"),
        },
      ].map((option) => ({
        ...option,
        count: cleanupStatusCounts.get(option.id as CleanupStatus) || 0,
      })),
    [cleanupStatusCounts, t],
  );

  const pageSizeNumber = Number(pageSize) || 20;
  const totalPages = Math.max(
    1,
    Math.ceil(filteredAccounts.length / pageSizeNumber),
  );
  const safePage = Math.min(page, totalPages);
  const accountIdSet = useMemo(
    () => new Set(accounts.map((account) => account.id)),
    [accounts],
  );
  const effectiveSelectedIds = useMemo(
    () => selectedIds.filter((id) => accountIdSet.has(id)),
    [accountIdSet, selectedIds],
  );
  const selectedAccounts = useMemo(
    () =>
      effectiveSelectedIds
        .map((id) => accounts.find((account) => account.id === id))
        .filter((account): account is Account => Boolean(account)),
    [accounts, effectiveSelectedIds],
  );
  const selectedEnableTargetIds = useMemo(
    () =>
      selectedAccounts
        .filter((account) => canBulkEnableAccount(account))
        .map((account) => account.id),
    [selectedAccounts],
  );
  const selectedDisableTargetIds = useMemo(
    () =>
      selectedAccounts
        .filter((account) => canBulkDisableAccount(account))
        .map((account) => account.id),
    [selectedAccounts],
  );
  const exportSelectionCount = effectiveSelectedIds.length;
  const exportTargetCount =
    exportSelectionCount > 0 ? exportSelectionCount : accounts.length;
  const exportScopeText =
    exportSelectionCount > 0
      ? `${t("当前已选择")} ${exportSelectionCount} ${t("个账号，本次将只导出选中的账号。")}`
      : `${t("当前未选择账号，本次将导出全部")} ${accounts.length} ${t("个账号。")}`;

  const visibleAccounts = useMemo(() => {
    const offset = (safePage - 1) * pageSizeNumber;
    return filteredAccounts.slice(offset, offset + pageSizeNumber);
  }, [filteredAccounts, pageSizeNumber, safePage]);

  const filteredAccountIndexMap = useMemo(
    () =>
      new Map(filteredAccounts.map((account, index) => [account.id, index])),
    [filteredAccounts],
  );

  const selectedAccount = useMemo(
    () => accounts.find((account) => account.id === selectedAccountId) ?? null,
    [accounts, selectedAccountId],
  );
  const currentEditingAccount = useMemo(
    () =>
      accountEditorState
        ? (accounts.find(
            (account) => account.id === accountEditorState.accountId,
          ) ?? null)
        : null,
    [accountEditorState, accounts],
  );

  const handleSearchChange = (value: string) => {
    setSearch(value);
    setPage(1);
  };

  const handlePlanFilterChange = (value: string | null) => {
    setPlanFilter(value || "all");
    setPage(1);
  };

  const handleStatusFilterChange = (value: StatusFilter) => {
    setStatusFilter(value);
    setPage(1);
  };

  const handlePageSizeChange = (value: string | null) => {
    setPageSize(value || "20");
    setPage(1);
  };

  const toggleSelect = (id: string) => {
    setSelectedIds((current) =>
      current.includes(id)
        ? current.filter((item) => item !== id)
        : [...current, id],
    );
  };

  const toggleSelectAllVisible = () => {
    const visibleIds = visibleAccounts.map((account) => account.id);
    const allSelected = visibleIds.every((id) =>
      effectiveSelectedIds.includes(id),
    );
    setSelectedIds((current) => {
      if (allSelected) {
        return current.filter((id) => !visibleIds.includes(id));
      }
      return Array.from(new Set([...current, ...visibleIds]));
    });
  };

  const openUsage = (account: Account) => {
    setSelectedAccountId(account.id);
    setUsageModalOpen(true);
  };

  const openModelAssociation = async (account: Account) => {
    setFetchingModelsAccountId(account.id);
    try {
      const result = await accountClient.fetchAccountModels(account.id, serviceAddr);
      setModelAssociationAccount(account);
      setModelAssociationItems(result.items);
    } catch (error) {
      toast.error(`${t("拉取模型失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setFetchingModelsAccountId(null);
    }
  };

  const handleModelAssociationOpenChange = (open: boolean) => {
    if (!open && !isAssociatingModels) {
      setModelAssociationAccount(null);
      setModelAssociationItems([]);
    }
  };

  const associateAccountModels = async (upstreamModels: string[]) => {
    if (!modelAssociationAccount) return;
    setIsAssociatingModels(true);
    try {
      const selected = new Set(upstreamModels);
      const displayNames = Object.fromEntries(
        modelAssociationItems
          .filter((item) => selected.has(item.upstreamModel) && item.displayName)
          .map((item) => [item.upstreamModel, item.displayName as string]),
      );
      const result = await accountClient.associateAccountModels(
        modelAssociationAccount.id,
        upstreamModels,
        displayNames,
        serviceAddr,
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["managed-models-v2"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
        queryClient.invalidateQueries({ queryKey: ["apikeys"] }),
      ]);
      toast.success(
        t(
          "关联完成：新增模型 {created}，追加 route {added}，未变更 {unchanged}",
          {
            created: result.createdModels.length,
            added: result.addedRoutes.length,
            unchanged: result.unchangedRoutes.length,
          },
        ),
      );
      setModelAssociationAccount(null);
      setModelAssociationItems([]);
    } catch (error) {
      toast.error(`${t("关联模型失败")}: ${getAppErrorMessage(error)}`);
    } finally {
      setIsAssociatingModels(false);
    }
  };

  const handleUsageModalOpenChange = (open: boolean) => {
    setUsageModalOpen(open);
    if (!open) {
      setSelectedAccountId("");
    }
  };

  const handleDeleteSelected = () => {
    if (!effectiveSelectedIds.length) {
      toast.error(t("请先选择要删除的账号"));
      return;
    }
    setDeleteDialogState({
      kind: "selected",
      ids: [...effectiveSelectedIds],
      count: effectiveSelectedIds.length,
    });
  };

  const handleToggleSelectedStatus = async (enabled: boolean) => {
    if (!effectiveSelectedIds.length) {
      toast.error(t("请先选择账号"));
      return;
    }
    const targetIds = enabled ? selectedEnableTargetIds : selectedDisableTargetIds;
    if (targetIds.length === 0) {
      toast.info(
        enabled
          ? t("当前选中账号没有可开启项")
          : t("当前选中账号没有可关闭项"),
      );
      return;
    }

    try {
      await toggleManyAccountStatuses(
        targetIds,
        enabled,
        effectiveSelectedIds.length,
      );
    } catch {
      // hook 内统一处理 toast，这里保留当前选择
    }
  };

  const openCleanupDialog = () => {
    if (!accounts.length) {
      toast.info(t("当前没有可清理的账号"));
      return;
    }
    setCleanupDialogOpen(true);
  };

const toggleCleanupStatus = (rawStatus: string) => {
  const status = normalizeCleanupStatus(rawStatus);
  if (!status) {
    return;
  }
  setCleanupStatusDraft((current) =>
    current.includes(status)
      ? current.filter((item) => item !== status)
        : [...current, status],
    );
  };

  const handleConfirmCleanupStatuses = async () => {
    if (!cleanupStatusDraft.length) {
      toast.error(t("请至少选择一种账号状态"));
      return;
    }
    const targetCount = cleanupStatusDraft.reduce(
      (total, status) => total + (cleanupStatusCounts.get(status) || 0),
      0,
    );
    if (targetCount <= 0) {
      toast.info(t("当前没有匹配所选状态的账号"));
      return;
    }
    try {
      await cleanupAccountsByStatuses(cleanupStatusDraft);
      setCleanupDialogOpen(false);
    } catch {
      // hook 内统一处理 toast，这里保持弹窗不关闭
    }
  };

  const handleWarmupAccounts = async () => {
    const targetIds = effectiveSelectedIds.length > 0 ? effectiveSelectedIds : [];
    const targetCount = targetIds.length > 0 ? targetIds.length : accounts.length;
    if (targetCount <= 0) {
      toast.info(t("当前没有可预热的账号"));
      return;
    }
    try {
      await warmupAccounts({
        accountIds: targetIds,
        message: "hi",
      });
    } catch {
      // 中文注释：错误提示已在 hook 内统一处理，这里不重复提示。
    }
  };

  const openExportDialog = () => {
    if (!isServiceReady) {
      toast.info(t("服务未连接，暂时无法导出账号"));
      return;
    }
    if (!accounts.length) {
      toast.info(t("当前没有可导出的账号"));
      return;
    }
    setExportModeDraft("multiple");
    setExportDialogOpen(true);
  };

  const handleConfirmExport = async () => {
    if (exportTargetCount <= 0) {
      toast.info(t("当前没有可导出的账号"));
      return;
    }
    try {
      await exportAccounts({
        selectedAccountIds:
          exportSelectionCount > 0 ? effectiveSelectedIds : [],
        exportMode: exportModeDraft,
      });
      setExportDialogOpen(false);
    } catch {
      // 中文注释：错误提示已在 hook 内统一处理，这里只阻止弹窗误关闭。
    }
  };

  const handleDeleteSingle = (account: Account) => {
    setDeleteDialogState({ kind: "single", account });
  };

  const openProxyDialog = async (account: Account) => {
    setProxyDialogAccount(account);
    setProxySettings(null);
    setProxyProfiles([]);
    setProxyEnabledDraft(false);
    setProxySourceDraft("profile");
    setProxyProfileIdDraft("");
    setProxyUrlDraft("");
    setIsProxySettingsLoading(true);
    try {
      const [settings, profiles] = await Promise.all([
        getAccountProxySettings(account.id),
        proxyProfilesClient.listProxyProfiles(),
      ]);
      setProxySettings(settings);
      setProxyProfiles(profiles.items);
      setProxyEnabledDraft(settings.enabled);
      setProxySourceDraft(settings.source);
      setProxyProfileIdDraft(settings.proxyProfileId || "");
      setProxyUrlDraft(settings.proxyUrl || "");
    } catch (error) {
      toast.error(`${t("读取账号代理失败")}: ${error instanceof Error ? error.message : String(error)}`);
      setProxyDialogAccount(null);
    } finally {
      setIsProxySettingsLoading(false);
    }
  };

  const handleProxyDialogOpenChange = (open: boolean) => {
    if (open) return;
    if (isSavingAccountProxy || isClearingAccountProxy || isTestingAccountProxy) {
      return;
    }
    setProxyDialogAccount(null);
    setProxySettings(null);
    setProxyProfiles([]);
    setProxyEnabledDraft(false);
    setProxySourceDraft("custom");
    setProxyProfileIdDraft("");
    setProxyUrlDraft("");
  };

  const openAccountTest = (account: Account) => {
    if (!canTestAccounts) return;
    setAccountTestAccountId(account.id);
    setAccountTestAccountSnapshot(account);
  };

  const handleAccountTestOpenChange = (open: boolean) => {
    if (open) return;
    setAccountTestAccountId(null);
    setAccountTestAccountSnapshot(null);
  };

  // 测试结束后静默刷新账号状态（不弹「账号用量已刷新」），让弹窗徽章与列表同步。
  const handleAccountTestFinished = () => {
    void refreshAccountsSilently();
  };

  const handleTestProxySettings = async () => {
    if (!proxyDialogAccount) return;
    try {
      const settings = await testAccountProxySettings({
        accountId: proxyDialogAccount.id,
        enabled: proxyEnabledDraft,
        source: "profile",
        proxyProfileId: proxyProfileIdDraft || null,
        proxyUrl: "",
      });
      if (settings) {
        setProxySettings(settings);
        setProxyEnabledDraft(settings.enabled);
        setProxySourceDraft(settings.source);
        setProxyProfileIdDraft(settings.proxyProfileId || "");
        setProxyUrlDraft(settings.proxyUrl || "");
      }
    } catch {
      // hook handles toast
    }
  };

  const handleSaveProxySettings = async () => {
    if (!proxyDialogAccount) return;
    try {
      const isTested =
        proxySettings &&
        proxySettings.source === "profile" &&
        proxySettings.proxyProfileId === (proxyProfileIdDraft || null);
      const settings = await setAccountProxySettings({
        accountId: proxyDialogAccount.id,
        enabled: proxyEnabledDraft,
        source: "profile",
        proxyProfileId: proxyProfileIdDraft || null,
        proxyUrl: "",
        ...(isTested
          ? {
              status: proxySettings.status,
              latencyMs: proxySettings.latencyMs,
              lastError: proxySettings.lastError,
              ip: proxySettings.ip,
              countryCode: proxySettings.countryCode,
              countryName: proxySettings.countryName,
              regionName: proxySettings.regionName,
              cityName: proxySettings.cityName,
              geoCheckedAt: proxySettings.geoCheckedAt,
              geoError: proxySettings.geoError,
            }
          : {}),
      });
      if (settings) {
        setProxySettings(settings);
        setProxyEnabledDraft(settings.enabled);
        setProxySourceDraft(settings.source);
        setProxyProfileIdDraft(settings.proxyProfileId || "");
        setProxyUrlDraft(settings.proxyUrl || "");
      }
    } catch {
      // hook handles toast
    }
  };

  const handleClearProxySettings = async () => {
    if (!proxyDialogAccount) return;
    try {
      const settings = await clearAccountProxySettings(proxyDialogAccount.id);
      if (settings) {
        setProxySettings(settings);
        setProxyEnabledDraft(settings.enabled);
        setProxySourceDraft(settings.source);
        setProxyProfileIdDraft(settings.proxyProfileId || "");
        setProxyUrlDraft(settings.proxyUrl || "");
      }
    } catch {
      // hook handles toast
    }
  };


  const openAccountEditor = (account: Account) => {
    setAccountEditorState({
      accountId: account.id,
      accountName: account.name,
      currentLabel: account.label,
      currentGroupName: account.groupName,
      currentTags: account.tags.join(", "),
      currentNote: account.note || "",
      currentSort: account.priority,
      currentForceEnabled: account.status.trim().toLowerCase() === "force_enabled",
      currentQuotaPrimaryWindowTokens: account.quotaCapacityPrimaryWindowTokens,
      currentQuotaSecondaryWindowTokens: account.quotaCapacitySecondaryWindowTokens,
    });
    setLabelDraft(account.label);
    setGroupNameDraft(account.groupName);
    setTagsDraft(account.tags.join(", "));
    setNoteDraft(account.note || "");
    setSortDraft(String(account.priority));
    setForceEnabledDraft(account.status.trim().toLowerCase() === "force_enabled");
    setQuotaPrimaryDraft(
      account.quotaCapacityPrimaryWindowTokens == null
        ? ""
        : String(account.quotaCapacityPrimaryWindowTokens),
    );
    setQuotaSecondaryDraft(
      account.quotaCapacitySecondaryWindowTokens == null
        ? ""
        : String(account.quotaCapacitySecondaryWindowTokens),
    );
  };

  const handleToggleForceEnabled = async (account: Account) => {
    const normalizedStatus = account.status.trim().toLowerCase();
    if (["disabled", "inactive", "unavailable", "banned"].includes(normalizedStatus)) {
      return;
    }
    try {
      await updateAccountProfile(account.id, {
        status: normalizedStatus === "force_enabled" ? "active" : "force_enabled",
      });
    } catch {
      // mutation 已统一处理 toast，这里保持菜单状态不变
    }
  };

  // 顶部/底部按全量列表定位，上移/下移仍按当前筛选结果取相邻账号。
  const resolveAccountMovePlacement = (
    account: Account,
    direction: AccountMoveDirection,
  ): AccountMovePlacement | null => {
    if (direction === "top" || direction === "bottom") {
      const boundaryAccount =
        direction === "top" ? accounts[0] : accounts[accounts.length - 1];
      if (boundaryAccount?.id === account.id) {
        toast.info(
          direction === "top"
            ? t("当前账号已经在最前面")
            : t("当前账号已经在最后面"),
        );
        return null;
      }
      return { type: direction };
    }

    const filteredIndex = filteredAccountIndexMap.get(account.id);
    if (filteredIndex == null) {
      toast.error(t("未找到当前账号，请刷新后重试"));
      return null;
    }

    const targetFilteredIndex =
      direction === "up" ? filteredIndex - 1 : filteredIndex + 1;
    if (targetFilteredIndex < 0) {
      toast.info(t("当前账号已经在最前面"));
      return null;
    }
    if (targetFilteredIndex >= filteredAccounts.length) {
      toast.info(t("当前账号已经在最后面"));
      return null;
    }

    return {
      type: direction === "up" ? "before" : "after",
      anchorAccountId: filteredAccounts[targetFilteredIndex].id,
    };
  };

  const handleMoveAccount = async (
    account: Account,
    direction: AccountMoveDirection,
  ) => {
    const placement = resolveAccountMovePlacement(account, direction);
    if (!placement) {
      return;
    }

    const reorderedAccounts = buildAccountsByMovedOrder(
      accounts,
      account,
      placement,
    );
    if (!reorderedAccounts) {
      toast.error(t("未找到目标账号，请刷新后重试"));
      return;
    }

    const updates = buildAccountOrderUpdates(reorderedAccounts);
    if (!updates.length) {
      toast.info(t("账号顺序未变化"));
      return;
    }

    try {
      await reorderAccounts(updates);
    } catch {
      // hook 内统一处理 toast，这里保持静默即可
    }
  };

  const handleApplyAccountSizeSort = async (
    mode: "large-first" | "small-first",
  ) => {
    if (accounts.length < 2) {
      toast.info(t("账号数量不足，无需重新排序"));
      return;
    }
    const reorderedAccounts = buildAccountsBySizeOrder(accounts, mode);
    const updates = buildAccountOrderUpdates(reorderedAccounts);
    if (!updates.length) {
      toast.info(
        mode === "large-first"
          ? t("当前已经是大号优先顺序")
          : t("当前已经是小号优先顺序"),
      );
      return;
    }
    try {
      await reorderAccounts(updates);
    } catch {
      // hook 已统一处理 toast，这里保持静默即可
    }
  };

  const handleConfirmAccountEditor = async () => {
    if (!accountEditorState) return;

    const nextLabel = labelDraft.trim();
    const nextGroupName = groupNameDraft.trim();
    const nextTags = normalizeTagsDraft(tagsDraft);
    const nextTagsText = nextTags.join(", ");
    const nextNote = noteDraft.trim();
    const parseOptionalTokenCapacity = (value: string) => {
      const trimmed = value.trim();
      if (!trimmed) return null;
      const parsed = Number(trimmed);
      if (!Number.isFinite(parsed) || parsed <= 0) {
        return Number.NaN;
      }
      return Math.trunc(parsed);
    };
    const nextPrimaryCapacity = parseOptionalTokenCapacity(quotaPrimaryDraft);
    const nextSecondaryCapacity = parseOptionalTokenCapacity(quotaSecondaryDraft);

    if (!nextLabel) {
      toast.error(t("请输入账号名称"));
      return;
    }
    const rawSort = sortDraft.trim();
    if (!rawSort) {
      toast.error(t("请输入顺序值"));
      return;
    }
    const parsed = Number(rawSort);
    if (!Number.isFinite(parsed)) {
      toast.error(t("顺序必须是数字"));
      return;
    }
    if (Number.isNaN(nextPrimaryCapacity) || Number.isNaN(nextSecondaryCapacity)) {
      toast.error(t("额度容量必须是大于 0 的数字，留空表示未覆盖"));
      return;
    }

    const nextSort = Math.max(0, Math.trunc(parsed));
    if (
      nextLabel === accountEditorState.currentLabel &&
      nextGroupName === accountEditorState.currentGroupName &&
      nextTagsText === accountEditorState.currentTags &&
      nextNote === accountEditorState.currentNote &&
      nextSort === accountEditorState.currentSort &&
      forceEnabledDraft === accountEditorState.currentForceEnabled &&
      nextPrimaryCapacity === accountEditorState.currentQuotaPrimaryWindowTokens &&
      nextSecondaryCapacity === accountEditorState.currentQuotaSecondaryWindowTokens
    ) {
      setAccountEditorState(null);
      return;
    }

    try {
      await updateAccountProfile(accountEditorState.accountId, {
        label: nextLabel,
        groupName: nextGroupName,
        note: nextNote || null,
        tags: nextTags,
        sort: nextSort,
        status:
          forceEnabledDraft === accountEditorState.currentForceEnabled
            ? undefined
            : forceEnabledDraft
              ? "force_enabled"
              : "active",
        quotaCapacityPrimaryWindowTokens: nextPrimaryCapacity ?? 0,
        quotaCapacitySecondaryWindowTokens: nextSecondaryCapacity ?? 0,
      });
      setAccountEditorState(null);
    } catch {
      // mutation 已统一处理 toast，这里保持弹窗不关闭
    }
  };

  const handleConfirmDelete = () => {
    if (!deleteDialogState) return;
    if (deleteDialogState.kind === "single") {
      deleteAccount(deleteDialogState.account.id);
      return;
    }
    deleteManyAccounts(deleteDialogState.ids);
    setSelectedIds((current) =>
      current.filter((id) => !deleteDialogState.ids.includes(id)),
    );
  };

  return (
    <>
    <AccountsPageView
      accounts={accounts}
      planTypes={planTypes}
      isLoading={isLoading}
      isServiceReady={isServiceReady}
      isPageActive={isPageActive}
      search={search}
      planFilter={planFilter}
      statusFilter={statusFilter}
      pageSize={pageSize}
      safePage={safePage}
      totalPages={totalPages}
      filteredAccounts={filteredAccounts}
      visibleAccounts={visibleAccounts}
      filteredAccountIndexMap={filteredAccountIndexMap}
      effectiveSelectedIds={effectiveSelectedIds}
      selectedEnableTargetCount={selectedEnableTargetIds.length}
      selectedDisableTargetCount={selectedDisableTargetIds.length}
      addAccountModalOpen={addAccountModalOpen}
      usageModalOpen={usageModalOpen}
      exportDialogOpen={exportDialogOpen}
      exportModeDraft={exportModeDraft}
      exportTargetCount={exportTargetCount}
      exportScopeText={exportScopeText}
      selectedAccount={selectedAccount}
      accountEditorState={accountEditorState}
      deleteDialogState={deleteDialogState}
      cleanupDialogOpen={cleanupDialogOpen}
      cleanupStatusDraft={cleanupStatusDraft}
      cleanupStatusOptions={cleanupStatusOptions}
      proxyDialogAccount={proxyDialogAccount}
      proxySettings={proxySettings}
      proxyProfiles={proxyProfiles}
      canTestAccounts={canTestAccounts}
      canManageAccountModels={canManageAccountModels}
      fetchingModelsAccountId={fetchingModelsAccountId}
      openModelAssociation={openModelAssociation}
      accountTestAccount={accountTestAccount}
      isProxySettingsLoading={isProxySettingsLoading}
      proxyEnabledDraft={proxyEnabledDraft}
      proxySourceDraft={proxySourceDraft}
      proxyProfileIdDraft={proxyProfileIdDraft}
      proxyUrlDraft={proxyUrlDraft}
      currentEditingAccount={currentEditingAccount}
      labelDraft={labelDraft}
      groupNameDraft={groupNameDraft}
      tagsDraft={tagsDraft}
      noteDraft={noteDraft}
      sortDraft={sortDraft}
      forceEnabledDraft={forceEnabledDraft}
      quotaPrimaryDraft={quotaPrimaryDraft}
      quotaSecondaryDraft={quotaSecondaryDraft}
      isRefreshingAllAccounts={isRefreshingAllAccounts}
      isRefreshingAccountId={isRefreshingAccountId}
      isRefreshingRtAccountId={isRefreshingRtAccountId}
      isRefreshingAllRtAccounts={isRefreshingAllRtAccounts}
      isExporting={isExporting}
      isWarmingUpAccounts={isWarmingUpAccounts}
      isDeletingMany={isDeletingMany}
      isCleaningAccountsByStatus={isCleaningAccountsByStatus}
      isUpdatingPreferred={isUpdatingPreferred}
      isSavingAccountProxy={isSavingAccountProxy}
      isClearingAccountProxy={isClearingAccountProxy}
      isTestingAccountProxy={isTestingAccountProxy}
      isReorderingAccounts={isReorderingAccounts}
      isUpdatingProfileAccountId={isUpdatingProfileAccountId}
      isUpdatingStatusAccountId={isUpdatingStatusAccountId}
      isUpdatingManyStatuses={isUpdatingManyStatuses}
      isUpdatingResetWarmup={isUpdatingResetWarmup}
      setAccountResetWarmupEnabled={setAccountResetWarmupEnabled}
      statusFilterOptions={statusFilterOptions}
      importFileActionLabel={importFileActionLabel}
      importDirectoryActionLabel={importDirectoryActionLabel}
      exportActionLabel={exportActionLabel}
      exportActionShortcut={exportActionShortcut}
      setAddAccountModalOpen={setAddAccountModalOpen}
      setExportDialogOpen={setExportDialogOpen}
      setExportModeDraft={setExportModeDraft}
      setDeleteDialogState={setDeleteDialogState}
      setCleanupDialogOpen={setCleanupDialogOpen}
      setProxyEnabledDraft={setProxyEnabledDraft}
      setProxySourceDraft={setProxySourceDraft}
      setProxyProfileIdDraft={setProxyProfileIdDraft}
      setProxyUrlDraft={setProxyUrlDraft}
      setAccountEditorState={setAccountEditorState}
      setLabelDraft={setLabelDraft}
      setGroupNameDraft={setGroupNameDraft}
      setTagsDraft={setTagsDraft}
      setNoteDraft={setNoteDraft}
      setSortDraft={setSortDraft}
      setForceEnabledDraft={setForceEnabledDraft}
      setQuotaPrimaryDraft={setQuotaPrimaryDraft}
      setQuotaSecondaryDraft={setQuotaSecondaryDraft}
      setPage={setPage}
      handleSearchChange={handleSearchChange}
      handlePlanFilterChange={handlePlanFilterChange}
      handleStatusFilterChange={handleStatusFilterChange}
      handlePageSizeChange={handlePageSizeChange}
      toggleSelect={toggleSelect}
      toggleSelectAllVisible={toggleSelectAllVisible}
      openUsage={openUsage}
      handleUsageModalOpenChange={handleUsageModalOpenChange}
      handleDeleteSelected={handleDeleteSelected}
      handleEnableSelected={() => void handleToggleSelectedStatus(true)}
      handleDisableSelected={() => void handleToggleSelectedStatus(false)}
      openCleanupDialog={openCleanupDialog}
      toggleCleanupStatus={toggleCleanupStatus}
      handleConfirmCleanupStatuses={handleConfirmCleanupStatuses}
      handleWarmupAccounts={handleWarmupAccounts}
      openExportDialog={openExportDialog}
      handleConfirmExport={handleConfirmExport}
      handleDeleteSingle={handleDeleteSingle}
      openProxyDialog={openProxyDialog}
      handleProxyDialogOpenChange={handleProxyDialogOpenChange}
      openAccountTest={openAccountTest}
      handleAccountTestOpenChange={handleAccountTestOpenChange}
      onAccountTestFinished={handleAccountTestFinished}
      handleSaveProxySettings={handleSaveProxySettings}
      handleClearProxySettings={handleClearProxySettings}
      handleTestProxySettings={handleTestProxySettings}
      openAccountEditor={openAccountEditor}
      handleMoveAccount={handleMoveAccount}
      handleApplyAccountSizeSort={handleApplyAccountSizeSort}
      handleConfirmAccountEditor={handleConfirmAccountEditor}
      handleConfirmDelete={handleConfirmDelete}
      refreshAllAccounts={refreshAllAccounts}
      refreshAllAccountRt={refreshAllAccountRt}
      refreshAccountList={refreshAccountList}
      refreshAccountRt={refreshAccountRt}
      importByFile={importByFile}
      importByDirectory={importByDirectory}
      refreshAccount={refreshAccount}
      clearPreferredAccount={clearPreferredAccount}
      setPreferredAccount={setPreferredAccount}
      toggleForceEnabled={handleToggleForceEnabled}
      toggleAccountStatus={toggleAccountStatus}
    />
      <AggregateApiModelAssociationModal
        open={isPageActive && Boolean(modelAssociationAccount)}
        onOpenChange={handleModelAssociationOpenChange}
        sourceName={modelAssociationAccount?.name || t("账号模型")}
        items={modelAssociationItems}
        isSaving={isAssociatingModels}
        onAssociate={associateAccountModels}
      />
    </>
  );
}
