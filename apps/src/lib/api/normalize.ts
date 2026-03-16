"use client";

import {
  Account,
  AccountListResult,
  AccountUsage,
  ApiKey,
  ApiKeyCreateResult,
  ApiKeyUsageStat,
  AppSettings,
  BackgroundTaskSettings,
  DeviceAuthInfo,
  EnvOverrideCatalogItem,
  LoginStartResult,
  ModelOption,
  RequestLog,
  RequestLogFilterSummary,
  RequestLogListResult,
  RequestLogTodaySummary,
  StartupSnapshot,
  UsageAggregateSummary,
} from "@/types";
import {
  calcAvailability,
  getUsageDisplayBuckets,
  isLowQuotaUsage,
  toNullableNumber,
} from "@/lib/utils/usage";

const DEFAULT_BACKGROUND_TASKS: BackgroundTaskSettings = {
  usagePollingEnabled: true,
  usagePollIntervalSecs: 600,
  gatewayKeepaliveEnabled: true,
  gatewayKeepaliveIntervalSecs: 180,
  tokenRefreshPollingEnabled: true,
  tokenRefreshPollIntervalSecs: 60,
  usageRefreshWorkers: 4,
  httpWorkerFactor: 4,
  httpWorkerMin: 8,
  httpStreamWorkerFactor: 1,
  httpStreamWorkerMin: 2,
};

function asObject(payload: unknown): Record<string, unknown> {
  return payload && typeof payload === "object" && !Array.isArray(payload)
    ? (payload as Record<string, unknown>)
    : {};
}

function asArray<T = unknown>(payload: unknown): T[] {
  return Array.isArray(payload) ? payload : [];
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value.trim() : fallback;
}

function asBoolean(value: unknown, fallback = false): boolean {
  if (typeof value === "boolean") return value;
  if (typeof value === "number") return value !== 0;
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (["1", "true", "yes", "on"].includes(normalized)) return true;
    if (["0", "false", "no", "off"].includes(normalized)) return false;
  }
  return fallback;
}

function asInteger(value: unknown, fallback: number, min = 0): number {
  const parsed = toNullableNumber(value);
  if (parsed == null) return fallback;
  return Math.max(min, Math.trunc(parsed));
}

function normalizeStringRecord(payload: unknown): Record<string, string> {
  const source = asObject(payload);
  return Object.entries(source).reduce<Record<string, string>>((result, [key, value]) => {
    result[key] = asString(value);
    return result;
  }, {});
}

export function normalizeUsageSnapshot(payload: unknown): AccountUsage | null {
  const source = asObject(payload);
  const accountId = asString(source.accountId ?? source.account_id);
  if (!accountId) return null;

  return {
    accountId,
    availabilityStatus: asString(source.availabilityStatus ?? source.availability_status),
    usedPercent: toNullableNumber(source.usedPercent ?? source.used_percent),
    windowMinutes: toNullableNumber(source.windowMinutes ?? source.window_minutes),
    resetsAt: toNullableNumber(source.resetsAt ?? source.resets_at),
    secondaryUsedPercent: toNullableNumber(
      source.secondaryUsedPercent ?? source.secondary_used_percent
    ),
    secondaryWindowMinutes: toNullableNumber(
      source.secondaryWindowMinutes ?? source.secondary_window_minutes
    ),
    secondaryResetsAt: toNullableNumber(
      source.secondaryResetsAt ?? source.secondary_resets_at
    ),
    creditsJson: asString(source.creditsJson ?? source.credits_json) || null,
    capturedAt: toNullableNumber(source.capturedAt ?? source.captured_at),
  };
}

export function normalizeUsageList(payload: unknown): AccountUsage[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeUsageSnapshot(item))
    .filter((item): item is AccountUsage => Boolean(item));
}

export function buildUsageMap(usages: AccountUsage[]): Map<string, AccountUsage> {
  return new Map(usages.map((item) => [item.accountId, item]));
}

