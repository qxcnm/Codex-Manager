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

export interface ResetEntitlementDisplayRow {
  id: string;
  label: string;
  count: number | null;
  expiresAt: number | null;
  activatesAt: number | null;
  manualActivationRequired: boolean;
  appliesTo: string[];
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
  authMode: "oauth" | "agentIdentity";
  agentIdentityStatus: string | null;
  hasAgentIdentityTask: boolean;
  credentialState:
    | "healthy"
    | "access_token_expired"
    | "access_token_rejected"
    | "refresh_token_expired"
    | "refresh_token_revoked"
    | "reauth_required"
    | "reauth_in_progress"
    | "account_deactivated"
    | "workspace_deactivated"
    | "network_unknown"
    | "credential_unknown"
    | "stopped_unknown";
  credentialAction: "none" | "refresh" | "reauthenticate" | "stop" | "retry_network";
  accessTokenExpiresAt: number | null;
  gatewayProbeStatus: string | null;
  gatewayProbeReason: string | null;
  gatewayProbeCheckedAt: number | null;
  gatewayProbeRetryAfter: number | null;
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
  createdAt: number;
  updatedAt: number;
  availabilityText: string;
  availabilityLevel: AvailabilityLevel;
  primaryRemainPercent: number | null;
  secondaryRemainPercent: number | null;
  resetEntitlements: ResetEntitlementDisplayRow[];
  usage: AccountUsage | null;
}

export interface AccountListResult {
  items: Account[];
  total: number;
  page: number;
  pageSize: number;
}

export interface CredentialRepairReportResult {
  accountId: string;
  outcome: string;
  status: string;
  statusReason: string;
  terminal: boolean;
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
