"use client";

import { accountClient } from "@/lib/api/account-client";
import { listGrokCredentials, probeGrokCredentialModels } from "@/lib/api/grok-client";
import {
  listKiroCredentials,
  queryKiroCredentialQuota,
  refreshKiroCredential,
} from "@/lib/api/kiro-client";
import {
  type AdapterProbeJobSnapshot,
  cancelAdapterProbeJob,
  readAdapterProbeJob,
  readLatestAdapterProbeJob,
  startAdapterProbeJob,
} from "@/lib/api/adapter-probe-client";

export type AdapterProbePoolId = "codex" | "kiro" | "grok";

export interface AdapterProbeResult {
  requested: number;
  succeeded: number;
  failed: number;
  cancelled?: number;
}

export interface AdapterProbeProgress extends AdapterProbeResult {
  id: string;
  completed: number;
}

const AUTO_PROBE_STORAGE_PREFIX = "openruntime.adapter-pool.auto-probe.";
const AUTO_PROBE_CHANGED_EVENT = "openruntime:auto-probe-changed";
const DEFAULT_AUTO_PROBE = true;
const ACTIVE_JOB_STORAGE_PREFIX = "openruntime.adapter-pool.probe-job.";

const sleep = (milliseconds: number) =>
  new Promise<void>((resolve) => window.setTimeout(resolve, milliseconds));

function toProgress(job: AdapterProbeJobSnapshot): AdapterProbeProgress {
  return {
    id: job.id,
    requested: job.requested,
    completed: job.completed,
    succeeded: job.succeeded,
    failed: job.failed,
    cancelled: job.cancelled,
  };
}

function toResult(job: AdapterProbeJobSnapshot): AdapterProbeResult {
  return {
    requested: job.requested,
    succeeded: job.succeeded,
    failed: job.failed,
    cancelled: job.cancelled,
  };
}

async function waitForAdapterProbeJob(
  initialJob: AdapterProbeJobSnapshot,
  onProgress?: (progress: AdapterProbeProgress) => void,
  keepPolling: () => boolean = () => true,
): Promise<AdapterProbeJobSnapshot | null> {
  let job = initialJob;
  while (["queued", "running", "cancelling"].includes(job.status)) {
    onProgress?.(toProgress(job));
    await sleep(500);
    if (!keepPolling()) return null;
    job = await readAdapterProbeJob(job.id);
  }
  onProgress?.(toProgress(job));
  return job;
}

function writeActiveProbeJob(poolId: AdapterProbePoolId, jobId: string | null): void {
  if (typeof window === "undefined") return;
  const key = `${ACTIVE_JOB_STORAGE_PREFIX}${poolId}`;
  if (jobId) window.localStorage.setItem(key, jobId);
  else window.localStorage.removeItem(key);
}

export function readAdapterAutoProbe(poolId: AdapterProbePoolId): boolean {
  if (typeof window === "undefined") return DEFAULT_AUTO_PROBE;
  const stored = window.localStorage.getItem(`${AUTO_PROBE_STORAGE_PREFIX}${poolId}`);
  if (stored == null) return DEFAULT_AUTO_PROBE;
  return stored === "true";
}

export function writeAdapterAutoProbe(poolId: AdapterProbePoolId, enabled: boolean): void {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(`${AUTO_PROBE_STORAGE_PREFIX}${poolId}`, String(enabled));
  window.dispatchEvent(
    new CustomEvent(AUTO_PROBE_CHANGED_EVENT, { detail: { poolId, enabled } }),
  );
}

export function subscribeAdapterAutoProbe(
  listener: (poolId: AdapterProbePoolId, enabled: boolean) => void,
): () => void {
  if (typeof window === "undefined") return () => undefined;
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<{ poolId?: string; enabled?: boolean }>).detail;
    if (
      detail &&
      ["codex", "kiro", "grok"].includes(String(detail.poolId)) &&
      typeof detail.enabled === "boolean"
    ) {
      listener(detail.poolId as AdapterProbePoolId, detail.enabled);
    }
  };
  window.addEventListener(AUTO_PROBE_CHANGED_EVENT, handler);
  return () => window.removeEventListener(AUTO_PROBE_CHANGED_EVENT, handler);
}

async function runWithConcurrency<T>(
  items: readonly T[],
  concurrency: number,
  worker: (item: T) => Promise<unknown>,
  onProgress?: (progress: AdapterProbeProgress) => void,
): Promise<AdapterProbeResult> {
  const queue = [...items];
  let succeeded = 0;
  let failed = 0;
  const workerCount = Math.min(Math.max(1, concurrency), queue.length);

  await Promise.all(
    Array.from({ length: workerCount }, async () => {
      while (queue.length > 0) {
        const item = queue.shift();
        if (item === undefined) break;
        try {
          await worker(item);
          succeeded += 1;
        } catch {
          failed += 1;
        } finally {
          onProgress?.({
            id: String(item),
            requested: items.length,
            completed: succeeded + failed,
            succeeded,
            failed,
          });
        }
      }
    }),
  );

  return { requested: items.length, succeeded, failed };
}

