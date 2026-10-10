import type {
  RequestLogPayloadDropReason,
  RequestLogPayloadQueueStats,
  RequestLogPayloadTraceDropReason,
} from "../../types";

export const REQUEST_LOG_PAYLOAD_DROP_REASONS: RequestLogPayloadDropReason[] = [
  "disk_full",
  "disk_slow",
  "io_error",
  "writer_unavailable",
  "spill_locked",
];

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asCount(value: unknown): number {
  const number = typeof value === "number" ? value : Number(value);
  return Number.isFinite(number) && number > 0 ? Math.floor(number) : 0;
}

export const REQUEST_LOG_PAYLOAD_TRACE_DROP_REASONS: RequestLogPayloadTraceDropReason[] = [
  ...REQUEST_LOG_PAYLOAD_DROP_REASONS,
  "stale_generation",
];

function asTraceDropReason(value: unknown): RequestLogPayloadTraceDropReason | null {
  return typeof value === "string" &&
    (REQUEST_LOG_PAYLOAD_TRACE_DROP_REASONS as string[]).includes(value)
    ? (value as RequestLogPayloadTraceDropReason)
    : null;
}

function asDropReason(value: unknown): RequestLogPayloadDropReason | null {
  return typeof value === "string" &&
    (REQUEST_LOG_PAYLOAD_DROP_REASONS as string[]).includes(value)
    ? (value as RequestLogPayloadDropReason)
    : null;
}

export function normalizeRequestLogPayloadQueueStats(
  payload: unknown
): RequestLogPayloadQueueStats {
  const source = asRecord(payload);
  const byReason = asRecord(source.droppedByReason ?? source.dropped_by_reason);
  const droppedByReason: Partial<Record<RequestLogPayloadDropReason, number>> = {};
  for (const reason of REQUEST_LOG_PAYLOAD_DROP_REASONS) {
    const count = asCount(byReason[reason]);
    if (count > 0) droppedByReason[reason] = count;
  }
  return {
    budgetBytes: asCount(source.budgetBytes ?? source.budget_bytes),
    queuedBytes: asCount(source.queuedBytes ?? source.queued_bytes),
    spillPendingBytes: asCount(source.spillPendingBytes ?? source.spill_pending_bytes),
    spillDiskBytes: asCount(source.spillDiskBytes ?? source.spill_disk_bytes),
    spilling: source.spilling === true,
    spillAvailable: source.spillAvailable !== false && source.spill_available !== false,
    spillBlockedReason: asDropReason(source.spillBlockedReason ?? source.spill_blocked_reason),
    spilledTotal: asCount(source.spilledTotal ?? source.spilled_total),
    droppedTotal: asCount(source.droppedTotal ?? source.dropped_total),
    droppedByReason,
    traceDropReason: asTraceDropReason(source.traceDropReason ?? source.trace_drop_reason),
  };
}
