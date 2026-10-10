"use client";

import { useQuery } from "@tanstack/react-query";
import { AlertTriangle } from "lucide-react";
import { serviceClient } from "@/lib/api/service-client";
import { REQUEST_LOG_PAYLOAD_DROP_REASONS } from "@/lib/api/request-log-payload-queue";
import { useI18n } from "@/lib/i18n/provider";
import type { RequestLogPayloadDropReason, RequestLogPayloadTraceDropReason } from "@/types";

export const REQUEST_LOG_PAYLOAD_DROP_REASON_LABELS: Record<RequestLogPayloadDropReason, string> = {
  disk_full: "磁盘空间不足",
  disk_slow: "磁盘写入跟不上",
  io_error: "写入失败",
  writer_unavailable: "写入线程不可用",
  spill_locked: "溢出目录被其他进程占用",
};

export const REQUEST_LOG_PAYLOAD_TRACE_DROP_REASON_LABELS: Record<
  RequestLogPayloadTraceDropReason,
  string
> = {
  ...REQUEST_LOG_PAYLOAD_DROP_REASON_LABELS,
  stale_generation: "清空日志或超出保留期",
};

export function buildPayloadQueueStatsQueryKey(
  serviceAddr: string | null,
  traceId: string | null = null,
) {
  return ["logs", "payload-queue-stats", serviceAddr, traceId] as const;
}

/**
 * Admin banner: request bodies that could not be recorded since the service
 * started (disk full, disk too slow, write errors, ...).
 */
export function RequestPayloadDropNotice({
  serviceAddr,
  enabled,
}: {
  serviceAddr: string | null;
  enabled: boolean;
}) {
  const { t } = useI18n();
  const { data: stats } = useQuery({
    queryKey: buildPayloadQueueStatsQueryKey(serviceAddr),
    queryFn: ({ signal }) =>
      serviceClient.getRequestLogPayloadQueueStats({ addr: serviceAddr }, { signal }),
    enabled,
    retry: false,
    staleTime: 15_000,
    refetchInterval: enabled ? 30_000 : false,
    refetchIntervalInBackground: false,
  });
  if (!stats || stats.droppedTotal <= 0) {
    return null;
  }
  const breakdown = REQUEST_LOG_PAYLOAD_DROP_REASONS.filter(
    (reason) => (stats.droppedByReason[reason] || 0) > 0,
  )
    .map(
      (reason) =>
        `${t(REQUEST_LOG_PAYLOAD_DROP_REASON_LABELS[reason])} ${stats.droppedByReason[reason]}`,
    )
    .join(" · ");
  return (
    <div className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-400">
      <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
      <div className="min-w-0">
        <div>
          {t("有 {count} 条请求内容因磁盘空间不足等原因未记录", {
            count: stats.droppedTotal,
          })}
        </div>
        {breakdown ? <div className="mt-0.5 opacity-80">{breakdown}</div> : null}
      </div>
    </div>
  );
}
