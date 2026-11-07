"use client";

import { ChevronRight, Copy, FileText, ListFilter } from "lucide-react";
import type { ReactNode } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { copyTextToClipboard } from "@/lib/utils/clipboard";
import { formatTsFromSeconds } from "@/lib/utils/usage";
import { cn } from "@/lib/utils";
import type { AggregateApi, ApiKey, RequestLog } from "@/types";
import {
  AccountKeyInfoCell,
  ModelEffortCell,
  RequestRouteInfoCell,
} from "./page-cells";
import {
  formatCompactKeyLabel,
  formatDuration,
  formatTableTokenAmount,
  normalizeAggregateApiUrl,
  normalizeRequestType,
  resolveAccountDisplayName,
  resolveAggregateApiDisplayName,
  resolveDisplayedStatusCode,
  resolveDisplayRequestPath,
  resolveFriendlyRequestPathLabel,
  type TranslateFn,
} from "./page-helpers";

type TimelineProps = {
  t: TranslateFn;
  logs: RequestLog[];
  isLogsLoading: boolean;
  serviceConnected: boolean;
  isDirectAccountMode: boolean;
  accountNameMap: Map<string, string>;
  apiKeyMap: Map<string, ApiKey>;
  aggregateApiMap: Map<string, AggregateApi>;
  onOpenDetail: (log: RequestLog) => void;
};

function Metadata({ label, children }: { label: string; children: ReactNode }) {
  return (
    <span className="inline-flex min-w-0 max-w-full items-baseline gap-1.5 rounded-md bg-muted/45 px-2 py-1 text-[11px] leading-4">
      <span className="shrink-0 text-muted-foreground">{label}</span>
      <span
        className="min-w-0 truncate font-mono text-foreground/85"
        title={typeof children === "string" ? children : undefined}
      >
        {children}
      </span>
    </span>
  );
}

