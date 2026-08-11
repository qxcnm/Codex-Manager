import { invoke, withAddr } from "./transport";

export interface KiroCredentialSummary {
  id: string;
  authMethod: "social" | "idc" | string;
  email: string | null;
  authRegion: string | null;
  apiRegion: string | null;
  subscription: string | null;
  status: string;
  priority: number;
  weight: number;
  creditLimit: number | null;
  creditUsed: number | null;
  expiresAt: number | null;
  cooldownUntil: number | null;
  failureCount: number;
  requestCount: number;
  successCount: number;
  lastLatencyMs: number | null;
  proxyUrl: string | null;
  proxyUsername: string | null;
  availableModels: string[];
  modelProbeCheckedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface KiroImportPreviewItem {
  sourceIndex: number;
  authMethod: string;
  email: string | null;
  region: string | null;
  subscription: string | null;
  confidence: number;
  duplicateHint: string;
  isUpdate: boolean;
  mappedFields: string[];
  metadata: Record<string, unknown>;
}

export interface KiroImportIssue {
  sourceIndex: number;
  message: string;
}

export interface KiroImportPreview {
  items: KiroImportPreviewItem[];
  issues: KiroImportIssue[];
}

export interface KiroImportResult {
  imported: number;
  failed: number;
  issues: KiroImportIssue[];
}

export interface KiroImportMapping {
  refreshToken: string;
  accessToken?: string;
  clientId?: string;
  clientSecret?: string;
  authMethod?: string;
  email?: string;
  region?: string;
  authRegion?: string;
  apiRegion?: string;
  subscription?: string;
  expiresAt?: string;
  proxyUrl?: string;
  proxyUsername?: string;
  proxyPassword?: string;
  creditLimit?: string;
  creditUsed?: string;
  machineId?: string;
}

export async function listKiroCredentials(): Promise<KiroCredentialSummary[]> {
  return invoke<KiroCredentialSummary[]>("service_kiro_credentials_list", withAddr());
}

export interface KiroModelProbeSummary {
  credentialId: string;
  availableModels: string[];
  checked: number;
  unknown: number;
  checkedAt: number;
}

export async function probeKiroCredentialModels(id: string): Promise<KiroModelProbeSummary> {
  return invoke<KiroModelProbeSummary>(
    "service_kiro_credential_probe_models",
    withAddr({ id }),
  );
}

export async function setKiroCredentialEnabled(id: string, enabled: boolean): Promise<boolean> {
  return invoke<boolean>("service_kiro_credential_set_enabled", withAddr({ id, enabled }));
}

export async function deleteKiroCredential(id: string): Promise<boolean> {
  return invoke<boolean>("service_kiro_credential_delete", withAddr({ id }));
}

export interface KiroCredentialRoutingUpdate {
  id: string;
  priority: number;
  weight: number;
  authRegion: string | null;
  apiRegion: string | null;
  proxyUrl: string | null;
  proxyUsername: string | null;
}

export async function updateKiroCredentialRouting(
  input: KiroCredentialRoutingUpdate,
): Promise<boolean> {
  return invoke<boolean>("service_kiro_credential_update_routing", withAddr({ ...input }));
}

export interface KiroQuotaSummary {
  credentialId: string;
  subscription: string | null;
  creditLimit: number;
  creditUsed: number;
  remaining: number;
  nextResetAt: number | null;
}

export async function refreshKiroCredential(id: string): Promise<boolean> {
  return invoke<boolean>("service_kiro_credential_refresh", withAddr({ id }));
}

export async function queryKiroCredentialQuota(id: string): Promise<KiroQuotaSummary> {
  return invoke<KiroQuotaSummary>("service_kiro_credential_quota", withAddr({ id }));
}

export async function previewKiroImport(
  json: string,
  mapping?: KiroImportMapping,
): Promise<KiroImportPreview> {
  return invoke<KiroImportPreview>(
    "service_kiro_import_preview",
    withAddr({ json, mapping: mapping ?? null }),
  );
}

export async function commitKiroImport(
  json: string,
  mapping?: KiroImportMapping,
): Promise<KiroImportResult> {
  return invoke<KiroImportResult>(
    "service_kiro_import_commit",
    withAddr({ json, mapping: mapping ?? null }),
  );
}
