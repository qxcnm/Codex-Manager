"use client";

import { useMemo, useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertCircle,
  BarChart3,
  KeyRound,
  LineChart as LineChartIcon,
  Pencil,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
  UserPlus,
  UsersRound,
  WalletCards,
} from "lucide-react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  XAxis,
  YAxis,
} from "recharts";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Empty, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { APP_SESSION_QUERY_KEY } from "@/hooks/useAppSession";
import { usePageTransitionReady } from "@/hooks/usePageTransitionReady";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { appClient } from "@/lib/api/app-client";
import { dashboardClient } from "@/lib/api/dashboard-client";
import { getAppErrorMessage } from "@/lib/api/transport";
import { estimateChartYAxisWidth } from "@/lib/dashboard/format";
import type { AppLocale } from "@/lib/i18n/config";
import { useI18n } from "@/lib/i18n/provider";
import { cn } from "@/lib/utils";
import { formatCompactNumber } from "@/lib/utils/usage";
import type { AccountManagerStatus, AppUser, MemberDashboardSummary } from "@/types";

const ACCOUNT_MANAGER_QUERY_KEYS = {
  status: ["account-manager", "status"] as const,
  users: ["account-manager", "users"] as const,
};

const CREDIT_MICROS_PER_USD = 1_000_000;

const SUPPORTED_INTL_LOCALES = ["zh-CN", "en-US", "ru-RU", "ko-KR"] as const;

const INTL_LOCALE_BY_APP_LOCALE: Record<
  "cn" | Exclude<AppLocale, "zh-CN">,
  string
> = {
  cn: "zh-CN",
  en: "en-US",
  ru: "ru-RU",
  ko: "ko-KR",
};

function intlLocaleFromAppLocale(locale: string): string {
  if (
    SUPPORTED_INTL_LOCALES.includes(
      locale as (typeof SUPPORTED_INTL_LOCALES)[number],
    )
  ) {
    return locale;
  }
  return (
    INTL_LOCALE_BY_APP_LOCALE[
      locale === "zh-CN" ? "cn" : (locale as Exclude<AppLocale, "zh-CN">)
    ] ?? "zh-CN"
  );
}

function formatCreditMicros(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? value : 0;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(normalized / CREDIT_MICROS_PER_USD);
}

function formatUsd(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(normalized);
}

function formatTokenAmount(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
  if (normalized < 1000) {
    return normalized.toLocaleString("zh-CN");
  }
  return formatCompactNumber(normalized, "0.00", 2, true);
}

function parseCreditInput(value: string): number | null {
  const normalized = Number(String(value || "").trim());
  if (!Number.isFinite(normalized) || normalized < 0) {
    return null;
  }
  return Math.round(normalized * CREDIT_MICROS_PER_USD);
}

