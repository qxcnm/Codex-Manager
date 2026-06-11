"use client";

import { formatLocalDateTimeFromSeconds } from "@/lib/utils/time";
import { Account, AccountUsage, AvailabilityLevel, RequestLog } from "@/types";

const COMPACT_NUMBER_UNITS = [
  { value: 1e18, suffix: "E" },
  { value: 1e15, suffix: "P" },
  { value: 1e12, suffix: "T" },
  { value: 1e9, suffix: "B" },
  { value: 1e6, suffix: "M" },
  { value: 1e3, suffix: "K" },
];
const MINUTES_PER_HOUR = 60;
const MINUTES_PER_DAY = 24 * MINUTES_PER_HOUR;
const WINDOW_ROUNDING_BIAS_MINUTES = 3;
const EXTRA_RATE_LIMITS_JSON_KEY = "_codexmanager_extra_rate_limits";

type UsageWindowDisplayMode = "primary-only" | "secondary-only" | "dual" | "unknown";
type TranslationValues = Record<string, string | number>;
export interface ExtraUsageDisplayRow {
  id: string;
  label: string;
  labelValues?: TranslationValues;
  labelSuffix?: string;
  remainPercent: number | null;
  resetsAt: number | null;
  windowLabel: string;
  windowLabelValues?: TranslationValues;
}

/**
 * 函数 `toNullableNumber`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
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

/**
 * 函数 `formatTsFromSeconds`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - timestamp: 参数 timestamp
 * - emptyLabel: 参数 emptyLabel
 *
 * # 返回
 * 返回函数执行结果
 */
export function formatTsFromSeconds(
  timestamp: number | null | undefined,
  emptyLabel = "未知"
): string {
  return formatLocalDateTimeFromSeconds(timestamp, emptyLabel);
}

export function formatRemainingDurationFromSeconds(
  timestamp: number | null | undefined,
  mode: "hours" | "days" = "hours",
  emptyLabel = "未知",
  locale = "zh-CN",
): string {
  if (!timestamp) return emptyLabel;
  const diffSeconds = Math.max(0, timestamp - Math.floor(Date.now() / 1000));
  const totalMinutes = Math.ceil(diffSeconds / 60);
  const normalizedLocale = String(locale || "")
    .trim()
    .toLowerCase();
  const unitLabels =
    normalizedLocale === "ru" || normalizedLocale === "ru-ru"
      ? { day: "д", hour: "ч", minute: "мин", second: "с" }
      : normalizedLocale === "en" || normalizedLocale === "en-us" || normalizedLocale === "en-gb"
        ? { day: "d", hour: "h", minute: "min", second: "s" }
        : normalizedLocale === "ko" || normalizedLocale === "ko-kr"
          ? { day: "일", hour: "시간", minute: "분", second: "초" }
          : { day: "天", hour: "时", minute: "分", second: "秒" };
  const formatUnit = (
    value: number,
    unit: keyof typeof unitLabels,
    pad = false,
  ) => `${pad ? String(value).padStart(2, "0") : String(value)} ${unitLabels[unit]}`;

  if (diffSeconds < 10 * 60) {
    const minutes = Math.floor(diffSeconds / 60);
    const seconds = diffSeconds % 60;
    if (mode === "hours") {
      const hours = Math.floor(minutes / MINUTES_PER_HOUR);
      const remainMinutes = minutes % MINUTES_PER_HOUR;
      if (hours > 0) {
        return [
          formatUnit(hours, "hour"),
          formatUnit(remainMinutes, "minute", true),
          formatUnit(seconds, "second", true),
        ].join(" ");
      }
      return [formatUnit(remainMinutes, "minute"), formatUnit(seconds, "second", true)].join(" ");
    }
    if (minutes > 0) {
      return [formatUnit(minutes, "minute"), formatUnit(seconds, "second", true)].join(" ");
    }
    return formatUnit(seconds, "second");
  }

  if (mode === "hours") {
    const hours = Math.floor(totalMinutes / MINUTES_PER_HOUR);
    const minutes = totalMinutes % MINUTES_PER_HOUR;
    return [formatUnit(hours, "hour"), formatUnit(minutes, "minute", true)].join(" ");
  }

  const days = Math.floor(totalMinutes / MINUTES_PER_DAY);
  const hours = Math.floor((totalMinutes % MINUTES_PER_DAY) / MINUTES_PER_HOUR);
  const minutes = totalMinutes % MINUTES_PER_HOUR;
  const parts: string[] = [];

  if (days > 0) {
    parts.push(formatUnit(days, "day"));
  }
  if (hours > 0) {
    parts.push(formatUnit(hours, "hour", days > 0));
  }
  if (parts.length > 0) {
    parts.push(formatUnit(minutes, "minute", true));
  } else {
    parts.push(formatUnit(minutes, "minute"));
  }

  return parts.join(" ");
}

