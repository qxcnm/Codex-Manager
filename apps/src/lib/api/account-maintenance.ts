function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function readStringField(payload: unknown, key: string, fallback = ""): string {
  const source = asRecord(payload);
  const value = source?.[key];
  return typeof value === "string" ? value.trim() : fallback;
}

function readBooleanField(payload: unknown, key: string, fallback = false): boolean {
  const source = asRecord(payload);
  const value = source?.[key];
  return typeof value === "boolean" ? value : fallback;
}

function readNumberField(payload: unknown, key: string, fallback = 0): number {
  const source = asRecord(payload);
  const value = source?.[key];
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) {
      return parsed;
    }
  }
  return fallback;
}

function readStringArrayField(payload: unknown, key: string): string[] {
  const source = asRecord(payload);
  const value = source?.[key];
  return Array.isArray(value)
    ? value
        .map((item) => (typeof item === "string" ? item.trim() : ""))
        .filter(Boolean)
    : [];
}

export interface AccountImportError {
  index: number;
  message: string;
}

export interface AccountImportResult {
  canceled?: boolean;
  total?: number;
  created?: number;
  updated?: number;
  failed?: number;
  errors?: AccountImportError[];
  fileCount?: number;
  directoryPath?: string;
  contents?: string[];
  importedAccountIds?: string[];
  usageRefreshAccountIds?: string[];
}

export interface AccountExportResult {
  canceled?: boolean;
  exported?: number;
  outputDir?: string;
}

export interface DeleteUnavailableFreeResult {
  deleted?: number;
}

export interface DeleteAccountsByStatusesResult {
  scanned?: number;
  deleted?: number;
  skippedStatus?: number;
  targetStatuses?: string[];
  deletedAccountIds?: string[];
}

export interface AccountWarmupItemResult {
  accountId: string;
  accountName: string;
  ok: boolean;
  message: string;
}

export interface AccountWarmupResult {
  requested?: number;
  succeeded?: number;
  failed?: number;
  results?: AccountWarmupItemResult[];
}

export interface AccountTestStartResult {
  testId?: string;
  started?: boolean;
  model?: string;
}

export function readAccountImportResult(payload: unknown): AccountImportResult {
  const source = asRecord(payload);
  const hasUsageRefreshAccountIds =
    source != null &&
    (Object.hasOwn(source, "usageRefreshAccountIds") ||
      Object.hasOwn(source, "usage_refresh_account_ids"));
  const errors = Array.isArray(source?.errors)
    ? source.errors
        .map((item) => {
          const entry = asRecord(item);
          if (!entry) {
            return null;
          }
          return {
            index: readNumberField(entry, "index"),
            message: readStringField(entry, "message"),
          };
        })
        .filter((item): item is AccountImportError => Boolean(item))
    : [];

  return {
    canceled: readBooleanField(payload, "canceled"),
    total: readNumberField(payload, "total"),
    created: readNumberField(payload, "created"),
    updated: readNumberField(payload, "updated"),
    failed: readNumberField(payload, "failed"),
    errors,
    fileCount: readNumberField(payload, "fileCount"),
    directoryPath: readStringField(payload, "directoryPath"),
    contents: readStringArrayField(payload, "contents"),
    importedAccountIds: readStringArrayField(payload, "importedAccountIds").concat(
      readStringArrayField(payload, "imported_account_ids")
    ),
    usageRefreshAccountIds: hasUsageRefreshAccountIds
      ? readStringArrayField(payload, "usageRefreshAccountIds").concat(
          readStringArrayField(payload, "usage_refresh_account_ids")
        )
      : undefined,
  };
}

export function readAccountExportResult(payload: unknown): AccountExportResult {
  return {
    canceled: readBooleanField(payload, "canceled"),
    exported: readNumberField(payload, "exported"),
    outputDir: readStringField(payload, "outputDir"),
  };
}

export function readDeleteUnavailableFreeResult(
  payload: unknown
): DeleteUnavailableFreeResult {
  return {
    deleted: readNumberField(payload, "deleted"),
  };
}

export function readDeleteAccountsByStatusesResult(
  payload: unknown
): DeleteAccountsByStatusesResult {
  return {
    scanned: readNumberField(payload, "scanned"),
    deleted: readNumberField(payload, "deleted"),
    skippedStatus: readNumberField(payload, "skippedStatus"),
    targetStatuses: readStringArrayField(payload, "targetStatuses"),
    deletedAccountIds: readStringArrayField(payload, "deletedAccountIds"),
  };
}

export function readAccountWarmupResult(payload: unknown): AccountWarmupResult {
  const source = asRecord(payload);
  const results = Array.isArray(source?.results)
    ? source.results
        .map((item) => {
          const entry = asRecord(item);
          if (!entry) {
            return null;
          }
          return {
            accountId: readStringField(entry, "accountId"),
            accountName: readStringField(entry, "accountName"),
            ok: readBooleanField(entry, "ok"),
            message: readStringField(entry, "message"),
          };
        })
        .filter((item): item is AccountWarmupItemResult => Boolean(item))
    : [];

  return {
    requested: readNumberField(payload, "requested"),
    succeeded: readNumberField(payload, "succeeded"),
    failed: readNumberField(payload, "failed"),
    results,
  };
}

export function readAccountTestStartResult(payload: unknown): AccountTestStartResult {
  return {
    testId: readStringField(payload, "testId"),
    started: readBooleanField(payload, "started"),
    model: readStringField(payload, "model"),
  };
}

export function readApiKeySecret(payload: unknown): string {
  return readStringField(payload, "key");
}