export function normalizeUsageAggregateSummary(payload: unknown): UsageAggregateSummary {
  const source = asObject(payload);
  return {
    primaryBucketCount: asInteger(source.primaryBucketCount, 0, 0),
    primaryKnownCount: asInteger(source.primaryKnownCount, 0, 0),
    primaryUnknownCount: asInteger(source.primaryUnknownCount, 0, 0),
    primaryRemainPercent: toNullableNumber(source.primaryRemainPercent),
    secondaryBucketCount: asInteger(source.secondaryBucketCount, 0, 0),
    secondaryKnownCount: asInteger(source.secondaryKnownCount, 0, 0),
    secondaryUnknownCount: asInteger(source.secondaryUnknownCount, 0, 0),
    secondaryRemainPercent: toNullableNumber(source.secondaryRemainPercent),
  };
}

export function normalizeTodaySummary(payload: unknown): RequestLogTodaySummary {
  const source = asObject(payload);
  const inputTokens = asInteger(source.inputTokens, 0, 0);
  const cachedInputTokens = asInteger(source.cachedInputTokens, 0, 0);
  const outputTokens = asInteger(source.outputTokens, 0, 0);
  const reasoningOutputTokens = asInteger(source.reasoningOutputTokens, 0, 0);
  return {
    inputTokens,
    cachedInputTokens,
    outputTokens,
    reasoningOutputTokens,
    todayTokens: asInteger(
      source.todayTokens,
      Math.max(0, inputTokens - cachedInputTokens) + outputTokens,
      0
    ),
    estimatedCost: Math.max(0, toNullableNumber(source.estimatedCost) ?? 0),
  };
}

export function normalizeAccount(item: unknown, usage?: AccountUsage | null): Account | null {
  const source = asObject(item);
  const id = asString(source.id);
  if (!id) return null;

  const name = asString(source.label || source.name) || id;
  const groupName = asString(source.groupName ?? source.group_name);
  const status = asString(source.status);
  const availability = calcAvailability(usage, { status });
  const usageBuckets = getUsageDisplayBuckets(usage);

  return {
    id,
    name,
    group: groupName,
    priority: asInteger(source.sort ?? source.priority, 0, 0),
    label: name,
    groupName,
    sort: asInteger(source.sort ?? source.priority, 0, 0),
    status,
    isAvailable: availability.level === "ok",
    isLowQuota: isLowQuotaUsage(usage),
    lastRefreshAt: usage?.capturedAt ?? null,
    availabilityText: availability.text,
    availabilityLevel: availability.level,
    primaryRemainPercent: usageBuckets.primaryRemainPercent,
    secondaryRemainPercent: usageBuckets.secondaryRemainPercent,
    usage: usage ?? null,
  };
}

export function normalizeAccountList(
  payload: unknown,
  usages: AccountUsage[] = []
): AccountListResult {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  const usageMap = buildUsageMap(usages);
  const normalizedItems = items
    .map((item) => normalizeAccount(item, usageMap.get(asString(asObject(item).id))))
    .filter((item): item is Account => Boolean(item));

  return {
    items: normalizedItems,
    total: asInteger(source.total, normalizedItems.length, 0),
    page: asInteger(source.page, 1, 1),
    pageSize: asInteger(source.pageSize, normalizedItems.length || 20, 1),
  };
}

export function attachUsagesToAccounts(
  accounts: Account[],
  usages: AccountUsage[]
): Account[] {
  const usageMap = buildUsageMap(usages);
  return accounts.map((account) => normalizeAccount(account, usageMap.get(account.id)) || account);
}

export function normalizeModelOptions(payload: unknown): ModelOption[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => {
      const current = asObject(item);
      const slug = asString(current.slug);
      if (!slug) return null;
      return {
        slug,
        displayName: asString(current.displayName ?? current.display_name) || slug,
      };
    })
    .filter((item): item is ModelOption => Boolean(item));
}

