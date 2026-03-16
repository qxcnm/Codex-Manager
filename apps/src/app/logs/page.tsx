"use client";

import { Suspense, useEffect, useMemo, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  CheckCircle2,
  Database,
  RefreshCw,
  Search,
  Shield,
  Trash2,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { accountClient } from "@/lib/api/account-client";
import { serviceClient } from "@/lib/api/service-client";
import { useAppStore } from "@/lib/store/useAppStore";
import { formatCompactNumber, formatTsFromSeconds } from "@/lib/utils/usage";
import { cn } from "@/lib/utils";
import { RequestLog } from "@/types";

type StatusFilter = "all" | "2xx" | "4xx" | "5xx";

function getStatusBadge(statusCode: number | null) {
  if (statusCode == null) {
    return <Badge variant="secondary">-</Badge>;
  }
  if (statusCode >= 200 && statusCode < 300) {
    return (
      <Badge className="border-green-500/20 bg-green-500/10 text-green-500">
        {statusCode}
      </Badge>
    );
  }
  if (statusCode >= 400 && statusCode < 500) {
    return (
      <Badge className="border-yellow-500/20 bg-yellow-500/10 text-yellow-500">
        {statusCode}
      </Badge>
    );
  }
  return (
    <Badge className="border-red-500/20 bg-red-500/10 text-red-500">
      {statusCode}
    </Badge>
  );
}

function SummaryCard({
  title,
  value,
  description,
  icon: Icon,
  toneClass,
}: {
  title: string;
  value: string;
  description: string;
  icon: LucideIcon;
  toneClass: string;
}) {
  return (
    <Card
      size="sm"
      className="glass-card border-none shadow-sm backdrop-blur-md transition-all hover:-translate-y-0.5"
    >
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-1.5">
        <CardTitle className="text-[13px] font-medium text-muted-foreground">
          {title}
        </CardTitle>
        <div
          className={cn(
            "flex h-8 w-8 items-center justify-center rounded-xl",
            toneClass,
          )}
        >
          <Icon className="h-3.5 w-3.5" />
        </div>
      </CardHeader>
      <CardContent className="space-y-0.5">
        <div className="text-[2rem] leading-none font-semibold tracking-tight">
          {value}
        </div>
        <p className="text-[11px] text-muted-foreground">{description}</p>
      </CardContent>
    </Card>
  );
}

function LogsPageSkeleton() {
  return (
    <div className="space-y-5">
      <Skeleton className="h-28 w-full rounded-3xl" />
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {Array.from({ length: 4 }).map((_, index) => (
          <Skeleton key={index} className="h-32 w-full rounded-3xl" />
        ))}
      </div>
      <Skeleton className="h-[420px] w-full rounded-3xl" />
    </div>
  );
}

function formatDuration(value: number | null): string {
  if (value == null) return "-";
  if (value >= 10_000) return `${Math.round(value / 1000)}s`;
  if (value >= 1000) return `${(value / 1000).toFixed(1).replace(/\.0$/, "")}s`;
  return `${Math.round(value)}ms`;
}

function fallbackAccountNameFromId(accountId: string): string {
  const raw = accountId.trim();
  if (!raw) return "";
  const sep = raw.indexOf("::");
  if (sep < 0) return "";
  return raw.slice(sep + 2).trim();
}

function fallbackAccountDisplayFromKey(keyId: string): string {
  const raw = keyId.trim();
  if (!raw) return "";
  return `Key ${raw.slice(0, 10)}`;
}

function formatCompactKeyLabel(keyId: string): string {
  if (!keyId) return "-";
  if (keyId.length <= 12) return keyId;
  return `${keyId.slice(0, 8)}...`;
}

function resolveDisplayRequestPath(log: RequestLog): string {
  const originalPath = String(log.originalPath || "").trim();
  if (originalPath) {
    return originalPath;
  }
  return String(log.path || log.requestPath || "").trim();
}

function resolveUpstreamDisplay(upstreamUrl: string): string {
  const raw = String(upstreamUrl || "").trim();
  if (!raw) return "";
  if (raw === "默认" || raw === "本地" || raw === "自定义") {
    return raw;
  }
  try {
    const url = new URL(raw);
    const pathname = url.pathname.replace(/\/+$/, "");
    return pathname ? `${url.host}${pathname}` : url.host;
  } catch {
    return raw;
  }
}

function resolveAccountDisplayName(
  log: RequestLog,
  accountNameMap: Map<string, string>,
): string {
  if (log.accountId) {
    const label = accountNameMap.get(log.accountId);
    if (label) {
      return label;
    }
    const fallbackName = fallbackAccountNameFromId(log.accountId);
    if (fallbackName) {
      return fallbackName;
    }
  }
  return fallbackAccountDisplayFromKey(log.keyId);
}

function formatModelEffortDisplay(log: RequestLog): string {
  const model = String(log.model || "").trim();
  const effort = String(log.reasoningEffort || "").trim();
  if (model && effort) {
    return `${model}/${effort}`;
  }
  return model || effort || "-";
}

function AccountKeyInfoCell({
  log,
  accountLabel,
}: {
  log: RequestLog;
  accountLabel: string;
}) {
  const displayAccount = accountLabel || log.accountId || "-";
  const hasNamedAccount =
    Boolean(accountLabel) &&
    accountLabel.trim() !== "" &&
    accountLabel !== log.accountId;

  return (
    <Tooltip>
      <TooltipTrigger render={<div />} className="block text-left">
        <div className="flex flex-col gap-0.5 opacity-80">
          <div className="flex items-center gap-1">
            <Zap className="h-3 w-3 text-yellow-500" />
            <span className="max-w-[140px] truncate">{displayAccount}</span>
          </div>
          <div className="flex items-center gap-1 text-[9px] text-muted-foreground">
            <Shield className="h-2.5 w-2.5" />
            <span className="font-mono">
              {formatCompactKeyLabel(log.keyId)}
            </span>
          </div>
        </div>
      </TooltipTrigger>
      <TooltipContent className="max-w-sm">
        <div className="flex min-w-[240px] flex-col gap-2">
          {hasNamedAccount ? (
            <div className="space-y-0.5">
              <div className="text-[10px] text-background/70">邮箱 / 名称</div>
              <div className="break-all font-mono text-[11px]">
                {accountLabel}
              </div>
            </div>
          ) : null}
          <div className="space-y-0.5">
            <div className="text-[10px] text-background/70">账号 ID</div>
            <div className="break-all font-mono text-[11px]">
              {log.accountId || "-"}
            </div>
          </div>
          <div className="space-y-0.5">
            <div className="text-[10px] text-background/70">密钥</div>
            <div className="break-all font-mono text-[11px]">
              {log.keyId || "-"}
            </div>
          </div>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

function RequestRouteInfoCell({ log }: { log: RequestLog }) {
  const displayPath = resolveDisplayRequestPath(log) || "-";
  const recordedPath = String(log.path || log.requestPath || "").trim();
  const originalPath = String(log.originalPath || "").trim();
  const adaptedPath = String(log.adaptedPath || "").trim();
  const upstreamUrl = String(log.upstreamUrl || "").trim();
  const upstreamDisplay = resolveUpstreamDisplay(upstreamUrl);

  return (
    <Tooltip>
      <TooltipTrigger render={<div />} className="block text-left">
        <div className="flex flex-col gap-0.5">
          <span className="font-bold text-primary">{log.method || "-"}</span>
          <span className="max-w-[200px] truncate text-muted-foreground">
            {displayPath}
          </span>
        </div>
      </TooltipTrigger>
      <TooltipContent className="max-w-md">
        <div className="flex min-w-[280px] flex-col gap-2">
          <div className="space-y-0.5">
            <div className="text-[10px] text-background/70">方法</div>
            <div className="font-mono text-[11px]">{log.method || "-"}</div>
          </div>
          <div className="space-y-0.5">
            <div className="text-[10px] text-background/70">显示地址</div>
            <div className="break-all font-mono text-[11px]">{displayPath}</div>
          </div>
          {recordedPath && recordedPath !== displayPath ? (
            <div className="space-y-0.5">
              <div className="text-[10px] text-background/70">记录地址</div>
              <div className="break-all font-mono text-[11px]">
                {recordedPath}
              </div>
            </div>
          ) : null}
          {originalPath && originalPath !== displayPath ? (
            <div className="space-y-0.5">
              <div className="text-[10px] text-background/70">原始地址</div>
              <div className="break-all font-mono text-[11px]">
                {originalPath}
              </div>
            </div>
          ) : null}
          {adaptedPath && adaptedPath !== displayPath ? (
            <div className="space-y-0.5">
              <div className="text-[10px] text-background/70">转发地址</div>
              <div className="break-all font-mono text-[11px]">
                {adaptedPath}
              </div>
            </div>
          ) : null}
          {log.responseAdapter ? (
            <div className="space-y-0.5">
              <div className="text-[10px] text-background/70">适配器</div>
              <div className="break-all font-mono text-[11px]">
                {log.responseAdapter}
              </div>
            </div>
          ) : null}
          {upstreamDisplay ? (
            <div className="space-y-0.5">
              <div className="text-[10px] text-background/70">上游</div>
              <div className="break-all font-mono text-[11px]">
                {upstreamDisplay}
              </div>
            </div>
          ) : null}
          {upstreamUrl ? (
            <div className="space-y-0.5">
              <div className="text-[10px] text-background/70">上游地址</div>
              <div className="break-all font-mono text-[11px]">
                {upstreamUrl}
              </div>
            </div>
          ) : null}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

function ErrorInfoCell({ error }: { error: string }) {
  const text = String(error || "").trim();
  if (!text) {
    return <span className="text-muted-foreground">-</span>;
  }

  return (
    <Tooltip>
      <TooltipTrigger render={<div />} className="block text-left">
        <span className="block max-w-[220px] truncate font-medium text-red-400">
          {text}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-md">
        <div className="max-w-[360px] break-all font-mono text-[11px]">
          {text}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

function ModelEffortCell({ log }: { log: RequestLog }) {
  const model = String(log.model || "").trim();
  const effort = String(log.reasoningEffort || "").trim();
  const display = formatModelEffortDisplay(log);

  return (
    <Tooltip>
      <TooltipTrigger render={<div />} className="block text-left">
        <span className="block max-w-[160px] truncate font-medium text-foreground">
          {display}
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-sm">
        <div className="flex min-w-[200px] flex-col gap-2">
          <div className="space-y-0.5">
            <div className="text-[10px] text-background/70">模型</div>
            <div className="break-all font-mono text-[11px]">
              {model || "-"}
            </div>
          </div>
          <div className="space-y-0.5">
            <div className="text-[10px] text-background/70">推理</div>
            <div className="break-all font-mono text-[11px]">
              {effort || "-"}
            </div>
          </div>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

function LogsPageContent() {
  const searchParams = useSearchParams();
  const { serviceStatus } = useAppStore();
  const queryClient = useQueryClient();
  const [search, setSearch] = useState(() => searchParams.get("query") || "");
  const [filter, setFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState("10");
  const [page, setPage] = useState(1);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const pageSizeNumber = Number(pageSize) || 10;

  const { data: accountsResult } = useQuery({
    queryKey: ["accounts", "lookup"],
    queryFn: () => accountClient.list(),
    enabled: serviceStatus.connected,
    staleTime: 60_000,
    retry: 1,
  });

  const { data: logsResult, isLoading } = useQuery({
    queryKey: ["logs", "list", search, filter, page, pageSizeNumber],
    queryFn: () =>
      serviceClient.listRequestLogs({
        query: search,
        statusFilter: filter,
        page,
        pageSize: pageSizeNumber,
      }),
    enabled: serviceStatus.connected,
    refetchInterval: 5000,
    retry: 1,
    placeholderData: (previousData) => previousData,
  });

  const { data: summaryResult } = useQuery({
    queryKey: ["logs", "summary", search, filter],
    queryFn: () =>
      serviceClient.getRequestLogSummary({
        query: search,
        statusFilter: filter,
      }),
    enabled: serviceStatus.connected,
    refetchInterval: 5000,
    retry: 1,
    placeholderData: (previousData) => previousData,
  });

  const clearMutation = useMutation({
    mutationFn: () => serviceClient.clearRequestLogs(),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["logs"] }),
        queryClient.invalidateQueries({ queryKey: ["today-summary"] }),
        queryClient.invalidateQueries({ queryKey: ["startup-snapshot"] }),
      ]);
      toast.success("日志已清空");
    },
    onError: (error: unknown) => {
      toast.error(error instanceof Error ? error.message : String(error));
    },
  });

  useEffect(() => {
    if (logsResult && logsResult.page !== page) {
      setPage(logsResult.page);
    }
  }, [logsResult, page]);

  const accountNameMap = useMemo(() => {
    return new Map(
      (accountsResult?.items || []).map((account) => [
        account.id,
        account.label || account.name || account.id,
      ]),
    );
  }, [accountsResult?.items]);

  const logs = logsResult?.items || [];
  const summary = summaryResult || {
    totalCount: logsResult?.total || 0,
    filteredCount: logsResult?.total || 0,
    successCount: 0,
    errorCount: 0,
    totalTokens: 0,
  };
  const totalPages = Math.max(
    1,
    Math.ceil((logsResult?.total || 0) / pageSizeNumber),
  );

  const currentFilterLabel =
    filter === "all"
      ? "全部状态"
      : filter === "2xx"
        ? "成功请求"
        : filter === "4xx"
          ? "客户端错误"
          : "服务端错误";
  const compactMetaText = `${summary.filteredCount}/${summary.totalCount} 条 · ${currentFilterLabel} · ${
    serviceStatus.connected ? "5 秒刷新" : "服务未连接"
  }`;

  return (
    <div className="animate-in space-y-5 fade-in duration-500">
      <Card className="glass-card border-none shadow-md backdrop-blur-md">
        <CardContent className="grid gap-3 pt-0 lg:grid-cols-[minmax(0,1fr)_auto_auto_auto] lg:items-center">
          <div className="relative min-w-0">
            <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="搜索路径、账号或密钥..."
              className="glass-card h-10 rounded-xl pl-10"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
                setPage(1);
              }}
            />
          </div>
          <div className="flex shrink-0 items-center gap-1 rounded-xl border border-border/60 bg-muted/30 p-1">
            {["all", "2xx", "4xx", "5xx"].map((item) => (
              <button
                key={item}
                onClick={() => {
                  setFilter(item as StatusFilter);
                  setPage(1);
                }}
                className={cn(
                  "rounded-lg px-3 py-1.5 text-xs font-semibold uppercase tracking-wide transition-all",
                  filter === item
                    ? "bg-background text-foreground shadow-sm"
                    : "text-muted-foreground hover:bg-background/60 hover:text-foreground",
                )}
              >
                {item.toUpperCase()}
              </button>
            ))}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              className="glass-card h-9 rounded-xl px-3.5"
              onClick={() =>
                queryClient.invalidateQueries({ queryKey: ["logs"] })
              }
            >
              <RefreshCw className="mr-1.5 h-4 w-4" /> 刷新
            </Button>
            <Button
              variant="destructive"
              size="sm"
              className="h-9 rounded-xl px-3.5"
              onClick={() => setClearConfirmOpen(true)}
              disabled={clearMutation.isPending}
            >
              <Trash2 className="mr-1.5 h-4 w-4" /> 清空日志
            </Button>
          </div>
          <div className="text-[11px] text-muted-foreground lg:justify-self-end lg:text-right">
            <span className="font-medium text-foreground">
              {compactMetaText}
            </span>
          </div>
        </CardContent>
      </Card>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <SummaryCard
          title="当前结果"
          value={`${summary.filteredCount}`}
          description={`总日志 ${summary.totalCount} 条`}
          icon={Zap}
          toneClass="bg-primary/12 text-primary"
        />
        <SummaryCard
          title="2XX 成功"
          value={`${summary.successCount}`}
          description="状态码 200-299"
          icon={CheckCircle2}
          toneClass="bg-green-500/12 text-green-500"
        />
        <SummaryCard
          title="异常请求"
          value={`${summary.errorCount}`}
          description="4xx / 5xx 或显式错误"
          icon={AlertTriangle}
          toneClass="bg-red-500/12 text-red-500"
        />
        <SummaryCard
          title="累计令牌"
          value={formatCompactNumber(summary.totalTokens, "0")}
          description="当前筛选结果中的 total tokens"
          icon={Database}
          toneClass="bg-amber-500/12 text-amber-500"
        />
      </div>

      <Card className="glass-card overflow-hidden border-none shadow-xl backdrop-blur-md">
        <CardHeader className="gap-1.5 border-b border-border/40 bg-muted/20 py-4">
          <div className="flex flex-col gap-1 xl:flex-row xl:items-end xl:justify-between">
            <div>
              <CardTitle className="text-[15px] font-semibold">
                请求明细 按{" "}
                <span className="font-medium text-foreground">
                  {currentFilterLabel}
                </span>{" "}
                展示
              </CardTitle>
            </div>
            <div className="text-xs text-muted-foreground"></div>
          </div>
        </CardHeader>
        <CardContent className="px-0">
          <Table className="min-w-[1320px] table-fixed">
            <TableHeader className="bg-muted/30">
              <TableRow>
                <TableHead className="h-12 w-[150px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  时间
                </TableHead>
                <TableHead className="w-[120px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  方法 / 路径
                </TableHead>
                <TableHead className="w-[224px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  账号 / 密钥
                </TableHead>
                <TableHead className="w-[180px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  模型 / 推理
                </TableHead>
                <TableHead className="w-[92px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  状态
                </TableHead>
                <TableHead className="w-[110px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  请求时长
                </TableHead>
                <TableHead className="w-[148px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  令牌
                </TableHead>
                <TableHead className="w-[240px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
                  错误
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {isLoading ? (
                Array.from({ length: 10 }).map((_, index) => (
                  <TableRow key={index}>
                    <TableCell>
                      <Skeleton className="h-4 w-32" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-40" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-32" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-24" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-6 w-12 rounded-full" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-12" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-20" />
                    </TableCell>
                    <TableCell>
                      <Skeleton className="h-4 w-full" />
                    </TableCell>
                  </TableRow>
                ))
              ) : logs.length === 0 ? (
                <TableRow>
                  <TableCell
                    colSpan={8}
                    className="h-52 px-4 text-center text-sm text-muted-foreground"
                  >
                    {!serviceStatus.connected
                      ? "服务未连接，无法获取日志"
                      : "暂无请求日志"}
                  </TableCell>
                </TableRow>
              ) : (
                logs.map((log: RequestLog) => (
                  <TableRow
                    key={log.id}
                    className="group text-xs hover:bg-muted/20"
                  >
                    <TableCell className="px-4 py-3 font-mono text-[11px] text-muted-foreground">
                      {formatTsFromSeconds(log.createdAt, "未知时间")}
                    </TableCell>
                    <TableCell className="px-4 py-3 align-top">
                      <RequestRouteInfoCell log={log} />
                    </TableCell>
                    <TableCell className="px-4 py-3 align-top">
                      <AccountKeyInfoCell
                        log={log}
                        accountLabel={resolveAccountDisplayName(
                          log,
                          accountNameMap,
                        )}
                      />
                    </TableCell>
                    <TableCell className="px-4 py-3 align-top">
                      <ModelEffortCell log={log} />
                    </TableCell>
                    <TableCell className="px-4 py-3 align-top">
                      {getStatusBadge(log.statusCode)}
                    </TableCell>
                    <TableCell className="px-4 py-3 font-mono text-primary">
                      {formatDuration(log.durationMs)}
                    </TableCell>
                    <TableCell className="px-4 py-3 align-top">
                      <div className="flex flex-col gap-0.5 text-[10px] text-muted-foreground">
                        <span>总 {log.totalTokens?.toLocaleString() || 0}</span>
                        <span>
                          输入 {log.inputTokens?.toLocaleString() || 0}
                        </span>
                        <span className="opacity-60">
                          缓存 {log.cachedInputTokens?.toLocaleString() || 0}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell className="px-4 py-3 text-left align-top">
                      <ErrorInfoCell error={log.error} />
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
          共 {summary.filteredCount} 条匹配日志
        </div>
        <div className="flex items-center gap-6">
          <div className="flex items-center gap-2">
            <span className="whitespace-nowrap text-xs text-muted-foreground">
              每页显示
            </span>
            <Select
              value={pageSize}
              onValueChange={(value) => {
                setPageSize(value || "10");
                setPage(1);
              }}
            >
              <SelectTrigger className="h-8 w-[78px] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {["5", "10", "20", "50", "100", "200"].map((value) => (
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
              disabled={page <= 1}
              onClick={() => setPage((current) => Math.max(1, current - 1))}
            >
              上一页
            </Button>
            <div className="min-w-[68px] text-center text-xs font-medium">
              第 {logsResult?.page || page} / {totalPages} 页
            </div>
            <Button
              variant="outline"
              size="sm"
              className="h-8 px-3 text-xs"
              disabled={page >= totalPages}
              onClick={() =>
                setPage((current) => Math.min(totalPages, current + 1))
              }
            >
              下一页
            </Button>
          </div>
        </div>
      </div>

      <ConfirmDialog
        open={clearConfirmOpen}
        onOpenChange={setClearConfirmOpen}
        title="清空请求日志"
        description="确定清空全部请求日志吗？该操作不可恢复。"
        confirmText="清空"
        confirmVariant="destructive"
        onConfirm={() => clearMutation.mutate()}
      />
    </div>
  );
}

export default function LogsPage() {
  return (
    <Suspense fallback={<LogsPageSkeleton />}>
      <LogsPageContent />
    </Suspense>
  );
}
