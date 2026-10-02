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

export type RequestLogDetailStorageMode = "preview" | "full";

export interface RequestLogDetailField {
  name: string;
  /** JSON text of the field value (raw text for non-JSON bodies). */
  value: string;
}

export interface RequestLogDetailContextSegment {
  traceId: string;
  createdAt: number;
  previousResponseId: string | null;
  listField: string | null;
  complete: boolean;
  items: string[];
}

export type RequestLogDetailStage = "client" | "upstream";

export interface RequestLogDetail {
  traceId: string;
  /** Capture stage of the returned body. */
  stage: RequestLogDetailStage;
  /** Capture stages stored for this trace, e.g. ["client"] or ["client", "upstream"]. */
  stages: RequestLogDetailStage[];
  /** preview: 16 KB capped text in `payload`; full: rebuilt from fields + items. */
  storageMode: RequestLogDetailStorageMode;
  payload: string;
  payloadBytes: number;
  payloadTruncated: boolean;
  redacted: boolean;
  createdAt: number;
  bodyKind: string | null;
  listField: string | null;
  complete: boolean;
  fields: RequestLogDetailField[];
  /** JSON text of each list item, in request order. */
  items: string[];
  /** Number of leading items shared with the parent request of the conversation. */
  inheritedItemCount: number;
  parentTraceId: string | null;
  previousResponseId: string | null;
  /** Earlier requests of the conversation for previous_response_id continuations, oldest first. */
  context: RequestLogDetailContextSegment[];
}