function RequestLogEntry({
  log,
  t,
  accountNameMap,
  apiKeyMap,
  aggregateApiMap,
  onOpenDetail,
}: Omit<
  TimelineProps,
  "logs" | "isLogsLoading" | "serviceConnected" | "isDirectAccountMode"
> & { log: RequestLog }) {
  const status = resolveDisplayedStatusCode(log);
  const failed = status != null && status >= 400;
  const succeeded = status != null && status >= 200 && status < 300;
  const title = failed
    ? t("请求异常")
    : succeeded
      ? t("请求完成")
      : t("请求已返回");
  const accountLabel = resolveAccountDisplayName(log, accountNameMap);
  const apiKey = apiKeyMap.get(log.keyId) || null;
  const aggregate =
    aggregateApiMap.get(log.actualSourceId) ||
    (apiKey?.aggregateApiId
      ? aggregateApiMap.get(apiKey.aggregateApiId)
      : null) ||
    (log.upstreamUrl
      ? [...aggregateApiMap.values()].find(
          (item) =>
            normalizeAggregateApiUrl(item.url) ===
            normalizeAggregateApiUrl(log.upstreamUrl),
        )
      : null) ||
    null;
  const isAggregate = log.actualSourceKind
    ? log.actualSourceKind === "aggregate_api"
    : Boolean(log.aggregateApiSupplierName || log.aggregateApiUrl || aggregate);
  const sourceLabel = isAggregate
    ? resolveAggregateApiDisplayName(log, aggregate, apiKey)
    : accountLabel || log.accountId || "-";
  const keyLabel = apiKey?.name?.trim() || formatCompactKeyLabel(log.keyId);
  const path = resolveDisplayRequestPath(log);
  const traceLabel = log.traceId || `#${log.id}`;
  const timeLabel = formatTsFromSeconds(log.createdAt, t("未知时间"));

  async function copySummary() {
    try {
      // Copy only the displayed request metadata, never API key objects or payloads.
      await copyTextToClipboard(
        JSON.stringify(
          {
            traceId: log.traceId || null,
            time: timeLabel,
            method: log.method,
            path,
            model: log.model,
            source: sourceLabel,
            keyId: log.keyId,
            statusCode: status,
            durationMs: log.durationMs,
            firstResponseMs: log.firstResponseMs,
            totalTokens: log.totalTokens,
            error: log.error || null,
          },
          null,
          2,
        ),
      );
      toast.success(t("已复制到剪贴板"));
    } catch {
      toast.error(t("复制失败"));
    }
  }

  return (
    <li
      data-testid={`request-log-entry-${log.id}`}
      className="grid min-w-0 gap-3 px-4 py-4 transition-colors hover:bg-muted/15 sm:grid-cols-[152px_minmax(0,1fr)] sm:gap-5 sm:px-5"
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 sm:flex-col sm:items-start sm:gap-1.5 sm:pt-0.5">
        <span className="font-mono text-[11px] leading-5 tabular-nums">
          {timeLabel}
        </span>
        <span className="max-w-full truncate font-mono text-[10px] text-muted-foreground">
          {log.id !== log.traceId ? `#${log.id} · ` : ""}
          {formatDuration(log.durationMs)}
        </span>
        <Badge
          variant="outline"
          className={cn(
            "h-5 rounded-full px-2 font-mono text-[10px]",
            failed
              ? "border-destructive/20 bg-destructive/8 text-destructive"
              : succeeded
                ? "border-emerald-500/25 bg-emerald-500/10 text-[var(--metric-emerald)]"
                : "border-border bg-muted/50 text-muted-foreground",
          )}
        >
          {status == null ? t("状态未知") : `HTTP ${status}`}
        </Badge>
      </div>
      <div className="min-w-0 space-y-2.5">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1.5">
            <h3 className="text-sm font-semibold leading-6">{title}</h3>
            <Badge
              variant="outline"
              className="h-5 max-w-full rounded-md border-border/60 font-mono text-[10px] text-muted-foreground"
            >
              <span className="truncate">{log.model || "-"}</span>
            </Badge>
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="h-7 shrink-0 gap-1.5 rounded-lg bg-muted/45 px-2 text-[11px] text-muted-foreground"
            onClick={() => void copySummary()}
          >
            <Copy className="size-3" />
            <span className="hidden sm:inline">{t("复制摘要")}</span>
            <span className="sr-only sm:hidden">{t("复制摘要")}</span>
          </Button>
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
          <span className="shrink-0 font-mono text-[10px] font-medium">
            {normalizeRequestType(log.requestType).toUpperCase()} ·{" "}
            {log.method || "-"}
          </span>
          <span
            className="min-w-0 line-clamp-2 break-all font-mono"
            title={path}
          >
            {resolveFriendlyRequestPathLabel(path, t) || "-"}
          </span>
        </div>
        <div className="flex min-w-0 flex-wrap gap-1.5">
          <Metadata label={isAggregate ? t("聚合 API") : t("账号")}>
            {sourceLabel}
          </Metadata>
          <Metadata label={t("密钥")}>{keyLabel}</Metadata>
          <Metadata label={t("用时")}>
            {formatDuration(log.durationMs)}
          </Metadata>
          <Metadata label={t("首响")}>
            {formatDuration(log.firstResponseMs)}
          </Metadata>
          <Metadata label="Token">
            {formatTableTokenAmount(log.totalTokens)}
          </Metadata>
        </div>
        {log.error ? (
          <p
            className="line-clamp-2 break-all text-xs leading-5 text-destructive"
            title={log.error}
          >
            {log.error}
          </p>
        ) : null}
        <details className="group/details min-w-0 rounded-lg border border-border/45 bg-muted/15">
          <summary className="flex min-h-8 cursor-pointer list-none items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
            <ChevronRight className="size-3 shrink-0 transition-transform group-open/details:rotate-90 motion-reduce:transition-none" />
            <span className="shrink-0">{t("查看明细")}</span>
            <span aria-hidden="true">·</span>
            <span className="min-w-0 truncate font-mono" title={traceLabel}>
              {traceLabel}
            </span>
          </summary>
          <div className="space-y-3 border-t border-border/45 p-3">
            <div className="grid min-w-0 gap-4 sm:grid-cols-2 xl:grid-cols-4">
              <div className="min-w-0 space-y-2">
                <p className="text-[10px] text-muted-foreground">
                  {t("类型 / 方法 / 路径")}
                </p>
                <RequestRouteInfoCell log={log} />
              </div>
              <div className="min-w-0 space-y-2">
                <p className="text-[10px] text-muted-foreground">
                  {t("账号 / 密钥")}
                </p>
                <AccountKeyInfoCell
                  log={log}
                  accountLabel={accountLabel}
                  accountNameMap={accountNameMap}
                  apiKeyMap={apiKeyMap}
                  aggregateApiMap={aggregateApiMap}
                />
              </div>
              <div className="min-w-0 space-y-2">
                <p className="text-[10px] text-muted-foreground">
                  {t("模型 / 推理 / 等级")}
                </p>
                <ModelEffortCell log={log} />
              </div>
              <div className="min-w-0 space-y-2">
                <p className="text-[10px] text-muted-foreground">Token</p>
                <dl className="grid grid-cols-2 gap-x-3 gap-y-1 text-[11px]">
                  {[
                    [t("输入"), log.inputTokens],
                    [t("输出"), log.outputTokens],
                    [t("缓存"), log.cachedInputTokens],
                    [t("总"), log.totalTokens],
                  ].map(([label, value]) => (
                    <div key={label} className="contents">
                      <dt className="text-muted-foreground">{label}</dt>
                      <dd className="text-right font-mono tabular-nums">
                        {formatTableTokenAmount(value as number | null)}
                      </dd>
                    </div>
                  ))}
                </dl>
              </div>
            </div>
            {log.error ? (
              <p className="whitespace-pre-wrap break-all rounded-md bg-destructive/5 p-2 text-xs leading-5 text-destructive">
                {log.error}
              </p>
            ) : null}
            <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border/45 pt-2">
              <span className="min-w-0 break-all font-mono text-[10px] text-muted-foreground">
                {traceLabel}
              </span>
              <Button
                variant="outline"
                size="sm"
                className="h-8 gap-1.5 text-xs"
                onClick={() => onOpenDetail(log)}
              >
                <FileText className="size-3.5" />
                {t("查看请求内容")}
              </Button>
            </div>
          </div>
        </details>
      </div>
    </li>
  );
}

