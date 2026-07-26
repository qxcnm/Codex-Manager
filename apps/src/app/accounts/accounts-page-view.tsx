"use client";

import type { Dispatch, SetStateAction } from "react";
import {
  ArrowDown,
  ArrowUp,
  ArrowUpDown,
  BarChart3,
  Download,
  FileUp,
  FolderOpen,
  KeyRound,
  Loader2,
  MoreVertical,
  Network,
  PencilLine,
  Pin,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  Zap,
} from "lucide-react";
import { AddAccountModal } from "@/components/modals/add-account-modal";
import { AccountResetCreditControl } from "@/components/account-reset-credit-control";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import UsageModal from "@/components/modals/usage-modal";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import { formatCompactNumber } from "@/lib/utils/usage";
import type {
  AccountProxySettings,
  AccountProxySource,
} from "@/lib/api/account-client";
import type { Account, ProxyProfile } from "@/types";
import { AccountProxyCell } from "@/components/accounts/account-proxy-cell";
import { AccountProxyGeoStatusGrid } from "@/components/accounts/account-proxy-status-grid";
import { AccountProxyStatusHeader } from "@/components/accounts/account-proxy-status-header";
import {
  type AccountEditorState,
  type AccountExportMode,
  type AccountSizeSortMode,
  type DeleteDialogState,
  type StatusFilter,
  AccountInfoCell,
  AccountStatusCell,
  QuotaOverviewCell,
  buildQuotaSummaryItems,
  formatAccountExportModeLabel,
  formatAccountPlanLabel,
  formatAccountPlanValueLabel,
  formatPlanFilterLabel,
  formatStatusFilterLabel,
  getAccountStatusAction,
} from "@/app/accounts/accounts-page-helpers";

interface PlanTypeOption {
  value: string;
  count: number;
}

interface StatusFilterOption {
  id: StatusFilter;
  label: string;
}

interface CleanupStatusOption {
  id: string;
  label: string;
  description: string;
  count: number;
}

export interface AccountsPageViewProps {
  accounts: Account[];
  planTypes: PlanTypeOption[];
  isLoading: boolean;
  isServiceReady: boolean;
  isPageActive: boolean;
  search: string;
  planFilter: string;
  statusFilter: StatusFilter;
  pageSize: string;
  safePage: number;
  totalPages: number;
  filteredAccounts: Account[];
  visibleAccounts: Account[];
  filteredAccountIndexMap: Map<string, number>;
  effectiveSelectedIds: string[];
  addAccountModalOpen: boolean;
  usageModalOpen: boolean;
  exportDialogOpen: boolean;
  exportModeDraft: AccountExportMode;
  exportTargetCount: number;
  exportScopeText: string;
  cleanupDialogOpen: boolean;
  cleanupStatusDraft: string[];
  cleanupStatusOptions: CleanupStatusOption[];
  proxyDialogAccount: Account | null;
  proxySettings: AccountProxySettings | null;
  proxyProfiles: ProxyProfile[];
  isProxySettingsLoading: boolean;
  proxyEnabledDraft: boolean;
  proxySourceDraft: AccountProxySource;
  proxyProfileIdDraft: string;
  proxyUrlDraft: string;
  selectedAccount: Account | null;
  accountEditorState: AccountEditorState | null;
  deleteDialogState: DeleteDialogState;
  currentEditingAccount: Account | null;
  labelDraft: string;
  groupNameDraft: string;
  tagsDraft: string;
  noteDraft: string;
  sortDraft: string;
  quotaPrimaryDraft: string;
  quotaSecondaryDraft: string;
  isRefreshingAllAccounts: boolean;
  isRefreshingAccountId: string | null;
  isRefreshingRtAccountId: string | null;
  isRefreshingAllRtAccounts: boolean;
  isExporting: boolean;
  isWarmingUpAccounts: boolean;
  isDeletingMany: boolean;
  isCleaningAccountsByStatus: boolean;
  isUpdatingPreferred: boolean;
  isSavingAccountProxy: boolean;
  isClearingAccountProxy: boolean;
  isTestingAccountProxy: boolean;
  isReorderingAccounts: boolean;
  isUpdatingProfileAccountId: string | null;
  isUpdatingStatusAccountId: string | null;
  statusFilterOptions: StatusFilterOption[];
  importFileActionLabel: string;
  importDirectoryActionLabel: string;
  exportActionLabel: string;
  exportActionShortcut: string;
  setAddAccountModalOpen: Dispatch<SetStateAction<boolean>>;
  setExportDialogOpen: Dispatch<SetStateAction<boolean>>;
  setExportModeDraft: Dispatch<SetStateAction<AccountExportMode>>;
  setDeleteDialogState: Dispatch<SetStateAction<DeleteDialogState>>;
  setCleanupDialogOpen: Dispatch<SetStateAction<boolean>>;
  setProxyEnabledDraft: Dispatch<SetStateAction<boolean>>;
  setProxySourceDraft: Dispatch<SetStateAction<AccountProxySource>>;
  setProxyProfileIdDraft: Dispatch<SetStateAction<string>>;
  setProxyUrlDraft: Dispatch<SetStateAction<string>>;
  setAccountEditorState: Dispatch<SetStateAction<AccountEditorState | null>>;
  setLabelDraft: Dispatch<SetStateAction<string>>;
  setGroupNameDraft: Dispatch<SetStateAction<string>>;
  setTagsDraft: Dispatch<SetStateAction<string>>;
  setNoteDraft: Dispatch<SetStateAction<string>>;
  setSortDraft: Dispatch<SetStateAction<string>>;
  setQuotaPrimaryDraft: Dispatch<SetStateAction<string>>;
  setQuotaSecondaryDraft: Dispatch<SetStateAction<string>>;
  setPage: Dispatch<SetStateAction<number>>;
  handleSearchChange: (value: string) => void;
  handlePlanFilterChange: (value: string | null) => void;
  handleStatusFilterChange: (value: StatusFilter) => void;
  handlePageSizeChange: (value: string | null) => void;
  toggleSelect: (id: string) => void;
  toggleSelectAllVisible: () => void;
  openUsage: (account: Account) => void;
  handleUsageModalOpenChange: (open: boolean) => void;
  handleDeleteSelected: () => void;
  openCleanupDialog: () => void;
  toggleCleanupStatus: (status: string) => void;
  handleConfirmCleanupStatuses: () => Promise<void>;
  handleWarmupAccounts: () => Promise<void>;
  openExportDialog: () => void;
  handleConfirmExport: () => Promise<void>;
  handleDeleteSingle: (account: Account) => void;
  openProxyDialog: (account: Account) => void;
  handleProxyDialogOpenChange: (open: boolean) => void;
  handleSaveProxySettings: () => Promise<void>;
  handleClearProxySettings: () => Promise<void>;
  handleTestProxySettings: () => Promise<void>;
  openAccountEditor: (account: Account) => void;
  handleMoveAccount: (
    account: Account,
    direction: "up" | "down",
  ) => Promise<void>;
  handleApplyAccountSizeSort: (mode: AccountSizeSortMode) => Promise<void>;
  handleConfirmAccountEditor: () => Promise<void>;
  handleConfirmDelete: () => void;
  refreshAllAccounts: () => void;
  refreshAllAccountRt: () => void;
  refreshAccountList: () => void;
  refreshAccountRt: (accountId: string) => void;
  importByFile: () => void;
  importByDirectory: () => void;
  refreshAccount: (accountId: string) => void;
  clearPreferredAccount: (accountId: string) => void;
  setPreferredAccount: (accountId: string) => void;
  toggleAccountStatus: (
    accountId: string,
    enabled: boolean,
    currentStatus: string,
  ) => void;
}

