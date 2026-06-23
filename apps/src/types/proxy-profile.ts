import type { ProxyGeoLike } from "@/lib/utils/proxy-geo";

export interface ProxyProfile extends ProxyGeoLike {
  id: string;
  name: string;
  proxyUrlRedacted: string;
  scheme: string | null;
  host: string | null;
  port: number | null;
  enabled: boolean;
  status: string;
  lastError: string | null;
  lastUrlLatencyMs: number | null;
  lastDownloadMbps: number | null;
  lastUploadMbps: number | null;
  lastTestedAt: number | null;
  asn: number | null;
  asOrg: string | null;
  tagsJson: string | null;
  notes: string | null;
  accountsCount?: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface ProxyProfileListResult {
  items: ProxyProfile[];
}

export interface ProxyProfileUrlTestResult {
  id: number;
  proxyProfileId: string;
  status: string;
  urlLatencyMs: number | null;
  statusCode: number | null;
  testUrl: string;
  finalUrl: string | null;
  redirected: boolean;
  testedAt: number;
  errorCode: string | null;
  error: string | null;
}

export interface ProxyProfileUrlTestListResult {
  items: ProxyProfileUrlTestResult[];
}

export type ProxyTestJobScope = "system_proxy" | "account_proxy";

export type ProxyTestJobKind = "latency" | "speed" | "cloudflare_style_speed";


export type ProxyTestJobStatus =
  | "queued"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";

export interface SpeedSample {
  payloadBytes: number;
  durationMs: number;
  mbps: number;
}

export interface SpeedMetricSummary {
  median: number;
  average: number;
  p90: number;
  best: number;
}

export interface DownloadDiagnosticResult {
  providerId: string;
  fileSizeId: string;
  status: string;
  error: string | null;
  downloadedBytes: number;
  durationMs: number;
  mbps: number;
}

export type ProxyTestJobPhase =
  | "queued"
  | "preflight"
  | "latency"
  | "download"
  | "upload"
  | "diagnostics"
  | "saving"
  | "done";

export type CfStyleStatus = "ok" | "partial" | "failed" | "timeout" | "cancelled";
export type ThroughputDirection = "download" | "upload";
export type CfStyleRunStatus = "ok" | "failed" | "timeout" | "cancelled";

export interface CfStyleLatencyResult {
  rawSamplesMs: number[];
  minMs: number;
  avgMs: number;
  medianMs: number;
  p90Ms: number;
  p95Ms: number;
  jitterMs: number;
}

export interface CfStyleThroughputRun {
  payloadBytes: number;
  transferredBytes: number;
  totalDurationMs: number;
  ttfbMs: number | null;
  transferDurationMs: number;
  rawMbps: number;
  adjustedMbps: number;
  status: CfStyleRunStatus;
  error: string | null;
}

export interface CfStyleThroughputResult {
  direction: ThroughputDirection;
  runs: CfStyleThroughputRun[];
  finalMbps: number;
  rawFinalMbps: number;
  adjustedFinalMbps: number;
  avgMbps: number;
  medianMbps: number;
  p90Mbps: number;
  maxMbps: number;
  totalBytes: number;
  totalDurationMs: number;
}

export interface CfStyleEndpointInfo {
  observedIp: string | null;
  observedCountry: string | null;
  observedColo: string | null;
}

export interface CfStyleUsedProxy {
  proxyUrlRedacted: string;
  proxyScheme: string;
  dnsNote: string;
}

export interface CfStyleSpeedTestError {
  phase: string;
  message: string;
}

export interface CfStyleResult {
  status: CfStyleStatus;
  latency: CfStyleLatencyResult | null;
  download: CfStyleThroughputResult | null;
  upload: CfStyleThroughputResult | null;
  usedProxy: CfStyleUsedProxy | null;
  endpointInfo: CfStyleEndpointInfo;
  startedAt: string;
  finishedAt: string;
  durationMs: number;
  errors: CfStyleSpeedTestError[];
}

export interface ProxyTestJobState {
  jobId: string;
  scope: ProxyTestJobScope;
  proxyProfileId: string | null;
  accountId: string | null;
  kind: ProxyTestJobKind;
  status: ProxyTestJobStatus;
  phase: ProxyTestJobPhase;
  downloadedBytes: number;
  uploadedBytes: number;
  downloadMbps: number | null;
  uploadMbps: number | null;
  latencyMs: number | null;
  startedAt: number;
  updatedAt: number;
  error: string | null;
  observedIp?: string | null;
  observedCountry?: string | null;
  observedColo?: string | null;
  downloadSamples?: SpeedSample[];
  uploadSamples?: SpeedSample[];
  downloadSummary?: SpeedMetricSummary | null;
  uploadSummary?: SpeedMetricSummary | null;
  downloadDiagnostics?: DownloadDiagnosticResult[];
  cfStyleResult?: CfStyleResult | null;
}


export interface ProxyTestProviderFilePreset {
  fileSizeId: string;
  downloadUrl: string;
  readLimitBytes: number | null;
}

export interface ProxyTestSpeedProviderPreset {
  id: string;
  label: string;
  providerFamily: string;
  files: ProxyTestProviderFilePreset[];
}

export interface ProxyTestFileSizePreset {
  id: string;
  label: string;
  bytes: number;
  warning: boolean;
}

export interface ProxyTestUploadEndpointStatus {
  status: string;
  configured: boolean;
  source: string;
  url: string | null;
}

export interface ProxyTestDefaults {
  speedProviderId: string;
  fileSizeId: string;
  latencyPresetId: string;
}

export interface ProxyTestPresetsResult {
  speedProviders: ProxyTestSpeedProviderPreset[];
  fileSizes: ProxyTestFileSizePreset[];
  defaults: ProxyTestDefaults;
  uploadEndpoint: ProxyTestUploadEndpointStatus;
}

export interface ProxySpeedTestEntry {
  id: number;
  scope: string;
  proxyProfileId: string | null;
  accountId: string | null;
  status: string;
  provider: string;
  observedIp: string | null;
  observedCountry: string | null;
  observedColo: string | null;
  maxPayloadBytes: number | null;
  samplesJson: string | null;
  downloadSummaryJson: string | null;
  uploadSummaryJson: string | null;
  startedAt: number;
  finishedAt: number;
  errorCode: string | null;
  error: string | null;
}

export interface ProxySpeedTestListResult {
  items: ProxySpeedTestEntry[];
}

export interface ProxyDiagnosticTestEntry {
  id: number;
  scope: string;
  proxyProfileId: string | null;
  accountId: string | null;
  status: string;
  provider: string;
  fileSizeId: string;
  downloadedBytes: number | null;
  durationMs: number | null;
  mbps: number | null;
  testedAt: number;
  error: string | null;
}

export interface ProxyDiagnosticTestListResult {
  items: ProxyDiagnosticTestEntry[];
}

export interface AccountProxyUrlTestEntry {
  id: number;
  accountId: string;
  status: string;
  urlLatencyMs: number | null;
  statusCode: number | null;
  testUrl: string;
  finalUrl: string | null;
  redirected: boolean;
  testedAt: number;
  errorCode: string | null;
  error: string | null;
}

export interface AccountProxyUrlTestListResult {
  items: AccountProxyUrlTestEntry[];
}

