import { invoke, withAddr } from "./transport";
import { normalizeModelCatalog, normalizeRequestLogs } from "./normalize";
import type {
  DashboardAdminUsageSummary,
  DashboardDailyUsagePoint,
  DashboardSourceUsageSummary,
  DashboardTokenUsage,
  DashboardUserUsageSummary,
  MemberDashboardAlert,
  MemberDashboardApiKeySummary,
  MemberDashboardKeyUsage,
  MemberDashboardModelUsage,
  MemberDashboardSummary,
  MemberDashboardUsagePoint,
  MemberDashboardUsageToday,
  MemberDashboardWallet,
} from "@/types";

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asNumber(value: unknown, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function asBoolean(value: unknown, fallback = false): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function nullableNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function nullableString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function readTokenUsage(value: unknown): DashboardTokenUsage {
  const source = asRecord(value);
  return {
    inputTokens: asNumber(source.inputTokens ?? source.input_tokens),
    cachedInputTokens: asNumber(
      source.cachedInputTokens ?? source.cached_input_tokens,
    ),
    outputTokens: asNumber(source.outputTokens ?? source.output_tokens),
    reasoningOutputTokens: asNumber(
      source.reasoningOutputTokens ?? source.reasoning_output_tokens,
    ),
    totalTokens: asNumber(source.totalTokens ?? source.total_tokens),
    estimatedCostUsd: asNumber(
      source.estimatedCostUsd ?? source.estimated_cost_usd,
    ),
    requestCount: asNumber(source.requestCount ?? source.request_count),
    successCount: asNumber(source.successCount ?? source.success_count),
    errorCount: asNumber(source.errorCount ?? source.error_count),
  };
}

function readDailyUsagePoint(value: unknown): DashboardDailyUsagePoint {
  const source = asRecord(value);
  return {
    dayStartTs: asNumber(source.dayStartTs ?? source.day_start_ts),
    dayEndTs: asNumber(source.dayEndTs ?? source.day_end_ts),
    usage: readTokenUsage(source.usage),
  };
}

function readUserUsageSummary(value: unknown): DashboardUserUsageSummary | null {
  const source = asRecord(value);
  const userId = asString(source.userId ?? source.user_id);
  if (!userId) return null;
  return {
    userId,
    username: nullableString(source.username),
    displayName: nullableString(source.displayName ?? source.display_name),
    role: nullableString(source.role),
    status: nullableString(source.status),
    walletAvailableCreditMicros: nullableNumber(
      source.walletAvailableCreditMicros ??
        source.wallet_available_credit_micros,
    ),
    todayUsage: readTokenUsage(source.todayUsage ?? source.today_usage),
    rangeUsage: readTokenUsage(source.rangeUsage ?? source.range_usage),
  };
}

function readSourceUsageSummary(value: unknown): DashboardSourceUsageSummary | null {
  const source = asRecord(value);
  const sourceId = asString(source.sourceId ?? source.source_id);
  if (!sourceId) return null;
  return {
    sourceKind: asString(source.sourceKind ?? source.source_kind),
    sourceId,
    name: nullableString(source.name),
    status: nullableString(source.status),
    provider: nullableString(source.provider),
    todayUsage: readTokenUsage(source.todayUsage ?? source.today_usage),
    rangeUsage: readTokenUsage(source.rangeUsage ?? source.range_usage),
  };
}

function readAdminUsageSummary(value: unknown): DashboardAdminUsageSummary {
  const source = asRecord(value);
  return {
    rangeStartTs: asNumber(source.rangeStartTs ?? source.range_start_ts),
    rangeEndTs: asNumber(source.rangeEndTs ?? source.range_end_ts),
    todayStartTs: asNumber(source.todayStartTs ?? source.today_start_ts),
    todayEndTs: asNumber(source.todayEndTs ?? source.today_end_ts),
    todayUsage: readTokenUsage(source.todayUsage ?? source.today_usage),
    dailyUsage: asArray(source.dailyUsage ?? source.daily_usage).map(
      readDailyUsagePoint,
    ),
    users: asArray(source.users)
      .map(readUserUsageSummary)
      .filter((item): item is DashboardUserUsageSummary => Boolean(item)),
    openaiAccounts: asArray(source.openaiAccounts ?? source.openai_accounts)
      .map(readSourceUsageSummary)
      .filter((item): item is DashboardSourceUsageSummary => Boolean(item)),
    kiroCredentials: asArray(source.kiroCredentials ?? source.kiro_credentials)
      .map(readSourceUsageSummary)
      .filter((item): item is DashboardSourceUsageSummary => Boolean(item)),
    grokCredentials: asArray(source.grokCredentials ?? source.grok_credentials)
      .map(readSourceUsageSummary)
      .filter((item): item is DashboardSourceUsageSummary => Boolean(item)),
    aggregateApis: asArray(source.aggregateApis ?? source.aggregate_apis)
      .map(readSourceUsageSummary)
      .filter((item): item is DashboardSourceUsageSummary => Boolean(item)),
  };
}

function readWallet(value: unknown): MemberDashboardWallet | null {
  const source = asRecord(value);
  const id = asString(source.id);
  if (!id) return null;
  return {
    id,
    balanceCreditMicros: asNumber(
      source.balanceCreditMicros ?? source.balance_credit_micros,
    ),
    frozenCreditMicros: asNumber(
      source.frozenCreditMicros ?? source.frozen_credit_micros,
    ),
    availableCreditMicros: asNumber(
      source.availableCreditMicros ?? source.available_credit_micros,
    ),
    status: asString(source.status) || "active",
    updatedAt: asNumber(source.updatedAt ?? source.updated_at),
  };
}

function readApiKeySummary(value: unknown): MemberDashboardApiKeySummary {
  const source = asRecord(value);
  return {
    totalCount: asNumber(source.totalCount ?? source.total_count),
    enabledCount: asNumber(source.enabledCount ?? source.enabled_count),
    disabledCount: asNumber(source.disabledCount ?? source.disabled_count),
    lastUsedAt: nullableNumber(source.lastUsedAt ?? source.last_used_at),
  };
}

function readUsageToday(value: unknown): MemberDashboardUsageToday {
  const source = asRecord(value);
  return {
    inputTokens: asNumber(source.inputTokens ?? source.input_tokens),
    cachedInputTokens: asNumber(
      source.cachedInputTokens ?? source.cached_input_tokens,
    ),
    outputTokens: asNumber(source.outputTokens ?? source.output_tokens),
    reasoningOutputTokens: asNumber(
      source.reasoningOutputTokens ?? source.reasoning_output_tokens,
    ),
    totalTokens: asNumber(source.totalTokens ?? source.total_tokens),
    estimatedCostUsd: asNumber(
      source.estimatedCostUsd ?? source.estimated_cost_usd,
    ),
    totalCount: asNumber(source.totalCount ?? source.total_count),
    successCount: asNumber(source.successCount ?? source.success_count),
    errorCount: asNumber(source.errorCount ?? source.error_count),
    successRate: nullableNumber(source.successRate ?? source.success_rate),
  };
}

function readUsagePoint(value: unknown): MemberDashboardUsagePoint {
  const source = asRecord(value);
  return {
    dayStartTs: asNumber(source.dayStartTs ?? source.day_start_ts),
    dayEndTs: asNumber(source.dayEndTs ?? source.day_end_ts),
    totalTokens: asNumber(source.totalTokens ?? source.total_tokens),
    estimatedCostUsd: asNumber(
      source.estimatedCostUsd ?? source.estimated_cost_usd,
    ),
  };
}

function readKeyUsage(value: unknown): MemberDashboardKeyUsage | null {
  const source = asRecord(value);
  const keyId = asString(source.keyId ?? source.key_id);
  if (!keyId) return null;
  return {
    keyId,
    name: nullableString(source.name),
    modelSlug: nullableString(source.modelSlug ?? source.model_slug),
    status: asString(source.status) || "enabled",
    todayTokens: asNumber(source.todayTokens ?? source.today_tokens),
    todayCostUsd: asNumber(source.todayCostUsd ?? source.today_cost_usd),
    totalTokens: asNumber(source.totalTokens ?? source.total_tokens),
    totalCostUsd: asNumber(source.totalCostUsd ?? source.total_cost_usd),
    lastUsedAt: nullableNumber(source.lastUsedAt ?? source.last_used_at),
  };
}

function readModelUsage(value: unknown): MemberDashboardModelUsage | null {
  const source = asRecord(value);
  const model = asString(source.model);
  if (!model) return null;
  return {
    model,
    totalTokens: asNumber(source.totalTokens ?? source.total_tokens),
    estimatedCostUsd: asNumber(
      source.estimatedCostUsd ?? source.estimated_cost_usd,
    ),
  };
}

function readAlert(value: unknown): MemberDashboardAlert | null {
  const source = asRecord(value);
  const kind = asString(source.kind);
  if (!kind) return null;
  return {
    kind,
    severity: asString(source.severity) || "info",
    title: asString(source.title),
    message: asString(source.message),
    actionLabel: nullableString(source.actionLabel ?? source.action_label),
    actionHref: nullableString(source.actionHref ?? source.action_href),
  };
}

function readMemberDashboardSummary(value: unknown): MemberDashboardSummary {
  const source = asRecord(value);
  return {
    userId: nullableString(source.userId ?? source.user_id),
    distributionEnabled: asBoolean(
      source.distributionEnabled ?? source.distribution_enabled,
    ),
    wallet: readWallet(source.wallet),
    apiKeySummary: readApiKeySummary(
      source.apiKeySummary ?? source.api_key_summary,
    ),
    usageToday: readUsageToday(source.usageToday ?? source.usage_today),
    usageTrend7d: asArray(source.usageTrend7d ?? source.usage_trend_7d).map(
      readUsagePoint,
    ),
    topKeys: asArray(source.topKeys ?? source.top_keys)
      .map(readKeyUsage)
      .filter((item): item is MemberDashboardKeyUsage => Boolean(item)),
    topModels: asArray(source.topModels ?? source.top_models)
      .map(readModelUsage)
      .filter((item): item is MemberDashboardModelUsage => Boolean(item)),
    availableModels: normalizeModelCatalog({
      models: source.availableModels ?? source.available_models,
    }).models,
    recentLogs: normalizeRequestLogs(source.recentLogs ?? source.recent_logs),
    alerts: asArray(source.alerts)
      .map(readAlert)
      .filter((item): item is MemberDashboardAlert => Boolean(item)),
  };
}

export const dashboardClient = {
  async getAdminUsageSummary(params?: {
    startTs?: number | null;
    endTs?: number | null;
    includeBreakdowns?: boolean;
  }): Promise<DashboardAdminUsageSummary> {
    const result = await invoke<unknown>(
      "service_dashboard_admin_usage_summary",
      withAddr({
        startTs: params?.startTs ?? null,
        endTs: params?.endTs ?? null,
        includeBreakdowns: params?.includeBreakdowns ?? null,
      }),
    );
    return readAdminUsageSummary(result);
  },
  async getMemberSummary(params?: {
    userId?: string | null;
    dayStartTs?: number;
    dayEndTs?: number;
    includeDetails?: boolean;
  }): Promise<MemberDashboardSummary> {
    const result = await invoke<unknown>(
      "service_dashboard_member_summary",
      withAddr({
        userId: params?.userId ?? null,
        dayStartTs: params?.dayStartTs ?? null,
        dayEndTs: params?.dayEndTs ?? null,
        includeDetails: params?.includeDetails ?? null,
      }),
    );
    return readMemberDashboardSummary(result);
  },
};
