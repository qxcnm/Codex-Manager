export interface RequestLog {
  id: string;
  traceId: string;
  keyId: string;
  accountId: string;
  initialAccountId: string;
  attemptedAccountIds: string[];
  initialAggregateApiId: string;
  attemptedAggregateApiIds: string[];
  requestPath: string;
  originalPath: string;
  adaptedPath: string;
  method: string;
  requestType: string;
  gatewayMode: string;
  routeStrategy: string;
  routeSource: string;
  path: string;
  clientModel: string;
  model: string;
  modelSource: string;
  upstreamModel: string;
  actualSourceKind: string;
  actualSourceId: string;
  clientReasoningEffort: string;
  reasoningEffort: string;
  reasoningSource: string;
  serviceTier: string;
  effectiveServiceTier: string;
  serviceTierSource: string;
  responseAdapter: string;
  canonicalSource: string;
  sizeRejectStage: string;
  upstreamUrl: string;
  aggregateApiSupplierName: string | null;
  aggregateApiUrl: string | null;
  statusCode: number | null;
  inputTokens: number | null;
  cachedInputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  reasoningOutputTokens: number | null;
  estimatedCostUsd: number | null;
  durationMs: number | null;
  firstResponseMs: number | null;
  error: string;
  createdAt: number | null;
}

export interface RequestLogListResult {
  items: RequestLog[];
  total: number;
  page: number;
  pageSize: number;
}

export interface RequestLogFilterSummary {
  totalCount: number;
  filteredCount: number;
  successCount: number;
  errorCount: number;
  totalTokens: number;
  totalCostUsd: number;
}

export interface RequestLogListWithSummaryResult extends RequestLogListResult {
  summary: RequestLogFilterSummary;
}

export interface RequestLogTodaySummary {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  todayTokens: number;
  estimatedCost: number;
}

export interface RequestLogDetail {
  traceId: string;
  payload: string;
  payloadBytes: number;
  payloadTruncated: boolean;
  createdAt: number;
}
