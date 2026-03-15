"use client";

import { Account, AccountUsage, AvailabilityKind, AvailabilityLevel, RequestLog } from "@/types";

const dateTimeFormatter = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

const COMPACT_NUMBER_UNITS = [
  { value: 1e18, suffix: "E" },
  { value: 1e15, suffix: "P" },
  { value: 1e12, suffix: "T" },
  { value: 1e9, suffix: "B" },
  { value: 1e6, suffix: "M" },
  { value: 1e3, suffix: "K" },
];

export function toNullableNumber(value: unknown): number | null {
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === "string") {
    const normalized = value.trim();
    if (!normalized) return null;
    const parsed = Number(normalized);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

export function formatTsFromSeconds(
  timestamp: number | null | undefined,
  emptyLabel = "未知"
): string {
  if (!timestamp) return emptyLabel;
  const date = new Date(timestamp * 1000);
  if (Number.isNaN(date.getTime())) return emptyLabel;
  return dateTimeFormatter.format(date);
}

function trimTrailingZeros(text: string): string {
  return text.replace(/\.0+$/, "").replace(/(\.\d*[1-9])0+$/, "$1");
}

export function formatCompactNumber(
  value: number | null | undefined,
  fallback = "-",
  maxFractionDigits = 1
): string {
  const parsed = toNullableNumber(value);
  if (parsed == null) return fallback;

  const normalized = Math.max(0, parsed);
  if (normalized < 1000) {
    return `${Math.round(normalized)}`;
  }

  for (const unit of COMPACT_NUMBER_UNITS) {
    if (normalized < unit.value) continue;
    const scaled = normalized / unit.value;
    return `${trimTrailingZeros(scaled.toFixed(maxFractionDigits))}${unit.suffix}`;
  }

  return `${Math.round(normalized)}`;
}

function isInactiveAccount(account?: { status?: string } | null): boolean {
  return String(account?.status || "").trim().toLowerCase() === "inactive";
}

export function remainingPercent(value: number | null | undefined): number | null {
  const parsed = toNullableNumber(value);
  if (parsed == null) return null;
  return Math.max(0, Math.min(100, Math.round(100 - parsed)));
}

export function calcAvailability(
  usage?: Partial<AccountUsage> | null,
  account?: { status?: string } | null
): { text: string; level: AvailabilityLevel; kind: AvailabilityKind } {
  const inactive = isInactiveAccount(account);
  const normalizedStatus = String(usage?.availabilityStatus || "")
    .trim()
    .toLowerCase();
  if (normalizedStatus === "unavailable") {
    return { text: "不可用", level: "bad", kind: "unavailable" };
  }
  if (!usage) {
    return inactive
      ? { text: "已失效", level: "bad", kind: "expired" }
      : { text: "未知", level: "unknown", kind: "unknown" };
  }
  if (normalizedStatus === "unknown") {
    return inactive
      ? { text: "已失效", level: "bad", kind: "expired" }
      : { text: "未知", level: "unknown", kind: "unknown" };
  }

  const primaryMissing =
    toNullableNumber(usage.usedPercent) == null ||
    toNullableNumber(usage.windowMinutes) == null;
  const secondaryPresent =
    toNullableNumber(usage.secondaryUsedPercent) != null ||
    toNullableNumber(usage.secondaryWindowMinutes) != null;
  const secondaryMissing =
    toNullableNumber(usage.secondaryUsedPercent) == null ||
    toNullableNumber(usage.secondaryWindowMinutes) == null;

  if (primaryMissing) {
    return inactive
      ? { text: "已失效", level: "bad", kind: "expired" }
      : { text: "用量缺失", level: "bad", kind: "unknown" };
  }
  if ((usage.usedPercent ?? 0) >= 100) {
    return { text: "不可用", level: "warn", kind: "unavailable" };
  }
  if (!secondaryPresent) {
    if (inactive) {
      return { text: "已失效", level: "bad", kind: "expired" };
    }
    if (normalizedStatus === "primary_window_available_only") {
      return { text: "单窗口可用", level: "ok", kind: "available" };
    }
    return { text: "单窗口可用", level: "ok", kind: "available" };
  }
  if (secondaryMissing) {
    return inactive
      ? { text: "已失效", level: "bad", kind: "expired" }
      : { text: "用量缺失", level: "bad", kind: "unknown" };
  }
  if ((usage.secondaryUsedPercent ?? 0) >= 100) {
    return { text: "不可用", level: "bad", kind: "unavailable" };
  }
  if (inactive) {
    return { text: "已失效", level: "bad", kind: "expired" };
  }
  if (normalizedStatus === "available") {
    return { text: "可用", level: "ok", kind: "available" };
  }
  if (normalizedStatus === "primary_window_available_only") {
    return { text: "单窗口可用", level: "ok", kind: "available" };
  }
  return { text: "可用", level: "ok", kind: "available" };
}

export function isLowQuotaUsage(usage?: Partial<AccountUsage> | null): boolean {
  const primaryRemain = remainingPercent(usage?.usedPercent);
  const secondaryRemain = remainingPercent(usage?.secondaryUsedPercent);
  return (
    (primaryRemain != null && primaryRemain <= 20) ||
    (secondaryRemain != null && secondaryRemain <= 20)
  );
}

export function canParticipateInRouting(level: AvailabilityLevel): boolean {
  return level !== "warn" && level !== "bad";
}

export function countsTowardPoolRemain(kind: AvailabilityKind): boolean {
  return kind === "available" || kind === "unavailable";
}

export function pickCurrentAccount(
  accounts: Account[],
  requestLogs: RequestLog[],
  manualPreferredAccountId?: string
): Account | null {
  if (!accounts.length) return null;

  const preferredId = String(manualPreferredAccountId || "").trim();
  if (preferredId) {
    const preferred = accounts.find((item) => item.id === preferredId);
    if (preferred && canParticipateInRouting(preferred.availabilityLevel)) {
      return preferred;
    }
  }

  let latestHit: RequestLog | null = null;
  for (const item of requestLogs) {
    if (!item.accountId) continue;
    if (!latestHit || (item.createdAt ?? 0) > (latestHit.createdAt ?? 0)) {
      latestHit = item;
    }
  }
  if (latestHit) {
    const fromLogs = accounts.find((item) => item.id === latestHit.accountId);
    if (fromLogs && canParticipateInRouting(fromLogs.availabilityLevel)) {
      return fromLogs;
    }
  }

  return (
    accounts.find((item) => canParticipateInRouting(item.availabilityLevel)) ||
    (preferredId ? accounts.find((item) => item.id === preferredId) : null) ||
    accounts[0] ||
    null
  );
}

export function pickBestRecommendations(accounts: Account[]): {
  primaryPick: Account | null;
  secondaryPick: Account | null;
} {
  let primaryPick: Account | null = null;
  let secondaryPick: Account | null = null;

  for (const account of accounts) {
    if (!canParticipateInRouting(account.availabilityLevel)) {
      continue;
    }
    if (
      account.primaryRemainPercent != null &&
      (!primaryPick ||
        (primaryPick.primaryRemainPercent ?? -1) < account.primaryRemainPercent)
    ) {
      primaryPick = account;
    }
    if (
      account.secondaryRemainPercent != null &&
      (!secondaryPick ||
        (secondaryPick.secondaryRemainPercent ?? -1) < account.secondaryRemainPercent)
    ) {
      secondaryPick = account;
    }
  }

  return { primaryPick, secondaryPick };
}