/**
 * 函数 `trimTrailingZeros`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - text: 参数 text
 *
 * # 返回
 * 返回函数执行结果
 */
function trimTrailingZeros(text: string): string {
  return text.replace(/\.0+$/, "").replace(/(\.\d*[1-9])0+$/, "$1");
}

/**
 * 函数 `formatCompactNumber`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 * - fallback: 参数 fallback
 * - maxFractionDigits: 参数 maxFractionDigits
 *
 * # 返回
 * 返回函数执行结果
 */
export function formatCompactNumber(
  value: number | null | undefined,
  fallback = "-",
  maxFractionDigits = 1,
  preserveTrailingZeros = false
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
    const fixed = scaled.toFixed(maxFractionDigits);
    return `${preserveTrailingZeros ? fixed : trimTrailingZeros(fixed)}${unit.suffix}`;
  }

  return `${Math.round(normalized)}`;
}

/**
 * 函数 `normalizedAccountStatus`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - account?: 参数 account?
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizedAccountStatus(account?: { status?: string } | null): string {
  return String(account?.status || "").trim().toLowerCase();
}

/**
 * 函数 `normalizedAccountStatusReason`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - account?: 参数 account?
 *
 * # 返回
 * 返回函数执行结果
 */
function normalizedAccountStatusReason(
  account?: { statusReason?: string } | null
): string {
  return String(account?.statusReason || "").trim().toLowerCase();
}

/**
 * 函数 `isDisabledAccount`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - account?: 参数 account?
 *
 * # 返回
 * 返回函数执行结果
 */
function isDisabledAccount(account?: { status?: string } | null): boolean {
  return normalizedAccountStatus(account) === "disabled";
}

/**
 * 函数 `isRecoveryRequiredAccount`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - account?: 参数 account?
 *
 * # 返回
 * 返回函数执行结果
 */
function isRecoveryRequiredAccount(account?: { status?: string } | null): boolean {
  return normalizedAccountStatus(account) === "inactive";
}

/**
 * 函数 `isUnavailableAccount`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - account?: 参数 account?
 *
 * # 返回
 * 返回函数执行结果
 */
function isUnavailableAccount(account?: { status?: string } | null): boolean {
  return normalizedAccountStatus(account) === "unavailable";
}

export function isLimitedAccount(account?: { status?: string } | null): boolean {
  return normalizedAccountStatus(account) === "limited";
}

/**
 * 函数 `isBannedAccount`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - account?: 参数 account?
 *
 * # 返回
 * 返回函数执行结果
 */
export function isBannedAccount(
  account?: { status?: string; statusReason?: string } | null
): boolean {
  const status = normalizedAccountStatus(account);
  if (status !== "banned" && status !== "unavailable") {
    return false;
  }
  const reason = normalizedAccountStatusReason(account);
  return (
    status === "banned" ||
    reason === "account_deactivated" ||
    reason === "workspace_deactivated" ||
    reason === "deactivated_workspace"
  );
}

/**
 * 函数 `remainingPercent`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
export function remainingPercent(value: number | null | undefined): number | null {
  const parsed = toNullableNumber(value);
  if (parsed == null) return null;
  return Math.max(0, Math.min(100, Math.round(100 - parsed)));
}

/**
 * 函数 `hasSecondarySignal`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usage?: 参数 usage?
 *
 * # 返回
 * 返回函数执行结果
 */
