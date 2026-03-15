"use client";

import { useMemo, useState } from "react";
import {
  BarChart3,
  ExternalLink,
  FolderOpen,
  MoreVertical,
  Plus,
  RefreshCw,
  Search,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { toast } from "sonner";
import { AddAccountModal } from "@/components/modals/add-account-modal";
import { AccountConfirmDialog } from "@/components/accounts/account-confirm-dialog";
import { AccountRowActionsMenu } from "@/components/accounts/account-row-actions-menu";
import UsageModal from "@/components/modals/usage-modal";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useAccounts } from "@/hooks/useAccounts";
import { cn } from "@/lib/utils";
import { toStaticRouteHref } from "@/lib/utils/navigation";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import { Account } from "@/types";

type StatusFilter = "all" | "available" | "low_quota";
type PendingAccountAction =
  | { type: "delete-selected"; accountIds: string[] }
  | { type: "delete-unavailable-free" }
  | null;

interface QuotaProgressProps {
  label: string;
  remainPercent: number | null;
  icon: LucideIcon;
}

function QuotaProgress({ label, remainPercent, icon: Icon }: QuotaProgressProps) {
  const value = remainPercent ?? 0;

  return (
    <div className="flex min-w-[120px] flex-col gap-1">
      <div className="flex items-center justify-between text-[10px]">
        <div className="flex items-center gap-1 text-muted-foreground">
          <Icon className="h-3 w-3" />
          <span>{label}</span>
        </div>
        <span className="font-medium">{remainPercent == null ? "--" : `${value}%`}</span>
      </div>
      <Progress value={value} className="h-1.5" />
    </div>
  );
}