export async function probeAdapterPool(
  poolId: AdapterProbePoolId,
  requestedIds?: readonly string[],
  onProgress?: (progress: AdapterProbeProgress) => void,
): Promise<AdapterProbeResult> {
  let ids = requestedIds?.length
    ? [...new Set(requestedIds.filter(Boolean))]
    : [];
  if (ids.length === 0 && poolId === "codex") {
    ids = (await accountClient.list()).items
      .filter((item) => item.status !== "disabled")
      .map((item) => item.id);
  } else if (ids.length === 0 && poolId === "kiro") {
    ids = (await listKiroCredentials())
      .filter((item) => item.status === "active")
      .map((item) => item.id);
  } else if (ids.length === 0) {
    ids = (await listGrokCredentials())
      .filter((item) => item.status === "active")
      .map((item) => item.id);
  }

  // 模型目录是 Codex 平台级目录，只刷新一次；账号探测由服务端任务执行。
  if (poolId === "codex") {
    void accountClient.listModels(true).catch(() => null);
  }
  const job = await startAdapterProbeJob({ poolId, credentialIds: ids });
  writeActiveProbeJob(poolId, job.id);
  const finished = await waitForAdapterProbeJob(job, onProgress);
  writeActiveProbeJob(poolId, null);
  return toResult(finished ?? job);
}

/** 恢复页面离开前仍在服务端运行的任务；停止轮询不会取消后台任务。 */
export async function resumeAdapterPoolProbe(
  poolId: AdapterProbePoolId,
  onProgress?: (progress: AdapterProbeProgress) => void,
  keepPolling: () => boolean = () => true,
): Promise<AdapterProbeResult | null> {
  const storedId =
    typeof window === "undefined"
      ? null
      : window.localStorage.getItem(`${ACTIVE_JOB_STORAGE_PREFIX}${poolId}`);
  const job = storedId
    ? await readAdapterProbeJob(storedId).catch(() => null)
    : await readLatestAdapterProbeJob(poolId);
  if (!job || !["queued", "running", "cancelling"].includes(job.status)) {
    writeActiveProbeJob(poolId, null);
    return null;
  }
  writeActiveProbeJob(poolId, job.id);
  const finished = await waitForAdapterProbeJob(job, onProgress, keepPolling);
  if (!finished) return null;
  writeActiveProbeJob(poolId, null);
  return toResult(finished);
}

export async function cancelAdapterPoolProbe(poolId: AdapterProbePoolId): Promise<boolean> {
  const storedId =
    typeof window === "undefined"
      ? null
      : window.localStorage.getItem(`${ACTIVE_JOB_STORAGE_PREFIX}${poolId}`);
  const job = storedId
    ? await readAdapterProbeJob(storedId).catch(() => null)
    : await readLatestAdapterProbeJob(poolId);
  if (!job || !["queued", "running", "cancelling"].includes(job.status)) return false;
  await cancelAdapterProbeJob(job.id);
  return true;
}


export async function refreshAdapterPool(
  poolId: AdapterProbePoolId,
  requestedIds?: readonly string[],
): Promise<AdapterProbeResult> {
  if (poolId === "codex") {
    const ids = requestedIds?.length
      ? [...new Set(requestedIds.filter(Boolean))]
      : (await accountClient.list()).items
          .filter((item) => item.status !== "disabled")
          .map((item) => item.id);
    return runWithConcurrency(ids, 4, async (id) => {
      // AT/RT 刷新失败时仍继续读取额度；AT-only 账号也能重新判断状态。
      await accountClient.refreshChatgptAuthTokens(id).catch(() => null);
      await accountClient.refreshUsage(id);
    });
  }

  if (poolId === "kiro") {
    const ids = requestedIds?.length
      ? [...new Set(requestedIds.filter(Boolean))]
      : (await listKiroCredentials())
          .filter((item) => item.status === "active")
          .map((item) => item.id);
    return runWithConcurrency(ids, 2, async (id) => {
      await refreshKiroCredential(id);
      await queryKiroCredentialQuota(id);
    });
  }

  const ids = requestedIds?.length
    ? [...new Set(requestedIds.filter(Boolean))]
    : (await listGrokCredentials())
        .filter((item) => item.status === "active")
        .map((item) => item.id);
  // Grok 的模型探测同时刷新登录状态、账号等级、额度窗口和限流状态。
  return runWithConcurrency(ids, 2, (id) => probeGrokCredentialModels(id));
}