export function normalizeApiKey(item: unknown): ApiKey | null {
  const source = asObject(item);
  const id = asString(source.id);
  if (!id) return null;

  return {
    id,
    name: asString(source.name) || "未命名",
    model: asString(source.modelSlug ?? source.model_slug),
    modelSlug: asString(source.modelSlug ?? source.model_slug),
    reasoningEffort: asString(source.reasoningEffort ?? source.reasoning_effort),
    protocol: asString(source.protocolType ?? source.protocol_type) || "openai_compat",
    clientType: asString(source.clientType ?? source.client_type),
    authScheme: asString(source.authScheme ?? source.auth_scheme),
    upstreamBaseUrl: asString(source.upstreamBaseUrl ?? source.upstream_base_url),
    staticHeadersJson: asString(source.staticHeadersJson ?? source.static_headers_json),
    status: asString(source.status) || "enabled",
    createdAt: toNullableNumber(source.createdAt ?? source.created_at),
    lastUsedAt: toNullableNumber(source.lastUsedAt ?? source.last_used_at),
  };
}

export function normalizeApiKeyList(payload: unknown): ApiKey[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeApiKey(item))
    .filter((item): item is ApiKey => Boolean(item));
}

export function normalizeApiKeyCreateResult(payload: unknown): ApiKeyCreateResult {
  const source = asObject(payload);
  return {
    id: asString(source.id),
    key: asString(source.key),
  };
}

export function normalizeApiKeyUsageStats(payload: unknown): ApiKeyUsageStat[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => {
      const current = asObject(item);
      const keyId = asString(current.keyId ?? current.key_id);
      if (!keyId) return null;
      return {
        keyId,
        totalTokens: asInteger(current.totalTokens ?? current.total_tokens, 0, 0),
      };
    })
    .filter((item): item is ApiKeyUsageStat => Boolean(item));
}

export function normalizeDeviceAuthInfo(payload: unknown): DeviceAuthInfo | null {
  const source = asObject(payload);
  const verificationUrl = asString(source.verificationUrl ?? source.verification_url);
  if (!verificationUrl) return null;

  return {
    userCodeUrl: asString(source.userCodeUrl ?? source.user_code_url),
    tokenUrl: asString(source.tokenUrl ?? source.token_url),
    verificationUrl,
    redirectUri: asString(source.redirectUri ?? source.redirect_uri),
  };
}

export function normalizeLoginStartResult(payload: unknown): LoginStartResult {
  const source = asObject(payload);
  return {
    authUrl: asString(source.authUrl ?? source.auth_url),
    loginId: asString(source.loginId ?? source.login_id),
    loginType: asString(source.loginType ?? source.login_type),
    issuer: asString(source.issuer),
    clientId: asString(source.clientId ?? source.client_id),
    redirectUri: asString(source.redirectUri ?? source.redirect_uri),
    warning: asString(source.warning),
    device: normalizeDeviceAuthInfo(source.device),
  };
}

export function normalizeRequestLog(item: unknown): RequestLog | null {
  const source = asObject(item);
  const createdAt = toNullableNumber(source.createdAt ?? source.created_at);
  const traceId = asString(source.traceId ?? source.trace_id);
  const keyId = asString(source.keyId ?? source.key_id);
  const accountId = asString(source.accountId ?? source.account_id);
  const requestPath = asString(source.requestPath ?? source.request_path);
  const method = asString(source.method);
  const id = traceId || [createdAt ?? "", method, requestPath, accountId, keyId].join("|");
  if (!id) return null;
  const durationMs = toNullableNumber(
    source.durationMs ??
      source.duration_ms ??
      source.latencyMs ??
      source.latency_ms ??
      source.elapsedMs ??
      source.elapsed_ms ??
      source.responseTimeMs ??
      source.response_time_ms
  );

  return {
    id,
    traceId,
    keyId,
    accountId,
    requestPath,
    originalPath: asString(source.originalPath ?? source.original_path),
    adaptedPath: asString(source.adaptedPath ?? source.adapted_path),
    method,
    path: requestPath,
    model: asString(source.model),
    reasoningEffort: asString(source.reasoningEffort ?? source.reasoning_effort),
    responseAdapter: asString(source.responseAdapter ?? source.response_adapter),
    upstreamUrl: asString(source.upstreamUrl ?? source.upstream_url),
    statusCode: toNullableNumber(source.statusCode ?? source.status_code),
    inputTokens: toNullableNumber(source.inputTokens ?? source.input_tokens),
    cachedInputTokens: toNullableNumber(
      source.cachedInputTokens ?? source.cached_input_tokens
    ),
    outputTokens: toNullableNumber(source.outputTokens ?? source.output_tokens),
    totalTokens: toNullableNumber(source.totalTokens ?? source.total_tokens),
    reasoningOutputTokens: toNullableNumber(
      source.reasoningOutputTokens ?? source.reasoning_output_tokens
    ),
    estimatedCostUsd: toNullableNumber(
      source.estimatedCostUsd ?? source.estimated_cost_usd
    ),
    durationMs,
    error: asString(source.error),
    createdAt,
  };
}

