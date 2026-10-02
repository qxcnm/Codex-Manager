"use client";

import { useQuery } from "@tanstack/react-query";
import { FileText } from "lucide-react";
import { useI18n } from "@/lib/i18n/provider";
import { serviceClient } from "@/lib/api/service-client";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import type { RequestLog } from "@/types";

function prettyJsonPayload(payload: string): string {
  try {
    return JSON.stringify(JSON.parse(payload), null, 2);
  } catch {
    return payload;
  }
}

export function RequestDetailModal({
  open,
  onOpenChange,
  log,
  serviceAddr,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  log: RequestLog | null;
  serviceAddr: string | null;
}) {
  const { t } = useI18n();
  const traceId = log?.traceId?.trim() || "";
  const { data: detail, isLoading } = useQuery({
    queryKey: ["logs", "detail", serviceAddr, traceId],
    queryFn: ({ signal }) =>
      serviceClient.requestLogDetail({ traceId, addr: serviceAddr }, { signal }),
    enabled: open && traceId.length > 0,
    staleTime: 30_000,
    retry: 1,
    gcTime: 60_000,
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card flex max-h-[90dvh] flex-col overflow-hidden p-0 sm:max-w-[720px]">
        <DialogHeader className="border-b px-6 py-4">
          <DialogTitle className="flex items-center gap-2 text-base">
            <FileText className="size-4 text-primary" />
            {t("请求内容")}
          </DialogTitle>
          <DialogDescription className="font-mono text-[11px] break-all">
            {traceId || "-"}
          </DialogDescription>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-6 py-4">
          <div className="grid grid-cols-2 gap-2 text-[11px] text-muted-foreground sm:grid-cols-4">
            <div>
              <div className="opacity-70">{t("路径")}</div>
              <div className="mt-0.5 font-mono break-all">{log?.requestPath || "-"}</div>
            </div>
            <div>
              <div className="opacity-70">{t("模型")}</div>
              <div className="mt-0.5 font-mono break-all">{log?.model || "-"}</div>
            </div>
            <div>
              <div className="opacity-70">{t("状态")}</div>
              <div className="mt-0.5 font-mono">
                {log?.statusCode != null ? String(log.statusCode) : "-"}
              </div>
            </div>
            <div>
              <div className="opacity-70">{t("原始大小")}</div>
              <div className="mt-0.5 font-mono">
                {detail ? `${detail.payloadBytes} B` : "-"}
              </div>
            </div>
          </div>
          <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
            {t("敏感凭据在写入时已脱敏；请求内容为发往上游的实际请求体。")}
          </div>
          {isLoading ? (
            <div className="flex flex-col gap-2">
              <Skeleton className="h-4 w-2/3" />
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-5/6" />
              <Skeleton className="h-4 w-1/2" />
            </div>
          ) : detail ? (
            <>
              {detail.payloadTruncated ? (
                <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-1.5 text-[11px] text-amber-600 dark:text-amber-400">
                  {t("请求内容超出存储上限，仅保留前 16 KB 预览。")}
                </div>
              ) : null}
              <pre className="max-h-[52dvh] overflow-auto rounded-md bg-muted/40 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">
                <code>{prettyJsonPayload(detail.payload)}</code>
              </pre>
            </>
          ) : (
            <div className="rounded-md border px-3 py-6 text-center text-xs text-muted-foreground">
              {t("未找到该请求的内容记录；日志可能产生于旧版本，或已被清理。")}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
