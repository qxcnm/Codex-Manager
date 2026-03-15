import test from "node:test";
import assert from "node:assert/strict";
import { computeAvailablePoolRemain } from "../src/lib/utils/pool-remain";
import type { Account } from "../src/types";

function createAccount(
  overrides: Partial<Account> = {},
): Account {
  return {
    id: overrides.id || "acc-1",
    name: overrides.name || "Account",
    group: overrides.group || "默认",
    priority: overrides.priority ?? 0,
    label: overrides.label || overrides.name || "Account",
    groupName: overrides.groupName || overrides.group || "默认",
    sort: overrides.sort ?? 0,
    status: overrides.status || "active",
    isAvailable: overrides.isAvailable ?? true,
    availabilityKind: overrides.availabilityKind || "available",
    isLowQuota: overrides.isLowQuota ?? false,
    lastRefreshAt: overrides.lastRefreshAt ?? null,
    availabilityText: overrides.availabilityText || "可用",
    availabilityLevel: overrides.availabilityLevel || "ok",
    primaryRemainPercent: overrides.primaryRemainPercent ?? null,
    secondaryRemainPercent: overrides.secondaryRemainPercent ?? null,
    usage: overrides.usage ?? null,
  };
}

test("computeAvailablePoolRemain uses available balance over available plus unavailable total capacity", () => {
  const result = computeAvailablePoolRemain([
    createAccount({
      id: "acc-available-1",
      isAvailable: true,
      availabilityLevel: "ok",
      usage: {
        accountId: "acc-available-1",
        availabilityStatus: "available",
        usedPercent: 20,
        windowMinutes: 300,
        resetsAt: null,
        secondaryUsedPercent: 40,
        secondaryWindowMinutes: 10_080,
        secondaryResetsAt: null,
        creditsJson: null,
        capturedAt: null,
      },
    }),
    createAccount({
      id: "acc-unavailable",
      isAvailable: false,
      availabilityLevel: "bad",
      availabilityKind: "unavailable",
      availabilityText: "不可用",
      usage: {
        accountId: "acc-unavailable",
        availabilityStatus: "unavailable",
        usedPercent: 95,
        windowMinutes: 300,
        resetsAt: null,
        secondaryUsedPercent: 90,
        secondaryWindowMinutes: 10_080,
        secondaryResetsAt: null,
        creditsJson: null,
        capturedAt: null,
      },
    }),
    createAccount({
      id: "acc-expired",
      isAvailable: false,
      availabilityLevel: "bad",
      availabilityKind: "expired",
      availabilityText: "已失效",
      status: "inactive",
      usage: {
        accountId: "acc-expired",
        availabilityStatus: "unavailable",
        usedPercent: 10,
        windowMinutes: 300,
        resetsAt: null,
        secondaryUsedPercent: 15,
        secondaryWindowMinutes: 10_080,
        secondaryResetsAt: null,
        creditsJson: null,
        capturedAt: null,
      },
    }),
    createAccount({
      id: "acc-available-2",
      isAvailable: true,
      availabilityLevel: "ok",
      usage: {
        accountId: "acc-available-2",
        availabilityStatus: "available",
        usedPercent: 60,
        windowMinutes: 300,
        resetsAt: null,
        secondaryUsedPercent: 50,
        secondaryWindowMinutes: 10_080,
        secondaryResetsAt: null,
        creditsJson: null,
        capturedAt: null,
      },
    }),
  ]);

  assert.deepEqual(result, {
    primaryBucketCount: 3,
    primaryKnownCount: 3,
    primaryUnknownCount: 0,
    primaryRemainPercent: 40,
    secondaryBucketCount: 3,
    secondaryKnownCount: 3,
    secondaryUnknownCount: 0,
    secondaryRemainPercent: 37,
  });
});