export function AccountsPageView(props: AccountsPageViewProps) {
  const { t } = useI18n();
  const {
    accounts,
    planTypes,
    isLoading,
    isServiceReady,
    isPageActive,
    search,
    planFilter,
    statusFilter,
    pageSize,
    safePage,
    totalPages,
    filteredAccounts,
    visibleAccounts,
    filteredAccountIndexMap,
    effectiveSelectedIds,
    addAccountModalOpen,
    usageModalOpen,
    exportDialogOpen,
    exportModeDraft,
    exportTargetCount,
    exportScopeText,
    cleanupDialogOpen,
    cleanupStatusDraft,
    cleanupStatusOptions,
    proxyDialogAccount,
    proxySettings,
    proxyProfiles,
    isProxySettingsLoading,
    proxyEnabledDraft,
    proxyProfileIdDraft,
    selectedAccount,
    accountEditorState,
    deleteDialogState,
    currentEditingAccount,
    labelDraft,
    groupNameDraft,
    tagsDraft,
    noteDraft,
    sortDraft,
    quotaPrimaryDraft,
    quotaSecondaryDraft,
    isRefreshingAllAccounts,
    isRefreshingAccountId,
    isRefreshingRtAccountId,
    isRefreshingAllRtAccounts,
    isExporting,
    isWarmingUpAccounts,
    isDeletingMany,
    isCleaningAccountsByStatus,
    isUpdatingPreferred,
    isSavingAccountProxy,
    isClearingAccountProxy,
    isTestingAccountProxy,
    isReorderingAccounts,
    isUpdatingProfileAccountId,
    isUpdatingStatusAccountId,
    statusFilterOptions,
    importFileActionLabel,
    importDirectoryActionLabel,
    exportActionLabel,
    exportActionShortcut,
    setAddAccountModalOpen,
    setExportDialogOpen,
    setExportModeDraft,
    setDeleteDialogState,
    setCleanupDialogOpen,
    setProxyEnabledDraft,
    setProxyProfileIdDraft,
    setAccountEditorState,
    setLabelDraft,
    setGroupNameDraft,
    setTagsDraft,
    setNoteDraft,
    setSortDraft,
    setQuotaPrimaryDraft,
    setQuotaSecondaryDraft,
    setPage,
    handleSearchChange,
    handlePlanFilterChange,
    handleStatusFilterChange,
    handlePageSizeChange,
    toggleSelect,
    toggleSelectAllVisible,
    openUsage,
    handleUsageModalOpenChange,
    handleDeleteSelected,
    openCleanupDialog,
    toggleCleanupStatus,
    handleConfirmCleanupStatuses,
    handleWarmupAccounts,
    openExportDialog,
    handleConfirmExport,
    handleDeleteSingle,
    openProxyDialog,
    handleProxyDialogOpenChange,
    handleSaveProxySettings,
    handleClearProxySettings,
    handleTestProxySettings,
    openAccountEditor,
    handleMoveAccount,
    handleApplyAccountSizeSort,
    handleConfirmAccountEditor,
    handleConfirmDelete,
    refreshAllAccounts,
    refreshAllAccountRt,
    refreshAccountList,
    refreshAccountRt,
    importByFile,
    importByDirectory,
    refreshAccount,
    clearPreferredAccount,
    setPreferredAccount,
    toggleAccountStatus,
  } = props;

  const accountProxyBusy =
    isProxySettingsLoading || isSavingAccountProxy || isClearingAccountProxy;
  const selectedProxyProfile =
    proxyProfiles.find((profile) => profile.id === proxyProfileIdDraft) || null;
  const needsMissingProxyProfileOption =
    Boolean(proxyProfileIdDraft) && !selectedProxyProfile;
  const cleanupSelectedCount = cleanupStatusOptions.reduce(
    (total, option) =>
      cleanupStatusDraft.includes(option.id) ? total + option.count : total,
    0,
  );

  return (
    <div className="space-y-6">
      {!isServiceReady ? (
        <Card className="glass-card mission-panel shadow-sm">
          <CardContent className="pt-6 text-sm text-muted-foreground">
            {t(
              "服务未连接，账号列表与相关操作暂不可用；连接恢复后会自动继续加载。",
            )}
          </CardContent>
        </Card>
      ) : null}

      <Card className="glass-card mission-panel shadow-sm">
        <CardContent className="grid min-w-0 gap-3 pt-0 lg:grid-cols-[200px_auto_minmax(0,1fr)_auto] lg:items-center">
          <div className="min-w-0">
            <Input
              placeholder={t("搜索账号名 / 编号...")}
              className="glass-card mission-panel h-10 rounded-xl px-3"
              value={search}
              onChange={(event) => handleSearchChange(event.target.value)}
            />
          </div>

          <div className="grid min-w-0 grid-cols-2 gap-2 sm:flex sm:shrink-0 sm:items-center sm:gap-3">
            <Select value={planFilter} onValueChange={handlePlanFilterChange}>
              <SelectTrigger className="h-10 w-full min-w-0 rounded-xl bg-card/50 sm:w-[140px] sm:shrink-0">
                <SelectValue placeholder={t("全部类型")}>
                  {(value) => formatPlanFilterLabel(String(value || ""), t)}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                    <SelectGroup>
                <SelectItem value="all">
                  {t("全部类型")} ({accounts.length})
                </SelectItem>
                {planTypes.map((planType) => (
                  <SelectItem key={planType.value} value={planType.value}>
                    {formatAccountPlanValueLabel(planType.value, t)} (
                    {planType.count})
                  </SelectItem>
                ))}
                </SelectGroup>
              </SelectContent>
            </Select>
            <Select
              value={statusFilter}
              onValueChange={(value) =>
                handleStatusFilterChange(value as StatusFilter)
              }
            >
              <SelectTrigger className="h-10 w-full min-w-0 rounded-xl bg-card/50 sm:w-[152px] sm:shrink-0">
                <SelectValue placeholder={t("全部状态")}>
                  {(value) => formatStatusFilterLabel(String(value || ""), t)}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                    <SelectGroup>
                {statusFilterOptions.map((filter) => (
                  <SelectItem key={filter.id} value={filter.id}>
                    {filter.label}
                  </SelectItem>
                ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>

          <div className="hidden min-w-0 lg:block" />

          <div className="flex min-w-0 flex-wrap items-center gap-2 sm:ml-auto sm:shrink-0 lg:ml-0 lg:justify-self-end">
            <Tooltip>
              <TooltipTrigger render={<span />} className="inline-flex">
                <Button
                  variant="outline"
                  className="glass-card mission-panel h-10 min-w-[88px] gap-2 rounded-xl px-3"
                  disabled={
                    !isServiceReady || isWarmingUpAccounts || accounts.length === 0
                  }
                  onClick={() => void handleWarmupAccounts()}
                >
                  {isWarmingUpAccounts ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Zap className="h-4 w-4" />
                  )}
                  <span className="text-sm font-medium">
                    {isWarmingUpAccounts ? t("预热中...") : t("预热")}
                  </span>
                </Button>
              </TooltipTrigger>
              <TooltipContent className="max-w-xs whitespace-pre-wrap break-words">
                {t(
                  "向选中账号发送 hi 进行预热；如果未选中账号，则默认预热全部账号。",
                )}
              </TooltipContent>
            </Tooltip>
            <DropdownMenu>
              <DropdownMenuTrigger>
                <Button
                  variant="outline"
                  className="glass-card mission-panel h-10 min-w-[50px] justify-between gap-2 rounded-xl px-3"
                  render={<span />}
                  nativeButton={false}
                >
                  <span className="flex items-center gap-2">
                    <span className="text-sm font-medium">{t("账号操作")}</span>
                    {effectiveSelectedIds.length > 0 ? (
                      <span className="rounded-full bg-primary/10 px-2 py-0.5 text-[10px] font-semibold text-primary">
                        {effectiveSelectedIds.length}
                      </span>
                    ) : null}
                  </span>
                  <MoreVertical className="h-4 w-4 text-muted-foreground" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                align="end"
                className="w-64 rounded-xl border border-border/70 bg-popover/95 p-2 shadow-sm"
              >
                                  <DropdownMenuGroup>
                  <DropdownMenuLabel className="px-2 py-1 text-[11px] uppercase tracking-[0.16em] text-muted-foreground/80">
                    {t("刷新")}
                  </DropdownMenuLabel>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={!isServiceReady || isRefreshingAllAccounts}
                    onClick={refreshAllAccounts}
                  >
                    <RefreshCw
                      className={cn(
                        "mr-2 h-4 w-4",
                        isRefreshingAllAccounts && "animate-spin",
                      )}
                    />
                    {t("刷新账号用量")}
                    <DropdownMenuShortcut>ALL</DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={!isServiceReady || isRefreshingAllRtAccounts}
                    onClick={refreshAllAccountRt}
                  >
                    <KeyRound
                      className={cn(
                        "mr-2 h-4 w-4",
                        isRefreshingAllRtAccounts && "animate-pulse",
                      )}
                    />
                    {t("刷新全部 AT/RT")}
                    <DropdownMenuShortcut>RT</DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={!isServiceReady}
                    onClick={refreshAccountList}
                  >
                    <RefreshCw className="mr-2 h-4 w-4" />
                    {t("刷新列表")}
                    <DropdownMenuShortcut>LIST</DropdownMenuShortcut>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuLabel className="px-2 py-1 text-[11px] uppercase tracking-[0.16em] text-muted-foreground/80">
                    {t("号池管理")}
                  </DropdownMenuLabel>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={!isServiceReady}
                    onClick={() => setAddAccountModalOpen(true)}
                  >
                    <Plus className="mr-2 h-4 w-4" /> {t("添加账号")}
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={!isServiceReady}
                    onClick={importByFile}
                  >
                    <FileUp className="mr-2 h-4 w-4" /> {importFileActionLabel}
                    <DropdownMenuShortcut>FILE</DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={!isServiceReady}
                    onClick={importByDirectory}
                  >
                    <FolderOpen className="mr-2 h-4 w-4" />
                    {importDirectoryActionLabel}
                    <DropdownMenuShortcut>DIR</DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={
                      !isServiceReady || isExporting || accounts.length === 0
                    }
                    onClick={openExportDialog}
                  >
                    <Download className="mr-2 h-4 w-4" />
                    {exportActionLabel}
                    <DropdownMenuShortcut>
                      {exportActionShortcut}
                    </DropdownMenuShortcut>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuLabel className="px-2 py-1 text-[11px] uppercase tracking-[0.16em] text-muted-foreground/80">
                    {t("排序")}
                  </DropdownMenuLabel>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={
                      !isServiceReady ||
                      isReorderingAccounts ||
                      accounts.length < 2
                    }
                    onClick={() => void handleApplyAccountSizeSort("large-first")}
                  >
                    <ArrowUpDown className="mr-2 h-4 w-4" />
                    {t("大号优先排序")}
                    <DropdownMenuShortcut>BIZ</DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    className="h-9 rounded-lg px-2"
                    disabled={
                      !isServiceReady ||
                      isReorderingAccounts ||
                      accounts.length < 2
                    }
                    onClick={() => void handleApplyAccountSizeSort("small-first")}
                  >
                    <ArrowDown className="mr-2 h-4 w-4" />
                    {t("小号优先排序")}
                    <DropdownMenuShortcut>FREE</DropdownMenuShortcut>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuLabel className="px-2 py-1 text-[11px] uppercase tracking-[0.16em] text-muted-foreground/80">
                    {t("清理")}
                  </DropdownMenuLabel>
                  <DropdownMenuItem
                    disabled={
                      !isServiceReady ||
                      !effectiveSelectedIds.length ||
                      isDeletingMany
                    }
                    variant="destructive"
                    className="h-9 rounded-lg px-2"
                    onClick={handleDeleteSelected}
                  >
                    <Trash2 className="mr-2 h-4 w-4" /> {t("删除选中账号")}
                    <DropdownMenuShortcut>
                      {effectiveSelectedIds.length || "-"}
                    </DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    variant="destructive"
                    className="h-9 rounded-lg px-2"
                    disabled={
                      !isServiceReady ||
                      isCleaningAccountsByStatus ||
                      accounts.length === 0
                    }
                    onClick={openCleanupDialog}
                  >
                    <Trash2 className="mr-2 h-4 w-4" /> {t("按状态清理账号")}
                  </DropdownMenuItem>
                </DropdownMenuGroup>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </CardContent>
      </Card>

      <Dialog open={exportDialogOpen} onOpenChange={setExportDialogOpen}>
        <DialogContent className="glass-card mission-panel border-border/70 sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("导出账号")}</DialogTitle>
            <DialogDescription>
              {t("选择导出方式；如果已勾选账号，则只导出当前选中项。")}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div className="rounded-xl border border-border/60 bg-muted/30 p-3 text-sm text-muted-foreground">
              {exportScopeText}
            </div>
            <div className="grid gap-3">
              <Label>{t("导出格式")}</Label>
              <Select
                value={exportModeDraft}
                onValueChange={(value) =>
                  setExportModeDraft(value as AccountExportMode)
                }
              >
                <SelectTrigger className="h-11 rounded-xl bg-background/70">
                  <SelectValue>
                    {(value) =>
                      formatAccountExportModeLabel(String(value || ""), t)
                    }
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                    <SelectGroup>
                  <SelectItem value="multiple">
                    {formatAccountExportModeLabel("multiple", t)}
                  </SelectItem>
                  <SelectItem value="single">
                    {formatAccountExportModeLabel("single", t)}
                  </SelectItem>
                  </SelectGroup>
                </SelectContent>
              </Select>
              <div className="rounded-xl bg-accent/20 px-3 py-2">
                <div className="text-xs text-muted-foreground">
                  {exportModeDraft === "single"
                    ? t(
                        "导出为一个 `accounts.json` 数组文件，适合整体备份和再次导入。",
                      )
                    : t(
                        "每个账号导出为一个独立 JSON 文件，适合逐个分发或单独管理。",
                      )}
                </div>
              </div>
            </div>
          </div>
          <DialogFooter>
            <DialogClose
              className={cn(
                buttonVariants({ variant: "outline" }),
                "rounded-xl",
              )}
              disabled={isExporting}
            >
              {t("取消")}
            </DialogClose>
            <Button
              className="rounded-xl"
              onClick={() => void handleConfirmExport()}
              disabled={isExporting || exportTargetCount <= 0}
            >
              {isExporting ? t("导出中...") : t("开始导出")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={isPageActive && cleanupDialogOpen}
        onOpenChange={(open) => {
          if (!isCleaningAccountsByStatus) {
            setCleanupDialogOpen(open);
          }
        }}
      >
        <DialogContent className="glass-card mission-panel border-border/70 sm:max-w-[520px]">
          <DialogHeader>
            <DialogTitle>{t("按状态清理账号")}</DialogTitle>
            <DialogDescription>
              {t("选择要删除的账号状态；删除后不可恢复。")}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="rounded-xl border border-destructive/20 bg-destructive/5 px-3 py-2 text-xs text-destructive">
              {t("将删除所有匹配所选状态的账号，不再额外限制账号套餐。")}
            </div>
            <div className="grid gap-2">
              {cleanupStatusOptions.map((option) => {
                const checked = cleanupStatusDraft.includes(option.id);
                return (
                  <div
                    key={option.id}
                    className={cn(
                      "flex w-full items-center gap-3 rounded-xl border px-3 py-3 text-left transition-colors",
                      checked
                        ? "border-primary/40 bg-primary/10"
                        : "border-border/70 bg-background/45",
                    )}
                  >
                    <Checkbox
                      checked={checked}
                      disabled={isCleaningAccountsByStatus}
                      onCheckedChange={() => toggleCleanupStatus(option.id)}
                      aria-label={option.label}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium">
                          {option.label}
                        </span>
                        <span className="rounded-full bg-muted px-2 py-0.5 text-[10px] font-semibold text-muted-foreground">
                          {option.count}
                        </span>
                      </div>
                      <p className="mt-1 text-xs text-muted-foreground">
                        {option.description}
                      </p>
                    </div>
                  </div>
                );
              })}
            </div>
            <div className="rounded-xl bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
              {t("预计删除")}{" "}
              <span className="font-semibold text-foreground">
                {cleanupSelectedCount}
              </span>{" "}
              {t("个账号")}
            </div>
          </div>
          <DialogFooter className="gap-2 sm:gap-2">
            <DialogClose
              className={buttonVariants({ variant: "outline" })}
              type="button"
              disabled={isCleaningAccountsByStatus}
            >
              {t("取消")}
            </DialogClose>
            <Button
              variant="destructive"
              disabled={
                isCleaningAccountsByStatus ||
                cleanupStatusDraft.length === 0 ||
                cleanupSelectedCount <= 0
              }
              onClick={() => void handleConfirmCleanupStatuses()}
            >
              {isCleaningAccountsByStatus ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Trash2 className="mr-2 h-4 w-4" />
              )}
              {t("确认清理")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Card className="glass-card mission-panel overflow-hidden py-0 shadow-sm">
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-12 text-center">
                  <Checkbox
                    checked={
                      visibleAccounts.length > 0 &&
                      visibleAccounts.every((account) =>
                        effectiveSelectedIds.includes(account.id),
                      )
                    }
                    onCheckedChange={toggleSelectAllVisible}
                  />
                </TableHead>
                <TableHead className="w-[clamp(280px,34vw,440px)] min-w-[280px] max-w-[440px] whitespace-normal">
                  {t("账号信息")}
                </TableHead>
                <TableHead className="min-w-[300px] text-center">
                  {t("额度详情")}
                </TableHead>
                <TableHead className="w-[132px]">{t("顺序")}</TableHead>
                <TableHead className="min-w-[180px]">{t("账号代理")}</TableHead>
                <TableHead className="w-[112px]">{t("状态")}</TableHead>
                <TableHead className="table-sticky-action-head w-[112px] text-center">
                  {t("操作")}
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                Array.from({ length: 5 }).map((_, index) => (
                  <TableRow key={index}>
                    <TableCell>
                      <Skeleton className="mx-auto h-4 w-4" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-32" />
                    </TableCell>
                    <TableCell>
                      <div className="space-y-2">
                        <Skeleton className="h-4 w-40" />
                        <Skeleton className="h-4 w-40" />
                        <Skeleton className="h-4 w-40" />
                      </div>
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-10" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-8 w-28" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-6 w-16 rounded-full" />
                    </TableCell>
                    <TableCell className="table-sticky-action-cell">
                      <Skeleton className="mx-auto h-8 w-24" />
                    </TableCell>
                  </TableRow>
                ))
              ) : visibleAccounts.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={7} className="h-48 text-center">
                    <div className="flex w-[calc(100dvw-6rem)] flex-col items-center justify-center gap-2 text-muted-foreground sm:w-auto">
                      <Search className="h-8 w-8 opacity-20" />
                      <p>{t("未找到符合条件的账号")}</p>
                    </div>
                  </TableCell>
                </TableRow>
              ) : (
                visibleAccounts.map((account) => {
                  const quotaItems = buildQuotaSummaryItems(account, t);
                  const statusAction = getAccountStatusAction(account, t);
                  const StatusActionIcon = statusAction.icon;
                  const isRefreshingCurrentAccount =
                    isRefreshingAccountId === account.id;
                  const isRefreshingCurrentRt =
                    isRefreshingRtAccountId === account.id;
                  const filteredIndex =
                    filteredAccountIndexMap.get(account.id) ?? -1;
                  const canMoveUp = filteredIndex > 0;
                  const canMoveDown =
                    filteredIndex !== -1 &&
                    filteredIndex < filteredAccounts.length - 1;
                  return (
                    <TableRow key={account.id} className="group">
                      <TableCell className="text-center">
                        <Checkbox
                          checked={effectiveSelectedIds.includes(account.id)}
                          onCheckedChange={() => toggleSelect(account.id)}
                        />
                      </TableCell>
                      <TableCell className="w-[clamp(280px,34vw,440px)] min-w-[280px] max-w-[440px] whitespace-normal align-top">
                        <AccountInfoCell
                          account={account}
                          isPreferred={account.preferred}
                        />
                      </TableCell>
                      <TableCell>
                        <QuotaOverviewCell items={quotaItems} />
                        <div className="mt-2 flex flex-wrap gap-1.5 text-[11px] text-muted-foreground">
                          {account.quotaCapacityPrimaryWindowTokens ||
                          account.quotaCapacitySecondaryWindowTokens ? (
                            <span className="inline-flex min-h-5 max-w-full items-center rounded-full border border-border/50 bg-background/40 px-2 py-0.5 leading-none break-words [overflow-wrap:anywhere]">
                              {t("容量覆盖")}:{" "}
                              {account.quotaCapacityPrimaryWindowTokens
                                ? `5h ${formatCompactNumber(
                                    account.quotaCapacityPrimaryWindowTokens,
                                    "0.00",
                                    2,
                                    true,
                                  )}`
                                : "5h --"}
                              {" / "}
                              {account.quotaCapacitySecondaryWindowTokens
                                ? `7d ${formatCompactNumber(
                                    account.quotaCapacitySecondaryWindowTokens,
                                    "0.00",
                                    2,
                                    true,
                                  )}`
                                : "7d --"}
                            </span>
                          ) : (
                            <span className="inline-flex min-h-5 max-w-full items-center rounded-full border border-border/50 bg-background/40 px-2 py-0.5 leading-none break-words [overflow-wrap:anywhere]">
                              {t("未设置账号容量覆盖")}
                            </span>
                          )}
                          <AccountResetCreditControl
                            account={account}
                            disabled={!isServiceReady}
                          />
                        </div>
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1">
                          <span className="min-w-8 rounded-md bg-muted/60 px-2 py-1 text-center font-mono text-xs font-semibold tabular-nums">
                            {account.priority}
                          </span>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary active:scale-95"
                            disabled={
                              !isServiceReady ||
                              !canMoveUp ||
                              isReorderingAccounts ||
                              isUpdatingProfileAccountId === account.id
                            }
                            onClick={() => void handleMoveAccount(account, "up")}
                            title={t("上移一位")}
                          >
                            <ArrowUp className="h-4 w-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary active:scale-95"
                            disabled={
                              !isServiceReady ||
                              !canMoveDown ||
                              isReorderingAccounts ||
                              isUpdatingProfileAccountId === account.id
                            }
                            onClick={() =>
                              void handleMoveAccount(account, "down")
                            }
                            title={t("下移一位")}
                          >
                            <ArrowDown className="h-4 w-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground transition-colors hover:bg-primary/10 hover:text-primary active:scale-95"
                            disabled={
                              !isServiceReady ||
                              isReorderingAccounts ||
                              isUpdatingProfileAccountId === account.id
                            }
                            onClick={() => openAccountEditor(account)}
                            title={t("编辑账号信息")}
                          >
                            <PencilLine className="h-4 w-4" />
                          </Button>
                        </div>
                      </TableCell>
                      <TableCell>
                        <AccountProxyCell account={account} />
                      </TableCell>
                      <TableCell>
                        <AccountStatusCell account={account} />
                      </TableCell>
                      <TableCell className="table-sticky-action-cell">
                        <div className="table-action-cell gap-1">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                            disabled={!isServiceReady}
                            onClick={() => openUsage(account)}
                            title={t("用量详情")}
                            aria-label={t("用量详情")}
                          >
                            <BarChart3 className="h-4 w-4" />
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
                                title={t("更多账号操作")}
                                aria-label={t("更多账号操作")}
                              >
                                <MoreVertical className="h-4 w-4" />
                                <span className="sr-only">
                                  {t("更多账号操作")}
                                </span>
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                                  <DropdownMenuGroup>
                              <DropdownMenuItem
                                className="gap-2"
                                disabled={
                                  !isServiceReady ||
                                  isRefreshingAllAccounts ||
                                  isRefreshingCurrentAccount
                                }
                                onClick={() => refreshAccount(account.id)}
                              >
                                <RefreshCw
                                  className={cn(
                                    "h-4 w-4",
                                    isRefreshingCurrentAccount && "animate-spin",
                                  )}
                                />
                                {t("刷新用量")}
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                className="gap-2"
                                disabled={!isServiceReady || isRefreshingCurrentRt}
                                onClick={() => refreshAccountRt(account.id)}
                              >
                                <KeyRound
                                  className={cn(
                                    "h-4 w-4",
                                    isRefreshingCurrentRt && "animate-pulse",
                                  )}
                                />
                                {t("刷新 AT/RT")}
                                <DropdownMenuShortcut>RT</DropdownMenuShortcut>
                              </DropdownMenuItem>
                              <DropdownMenuSeparator />
                              <DropdownMenuItem
                                className="gap-2"
                                disabled={!isServiceReady || isUpdatingPreferred}
                                onClick={() =>
                                  account.preferred
                                    ? clearPreferredAccount(account.id)
                                    : setPreferredAccount(account.id)
                                }
                              >
                                <Pin className="h-4 w-4" />
                                {account.preferred ? t("取消优先") : t("设为优先")}
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                className="gap-2"
                                disabled={!isServiceReady}
                                onClick={() => void openProxyDialog(account)}
                              >
                                <Network className="h-4 w-4" />
                                {t("账号代理")}
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                className="gap-2"
                                disabled={
                                  !isServiceReady ||
                                  isUpdatingStatusAccountId === account.id ||
                                  statusAction.action === null
                                }
                                onClick={() =>
                                  statusAction.action &&
                                  toggleAccountStatus(
                                    account.id,
                                    statusAction.action === "enable",
                                    account.status,
                                  )
                                }
                              >
                                <StatusActionIcon className="h-4 w-4" />
                                {statusAction.label}
                              </DropdownMenuItem>
                              <DropdownMenuSeparator />
                              <DropdownMenuItem
                                className="gap-2 text-red-500"
                                disabled={!isServiceReady}
                                onClick={() => handleDeleteSingle(account)}
                              >
                                <Trash2 className="h-4 w-4" /> {t("删除")}
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

      <div className="flex flex-col gap-3 px-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="text-xs text-muted-foreground">
          {t("共")} {filteredAccounts.length} {t("个账号")}
          {effectiveSelectedIds.length > 0 ? (
            <span className="ml-1 text-primary">
              ({t("已选择")} {effectiveSelectedIds.length} {t("个")})
            </span>
          ) : null}
        </div>
        <div className="flex w-full flex-col gap-3 sm:w-auto sm:flex-row sm:items-center sm:gap-6">
          <div className="flex items-center justify-between gap-2 sm:justify-start">
            <span className="whitespace-nowrap text-xs text-muted-foreground">
              {t("每页显示")}
            </span>
            <Select value={pageSize} onValueChange={handlePageSizeChange}>
              <SelectTrigger className="h-8 w-[70px] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                    <SelectGroup>
                {["5", "10", "20", "50", "100", "500"].map((value) => (
                  <SelectItem key={value} value={value}>
                    {value}
                  </SelectItem>
                ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
              disabled={safePage <= 1}
              onClick={() => setPage((current) => Math.max(1, current - 1))}
            >
              {t("上一页")}
            </Button>
            <div className="min-w-[60px] text-center text-xs font-medium">
              {t("第")} {safePage} / {totalPages} {t("页")}
            </div>
            <Button
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
              disabled={safePage >= totalPages}
              onClick={() =>
                setPage((current) => Math.min(totalPages, current + 1))
              }
            >
              {t("下一页")}
            </Button>
          </div>
        </div>
      </div>

      {addAccountModalOpen ? (
        <AddAccountModal
          open={isPageActive && addAccountModalOpen}
          onOpenChange={setAddAccountModalOpen}
        />
      ) : null}
      <UsageModal
        account={selectedAccount}
        open={isPageActive && usageModalOpen}
        onOpenChange={handleUsageModalOpenChange}
        onRefresh={refreshAccount}
        onRefreshRt={refreshAccountRt}
        isRefreshing={
          isRefreshingAllAccounts ||
          (!!selectedAccount && isRefreshingAccountId === selectedAccount.id)
        }
        isRefreshingRt={
          !!selectedAccount && isRefreshingRtAccountId === selectedAccount.id
        }
      />
      <Dialog
        open={isPageActive && Boolean(proxyDialogAccount)}
        onOpenChange={handleProxyDialogOpenChange}
      >
        <DialogContent className="glass-card max-h-[calc(100vh-2rem)] overflow-hidden p-0 sm:max-w-[672px]">
          <DialogHeader className="px-6 pt-6">
            <DialogTitle>{t("账号代理")}</DialogTitle>
            <DialogDescription>
              {proxyDialogAccount?.name || t("为当前账号选择独立代理配置")}
            </DialogDescription>
          </DialogHeader>
          <div className="grid max-h-[calc(100vh-13rem)] gap-4 overflow-y-auto px-6 py-4">
            <div className="flex items-center justify-between gap-4 rounded-xl border border-border/70 bg-muted/20 px-3 py-3">
              <div className="min-w-0">
                <Label htmlFor="account-proxy-enabled" className="text-sm">
                  {t("启用账号代理")}
                </Label>
                <p className="mt-1 text-[11px] leading-4 text-muted-foreground">
                  {t("启用后，该账号对 OpenAI API 的请求将通过指定的代理发送。")}
                </p>
              </div>
              <Switch
                id="account-proxy-enabled"
                checked={proxyEnabledDraft}
                disabled={accountProxyBusy}
                onCheckedChange={(value) =>
                  setProxyEnabledDraft(Boolean(value))
                }
              />
            </div>

            <div className="grid gap-2">
              <Label htmlFor="account-proxy-profile">{t("代理配置")}</Label>
              <Select
                value={proxyProfileIdDraft || "__empty__"}
                disabled={accountProxyBusy || proxyProfiles.length === 0}
                onValueChange={(value) =>
                  setProxyProfileIdDraft(
                    !value || value === "__empty__" ? "" : value,
                  )
                }
              >
                <SelectTrigger
                  id="account-proxy-profile"
                  className="rounded-xl bg-card/50"
                >
                  <SelectValue
                    placeholder={
                      proxyProfiles.length === 0
                        ? t("暂无可用代理配置")
                        : t("选择代理配置")
                    }
                  >
                    {proxyProfileIdDraft
                      ? selectedProxyProfile?.name ||
                        proxySettings?.proxyProfileName ||
                        proxyProfileIdDraft
                      : undefined}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {proxyProfiles.length === 0 ? (
                    <SelectItem value="__empty__" disabled>
                      {t("暂无可用代理配置")}
                    </SelectItem>
                  ) : null}
                  {needsMissingProxyProfileOption ? (
                    <SelectItem value={proxyProfileIdDraft} disabled>
                      {proxySettings?.proxyProfileName || t("已删除的代理配置")}
                    </SelectItem>
                  ) : null}
                  {proxyProfiles.map((profile) => (
                    <SelectItem key={profile.id} value={profile.id}>
                      {profile.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <div className="rounded-xl border border-border/60 bg-muted/20 px-3 py-3 text-sm leading-5 text-muted-foreground">
                <div className="break-all">
                  {selectedProxyProfile?.proxyUrlRedacted ||
                    proxySettings?.proxyUrlRedacted ||
                    t("未绑定代理端点")}
                </div>
                <div className="mt-1">
                  {selectedProxyProfile
                    ? selectedProxyProfile.enabled
                      ? t("当前配置已启用")
                      : t("当前配置已禁用，保存后该账号会 fail-closed")
                    : t("未选择代理配置")}
                </div>
              </div>
            </div>

            {isProxySettingsLoading ? (
              <div className="flex items-center justify-center py-8 text-sm text-muted-foreground">
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                {t("加载中...")}
              </div>
            ) : (
              <div className="rounded-xl bg-muted/20 px-4 py-4 text-xs">
                <AccountProxyStatusHeader
                  status={proxySettings?.status}
                  latencyMs={proxySettings?.latencyMs}
                  lastTestedAt={proxySettings?.lastCheckAt}
                  t={t}
                />
                <AccountProxyGeoStatusGrid geo={proxySettings} t={t} />
              </div>
            )}

            {proxySettings?.lastError ? (
              <div className="rounded-xl border border-destructive/20 bg-destructive/5 px-4 py-3 text-xs">
                <div className="text-muted-foreground">{t("错误")}</div>
                <div className="mt-1 break-words font-mono text-destructive [overflow-wrap:anywhere]">
                  {proxySettings.lastError}
                </div>
              </div>
            ) : null}
          </div>
          <DialogFooter className="mx-0 mb-0 gap-2 rounded-b-xl border-t bg-muted/40 px-6 py-4 sm:gap-2">
            <Button
              type="button"
              variant="destructive"
              disabled={accountProxyBusy || isTestingAccountProxy}
              onClick={() => void handleClearProxySettings()}
            >
              {isClearingAccountProxy ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : null}
              {t("清除")}
            </Button>
            <Button
              type="button"
              variant="outline"
              disabled={accountProxyBusy || isTestingAccountProxy}
              onClick={() => void handleTestProxySettings()}
            >
              <RefreshCw
                className={cn(
                  "mr-2 h-4 w-4",
                  isTestingAccountProxy && "animate-spin",
                )}
              />
              {isTestingAccountProxy ? t("测试中...") : t("测试")}
            </Button>
            <DialogClose
              className={buttonVariants({ variant: "outline" })}
              disabled={accountProxyBusy || isTestingAccountProxy}
            >
              {t("关闭")}
            </DialogClose>
            <Button
              type="button"
              disabled={accountProxyBusy || isTestingAccountProxy}
              onClick={() => void handleSaveProxySettings()}
            >
              {isSavingAccountProxy ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : null}
              {t("保存")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        open={isPageActive && Boolean(deleteDialogState)}
        onOpenChange={(open) => {
          if (!open) {
            setDeleteDialogState(null);
          }
        }}
        title={
          deleteDialogState?.kind === "single"
            ? t("删除账号")
            : t("批量删除账号")
        }
        description={
          deleteDialogState?.kind === "single"
            ? `${t("确定删除账号")} ${deleteDialogState.account.name} ${t("吗？删除后不可恢复。")}`
            : `${t("确定删除选中的")} ${deleteDialogState?.count || 0} ${t("个账号吗？删除后不可恢复。")}`
        }
        confirmText={t("删除")}
        confirmVariant="destructive"
        onConfirm={handleConfirmDelete}
      />
      <Dialog
        open={isPageActive && Boolean(accountEditorState)}
        onOpenChange={(open) => {
          if (!open && !isUpdatingProfileAccountId) {
            setAccountEditorState(null);
          }
        }}
      >
        <DialogContent className="glass-card mission-panel max-h-[calc(100vh-2rem)] overflow-hidden p-0 sm:max-w-[560px]">
          <DialogHeader className="px-6 pt-6">
            <DialogTitle>{t("编辑账号信息")}</DialogTitle>
            <DialogDescription>
              {accountEditorState
                ? `${t("修改")} ${accountEditorState.accountName} ${t("的名称、分组、标签、备注、排序与额度池配置。")}`
                : t("修改账号的基础资料。")}
            </DialogDescription>
          </DialogHeader>
          <div className="grid max-h-[calc(100vh-13rem)] gap-4 overflow-y-auto px-6 py-4">
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="grid gap-2">
                <Label htmlFor="account-label-input">{t("账号名称")}</Label>
                <Input
                  id="account-label-input"
                  value={labelDraft}
                  disabled={Boolean(isUpdatingProfileAccountId)}
                  onChange={(event) => setLabelDraft(event.target.value)}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="account-group-input">{t("账号分组")}</Label>
                <Input
                  id="account-group-input"
                  value={groupNameDraft}
                  disabled={Boolean(isUpdatingProfileAccountId)}
                  onChange={(event) => setGroupNameDraft(event.target.value)}
                  placeholder={t("例如：团队 A")}
                />
              </div>
              <div className="grid gap-2 sm:col-span-2">
                <Label htmlFor="account-tags-input">
                  {t("标签（逗号分隔）")}
                </Label>
                <Input
                  id="account-tags-input"
                  value={tagsDraft}
                  disabled={Boolean(isUpdatingProfileAccountId)}
                  onChange={(event) => setTagsDraft(event.target.value)}
                  placeholder={t("例如：高频, 团队A")}
                />
              </div>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="account-note-input">{t("备注")}</Label>
              <Textarea
                id="account-note-input"
                value={noteDraft}
                disabled={Boolean(isUpdatingProfileAccountId)}
                onChange={(event) => setNoteDraft(event.target.value)}
                placeholder={t("例如：主账号 / 测试号 / 团队共享")}
                className="min-h-[108px]"
              />
            </div>
            <div className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_120px] sm:items-end">
              <div className="grid gap-2">
                <Label htmlFor="account-sort-input">{t("顺序值")}</Label>
                <Input
                  id="account-sort-input"
                  type="number"
                  min={0}
                  step={1}
                  value={sortDraft}
                  disabled={Boolean(isUpdatingProfileAccountId)}
                  onChange={(event) => setSortDraft(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void handleConfirmAccountEditor();
                    }
                  }}
                />
              </div>
              <div className="grid gap-1 rounded-xl bg-muted/30 px-3 py-2 text-[11px] text-muted-foreground">
                <span>{t("值越小越靠前")}</span>
                <span>{t("仅修改当前账号")}</span>
              </div>
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="grid gap-2">
                <Label htmlFor="account-quota-primary-input">
                  {t("5h 容量覆盖（Token）")}
                </Label>
                <Input
                  id="account-quota-primary-input"
                  type="number"
                  min={1}
                  step={1}
                  value={quotaPrimaryDraft}
                  disabled={Boolean(isUpdatingProfileAccountId)}
                  onChange={(event) => setQuotaPrimaryDraft(event.target.value)}
                  placeholder={t("留空使用计划模板")}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="account-quota-secondary-input">
                  {t("7d 容量覆盖（Token）")}
                </Label>
                <Input
                  id="account-quota-secondary-input"
                  type="number"
                  min={1}
                  step={1}
                  value={quotaSecondaryDraft}
                  disabled={Boolean(isUpdatingProfileAccountId)}
                  onChange={(event) => setQuotaSecondaryDraft(event.target.value)}
                  placeholder={t("留空使用计划模板")}
                />
              </div>
            </div>
            <div className="grid gap-3 rounded-xl bg-muted/20 px-3 py-3 text-[11px] text-muted-foreground sm:grid-cols-2">
              <div className="space-y-1">
                <div>{t("账号 ID")}</div>
                <div className="break-all font-mono">
                  {accountEditorState?.accountId || "-"}
                </div>
              </div>
              <div className="space-y-1">
                <div>{t("账号类型")}</div>
                <div className="font-medium text-foreground/80">
                  {currentEditingAccount
                    ? formatAccountPlanLabel(currentEditingAccount, t) || t("未知")
                    : t("未知")}
                </div>
              </div>
            </div>
          </div>
          <DialogFooter className="mx-0 mb-0 gap-2 rounded-b-xl border-t bg-muted/40 px-6 py-4 sm:gap-2">
            <DialogClose
              className={buttonVariants({ variant: "outline" })}
              type="button"
              disabled={Boolean(isUpdatingProfileAccountId)}
            >
              {t("取消")}
            </DialogClose>
            <Button
              disabled={Boolean(isUpdatingProfileAccountId)}
              onClick={() => void handleConfirmAccountEditor()}
            >
              {t("保存")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
