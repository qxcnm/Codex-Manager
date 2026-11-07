"use client";

import { Eye } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import {
  AccountKeyInfoCell,
  ErrorInfoCell,
  ModelEffortCell,
  RequestRouteInfoCell,
} from "./page-cells";
import {
  formatDuration,
  formatTableTokenAmount,
  getStatusBadge,
  resolveAccountDisplayName,
  resolveDisplayedStatusCode,
  type TranslateFn,
} from "./page-helpers";
import type { AggregateApi, ApiKey, RequestLog } from "@/types";

export function RequestLogTable({
  t,
  logs,
  isLogsLoading,
  serviceConnected,
  isDirectAccountMode,
  accountNameMap,
  apiKeyMap,
  aggregateApiMap,
  onOpenDetail,
}: {
  t: TranslateFn;
  logs: RequestLog[];
  isLogsLoading: boolean;
  serviceConnected: boolean;
  isDirectAccountMode: boolean;
  accountNameMap: Map<string, string>;
  apiKeyMap: Map<string, ApiKey>;
  aggregateApiMap: Map<string, AggregateApi>;
  onOpenDetail: (log: RequestLog) => void;
}) {
  return (
    <div className="overflow-x-auto">
      <Table className="min-w-[1500px] table-fixed">
        <TableHeader>
          <TableRow>
            <TableHead className="h-12 w-[150px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("时间")}
            </TableHead>
            <TableHead className="w-[240px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("类型 / 方法 / 路径")}
            </TableHead>
            <TableHead className="w-[224px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("账号 / 密钥")}
            </TableHead>
            <TableHead className="w-[220px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("模型 / 推理 / 等级")}
            </TableHead>
            <TableHead className="w-[92px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("状态")}
            </TableHead>
            <TableHead className="w-[128px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("用时 / 首响")}
            </TableHead>
            <TableHead className="w-[148px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("Token")}
            </TableHead>
            <TableHead className="w-[240px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("错误")}
            </TableHead>
            <TableHead className="w-[72px] px-2 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
              {t("操作")}
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {isLogsLoading ? (
            Array.from({ length: 10 }).map((_, index) => (
              <TableRow key={index}>
                <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                <TableCell><Skeleton className="h-4 w-40" /></TableCell>
                <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                <TableCell><Skeleton className="h-6 w-12 rounded-full" /></TableCell>
                <TableCell><Skeleton className="h-4 w-12" /></TableCell>
                <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                <TableCell><Skeleton className="h-4 w-full" /></TableCell>
                <TableCell><Skeleton className="h-4 w-7" /></TableCell>
              </TableRow>
            ))
          ) : logs.length === 0 ? (
            <TableRow>
              <TableCell
                colSpan={9}
                className="h-52 px-4 text-center text-sm text-muted-foreground"
              >
                {!serviceConnected
                  ? t("服务未连接，无法获取日志")
                  : isDirectAccountMode
                    ? t("当前筛选下暂无已记录的网关请求；本机 Codex 的账号直连请求不会写入此日志。")
                    : t("暂无请求日志")}
              </TableCell>
            </TableRow>
          ) : (
            logs.map((log) => (
              <TableRow key={log.id} className="group text-xs hover:bg-muted/20">
                <TableCell className="px-4 py-3 font-mono text-[11px] text-muted-foreground">
                  {formatTsFromSeconds(log.createdAt, t("未知时间"))}
                </TableCell>
                <TableCell className="px-4 py-3 align-top">
                  <RequestRouteInfoCell log={log} />
                </TableCell>
                <TableCell className="px-4 py-3 align-top">
                  <AccountKeyInfoCell
                    log={log}
                    accountLabel={resolveAccountDisplayName(log, accountNameMap)}
                    accountNameMap={accountNameMap}
                    apiKeyMap={apiKeyMap}
                    aggregateApiMap={aggregateApiMap}
                  />
                </TableCell>
                <TableCell className="px-4 py-3 align-top">
                  <ModelEffortCell log={log} />
                </TableCell>
                <TableCell className="px-4 py-3 align-top">
                  {getStatusBadge(resolveDisplayedStatusCode(log))}
                </TableCell>
                <TableCell className="px-4 py-3 align-top font-mono">
                  <span
                    className="text-xs text-primary"
                    title={t("首响表示从请求开始到首个上游响应片段的耗时")}
                  >
                    {formatDuration(log.durationMs)}/{formatDuration(log.firstResponseMs)}
                  </span>
                </TableCell>
                <TableCell className="px-4 py-3 align-top">
                  <div className="flex flex-col gap-0.5 text-[10px] text-muted-foreground">
                    <span>{t("总")} {formatTableTokenAmount(log.totalTokens)}</span>
                    <span>{t("输入")} {formatTableTokenAmount(log.inputTokens)}</span>
                    <span className="opacity-60">
                      {t("缓存")} {formatTableTokenAmount(log.cachedInputTokens)}
                    </span>
                  </div>
                </TableCell>
                <TableCell className="px-4 py-3 text-left align-top">
                  <ErrorInfoCell error={log.error} />
                </TableCell>
                <TableCell className="px-2 py-3 align-top">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-7 opacity-40 transition-opacity group-hover:opacity-100"
                    title={t("查看请求内容")}
                    onClick={() => onOpenDetail(log)}
                  >
                    <Eye className="size-3.5" />
                  </Button>
                </TableCell>
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </div>
  );
}
