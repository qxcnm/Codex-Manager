import type { Account, UsageAggregateSummary } from "@/types";
import { countsTowardPoolRemain, remainingPercent } from "./usage";

const MINUTES_PER_HOUR = 60;
const MINUTES_PER_DAY = 24 * MINUTES_PER_HOUR;
const ROUNDING_BIAS = 3;

function isLongWindow(windowMinutes: number | null | undefined): boolean {
  return typeof windowMinutes === "number" && windowMinutes > MINUTES_PER_DAY + ROUNDING_BIAS;
}

function extractPlanType(value: unknown): string {
  if (Array.isArray(value)) {
    for (const item of value) {
      const planType = extractPlanType(item);
      if (planType) return planType;
    }
    return "";
  }

  if (!value || typeof value !== "object") {
    return "";
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
    const current = source[key];
    if (typeof current === "string" && current.trim()) {
      return current.trim().toLowerCase();
    }
  }

  for (const nested of Object.values(source)) {
    const planType = extractPlanType(nested);
    if (planType) return planType;
  }

  return "";
}

function isFreePlanUsage(creditsJson: string | null | undefined): boolean {
  const text = String(creditsJson || "").trim();
  if (!text) return false;

  try {
    return extractPlanType(JSON.parse(text)).includes("free");
  } catch {
    return false;
  }
}

export function computeAvailablePoolRemain(accounts: Account[]): UsageAggregateSummary {
  let primaryBucketCount = 0;
  let primaryKnownCount = 0;
  let primaryRemainingTotal = 0;
  let secondaryBucketCount = 0;
  let secondaryKnownCount = 0;
  let secondaryRemainingTotal = 0;

  for (const account of accounts) {
    if (!countsTowardPoolRemain(account.availabilityKind)) {
      continue;
    }
    const contributesAvailableBalance = account.availabilityKind === "available";

    const usage = account.usage;
    const hasPrimarySignal = usage?.usedPercent != null || usage?.windowMinutes != null;
    const hasSecondarySignal =
      usage?.secondaryUsedPercent != null || usage?.secondaryWindowMinutes != null;
    const primaryBelongsToSecondary =
      !hasSecondarySignal &&
      (isLongWindow(usage?.windowMinutes) || isFreePlanUsage(usage?.creditsJson));

    if (hasPrimarySignal) {
      if (primaryBelongsToSecondary) {
        secondaryBucketCount += 1;
      } else {
        primaryBucketCount += 1;
      }
    }

    const primaryRemain = remainingPercent(usage?.usedPercent);
    if (primaryRemain != null) {
      if (primaryBelongsToSecondary) {
        secondaryKnownCount += 1;
        if (contributesAvailableBalance) {
          secondaryRemainingTotal += primaryRemain;
        }
      } else {
        primaryKnownCount += 1;
        if (contributesAvailableBalance) {
          primaryRemainingTotal += primaryRemain;
        }
      }
    }

    if (hasSecondarySignal) {
      secondaryBucketCount += 1;
    }

    const secondaryRemain = remainingPercent(usage?.secondaryUsedPercent);
    if (secondaryRemain != null) {
      secondaryKnownCount += 1;
      if (contributesAvailableBalance) {
        secondaryRemainingTotal += secondaryRemain;
      }
    }
  }

  return {
    primaryBucketCount,
    primaryKnownCount,
    primaryUnknownCount: Math.max(0, primaryBucketCount - primaryKnownCount),
    primaryRemainPercent:
      primaryKnownCount > 0 ? Math.round(primaryRemainingTotal / primaryKnownCount) : null,
    secondaryBucketCount,
    secondaryKnownCount,
    secondaryUnknownCount: Math.max(0, secondaryBucketCount - secondaryKnownCount),
    secondaryRemainPercent:
      secondaryKnownCount > 0
        ? Math.round(secondaryRemainingTotal / secondaryKnownCount)
        : null,
  };
}
