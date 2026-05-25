import type { AvailabilityLevel } from "@/types/runtime";

export interface AccountUsage {
  accountId: string;
  availabilityStatus: string;
  usedPercent: number | null;
  windowMinutes: number | null;
  resetsAt: number | null;
  secondaryUsedPercent: number | null;
  secondaryWindowMinutes: number | null;
  secondaryResetsAt: number | null;
  creditsJson: string | null;
  capturedAt: number | null;
}

export interface Account {
  id: string;
  name: string;
  group: string;
  priority: number;
  preferred: boolean;
  label: string;
  groupName: string;
  sort: number;
  status: string;
  statusReason: string;
  hasToken: boolean;
  planType: string | null;
  planTypeRaw: string | null;
  hasSubscription: boolean | null;
  subscriptionPlan: string | null;
  subscriptionExpiresAt: number | null;
  subscriptionRenewsAt: number | null;
  note: string | null;
  tags: string[];
  modelSlugs: string[];
  quotaCapacityPrimaryWindowTokens: number | null;
  quotaCapacitySecondaryWindowTokens: number | null;
  isAvailable: boolean;
  isLowQuota: boolean;
  lastRefreshAt: number | null;
  availabilityText: string;
  availabilityLevel: AvailabilityLevel;
  primaryRemainPercent: number | null;
  secondaryRemainPercent: number | null;
  usage: AccountUsage | null;
}

export interface AccountListResult {
  items: Account[];
  total: number;
  page: number;
  pageSize: number;
}

export interface LocalCodexAccountStatus {
  activeAccountId: string | null;
  liveAuthPresent: boolean;
  liveAuthPath: string;
}

export interface LocalCodexAccountPoolSwitchResult {
  success: boolean;
  activeAccountId: string | null;
  backupPath: string | null;
  warning: string | null;
  status: LocalCodexAccountStatus;
}

export interface UsageAggregateSummary {
  primaryBucketCount: number;
  primaryKnownCount: number;
  primaryUnknownCount: number;
  primaryRemainPercent: number | null;
  secondaryBucketCount: number;
  secondaryKnownCount: number;
  secondaryUnknownCount: number;
  secondaryRemainPercent: number | null;
}
