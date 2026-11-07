import type { WebCommandDescriptor } from "./shared";

export function createServiceHealthWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_health_summary: {
      rpcMethod: "serviceHealth/summary",
      mapParams: () => ({}),
    },
    service_health_report: {
      rpcMethod: "serviceHealth/report",
      mapParams: (params) => ({
        provider: params?.provider,
        symptom: params?.symptom,
        region: params?.region,
      }),
    },
    service_health_resolve: {
      rpcMethod: "serviceHealth/resolve",
      mapParams: (params) => ({ id: params?.id }),
    },
  };
}