export function RequestLogTimeline(props: TimelineProps) {
  const { t, logs, isLogsLoading, serviceConnected, isDirectAccountMode } =
    props;
  if (isLogsLoading)
    return (
      <div aria-busy="true" className="divide-y divide-border/45">
        {Array.from({ length: 5 }, (_, i) => (
          <div
            key={i}
            className="grid gap-5 p-5 sm:grid-cols-[152px_minmax(0,1fr)]"
          >
            <Skeleton className="h-5 w-32" />
            <div className="space-y-3">
              <Skeleton className="h-5 w-48 max-w-full" />
              <Skeleton className="h-5 w-3/4" />
              <Skeleton className="h-8 w-full" />
            </div>
          </div>
        ))}
      </div>
    );
  if (!logs.length)
    return (
      <div className="flex min-h-52 flex-col items-center justify-center gap-3 px-5 py-10 text-center text-sm text-muted-foreground">
        <ListFilter className="size-7 opacity-50" />
        <p>
          {!serviceConnected
            ? t("服务未连接，无法获取日志")
            : isDirectAccountMode
              ? t(
                  "当前筛选下暂无已记录的网关请求；本机 Codex 的账号直连请求不会写入此日志。",
                )
              : t("暂无请求日志")}
        </p>
      </div>
    );
  return (
    <ol
      aria-label={t("请求时间线")}
      className="min-w-0 divide-y divide-border/45"
    >
      {logs.map((log) => (
        <RequestLogEntry key={log.id} {...props} log={log} />
      ))}
    </ol>
  );
}
