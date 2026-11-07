import type { WebCommandDescriptor } from "./shared";

export function createModelDiscoveryWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_model_discovery_list: { rpcMethod: "modelDiscovery/list" },
    service_model_discovery_update: { rpcMethod: "modelDiscovery/update" },
  };
}