export default function AccountsPage() {
  const {
    accounts,
    groups,
    isLoading,
    refreshAccount,
    refreshAllAccounts,
    deleteAccount,
    deleteManyAccounts,
    deleteUnavailableFree,
    importByDirectory,
    isRefreshing,
    isDeleting,
    isDeletingMany,
    isDeletingUnavailableFree,
  } = useAccounts();

  const [search, setSearch] = useState("");
  const [groupFilter, setGroupFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState("20");
  const [page, setPage] = useState(1);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [addAccountModalOpen, setAddAccountModalOpen] = useState(false);
  const [usageModalOpen, setUsageModalOpen] = useState(false);
  const [selectedAccount, setSelectedAccount] = useState<Account | null>(null);
  const [pendingAction, setPendingAction] = useState<PendingAccountAction>(null);

  const filteredAccounts = useMemo(() => {
    return accounts.filter((account) => {
      const matchSearch =
        !search ||
        account.name.toLowerCase().includes(search.toLowerCase()) ||
        account.id.toLowerCase().includes(search.toLowerCase());
      const matchGroup = groupFilter === "all" || (account.group || "默认") === groupFilter;
      const matchStatus =
        statusFilter === "all" ||
        (statusFilter === "available" && account.isAvailable) ||
        (statusFilter === "low_quota" && account.isLowQuota);
      return matchSearch && matchGroup && matchStatus;
    });
  }, [accounts, groupFilter, search, statusFilter]);

  const pageSizeNumber = Number(pageSize) || 20;
  const totalPages = Math.max(1, Math.ceil(filteredAccounts.length / pageSizeNumber));
  const safePage = Math.min(page, totalPages);
  const accountIdSet = useMemo(() => new Set(accounts.map((account) => account.id)), [accounts]);
  const effectiveSelectedIds = useMemo(
    () => selectedIds.filter((id) => accountIdSet.has(id)),
    [accountIdSet, selectedIds]
  );

  const visibleAccounts = useMemo(() => {
    const offset = (safePage - 1) * pageSizeNumber;
    return filteredAccounts.slice(offset, offset + pageSizeNumber);
  }, [filteredAccounts, pageSizeNumber, safePage]);

  const handleSearchChange = (value: string) => {
    setSearch(value);
    setPage(1);
  };

  const handleGroupFilterChange = (value: string | null) => {
    setGroupFilter(value || "all");
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
      current.includes(id) ? current.filter((item) => item !== id) : [...current, id]
    );
  };

  const toggleSelectAllVisible = () => {
    const visibleIds = visibleAccounts.map((account) => account.id);
    const allSelected = visibleIds.every((id) => effectiveSelectedIds.includes(id));
    setSelectedIds((current) => {
      if (allSelected) {
        return current.filter((id) => !visibleIds.includes(id));
      }
      return Array.from(new Set([...current, ...visibleIds]));
    });
  };

  const openUsage = (account: Account) => {
    setSelectedAccount(account);
    setUsageModalOpen(true);
  };

  const handleDeleteSelected = () => {
    if (!effectiveSelectedIds.length) {
      toast.error("请先选择要删除的账号");
      return;
    }
    setPendingAction({
      type: "delete-selected",
      accountIds: effectiveSelectedIds,
    });
  };

  const handleDeleteSingle = (account: Account) => {
    deleteAccount(account.id);
  };

  const handleConfirmPendingAction = () => {
    if (!pendingAction) return;
    if (pendingAction.type === "delete-selected") {
      deleteManyAccounts(pendingAction.accountIds);
      setSelectedIds((current) =>
        current.filter((id) => !pendingAction.accountIds.includes(id))
      );
      setPendingAction(null);
      return;
    }

    deleteUnavailableFree();
    setPendingAction(null);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="flex flex-wrap items-center gap-3">
            <div className="relative w-64">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="搜索账号名 / 编号..."
                className="h-10 bg-card/50 pl-9"
                value={search}
                onChange={(event) => handleSearchChange(event.target.value)}
              />
            </div>
            <Select value={groupFilter} onValueChange={handleGroupFilterChange}>
              <SelectTrigger className="h-10 w-[160px] bg-card/50">
                <SelectValue placeholder="全部分组" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">全部分组 ({accounts.length})</SelectItem>
                {groups.map((group) => (
                  <SelectItem key={group.label} value={group.label}>
                    {group.label} ({group.count})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <div className="flex items-center rounded-lg border bg-muted/30 p-1">
              {[
                { id: "all", label: "全部" },
                { id: "available", label: "可用" },
                { id: "low_quota", label: "低配额" },
              ].map((filter) => (
                <button
                  key={filter.id}
                  onClick={() => handleStatusFilterChange(filter.id as StatusFilter)}
                  className={cn(
                    "rounded-md px-4 py-1.5 text-xs font-medium transition-all",
                    statusFilter === filter.id
                      ? "bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground"
                  )}
                >
                  {filter.label}
                </button>
              ))}
            </div>
          </div>

          <div className="flex items-center gap-2">
            <DropdownMenu>
              <DropdownMenuTrigger
                render={
                  <Button
                    variant="outline"
                    className="h-10 gap-2"
                    render={<span />}
                    nativeButton={false}
                  >
                    账号操作 <MoreVertical className="h-4 w-4" />
                  </Button>
                }
                nativeButton={false}
              />
              <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuItem onClick={() => setAddAccountModalOpen(true)}>
                  <Plus className="mr-2 h-4 w-4" /> 添加账号
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => importByDirectory()}>
                  <FolderOpen className="mr-2 h-4 w-4" /> 按文件夹导入
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={() => refreshAllAccounts()}>
                  <RefreshCw className="mr-2 h-4 w-4" /> 刷新所有账号
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  disabled={!effectiveSelectedIds.length || isDeletingMany}
                  className="text-destructive"
                  onClick={handleDeleteSelected}
                >
                  <Trash2 className="mr-2 h-4 w-4" /> 删除选中账号
                </DropdownMenuItem>
                <DropdownMenuItem
                  onClick={() => setPendingAction({ type: "delete-unavailable-free" })}
                  className="text-destructive"
                >
                  <Trash2 className="mr-2 h-4 w-4" /> 一键清理不可用免费
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <Button
              className="h-10 gap-2 shadow-lg shadow-primary/20"
              onClick={() => refreshAllAccounts()}
              disabled={isRefreshing}
            >
              <RefreshCw className={cn("h-4 w-4", isRefreshing && "animate-spin")} />
              刷新所有
            </Button>
          </div>
        </div>
      </div>

      <Card className="overflow-hidden border-none bg-card/50 shadow-xl backdrop-blur-md">
        <CardContent className="p-0">
          <Table>
            <TableHeader className="bg-muted/30">
              <TableRow>
                <TableHead className="w-12 text-center">
                  <Checkbox
                    checked={
                      visibleAccounts.length > 0 &&
                      visibleAccounts.every((account) => effectiveSelectedIds.includes(account.id))
                    }
                    onCheckedChange={toggleSelectAllVisible}
                  />
                </TableHead>
                <TableHead className="max-w-[220px]">账号信息</TableHead>
                <TableHead>5h 额度</TableHead>
                <TableHead>7d 额度</TableHead>
                <TableHead className="w-20">顺序</TableHead>
                <TableHead>状态</TableHead>
                <TableHead className="text-center">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                Array.from({ length: 5 }).map((_, index) => (
                  <TableRow key={index}>
                    <TableCell><Skeleton className="mx-auto h-4 w-4" /></TableCell>
                    <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                    <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                    <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                    <TableCell><Skeleton className="h-4 w-10" /></TableCell>
                    <TableCell><Skeleton className="h-6 w-16 rounded-full" /></TableCell>
                    <TableCell><Skeleton className="mx-auto h-8 w-24" /></TableCell>
                  </TableRow>
                ))
              ) : visibleAccounts.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={7} className="h-48 text-center">
                    <div className="flex flex-col items-center justify-center gap-2 text-muted-foreground">
                      <Search className="h-8 w-8 opacity-20" />
                      <p>未找到符合条件的账号</p>
                    </div>
                  </TableCell>
                </TableRow>
              ) : (
                visibleAccounts.map((account) => (
                  <TableRow key={account.id} className="group transition-colors hover:bg-muted/30">
                    <TableCell className="text-center">
                      <Checkbox
                        checked={effectiveSelectedIds.includes(account.id)}
                        onCheckedChange={() => toggleSelect(account.id)}
                      />
                    </TableCell>
                    <TableCell className="max-w-[220px]">
                      <div className="flex flex-col overflow-hidden">
                        <div className="flex items-center gap-2 overflow-hidden">
                          <span className="truncate text-sm font-semibold">{account.name}</span>
                          <Badge
                            variant="secondary"
                            className="h-4 shrink-0 bg-accent/50 px-1.5 text-[9px]"
                          >
                            {account.group || "默认"}
                          </Badge>
                        </div>
                        <span className="truncate font-mono text-[10px] uppercase text-muted-foreground opacity-60">
                          {account.id.slice(0, 16)}...
                        </span>
                        <span className="mt-1 text-[10px] text-muted-foreground">
                          最近刷新: {formatTsFromSeconds(account.lastRefreshAt, "从未刷新")}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <QuotaProgress
                        label="5小时"
                        remainPercent={account.primaryRemainPercent}
                        icon={RefreshCw}
                      />
                    </TableCell>
                    <TableCell>
                      <QuotaProgress
                        label="7天"
                        remainPercent={account.secondaryRemainPercent}
                        icon={RefreshCw}
                      />
                    </TableCell>
                    <TableCell>
                      <span className="rounded bg-muted/50 px-2 py-0.5 font-mono text-xs">
                        {account.priority}
                      </span>
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1.5">
                        <div
                          className={cn(
                            "h-1.5 w-1.5 rounded-full",
                            account.availabilityKind === "available"
                              ? "bg-green-500"
                              : account.availabilityKind === "expired"
                                ? "bg-amber-500"
                                : "bg-red-500"
                          )}
                        />
                        <span
                          className={cn(
                            "text-[11px] font-medium",
                            account.availabilityKind === "available"
                              ? "text-green-600 dark:text-green-400"
                              : account.availabilityKind === "expired"
                                ? "text-amber-600 dark:text-amber-400"
                                : "text-red-600 dark:text-red-400"
                          )}
                        >
                          {account.availabilityText}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="table-action-cell gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                          onClick={() => openUsage(account)}
                          title="用量详情"
                        >
                          <BarChart3 className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-muted-foreground transition-colors hover:text-primary"
                          onClick={() => refreshAccount(account.id)}
                          title="立即刷新"
                        >
                          <RefreshCw className={cn("h-4 w-4", isRefreshing && "animate-spin")} />
                        </Button>
                        <AccountRowActionsMenu
                          account={account}
                          onOpenDetails={(current) => {
                            const searchParams = new URLSearchParams({
                              query: current.id,
                            });
                            window.location.assign(toStaticRouteHref("/logs", searchParams));
                          }}
                          onDelete={handleDeleteSingle}
                        />
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <div className="flex items-center justify-between px-2">
        <div className="text-xs text-muted-foreground">
          共 {filteredAccounts.length} 个账号
          {effectiveSelectedIds.length > 0 ? (
            <span className="ml-1 text-primary">(已选择 {effectiveSelectedIds.length} 个)</span>
          ) : null}
        </div>
        <div className="flex items-center gap-6">
          <div className="flex items-center gap-2">
            <span className="whitespace-nowrap text-xs text-muted-foreground">每页显示</span>
            <Select value={pageSize} onValueChange={handlePageSizeChange}>
              <SelectTrigger className="h-8 w-[70px] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {["5", "10", "20", "50", "100", "500"].map((value) => (
                  <SelectItem key={value} value={value}>
                    {value}
                  </SelectItem>
                ))}
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
              上一页
            </Button>
            <div className="min-w-[60px] text-center text-xs font-medium">
              第 {safePage} / {totalPages} 页
            </div>
            <Button
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
              disabled={safePage >= totalPages}
              onClick={() => setPage((current) => Math.min(totalPages, current + 1))}
            >
              下一页
            </Button>
          </div>
        </div>
      </div>

      <AddAccountModal open={addAccountModalOpen} onOpenChange={setAddAccountModalOpen} />
      <UsageModal
        account={selectedAccount}
        open={usageModalOpen}
        onOpenChange={setUsageModalOpen}
        onRefresh={refreshAccount}
        isRefreshing={isRefreshing}
      />
      <AccountConfirmDialog
        open={pendingAction !== null}
        onOpenChange={(open) => {
          if (!open) {
            setPendingAction(null);
          }
        }}
        title={
          pendingAction?.type === "delete-selected"
            ? `确定删除选中的 ${pendingAction.accountIds.length} 个账号吗？`
            : "确定清理不可用免费账号吗？"
        }
        description={
          pendingAction?.type === "delete-selected"
            ? "删除后不可恢复。"
            : "将删除当前不可用且识别为免费计划的账号，此操作不可恢复。"
        }
        confirmLabel={pendingAction?.type === "delete-selected" ? "确认删除" : "确认清理"}
        onConfirm={handleConfirmPendingAction}
        isPending={isDeleting || isDeletingMany || isDeletingUnavailableFree}
      />
    </div>
  );
}
