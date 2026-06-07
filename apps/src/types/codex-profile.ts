export type CodexProfileMode =
  | "missing"
  | "unmanaged"
  | "direct_account"
  | "gateway"
  | "managed_unknown";

export interface CodexProfileStatus {
  codexHome: string;
  authPath: string;
  configPath: string;
  managedStorageRoot: string;
  markerPath: string;
  historyBackupRoot: string;
  historyBackupCount: number;
  historyBackupBytes: number;
  historyRetention: CodexProfileHistoryRetention;
  mode: CodexProfileMode;
  selectedAccountId: string | null;
  selectedApiKeyId: string | null;
  gatewayBaseUrl: string | null;
  providerId: string;
  hasBackup: boolean;
  lastAppliedAt: number | null;
  profileWritable: boolean;
  error: string | null;
  warnings: string[];
  historyRepair: CodexProfileHistoryRepairSummary | null;
}

export interface CodexProfileHistoryRetention {
  maxHistoryBackupsPerProfile: number;
  maxHistoryBackupAgeDays: number;
  minHistoryBackupsPerProfile: number;
}

export interface CodexProfileHistoryRepairSummary {
  codexHome: string;
  targetProvider: string;
  changedRolloutFileCount: number;
  updatedSqliteRowCount: number;
  addedSessionIndexEntryCount: number;
  backupDir: string | null;
  warnings: string[];
  message: string;
}

export interface CodexProfilePruneHistoryBackupsResult {
  codexHome: string;
  historyBackupRoot: string;
  beforeCount: number;
  afterCount: number;
  removedCount: number;
  beforeBytes: number;
  afterBytes: number;
  removedBytes: number;
  retention: CodexProfileHistoryRetention;
  warnings: string[];
}

export interface CodexProfileAccountCandidate {
  id: string;
  label: string;
  groupName: string | null;
  status: string;
  chatgptAccountId: string | null;
  workspaceId: string | null;
  issuer: string;
  lastRefresh: number | null;
}

export interface CodexProfileApiKeyCandidate {
  id: string;
  name: string | null;
  status: string;
  modelSlug: string | null;
  reasoningEffort: string | null;
}

export interface CodexProfileCandidates {
  accounts: CodexProfileAccountCandidate[];
  apiKeys: CodexProfileApiKeyCandidate[];
}