function formatTime(
  value: number | null | undefined,
  t: (message: string) => string,
  locale: AppLocale,
): string {
  if (!value) return t("从未");
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return t("未知");
  return date.toLocaleString(intlLocaleFromAppLocale(locale), {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatShortDate(value: number | null | undefined, locale: AppLocale): string {
  if (!value) return "--";
  const date = new Date(value * 1000);
  if (Number.isNaN(date.getTime())) return "--";
  return date.toLocaleDateString(intlLocaleFromAppLocale(locale), {
    month: "2-digit",
    day: "2-digit",
  });
}

function modeLabel(mode: string, t: (message: string) => string): string {
  switch (mode) {
    case "accounts":
      return t("账号登录");
    case "password":
      return t("共享密码");
    case "none":
      return t("未开启");
    default:
      return mode || t("未知");
  }
}

function roleLabel(role: string, t: (message: string) => string): string {
  return role === "admin" ? t("管理员") : t("成员");
}

function isAdminUser(user: AppUser): boolean {
  return user.role === "admin";
}

function userCanOwnWallet(user: AppUser): boolean {
  return !isAdminUser(user);
}

function statusLabel(status: string, t: (message: string) => string): string {
  if (status === "disabled") return t("禁用");
  return status === "active" ? t("启用") : status || t("未知");
}

function userSelectLabel(
  user: AppUser | null | undefined,
  t: (message: string) => string,
): string {
  if (!user) return t("选择可分发成员");
  return user.displayName ? `${user.displayName} (${user.username})` : user.username;
}

function StatCard({
  title,
  value,
  detail,
  icon: Icon,
}: {
  title: string;
  value: string;
  detail: string;
  icon: typeof ShieldCheck;
}) {
  return (
    <Card className="glass-card shadow-sm">
      <CardHeader className="flex flex-row items-start justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardDescription>{title}</CardDescription>
          <CardTitle className="text-2xl">{value}</CardTitle>
        </div>
        <div className="rounded-lg bg-primary/10 p-2 text-primary">
          <Icon className="h-4 w-4" />
        </div>
      </CardHeader>
      <CardContent>
        <p className="text-xs text-muted-foreground">{detail}</p>
      </CardContent>
    </Card>
  );
}

function UserUsageTrendLine({ summary }: { summary: MemberDashboardSummary }) {
  const { locale } = useI18n();
  const chartConfig = {
    totalTokens: {
      label: "Token",
      color: "var(--primary)",
    },
  } satisfies ChartConfig;
  const chartData = summary.usageTrend7d.map((item) => ({
    date: formatShortDate(item.dayStartTs, locale),
    totalTokens: item.totalTokens,
    estimatedCostUsd: item.estimatedCostUsd,
  }));
  const yAxisWidth = estimateChartYAxisWidth(
    [0, ...chartData.map((item) => item.totalTokens)],
    formatTokenAmount,
  );

  return (
    <ChartContainer
      config={chartConfig}
      className="h-56 w-full rounded-xl bg-background/30 p-3"
      initialDimension={{ width: 640, height: 224 }}
    >
      <AreaChart
        accessibilityLayer
        data={chartData}
        margin={{ top: 18, right: 14, left: 10, bottom: 4 }}
      >
        <defs>
          <linearGradient id="memberFillTotalTokens" x1="0" y1="0" x2="0" y2="1">
            <stop
              offset="5%"
              stopColor="var(--color-totalTokens)"
              stopOpacity={0.32}
            />
            <stop
              offset="95%"
              stopColor="var(--color-totalTokens)"
              stopOpacity={0.03}
            />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} strokeDasharray="4 8" />
        <XAxis
          dataKey="date"
          tickLine={false}
          axisLine={false}
          tickMargin={10}
          minTickGap={18}
        />
        <YAxis
          tickLine={false}
          axisLine={false}
          tickMargin={10}
          width={yAxisWidth}
          tickFormatter={(value) => formatTokenAmount(Number(value))}
        />
        <ChartTooltip
          cursor={false}
          content={
            <ChartTooltipContent
              indicator="line"
              labelFormatter={(value) => value}
              formatter={(value, name, item) => {
                const row = item.payload as {
                  estimatedCostUsd?: number;
                };
                return (
                  <div className="grid min-w-36 gap-1">
                    <div className="flex items-center justify-between gap-3">
                      <span className="text-muted-foreground">{String(name)}</span>
                      <span className="font-mono font-medium text-foreground">
                        {formatTokenAmount(Number(value))}
                      </span>
                    </div>
                    <div className="flex items-center justify-between gap-3 text-muted-foreground">
                      <span>Cost</span>
                      <span>{formatUsd(row.estimatedCostUsd)}</span>
                    </div>
                  </div>
                );
              }}
            />
          }
        />
        <Area
          dataKey="totalTokens"
          type="monotone"
          fill="url(#memberFillTotalTokens)"
          stroke="var(--color-totalTokens)"
          strokeWidth={3}
          dot={{ r: 4, strokeWidth: 2, fill: "var(--background)" }}
          activeDot={{ r: 6, strokeWidth: 2 }}
        />
      </AreaChart>
    </ChartContainer>
  );
}

function UserUsageDetail({
  user,
  summary,
}: {
  user: AppUser;
  summary: MemberDashboardSummary;
}) {
  const { t, locale } = useI18n();
  const successRate =
    summary.usageToday.successRate == null
      ? "--"
      : `${Math.round(summary.usageToday.successRate * 100)}%`;
  return (
    <div className="grid gap-4">
      <div className="grid gap-3 sm:grid-cols-3">
        <div className="rounded-xl bg-background/35 p-3">
          <div className="text-xs text-muted-foreground">{t("可用额度")}</div>
          <div className="mt-1 text-lg font-semibold">
            {formatCreditMicros(summary.wallet?.availableCreditMicros ?? user.wallet?.availableCreditMicros)}
          </div>
        </div>
        <div className="rounded-xl bg-background/35 p-3">
          <div className="text-xs text-muted-foreground">{t("今日 Token")}</div>
          <div className="mt-1 text-lg font-semibold">
            {formatTokenAmount(summary.usageToday.totalTokens)}
          </div>
        </div>
        <div className="rounded-xl bg-background/35 p-3">
          <div className="text-xs text-muted-foreground">{t("成功率 / 费用")}</div>
          <div className="mt-1 text-lg font-semibold">
            {successRate} · {formatUsd(summary.usageToday.estimatedCostUsd)}
          </div>
        </div>
      </div>

      <div>
        <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
          <LineChartIcon className="h-4 w-4 text-primary" />
          {t("Token 消耗曲线")}
        </div>
        <UserUsageTrendLine summary={summary} />
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <div className="rounded-xl bg-background/25 p-3">
          <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
            <KeyRound className="h-4 w-4 text-primary" />
            {t("Key 消耗明细")}
          </div>
          <div className="space-y-2">
            {summary.topKeys.length === 0 ? (
              <Empty className="min-h-20 border bg-muted/20 p-3">
                <EmptyHeader>
                  <EmptyTitle>{t("暂无 Key 用量")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              summary.topKeys.map((item) => (
                <div
                  key={item.keyId}
                  className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 text-xs"
                >
                  <div className="min-w-0">
                    <div className="truncate font-medium">{item.name || item.keyId}</div>
                    <div className="truncate text-muted-foreground">{item.modelSlug || "auto"}</div>
                  </div>
                  <div className="text-right font-semibold">
                    {formatTokenAmount(item.todayTokens || item.totalTokens)}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
        <div className="rounded-xl bg-background/25 p-3">
          <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
            <BarChart3 className="h-4 w-4 text-primary" />
            {t("模型消耗明细")}
          </div>
          <div className="space-y-2">
            {summary.topModels.length === 0 ? (
              <Empty className="min-h-20 border bg-muted/20 p-3">
                <EmptyHeader>
                  <EmptyTitle>{t("暂无模型用量")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            ) : (
              summary.topModels.map((item) => (
                <div
                  key={item.model}
                  className="grid grid-cols-[minmax(0,1fr)_auto] gap-3 text-xs"
                >
                  <div className="truncate font-mono font-medium">{item.model}</div>
                  <div className="text-right font-semibold">
                    {formatTokenAmount(item.totalTokens)}
                  </div>
                </div>
              ))
            )}
          </div>
        </div>
      </div>

      <div className="rounded-xl bg-background/25 p-3">
        <div className="mb-2 text-sm font-semibold">{t("近期请求上下文")}</div>
        {summary.recentLogs.length === 0 ? (
          <Empty className="min-h-20 border bg-muted/20 p-3">
            <EmptyHeader>
              <EmptyTitle>{t("暂无请求日志")}</EmptyTitle>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="divide-y divide-border/40">
            {summary.recentLogs.slice(0, 5).map((log) => (
              <div
                key={log.id}
                className="grid gap-2 py-2 text-xs sm:grid-cols-[minmax(0,1fr)_auto]"
              >
                <div className="min-w-0">
                  <div className="truncate font-mono font-medium">{log.model || "unknown"}</div>
                  <div className="truncate text-muted-foreground">{formatTime(log.createdAt, t, locale)}</div>
                </div>
                <div className="flex gap-3 text-muted-foreground sm:justify-end">
                  <span>{log.statusCode || "-"}</span>
                  <span>{formatTokenAmount(log.totalTokens)}</span>
                  <span>{formatUsd(log.estimatedCostUsd)}</span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default function AccountManagerPage() {
  const { t, locale } = useI18n();
  const queryClient = useQueryClient();
  const { canAccessManagementRpc } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/account-manager/");
  const shouldQuery =
    useDeferredDesktopActivation(canAccessManagementRpc) && isPageActive;
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [topUpUserId, setTopUpUserId] = useState<string | null>(null);
  const [editUserId, setEditUserId] = useState<string | null>(null);
  const [usageUserId, setUsageUserId] = useState<string | null>(null);
  const [deleteUserId, setDeleteUserId] = useState<string | null>(null);
  const [createDraft, setCreateDraft] = useState({
    username: "",
    displayName: "",
    password: "",
    role: "member",
    initialBalance: "0",
  });
  const [topUpDraft, setTopUpDraft] = useState({
    amount: "0",
    note: "",
  });
  const [editDraft, setEditDraft] = useState({
    displayName: "",
    role: "member",
    status: "active",
    password: "",
  });

  const statusQuery = useQuery<AccountManagerStatus>({
    queryKey: ACCOUNT_MANAGER_QUERY_KEYS.status,
    queryFn: () => appClient.getAccountManagerStatus(),
    enabled: shouldQuery,
  });
  const usersQuery = useQuery<AppUser[]>({
    queryKey: ACCOUNT_MANAGER_QUERY_KEYS.users,
    queryFn: () => appClient.listAppUsers(),
    enabled: shouldQuery,
  });
  const usageDetailQuery = useQuery<MemberDashboardSummary>({
    queryKey: ["account-manager", "user-usage", usageUserId],
    queryFn: () => dashboardClient.getMemberSummary({ userId: usageUserId }),
    enabled: shouldQuery && Boolean(usageUserId),
    retry: 1,
  });

  usePageTransitionReady(
    "/account-manager/",
    !canAccessManagementRpc ||
      statusQuery.isFetched ||
      statusQuery.isError ||
      !isPageActive,
  );

  const users = usersQuery.data ?? [];
  const usersById = useMemo(
    () => new Map(users.map((user) => [user.id, user])),
    [users],
  );
  const walletUsers = useMemo(
    () => users.filter((user) => userCanOwnWallet(user)),
    [users],
  );
  const status = statusQuery.data;
  const topUpUser = topUpUserId ? usersById.get(topUpUserId) ?? null : null;
  const editUser = editUserId ? usersById.get(editUserId) ?? null : null;
  const usageUser = usageUserId ? usersById.get(usageUserId) ?? null : null;
  const deleteUser = deleteUserId ? usersById.get(deleteUserId) ?? null : null;

  const refreshAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: ACCOUNT_MANAGER_QUERY_KEYS.status,
      }),
      queryClient.invalidateQueries({
        queryKey: ACCOUNT_MANAGER_QUERY_KEYS.users,
      }),
      queryClient.invalidateQueries({
        queryKey: ["account-manager", "user-usage"],
      }),
      queryClient.invalidateQueries({
        queryKey: ["account-manager", "api-key-owners"],
      }),
      queryClient.invalidateQueries({ queryKey: APP_SESSION_QUERY_KEY }),
    ]);
  };

  const createUser = useMutation({
    mutationFn: async () => {
      const username = createDraft.username.trim();
      const password = createDraft.password;
      if (!username) throw new Error(t("请输入用户名"));
      if (!password) throw new Error(t("请输入初始密码"));
      const creatingAdmin = createDraft.role === "admin";
      const initialBalanceCreditMicros = creatingAdmin
        ? null
        : parseCreditInput(createDraft.initialBalance);
      if (!creatingAdmin && initialBalanceCreditMicros === null) {
        throw new Error(t("初始额度必须是非负数字"));
      }
      return appClient.createAppUser({
        username,
        password,
        displayName: createDraft.displayName.trim() || null,
        role: createDraft.role,
        initialBalanceCreditMicros,
      });
    },
    onSuccess: async () => {
      setCreateDraft({
        username: "",
        displayName: "",
        password: "",
        role: "member",
        initialBalance: "0",
      });
      setCreateDialogOpen(false);
      await refreshAll();
      toast.success(t("账号已创建"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("创建失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const setWalletAvailable = useMutation({
    mutationFn: async () => {
      if (!topUpUser || !userCanOwnWallet(topUpUser)) {
        throw new Error(t("请选择可分发成员"));
      }
      const amountCreditMicros = parseCreditInput(topUpDraft.amount);
      if (amountCreditMicros === null) {
        throw new Error(t("可用额度必须是非负数字"));
      }
      return appClient.setWalletAvailable({
        ownerKind: "user",
        ownerId: topUpUser.id,
        availableCreditMicros: amountCreditMicros,
        note: topUpDraft.note.trim() || null,
      });
    },
    onSuccess: async () => {
      setTopUpUserId(null);
      setTopUpDraft({ amount: "0", note: "" });
      await refreshAll();
      toast.success(t("可用额度已更新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("额度更新失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const updateUser = useMutation({
    mutationFn: async () => {
      if (!editUser) throw new Error(t("请选择要编辑的账号"));
      const password = editDraft.password.trim();
      return appClient.updateAppUser({
        id: editUser.id,
        displayName: editDraft.displayName.trim() || null,
        role: editDraft.role,
        status: editDraft.status,
        password: password || null,
      });
    },
    onSuccess: async () => {
      setEditUserId(null);
      setEditDraft({
        displayName: "",
        role: "member",
        status: "active",
        password: "",
      });
      await refreshAll();
      toast.success(t("账号已更新"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("更新失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const deleteUserMutation = useMutation({
    mutationFn: async () => {
      if (!deleteUser) throw new Error(t("请选择要删除的账号"));
      await appClient.deleteAppUser(deleteUser.id);
    },
    onSuccess: async () => {
      setDeleteUserId(null);
      await refreshAll();
      toast.success(t("账号已删除"));
    },
    onError: (error: unknown) => {
      toast.error(`${t("删除失败")}: ${getAppErrorMessage(error)}`);
    },
  });

  const handleCreateUser = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    createUser.mutate();
  };

  const handleTopUp = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setWalletAvailable.mutate();
  };

  const handleUpdateUser = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    updateUser.mutate();
  };

  const openTopUpDialog = (user: AppUser) => {
    setTopUpUserId(user.id);
    setTopUpDraft({
      amount: String(
        (user.wallet?.availableCreditMicros ?? 0) / CREDIT_MICROS_PER_USD,
      ),
      note: "",
    });
  };

  const openEditDialog = (user: AppUser) => {
    setEditUserId(user.id);
    setEditDraft({
      displayName: user.displayName || "",
      role: user.role === "admin" ? "admin" : "member",
      status: user.status === "disabled" ? "disabled" : "active",
      password: "",
    });
  };

  const isRefreshing = statusQuery.isFetching || usersQuery.isFetching;

  return (
    <div className="container mx-auto space-y-6 p-6">
      <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
        <div>
          <h1 className="text-3xl font-bold tracking-tight">{t("账号管理")}</h1>
          <p className="mt-2 text-sm text-muted-foreground">
            {t("管理 Web 登录成员和额度分发钱包。平台 Key 归属在平台密钥中配置。")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            className="glass-card h-10 gap-2 rounded-xl px-3 shadow-sm"
            disabled={!canAccessManagementRpc || isRefreshing}
            onClick={() => void refreshAll()}
          >
            <RefreshCw className={cn("h-4 w-4", isRefreshing && "animate-spin")} />
            {t("刷新")}
          </Button>
          <Button
            className="h-10 gap-2 shadow-sm shadow-primary/20"
            disabled={!canAccessManagementRpc}
            onClick={() => setCreateDialogOpen(true)}
          >
            <Plus className="h-4 w-4" />
            {t("新建账号")}
          </Button>
        </div>
      </div>

      {!canAccessManagementRpc ? (
        <Card className="glass-card shadow-sm">
          <CardContent className="flex items-start gap-3 py-5">
            <AlertCircle className="mt-0.5 h-5 w-5 text-destructive" />
            <div className="space-y-1">
              <p className="font-medium">{t("当前 Web 运行方式不受支持")}</p>
              <p className="text-sm text-muted-foreground">
                {t("请通过 CodexManager Web 运行壳访问账号管理。")}
              </p>
            </div>
          </CardContent>
        </Card>
      ) : null}

      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
        <StatCard
          title={t("登录模式")}
          value={modeLabel(status?.mode || "none", t)}
          detail={
            status?.appUsersConfigured
              ? t("账号系统已初始化")
              : t("账号系统未初始化")
          }
          icon={ShieldCheck}
        />
        <StatCard
          title={t("额度分发")}
          value={status?.distributionEnabled ? t("已开启") : t("未开启")}
          detail={t("开启后平台 Key 会按归属钱包扣减额度")}
          icon={WalletCards}
        />
        <StatCard
          title={t("管理员")}
          value={String(status?.activeAdminCount ?? 0)}
          detail={t("当前启用的管理员账号数量")}
          icon={UsersRound}
        />
        <StatCard
          title={t("可分发成员")}
          value={String(walletUsers.length)}
          detail={t("不包含管理员账号")}
          icon={UserPlus}
        />
      </div>

      <Card className="glass-card overflow-hidden py-0 shadow-sm">
        <CardHeader className="border-b bg-background/35 py-4">
          <CardTitle>{t("登录账号")}</CardTitle>
          <CardDescription>
            {t("管理员只负责控制面管理；成员才参与额度分发和平台 Key 消费。")}
          </CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="px-4">{t("用户名")}</TableHead>
                <TableHead>{t("角色")}</TableHead>
                <TableHead>{t("状态")}</TableHead>
                <TableHead>{t("可用额度")}</TableHead>
                <TableHead>{t("最后登录")}</TableHead>
                <TableHead>{t("账号 ID")}</TableHead>
                <TableHead className="pr-4 text-right">{t("操作")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {usersQuery.isLoading ? (
                <TableRow>
                  <TableCell colSpan={7} className="h-24 text-center">
                    {t("读取中...")}
                  </TableCell>
                </TableRow>
              ) : users.length === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={7}
                    className="h-24 text-center text-muted-foreground"
                  >
                    <Empty className="min-h-20 border-0 bg-transparent">
                      <EmptyHeader>
                        <EmptyTitle>{t("暂无登录账号")}</EmptyTitle>
                      </EmptyHeader>
                    </Empty>
                  </TableCell>
                </TableRow>
              ) : (
                users.map((user) => (
                  <TableRow key={user.id}>
                    <TableCell className="px-4">
                      <div className="flex flex-col gap-1">
                        <span className="font-medium">{user.username}</span>
                        {user.displayName ? (
                          <span className="text-xs text-muted-foreground">
                            {user.displayName}
                          </span>
                        ) : null}
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge
                        variant={user.role === "admin" ? "default" : "secondary"}
                      >
                        {roleLabel(user.role, t)}
                      </Badge>
                    </TableCell>
                    <TableCell>{statusLabel(user.status, t)}</TableCell>
                    <TableCell>
                      {isAdminUser(user) ? (
                        <Badge variant="outline">{t("不参与分发")}</Badge>
                      ) : (
                        formatCreditMicros(user.wallet?.availableCreditMicros)
                      )}
                    </TableCell>
                    <TableCell>{formatTime(user.lastLoginAt, t, locale)}</TableCell>
                    <TableCell className="max-w-[180px] truncate font-mono text-xs text-muted-foreground">
                      {user.id}
                    </TableCell>
                    <TableCell className="pr-4 text-right">
                      <div className="flex flex-wrap justify-end gap-2">
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={!canAccessManagementRpc || !userCanOwnWallet(user)}
                          onClick={() => openTopUpDialog(user)}
                        >
                          {t("额度")}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="gap-1"
                          disabled={!canAccessManagementRpc}
                          onClick={() => setUsageUserId(user.id)}
                        >
                          <LineChartIcon className="h-3.5 w-3.5" />
                          {t("用量")}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="gap-1"
                          disabled={!canAccessManagementRpc}
                          onClick={() => openEditDialog(user)}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                          {t("编辑")}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="gap-1 text-destructive hover:text-destructive"
                          disabled={!canAccessManagementRpc}
                          onClick={() => setDeleteUserId(user.id)}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                          {t("删除")}
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent className="glass-card sm:max-w-[560px]">
          <DialogHeader>
            <DialogTitle>{t("新建登录账号")}</DialogTitle>
            <DialogDescription>
              {t("成员账号可拥有额度钱包；管理员账号只负责管理。")}
            </DialogDescription>
          </DialogHeader>
          <form className="grid gap-4" onSubmit={handleCreateUser}>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-1.5">
                <Label htmlFor="app-user-username">{t("用户名")}</Label>
                <Input
                  id="app-user-username"
                  value={createDraft.username}
                  onChange={(event) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      username: event.target.value,
                    }))
                  }
                  placeholder="member-one"
                />
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="app-user-display-name">{t("显示名称")}</Label>
                <Input
                  id="app-user-display-name"
                  value={createDraft.displayName}
                  onChange={(event) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      displayName: event.target.value,
                    }))
                  }
                  placeholder="Member One"
                />
              </div>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="app-user-password">{t("初始密码")}</Label>
              <Input
                id="app-user-password"
                type="password"
                value={createDraft.password}
                onChange={(event) =>
                  setCreateDraft((draft) => ({
                    ...draft,
                    password: event.target.value,
                  }))
                }
                placeholder="password123"
              />
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-1.5">
                <Label>{t("角色")}</Label>
                <Select
                  value={createDraft.role}
                  onValueChange={(value) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      role: String(value),
                    }))
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue>
                      {(value) => roleLabel(String(value || "member"), t)}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="member">{t("成员")}</SelectItem>
                    <SelectItem value="admin">{t("管理员")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="app-user-initial-balance">{t("初始额度")}</Label>
                <Input
                  id="app-user-initial-balance"
                  inputMode="decimal"
                  value={createDraft.initialBalance}
                  onChange={(event) =>
                    setCreateDraft((draft) => ({
                      ...draft,
                      initialBalance: event.target.value,
                    }))
                  }
                  disabled={createDraft.role === "admin"}
                  placeholder={
                    createDraft.role === "admin" ? t("管理员不设置额度") : "0"
                  }
                />
              </div>
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setCreateDialogOpen(false)}
              >
                {t("取消")}
              </Button>
              <Button
                type="submit"
                className="gap-2"
                disabled={!canAccessManagementRpc || createUser.isPending}
              >
                <UserPlus className="h-4 w-4" />
                {createUser.isPending ? t("创建中...") : t("创建账号")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(editUserId)}
        onOpenChange={(open) => {
          if (!open) {
            setEditUserId(null);
            setEditDraft({
              displayName: "",
              role: "member",
              status: "active",
              password: "",
            });
          }
        }}
      >
        <DialogContent className="glass-card sm:max-w-[560px]">
          <DialogHeader>
            <DialogTitle>{t("编辑登录账号")}</DialogTitle>
            <DialogDescription>
              {editUser ? userSelectLabel(editUser, t) : t("选择登录账号")}
            </DialogDescription>
          </DialogHeader>
          <form className="grid gap-4" onSubmit={handleUpdateUser}>
            <div className="grid gap-1.5">
              <Label htmlFor="edit-app-user-display-name">{t("显示名称")}</Label>
              <Input
                id="edit-app-user-display-name"
                value={editDraft.displayName}
                onChange={(event) =>
                  setEditDraft((draft) => ({
                    ...draft,
                    displayName: event.target.value,
                  }))
                }
                placeholder={t("可选")}
              />
            </div>
            <div className="grid gap-3 sm:grid-cols-2">
              <div className="grid gap-1.5">
                <Label>{t("角色")}</Label>
                <Select
                  value={editDraft.role}
                  onValueChange={(value) =>
                    setEditDraft((draft) => ({
                      ...draft,
                      role: String(value || "member"),
                    }))
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue>
                      {(value) => roleLabel(String(value || "member"), t)}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="member">{t("成员")}</SelectItem>
                    <SelectItem value="admin">{t("管理员")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label>{t("状态")}</Label>
                <Select
                  value={editDraft.status}
                  onValueChange={(value) =>
                    setEditDraft((draft) => ({
                      ...draft,
                      status: String(value || "active"),
                    }))
                  }
                >
                  <SelectTrigger className="w-full">
                    <SelectValue>
                      {(value) => statusLabel(String(value || "active"), t)}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                    <SelectItem value="active">{t("启用")}</SelectItem>
                    <SelectItem value="disabled">{t("禁用")}</SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </div>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="edit-app-user-password">{t("重置密码")}</Label>
              <Input
                id="edit-app-user-password"
                type="password"
                value={editDraft.password}
                onChange={(event) =>
                  setEditDraft((draft) => ({
                    ...draft,
                    password: event.target.value,
                  }))
                }
                placeholder={t("不修改则留空")}
              />
              <p className="text-xs text-muted-foreground">
                {t("填写后会替换该账号的登录密码，至少 8 位。")}
              </p>
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setEditUserId(null)}
              >
                {t("取消")}
              </Button>
              <Button
                type="submit"
                className="gap-2"
                disabled={!canAccessManagementRpc || !editUser || updateUser.isPending}
              >
                <Pencil className="h-4 w-4" />
                {updateUser.isPending ? t("保存中...") : t("保存修改")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(usageUserId)}
        onOpenChange={(open) => {
          if (!open) {
            setUsageUserId(null);
          }
        }}
      >
        <DialogContent className="glass-card max-h-[85vh] overflow-y-auto sm:max-w-[760px]">
          <DialogHeader>
            <DialogTitle>{t("成员用量详情")}</DialogTitle>
            <DialogDescription>
              {usageUser ? userSelectLabel(usageUser, t) : t("选择登录账号")}
            </DialogDescription>
          </DialogHeader>
          {!usageUser ? (
            <div className="rounded-xl bg-background/35 p-4 text-sm text-muted-foreground">
              {t("未找到登录账号")}
            </div>
          ) : usageDetailQuery.isLoading ? (
            <div className="grid gap-3">
              <Skeleton className="h-20 w-full rounded-xl" />
              <Skeleton className="h-44 w-full rounded-xl" />
              <Skeleton className="h-28 w-full rounded-xl" />
            </div>
          ) : usageDetailQuery.isError ? (
            <div className="rounded-xl border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive">
              {t("用量详情读取失败")}
            </div>
          ) : usageDetailQuery.data ? (
            <UserUsageDetail user={usageUser} summary={usageDetailQuery.data} />
          ) : (
            <Empty className="min-h-28 border bg-background/35">
              <EmptyHeader>
                <EmptyTitle>{t("暂无用量详情")}</EmptyTitle>
              </EmptyHeader>
            </Empty>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setUsageUserId(null)}
            >
              {t("关闭")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(deleteUserId)}
        onOpenChange={(open) => {
          if (!open) {
            setDeleteUserId(null);
          }
        }}
      >
        <DialogContent className="glass-card sm:max-w-[460px]">
          <DialogHeader>
            <DialogTitle>{t("删除登录账号")}</DialogTitle>
            <DialogDescription>
              {deleteUser
                ? `${t("确认删除")}：${userSelectLabel(deleteUser, t)}`
                : t("选择登录账号")}
            </DialogDescription>
          </DialogHeader>
          <div className="rounded-xl border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive">
            {t("删除后会移除该账号会话、钱包、额度流水和平台 Key 归属，操作不可恢复。")}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setDeleteUserId(null)}
            >
              {t("取消")}
            </Button>
            <Button
              type="button"
              variant="destructive"
              className="gap-2"
              disabled={!canAccessManagementRpc || !deleteUser || deleteUserMutation.isPending}
              onClick={() => deleteUserMutation.mutate()}
            >
              <Trash2 className="h-4 w-4" />
              {deleteUserMutation.isPending ? t("删除中...") : t("确认删除")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={Boolean(topUpUserId)}
        onOpenChange={(open) => {
          if (!open) {
            setTopUpUserId(null);
            setTopUpDraft({ amount: "0", note: "" });
          }
        }}
      >
        <DialogContent className="glass-card sm:max-w-[480px]">
          <DialogHeader>
            <DialogTitle>{t("修改可用额度")}</DialogTitle>
            <DialogDescription>
              {topUpUser
                ? `${t("目标账号")}：${userSelectLabel(topUpUser, t)}`
                : t("选择可分发成员")}
            </DialogDescription>
          </DialogHeader>
          <form className="grid gap-4" onSubmit={handleTopUp}>
            <div className="grid gap-1.5">
              <Label htmlFor="wallet-top-up-amount">{t("可用额度")}</Label>
              <Input
                id="wallet-top-up-amount"
                inputMode="decimal"
                value={topUpDraft.amount}
                onChange={(event) =>
                  setTopUpDraft((draft) => ({
                    ...draft,
                    amount: event.target.value,
                  }))
                }
                placeholder="0"
              />
              <p className="text-xs text-muted-foreground">
                {t("这里会直接设置该成员钱包的可用额度，不是追加充值。")}
              </p>
            </div>
            <div className="grid gap-1.5">
              <Label htmlFor="wallet-top-up-note">{t("备注")}</Label>
              <Input
                id="wallet-top-up-note"
                value={topUpDraft.note}
                onChange={(event) =>
                  setTopUpDraft((draft) => ({
                    ...draft,
                    note: event.target.value,
                  }))
                }
                placeholder={t("可选")}
              />
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setTopUpUserId(null)}
              >
                {t("取消")}
              </Button>
              <Button
                type="submit"
                className="gap-2"
                disabled={
                  !canAccessManagementRpc ||
                  !topUpUser ||
                  setWalletAvailable.isPending
                }
              >
                <WalletCards className="h-4 w-4" />
                {setWalletAvailable.isPending ? t("保存中...") : t("保存额度")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
}