function hasSecondarySignal(usage?: Partial<AccountUsage> | null): boolean {
  return (
    toNullableNumber(usage?.secondaryUsedPercent) != null ||
    toNullableNumber(usage?.secondaryWindowMinutes) != null
  );
}

/**
 * 函数 `isLongWindow`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - windowMinutes: 参数 windowMinutes
 *
 * # 返回
 * 返回函数执行结果
 */
function isLongWindow(windowMinutes: number | null | undefined): boolean {
  const parsed = toNullableNumber(windowMinutes);
  return parsed != null && parsed > MINUTES_PER_DAY + WINDOW_ROUNDING_BIAS_MINUTES;
}

/**
 * 函数 `parseCreditsJson`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - raw: 参数 raw
 *
 * # 返回
 * 返回函数执行结果
 */
function parseCreditsJson(raw: string | null | undefined): unknown | null {
  const text = String(raw || "").trim();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function asObjectRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function humanizeExtraRateLimitLabel(raw: string): string {
  const normalized = raw.trim().toLowerCase();
  if (!normalized) return "额外额度";
  if (normalized.includes("spark") || normalized === "codex_other") return "Spark 额度";
  if (normalized.includes("code_review") || normalized.includes("code review")) {
    return "Code Review 额度";
  }

  return raw
    .replace(/_rate_limit$/i, "")
    .replace(/[_-]+/g, " ")
    .split(" ")
    .map((part) => (part ? `${part[0].toUpperCase()}${part.slice(1)}` : ""))
    .join(" ")
    .trim() || "额外额度";
}

function formatWindowLabel(
  windowMinutes: number | null
): { label: string; values?: TranslationValues } {
  if (windowMinutes == null || windowMinutes <= 0) {
    return { label: "独立窗口" };
  }
  if (windowMinutes % MINUTES_PER_DAY === 0) {
    const days = windowMinutes / MINUTES_PER_DAY;
    return { label: "{count}天窗口", values: { count: days } };
  }
  if (windowMinutes % MINUTES_PER_HOUR === 0) {
    const hours = windowMinutes / MINUTES_PER_HOUR;
    return { label: "{count}小时窗口", values: { count: hours } };
  }
  return { label: "{count}分钟窗口", values: { count: windowMinutes } };
}

function extractExtraRateLimitWindows(raw: string | null | undefined): ExtraUsageDisplayRow[] {
  const credits = parseCreditsJson(raw);
  const payload = asObjectRecord(credits);
  const items = Array.isArray(payload?.[EXTRA_RATE_LIMITS_JSON_KEY])
    ? (payload?.[EXTRA_RATE_LIMITS_JSON_KEY] as unknown[])
    : [];

  return items.reduce<ExtraUsageDisplayRow[]>((rows, item, index) => {
    const source = asObjectRecord(item);
    if (!source) return rows;

    const labelSeed =
      (typeof source.limit_name === "string" && source.limit_name.trim()) ||
      (typeof source.limit_id === "string" && source.limit_id.trim()) ||
      (typeof source.source_key === "string" && source.source_key.trim()) ||
      `extra-${index + 1}`;
    const baseLabel = humanizeExtraRateLimitLabel(labelSeed);
    const windows = [
      { key: "primary_window" },
      { key: "secondary_window", suffix: " · 长周期" },
    ];

    for (const { key, suffix } of windows) {
      const window = asObjectRecord(source[key]);
      if (!window) continue;

      const remainPercent = remainingPercent(toNullableNumber(window.used_percent));
      const resetsAt = toNullableNumber(window.reset_at);
      const windowSeconds = toNullableNumber(window.limit_window_seconds);
      const minutes = windowSeconds == null ? null : Math.max(1, Math.ceil(windowSeconds / 60));
      if (remainPercent == null && resetsAt == null && minutes == null) {
        continue;
      }

      const windowLabel = formatWindowLabel(minutes);
      rows.push({
        id: `${labelSeed}-${key}-${index}`,
        label: baseLabel,
        labelSuffix: suffix,
        remainPercent,
        resetsAt,
        windowLabel: windowLabel.label,
        windowLabelValues: windowLabel.values,
      });
    }

    return rows;
  }, []);
}

/**
 * 函数 `extractPlanTypeRecursive`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - value: 参数 value
 *
 * # 返回
 * 返回函数执行结果
 */
function extractPlanTypeRecursive(value: unknown): string | null {
  if (Array.isArray(value)) {
    for (const item of value) {
      const nested = extractPlanTypeRecursive(item);
      if (nested) return nested;
    }
    return null;
  }

  if (!value || typeof value !== "object") {
    return null;
  }

  const source = value as Record<string, unknown>;
  for (const key of [
    "plan_type",
    "planType",
    "subscription_tier",
    "subscriptionTier",
    "tier",
    "account_type",
    "accountType",
    "type",
  ]) {
    const text = typeof source[key] === "string" ? source[key].trim().toLowerCase() : "";
    if (text) return text;
  }

  for (const nested of Object.values(source)) {
    const result = extractPlanTypeRecursive(nested);
    if (result) return result;
  }

  return null;
}

/**
 * 函数 `isFreePlanUsage`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - raw: 参数 raw
 *
 * # 返回
 * 返回函数执行结果
 */
function isFreePlanUsage(raw: string | null | undefined): boolean {
  const credits = parseCreditsJson(raw);
  const planType = extractPlanTypeRecursive(credits);
  return Boolean(planType && planType.includes("free"));
}

/**
 * 函数 `getUsageWindowDisplayMode`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usage?: 参数 usage?
 *
 * # 返回
 * 返回函数执行结果
 */
export function getUsageWindowDisplayMode(
  usage?: Partial<AccountUsage> | null
): UsageWindowDisplayMode {
  const hasPrimarySignal =
    toNullableNumber(usage?.usedPercent) != null || toNullableNumber(usage?.windowMinutes) != null;
  const secondarySignal = hasSecondarySignal(usage);

  if (!hasPrimarySignal && !secondarySignal) {
    return "unknown";
  }
  if (
    hasPrimarySignal &&
    !secondarySignal &&
    (isLongWindow(usage?.windowMinutes) || isFreePlanUsage(usage?.creditsJson))
  ) {
    return "secondary-only";
  }
  if (hasPrimarySignal && !secondarySignal) {
    return "primary-only";
  }
  return "dual";
}

/**
 * 函数 `getUsageDisplayBuckets`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usage?: 参数 usage?
 *
 * # 返回
 * 返回函数执行结果
 */
export function getUsageDisplayBuckets(usage?: Partial<AccountUsage> | null): {
  mode: UsageWindowDisplayMode;
  primaryRemainPercent: number | null;
  primaryResetsAt: number | null;
  secondaryRemainPercent: number | null;
  secondaryResetsAt: number | null;
} {
  const mode = getUsageWindowDisplayMode(usage);
  if (mode === "secondary-only") {
    return {
      mode,
      primaryRemainPercent: null,
      primaryResetsAt: null,
      secondaryRemainPercent: remainingPercent(usage?.usedPercent),
      secondaryResetsAt: toNullableNumber(usage?.resetsAt),
    };
  }

  return {
    mode,
    primaryRemainPercent: remainingPercent(usage?.usedPercent),
    primaryResetsAt: toNullableNumber(usage?.resetsAt),
    secondaryRemainPercent: remainingPercent(usage?.secondaryUsedPercent),
    secondaryResetsAt: toNullableNumber(usage?.secondaryResetsAt),
  };
}

export function getExtraUsageDisplayRows(
  usage?: Partial<AccountUsage> | null
): ExtraUsageDisplayRow[] {
  return extractExtraRateLimitWindows(usage?.creditsJson);
}

/**
 * 函数 `calcAvailability`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usage?: 参数 usage?
 * - account?: 参数 account?
 *
 * # 返回
 * 返回函数执行结果
 */
export function calcAvailability(
  usage?: Partial<AccountUsage> | null,
  account?: { status?: string; statusReason?: string; hasToken?: boolean } | null
): { text: string; level: AvailabilityLevel } {
  const primaryExhausted = (usage?.usedPercent ?? 0) >= 100;
  const secondaryExhausted = (usage?.secondaryUsedPercent ?? 0) >= 100;

  if (isDisabledAccount(account)) {
    return { text: "已禁用", level: "bad" };
  }
  if (isRecoveryRequiredAccount(account)) {
    return { text: "不可用", level: "bad" };
  }
  if (isBannedAccount(account)) {
    return { text: "封禁", level: "bad" };
  }
  if (isLimitedAccount(account)) {
    return { text: "限流", level: "bad" };
  }
  if (isUnavailableAccount(account)) {
    return { text: "不可用", level: "bad" };
  }
  if (account?.hasToken === false) {
    return { text: "缺少授权 Token", level: "bad" };
  }
  if (!usage) {
    return { text: "未知", level: "unknown" };
  }

  const normalizedStatus = String(usage.availabilityStatus || "")
    .trim()
    .toLowerCase();
  const displayMode = getUsageWindowDisplayMode(usage);
  if (normalizedStatus === "available") {
    return { text: "可用", level: "ok" };
  }
  if (normalizedStatus === "primary_window_available_only") {
    return {
      text: displayMode === "secondary-only" ? "仅7天额度" : "7天窗口未提供",
      level: "ok",
    };
  }
  if (normalizedStatus === "unavailable") {
    if (primaryExhausted || secondaryExhausted) {
      return { text: "限流", level: "bad" };
    }
    return { text: "不可用", level: "bad" };
  }
  if (normalizedStatus === "unknown") {
    return { text: "未知", level: "unknown" };
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

  if (primaryMissing) return { text: "用量缺失", level: "bad" };
  if (primaryExhausted) {
    return { text: "限流", level: "bad" };
  }
  if (!secondaryPresent) {
    return {
      text: displayMode === "secondary-only" ? "仅7天额度" : "7天窗口未提供",
      level: "ok",
    };
  }
  if (secondaryMissing) {
    return { text: "用量缺失", level: "bad" };
  }
  if (secondaryExhausted) {
    return { text: "限流", level: "bad" };
  }
  return { text: "可用", level: "ok" };
}

/**
 * 函数 `isPrimaryWindowOnlyUsage`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usage?: 参数 usage?
 *
 * # 返回
 * 返回函数执行结果
 */
export function isPrimaryWindowOnlyUsage(
  usage?: Partial<AccountUsage> | null
): boolean {
  return getUsageWindowDisplayMode(usage) === "primary-only";
}

/**
 * 函数 `isSecondaryWindowOnlyUsage`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usage?: 参数 usage?
 *
 * # 返回
 * 返回函数执行结果
 */
export function isSecondaryWindowOnlyUsage(
  usage?: Partial<AccountUsage> | null
): boolean {
  return getUsageWindowDisplayMode(usage) === "secondary-only";
}

/**
 * 函数 `isLowQuotaUsage`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - usage?: 参数 usage?
 *
 * # 返回
 * 返回函数执行结果
 */
export function isLowQuotaUsage(usage?: Partial<AccountUsage> | null): boolean {
  const buckets = getUsageDisplayBuckets(usage);
  const primaryRemain = buckets.primaryRemainPercent;
  const secondaryRemain = buckets.secondaryRemainPercent;
  return (
    (primaryRemain != null && primaryRemain > 0 && primaryRemain <= 20) ||
    (secondaryRemain != null && secondaryRemain > 0 && secondaryRemain <= 20)
  );
}

/**
 * 函数 `canParticipateInRouting`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - level: 参数 level
 *
 * # 返回
 * 返回函数执行结果
 */
export function canParticipateInRouting(level: AvailabilityLevel): boolean {
  return level !== "warn" && level !== "bad";
}

/**
 * 函数 `pickCurrentAccount`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - accounts: 参数 accounts
 * - requestLogs: 参数 requestLogs
 *
 * # 返回
 * 返回函数执行结果
 */
export function pickCurrentAccount(
  accounts: Account[],
  requestLogs: RequestLog[]
): Account | null {
  if (!accounts.length) return null;

  const preferred = accounts.find((item) => item.preferred);
  if (preferred && canParticipateInRouting(preferred.availabilityLevel)) {
    return preferred;
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
    preferred ||
    accounts[0] ||
    null
  );
}

/**
 * 函数 `pickBestRecommendations`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - accounts: 参数 accounts
 *
 * # 返回
 * 返回函数执行结果
 */
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
