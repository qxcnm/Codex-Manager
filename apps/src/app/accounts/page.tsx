"use client";

import { useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { useAccounts } from "@/hooks/useAccounts";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { proxyProfilesClient } from "@/lib/api/proxy-profiles";
import {
  accountClient,
  type AccountProxySettings,
  type AccountProxySource,
} from "@/lib/api/account-client";
import { useI18n } from "@/lib/i18n/provider";
import {
  buildAccountsBySizeOrder,
  buildAccountOrderUpdates,
  type AccountEditorState,
  type DeleteDialogState,
  normalizeAccountPlanKey,
  normalizeTagsDraft,
  type StatusFilter,
} from "@/app/accounts/accounts-page-helpers";
import { AccountsPageView } from "@/app/accounts/accounts-page-view";
import { isBannedAccount, isLimitedAccount } from "@/lib/utils/usage";
import type { Account, ProxyProfile, ProxyTestJobState } from "@/types";

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

export default function AccountsPage() {
  const { t } = useI18n();
  const { isDesktopRuntime, canUseBrowserDownloadExport } =
    useRuntimeCapabilities();
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
    isUpdatingStatusAccountId,
  } = useAccounts();
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
  const [tagsDraft, setTagsDraft] = useState("");
  const [noteDraft, setNoteDraft] = useState("");
  const [sortDraft, setSortDraft] = useState("");
  const [modelWhitelistDraft, setModelWhitelistDraft] = useState("");
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
  const [activeJobs, setActiveJobs] = useState<Record<string, ProxyTestJobState>>({});


  const [presetsData, setPresetsData] = useState<any>(null);
  const [isLoadingPresets, setIsLoadingPresets] = useState(false);
  const [isPresetsError, setIsPresetsError] = useState(false);
  const [presetsError, setPresetsError] = useState<any>(null);
  const [isCancellingJobId, setIsCancellingJobId] = useState<string | null>(null);
  const trackedJobIdsRef = useRef<Record<string, string>>({});
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
    setIsLoadingPresets(true);
    setIsPresetsError(false);
    setPresetsError(null);
    try {
      const [settings, profiles, presets] = await Promise.all([
        getAccountProxySettings(account.id),
        proxyProfilesClient.listProxyProfiles(),
        proxyProfilesClient.listProxyTestPresets(),
      ]);
      setProxySettings(settings);
      setProxyProfiles(profiles.items);
      setPresetsData(presets);
      setProxyEnabledDraft(settings.enabled);
      setProxySourceDraft(settings.source);
      setProxyProfileIdDraft(settings.proxyProfileId || "");
      setProxyUrlDraft(settings.proxyUrl || "");




      setIsLoadingPresets(false);
    } catch (error) {
      toast.error(`${t("读取账号代理失败")}: ${error instanceof Error ? error.message : String(error)}`);
      setIsPresetsError(true);
      setPresetsError(error);
      setIsLoadingPresets(false);
      setProxyDialogAccount(null);
    } finally {
      setIsProxySettingsLoading(false);
    }
  };

  const handleProxyDialogOpenChange = (open: boolean) => {
    if (open) return;
    const currentActiveJob = proxyDialogAccount ? activeJobs[proxyDialogAccount.id] : undefined;
    const isJobRunning = currentActiveJob && !(currentActiveJob.status === "completed" || currentActiveJob.status === "failed" || currentActiveJob.status === "cancelled");
    if (isSavingAccountProxy || isClearingAccountProxy || isTestingAccountProxy || isJobRunning) {
      return;
    }
    setProxyDialogAccount(null);
    setProxySettings(null);
    setProxyProfiles([]);
    setProxyEnabledDraft(false);
    setProxySourceDraft("custom");
    setProxyProfileIdDraft("");
    setProxyUrlDraft("");

    setPresetsData(null);
  };

  const setActiveJob = (accountId: string, job: ProxyTestJobState) => {
    setActiveJobs((current) => ({
      ...current,
      [accountId]: job,
    }));
  };

  const clearActiveJob = (accountId: string, jobId?: string) => {
    setActiveJobs((current) => {
      if (!(accountId in current)) return current;
      const next = { ...current };
      delete next[accountId];
      return next;
    });
    if (!jobId || trackedJobIdsRef.current[accountId] === jobId) {
      delete trackedJobIdsRef.current[accountId];
    }
  };

  const finishTrackedJob = async (accountId: string, job: ProxyTestJobState) => {
    clearActiveJob(accountId, job.jobId);
    void refreshAccountList();
    if (proxyDialogAccount && proxyDialogAccount.id === accountId) {
      try {
        const settings = await getAccountProxySettings(accountId);
        setProxySettings(settings);
      } catch (e) {
        console.error("Failed to refresh account proxy settings", e);
      }
    }
    try {
      const updatedProfiles = await proxyProfilesClient.listProxyProfiles();
      setProxyProfiles(updatedProfiles.items);
    } catch (e) {
      console.error("Failed to list proxy profiles", e);
    }
    if (job.status === "completed") {
      toast.success(job.kind === "speed" ? t("速度测试通过") : t("代理测试通过"));
      return;
    }
    if (job.status === "cancelled") {
      toast(t("测试已取消"));
      return;
    }
    if (job.error) {
      toast.warning(`${t("测试失败")}: ${job.error}`);
      return;
    }
    toast.warning(job.kind === "speed" ? t("速度测试未通过") : t("代理测试未通过"));
  };

  const pollAccountProxyTestJob = async (accountId: string, initialJob: ProxyTestJobState) => {
    trackedJobIdsRef.current[accountId] = initialJob.jobId;
    setActiveJob(accountId, initialJob);

    let currentJob = initialJob;
    while (!(currentJob.status === "completed" || currentJob.status === "failed" || currentJob.status === "cancelled")) {
      await new Promise((resolve) => window.setTimeout(resolve, 750));
      if (trackedJobIdsRef.current[accountId] !== initialJob.jobId) return;
      try {
        currentJob = await accountClient.getProxyTestJob(accountId, initialJob.jobId);
      } catch (pollError) {
        clearActiveJob(accountId, initialJob.jobId);
        void refreshAccountList();
        toast.error(`${t("读取测试状态失败")}: ${pollError instanceof Error ? pollError.message : String(pollError)}`);
        return;
      }
      if (trackedJobIdsRef.current[accountId] !== initialJob.jobId) return;
      setActiveJob(accountId, currentJob);
    }

    if (trackedJobIdsRef.current[accountId] !== initialJob.jobId) return;
    await finishTrackedJob(accountId, currentJob);
  };

  const runAccountLatencyTest = async (accountId: string) => {
    if (!proxyProfileIdDraft) {
      toast.error(t("请选择代理配置"));
      return;
    }
    try {
      const job = await accountClient.latencyTestProxy({
        accountId,
      });
      void pollAccountProxyTestJob(accountId, job);
    } catch (error) {
      toast.error(`${t("测试失败")}: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const runAccountSpeedTest = async (accountId: string) => {
    if (!proxyProfileIdDraft) {
      toast.error(t("请选择代理配置"));
      return;
    }
    try {
      const job = await accountClient.cloudflareSpeedTestProxy({
        accountId,
        config: {
          downloadPreset: "all",
          uploadPreset: "all",
          runUpload: true,
        },
      });
      void pollAccountProxyTestJob(accountId, job);
    } catch (error) {
      toast.error(`${t("测试失败")}: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const cancelAccountSpeedTest = async (accountId: string, jobId: string) => {
    setIsCancellingJobId(jobId);
    try {
      await accountClient.cancelProxyTestJob(accountId, jobId);
      toast(t("已请求取消测试"));
    } catch (error) {
      toast.error(`${t("取消测试失败")}: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setIsCancellingJobId(null);
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

  const openAccountEditor = (account: Account) => {
    setAccountEditorState({
      accountId: account.id,
      accountName: account.name,
      currentLabel: account.label,
      currentTags: account.tags.join(", "),
      currentNote: account.note || "",
      currentSort: account.priority,
      currentModelSlugs: account.modelSlugs.join(", "),
      currentQuotaPrimaryWindowTokens: account.quotaCapacityPrimaryWindowTokens,
      currentQuotaSecondaryWindowTokens: account.quotaCapacitySecondaryWindowTokens,
    });
    setLabelDraft(account.label);
    setTagsDraft(account.tags.join(", "));
    setNoteDraft(account.note || "");
    setSortDraft(String(account.priority));
    setModelWhitelistDraft(account.modelSlugs.join(", "));
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

  const handleMoveAccount = async (
    account: Account,
    direction: "up" | "down",
  ) => {
    const filteredIndex = filteredAccountIndexMap.get(account.id);
    if (filteredIndex == null) {
      toast.error(t("未找到当前账号，请刷新后重试"));
      return;
    }

    const targetFilteredIndex =
      direction === "up" ? filteredIndex - 1 : filteredIndex + 1;
    if (targetFilteredIndex < 0) {
      toast.info(t("当前账号已经在最前面"));
      return;
    }
    if (targetFilteredIndex >= filteredAccounts.length) {
      toast.info(t("当前账号已经在最后面"));
      return;
    }

    const targetAccount = filteredAccounts[targetFilteredIndex];
    const reorderedAccounts = accounts.filter((item) => item.id !== account.id);
    const anchorIndex = reorderedAccounts.findIndex(
      (item) => item.id === targetAccount.id,
    );
    if (anchorIndex === -1) {
      toast.error(t("未找到目标账号，请刷新后重试"));
      return;
    }

    reorderedAccounts.splice(
      direction === "up" ? anchorIndex : anchorIndex + 1,
      0,
      account,
    );
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
    const nextTags = normalizeTagsDraft(tagsDraft);
    const nextTagsText = nextTags.join(", ");
    const nextNote = noteDraft.trim();
    const nextModelSlugs = normalizeTagsDraft(modelWhitelistDraft);
    const nextModelSlugsText = nextModelSlugs.join(", ");
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
      nextTagsText === accountEditorState.currentTags &&
      nextNote === accountEditorState.currentNote &&
      nextSort === accountEditorState.currentSort &&
      nextModelSlugsText === accountEditorState.currentModelSlugs &&
      nextPrimaryCapacity === accountEditorState.currentQuotaPrimaryWindowTokens &&
      nextSecondaryCapacity === accountEditorState.currentQuotaSecondaryWindowTokens
    ) {
      setAccountEditorState(null);
      return;
    }

    try {
      await updateAccountProfile(accountEditorState.accountId, {
        label: nextLabel,
        note: nextNote || null,
        tags: nextTags,
        sort: nextSort,
        modelSlugs: nextModelSlugs,
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
      isProxySettingsLoading={isProxySettingsLoading}
      proxyEnabledDraft={proxyEnabledDraft}
      proxySourceDraft={proxySourceDraft}
      proxyProfileIdDraft={proxyProfileIdDraft}
      proxyUrlDraft={proxyUrlDraft}
      currentEditingAccount={currentEditingAccount}
      labelDraft={labelDraft}
      tagsDraft={tagsDraft}
      noteDraft={noteDraft}
      sortDraft={sortDraft}
      modelWhitelistDraft={modelWhitelistDraft}
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
      activeJobs={activeJobs}

      presetsData={presetsData}
      isLoadingPresets={isLoadingPresets}
      isPresetsError={isPresetsError}
      presetsError={presetsError}
      isCancellingJobId={isCancellingJobId}

      runAccountLatencyTest={runAccountLatencyTest}
      runAccountSpeedTest={runAccountSpeedTest}
      cancelAccountSpeedTest={cancelAccountSpeedTest}
      isUpdatingProfileAccountId={isUpdatingProfileAccountId}
      isUpdatingStatusAccountId={isUpdatingStatusAccountId}
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
      setTagsDraft={setTagsDraft}
      setNoteDraft={setNoteDraft}
      setSortDraft={setSortDraft}
      setModelWhitelistDraft={setModelWhitelistDraft}
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
      openCleanupDialog={openCleanupDialog}
      toggleCleanupStatus={toggleCleanupStatus}
      handleConfirmCleanupStatuses={handleConfirmCleanupStatuses}
      handleWarmupAccounts={handleWarmupAccounts}
      openExportDialog={openExportDialog}
      handleConfirmExport={handleConfirmExport}
      handleDeleteSingle={handleDeleteSingle}
      openProxyDialog={openProxyDialog}
      handleProxyDialogOpenChange={handleProxyDialogOpenChange}
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
      toggleAccountStatus={toggleAccountStatus}
    />
  );
}
