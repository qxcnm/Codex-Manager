export type ManagedModelOriginV2 = "builtin" | "custom";
export type ModelPriceStatusV2 = "official" | "estimated" | "custom" | "missing";
export type ModelInstructionsModeV2 = "passthrough" | "fallback" | "override";
export type ModelFastPolicyV2 = "passthrough" | "filter" | "force" | "block";
export type ModelVisibilityV2 = "list" | "hide";
export type ModelRouteSourceKindV2 = "account_pool" | "aggregate_api";
export type ModelRouteBatchModeV2 = "merge" | "replace";
export type ManagedModelImportConflictStrategyV2 = "keep_existing" | "replace_custom";

export interface ModelPriceTierV2 {
  minInputTokens: number;
  inputMicrousdPer1m: number;
  cachedInputMicrousdPer1m: number;
  cacheWriteMicrousdPer1m: number | null;
  outputMicrousdPer1m: number;
}

export interface ModelPriceV2 {
  priceStatus: ModelPriceStatusV2;
  priceSource: string | null;
  inputMicrousdPer1m: number | null;
  cachedInputMicrousdPer1m: number | null;
  cacheWriteMicrousdPer1m: number | null;
  outputMicrousdPer1m: number | null;
}

export interface ModelRouteV2 {
  id: string;
  sourceKind: ModelRouteSourceKindV2;
  sourceId: string;
  upstreamModel: string;
  enabled: boolean;
  priority: number;
  weight: number;
}

export interface ModelRouteBatchTemplateV2 {
  sourceKind: ModelRouteSourceKindV2;
  sourceId: string;
  priority: number;
  weight: number;
}

export interface ManagedModelBatchRouteAssignmentV2 {
  slugs: string[];
  mode: ModelRouteBatchModeV2;
  routes: ModelRouteBatchTemplateV2[];
}

export interface ManagedModelBatchRouteResultV2 {
  updated: string[];
  failed: Array<{ slug: string; reason: string }>;
}

export interface ManagedModelV2 {
  id: string;
  slug: string;
  displayName: string;
  description: string | null;
  provider: string | null;
  family: string | null;
  category: string | null;
  tags: string[];
  origin: ManagedModelOriginV2;
  enabled: boolean;
  supportedInApi: boolean;
  visibility: ModelVisibilityV2;
  sortOrder: number;
  contextWindow: number | null;
  maxContextWindow: number | null;
  defaultReasoningEffort: string | null;
  capabilities: Record<string, unknown>;
  instructionsMode: ModelInstructionsModeV2;
  instructionsText: string | null;
  fastPolicy: ModelFastPolicyV2;
  builtinRevision: number | null;
  userEdited: boolean;
  price: ModelPriceV2;
  priceTiers: ModelPriceTierV2[];
  routes: ModelRouteV2[];
  permissionGroupIds: string[];
  createdAt: number;
  updatedAt: number;
}

export interface ModelCatalogV2Stats {
  total: number;
  enabled: number;
  builtin: number;
  custom: number;
  priceMissing: number;
  missingRoute: number;
}

export interface ManagedModelListV2Result {
  items: ManagedModelV2[];
  stats: ModelCatalogV2Stats;
}

export interface ModelPriceSourceSyncSummaryV2 {
  name: string;
  url: string;
  status: "ok" | "error";
  providers: number;
  modelsSeen: number;
  pricedModels: number;
  skippedNonUsd: number;
  skippedMissingTextPrice: number;
  skippedSubscription: number;
  skippedTiers: number;
  error?: string;
}

export interface ManagedModelPriceSyncV2Result {
  sources: ModelPriceSourceSyncSummaryV2[];
  catalogPrices: number;
  scannedModels: number;
  updated: number;
  unchanged: number;
  preservedCustom: number;
  unmatched: number;
  ambiguous: number;
  updatedSlugs: string[];
}

export interface ManagedModelV2Upsert {
  previousSlug?: string | null;
  model: ManagedModelV2;
}

export interface ManagedModelStateV2Update {
  slug: string;
  enabled: boolean;
  visibility: ModelVisibilityV2;
}

export interface ManagedModelBatchStateV2Update {
  slugs: string[];
  enabled: boolean;
  visibility: ModelVisibilityV2;
}

export interface ManagedModelImportV2Params {
  jsonContent: string;
  conflictStrategy: ManagedModelImportConflictStrategyV2;
}

export interface ManagedModelImportPreviewV2Result {
  added: string[];
  updated: string[];
  conflicts: string[];
  skipped: string[];
  errors: string[];
  ignoredFields: string[];
  committed: number;
}
