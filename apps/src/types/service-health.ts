export type ServiceHealthSource = "local";

export type ServiceHealthProvider = "codex" | "claude" | "unknown";

export type ServiceHealthRegion =
  | "unknown"
  | "apac"
  | "europe"
  | "north_america"
  | "south_america"
  | "middle_east"
  | "africa"
  | "oceania";

export type ServiceHealthSymptom = "unavailable" | "slow" | "rate_limited" | "errors";

export type ServiceHealthCoverage = "this_deployment";

export type ServiceHealthScope = "insufficient_data";

export type ServiceHealthStatus = "unknown" | "healthy" | "degraded";

export interface ServiceHealthProviderSummary {
  provider: ServiceHealthProvider;
  status: ServiceHealthStatus;
  sampleCount: number;
  successCount: number;
  rateLimitedCount: number;
  upstreamErrorCount: number;
  localErrorCount: number;
  networkErrorCount: number;
  unknownErrorCount: number;
  availabilityPercent: number | null;
  avgFirstResponseMs: number | null;
  p95FirstResponseMs: number | null;
  activeReportCount: number;
  reporterCount: number;
  sourceCount: number;
  regions: ServiceHealthRegion[];
}

export interface ServiceHealthReportInput {
  provider: ServiceHealthProvider;
  symptom: ServiceHealthSymptom;
  region: ServiceHealthRegion;
}

export interface ServiceHealthReport extends ServiceHealthReportInput {
  id: string;
  createdAt: number;
  updatedAt: number;
  expiresAt: number;
}

export interface ServiceHealthSummary {
  source: ServiceHealthSource;
  coverage: ServiceHealthCoverage;
  windowStart: number;
  windowEnd: number;
  updatedAt: number;
  sampled: boolean;
  sampleLimit: 2000;
  sampleCount: number;
  sourceCount: number;
  regionCount: number;
  scope: ServiceHealthScope;
  providers: ServiceHealthProviderSummary[];
  myReports: ServiceHealthReport[];
}

export interface ServiceHealthReportResult {
  report: ServiceHealthReport;
}

export interface ServiceHealthActionResult {
  ok: true;
}
