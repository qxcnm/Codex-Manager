"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronDown, ChevronRight, Copy, FileText } from "lucide-react";
import { toast } from "sonner";
import { useI18n } from "@/lib/i18n/provider";
import { serviceClient } from "@/lib/api/service-client";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import type { RequestLog, RequestLogDetail, RequestLogDetailStage } from "@/types";
import type { TranslateFn } from "./page-helpers";
import {
  buildPayloadQueueStatsQueryKey,
  REQUEST_LOG_PAYLOAD_TRACE_DROP_REASON_LABELS,
} from "./payload-drop-notice";

const RAW_BODY_FIELD = "$body";
const SUMMARY_MAX_CHARS = 160;

type DetailRow =
  | { kind: "header"; key: string; label: string; tone: "normal" | "muted" | "warning" }
  | {
      kind: "entry";
      key: string;
      label: string;
      content: string;
      inherited: boolean;
      isJson: boolean;
    };

function prettyJsonText(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

function textOfContent(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    for (const part of content) {
      if (part && typeof part === "object") {
        const record = part as Record<string, unknown>;
        const text = record.text ?? record.input_text ?? record.output_text;
        if (typeof text === "string" && text.trim()) return text;
      }
    }
  }
  return "";
}

/** Short one-line description of a list item: role / type plus a text snippet. */
function summarizeItem(raw: string): { tag: string; snippet: string } {
  const value = parseJson(raw);
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { tag: "", snippet: raw.slice(0, SUMMARY_MAX_CHARS) };
  }
  const record = value as Record<string, unknown>;
  const tag = [record.type, record.role, record.name]
    .filter((part): part is string => typeof part === "string" && part.length > 0)
    .join(" · ");
  const text =
    textOfContent(record.content) ||
    textOfContent(record.parts) ||
    (typeof record.arguments === "string" ? record.arguments : "") ||
    (typeof record.output === "string" ? record.output : "");
  const snippet = (text || raw).replace(/\s+/g, " ").slice(0, SUMMARY_MAX_CHARS);
  return { tag, snippet };
}

function buildFullRequestJson(detail: RequestLogDetail): string {
  if (detail.storageMode === "preview") return detail.payload;
  const rawBody = detail.fields.find((field) => field.name === RAW_BODY_FIELD);
  if (rawBody && (detail.bodyKind === "text" || detail.bodyKind === "base64")) {
    return rawBody.value;
  }
  const body: Record<string, unknown> = {};
  for (const field of detail.fields) {
    body[field.name] = parseJson(field.value) ?? field.value;
  }
  if (detail.listField) {
    body[detail.listField] = detail.items.map((item) => parseJson(item) ?? item);
  }
  return JSON.stringify(body, null, 2);
}

function buildRows(detail: RequestLogDetail, t: TranslateFn): DetailRow[] {
  const rows: DetailRow[] = [];
  if (detail.storageMode === "preview") {
    rows.push({
      kind: "entry",
      key: "preview",
      label: t("请求内容"),
      content: detail.payload,
      inherited: false,
      isJson: !detail.payloadTruncated,
    });
    return rows;
  }

  detail.context.forEach((segment, segmentIndex) => {
    rows.push({
      kind: "header",
      key: `ctx-${segmentIndex}`,
      label: t("上文请求 {trace}（{count} 条）", {
        trace: segment.traceId,
        count: segment.items.length,
      }),
      tone: "muted",
    });
    segment.items.forEach((item, itemIndex) => {
      const summary = summarizeItem(item);
      rows.push({
        kind: "entry",
        key: `ctx-${segmentIndex}-${itemIndex}`,
        label: `#${itemIndex + 1} ${summary.tag}`.trim(),
        content: item,
        inherited: true,
        isJson: true,
      });
    });
  });
  if (detail.context.length > 0) {
    rows.push({
      kind: "header",
      key: "ctx-gap",
      label: t("上一轮模型输出（{id}）不在请求日志中，以下为本次请求新增内容", {
        id: detail.previousResponseId ?? "-",
      }),
      tone: "warning",
    });
  } else if (detail.previousResponseId) {
    rows.push({
      kind: "header",
      key: "ctx-missing",
      label: t("本请求通过 previous_response_id（{id}）续接上文，但未找到同会话的上一请求记录", {
        id: detail.previousResponseId,
      }),
      tone: "warning",
    });
  }

  if (detail.fields.length > 0) {
    rows.push({
      kind: "header",
      key: "fields",
      label: t("顶层字段（{count}）", { count: detail.fields.length }),
      tone: "normal",
    });
    detail.fields.forEach((field) => {
      rows.push({
        kind: "entry",
        key: `field-${field.name}`,
        label: field.name === RAW_BODY_FIELD ? t("请求体") : field.name,
        content: field.value,
        inherited: false,
        isJson: field.name !== RAW_BODY_FIELD,
      });
    });
  }

  if (detail.listField) {
    rows.push({
      kind: "header",
      key: "items",
      label:
        detail.inheritedItemCount > 0
          ? t("{field}（{count} 条，前 {shared} 条与上一请求相同）", {
              field: detail.listField,
              count: detail.items.length,
              shared: detail.inheritedItemCount,
            })
          : t("{field}（{count} 条）", {
              field: detail.listField,
              count: detail.items.length,
            }),
      tone: "normal",
    });
    detail.items.forEach((item, index) => {
      const summary = summarizeItem(item);
      rows.push({
        kind: "entry",
        key: `item-${index}`,
        label: `#${index + 1} ${summary.tag}`.trim(),
        content: item,
        inherited: index < detail.inheritedItemCount,
        isJson: true,
      });
    });
  }
  return rows;
}

