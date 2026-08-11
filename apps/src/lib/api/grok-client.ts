import { invoke, withAddr } from "./transport";

export interface GrokCredentialSummary {
  id: string;
  accountMasked: string;
  status: string;
  priority: number;
  weight: number;
  proxyUrl: string | null;
  webTier: string;
  availableModels: string[];
  quotaWindows: GrokQuotaWindow[];
  expiresAt: number | null;
  cooldownUntil: number | null;
  failureCount: number;
  requestCount: number;
  successCount: number;
  lastLatencyMs: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface GrokQuotaWindow {
  credentialId: string;
  mode: string;
  remainingQueries: number;
  totalQueries: number;
  windowSizeSeconds: number;
  resetAt: number;
  checkedAt: number;
}

export interface GrokModelProbeSummary {
  credentialId: string;
  tier: string;
  availableModels: string[];
  quotaWindows: Array<Omit<GrokQuotaWindow, "credentialId" | "checkedAt">>;
  checkedAt: number;
}

export interface GrokImportIssue {
  sourceIndex: number;
  message: string;
}

export interface GrokImportPreviewItem {
  sourceIndex: number;
  accountMasked: string;
  confidence: number;
  isUpdate: boolean;
  mappedFields: string[];
}

export interface GrokImportPreview {
  items: GrokImportPreviewItem[];
  issues: GrokImportIssue[];
}

export interface GrokImportResult {
  imported: number;
  failed: number;
  issues: GrokImportIssue[];
}

export async function listGrokCredentials(): Promise<GrokCredentialSummary[]> {
  return invoke<GrokCredentialSummary[]>("service_grok_credentials_list", withAddr());
}

export async function deleteGrokCredential(id: string): Promise<boolean> {
  return invoke<boolean>("service_grok_credential_delete", withAddr({ id }));
}

export async function setGrokCredentialEnabled(id: string, enabled: boolean): Promise<boolean> {
  return invoke<boolean>("service_grok_credential_set_enabled", withAddr({ id, enabled }));
}

export async function probeGrokCredentialModels(id: string): Promise<GrokModelProbeSummary> {
  return invoke<GrokModelProbeSummary>(
    "service_grok_credential_probe_models",
    withAddr({ id }),
  );
}

export async function previewGrokImport(text: string): Promise<GrokImportPreview> {
  return invoke<GrokImportPreview>("service_grok_import_preview", withAddr({ text }));
}

export async function commitGrokImport(text: string): Promise<GrokImportResult> {
  return invoke<GrokImportResult>("service_grok_import_commit", withAddr({ text }));
}
