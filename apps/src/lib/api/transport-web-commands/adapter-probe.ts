import type { WebCommandDescriptor } from "./shared";

export function createAdapterProbeWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_adapter_probe_job_start: { rpcMethod: "adapterProbe/job/start" },
    service_adapter_probe_job_read: { rpcMethod: "adapterProbe/job/read" },
    service_adapter_probe_job_latest: { rpcMethod: "adapterProbe/job/latest" },
    service_adapter_probe_job_cancel: { rpcMethod: "adapterProbe/job/cancel" },
  };
}