export function normalizeRequestLogs(payload: unknown): RequestLog[] {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload);
  return items
    .map((item) => normalizeRequestLog(item))
    .filter((item): item is RequestLog => Boolean(item));
}

export function normalizeRequestLogListResult(payload: unknown): RequestLogListResult {
  const source = asObject(payload);
  const items = normalizeRequestLogs(source.items ?? payload);
  return {
    items,
    total: asInteger(source.total, items.length, 0),
    page: asInteger(source.page, 1, 1),
    pageSize: asInteger(source.pageSize, items.length || 20, 1),
  };
}

export function normalizeRequestLogFilterSummary(
  payload: unknown
): RequestLogFilterSummary {
  const source = asObject(payload);
  return {
    totalCount: asInteger(source.totalCount, 0, 0),
    filteredCount: asInteger(source.filteredCount, 0, 0),
    successCount: asInteger(source.successCount, 0, 0),
    errorCount: asInteger(source.errorCount, 0, 0),
    totalTokens: asInteger(source.totalTokens, 0, 0),
  };
}

export function normalizeBackgroundTasks(payload: unknown): BackgroundTaskSettings {
  const source = asObject(payload);
  return {
    usagePollingEnabled: asBoolean(
      source.usagePollingEnabled,
      DEFAULT_BACKGROUND_TASKS.usagePollingEnabled
    ),
    usagePollIntervalSecs: asInteger(
      source.usagePollIntervalSecs,
      DEFAULT_BACKGROUND_TASKS.usagePollIntervalSecs,
      1
    ),
    gatewayKeepaliveEnabled: asBoolean(
      source.gatewayKeepaliveEnabled,
      DEFAULT_BACKGROUND_TASKS.gatewayKeepaliveEnabled
    ),
    gatewayKeepaliveIntervalSecs: asInteger(
      source.gatewayKeepaliveIntervalSecs,
      DEFAULT_BACKGROUND_TASKS.gatewayKeepaliveIntervalSecs,
      1
    ),
    tokenRefreshPollingEnabled: asBoolean(
      source.tokenRefreshPollingEnabled,
      DEFAULT_BACKGROUND_TASKS.tokenRefreshPollingEnabled
    ),
    tokenRefreshPollIntervalSecs: asInteger(
      source.tokenRefreshPollIntervalSecs,
      DEFAULT_BACKGROUND_TASKS.tokenRefreshPollIntervalSecs,
      1
    ),
    usageRefreshWorkers: asInteger(
      source.usageRefreshWorkers,
      DEFAULT_BACKGROUND_TASKS.usageRefreshWorkers,
      1
    ),
    httpWorkerFactor: asInteger(
      source.httpWorkerFactor,
      DEFAULT_BACKGROUND_TASKS.httpWorkerFactor,
      1
    ),
    httpWorkerMin: asInteger(
      source.httpWorkerMin,
      DEFAULT_BACKGROUND_TASKS.httpWorkerMin,
      1
    ),
    httpStreamWorkerFactor: asInteger(
      source.httpStreamWorkerFactor,
      DEFAULT_BACKGROUND_TASKS.httpStreamWorkerFactor,
      1
    ),
    httpStreamWorkerMin: asInteger(
      source.httpStreamWorkerMin,
      DEFAULT_BACKGROUND_TASKS.httpStreamWorkerMin,
      1
    ),
  };
}

