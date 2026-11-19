export interface ApiKey {
  id: string;
  name: string;
  model: string;
  modelSlug: string;
  reasoningEffort: string;
  serviceTier: string;
  rotationStrategy: string;
  aggregateApiId: string | null;
  accountPlanFilter: string | null;
  accountGroupFilter: string | null;
  aggregateApiUrl: string | null;
  quotaLimitTokens: number | null;
  protocol: string;
  clientType: string;
  authScheme: string;
  upstreamBaseUrl: string;
  staticHeadersJson: string;
  status: string;
  createdAt: number | null;
  lastUsedAt: number | null;
}

export interface ApiKeyCreateResult {
  id: string;
  key: string;
}

export interface AggregateApi {
  id: string;
  providerType: string;
  supplierName: string | null;
  sort: number;
  url: string;
  userAgent: string | null;
  authType: string;
  authParams: Record<string, unknown> | null;
  action: string | null;
  status: string;
  createdAt: number | null;
  updatedAt: number | null;
  lastTestAt: number | null;
  lastTestStatus: string | null;
  lastTestError: string | null;
  balanceQueryEnabled: boolean;
  balanceQueryTemplate: string | null;
  balanceQueryBaseUrl: string | null;
  balanceQueryUserId: string | null;
  balanceQueryConfigJson: string | null;
  lastBalanceAt: number | null;
  lastBalanceStatus: string | null;
  lastBalanceError: string | null;
  lastBalanceJson: string | null;
  modelSlugs: string[];
}

export interface AggregateApiCreateResult {
  id: string;
  key: string;
}

export interface AggregateApiSecretResult {
  id: string;
  key: string;
  authType: string;
  username: string | null;
  password: string | null;
}

export interface AggregateApiTestResult {
  id: string;
  ok: boolean;
  statusCode: number | null;
  message: string | null;
  testedAt: number;
  latencyMs: number;
}

export interface AggregateApiBalanceSnapshot {
  isValid: boolean;
  invalidMessage: string | null;
  remaining: number | null;
  unit: string | null;
  planName: string | null;
  total: number | null;
  used: number | null;
  extra: Record<string, unknown> | null;
}

export interface AggregateApiBalanceRefreshResult {
  id: string;
  ok: boolean;
  balance: AggregateApiBalanceSnapshot | null;
  message: string | null;
  queriedAt: number;
  latencyMs: number;
}

export interface AggregateApiFetchedModel {
  upstreamModel: string;
  displayName: string | null;
  metadata?: Record<string, unknown> | null;
  existingModelSlug: string | null;
  alreadyLinked: boolean;
}

export interface AggregateApiFetchModelsResult {
  apiId: string;
  providerType: string;
  fetchedAt: number;
  items: AggregateApiFetchedModel[];
}

export interface AggregateApiAssociateModelsResult {
  createdModels: string[];
  addedRoutes: string[];
  unchangedRoutes: string[];
}

export interface ApiKeyUsageStat {
  keyId: string;
  todayTokens: number;
  todayEstimatedCostUsd: number;
  totalTokens: number;
  estimatedCostUsd: number;
}