function EntryRow({
  row,
  expanded,
  onToggle,
  t,
}: {
  row: Extract<DetailRow, { kind: "entry" }>;
  expanded: boolean;
  onToggle: () => void;
  t: TranslateFn;
}) {
  const summary = useMemo(
    () => (row.isJson ? summarizeItem(row.content).snippet : row.content.slice(0, SUMMARY_MAX_CHARS)),
    [row.content, row.isJson]
  );
  const pretty = useMemo(
    () => (expanded ? (row.isJson ? prettyJsonText(row.content) : row.content) : ""),
    [expanded, row.content, row.isJson]
  );
  return (
    <div className="border-b border-border/50 py-1.5">
      <button
        type="button"
        className="flex w-full items-start gap-2 rounded px-1 py-0.5 text-left hover:bg-muted/40"
        onClick={onToggle}
      >
        {expanded ? (
          <ChevronDown className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronRight className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
        )}
        <span className="shrink-0 font-mono text-[11px] font-medium">{row.label}</span>
        {row.inherited ? (
          <span className="shrink-0 rounded bg-muted px-1 text-[10px] text-muted-foreground">
            {t("沿用")}
          </span>
        ) : null}
        <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted-foreground">
          {summary}
        </span>
        <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
          {formatBytes(new TextEncoder().encode(row.content).length)}
        </span>
      </button>
      {expanded ? (
        <pre className="mt-1 max-h-[45dvh] overflow-auto rounded-md bg-muted/40 p-3 font-mono text-[11px] leading-relaxed break-all whitespace-pre-wrap">
          <code>{pretty}</code>
        </pre>
      ) : null}
    </div>
  );
}

