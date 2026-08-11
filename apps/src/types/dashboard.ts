import type { ModelInfo } from "@/types/model";
import type { RequestLog } from "@/types/request-log";

export interface DashboardTokenUsage {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
  requestCount: number;
  successCount: number;
  errorCount: number;
}

export interface DashboardDailyUsagePoint {
  dayStartTs: number;
  dayEndTs: number;
  usage: DashboardTokenUsage;
}

export interface DashboardUserUsageSummary {
  userId: string;
  username: string | null;
  displayName: string | null;
  role: string | null;
  status: string | null;
  walletAvailableCreditMicros: number | null;
  todayUsage: DashboardTokenUsage;
  rangeUsage: DashboardTokenUsage;
}

export interface DashboardSourceUsageSummary {
  sourceKind: string;
  sourceId: string;
  name: string | null;
  status: string | null;
  provider: string | null;
  todayUsage: DashboardTokenUsage;
  rangeUsage: DashboardTokenUsage;
}

export interface DashboardAdminUsageSummary {
  rangeStartTs: number;
  rangeEndTs: number;
  todayStartTs: number;
  todayEndTs: number;
  todayUsage: DashboardTokenUsage;
  dailyUsage: DashboardDailyUsagePoint[];
  users: DashboardUserUsageSummary[];
  openaiAccounts: DashboardSourceUsageSummary[];
  kiroCredentials: DashboardSourceUsageSummary[];
  grokCredentials: DashboardSourceUsageSummary[];
  aggregateApis: DashboardSourceUsageSummary[];
}

export interface MemberDashboardWallet {
  id: string;
  balanceCreditMicros: number;
  frozenCreditMicros: number;
  availableCreditMicros: number;
  status: string;
  updatedAt: number;
}

export interface MemberDashboardApiKeySummary {
  totalCount: number;
  enabledCount: number;
  disabledCount: number;
  lastUsedAt: number | null;
}

export interface MemberDashboardUsageToday {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
  totalCount: number;
  successCount: number;
  errorCount: number;
  successRate: number | null;
}

export interface MemberDashboardUsagePoint {
  dayStartTs: number;
  dayEndTs: number;
  totalTokens: number;
  estimatedCostUsd: number;
}

export interface MemberDashboardKeyUsage {
  keyId: string;
  name: string | null;
  modelSlug: string | null;
  status: string;
  todayTokens: number;
  todayCostUsd: number;
  totalTokens: number;
  totalCostUsd: number;
  lastUsedAt: number | null;
}

export interface MemberDashboardModelUsage {
  model: string;
  totalTokens: number;
  estimatedCostUsd: number;
}

export interface MemberDashboardAlert {
  kind: string;
  severity: "info" | "warning" | "critical" | string;
  title: string;
  message: string;
  actionLabel: string | null;
  actionHref: string | null;
}

export interface MemberDashboardSummary {
  userId: string | null;
  distributionEnabled: boolean;
  wallet: MemberDashboardWallet | null;
  apiKeySummary: MemberDashboardApiKeySummary;
  usageToday: MemberDashboardUsageToday;
  usageTrend7d: MemberDashboardUsagePoint[];
  topKeys: MemberDashboardKeyUsage[];
  topModels: MemberDashboardModelUsage[];
  availableModels: ModelInfo[];
  recentLogs: RequestLog[];
  alerts: MemberDashboardAlert[];
}
