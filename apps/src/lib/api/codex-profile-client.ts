import { invoke, withAddr } from "./transport";
import type {
  CodexProfileAccountCandidate,
  CodexProfileApiKeyCandidate,
  CodexProfileCandidates,
  CodexProfileHistoryRepairSummary,
  CodexProfileHistoryRetention,
  CodexProfileMode,
  CodexProfilePruneHistoryBackupsResult,
  CodexProfileStatus,
} from "@/types";

function asObject(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asArray(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function asString(value: unknown, fallback = ""): string {
  return typeof value === "string" ? value.trim() : fallback;
}

function asBoolean(value: unknown, fallback = false): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function toNullableString(value: unknown): string | null {
  const text = asString(value);
  return text || null;
}

function toNullableNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function asNumber(value: unknown, fallback = 0): number {
  const parsed = toNullableNumber(value);
  return parsed ?? fallback;
}

function asStringArray(value: unknown): string[] {
  return asArray(value)
    .map((item) => asString(item))
    .filter(Boolean);
}

function normalizeMode(value: unknown): CodexProfileMode {
  const mode = asString(value).toLowerCase();
  if (
    mode === "missing" ||
    mode === "unmanaged" ||
    mode === "direct_account" ||
    mode === "gateway" ||
    mode === "managed_unknown"
  ) {
    return mode;
  }
  return "unmanaged";
}

export const CODEX_PROFILE_STATUS_QUERY_KEY = ["codex-profile", "status"] as const;
export const CODEX_PROFILE_CANDIDATES_QUERY_KEY = [
  "codex-profile",
  "candidates",
] as const;

export function normalizeCodexProfileStatus(payload: unknown): CodexProfileStatus {
  const source = asObject(payload);
  return {
    codexHome: asString(source.codexHome ?? source.codex_home),
    authPath: asString(source.authPath ?? source.auth_path),
    configPath: asString(source.configPath ?? source.config_path),
    managedStorageRoot: asString(
      source.managedStorageRoot ?? source.managed_storage_root,
    ),
    markerPath: asString(source.markerPath ?? source.marker_path),
    historyBackupRoot: asString(
      source.historyBackupRoot ?? source.history_backup_root,
    ),
    historyBackupCount: asNumber(
      source.historyBackupCount ?? source.history_backup_count,
    ),
    historyBackupBytes: asNumber(
      source.historyBackupBytes ?? source.history_backup_bytes,
    ),
    historyRetention: normalizeCodexProfileHistoryRetention(
      source.historyRetention ?? source.history_retention,
    ),
    mode: normalizeMode(source.mode),
    selectedAccountId: toNullableString(
      source.selectedAccountId ?? source.selected_account_id,
    ),
    selectedApiKeyId: toNullableString(source.selectedApiKeyId ?? source.selected_api_key_id),
    gatewayBaseUrl: toNullableString(source.gatewayBaseUrl ?? source.gateway_base_url),
    providerId: asString(source.providerId ?? source.provider_id) || "cm",
    hasBackup: asBoolean(source.hasBackup ?? source.has_backup),
    lastAppliedAt: toNullableNumber(source.lastAppliedAt ?? source.last_applied_at),
    profileWritable: asBoolean(source.profileWritable ?? source.profile_writable),
    error: toNullableString(source.error),
    warnings: asStringArray(source.warnings),
    historyRepair: normalizeCodexProfileHistoryRepair(
      source.historyRepair ?? source.history_repair,
    ),
  };
}

export function normalizeCodexProfileHistoryRetention(
  payload: unknown,
): CodexProfileHistoryRetention {
  const source = asObject(payload);
  return {
    maxHistoryBackupsPerProfile: asNumber(
      source.maxHistoryBackupsPerProfile ??
        source.max_history_backups_per_profile,
      3,
    ),
    maxHistoryBackupAgeDays: asNumber(
      source.maxHistoryBackupAgeDays ?? source.max_history_backup_age_days,
      7,
    ),
    minHistoryBackupsPerProfile: asNumber(
      source.minHistoryBackupsPerProfile ??
        source.min_history_backups_per_profile,
      1,
    ),
  };
}

export function normalizeCodexProfileHistoryRepair(
  payload: unknown,
): CodexProfileHistoryRepairSummary | null {
  if (!payload) return null;
  const source = asObject(payload);
  return {
    codexHome: asString(source.codexHome ?? source.codex_home),
    targetProvider: asString(source.targetProvider ?? source.target_provider),
    changedRolloutFileCount: asNumber(
      source.changedRolloutFileCount ?? source.changed_rollout_file_count,
    ),
    updatedSqliteRowCount: asNumber(
      source.updatedSqliteRowCount ?? source.updated_sqlite_row_count,
    ),
    addedSessionIndexEntryCount: asNumber(
      source.addedSessionIndexEntryCount ??
        source.added_session_index_entry_count,
    ),
    backupDir: toNullableString(source.backupDir ?? source.backup_dir),
    warnings: asStringArray(source.warnings),
    message: asString(source.message),
  };
}

function normalizeAccountCandidate(
  payload: unknown,
): CodexProfileAccountCandidate | null {
  const source = asObject(payload);
  const id = asString(source.id);
  if (!id) return null;
  return {
    id,
    label: asString(source.label) || id,
    groupName: toNullableString(source.groupName ?? source.group_name),
    status: asString(source.status) || "active",
    chatgptAccountId: toNullableString(
      source.chatgptAccountId ?? source.chatgpt_account_id,
    ),
    workspaceId: toNullableString(source.workspaceId ?? source.workspace_id),
    issuer: asString(source.issuer),
    lastRefresh: toNullableNumber(source.lastRefresh ?? source.last_refresh),
  };
}

function normalizeApiKeyCandidate(
  payload: unknown,
): CodexProfileApiKeyCandidate | null {
  const source = asObject(payload);
  const id = asString(source.id);
  if (!id) return null;
  return {
    id,
    name: toNullableString(source.name),
    status: asString(source.status) || "active",
    modelSlug: toNullableString(source.modelSlug ?? source.model_slug),
    reasoningEffort: toNullableString(
      source.reasoningEffort ?? source.reasoning_effort,
    ),
  };
}

export function normalizeCodexProfileCandidates(
  payload: unknown,
): CodexProfileCandidates {
  const source = asObject(payload);
  return {
    accounts: asArray(source.accounts)
      .map(normalizeAccountCandidate)
      .filter((item): item is CodexProfileAccountCandidate => Boolean(item)),
    apiKeys: asArray(source.apiKeys ?? source.api_keys)
      .map(normalizeApiKeyCandidate)
      .filter((item): item is CodexProfileApiKeyCandidate => Boolean(item)),
  };
}

export function normalizeCodexProfilePruneHistoryBackupsResult(
  payload: unknown,
): CodexProfilePruneHistoryBackupsResult {
  const source = asObject(payload);
  return {
    codexHome: asString(source.codexHome ?? source.codex_home),
    historyBackupRoot: asString(
      source.historyBackupRoot ?? source.history_backup_root,
    ),
    beforeCount: asNumber(source.beforeCount ?? source.before_count),
    afterCount: asNumber(source.afterCount ?? source.after_count),
    removedCount: asNumber(source.removedCount ?? source.removed_count),
    beforeBytes: asNumber(source.beforeBytes ?? source.before_bytes),
    afterBytes: asNumber(source.afterBytes ?? source.after_bytes),
    removedBytes: asNumber(source.removedBytes ?? source.removed_bytes),
    retention: normalizeCodexProfileHistoryRetention(source.retention),
    warnings: asStringArray(source.warnings),
  };
}

export const codexProfileClient = {
  async get(codexHome?: string | null): Promise<CodexProfileStatus> {
    const result = await invoke<unknown>(
      "service_codex_profile_get",
      withAddr({ codexHome: codexHome || null }),
    );
    return normalizeCodexProfileStatus(result);
  },
  async setConfig(codexHome: string): Promise<CodexProfileStatus> {
    const result = await invoke<unknown>(
      "service_codex_profile_set_config",
      withAddr({ codexHome }),
    );
    return normalizeCodexProfileStatus(result);
  },
  async listCandidates(): Promise<CodexProfileCandidates> {
    const result = await invoke<unknown>(
      "service_codex_profile_list_candidates",
      withAddr(),
    );
    return normalizeCodexProfileCandidates(result);
  },
  async applyDirectAccount(params: {
    accountId: string;
    codexHome?: string | null;
  }): Promise<CodexProfileStatus> {
    const result = await invoke<unknown>(
      "service_codex_profile_apply_direct_account",
      withAddr({
        accountId: params.accountId,
        codexHome: params.codexHome || null,
      }),
    );
    return normalizeCodexProfileStatus(result);
  },
  async applyGateway(params: {
    apiKeyId: string;
    codexHome?: string | null;
    baseUrl?: string | null;
  }): Promise<CodexProfileStatus> {
    const result = await invoke<unknown>(
      "service_codex_profile_apply_gateway",
      withAddr({
        apiKeyId: params.apiKeyId,
        codexHome: params.codexHome || null,
        baseUrl: params.baseUrl || null,
      }),
    );
    return normalizeCodexProfileStatus(result);
  },
  async restore(codexHome?: string | null): Promise<CodexProfileStatus> {
    const result = await invoke<unknown>(
      "service_codex_profile_restore",
      withAddr({ codexHome: codexHome || null }),
    );
    return normalizeCodexProfileStatus(result);
  },
  async repairHistory(
    codexHome?: string | null,
  ): Promise<CodexProfileHistoryRepairSummary> {
    const result = await invoke<unknown>(
      "service_codex_profile_repair_history",
      withAddr({ codexHome: codexHome || null }),
    );
    return (
      normalizeCodexProfileHistoryRepair(result) || {
        codexHome: codexHome || "",
        targetProvider: "",
        changedRolloutFileCount: 0,
        updatedSqliteRowCount: 0,
        addedSessionIndexEntryCount: 0,
        backupDir: null,
        warnings: ["Invalid history repair response"],
        message: "Invalid history repair response",
      }
    );
  },
  async pruneHistoryBackups(
    codexHome?: string | null,
  ): Promise<CodexProfilePruneHistoryBackupsResult> {
    const result = await invoke<unknown>(
      "service_codex_profile_prune_history_backups",
      withAddr({ codexHome: codexHome || null }),
    );
    return normalizeCodexProfilePruneHistoryBackupsResult(result);
  },
};