function DetailEntries({ detail, t }: { detail: RequestLogDetail; t: TranslateFn }) {
  const rows = useMemo(() => buildRows(detail, t), [detail, t]);
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(detail.storageMode === "preview" ? ["preview"] : [])
  );
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: (index) => (rows[index]?.kind === "header" ? 30 : 34),
    overscan: 12,
    getItemKey: (index) => rows[index]?.key ?? index,
  });

  const toggle = useCallback((key: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const entryKeys = useMemo(
    () => rows.filter((row) => row.kind === "entry").map((row) => row.key),
    [rows]
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      {entryKeys.length > 1 ? (
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-[11px]"
            onClick={() => setExpanded(new Set(entryKeys))}
          >
            {t("全部展开")}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-[11px]"
            onClick={() => setExpanded(new Set())}
          >
            {t("全部收起")}
          </Button>
        </div>
      ) : null}
      <div ref={scrollRef} className="h-[56dvh] overflow-auto rounded-md border px-2">
        <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const row = rows[virtualRow.index];
            if (!row) return null;
            return (
              <div
                key={virtualRow.key}
                data-index={virtualRow.index}
                ref={virtualizer.measureElement}
                className="absolute top-0 left-0 w-full"
                style={{ transform: `translateY(${virtualRow.start}px)` }}
              >
                {row.kind === "header" ? (
                  <div
                    className={
                      row.tone === "warning"
                        ? "my-1 rounded-md border border-amber-500/40 bg-amber-500/10 px-2 py-1 text-[11px] text-amber-600 dark:text-amber-400"
                        : row.tone === "muted"
                          ? "pt-3 pb-1 text-[11px] font-semibold text-muted-foreground"
                          : "pt-3 pb-1 text-[11px] font-semibold"
                    }
                  >
                    {row.label}
                  </div>
                ) : (
                  <EntryRow
                    row={row}
                    expanded={expanded.has(row.key)}
                    onToggle={() => toggle(row.key)}
                    t={t}
                  />
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export function RequestDetailModal({
  open,
  onOpenChange,
  log,
  serviceAddr,
  canInspectPayloadQueue = false,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  log: RequestLog | null;
  serviceAddr: string | null;
  /** Admins can ask the write queue whether this trace's content was dropped. */
  canInspectPayloadQueue?: boolean;
}) {
  const { t } = useI18n();
  const traceId = log?.traceId?.trim() || "";
  const [stage, setStage] = useState<RequestLogDetailStage | null>(null);

  // A new trace starts from the default stage again.
  useEffect(() => {
    setStage(null);
  }, [traceId, open]);

  const { data: detail, isLoading } = useQuery({
    queryKey: ["logs", "detail", serviceAddr, traceId, stage],
    queryFn: ({ signal }) =>
      serviceClient.requestLogDetail(
        { traceId, stage, addr: serviceAddr },
        { signal }
      ),
    enabled: open && traceId.length > 0,
    staleTime: 30_000,
    retry: 1,
    gcTime: 60_000,
  });

  const { data: payloadQueueStats } = useQuery({
    queryKey: buildPayloadQueueStatsQueryKey(serviceAddr, traceId),
    queryFn: ({ signal }) =>
      serviceClient.getRequestLogPayloadQueueStats(
        { traceId, addr: serviceAddr },
        { signal }
      ),
    enabled:
      canInspectPayloadQueue && open && traceId.length > 0 && !isLoading && !detail,
    retry: false,
    staleTime: 30_000,
    gcTime: 60_000,
  });
  const dropReason = !detail ? payloadQueueStats?.traceDropReason ?? null : null;

  const copyFullRequest = useCallback(async () => {
    if (!detail) return;
    try {
      await navigator.clipboard.writeText(buildFullRequestJson(detail));
      toast.success(t("已复制完整请求内容"));
    } catch {
      toast.error(t("复制失败"));
    }
  }, [detail, t]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="glass-card flex max-h-[92dvh] flex-col overflow-hidden p-0 sm:max-w-[860px]">
        <DialogHeader className="border-b px-6 py-4">
          <DialogTitle className="flex items-center gap-2 text-base">
            <FileText className="size-4 text-primary" />
            {t("请求内容")}
          </DialogTitle>
          <DialogDescription className="font-mono text-[11px] break-all">
            {traceId || "-"}
          </DialogDescription>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden px-6 py-4">
          <div className="grid grid-cols-2 gap-2 text-[11px] text-muted-foreground sm:grid-cols-5">
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
                {detail ? formatBytes(detail.payloadBytes) : "-"}
              </div>
            </div>
            <div>
              <div className="opacity-70">{t("存储方式")}</div>
              <div className="mt-0.5">
                {detail
                  ? detail.storageMode === "full"
                    ? t("完整存储")
                    : t("16 KB 预览")
                  : "-"}
              </div>
            </div>
          </div>
          {detail ? (
            <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
              {detail.stages.length > 1 ? (
                <div className="flex flex-wrap items-center gap-1">
                  <span className="opacity-70">{t("请求体来源")}</span>
                  {detail.stages.map((captureStage) => (
                    <Button
                      key={captureStage}
                      type="button"
                      variant={detail.stage === captureStage ? "secondary" : "ghost"}
                      size="sm"
                      className="h-6 px-2 text-[11px]"
                      onClick={() => setStage(captureStage)}
                    >
                      {captureStage === "client"
                        ? t("客户端原始")
                        : t("上游尝试 {number}", {
                            number: detail.stages.filter((item) => item !== "client")
                              .indexOf(captureStage) + 1,
                          })}
                    </Button>
                  ))}
                </div>
              ) : null}
              <span>
                {detail.redacted ? t("凭据字段已脱敏；") : t("未脱敏，可能包含凭据；")}
                {detail.stage.startsWith("upstream")
                  ? t("内容为发往上游的实际请求体。")
                  : t("内容为客户端原始请求体。")}
              </span>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="ml-auto h-7 gap-1 text-[11px]"
                onClick={() => void copyFullRequest()}
              >
                <Copy className="size-3" />
                {t("复制完整请求")}
              </Button>
            </div>
          ) : null}
          {detail?.attempt ? (
            <div className="rounded-md border bg-muted/30 px-3 py-2 font-mono text-[11px] break-all">
              <div>{detail.attempt.method} {detail.attempt.url} · {detail.attempt.transport}</div>
              <div className="mt-1 text-muted-foreground">
                {t("传输字节 SHA-256")}: {detail.attempt.wireSha256}
                {detail.attempt.contentEncoding
                  ? ` · ${t("传输编码")}: ${detail.attempt.contentEncoding}`
                  : ""}
              </div>
              {detail.attempt.contentEncoding ? (
                <div className="mt-1 text-muted-foreground">
                  {t("请求体展示为解码后内容；上方摘要对应实际发送的压缩字节。")}
                </div>
              ) : null}
            </div>
          ) : null}
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
              {!detail.complete && detail.storageMode === "full" ? (
                <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-1.5 text-[11px] text-amber-600 dark:text-amber-400">
                  {t("该请求依赖的上一请求记录已被清理，部分消息无法还原。")}
                </div>
              ) : null}
              <DetailEntries key={`${detail.traceId}-${detail.stage}`} detail={detail} t={t} />
            </>
          ) : dropReason ? (
            <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-6 text-center text-xs text-amber-700 dark:text-amber-400">
              {t("此请求内容因{reason}未记录", {
                reason: t(REQUEST_LOG_PAYLOAD_TRACE_DROP_REASON_LABELS[dropReason]),
              })}
            </div>
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
