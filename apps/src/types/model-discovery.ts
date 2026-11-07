export type ModelDiscoverySourceKind = "openai_account" | "aggregate_api";

export interface ModelDiscoveryTask {
  sourceKind: ModelDiscoverySourceKind;
  sourceId: string;
  enabled: boolean;
  intervalMinutes: number;
  running: boolean;
  lastStartedAt: number | null;
  lastFinishedAt: number | null;
  lastSuccessAt: number | null;
  nextRunAt: number | null;
  lastError: string | null;
  lastWarning: string | null;
  discoveredCount: number;
  createdModelCount: number;
  addedRouteCount: number;
}

export interface ModelDiscoveryList {
  items: ModelDiscoveryTask[];
}

export interface ModelDiscoveryUpdateInput {
  sourceKind: ModelDiscoverySourceKind;
  sourceId: string;
  enabled: boolean;
  intervalMinutes?: number;
}
