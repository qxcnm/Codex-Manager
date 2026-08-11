import { invoke, withAddr } from "./transport";

export type AdapterProbePoolId = "codex" | "kiro" | "grok";
export type AdapterProbeJobStatus =
  | "queued"
  | "running"
  | "cancelling"
  | "completed"
  | "cancelled";

export interface AdapterProbeJobItemResult {
  credentialId: string;
  status: "available" | "failed" | string;
  errorCode: string | null;
  attempts: number;
  latencyMs: number;
}

export interface AdapterProbeJobSnapshot {
  id: string;
  poolId: AdapterProbePoolId;
  status: AdapterProbeJobStatus;
  requested: number;
  completed: number;
  succeeded: number;
  failed: number;
  cancelled: number;
  concurrency: number;
  createdAt: number;
  startedAt: number | null;
  finishedAt: number | null;
  results: AdapterProbeJobItemResult[];
}

export async function startAdapterProbeJob(input: {
  poolId: AdapterProbePoolId;
  credentialIds: string[];
  concurrency?: number;
}): Promise<AdapterProbeJobSnapshot> {
  return invoke<AdapterProbeJobSnapshot>(
    "service_adapter_probe_job_start",
    withAddr(input),
  );
}

export async function readAdapterProbeJob(id: string): Promise<AdapterProbeJobSnapshot> {
  return invoke<AdapterProbeJobSnapshot>(
    "service_adapter_probe_job_read",
    withAddr({ id }),
  );
}

export async function readLatestAdapterProbeJob(
  poolId: AdapterProbePoolId,
): Promise<AdapterProbeJobSnapshot | null> {
  return invoke<AdapterProbeJobSnapshot | null>(
    "service_adapter_probe_job_latest",
    withAddr({ poolId }),
  );
}

export async function cancelAdapterProbeJob(id: string): Promise<AdapterProbeJobSnapshot> {
  return invoke<AdapterProbeJobSnapshot>(
    "service_adapter_probe_job_cancel",
    withAddr({ id }),
  );
}
