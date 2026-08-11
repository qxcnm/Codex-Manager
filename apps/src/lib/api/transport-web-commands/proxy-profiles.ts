import type { WebCommandDescriptor } from "./shared";

export function createProxyProfileWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_proxy_profiles_list: { rpcMethod: "proxyProfiles/list" },
    service_proxy_profile_upsert: { rpcMethod: "proxyProfiles/upsert" },
    service_proxy_profile_delete: { rpcMethod: "proxyProfiles/delete" },
    service_proxy_profile_probe: { rpcMethod: "proxyProfiles/probe" },
    service_proxy_profile_bind_accounts: { rpcMethod: "proxyProfiles/bindAccounts" },
  };
}