export function normalizeEnvOverrideCatalog(payload: unknown): EnvOverrideCatalogItem[] {
  return asArray(payload).reduce<EnvOverrideCatalogItem[]>((result, item) => {
    const source = asObject(item);
    const key = asString(source.key);
    if (!key) return result;
    result.push({
      key,
      label: asString(source.label) || key,
      defaultValue: asString(source.defaultValue ?? source.default_value),
      scope: asString(source.scope),
      applyMode: asString(source.applyMode ?? source.apply_mode),
    });
    return result;
  }, []);
}

export function normalizeAppSettings(payload: unknown): AppSettings {
  const source = asObject(payload);
  return {
    updateAutoCheck: asBoolean(source.updateAutoCheck, true),
    closeToTrayOnClose: asBoolean(source.closeToTrayOnClose, false),
    closeToTraySupported: asBoolean(source.closeToTraySupported, true),
    lowTransparency: asBoolean(source.lowTransparency, false),
    lightweightModeOnCloseToTray: asBoolean(
      source.lightweightModeOnCloseToTray,
      false
    ),
    webAccessPasswordConfigured: asBoolean(
      source.webAccessPasswordConfigured,
      false
    ),
    serviceAddr: asString(source.serviceAddr) || "localhost:48760",
    serviceListenMode: asString(source.serviceListenMode) || "loopback",
    serviceListenModeOptions: asArray(source.serviceListenModeOptions).map((item) =>
      asString(item)
    ),
    routeStrategy: asString(source.routeStrategy) || "ordered",
    routeStrategyOptions: asArray(source.routeStrategyOptions).map((item) =>
      asString(item)
    ),
    freeAccountMaxModel: asString(source.freeAccountMaxModel) || "gpt-5.2",
    freeAccountMaxModelOptions: asArray(source.freeAccountMaxModelOptions).map((item) =>
      asString(item)
    ),
    cpaNoCookieHeaderModeEnabled: asBoolean(
      source.cpaNoCookieHeaderModeEnabled,
      false
    ),
    upstreamProxyUrl: asString(source.upstreamProxyUrl),
    upstreamStreamTimeoutMs: asInteger(source.upstreamStreamTimeoutMs, 1_800_000, 0),
    sseKeepaliveIntervalMs: asInteger(source.sseKeepaliveIntervalMs, 15_000, 1),
    backgroundTasks: normalizeBackgroundTasks(source.backgroundTasks),
    envOverrides: normalizeStringRecord(source.envOverrides),
    envOverrideCatalog: normalizeEnvOverrideCatalog(source.envOverrideCatalog),
    envOverrideReservedKeys: asArray(source.envOverrideReservedKeys).map((item) =>
      asString(item)
    ),
    envOverrideUnsupportedKeys: asArray(source.envOverrideUnsupportedKeys).map((item) =>
      asString(item)
    ),
    theme: asString(source.theme) || "tech",
  };
}

export function normalizeStartupSnapshot(payload: unknown): StartupSnapshot {
  const source = asObject(payload);
  const usageSnapshots = normalizeUsageList(source.usageSnapshots);
  const usageMap = buildUsageMap(usageSnapshots);
  const accounts = asArray(source.accounts)
    .map((item) => normalizeAccount(item, usageMap.get(asString(asObject(item).id))))
    .filter((item): item is Account => Boolean(item));

  return {
    accounts,
    usageSnapshots,
    usageAggregateSummary: normalizeUsageAggregateSummary(source.usageAggregateSummary),
    apiKeys: normalizeApiKeyList(source.apiKeys),
    apiModelOptions: normalizeModelOptions(source.apiModelOptions),
    manualPreferredAccountId: asString(source.manualPreferredAccountId),
    requestLogTodaySummary: normalizeTodaySummary(source.requestLogTodaySummary),
    requestLogs: normalizeRequestLogs(source.requestLogs),
  };
}
