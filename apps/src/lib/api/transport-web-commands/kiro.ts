import type { WebCommandDescriptor } from "./shared";

export function createKiroWebCommands(): Record<string, WebCommandDescriptor> {
  return {
    service_kiro_credentials_list: { rpcMethod: "kiro/credentials/list" },
    service_kiro_credential_probe_models: { rpcMethod: "kiro/credentials/probeModels" },
    service_kiro_credential_set_enabled: { rpcMethod: "kiro/credentials/setEnabled" },
    service_kiro_credential_delete: { rpcMethod: "kiro/credentials/delete" },
    service_kiro_credential_update_routing: { rpcMethod: "kiro/credentials/updateRouting" },
    service_kiro_credential_refresh: { rpcMethod: "kiro/credentials/refresh" },
    service_kiro_credential_quota: { rpcMethod: "kiro/credentials/quota" },
    service_kiro_import_preview: { rpcMethod: "kiro/import/preview" },
    service_kiro_import_commit: { rpcMethod: "kiro/import/commit" },
  };
}
