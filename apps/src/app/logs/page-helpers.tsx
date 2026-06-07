import type { LucideIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import { formatCompactNumber } from "@/lib/utils/usage";
import type { AggregateApi, ApiKey, RequestLog } from "@/types";

export type StatusFilter = "all" | "2xx" | "4xx" | "5xx";
export type LogsTab = "requests";
export type TimeRangePreset = "all" | "30m" | "2h" | "24h" | "today" | "custom";
export type TranslateFn = (
  message: string,
  values?: Record<string, string | number>,
) => string;

function padDateTimeSegment(value: number): string {
  return String(value).padStart(2, "0");
}

export function toDateTimeLocalValue(
  timestampSeconds: number | null | undefined,
): string {
  if (!timestampSeconds) return "";
  const date = new Date(timestampSeconds * 1000);
  if (Number.isNaN(date.getTime())) return "";
  const year = date.getFullYear();
  const month = padDateTimeSegment(date.getMonth() + 1);
  const day = padDateTimeSegment(date.getDate());
  const hour = padDateTimeSegment(date.getHours());
  const minute = padDateTimeSegment(date.getMinutes());
  return `${year}-${month}-${day}T${hour}:${minute}`;
}

export function fromDateTimeLocalValue(value: string): number | null {
  const normalized = String(value || "").trim();
  if (!normalized) return null;
  const parsed = new Date(normalized);
  if (Number.isNaN(parsed.getTime())) {
    return null;
  }
  return Math.floor(parsed.getTime() / 1000);
}

export function buildFixedTimePreset(
  preset: Exclude<TimeRangePreset, "all" | "custom">,
  localDayStartTs: number,
  localDayEndTs: number,
): { startInput: string; endInput: string } {
  if (preset === "today") {
    return {
      startInput: toDateTimeLocalValue(localDayStartTs),
      endInput: toDateTimeLocalValue(localDayEndTs),
    };
  }

  const nowTs = Math.floor(Date.now() / 1000);
  const durationSeconds =
    preset === "30m" ? 30 * 60 : preset === "2h" ? 2 * 60 * 60 : 24 * 60 * 60;
  return {
    startInput: toDateTimeLocalValue(nowTs - durationSeconds),
    endInput: toDateTimeLocalValue(nowTs),
  };
}

export function getStatusBadge(statusCode: number | null) {
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

export function SummaryCard({
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
    <Card size="sm" className="glass-card shadow-sm transition-all">
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

export function LogsPageSkeleton() {
  return (
    <div className="space-y-5">
      <Skeleton className="h-28 w-full rounded-xl" />
      <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        {Array.from({ length: 4 }).map((_, index) => (
          <Skeleton key={index} className="h-32 w-full rounded-xl" />
        ))}
      </div>
      <Skeleton className="h-[420px] w-full rounded-xl" />
    </div>
  );
}

export function formatDuration(value: number | null): string {
  if (value == null) return "-";
  if (value >= 10_000) return `${Math.round(value / 1000)}s`;
  if (value >= 1000) return `${(value / 1000).toFixed(1).replace(/\.0$/, "")}s`;
  return `${Math.round(value)}ms`;
}

function formatTokenAmount(value: number | null | undefined): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
  return normalized.toLocaleString("zh-CN", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
}

export function formatCompactTokenAmount(
  value: number | null | undefined,
): string {
  const normalized =
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : 0;
  if (normalized < 1000) {
    return formatTokenAmount(normalized);
  }
  return formatCompactNumber(normalized, "0.00", 2, true);
}

export function formatTableTokenAmount(
  value: number | null | undefined,
): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "-";
  }
  const normalized = Math.max(0, value);
  return Math.round(normalized).toLocaleString("zh-CN");
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

export function formatCompactKeyLabel(keyId: string): string {
  if (!keyId) return "-";
  if (keyId.length <= 12) return keyId;
  return `${keyId.slice(0, 8)}...`;
}

export function resolveDisplayRequestPath(log: RequestLog): string {
  const originalPath = String(log.originalPath || "").trim();
  if (originalPath) {
    return originalPath;
  }
  return String(log.path || log.requestPath || "").trim();
}

export function resolveFriendlyRequestPathLabel(
  path: string,
  t: TranslateFn,
): string {
  const normalized = String(path || "").trim();
  switch (normalized) {
    case "/v1/responses/compact":
      return t("上下文压缩");
    case "/internal/account/warmup":
      return t("账号预热");
    default:
      return normalized;
  }
}

export function resolveUpstreamDisplay(
  upstreamUrl: string,
  t: TranslateFn,
): string {
  const raw = String(upstreamUrl || "").trim();
  if (!raw) return "";
  if (raw === "默认" || raw === "本地" || raw === "自定义") {
    return t(raw);
  }
  try {
    const url = new URL(raw);
    const pathname = url.pathname.replace(/\/+$/, "");
    return pathname ? `${url.host}${pathname}` : url.host;
  } catch {
    return raw;
  }
}

export function resolveAccountDisplayName(
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

export function resolveAccountDisplayNameById(
  accountId: string,
  accountNameMap: Map<string, string>,
): string {
  const normalized = String(accountId || "").trim();
  if (!normalized) return "";
  return (
    accountNameMap.get(normalized) ||
    fallbackAccountNameFromId(normalized) ||
    normalized
  );
}

export function resolveDisplayedStatusCode(log: RequestLog): number | null {
  const statusCode = log.statusCode;
  const hasError = Boolean(String(log.error || "").trim());
  if (statusCode == null) {
    return hasError ? 502 : null;
  }
  if (hasError && statusCode < 400) {
    return 502;
  }
  return statusCode;
}

export function resolveAggregateApiDisplayName(
  log: RequestLog,
  aggregateApi: AggregateApi | null,
  apiKey: ApiKey | null,
): string {
  if (log.aggregateApiSupplierName && log.aggregateApiSupplierName.trim()) {
    return log.aggregateApiSupplierName.trim();
  }
  if (aggregateApi?.supplierName && aggregateApi.supplierName.trim()) {
    return aggregateApi.supplierName.trim();
  }
  if (apiKey?.aggregateApiUrl) {
    return apiKey.aggregateApiUrl.trim();
  }
  return "-";
}

export function resolveAggregateApiTooltipUrl(
  log: RequestLog,
  aggregateApi: AggregateApi | null,
  apiKey: ApiKey | null,
): string {
  if (log.aggregateApiUrl && log.aggregateApiUrl.trim()) {
    return log.aggregateApiUrl.trim();
  }
  if (aggregateApi?.url && aggregateApi.url.trim()) {
    return aggregateApi.url.trim();
  }
  if (apiKey?.aggregateApiUrl) {
    return apiKey.aggregateApiUrl.trim();
  }
  return "-";
}

export function resolveAggregateApiDisplayNameById(
  aggregateApiId: string,
  aggregateApiMap: Map<string, AggregateApi>,
): string {
  const normalized = String(aggregateApiId || "").trim();
  if (!normalized) return "";
  const aggregateApi = aggregateApiMap.get(normalized);
  if (aggregateApi?.supplierName && aggregateApi.supplierName.trim()) {
    return aggregateApi.supplierName.trim();
  }
  if (aggregateApi?.url && aggregateApi.url.trim()) {
    return aggregateApi.url.trim();
  }
  return normalized;
}

export function normalizeAggregateApiUrl(value: string): string {
  return String(value || "").trim().replace(/\/+$/, "");
}

export function formatModelEffortDisplay(log: RequestLog): string {
  const model = String(log.model || "").trim();
  const effort = String(log.reasoningEffort || "").trim();
  if (model && effort) {
    return `${model}/${effort}`;
  }
  return model || effort || "-";
}

export function normalizeRequestType(value: string): "ws" | "http" {
  return String(value || "").trim().toLowerCase() === "ws" ? "ws" : "http";
}

function normalizeDisplayServiceTier(value: string | null | undefined): string {
  const normalized = String(value || "").trim().toLowerCase();
  if (!normalized || normalized === "auto") {
    return "";
  }
  if (normalized === "priority") {
    return "fast";
  }
  return normalized;
}

export function resolveDisplayServiceTier(
  requestServiceTier: string | null | undefined,
): string {
  const direct = normalizeDisplayServiceTier(requestServiceTier);
  if (direct) {
    return direct;
  }
  return "auto";
}

export function RequestTypeBadge({ requestType }: { requestType: string }) {
  const normalized = normalizeRequestType(requestType);
  const label = normalized.toUpperCase();
  const toneClass =
    normalized === "ws"
      ? "border-cyan-500/20 bg-cyan-500/10 text-cyan-500"
      : "border-slate-500/20 bg-slate-500/10 text-slate-500";
  return (
    <Badge className={cn("h-5 rounded-full px-1.5 text-[10px] font-medium", toneClass)}>
      {label}
    </Badge>
  );
}

export function ServiceTierBadge({ serviceTier }: { serviceTier: string }) {
  const normalized = resolveDisplayServiceTier(serviceTier);
  const toneClass =
    normalized === "fast"
      ? "border-amber-500/20 bg-amber-500/10 text-amber-500"
      : "border-slate-500/20 bg-slate-500/10 text-slate-500";
  return (
    <Badge className={cn("h-5 rounded-full px-1.5 text-[10px] font-medium", toneClass)}>
      {normalized}
    </Badge>
  );
}
