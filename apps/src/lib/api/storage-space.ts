import { invoke, withAddr } from "@/lib/api/transport";

export type StorageAutoVacuumMode = "none" | "full" | "incremental";

export interface StorageSpaceUsage {
  backend: "sqlite" | "remote";
  pageSize: number;
  pageCount: number;
  freelistCount: number;
  /** Bytes held by live pages; this, not the file size, is the real usage. */
  usedBytes: number;
  /** Free pages that can be returned to the operating system. */
  reclaimableBytes: number;
  fileBytes: number | null;
  walBytes: number | null;
  autoVacuum: StorageAutoVacuumMode;
  purgePending: boolean;
  checkpointPending: boolean;
  reclaimRunning: boolean;
  rebuildRunning: boolean;
  lastError: string | null;
}

export interface StorageReclaimResult {
  mode: "incremental" | "full" | "rebuild";
  started: boolean;
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function asNumber(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function asNullableNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function readStorageSpaceUsage(value: unknown): StorageSpaceUsage {
  const source = asRecord(value);
  const autoVacuum = source.autoVacuum;
  return {
    backend: source.backend === "remote" ? "remote" : "sqlite",
    pageSize: asNumber(source.pageSize),
    pageCount: asNumber(source.pageCount),
    freelistCount: asNumber(source.freelistCount),
    usedBytes: asNumber(source.usedBytes),
    reclaimableBytes: asNumber(source.reclaimableBytes),
    fileBytes: asNullableNumber(source.fileBytes),
    walBytes: asNullableNumber(source.walBytes),
    autoVacuum:
      autoVacuum === "incremental" || autoVacuum === "full" ? autoVacuum : "none",
    purgePending: source.purgePending === true,
    checkpointPending: source.checkpointPending === true,
    reclaimRunning: source.reclaimRunning === true,
    rebuildRunning: source.rebuildRunning === true,
    lastError:
      typeof source.lastError === "string" && source.lastError.trim()
        ? source.lastError
        : null,
  };
}

export async function getStorageSpaceUsage(): Promise<StorageSpaceUsage> {
  const result = await invoke<unknown>("service_storage_space_usage", withAddr());
  return readStorageSpaceUsage(result);
}

export async function reclaimStorageSpace(
  rebuild: boolean,
): Promise<StorageReclaimResult> {
  const result = asRecord(
    await invoke<unknown>("service_storage_reclaim", withAddr({ rebuild })),
  );
  const mode = result.mode;
  return {
    mode: mode === "rebuild" || mode === "full" ? mode : "incremental",
    started: result.started === true,
  };
}
